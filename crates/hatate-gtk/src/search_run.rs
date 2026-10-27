//! Running a search from the window (`gui/main_window.py` at `76c85c4`: `action_toggle_search`,
//! `action_start_search`, `action_stop_search`, `_launch_search_worker`, `_on_worker_progress`,
//! `_on_worker_waiting`, `_on_wait_countdown`, `_on_worker_image_updated`, `_on_auto_imported`,
//! `_on_worker_finished`, the SauceNAO quota readout and pause handlers, and the engine-alert
//! forwarder).
//!
//! The run is the merged `hatate_core::batch::search_pending_with` over **copies** of the
//! entries, on the worker bridge's runtime. Python's worker mutates the window's own entry
//! objects and says so with Qt signals; the copies tell the window the same thing through a
//! [`Reporter`] that posts [`Msg`]s on the window's [`Bus`]. The bus drains on the GTK main
//! context, in order, so every handler below runs on the main thread, and a handler that finds the
//! run id it was posted under is no longer the current one drops the message (a retired run, a
//! window that is closing).
//!
//! The rows are the queue model's: a message for a row replaces the model's entry with the worker's
//! copy and raises `RowChanged` (6.d's view redraws from that). The model's `search_running` flag
//! is the one `isRunning()` reads, so the MCP re-search and a Start Search cannot both run.

use crate::dialogs::{self, Buttons, Kind};
use crate::shell::{QUOTA_PAUSED_ETA, QUOTA_PAUSED_NOTE};
use crate::text::{describe_quota_pause, round_half_even};
use crate::window::MainWindow;
use async_channel::{Receiver, Sender};
use gtk4::gio;
use gtk4::prelude::*;
use hatate_core::batch::{
    Autosave, BatchEnv, Ending, PendingOptions, Reporter, SessionSink, search_pending_with,
};
use hatate_core::clock::{Clock, SystemClock};
use hatate_core::engine_alerts::{EngineAlert, EngineAlerts, Listener};
use hatate_core::engines::lens_search::LensConnection;
use hatate_core::engines::saucenao::{SauceNaoQuota, describe_quota, describe_short_window};
use hatate_core::eta::{RunEstimate, format_finish_time};
use hatate_core::hydrus_import::ImportResult;
use hatate_core::localtime::local_wall_clock;
use hatate_core::mcp::tool_handlers::HydrusClientFactory;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::queue::{EntryId, RemoveReason};
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::run_state::{RunChannel, RunEnd, RunEvent, RunSink, RunState};
use hatate_core::search_engine::SearchEngine;
use hatate_core::stop_record::StopRecordFile;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// `random.uniform`'s source: xorshift64*, seeded from the clock and the pid (the same generator
/// `hatate batch` uses; pacing jitter needs no better).
fn unit_random() -> Arc<dyn Fn() -> f64 + Send + Sync> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
        ^ (u64::from(std::process::id()) << 32);
    let state = AtomicU64::new(seed | 1);
    Arc::new(move || {
        let mut x = state.load(Ordering::Relaxed);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.store(x, Ordering::Relaxed);
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// Everything a run is built from: the search engine and the things the loop asks of the engines.
/// The shipped window builds it from the real net layer on the first Start Search
/// ([`SearchBackend::live`]); a test gives its own, over scripted engines and a manual clock.
#[derive(Clone)]
pub struct SearchBackend {
    pub engine: Arc<SearchEngine>,
    /// `saucenao.is_daily_quota_exhausted`.
    pub quota_exhausted: Arc<dyn Fn() -> bool + Send + Sync>,
    /// `saucenao.get_last_quota`.
    pub last_quota: Arc<dyn Fn() -> Option<SauceNaoQuota> + Send + Sync>,
    /// What `_launch_search_worker` does first: forget the remembered quota exhaustion and every
    /// engine that stood down on an earlier run.
    pub reset_run_state: Arc<dyn Fn() + Send + Sync>,
    pub hydrus_client: HydrusClientFactory,
    /// `core.engine_alerts`: the channel the engines raise on.
    pub alerts: Arc<EngineAlerts>,
    /// A number in `[0, 1)`.
    pub unit_random: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Takes the engines down (`LiveEngines::shutdown`) when the window closes.
    pub shutdown: Arc<dyn Fn() + Send + Sync>,
}

impl SearchBackend {
    /// The shipped engines on the shipped net layer (rustls with the explicit provider, the real
    /// hosts): what `hatate batch` builds. There is no flag or variable that points an engine at
    /// another host. `lens` receives the Google Lens connection changes.
    pub fn live(
        paths: &Paths,
        lens: hatate_core::engines::lens_search::ConnectionSink,
    ) -> Result<SearchBackend, String> {
        use hatate_core::engine_backends::LiveEngines;
        use hatate_core::engine_runner::EngineRunner;
        use hatate_core::net::{NetClient, NetConfig, install_crypto_provider};
        install_crypto_provider();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
        let net = NetClient::new(NetConfig::default())
            .map_err(|e| format!("could not build the network client: {e}"))?;
        let mut live = LiveEngines::for_profile(net.clone(), clock.clone(), paths);
        live.lens = live.lens.clone().with_connection_sink(lens);
        let live = Arc::new(live);
        let runner = EngineRunner::new(live.clone(), clock.clone());
        let engine = Arc::new(SearchEngine::new(net, runner, paths.clone(), clock));
        let (a, b, c, d) = (live.clone(), live.clone(), live.clone(), live.clone());
        Ok(SearchBackend {
            engine,
            quota_exhausted: Arc::new(move || a.saucenao.is_daily_quota_exhausted()),
            last_quota: Arc::new(move || b.saucenao.get_last_quota()),
            reset_run_state: Arc::new(move || c.reset_run_state()),
            hydrus_client: Arc::new(|s| hatate_core::hydrus_client::HydrusClient::new(s)),
            alerts: d.lens.alerts().clone(),
            unit_random: unit_random(),
            shutdown: Arc::new(move || live.shutdown()),
        })
    }
}

/// What `_launch_search_worker` is given beyond the entries.
#[derive(Clone, Default)]
pub struct LaunchOptions {
    /// `status_prefix`: "Searching" or "Re-searching".
    pub status_prefix: String,
    /// `override_engines`: the one engine to use for a row.
    pub override_engines: HashMap<EntryId, String>,
    pub bypass_cache: bool,
    pub force_continue_without_saucenao: bool,
}

impl LaunchOptions {
    pub fn searching() -> LaunchOptions {
        LaunchOptions {
            status_prefix: "Searching".to_owned(),
            ..LaunchOptions::default()
        }
    }
}

/// What the worker tells the window, one per Python signal.
#[derive(Debug)]
pub enum Msg {
    /// `image_updated` with the entry now `Searching`.
    Searching { entry: Entry },
    /// `image_updated` after the search; `index` is the entry's place in the batch.
    Updated { index: usize, entry: Entry },
    /// `progress(done, total)`.
    Progress { done: usize, total: usize },
    /// `wait_countdown(label, remaining, total)`; `("", 0, 0)` clears it.
    Countdown {
        label: String,
        remaining: f64,
        total: f64,
    },
    /// `auto_imported(entry, result)`.
    AutoImported { entry: Entry, result: ImportResult },
    /// `continuing_without_saucenao(searched, remaining)`.
    Continuing { searched: i64, remaining: i64 },
    /// `paused_out_of_quota(searched, remaining)`.
    Paused { searched: i64, remaining: i64 },
    /// How the run ended, as the channel read it (`Failed` carries the path-free message).
    Ended(RunEnd),
    /// `finished_all`, with the entries the run dropped (`remove_after_import`).
    Finished { ending: Ending },
}

/// What reaches the main thread.
#[derive(Debug)]
pub enum BusEvent {
    Run(u64, Msg),
    Lens(LensConnection),
    Alert(EngineAlert),
}

/// The window's inbox. Cloneable, and `Send` so a worker thread, an engine's alert listener and
/// the Lens client can all post to it.
#[derive(Clone)]
pub struct Bus {
    tx: Sender<BusEvent>,
}

impl Bus {
    fn post(&self, event: BusEvent) {
        let _ = self.tx.try_send(event);
    }
}

/// The search run's state on the window.
pub struct SearchRun {
    /// Given by the environment (a test), or built on the first Start Search.
    backend: RefCell<Option<SearchBackend>>,
    pub bus: Bus,
    inbox: RefCell<Option<Receiver<BusEvent>>>,
    /// The run whose messages are heard; 0 when none is.
    run_id: Cell<u64>,
    next_run: Cell<u64>,
    /// `worker.stop()`: raised by Stop Search, read by the loop between images and in its waits.
    stop: RefCell<Arc<AtomicBool>>,
    /// `self.worker` as the slot the bridge tracks.
    pub job: crate::worker::Slot,
    /// The batch's row ids, in the order the worker was given the entries.
    ids: RefCell<Vec<EntryId>>,
    /// The batch's rows by path, for the signals that name an entry and not its place.
    by_path: RefCell<HashMap<String, EntryId>>,
    /// `self._run_estimate`.
    pub estimate: RefCell<RunEstimate>,
    /// The listener subscribed for the length of the run (`_engine_alert_listener`).
    listener: RefCell<Option<Listener>>,
    /// The seconds-remaining the strip last had (`_run_estimate.seconds_remaining()` when
    /// `_run_active`), or `None`.
    pub eta_seconds: Cell<Option<f64>>,
    /// The last Lens connection state heard.
    pub lens: Cell<Option<LensConnection>>,
    monotonic: Rc<dyn Fn() -> f64>,
}

impl SearchRun {
    pub fn new(backend: Option<SearchBackend>, monotonic: Rc<dyn Fn() -> f64>) -> Rc<SearchRun> {
        let (tx, rx) = async_channel::unbounded();
        Rc::new(SearchRun {
            backend: RefCell::new(backend),
            bus: Bus { tx },
            inbox: RefCell::new(Some(rx)),
            run_id: Cell::new(0),
            next_run: Cell::new(1),
            stop: RefCell::new(Arc::new(AtomicBool::new(false))),
            job: crate::worker::Slot::new(),
            ids: RefCell::new(Vec::new()),
            by_path: RefCell::new(HashMap::new()),
            estimate: RefCell::new(RunEstimate::new()),
            listener: RefCell::new(None),
            eta_seconds: Cell::new(None),
            lens: Cell::new(None),
            monotonic,
        })
    }

    /// The engine alerts of the backend, once there is one.
    pub fn alerts(&self) -> Option<Arc<EngineAlerts>> {
        self.backend.borrow().as_ref().map(|b| b.alerts.clone())
    }

    /// Whether a run's messages are still being heard.
    pub fn listening(&self) -> bool {
        self.run_id.get() != 0
    }

    /// The backend, built on first use.
    fn backend(&self, paths: &Paths) -> Result<SearchBackend, String> {
        if let Some(b) = self.backend.borrow().as_ref() {
            return Ok(b.clone());
        }
        let bus = self.bus.clone();
        let lens: hatate_core::engines::lens_search::ConnectionSink =
            Arc::new(move |c| bus.post(BusEvent::Lens(c)));
        let built = SearchBackend::live(paths, lens)?;
        *self.backend.borrow_mut() = Some(built.clone());
        Ok(built)
    }

    /// Raises the stop flag of the run under way (`worker.stop()`).
    pub fn request_stop(&self) {
        self.stop.borrow().store(true, Ordering::SeqCst);
    }

    /// Takes the engines down and stops listening (`closeEvent`).
    pub fn close(&self) {
        self.run_id.set(0);
        self.request_stop();
        self.job.retire();
        self.drop_listener();
        if let Some(b) = self.backend.borrow().as_ref() {
            (b.shutdown)();
        }
    }

    fn drop_listener(&self) {
        if let Some(listener) = self.listener.borrow_mut().take() {
            if let Some(alerts) = self.alerts() {
                alerts.unsubscribe(&listener);
            }
        }
    }
}

/// The reporter the loop is given: the channel for the run's endings, then this for the signals
/// Python's worker emits.
struct WindowNext {
    bus: Bus,
    run: u64,
}

impl WindowNext {
    fn post(&self, msg: Msg) {
        self.bus.post(BusEvent::Run(self.run, msg));
    }
}

impl Reporter for WindowNext {
    fn line(&self, _text: &str) {}

    fn countdown(&self, label: &str, remaining: f64, total: f64) {
        self.post(Msg::Countdown {
            label: label.to_owned(),
            remaining,
            total,
        });
    }

    fn auto_imported(&self, entry: &Entry, result: &ImportResult) {
        self.post(Msg::AutoImported {
            entry: entry.clone(),
            result: result.clone(),
        });
    }

    fn searching(&self, entry: &Entry, _done: usize, _total: usize) {
        self.post(Msg::Searching {
            entry: entry.clone(),
        });
    }

    fn updated(&self, index: usize, entry: &Entry) {
        self.post(Msg::Updated {
            index,
            entry: entry.clone(),
        });
    }

    fn progress(&self, done: usize, total: usize) {
        self.post(Msg::Progress { done, total });
    }

    fn continuing_without_saucenao(&self, searched: i64, remaining: i64) {
        self.post(Msg::Continuing {
            searched,
            remaining,
        });
    }

    fn paused_for_quota(&self, searched: i64, remaining: i64) {
        self.post(Msg::Paused {
            searched,
            remaining,
        });
    }

    fn finished(&self, ending: &Ending) {
        self.post(Msg::Finished {
            ending: ending.clone(),
        });
    }
}

/// The session is written by the window's own autosave, not by the run.
struct NoSave;

impl SessionSink for NoSave {
    fn save(&self, _entries: &[Entry], _now: f64) -> Result<(), String> {
        Ok(())
    }
}

/// `"%.0f"` of a number of seconds, as Python rounds it.
fn whole(seconds: f64) -> i64 {
    round_half_even(seconds)
}

/// `_format_finish_time(seconds)` against the local clock now.
fn finish_time(now: f64, seconds: f64) -> String {
    let w = local_wall_clock(now);
    let naive = w.day_number() * 86_400
        + i64::from(w.hour) * 3600
        + i64::from(w.minute) * 60
        + i64::from(w.second);
    format_finish_time(seconds, naive)
}

impl MainWindow {
    /// Starts the bus drain on the main context. Called once, when the window is built.
    pub fn start_search_inbox(&self) {
        let Some(rx) = self.s.search.inbox.borrow_mut().take() else {
            return;
        };
        let weak = self.downgrade();
        gtk4::glib::spawn_future_local(async move {
            while let Ok(event) = rx.recv().await {
                let Some(w) = weak.upgrade() else { break };
                w.on_bus_event(event);
            }
        });
    }

    fn on_bus_event(&self, event: BusEvent) {
        if self.s.closing.get() {
            return;
        }
        match event {
            BusEvent::Run(run, msg) => {
                if run == self.s.search.run_id.get() {
                    self.on_run_message(msg);
                }
            }
            BusEvent::Lens(c) => self.on_lens_connection(c),
            BusEvent::Alert(a) => self.on_engine_alert(&a),
        }
    }

    fn on_run_message(&self, msg: Msg) {
        match msg {
            Msg::Searching { entry } => self.on_worker_image_updated(None, entry),
            Msg::Updated { index, entry } => self.on_worker_image_updated(Some(index), entry),
            Msg::Progress { done, total } => self.on_worker_progress(done, total),
            Msg::Countdown {
                label,
                remaining,
                total,
            } => {
                if label == "Rate limit" {
                    self.on_worker_waiting(remaining);
                }
                self.on_wait_countdown(&label, remaining, total);
            }
            Msg::AutoImported { entry, result } => self.on_auto_imported(entry, &result),
            Msg::Continuing {
                searched,
                remaining,
            } => self.on_continuing_without_saucenao(searched, remaining),
            Msg::Paused {
                searched,
                remaining,
            } => self.on_paused_out_of_quota(searched, remaining),
            Msg::Ended(end) => self.on_run_ended(&end),
            Msg::Finished { ending } => self.on_worker_finished(&ending),
        }
    }

    // ---- the Sites menu -------------------------------------------------------------------

    /// `_build_sites_menu`'s actions: `site_actions[site]`, checked when `site` is in
    /// `settings.enabled_sites`.
    pub(crate) fn install_site_actions(&self) {
        let enabled = self.s.settings.lock().unwrap().enabled_sites.clone();
        for (i, site) in crate::menu::all_site_options().into_iter().enumerate() {
            let action = gio::SimpleAction::new_stateful(
                &crate::menu::site_action(i),
                None,
                &enabled.contains(&site).to_variant(),
            );
            let w = self.downgrade();
            action.connect_activate(move |a, _| {
                let checked = !a.state().and_then(|v| v.get::<bool>()).unwrap_or(false);
                a.set_state(&checked.to_variant());
                if let Some(w) = w.upgrade() {
                    w.on_site_toggled(&site, checked);
                }
            });
            self.win.add_action(&action);
        }
    }

    /// `_on_site_toggled(site, checked)`.
    pub fn on_site_toggled(&self, site: &str, checked: bool) {
        let all = crate::menu::all_site_options();
        let mut s = self.s.settings.lock().unwrap();
        let mut enabled: std::collections::HashSet<String> =
            s.enabled_sites.iter().cloned().collect();
        if checked {
            enabled.insert(site.to_owned());
        } else {
            enabled.remove(site);
        }
        s.enabled_sites = all.into_iter().filter(|x| enabled.contains(x)).collect();
        tracing::debug!(
            "Site filter: {site} -> {checked} (now enabled: {:?})",
            s.enabled_sites
        );
        if let Err(e) = s.save(&self.s.paths) {
            tracing::warn!("could not save the settings: {e}");
        }
    }

    /// `_set_all_sites(checked)`: every check follows, without one handler call per site.
    pub fn set_all_sites(&self, checked: bool) {
        let all = crate::menu::all_site_options();
        for i in 0..all.len() {
            if let Some(a) = self
                .win
                .lookup_action(&crate::menu::site_action(i))
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_state(&checked.to_variant());
            }
        }
        let mut s = self.s.settings.lock().unwrap();
        s.enabled_sites = if checked { all } else { Vec::new() };
        tracing::info!(
            "Site filter: {} all sites",
            if checked { "enabled" } else { "disabled" }
        );
        if let Err(e) = s.save(&self.s.paths) {
            tracing::warn!("could not save the settings: {e}");
        }
    }

    // ---- start, stop ----------------------------------------------------------------------

    /// `action_toggle_search`.
    pub fn action_toggle_search(&self) {
        if self.search_running() {
            self.action_stop_search();
        } else {
            self.action_start_search();
        }
    }

    /// `action_start_search`.
    pub fn action_start_search(&self) {
        if self.search_running() {
            dialogs::information(&*self.s.prompter, "Search", "A search is already running.");
            return;
        }
        let pending: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            (0..q.len() as i64)
                .filter_map(|r| q.id_at(r))
                .filter(|id| {
                    q.entry(*id)
                        .is_some_and(|e| e.status == MatchStatus::NotSearched)
                })
                .collect()
        };
        if pending.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Search",
                "No unsearched images in the list.",
            );
            return;
        }
        tracing::info!(
            "Start Search clicked: {} unsearched image(s)",
            pending.len()
        );
        self.launch_search_worker(pending, LaunchOptions::searching());
    }

    /// `action_stop_search`.
    pub fn action_stop_search(&self) {
        if !self.search_running() {
            return;
        }
        tracing::info!("Stop Search clicked");
        self.s.search.request_stop();
        self.say("Stopping after current image\u{2026}");
        // Re-enabled once the worker actually finishes.
        self.s.shell.search_toggle_btn.set_sensitive(false);
    }

    /// `_launch_search_worker(entries, status_prefix, override_engines, bypass_cache,
    /// force_continue_without_saucenao)`.
    pub fn launch_search_worker(&self, ids: Vec<EntryId>, opts: LaunchOptions) {
        let backend = match self.s.search.backend(&self.s.paths) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("Could not start the search engines: {e}");
                dialogs::critical(
                    &*self.s.prompter,
                    "Search",
                    "The search could not start. See Help > View Logs.",
                );
                return;
            }
        };
        // Starting a search is the user saying "try again", so forget any remembered quota
        // exhaustion (and every engine that stood down). If the allowance really hasn't reset,
        // SauceNAO's first response sets the flag again and the batch pauses after one image.
        (backend.reset_run_state)();
        // A run is starting, so "stopped 2h 14m ago" / "last run failed" describes the run before
        // this one rather than the strip's own.
        self.s.shell.run_note_label.set_text("");
        // Pressing Start Search is already the user acting on a persisted pause banner.
        if let Err(e) = QuotaPauseFile::new(&self.s.paths).clear() {
            tracing::warn!("Could not clear the SauceNAO quota pause: {e}");
        }
        self.hide_quota_banner();

        // Copies of the rows, in the order asked; a row named twice is searched twice.
        let (batch, pending, run_ids): (Vec<Entry>, Vec<usize>, Vec<EntryId>) = {
            let q = self.s.queue.borrow();
            let mut entries: Vec<Entry> = Vec::new();
            let mut run_ids: Vec<EntryId> = Vec::new();
            let mut pending: Vec<usize> = Vec::new();
            for id in &ids {
                let Some(e) = q.entry(*id) else { continue };
                match run_ids.iter().position(|r| r == id) {
                    Some(p) => pending.push(p),
                    None => {
                        pending.push(entries.len());
                        entries.push(e.clone());
                        run_ids.push(*id);
                    }
                }
            }
            (entries, pending, run_ids)
        };
        if pending.is_empty() {
            return;
        }
        let total = pending.len();
        let by_path: HashMap<String, EntryId> = batch
            .iter()
            .zip(run_ids.iter())
            .map(|(e, id)| (e.path.clone(), *id))
            .collect();
        let overrides: HashMap<usize, String> = run_ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| opts.override_engines.get(id).map(|e| (i, e.clone())))
            .collect();

        let run = self.s.search.next_run.get();
        self.s.search.next_run.set(run + 1);
        self.s.search.run_id.set(run);
        *self.s.search.ids.borrow_mut() = run_ids;
        *self.s.search.by_path.borrow_mut() = by_path;
        let stop = Arc::new(AtomicBool::new(false));
        *self.s.search.stop.borrow_mut() = stop.clone();

        // Open the engine-alert channel for the length of the run. Engines raise these from the
        // worker's own threads.
        self.s.search.drop_listener();
        let bus = self.s.search.bus.clone();
        let listener: Listener =
            Arc::new(move |a: &EngineAlert| bus.post(BusEvent::Alert(a.clone())));
        backend.alerts.subscribe(listener.clone());
        *self.s.search.listener.borrow_mut() = Some(listener);

        self.set_progress_gauge(0, total);
        self.s.search.estimate.borrow_mut().reset();
        self.s.search.eta_seconds.set(None);
        self.s.run_active.set(true);
        self.s.queue.borrow_mut().set_search_running(true);
        self.refresh_run_progress_label();
        self.s
            .shell
            .search_toggle_btn
            .set_label("\u{25A0} Stop Search");
        self.say(&format!(
            "{} {} image(s)\u{2026}",
            opts.status_prefix, total
        ));

        let settings = self.s.settings.clone();
        let paths = self.s.paths.clone();
        let bus = self.s.search.bus.clone();
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let pending_opts = PendingOptions {
            override_engines: overrides,
            bypass_cache: opts.bypass_cache,
            force_continue_without_saucenao: opts.force_continue_without_saucenao,
        };
        let job = self.s.workers.run(
            move |_ctx: &crate::worker::Ctx<()>| {
                run_search(
                    &backend,
                    &settings,
                    &paths,
                    stop,
                    bus,
                    run,
                    batch,
                    pending,
                    pending_opts,
                    handle,
                );
            },
            |_| {},
        );
        self.s.search.job.replace(job);
    }

    /// The gauge, in Qt's terms: `progress_bar.setMaximum(total); setValue(done)`.
    fn set_progress_gauge(&self, done: usize, total: usize) {
        self.s.run_progress.set((done as i64, total as i64));
        let fraction = if total == 0 {
            0.0
        } else {
            done as f64 / total as f64
        };
        self.s
            .shell
            .progress_bar
            .set_fraction(fraction.clamp(0.0, 1.0));
    }

    // ---- signals ---------------------------------------------------------------------------

    /// `_on_worker_progress(done, total)`.
    fn on_worker_progress(&self, done: usize, total: usize) {
        self.set_progress_gauge(done, total);
        let now = (self.s.search.monotonic)();
        {
            let mut est = self.s.search.estimate.borrow_mut();
            est.record(done as i64, total as i64, Some(now));
        }
        self.refresh_run_progress_label();
    }

    /// `_on_worker_waiting(seconds)`.
    fn on_worker_waiting(&self, seconds: f64) {
        self.say(&format!(
            "Waiting {}s before next search (rate limiting)\u{2026}",
            whole(seconds)
        ));
    }

    /// `_on_wait_countdown(label, remaining, total)`.
    fn on_wait_countdown(&self, label: &str, remaining: f64, _total: f64) {
        let readout = &self.s.shell.wait_countdown_label;
        if label.is_empty() || remaining <= 0.0 {
            readout.set_text("");
            return;
        }
        // The mockup's vocabulary: "waiting 38s", not "Rate limit: 38s".
        let what = if label == "Rate limit" {
            "waiting".to_owned()
        } else {
            label.to_lowercase()
        };
        readout.set_text(&format!("{what} {}s", whole(remaining)));
    }

    /// `_on_worker_image_updated(entry)`: the row follows the worker's copy.
    fn on_worker_image_updated(&self, index: Option<usize>, entry: Entry) {
        let id = match index {
            Some(i) => self.s.search.ids.borrow().get(i).copied(),
            None => self.s.search.by_path.borrow().get(&entry.path).copied(),
        };
        let searching = entry.status == MatchStatus::Searching;
        let name = entry.filename().to_owned();
        if let Some(id) = id {
            self.replace_row(id, entry);
        }
        if searching {
            self.say(&format!("Searching {name}\u{2026}"));
        } else {
            self.refresh_saucenao_quota_label();
        }
        // DEFERRED(6.d): select the row and refresh the preview and the tag list.
    }

    /// Puts the worker's copy in the model and raises the row's change.
    fn replace_row(&self, id: EntryId, entry: Entry) {
        let mut q = self.s.queue.borrow_mut();
        if let Some(slot) = q.entry_mut(id) {
            *slot = entry;
            q.refresh_row(id);
        }
    }

    /// `_on_auto_imported(entry, result)`: a failed or unconfirmed import only redraws the row; a
    /// success drops it when `remove_after_import` says so.
    fn on_auto_imported(&self, entry: Entry, result: &ImportResult) {
        let id = self.s.search.by_path.borrow().get(&entry.path).copied();
        let Some(id) = id else { return };
        let removes = {
            let s = self.s.settings.lock().unwrap();
            hatate_core::batch::removes_row(&s, result)
        };
        self.replace_row(id, entry);
        if removes {
            self.s
                .queue
                .borrow_mut()
                .remove(&[id], RemoveReason::After("auto-imported"));
            self.list_changed();
        }
    }

    /// `_on_continuing_without_saucenao(searched, remaining)`: said once, in the status bar.
    fn on_continuing_without_saucenao(&self, _searched: i64, remaining: i64) {
        self.say(&format!(
            "SauceNAO's daily quota is spent - carrying on without it for the remaining \
             {remaining} image(s). Those results are marked Provisional and are not cached; \
             re-search them once the allowance resets."
        ));
    }

    /// `_on_paused_out_of_quota(searched, remaining)`: no modal (ruling C-4); the banner carries
    /// what the dialog said and both its actions.
    fn on_paused_out_of_quota(&self, searched: i64, remaining: i64) {
        let now = (self.s.clock)();
        let state = QuotaPauseFile::new(&self.s.paths).read();
        let limit = self
            .last_quota()
            .and_then(|q| q.long_limit)
            .filter(|l| *l != 0);
        let used = limit.map_or(String::new(), |l| format!(" ({l}/day used)"));
        match &state {
            Some(s) => self.say(&describe_quota_pause(s, now)),
            None => self.say(&format!(
                "Paused: SauceNAO daily quota exhausted{used} - {searched} searched, \
                 {remaining} left unsearched"
            )),
        }
        let reset_at = state.as_ref().and_then(|s| s.reset_epoch());
        let paused_at = state.as_ref().map_or(now, |s| s.paused_at);
        self.show_quota_banner(paused_at, remaining, reset_at);
    }

    fn last_quota(&self) -> Option<SauceNaoQuota> {
        self.s
            .search
            .backend
            .borrow()
            .as_ref()
            .and_then(|b| (b.last_quota)())
    }

    /// `_refresh_saucenao_quota_label()` / `_render_saucenao_quota_label()`.
    pub fn refresh_saucenao_quota_label(&self) {
        let quota = self.last_quota();
        let text = describe_quota(quota.as_ref()).replacen("SauceNAO: ", "SauceNAO ", 1);
        let tip = describe_short_window(quota.as_ref());
        let readout = &self.s.shell.saucenao_quota_label;
        readout.set_text(&text);
        readout.set_tooltip(&tip);
    }

    /// `_on_worker_finished()`.
    fn on_worker_finished(&self, ending: &Ending) {
        self.s.search.drop_listener();
        self.s.run_active.set(false);
        self.s.search.eta_seconds.set(None);
        {
            let mut q = self.s.queue.borrow_mut();
            q.set_search_running(false);
        }
        self.refresh_run_progress_label();
        self.say("Search finished");
        self.s
            .shell
            .search_toggle_btn
            .set_label("\u{25B6} Start Search");
        self.s.shell.search_toggle_btn.set_sensitive(true);
        self.s.search.run_id.set(0);
        if let Ending::Failed(fault) = ending {
            // Shown as a failed run, not a crash: the message is the fault's own, without a path.
            tracing::warn!("The search run failed: {}", fault.detail());
            self.on_run_ended(&RunEnd::Failed(fault.user_message()));
        }
    }

    /// The channel's reading of how the run ended. `DEFERRED(A2)`: the Failed banner.
    fn on_run_ended(&self, end: &RunEnd) {
        if let RunEnd::Failed(message) = end {
            self.s
                .shell
                .run_note_label
                .set_text("last run failed \u{2014} see log");
            self.say(message);
        }
    }

    // ---- re-search (6.d's context menu calls these) ------------------------------------------

    /// `_live_entries(entries)`: the rows that are still in the list.
    fn live_rows(&self, ids: &[EntryId]) -> Vec<EntryId> {
        let q = self.s.queue.borrow();
        ids.iter()
            .copied()
            .filter(|id| q.position_of(*id).is_some())
            .collect()
    }

    /// `_research_rows(entries)`: re-runs the search for the rows whatever their status, with the
    /// search cache bypassed (the point is a fresh look).
    pub fn research_rows(&self, ids: &[EntryId]) {
        if self.search_running() {
            dialogs::information(&*self.s.prompter, "Search", "A search is already running.");
            return;
        }
        let rows = self.live_rows(ids);
        if rows.is_empty() {
            return;
        }
        {
            let q = self.s.queue.borrow();
            let names: Vec<&str> = rows
                .iter()
                .filter_map(|id| q.entry(*id).map(|e| e.filename()))
                .collect();
            tracing::info!("Re-search requested for {} image(s): {names:?}", rows.len());
        }
        self.launch_search_worker(
            rows,
            LaunchOptions {
                status_prefix: "Re-searching".to_owned(),
                bypass_cache: true,
                ..LaunchOptions::default()
            },
        );
    }

    /// `_research_rows_with_engine(entries, engine)`: re-searches the rows with ONE named engine,
    /// whatever their state.
    pub fn research_rows_with_engine(&self, ids: &[EntryId], engine: &str) {
        if self.search_running() {
            dialogs::information(&*self.s.prompter, "Search", "A search is already running.");
            return;
        }
        let rows = self.live_rows(ids);
        if rows.is_empty() {
            return;
        }
        let label = hatate_core::engine_ids::label_for(engine).to_owned();
        tracing::info!("{label}-only search requested for {} image(s)", rows.len());
        let overrides = rows.iter().map(|id| (*id, engine.to_owned())).collect();
        self.launch_search_worker(
            rows,
            LaunchOptions {
                status_prefix: format!("Searching ({label} only)"),
                override_engines: overrides,
                bypass_cache: true,
                ..LaunchOptions::default()
            },
        );
    }

    /// `_research_rows_with_opposite_engine(entries)`: each row with whichever engine did NOT find
    /// its current match, ignoring the configured order. A row with no match gets the opposite of
    /// the configured primary.
    pub fn research_rows_with_opposite_engine(&self, ids: &[EntryId]) {
        use hatate_core::engine_ids::{effective_primary, opposite_engine};
        if self.search_running() {
            dialogs::information(&*self.s.prompter, "Search", "A search is already running.");
            return;
        }
        let primary = self.s.settings.lock().unwrap().primary_engine.clone();
        let primary = effective_primary(&primary).to_owned();
        let fallback_for_unsearched = opposite_engine(None, &primary);
        let rows = self.live_rows(ids);
        let mut overrides: HashMap<EntryId, String> = HashMap::new();
        let mut unsearched = 0;
        {
            let q = self.s.queue.borrow();
            for id in &rows {
                let current = q
                    .entry(*id)
                    .and_then(|e| e.selected_candidate())
                    .map(|c| c.engine.clone());
                if !current.as_deref().is_some_and(|c| !c.trim().is_empty()) {
                    unsearched += 1;
                }
                overrides.insert(
                    *id,
                    opposite_engine(current.as_deref(), &primary).to_owned(),
                );
            }
        }
        if rows.is_empty() {
            return;
        }
        if unsearched > 0 {
            tracing::info!(
                "Opposite-engine search: {unsearched} row(s) had no match to flip, using \
                 {fallback_for_unsearched:?} (the opposite of the configured primary {primary:?})"
            );
        }
        self.launch_search_worker(
            rows,
            LaunchOptions {
                status_prefix: "Searching (opposite engine)".to_owned(),
                override_engines: overrides,
                bypass_cache: true,
                ..LaunchOptions::default()
            },
        );
    }

    // ---- quota banner actions ---------------------------------------------------------------

    /// `_resume_without_saucenao()`: "Continue with other engines" resumes on whatever is still
    /// unsearched, skipping SauceNAO for just this run. The saved setting is left as it is.
    pub fn resume_without_saucenao(&self) {
        if self.search_running() {
            return;
        }
        let pending: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            (0..q.len() as i64)
                .filter_map(|r| q.id_at(r))
                .filter(|id| {
                    q.entry(*id)
                        .is_some_and(|e| e.status == MatchStatus::NotSearched)
                })
                .collect()
        };
        if pending.is_empty() {
            return;
        }
        tracing::info!(
            "Continuing without SauceNAO: {} unsearched image(s)",
            pending.len()
        );
        self.launch_search_worker(
            pending,
            LaunchOptions {
                force_continue_without_saucenao: true,
                ..LaunchOptions::searching()
            },
        );
    }

    // ---- engine alerts, Lens ---------------------------------------------------------------

    /// `_on_engine_alert(alert)`: an engine cannot run at all - say so where it will be seen. The
    /// status bar is written too, because the dialog is dismissed and the run is not.
    fn on_engine_alert(&self, alert: &EngineAlert) {
        self.say(&format!("{} was skipped - {}", alert.engine, alert.title));
        self.s.prompter.message(
            Kind::Warning,
            &alert.title,
            &alert.text(),
            Buttons::Ok,
            Box::new(|_| {}),
        );
    }

    /// Renders `LensConnection` (new in the Rust window; Python shows nothing of it): the strip's
    /// status line says what Lens is doing, and goes quiet again once it answers.
    fn on_lens_connection(&self, c: LensConnection) {
        self.s.search.lens.set(Some(c));
        let text = match c {
            LensConnection::NotReachable => {
                "Google Lens: the browser could not be reached - see Help > View Logs"
            }
            LensConnection::WaitingForLens => "Google Lens: waiting for an answer\u{2026}",
            LensConnection::Blocked => "Google Lens: standing down for this run",
            LensConnection::Ready => return,
            _ => return,
        };
        self.say(text);
    }

    /// `closeEvent`'s share: take the listener off and stop listening.
    pub fn close_search_run(&self) {
        self.s.search.close();
    }

    /// `_refresh_run_progress_label()` is in `session_ops`; this is the live half it reads.
    pub fn live_eta_text(&self) -> Option<String> {
        if !self.s.run_active.get() {
            return None;
        }
        let remaining = self.s.search.estimate.borrow().seconds_remaining();
        if remaining < 0.0 {
            return None;
        }
        let mut eta = format!("ETA ~{}", crate::text::format_duration(remaining, false));
        let finish = finish_time((self.s.clock)(), remaining);
        if !finish.is_empty() {
            eta.push_str(&format!(" \u{b7} {finish}"));
        }
        Some(eta)
    }
}

/// The text the strip shows for a paused or idle estimate.
pub fn paused_eta(quota_paused: bool) -> &'static str {
    if quota_paused {
        QUOTA_PAUSED_ETA
    } else {
        crate::shell::IDLE_ETA
    }
}

/// The note `hide_quota_banner` clears.
pub const PAUSE_NOTE: &str = QUOTA_PAUSED_NOTE;

/// The run, on a worker thread: `SearchWorker._run` over the batch.
#[allow(clippy::too_many_arguments)]
fn run_search(
    backend: &SearchBackend,
    settings: &Arc<Mutex<hatate_core::config::Settings>>,
    paths: &Paths,
    stop: Arc<AtomicBool>,
    bus: Bus,
    run: u64,
    mut batch: Vec<Entry>,
    pending: Vec<usize>,
    opts: PendingOptions,
    handle: tokio::runtime::Handle,
) {
    let sink_bus = bus.clone();
    let sink: RunSink = Arc::new(move |event| match event {
        RunEvent::State(RunState::Ended(end)) => {
            sink_bus.post(BusEvent::Run(run, Msg::Ended(end)));
        }
        RunEvent::State(_) => {}
        RunEvent::Lens(c) => sink_bus.post(BusEvent::Lens(c)),
    });
    let next = Arc::new(WindowNext {
        bus: bus.clone(),
        run,
    });
    let channel = RunChannel::new(sink, backend.engine.clock.clone())
        .with_stop_record(StopRecordFile::new(paths))
        .with_next(next);
    let reporter: Arc<dyn Reporter> = Arc::new(channel);
    let quota_file = QuotaPauseFile::new(paths);
    let env = BatchEnv {
        engine: &backend.engine,
        settings,
        stop,
        quota_exhausted: &*backend.quota_exhausted,
        quota_file: &quota_file,
        sink: &NoSave,
        reporter: reporter.clone(),
        unit_random: &*backend.unit_random,
        reset_run_state: &*backend.reset_run_state,
        hydrus_client: &*backend.hydrus_client,
    };
    let now = backend.engine.clock.wall();
    let mut autosave = Autosave::new(&NoSave, &*reporter, now);
    let end = handle.block_on(search_pending_with(
        &env,
        &mut batch,
        pending,
        &mut autosave,
        &opts,
    ));
    tracing::info!(
        "Search run ended: {:?} after {} search(es)",
        end.ending,
        end.searched
    );
}
