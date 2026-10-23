//! The search loop of `hatate batch` (`workers/search_worker.py` without Qt), and the steps
//! around it that `gui/main_window.py` does for "Add Folder" and "Start Search": scan, hash, drop
//! duplicates, add, search what is not yet searched, keep the session saved.
//!
//! What is here that the Python loop does with signals is a [`Reporter`] (a line of progress, a
//! countdown) and a stop flag. What it does with a timer is [`Autosave`]. The search itself is
//! `SearchEngine::search_image` (Phase 3.h); the waits between images are `net::pacing` (Phase 1).
//!
//! **Not in this build:** Hydrus auto-import (`_maybe_auto_import`, the staging directory and its
//! `owner.pid`, `_switch_to_importable_match`). Phase 4 owns it. A batch with `auto_import_enabled`
//! on says so once and carries on without it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

use super::hashing::hash_files;
use super::intake::{filter_duplicate_paths, scan_folder};
use super::matched_urls::log_matched_url;
use crate::config::Settings;
use crate::model::{Entry, MatchStatus};
use crate::net::pacing::{EngineRateLimiter, engine_interval_for, remaining_delay, uniform};
use crate::quota_pause::{QuotaPause, QuotaPauseFile};
use crate::search_engine::{SearchEngine, SearchOptions};
use crate::session::SessionWriter;

/// `MIN_INTERVAL_SECONDS` of `gui/session_autosave.py`.
pub const MIN_AUTOSAVE_INTERVAL_SECONDS: i64 = 10;

/// How often a wait looks at the stop flag and the clock (`_wait_with_countdown`'s `step`).
const WAIT_STEP: f64 = 0.25;

/// Where progress goes. The `batch` command prints it to stderr; a test records it.
pub trait Reporter: Send + Sync {
    /// One line of progress or news.
    fn line(&self, text: &str);
    /// `wait_countdown(label, remaining, total)`: a wait is under way. `("", 0.0, 0.0)` clears it.
    fn countdown(&self, label: &str, remaining: f64, total: f64);
}

/// A reporter that says nothing.
pub struct Silent;

impl Reporter for Silent {
    fn line(&self, _text: &str) {}
    fn countdown(&self, _label: &str, _remaining: f64, _total: f64) {}
}

/// Where the list is saved.
pub trait SessionSink: Send + Sync {
    fn save(&self, entries: &[Entry], now: f64) -> Result<(), String>;
}

impl SessionSink for SessionWriter {
    fn save(&self, entries: &[Entry], now: f64) -> Result<(), String> {
        SessionWriter::save(self, entries, now).map_err(|e| e.to_string())
    }
}

/// Everything the loop is given. The batch owns none of it: the caller builds the engine (the net
/// layer, the engines, the cache) and decides where the session goes.
pub struct BatchEnv<'a> {
    pub engine: &'a SearchEngine,
    pub settings: &'a Mutex<Settings>,
    /// Raised by the signal handler. Checked between images, inside every wait and by the
    /// engines' own waits (SauceNAO's burst window).
    pub stop: Arc<AtomicBool>,
    /// `saucenao.is_daily_quota_exhausted`.
    pub quota_exhausted: &'a (dyn Fn() -> bool + Send + Sync),
    pub quota_file: &'a QuotaPauseFile,
    pub sink: &'a dyn SessionSink,
    pub reporter: Arc<dyn Reporter>,
    /// A number in `[0, 1)`: `random.uniform`'s source.
    pub unit_random: &'a (dyn Fn() -> f64 + Send + Sync),
    /// What `_launch_search_worker` does before it starts the worker: forget the remembered quota
    /// exhaustion, and every engine that stood down on an earlier run (ascii2d, Google, Lens,
    /// Yandex).
    pub reset_run_state: &'a (dyn Fn() + Send + Sync),
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Every image in the queue was searched.
    Finished,
    /// Stop was requested.
    Stopped,
    /// SauceNAO's daily allowance ran out and the run paused (recorded in the pause file).
    PausedForQuota { searched: i64, remaining: i64 },
}

/// What a batch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub ending: Ending,
    /// Files found by the scan.
    pub found: usize,
    /// Found files that were already in the list.
    pub duplicates: usize,
    /// Files added to the list.
    pub added: usize,
    /// Images that went through `search_image` (a retry counts again).
    pub searched: usize,
    /// Whether the last save of the list failed (the message is in the log and on the reporter).
    pub save_failed: bool,
    /// The scan found nothing to add, or a stop arrived while hashing.
    pub nothing_to_search: bool,
}

/// `SessionAutosaver`'s clock: saves the whole list when the interval has passed.
pub struct Autosave<'a> {
    sink: &'a dyn SessionSink,
    reporter: &'a dyn Reporter,
    last: f64,
    pub failed: bool,
}

impl<'a> Autosave<'a> {
    pub fn new(sink: &'a dyn SessionSink, reporter: &'a dyn Reporter, now: f64) -> Self {
        Autosave { sink, reporter, last: now, failed: false }
    }

    /// Saves if autosave is on and the interval (at least ten seconds) has passed.
    pub fn tick(&mut self, settings: &Settings, entries: &[Entry], now: f64) {
        if !settings.autosave_session || entries.is_empty() {
            return;
        }
        let interval = settings.autosave_interval_seconds.max(MIN_AUTOSAVE_INTERVAL_SECONDS);
        if now - self.last >= interval as f64 {
            self.save(entries, now);
        }
    }

    /// Saves now, whatever the interval. An empty list is not written: clearing the list must not
    /// quietly destroy a session that still had useful contents (`SessionAutosaver.save_now`).
    pub fn save(&mut self, entries: &[Entry], now: f64) {
        self.last = now;
        if entries.is_empty() {
            return;
        }
        match self.sink.save(entries, now) {
            Ok(()) => {
                self.failed = false;
                debug!("Autosaved session ({} entries)", entries.len());
            }
            Err(why) => {
                self.failed = true;
                warn!("Autosave failed: {why}");
                self.reporter.line(&format!("warning: could not save the session: {why}"));
            }
        }
    }
}

/// The whole batch: `folder` is scanned, the files are hashed and de-duplicated against
/// `entries`, the new ones are added, and everything not yet searched is searched. The list is
/// saved when files are added, on the autosave interval, and at the end however it ends.
pub async fn run_batch(env: &BatchEnv<'_>, folder: &Path, entries: &mut Vec<Entry>) -> Outcome {
    let clock = env.engine.clock.clone();
    let mut autosave = Autosave::new(env.sink, env.reporter.as_ref(), clock.wall());
    let mut outcome = Outcome {
        ending: Ending::Finished,
        found: 0,
        duplicates: 0,
        added: 0,
        searched: 0,
        save_failed: false,
        nothing_to_search: false,
    };

    let settings = env.settings.lock().unwrap().clone();
    if settings.auto_import_enabled {
        env.reporter
            .line("auto-import is not in this build yet; carrying on without it");
    }

    let files = scan_folder(folder);
    outcome.found = files.len();
    env.reporter
        .line(&format!("found {} file(s) under {}", files.len(), folder.display()));

    // `_import_files`: the entries already in the list that have no hash yet are hashed in the
    // same pass, piggybacking on it rather than a separate one later.
    let needing_hash: Vec<PathBuf> = entries
        .iter()
        .filter(|e| e.hydrus_hash.as_deref().is_none_or(str::is_empty))
        .map(|e| PathBuf::from(&e.path))
        .collect();
    let to_hash: Vec<PathBuf> = needing_hash.into_iter().chain(files.iter().cloned()).collect();

    if !to_hash.is_empty() {
        env.reporter.line(&format!("hashing {} file(s)", to_hash.len()));
        let progress = |done: usize, total: usize, eta: f64| {
            let eta = if eta >= 0.0 { format!(", about {eta:.0}s left") } else { String::new() };
            env.reporter.line(&format!("hashing {done}/{total}{eta}"));
        };
        let Some(path_to_hash) = hash_files(
            &to_hash,
            settings.hash_workers,
            &settings.hash_source,
            &env.stop,
            Some(&progress),
        ) else {
            outcome.ending = Ending::Stopped;
            outcome.nothing_to_search = true;
            return outcome;
        };

        for e in entries.iter_mut() {
            if e.hydrus_hash.as_deref().is_none_or(str::is_empty) {
                if let Some(h) = path_to_hash.get(Path::new(&e.path)) {
                    e.hydrus_hash = Some(h.clone());
                }
            }
        }
        let (unique, duplicates) = filter_duplicate_paths(&files, &path_to_hash, entries);
        outcome.duplicates = duplicates;
        outcome.added = unique.len();
        info!(
            "Adding {} file(s) to the list ({duplicates} duplicate(s) skipped)",
            unique.len()
        );
        for p in &unique {
            let mut entry = Entry::new(p.to_string_lossy());
            entry.hydrus_hash = path_to_hash.get(p).cloned();
            entries.push(entry);
        }
        env.reporter.line(&format!(
            "added {} file(s), {} total ({duplicates} duplicate(s) skipped)",
            unique.len(),
            entries.len()
        ));
        autosave.save(entries, clock.wall());
    }

    let pending: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.status == MatchStatus::NotSearched)
        .map(|(i, _)| i)
        .collect();
    if pending.is_empty() {
        outcome.nothing_to_search = true;
        env.reporter.line("no unsearched images in the list");
    } else {
        info!("Start Search: {} unsearched image(s)", pending.len());
        (env.reset_run_state)();
        if let Err(e) = env.quota_file.clear() {
            warn!("Could not clear the SauceNAO quota pause: {e}");
        }
        let (ending, searched) = search_pending(env, entries, pending, &mut autosave).await;
        outcome.ending = ending;
        outcome.searched = searched;
    }

    if env.stop.load(Ordering::SeqCst) && outcome.ending == Ending::Finished {
        outcome.ending = Ending::Stopped;
    }
    autosave.save(entries, clock.wall());
    outcome.save_failed = autosave.failed;
    outcome
}

/// `SearchWorker._run`: searches `pending` (indexes into `entries`) one at a time, pacing the
/// requests, retrying network faults once, and stopping on the flag or an exhausted daily quota.
/// Returns how it ended and how many searches ran.
pub async fn search_pending(
    env: &BatchEnv<'_>,
    entries: &mut [Entry],
    pending: Vec<usize>,
    autosave: &mut Autosave<'_>,
) -> (Ending, usize) {
    let clock = env.engine.clock.clone();
    let limiter = EngineRateLimiter::new();
    let mut announced_quota_continue = false;
    let mut retries_queued = false;
    let mut queue = pending;
    let mut searched = 0usize;
    let mut ending = Ending::Finished;
    info!("Search worker starting: {} image(s) to process", queue.len());

    let mut i = 0usize;
    while i < queue.len() {
        let idx = queue[i];
        let total = queue.len();
        if env.stop.load(Ordering::SeqCst) {
            info!("Search worker stopping early at {i}/{total} (user requested)");
            ending = Ending::Stopped;
            break;
        }

        {
            let entry = &mut entries[idx];
            entry.interrupted_mid_search = false;
            entry.status = MatchStatus::Searching;
            entry.result_source = None;
        }
        let engines_finished_at: Mutex<Option<f64>> = Mutex::new(None);
        let engines_reported = AtomicUsize::new(0);
        let note_done = || {
            *engines_finished_at.lock().unwrap() = Some(clock.monotonic());
        };
        let note_finished = |engine: &str| {
            engines_reported.fetch_add(1, Ordering::SeqCst);
            limiter.record(engine, clock.monotonic());
        };

        let skip_saucenao = skip_saucenao_now(env);
        let (use_cache, delay_min, delay_max) = {
            let s = env.settings.lock().unwrap();
            (s.use_search_cache, s.delay_min_seconds, s.delay_max_seconds)
        };
        let stop_flag = env.stop.clone();
        let tick: crate::engines::client::OnTick = {
            let reporter = env.reporter.clone();
            Arc::new(move |label: &str, remaining: f64, total: f64| {
                reporter.countdown(label, remaining, total)
            })
        };
        let result = env
            .engine
            .search_image(
                &mut entries[idx],
                env.settings,
                SearchOptions {
                    override_engine: None,
                    use_cache,
                    on_tick: Some(tick),
                    on_engines_done: Some(&note_done),
                    on_engine_finished: Some(&note_finished),
                    should_stop: Some(Arc::new(move || stop_flag.load(Ordering::SeqCst))),
                    skip_saucenao,
                },
            )
            .await;
        searched += 1;
        if let Err(exc) = result {
            // A bug or an unexpected condition must not end the whole batch: mark this one image
            // and keep going.
            tracing::error!("Unexpected error searching {}: {exc}", entries[idx].filename());
            let entry = &mut entries[idx];
            entry.status = MatchStatus::Error;
            entry.error_message = Some(format!("Unexpected error: {exc}"));
        }

        let entry_status = entries[idx].status;
        env.reporter.line(&format!(
            "({}/{total}) {} -> {}",
            i + 1,
            entries[idx].filename(),
            entry_status.as_str()
        ));

        let snapshot = env.settings.lock().unwrap().clone();
        if snapshot.log_matched_urls
            && matches!(entry_status, MatchStatus::Good | MatchStatus::Poor)
        {
            if let Some(url) = entries[idx]
                .selected_candidate()
                .map(|c| c.url.as_str())
                .filter(|u| !u.is_empty())
            {
                log_matched_url(
                    &snapshot.log_file_path,
                    &clock.local_stamp(clock.wall()),
                    entries[idx].filename(),
                    url,
                );
            }
        }
        autosave.tick(&snapshot, entries, clock.wall());

        let remaining = (total - (i + 1)) as i64;
        if remaining > 0 && skip_saucenao_now(env) {
            if !announced_quota_continue {
                announced_quota_continue = true;
                warn!(
                    "SauceNAO's daily quota is exhausted - carrying on without it for the \
                     remaining {remaining} image(s). Those results are marked provisional and \
                     are not cached, so re-searching them once the allowance resets gets the \
                     full answer"
                );
                env.reporter.line(&format!(
                    "SauceNAO's daily quota is exhausted: carrying on without it for the \
                     remaining {remaining} image(s)"
                ));
            }
        } else if remaining > 0 && saucenao_quota_should_pause(env) {
            let done = (i + 1) as i64;
            warn!(
                "SauceNAO's daily quota is exhausted - pausing after {done}/{total} image(s), \
                 leaving {remaining} unsearched"
            );
            // Persisted, not just printed: the point of an unattended run is that nobody
            // necessarily is looking (DAN-486).
            if let Err(e) = env
                .quota_file
                .record(&QuotaPause::starting_at(clock.wall(), done, remaining))
            {
                warn!("Could not record the SauceNAO quota pause: {e}");
            }
            env.reporter.line(&format!(
                "SauceNAO's daily quota is exhausted: paused after {done} image(s), \
                 {remaining} left unsearched"
            ));
            ending = Ending::PausedForQuota { searched: done, remaining };
            break;
        }

        if i == queue.len() - 1 && !retries_queued {
            // End of the first pass. An ERROR is a network fault, not a verdict (the cache refuses
            // to remember one), so each gets exactly one more attempt.
            retries_queued = true;
            let again = queue_retries(env, entries, &queue);
            queue.extend(again);
        }

        if env.stop.load(Ordering::SeqCst) || i == queue.len() - 1 {
            i += 1;
            continue;
        }

        if entries[idx].result_source.as_deref() == Some("cached") {
            // No real engine request was made, so there is nothing to rate-limit against.
            debug!("{} was served from the search cache, skipping the rate-limit wait",
                entries[idx].filename());
            i += 1;
            continue;
        }

        let delay = uniform(delay_min, delay_max, (env.unit_random)());
        let next_override: Option<&str> = None;
        let upcoming = crate::engine_runner::planned_engines(&snapshot, next_override, false);
        let intervals = upcoming
            .iter()
            .map(|engine| {
                let (low, high) = engine_interval_for(&snapshot, engine);
                ((*engine).to_owned(), uniform(low, high, (env.unit_random)()))
            })
            .collect();
        let wait = if engines_reported.load(Ordering::SeqCst) > 0 {
            limiter.wait_needed(&upcoming, &intervals, clock.monotonic())
        } else {
            // A fresh search ran but no engine reported finishing, so the per-engine clocks cannot
            // be trusted for it. Fall back to the global delay: this mechanism must fail by
            // waiting too long, never by not waiting.
            warn!(
                "No engine reported completing for {}; falling back to the global delay rather \
                 than per-host pacing",
                entries[idx].filename()
            );
            remaining_delay(delay, *engines_finished_at.lock().unwrap(), clock.monotonic())
        };
        if wait < delay {
            debug!(
                "Waiting {wait:.1}s before next search (rate limiting; {:.1}s of the {delay:.1}s \
                 gap already elapsed on non-engine work)",
                delay - wait
            );
        } else {
            debug!("Waiting {wait:.1}s before next search (rate limiting)");
        }
        if wait > 0.0 {
            wait_with_countdown(env, wait, entries, autosave).await;
        }
        i += 1;
    }
    info!("Search worker finished ({searched} search(es))");
    (ending, searched)
}

/// `_queue_retries`: the images worth one more attempt after the first pass.
fn queue_retries(env: &BatchEnv<'_>, entries: &[Entry], done: &[usize]) -> Vec<usize> {
    let settings = env.settings.lock().unwrap().clone();
    if !settings.retry_failed_searches {
        return Vec::new();
    }
    if env.stop.load(Ordering::SeqCst) || saucenao_quota_should_pause(env) {
        return Vec::new();
    }
    let failed: Vec<usize> = done
        .iter()
        .copied()
        .filter(|&i| entries[i].status == MatchStatus::Error)
        .collect();
    let retryable: Vec<usize> = failed
        .iter()
        .copied()
        .filter(|&i| env.engine.runner.retry_could_help(entries[i].error_message.as_deref()))
        .collect();
    if retryable.len() < failed.len() {
        // Their only failures were engines that have stood down for the run: a retry would skip
        // that engine and fail again.
        info!(
            "Not retrying {} image(s): the engines that failed on them have stopped for this run",
            failed.len() - retryable.len()
        );
    }
    if retryable.is_empty() {
        return Vec::new();
    }
    info!(
        "First pass done - retrying {} image(s) that failed on a network fault",
        retryable.len()
    );
    env.reporter.line(&format!(
        "first pass done: retrying {} image(s) that failed on a network fault",
        retryable.len()
    ));
    retryable
}

fn saucenao_in_use(settings: &Settings) -> bool {
    settings.primary_engine == "saucenao" || settings.secondary_engine_mode != "disabled"
}

/// `_skip_saucenao_now`: whether to leave SauceNAO out of the searches from here on. Only when the
/// setting asks for it. A batch has no "Continue with other engines" dialog, so the one-shot
/// override of the Python worker (`force_continue_without_saucenao`) is always off.
fn skip_saucenao_now(env: &BatchEnv<'_>) -> bool {
    let s = env.settings.lock().unwrap();
    if !s.continue_without_saucenao_on_quota || !saucenao_in_use(&s) {
        return false;
    }
    drop(s);
    (env.quota_exhausted)()
}

/// `_saucenao_quota_should_pause`: whether to stop because SauceNAO's daily allowance is spent.
/// Only when SauceNAO is in the engine order, and not when the user has said to carry on without it.
fn saucenao_quota_should_pause(env: &BatchEnv<'_>) -> bool {
    let s = env.settings.lock().unwrap();
    if !s.saucenao.pause_search_on_quota_exhausted
        || s.continue_without_saucenao_on_quota
        || !saucenao_in_use(&s)
    {
        return false;
    }
    drop(s);
    (env.quota_exhausted)()
}

/// `_wait_with_countdown`: sleeps `delay` seconds, reporting once each time the displayed second
/// changes. Counted against a fixed deadline rather than by adding up sleeps, so a long wait does
/// not drift past what it was asked for. Wakes for the stop flag; keeps the session on its
/// autosave interval while it waits.
async fn wait_with_countdown(
    env: &BatchEnv<'_>,
    delay: f64,
    entries: &[Entry],
    autosave: &mut Autosave<'_>,
) {
    if delay <= 0.0 {
        return;
    }
    let clock = env.engine.clock.clone();
    let deadline = clock.monotonic() + delay;
    let mut last_shown: Option<i64> = None;
    while !env.stop.load(Ordering::SeqCst) {
        let remaining = deadline - clock.monotonic();
        if remaining <= 0.0 {
            break;
        }
        let shown = remaining.round() as i64;
        if last_shown != Some(shown) {
            env.reporter.countdown("Rate limit", remaining, delay);
            last_shown = Some(shown);
        }
        let snapshot = env.settings.lock().unwrap().clone();
        autosave.tick(&snapshot, entries, clock.wall());
        // Never sleep past the deadline: crediting non-engine work routinely leaves a wait shorter
        // than one step.
        clock.sleep(remaining.min(WAIT_STEP)).await;
    }
    env.reporter.countdown("", 0.0, 0.0);
}
