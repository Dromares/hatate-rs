//! The worker bridge (`gui/worker_lifecycle.py` at `76c85c4`, and the `QThread` workers it
//! manages).
//!
//! Python runs every long job on a `QThread` and reports back with Qt signals; the registry in
//! `worker_lifecycle.py` exists so that a replaced worker is never destroyed while its thread is
//! still going. Here the app owns **one** tokio runtime ([`Workers`]). A job runs on it, and
//! reports back over a channel that a future on the GTK main context drains, so every callback
//! (`on_event`) runs on the main thread, in order, and nothing the job does can touch a widget.
//!
//! * [`Workers::run`] is blocking work (hashing files, reading a session) on the runtime's
//!   blocking pool; [`Workers::run_async`] is a future (a network call) on its worker threads.
//! * A job gets a [`Ctx`]: `report(progress)` sends a progress value to the main thread, and
//!   `stopped()` is the stop flag the job checks between units of work. A job that does not check
//!   it still stops being *heard*: [`JobHandle::retire`] drops its callbacks (`retire` drops a
//!   `QThread`'s signals), so a stale result never reaches a list that has moved on.
//! * [`Slot`] is one named worker attribute (`self.file_hash_worker`): `replace` retires the old
//!   job (`WorkerRegistry.retire`) and `retire_attrs` retires a group.
//! * [`Workers::shutdown`] is `closeEvent`'s teardown: every job is told to stop and silenced, and
//!   the runtime is given a grace period (`grace_ms=2000`) to wind down. A job that outlives it
//!   is abandoned, not joined: the process is exiting.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gtk4::glib;

/// What a job tells the main thread.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<P, T> {
    /// `report(p)` from the job (a `progress` signal).
    Progress(P),
    /// The job returned (a `finished` / `done` signal). The last event a job sends.
    Done(T),
}

/// What a running job holds: where to report, and whether it has been asked to stop.
pub struct Ctx<P> {
    tx: async_channel::Sender<P>,
    stop: Arc<AtomicBool>,
}

impl<P> Ctx<P> {
    /// Sends `p` to the main thread. Never blocks; a job that has been retired drops it.
    pub fn report(&self, p: P) {
        if !self.stop.load(Ordering::Relaxed) {
            let _ = self.tx.try_send(p);
        }
    }

    /// Whether the job was asked to stop (`worker.stop()`); check between units of work.
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// The flag itself, for an engine that takes one (`hash_files`).
    pub fn stop_flag(&self) -> &Arc<AtomicBool> {
        &self.stop
    }
}

/// The main thread's handle on one job.
#[derive(Clone)]
pub struct JobHandle {
    stop: Arc<AtomicBool>,
    heard: Rc<Cell<bool>>,
    done: Rc<Cell<bool>>,
}

impl JobHandle {
    /// Asks the job to stop; its remaining events are still delivered
    /// (`SearchWorker.stop()` followed by its `finished`).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// `WorkerRegistry.retire`: asks the job to stop and stops listening to it. No callback runs
    /// after this returns.
    pub fn retire(&self) {
        self.stop();
        self.heard.set(false);
    }

    /// The job is running and still being listened to (`isRunning()`).
    pub fn is_running(&self) -> bool {
        self.heard.get() && !self.done.get()
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// One named worker attribute: at most one live job.
#[derive(Default)]
pub struct Slot {
    current: RefCell<Option<JobHandle>>,
}

impl Slot {
    pub fn new() -> Slot {
        Slot::default()
    }

    /// The job in the slot, if it is still running.
    pub fn running(&self) -> Option<JobHandle> {
        self.current.borrow().clone().filter(JobHandle::is_running)
    }

    /// `x_worker.isRunning()`.
    pub fn is_running(&self) -> bool {
        self.running().is_some()
    }

    /// Retires what is in the slot and puts `job` there (`retire(self.x_worker); self.x_worker =
    /// X(...)`).
    pub fn replace(&self, job: JobHandle) {
        self.retire();
        *self.current.borrow_mut() = Some(job);
    }

    /// Retires what is in the slot and empties it (`retire_attrs(..., clear=True)`).
    pub fn retire(&self) {
        if let Some(old) = self.current.borrow_mut().take() {
            old.retire();
        }
    }

    /// Asks the job to stop but keeps listening (`stop_search`).
    pub fn stop(&self) {
        if let Some(job) = self.current.borrow().as_ref() {
            job.stop();
        }
    }
}

/// The app's one runtime and the jobs on it.
pub struct Workers {
    rt: RefCell<Option<tokio::runtime::Runtime>>,
    context: glib::MainContext,
    jobs: RefCell<Vec<JobHandle>>,
}

impl Workers {
    /// A runtime of `threads` worker threads, reporting to `context` (the main context in the app).
    pub fn with_context(threads: usize, context: glib::MainContext) -> Workers {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads.max(1))
            .thread_name("hatate-worker")
            .enable_time()
            .build()
            .expect("the worker runtime starts");
        Workers {
            rt: RefCell::new(Some(rt)),
            context,
            jobs: RefCell::new(Vec::new()),
        }
    }

    /// Two worker threads, reporting to the thread-default main context.
    pub fn new() -> Workers {
        Workers::with_context(2, glib::MainContext::ref_thread_default())
    }

    /// The runtime, for code that needs `block_on` or its handle off the main thread.
    pub fn handle(&self) -> Option<tokio::runtime::Handle> {
        self.rt.borrow().as_ref().map(|rt| rt.handle().clone())
    }

    /// Runs `work` on the blocking pool. `on_event` runs on the main thread.
    pub fn run<P, T, W>(&self, work: W, on_event: impl FnMut(Event<P, T>) + 'static) -> JobHandle
    where
        P: Send + 'static,
        T: Send + 'static,
        W: FnOnce(&Ctx<P>) -> T + Send + 'static,
    {
        let (handle, tx, stop, drain) = self.start::<P, T>(on_event);
        let ctx = Ctx {
            tx: tx.0,
            stop: stop.clone(),
        };
        let done_tx = tx.1;
        if let Some(rt) = self.rt.borrow().as_ref() {
            rt.spawn_blocking(move || {
                let out = work(&ctx);
                drop(ctx);
                let _ = done_tx.try_send(out);
            });
        }
        self.context.spawn_local(drain);
        handle
    }

    /// Runs `work` as a task on the runtime's worker threads.
    pub fn run_async<P, T, F, Fut>(
        &self,
        work: F,
        on_event: impl FnMut(Event<P, T>) + 'static,
    ) -> JobHandle
    where
        P: Send + 'static,
        T: Send + 'static,
        F: FnOnce(Arc<Ctx<P>>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let (handle, tx, stop, drain) = self.start::<P, T>(on_event);
        let ctx = Arc::new(Ctx {
            tx: tx.0,
            stop: stop.clone(),
        });
        let done_tx = tx.1;
        if let Some(rt) = self.rt.borrow().as_ref() {
            rt.spawn(async move {
                let out = work(ctx.clone()).await;
                drop(ctx);
                let _ = done_tx.try_send(out);
            });
        }
        self.context.spawn_local(drain);
        handle
    }

    #[allow(clippy::type_complexity)]
    fn start<P, T>(
        &self,
        on_event: impl FnMut(Event<P, T>) + 'static,
    ) -> (
        JobHandle,
        (async_channel::Sender<P>, async_channel::Sender<T>),
        Arc<AtomicBool>,
        impl Future<Output = ()> + 'static,
    )
    where
        P: Send + 'static,
        T: Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = JobHandle {
            stop: stop.clone(),
            heard: Rc::new(Cell::new(true)),
            done: Rc::new(Cell::new(false)),
        };
        self.jobs.borrow_mut().retain(JobHandle::is_running);
        self.jobs.borrow_mut().push(handle.clone());
        let (ptx, prx) = async_channel::unbounded::<P>();
        let (dtx, drx) = async_channel::bounded::<T>(1);
        let drain = drain_events(prx, drx, on_event, handle.clone());
        (handle, (ptx, dtx), stop, drain)
    }

    /// How many jobs are running and listened to.
    pub fn running(&self) -> usize {
        self.jobs.borrow().iter().filter(|j| j.is_running()).count()
    }

    /// Retires every job (`_stop_background_workers`, `closeEvent`): each is told to stop and
    /// silenced.
    pub fn retire_all(&self) {
        for job in self.jobs.borrow().iter() {
            job.retire();
        }
    }

    /// `closeEvent`'s teardown: retire every job and give the runtime `grace` to wind down.
    /// Whatever is still running after that is abandoned with the process.
    pub fn shutdown(&self, grace: Duration) {
        self.retire_all();
        if let Some(rt) = self.rt.borrow_mut().take() {
            rt.shutdown_timeout(grace);
        }
    }
}

impl Default for Workers {
    fn default() -> Workers {
        Workers::new()
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.borrow_mut().take() {
            rt.shutdown_background();
        }
    }
}

/// The main-thread side of one job: forwards progress, then the result, while the job is heard.
async fn drain_events<P, T>(
    progress: async_channel::Receiver<P>,
    done: async_channel::Receiver<T>,
    mut on_event: impl FnMut(Event<P, T>),
    handle: JobHandle,
) {
    // The job sends progress, then drops its progress sender (when its context goes), then sends
    // the result. So: read progress until the channel closes, then read the result.
    while let Ok(p) = progress.recv().await {
        if !handle.heard.get() {
            handle.done.set(true);
            return;
        }
        on_event(Event::Progress(p));
    }
    let result = done.recv().await;
    handle.done.set(true);
    match result {
        Ok(t) if handle.heard.get() => on_event(Event::Done(t)),
        Ok(_) => {}
        Err(_) => tracing::warn!("a worker ended without a result (it panicked)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Instant;

    /// Runs `ctx`'s loop until `cond` or a timeout.
    fn pump(ctx: &glib::MainContext, mut cond: impl FnMut() -> bool) -> bool {
        let t = Instant::now();
        while !cond() {
            if t.elapsed() > Duration::from_secs(10) {
                return false;
            }
            ctx.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    fn workers() -> (glib::MainContext, Workers) {
        let ctx = glib::MainContext::new();
        let w = Workers::with_context(2, ctx.clone());
        (ctx, w)
    }

    #[test]
    fn progress_then_result_arrive_in_order_on_the_main_thread() {
        let (ctx, w) = workers();
        let main = std::thread::current().id();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        ctx.with_thread_default(|| {
            w.run(
                |c: &Ctx<u32>| {
                    for i in 0..5 {
                        c.report(i);
                    }
                    "finished"
                },
                move |e| {
                    assert_eq!(
                        std::thread::current().id(),
                        main,
                        "callback off the main thread"
                    );
                    s.borrow_mut().push(e);
                },
            );
            assert!(pump(&ctx, || seen.borrow().len() == 6));
        })
        .unwrap();
        let seen = seen.borrow();
        assert_eq!(seen[0], Event::Progress(0));
        assert_eq!(seen[4], Event::Progress(4));
        assert_eq!(seen[5], Event::Done("finished"));
    }

    #[test]
    fn a_retired_job_is_never_heard_again() {
        let (ctx, w) = workers();
        let heard = Rc::new(Cell::new(0));
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let h = heard.clone();
        let g = gate.clone();
        ctx.with_thread_default(|| {
            let job = w.run(
                move |c: &Ctx<u32>| {
                    let _wait = g.lock().unwrap();
                    c.report(1);
                    7
                },
                move |_e: Event<u32, i32>| h.set(h.get() + 1),
            );
            assert!(job.is_running());
            job.retire();
            assert!(!job.is_running());
            drop(held);
            // Let the job finish and the loop run a while: nothing may arrive.
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(300) {
                ctx.iteration(false);
                std::thread::sleep(Duration::from_millis(5));
            }
        })
        .unwrap();
        assert_eq!(heard.get(), 0);
    }

    #[test]
    fn a_stop_is_visible_to_the_job_and_its_result_is_still_delivered() {
        let (ctx, w) = workers();
        let result = Rc::new(RefCell::new(None));
        let r = result.clone();
        ctx.with_thread_default(|| {
            let job = w.run(
                |c: &Ctx<()>| {
                    let mut n = 0;
                    while !c.stopped() {
                        n += 1;
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    n > 0
                },
                move |e: Event<(), bool>| {
                    if let Event::Done(v) = e {
                        *r.borrow_mut() = Some(v);
                    }
                },
            );
            std::thread::sleep(Duration::from_millis(20));
            job.stop();
            assert!(pump(&ctx, || result.borrow().is_some()));
        })
        .unwrap();
        assert_eq!(*result.borrow(), Some(true));
    }

    #[test]
    fn a_slot_holds_one_job_and_replacing_silences_the_old_one() {
        let (ctx, w) = workers();
        let slot = Slot::new();
        let order = Rc::new(RefCell::new(Vec::new()));
        ctx.with_thread_default(|| {
            let o1 = order.clone();
            let first = w.run(
                |c: &Ctx<()>| {
                    while !c.stopped() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    "first"
                },
                move |e: Event<(), &'static str>| o1.borrow_mut().push(format!("{e:?}")),
            );
            slot.replace(first.clone());
            assert!(slot.is_running());
            let o2 = order.clone();
            slot.replace(w.run(
                |_c: &Ctx<()>| "second",
                move |e: Event<(), &'static str>| o2.borrow_mut().push(format!("{e:?}")),
            ));
            assert!(first.is_stopping() && !first.is_running());
            assert!(pump(&ctx, || !order.borrow().is_empty()));
        })
        .unwrap();
        assert_eq!(*order.borrow(), vec![r#"Done("second")"#.to_owned()]);
    }

    #[test]
    fn an_async_job_runs_on_the_runtime_and_a_stalled_one_does_not_block_the_loop() {
        let (ctx, w) = workers();
        let done = Rc::new(Cell::new(false));
        let d = done.clone();
        ctx.with_thread_default(|| {
            let job = w.run_async(
                |_c: Arc<Ctx<()>>| async {
                    // A server that never answers.
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                },
                move |_e: Event<(), ()>| d.set(true),
            );
            // The main loop keeps turning while the job is stalled.
            let ticks = Rc::new(Cell::new(0));
            let t = ticks.clone();
            ctx.spawn_local(async move {
                for _ in 0..5 {
                    glib::timeout_future(Duration::from_millis(5)).await;
                    t.set(t.get() + 1);
                }
            });
            assert!(pump(&ctx, || ticks.get() == 5));
            assert!(job.is_running() && !done.get());
        })
        .unwrap();
        // Dropping `w` abandons the stalled task rather than waiting an hour.
        let t = Instant::now();
        w.shutdown(Duration::from_millis(100));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn shutdown_retires_every_job() {
        let (ctx, w) = workers();
        ctx.with_thread_default(|| {
            let job = w.run(
                |c: &Ctx<()>| {
                    while !c.stopped() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                },
                |_e: Event<(), ()>| panic!("a retired job was heard"),
            );
            assert_eq!(w.running(), 1);
            w.shutdown(Duration::from_secs(2));
            assert!(!job.is_running());
            assert_eq!(w.running(), 0);
        })
        .unwrap();
    }
}
