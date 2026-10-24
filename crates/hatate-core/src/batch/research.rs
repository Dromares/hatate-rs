//! The search a queue request starts (`MainWindow._research_rows` and
//! `_launch_search_worker(..., bypass_cache=True)`, DAN-2167).
//!
//! The window hands the worker its entries and lets it change them in place. The queue model is
//! owned by one thread, so the search runs over copies and the model takes the searched copies
//! back when the run ends ([`SearchDone::finish`]). The run itself is the merged
//! [`search_pending`]: pacing, retry, quota pause, auto-import, and every engine. What differs
//! from a batch is only what the window decides: the cache is bypassed (the point of a re-search
//! is a fresh look) and the session is not written from here (the model's owner saves its list).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tracing::{info, warn};

use super::run::{Autosave, BatchEnv, Reporter, SessionSink, search_pending};
use crate::config::{HydrusSettings, Settings};
use crate::hydrus_client::{HydrusClient, HydrusError};
use crate::model::Entry;
use crate::queue::{EntryId, SearchDone, SearchLauncher};
use crate::quota_pause::QuotaPauseFile;
use crate::search_engine::SearchEngine;

/// A sink that writes nothing: the queue's owner saves its own list.
struct NoSave;

impl SessionSink for NoSave {
    fn save(&self, _entries: &[Entry], _now: f64) -> Result<(), String> {
        Ok(())
    }
}

/// Runs queue searches with [`search_pending`] on a thread of its own.
pub struct BatchSearchLauncher {
    pub engine: Arc<SearchEngine>,
    /// `window.settings`.
    pub settings: Arc<Mutex<Settings>>,
    /// Raised to stop the run between images (the window's Stop Search).
    pub stop: Arc<AtomicBool>,
    /// `saucenao.is_daily_quota_exhausted`.
    pub quota_exhausted: Arc<dyn Fn() -> bool + Send + Sync>,
    pub quota_file: QuotaPauseFile,
    pub reporter: Arc<dyn Reporter>,
    /// A number in `[0, 1)`.
    pub unit_random: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// What `_launch_search_worker` does before it starts the worker.
    pub reset_run_state: Arc<dyn Fn() + Send + Sync>,
    pub hydrus_client:
        Arc<dyn Fn(&HydrusSettings) -> Result<HydrusClient, HydrusError> + Send + Sync>,
    pub runtime: tokio::runtime::Handle,
}

impl SearchLauncher for BatchSearchLauncher {
    fn launch(
        &self,
        batch: Vec<(EntryId, Entry)>,
        pending: Vec<usize>,
        done: SearchDone,
    ) -> Result<(), String> {
        // Starting a search is the user saying "try again".
        (self.reset_run_state)();
        self.stop.store(false, std::sync::atomic::Ordering::SeqCst);
        let me = Arc::new(Run {
            engine: self.engine.clone(),
            settings: self.settings.clone(),
            stop: self.stop.clone(),
            quota_exhausted: self.quota_exhausted.clone(),
            quota_file: self.quota_file.clone(),
            reporter: self.reporter.clone(),
            unit_random: self.unit_random.clone(),
            reset_run_state: self.reset_run_state.clone(),
            hydrus_client: self.hydrus_client.clone(),
            runtime: self.runtime.clone(),
        });
        std::thread::Builder::new()
            .name("queue-research".to_owned())
            .spawn(move || me.run(batch, pending, done))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

struct Run {
    engine: Arc<SearchEngine>,
    settings: Arc<Mutex<Settings>>,
    stop: Arc<AtomicBool>,
    quota_exhausted: Arc<dyn Fn() -> bool + Send + Sync>,
    quota_file: QuotaPauseFile,
    reporter: Arc<dyn Reporter>,
    unit_random: Arc<dyn Fn() -> f64 + Send + Sync>,
    reset_run_state: Arc<dyn Fn() + Send + Sync>,
    hydrus_client:
        Arc<dyn Fn(&HydrusSettings) -> Result<HydrusClient, HydrusError> + Send + Sync>,
    runtime: tokio::runtime::Handle,
}

impl Run {
    fn run(&self, batch: Vec<(EntryId, Entry)>, pending: Vec<usize>, done: SearchDone) {
        let (ids, mut entries): (Vec<EntryId>, Vec<Entry>) = batch.into_iter().unzip();
        // `bypass_cache`: `use_cache = settings.use_search_cache and not bypass_cache`.
        let bypass = {
            let mut copy = self.settings.lock().expect("settings lock").clone();
            copy.use_search_cache = false;
            Mutex::new(copy)
        };
        let env = BatchEnv {
            engine: &self.engine,
            settings: &bypass,
            stop: self.stop.clone(),
            quota_exhausted: &*self.quota_exhausted,
            quota_file: &self.quota_file,
            sink: &NoSave,
            reporter: self.reporter.clone(),
            unit_random: &*self.unit_random,
            reset_run_state: &*self.reset_run_state,
            hydrus_client: &*self.hydrus_client,
        };
        let now = self.engine.clock.wall();
        let mut autosave = Autosave::new(&NoSave, &*self.reporter, now);
        info!("Search worker for a re-search: {} image(s)", pending.len());
        let end = self
            .runtime
            .block_on(search_pending(&env, &mut entries, pending, &mut autosave));
        info!(
            "Re-search ended: {:?} after {} search(es)",
            end.ending, end.searched
        );
        if end.removed.len() != entries.len() {
            warn!("Re-search: the removal flags do not line up with the entries");
        }
        let removed: Vec<EntryId> = ids
            .iter()
            .zip(end.removed.iter().chain(std::iter::repeat(&false)))
            .filter(|(_, gone)| **gone)
            .map(|(id, _)| *id)
            .collect();
        done.finish(ids.into_iter().zip(entries).collect(), removed);
    }
}
