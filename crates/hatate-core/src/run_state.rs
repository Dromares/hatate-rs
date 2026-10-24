//! The runner's state, as one stream the window renders (`phase6-calls` call 2, DAN-2116 item 5,
//! DAN-2183, DAN-2224).
//!
//! [`RunChannel`] is a [`Reporter`]: hand it to the search loop (`BatchEnv.reporter`) and it turns
//! the loop's hooks into [`RunState`]s, in the order Python's worker signals arrive (`image_updated`
//! -> `searching`, `progress`, `wait_countdown`, `paused_out_of_quota`, `finished_all`). The same
//! sink gets [`LensConnection`] changes, through [`RunChannel::lens_sink`], which is what
//! `LensClient::with_connection_sink` takes. `hatate batch` does not use it and keeps its
//! output and exit codes.
//!
//! Endings (the vocabulary of plan rev 12):
//! - [`RunEnd::Completed`]: every image was searched.
//! - [`RunEnd::Interrupted`]: a stop request (Stop, SIGINT, SIGTERM) flushed the session and the
//!   crash marker was removed. The channel notes it in `hatate-rs.json` ([`crate::stop_record`]) when
//!   it is given the file.
//! - [`RunEnd::Crashed`] and [`RunEnd::FailedEmpty`]: launch states only. The previous run left the
//!   crash marker; [`crate::stop_record::launch_banner`] decides which. Core cannot tell a kill
//!   from a panic.
//! - [`RunEnd::Failed`]: `Ending::Failed(RunFault)`, with the fault's path-free `user_message`.
//!   Exit 1, the session kept, the rest unsearched. Never persisted.
//!
//! Paused for quota is a state of its own, not an ending.

use std::sync::{Arc, Mutex};

use tracing::warn;

use crate::batch::run::{Ending, Reporter};
use crate::clock::Clock;
use crate::engines::lens_search::{ConnectionSink, LensConnection};
use crate::eta::RunEstimate;
use crate::model::Entry;
use crate::stop_record::{StopRecord, StopRecordFile};

/// How far a run is.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub done: i64,
    pub total: i64,
    /// The file being searched (`Entry::filename`), the one `searching` last named.
    pub current: Option<String>,
    /// `RunEstimate::seconds_remaining`, or `None` while the pace is unknown.
    pub eta_seconds: Option<f64>,
}

/// A rate-limit or engine wait under way (`wait_countdown`).
#[derive(Debug, Clone, PartialEq)]
pub struct Wait {
    pub progress: Progress,
    /// `"Rate limit"`, or an engine's own label.
    pub label: String,
    /// Seconds left.
    pub remaining: f64,
    /// Seconds the wait started with.
    pub total: f64,
}

/// How a run ended, or what a launch found of the last one.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEnd {
    Completed,
    Interrupted,
    Crashed,
    /// `fault.user_message()`: what failed and what to do, no path, URL or OS text.
    Failed(String),
    FailedEmpty,
}

/// What the run strip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    Idle,
    Running(Progress),
    Waiting(Wait),
    /// SauceNAO's daily allowance ran out (`paused_out_of_quota`).
    PausedForQuota {
        searched: i64,
        remaining: i64,
    },
    Ended(RunEnd),
}

/// One thing on the channel.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    State(RunState),
    Lens(LensConnection),
}

/// Where the events go. Called on whatever thread the loop runs on: the window hops to its main
/// loop through the worker bridge.
pub type RunSink = Arc<dyn Fn(RunEvent) + Send + Sync + 'static>;

struct Inner {
    state: RunState,
    estimate: RunEstimate,
    progress: Progress,
}

impl Inner {
    fn fresh() -> Inner {
        Inner {
            state: RunState::Idle,
            estimate: RunEstimate::new(),
            progress: Progress {
                done: 0,
                total: 0,
                current: None,
                eta_seconds: None,
            },
        }
    }
}

/// The [`Reporter`] that makes [`RunState`]s.
#[derive(Clone)]
pub struct RunChannel {
    inner: Arc<Mutex<Inner>>,
    sink: RunSink,
    clock: Arc<dyn Clock>,
    stop_record: Option<StopRecordFile>,
    next: Option<Arc<dyn Reporter>>,
}

impl RunChannel {
    pub fn new(sink: RunSink, clock: Arc<dyn Clock>) -> RunChannel {
        RunChannel {
            inner: Arc::new(Mutex::new(Inner::fresh())),
            sink,
            clock,
            stop_record: None,
            next: None,
        }
    }

    /// Where the stop record goes. Without it a stop is only a state.
    #[must_use]
    pub fn with_stop_record(mut self, file: StopRecordFile) -> RunChannel {
        self.stop_record = Some(file);
        self
    }

    /// Passes every hook on to another reporter (the window's log, say) after the channel has
    /// handled it.
    #[must_use]
    pub fn with_next(mut self, next: Arc<dyn Reporter>) -> RunChannel {
        self.next = Some(next);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The state now.
    pub fn state(&self) -> RunState {
        self.lock().state.clone()
    }

    /// The sink for `LensClient::with_connection_sink`: a connection change goes out on this
    /// channel, in order with the run's own states.
    pub fn lens_sink(&self) -> ConnectionSink {
        let sink = Arc::clone(&self.sink);
        Arc::new(move |c| sink(RunEvent::Lens(c)))
    }

    /// Shows a state that is not the loop's: a launch finding, or a run that ended before the
    /// search began. Replaces whatever is there.
    pub fn show(&self, state: RunState) {
        let mut inner = self.lock();
        inner.state = state.clone();
        drop(inner);
        (self.sink)(RunEvent::State(state));
    }

    /// A run starts with `total` images: the estimate begins afresh and the stop record, if any,
    /// is cleared (a run is under way, so there is no unfinished stop to remind of). Called by
    /// the window when it launches the worker; [`Reporter::searching`] does it by itself for a
    /// loop nobody announced.
    pub fn begin(&self, total: i64) {
        let state = self.reset(total);
        (self.sink)(RunEvent::State(state));
    }

    /// [`RunChannel::begin`] without announcing it.
    fn reset(&self, total: i64) -> RunState {
        self.clear_stop_record();
        let mut inner = self.lock();
        inner.estimate.reset();
        inner.progress = Progress {
            done: 0,
            total,
            current: None,
            eta_seconds: None,
        };
        let state = RunState::Running(inner.progress.clone());
        inner.state = state.clone();
        state
    }

    /// The run is over. Idempotent: the first ending of a run wins.
    pub fn end(&self, ending: &Ending) {
        let (state, unsearched) = {
            let inner = self.lock();
            if matches!(inner.state, RunState::Ended(_)) {
                return;
            }
            let unsearched = (inner.progress.total - inner.progress.done).max(0);
            let state = match ending {
                Ending::Finished => RunState::Ended(RunEnd::Completed),
                Ending::Stopped => RunState::Ended(RunEnd::Interrupted),
                Ending::PausedForQuota {
                    searched,
                    remaining,
                } => RunState::PausedForQuota {
                    searched: *searched,
                    remaining: *remaining,
                },
                Ending::Failed(fault) => RunState::Ended(RunEnd::Failed(fault.user_message())),
            };
            (state, unsearched)
        };
        if matches!(ending, Ending::Stopped) {
            self.write_stop_record(unsearched);
        }
        let changed = {
            let mut inner = self.lock();
            let changed = inner.state != state;
            inner.state = state.clone();
            changed
        };
        if changed {
            (self.sink)(RunEvent::State(state));
        }
    }

    fn write_stop_record(&self, unsearched: i64) {
        let Some(file) = &self.stop_record else {
            return;
        };
        let record = StopRecord {
            at: self.clock.wall(),
            unsearched,
        };
        if let Err(e) = file.write(&record) {
            // Non-fatal: a stop never fails because the note could not be written.
            warn!("Could not record the stop for the next launch: {e}");
        }
    }

    /// Resume Queue and Dismiss.
    pub fn clear_stop_record(&self) {
        if let Some(file) = &self.stop_record {
            if let Err(e) = file.clear() {
                warn!("Could not clear the stop record: {e}");
            }
        }
    }

    fn set(&self, state: RunState) {
        let mut inner = self.lock();
        if matches!(inner.state, RunState::Ended(_)) || inner.state == state {
            return;
        }
        inner.state = state.clone();
        drop(inner);
        (self.sink)(RunEvent::State(state));
    }
}

impl Reporter for RunChannel {
    fn line(&self, text: &str) {
        if let Some(next) = &self.next {
            next.line(text);
        }
    }

    fn countdown(&self, label: &str, remaining: f64, total: f64) {
        let state = {
            let inner = self.lock();
            if matches!(inner.state, RunState::Idle | RunState::Ended(_)) {
                None
            } else if label.is_empty() || remaining <= 0.0 {
                Some(RunState::Running(inner.progress.clone()))
            } else {
                Some(RunState::Waiting(Wait {
                    progress: inner.progress.clone(),
                    label: label.to_owned(),
                    remaining,
                    total,
                }))
            }
        };
        if let Some(state) = state {
            self.set(state);
        }
        if let Some(next) = &self.next {
            next.countdown(label, remaining, total);
        }
    }

    fn auto_imported(&self, entry: &Entry, result: &crate::hydrus_import::ImportResult) {
        if let Some(next) = &self.next {
            next.auto_imported(entry, result);
        }
    }

    fn searching(&self, entry: &Entry, done: usize, total: usize) {
        let starts = matches!(self.lock().state, RunState::Idle | RunState::Ended(_));
        if starts {
            // Silently: the state below is the first thing the run says.
            self.reset(total as i64);
        }
        let state = {
            let mut inner = self.lock();
            inner.progress.done = done as i64;
            inner.progress.total = total as i64;
            inner.progress.current = Some(entry.filename().to_owned());
            RunState::Running(inner.progress.clone())
        };
        self.set(state);
        if let Some(next) = &self.next {
            next.searching(entry, done, total);
        }
    }

    fn progress(&self, done: usize, total: usize) {
        let now = self.clock.monotonic();
        let state = {
            let mut inner = self.lock();
            inner.estimate.record(done as i64, total as i64, Some(now));
            let remaining = inner.estimate.seconds_remaining();
            inner.progress.done = done as i64;
            inner.progress.total = total as i64;
            inner.progress.eta_seconds = (remaining >= 0.0).then_some(remaining);
            RunState::Running(inner.progress.clone())
        };
        self.set(state);
        if let Some(next) = &self.next {
            next.progress(done, total);
        }
    }

    fn paused_for_quota(&self, searched: i64, remaining: i64) {
        self.set(RunState::PausedForQuota {
            searched,
            remaining,
        });
        if let Some(next) = &self.next {
            next.paused_for_quota(searched, remaining);
        }
    }

    fn finished(&self, ending: &Ending) {
        self.end(ending);
        if let Some(next) = &self.next {
            next.finished(ending);
        }
    }
}
