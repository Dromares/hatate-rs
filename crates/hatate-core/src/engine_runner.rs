//! Running the search engines for one image (`core/engine_runner.py`): which ones, in what waves,
//! and collecting what each returns into candidates and errors.
//!
//! **Where the cut is.** The wave loop itself lives in Python's `search_engine.search_image`
//! (ticket 3.h), so it is not here; this module is everything that loop calls: [`engine_waves`],
//! [`planned_engines`], [`fallback_is_unnecessary`], [`no_match`], [`EngineRunner::run_engines_parallel`]
//! (one wave), the `_collect_*` functions, [`retry_could_help`] and [`guess_host`].
//!
//! **The engines are behind [`Engines`].** Python's collectors call each engine module's
//! `search` and read that module's `is_blocked`; here the collectors call the same two things
//! through a trait, so a test scripts the answers an engine gives (as the differential does) and
//! the shipped [`crate::engine_backends::LiveEngines`] hands them to the Phase 3 clients.
//!
//! **Isolation.** Python runs a wave of more than one engine on a thread pool and reports an
//! engine that raised as `"<label>: <exc>"`. Each engine here is a tokio task: an engine that
//! panics is that engine's failure with the panic message, and the others finish. A wave of one
//! engine propagates what Python propagates: [`Raised`].
//!
//! **Google Lens and the Pawchive index** are not ported yet (Phases 5 and 4). The live backend
//! answers for them as Python does when the engine cannot run: Lens as
//! `GoogleLensUnavailableError` (the first image gets the error and the engine latches, later
//! images get "not searched (it stood down earlier this run)"), the Pawchive index as an empty
//! index (`index.is_empty()` is true, so the collector returns False without a file read).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use crate::availability::url_is_structurally_dead;
use crate::clock::{BoxFuture, Clock};
use crate::config::SauceNaoSettings;
use crate::config::{Secret, Settings};
use crate::engine_ids::{self as ids};
use crate::engines::ascii2d::{Ascii2dError, Ascii2dMatch};
use crate::engines::client::OnTick;
use crate::engines::google_images::{GoogleImagesError, GoogleImagesMatch};
use crate::engines::google_lens::GoogleLensMatch;
use crate::engines::iqdb::{IqdbError, IqdbMatch};
use crate::engines::pawchive_lookup::{PawchiveError, PawchiveMatch};
use crate::engines::saucenao::{SauceNaoError, SauceNaoMatch, ShouldStop};
use crate::engines::tracemoe::{TraceMoeError, TraceMoeMatch};
use crate::engines::yandex::{YandexError, YandexMatch};
use crate::model::{Candidate, CandidateTag, Entry, MatchStatus, Tag};
use crate::pycompat;
use crate::pyurl;
use crate::sites::{canonicalize_url, classify_site};
use crate::tag_rules::{apply_namespace_remap, apply_tag_blacklist};

/// `SLOW_SEARCH_THRESHOLD_SECONDS`: an engine call this slow that returns nothing is not trusted
/// as a confident NOT_FOUND.
pub const SLOW_SEARCH_THRESHOLD_SECONDS: f64 = 45.0;

/// An exception that left an engine's collector in Python: not one of the engine's own error
/// types, so the collector did not catch it. In a wave of one engine it propagates to the caller;
/// in a larger wave it becomes `"<label>: <message>"` in the errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised(pub String);

impl std::fmt::Display for Raised {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Raised {}

/// `GoogleLensError` and its two subclasses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensError {
    /// `GoogleLensUnavailableError`: no browser engine can render the results page. Latched.
    Unavailable(String),
    /// `GoogleLensBlockedError`: Google challenged and the check went unanswered.
    Blocked(String),
    /// `GoogleLensError`.
    Failed(String),
    /// An exception Python lets out of `search` that is not a `GoogleLensError`
    /// (`find_parser`'s `ValueError`, a decompression bomb).
    Unexpected(String),
}

/// A match from the local Pawchive index (`pawchive_index.IndexMatch`): the fields the collector
/// reads.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    /// Same SHA-256: the very same file, labelled as the exact engine.
    pub exact: bool,
}

/// What searching the local Pawchive index says.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexAnswer {
    /// `index.is_empty()`: nothing indexed yet, and no file was read for it.
    Empty,
    /// `pawchive_index.fingerprints(path)` gave no hash: the local file could not be read.
    Unreadable,
    /// `index.search(...)`.
    Matches(Vec<IndexMatch>),
}

/// `sqlite3.Error` from the index, as `str(exc)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexError(pub String);

/// The engines the runner calls, and the state it reads from them. Every method is what Python's
/// collector calls on the engine module of the same name. The futures are `Send` so a wave's
/// engines run on separate tasks.
pub trait Engines: Send + Sync + 'static {
    /// `iqdb.search` / `iqdb.search_3d`.
    fn iqdb<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        three_d: bool,
    ) -> BoxFuture<'a, Result<Vec<IqdbMatch>, IqdbError>>;

    /// `saucenao.search`.
    fn saucenao<'a>(
        &'a self,
        path: &'a Path,
        settings: &'a SauceNaoSettings,
        timeout: f64,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, Result<Vec<SauceNaoMatch>, SauceNaoError>>;

    /// `ascii2d.is_blocked`.
    fn ascii2d_is_blocked(&self) -> bool;
    /// `ascii2d.search`.
    fn ascii2d<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<Ascii2dMatch>, Ascii2dError>>;

    /// `tracemoe.search`.
    fn tracemoe<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        min_similarity: f64,
    ) -> BoxFuture<'a, Result<Vec<TraceMoeMatch>, TraceMoeError>>;

    /// `google_images.is_blocked`.
    fn google_images_is_blocked(&self) -> bool;
    /// `google_images.search`.
    fn google_images<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        api_key: &'a Secret,
    ) -> BoxFuture<'a, Result<Vec<GoogleImagesMatch>, GoogleImagesError>>;

    /// `google_lens.is_blocked`.
    fn google_lens_is_blocked(&self) -> bool;
    /// `google_lens.is_resting`.
    fn google_lens_is_resting(&self) -> bool;
    /// `google_lens.resting_until`.
    fn google_lens_resting_until(&self) -> String;
    /// `google_lens.wait_for_turn(on_tick)`.
    fn google_lens_wait_for_turn<'a>(&'a self, on_tick: Option<OnTick>) -> BoxFuture<'a, ()>;
    /// `google_lens.search`.
    fn google_lens<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<Vec<GoogleLensMatch>, LensError>>;

    /// `yandex.is_blocked`.
    fn yandex_is_blocked(&self) -> bool;
    /// `yandex.is_resting`.
    fn yandex_is_resting(&self) -> bool;
    /// `yandex.resting_until`.
    fn yandex_resting_until(&self) -> String;
    /// `yandex.wait_for_turn(on_tick, should_stop)`.
    fn yandex_wait_for_turn<'a>(
        &'a self,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, ()>;
    /// `yandex.search`.
    fn yandex<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<YandexMatch>, YandexError>>;

    /// `pawchive_lookup.file_sha256`; the error is `str(OSError)`.
    fn file_sha256<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String, String>>;
    /// `pawchive_lookup.lookup`.
    fn pawchive_lookup<'a>(
        &'a self,
        sha256: &'a str,
        timeout: f64,
    ) -> BoxFuture<'a, Result<Vec<PawchiveMatch>, PawchiveError>>;

    /// The local index: `is_empty()`, `fingerprints(path)` and `search(hash16, hydrus_hash)`.
    fn pawchive_index<'a>(
        &'a self,
        path: &'a Path,
        hydrus_hash: Option<&'a str>,
    ) -> BoxFuture<'a, Result<IndexAnswer, IndexError>>;
}

// ----------------------------------------------------------------------
// Planning
// ----------------------------------------------------------------------

/// The `'static` id for a name in `ALL_ENGINES`.
fn static_id(name: &str) -> Option<&'static str> {
    ids::ALL_ENGINES.iter().copied().find(|id| *id == name)
}

/// `planned_engines`: every engine the next search could query, in no particular order. Includes
/// the conditional fallback wave, since pacing that waits for a host that turns out not to be
/// queried costs a little time and leaving it out risks a ban.
pub fn planned_engines(
    settings: &Settings,
    override_engine: Option<&str>,
    skip_saucenao: bool,
) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for wave in engine_waves(settings, override_engine, skip_saucenao) {
        for engine in wave {
            if !seen.contains(&engine) {
                seen.push(engine);
            }
        }
    }
    seen
}

/// `_engine_waves`: which engines to run, grouped into parallel waves. A forced engine is a wave of
/// one that ignores the configured order and the toggles. Otherwise wave 1 is the primary plus
/// every enabled extra, with the secondary joining it when the mode is "always"; in "fallback"
/// mode the secondary is wave 2 and runs only if the primary found nothing.
pub fn engine_waves(
    settings: &Settings,
    override_engine: Option<&str>,
    skip_saucenao: bool,
) -> Vec<Vec<&'static str>> {
    if let Some(over) = override_engine.filter(|e| !e.is_empty()) {
        if let Some(id) = static_id(over) {
            return vec![vec![id]];
        }
        warn!("Ignoring unknown override engine {over:?}");
    }

    let primary = static_id(ids::effective_primary(&settings.primary_engine)).unwrap_or(ids::IQDB);
    let secondary = if primary == ids::IQDB {
        ids::SAUCENAO
    } else {
        ids::IQDB
    };
    let mode = match settings.secondary_engine_mode.as_str() {
        m @ ("always" | "fallback" | "disabled") => m,
        _ => "always",
    };

    let mut extras: Vec<&'static str> = Vec::new();
    for (on, id) in [
        (settings.enable_ascii2d, ids::ASCII2D),
        (settings.enable_tracemoe, ids::TRACEMOE),
        (settings.enable_iqdb3d, ids::IQDB3D),
        (settings.enable_google_images, ids::GOOGLE_IMAGES),
        (settings.enable_google_lens, ids::GOOGLE_LENS),
        (settings.enable_yandex, ids::YANDEX),
    ] {
        if on {
            extras.push(id);
        }
    }
    debug!("engine order primary={primary} secondary={secondary} mode={mode} extras={extras:?}");

    // Held back, the extras become the fallback wave instead of running on every image.
    let (deferred, mut upfront) = if settings.extras_only_as_fallback {
        (extras, Vec::new())
    } else {
        (Vec::new(), extras)
    };
    // Pawchive always runs up front, after the primary and never before it: waves[0][0] IS the
    // primary as far as `fallback_is_unnecessary` is concerned. The local index likewise.
    if settings.enable_pawchive_index {
        upfront.insert(0, ids::PAWCHIVE_INDEX);
    }
    if settings.enable_pawchive {
        upfront.insert(0, ids::PAWCHIVE);
    }

    let mut waves: Vec<Vec<&'static str>> = match mode {
        "always" => {
            let mut first = vec![primary, secondary];
            first.extend(&upfront);
            vec![first, deferred]
        }
        "fallback" => {
            let mut first = vec![primary];
            first.extend(&upfront);
            let mut second = vec![secondary];
            second.extend(&deferred);
            vec![first, second]
        }
        _ => {
            let mut first = vec![primary];
            first.extend(&upfront);
            vec![first, deferred]
        }
    };
    waves.retain(|wave| !wave.is_empty());

    if skip_saucenao {
        // Its daily allowance is gone. Dropping it can empty a wave (when SauceNAO IS the
        // primary), and an empty wave must not survive into the plan.
        for wave in &mut waves {
            wave.retain(|e| *e != ids::SAUCENAO);
        }
        waves.retain(|wave| !wave.is_empty());
    }
    waves
}

/// `_fallback_is_unnecessary`: whether the fallback wave can be skipped, and why (for the log).
///
/// With `fallback_below_similarity` set, a result only counts if it is measured (an engine that
/// reports real similarity, or a candidate whose similarity was measured afterwards), on a site
/// the user keeps, and at least that similar. At 0, the primary engine having found anything at
/// all is enough, and deliberately only the primary.
///
/// Panics on an empty `waves` when the threshold is 0, where Python raises `IndexError`; the
/// caller only asks with a fallback wave to skip, so there is always a first wave.
pub fn fallback_is_unnecessary(
    settings: &Settings,
    waves: &[Vec<&'static str>],
    found_by_engine: &HashMap<String, bool>,
    candidates: &[Candidate],
) -> (bool, String) {
    let threshold = settings.fallback_below_similarity;
    if threshold > 0.0 {
        let measured: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| {
                (c.similarity_measured || ids::reports_real_similarity(Some(&c.engine)))
                    && settings
                        .enabled_sites
                        .contains(&classify_site(c.source_name.as_deref(), &c.url))
            })
            .collect();
        let best = measured
            .iter()
            .map(|c| c.similarity)
            .fold(None, |best: Option<f64>, s| {
                Some(best.map_or(s, |b| if s > b { s } else { b }))
            })
            .unwrap_or(0.0);
        if best >= threshold {
            return (
                true,
                format!("best match is {best:.0}%, at or above the {threshold:.0}% threshold"),
            );
        }
        let ignored = candidates.len() - measured.len();
        let note = if ignored > 0 {
            format!(" ({ignored} result(s) from engines that don't measure similarity)")
        } else {
            String::new()
        };
        return (
            false,
            format!("best measured match is {best:.0}%, under the {threshold:.0}% threshold{note}"),
        );
    }

    let primary = waves[0][0];
    if found_by_engine.get(primary).copied().unwrap_or(false) {
        (
            true,
            format!("the primary engine ({primary}) found results"),
        )
    } else {
        (
            false,
            format!("the primary engine ({primary}) found nothing"),
        )
    }
}

/// `_NOT_A_REAL_SEARCH`: engines whose silence says nothing about whether the picture exists.
const NOT_A_REAL_SEARCH: [&str; 2] = [ids::PAWCHIVE, ids::PAWCHIVE_INDEX];

/// `_no_match`: `(status, message, final)` for a search that ended with no candidates.
///
/// Every engine failed: ERROR, retried as before. One answered and others failed: NOT_FOUND with
/// the failures as a note, and not cached (`final` false). No failures: NOT_FOUND, cached.
pub fn no_match(
    errors: &[String],
    answered: &HashSet<String>,
) -> (MatchStatus, Option<String>, bool) {
    if errors.is_empty() {
        return (MatchStatus::NotFound, None, true);
    }
    let message = errors.join("; ");
    if answered
        .iter()
        .any(|engine| !NOT_A_REAL_SEARCH.contains(&engine.as_str()))
    {
        return (MatchStatus::NotFound, Some(message), false);
    }
    (MatchStatus::Error, Some(message), true)
}

// ----------------------------------------------------------------------
// Running
// ----------------------------------------------------------------------

/// What a task needs of the entry; owned so a task can outlive the borrow.
#[derive(Clone)]
struct Job {
    path: PathBuf,
    filename: String,
    hydrus_hash: Option<String>,
}

impl Job {
    fn of(entry: &Entry) -> Self {
        Job {
            path: PathBuf::from(&entry.path),
            filename: entry.filename().to_owned(),
            hydrus_hash: entry.hydrus_hash.clone(),
        }
    }
}

/// What one engine's run gave: whether it found anything, and what to merge.
type EngineRun = (bool, Vec<Candidate>, Vec<String>);

/// `on_engine_finished`: called with the engine id once its request is done, however it ended.
pub type OnEngineFinished<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// The runner: the engines it asks and the clock it times them on.
#[derive(Clone)]
pub struct EngineRunner {
    engines: Arc<dyn Engines>,
    clock: Arc<dyn Clock>,
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(text) => *text,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(text) => (*text).to_owned(),
            Err(_) => "the engine task panicked".to_owned(),
        },
    }
}

fn raised_of(join: tokio::task::JoinError) -> Raised {
    if join.is_panic() {
        Raised(panic_message(join.into_panic()))
    } else {
        Raised(join.to_string())
    }
}

impl EngineRunner {
    pub fn new(engines: Arc<dyn Engines>, clock: Arc<dyn Clock>) -> Self {
        EngineRunner { engines, clock }
    }

    /// `_run_engines_parallel`: runs one wave, concurrently when there is more than one engine.
    /// Each engine appends to its own lists and the results are merged as the engines finish, so
    /// two tasks never touch `candidates` at once. Returns `{engine_id: found_any}` and adds each
    /// engine that finished without an error to `answered`.
    ///
    /// In a wave of one engine an exception propagates ([`Raised`]), after `on_engine_finished`
    /// has been told (Python's `finally`).
    #[allow(clippy::too_many_arguments)]
    pub async fn run_engines_parallel(
        &self,
        engine_names: &[&str],
        entry: &Entry,
        settings: &Arc<Settings>,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
        on_engine_finished: Option<OnEngineFinished<'_>>,
        should_stop: Option<ShouldStop>,
        mut answered: Option<&mut HashSet<String>>,
    ) -> Result<HashMap<String, bool>, Raised> {
        let mut found_by_name: HashMap<String, bool> = HashMap::new();
        if engine_names.is_empty() {
            return Ok(found_by_name);
        }
        let job = Job::of(entry);

        if engine_names.len() == 1 {
            let name = engine_names[0];
            let run = self.spawn_engine(name, &job, settings, on_tick, should_stop);
            let outcome = run.await.map_err(raised_of);
            // In the `finally` so a failed request still counts as having been made.
            if let Some(finished) = on_engine_finished {
                finished(name);
            }
            let (found, extra_candidates, extra_errors) = outcome.and_then(|inner| inner)?;
            let clean = extra_errors.is_empty();
            candidates.extend(extra_candidates);
            errors.extend(extra_errors);
            if clean {
                if let Some(answered) = answered.as_deref_mut() {
                    answered.insert(name.to_owned());
                }
            }
            found_by_name.insert(name.to_owned(), found);
            return Ok(found_by_name);
        }

        let mut set: JoinSet<Result<EngineRun, Raised>> = JoinSet::new();
        let mut names: HashMap<tokio::task::Id, String> = HashMap::new();
        for name in engine_names {
            let handle = set.spawn(self.run_isolated(name, &job, settings, &on_tick, &should_stop));
            names.insert(handle.id(), (*name).to_owned());
        }
        while let Some(joined) = set.join_next_with_id().await {
            let (name, outcome) = match joined {
                Ok((id, outcome)) => (names.remove(&id).unwrap_or_default(), outcome),
                Err(join) => (
                    names.remove(&join.id()).unwrap_or_default(),
                    Err(raised_of(join)),
                ),
            };
            if let Some(finished) = on_engine_finished {
                finished(&name);
            }
            match outcome {
                Err(Raised(message)) => {
                    error!("Engine {name} raised: {message}");
                    errors.push(format!("{}: {message}", ids::label_for(&name)));
                    found_by_name.insert(name, false);
                }
                Ok((found, extra_candidates, extra_errors)) => {
                    let clean = extra_errors.is_empty();
                    candidates.extend(extra_candidates);
                    errors.extend(extra_errors);
                    found_by_name.insert(name.clone(), found);
                    if clean {
                        if let Some(answered) = answered.as_deref_mut() {
                            answered.insert(name);
                        }
                    }
                }
            }
        }
        Ok(found_by_name)
    }

    /// One engine on its own task, panics included.
    fn spawn_engine(
        &self,
        name: &str,
        job: &Job,
        settings: &Arc<Settings>,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> tokio::task::JoinHandle<Result<EngineRun, Raised>> {
        tokio::spawn(self.run_isolated(name, job, settings, &on_tick, &should_stop))
    }

    /// `_run_engine_isolated`: runs one engine into lists of its own.
    fn run_isolated(
        &self,
        name: &str,
        job: &Job,
        settings: &Arc<Settings>,
        on_tick: &Option<OnTick>,
        should_stop: &Option<ShouldStop>,
    ) -> impl std::future::Future<Output = Result<EngineRun, Raised>> + Send + 'static {
        let runner = self.clone();
        let name = name.to_owned();
        let job = job.clone();
        let settings = Arc::clone(settings);
        let on_tick = on_tick.clone();
        let should_stop = should_stop.clone();
        async move {
            let mut local_candidates: Vec<Candidate> = Vec::new();
            let mut local_errors: Vec<String> = Vec::new();
            let found = runner
                .run_engine(
                    &name,
                    &job,
                    &settings,
                    &mut local_candidates,
                    &mut local_errors,
                    on_tick,
                    should_stop,
                )
                .await?;
            Ok((found, local_candidates, local_errors))
        }
    }

    /// `_run_engine`: runs one engine, appending its results to `candidates` and `errors`. True if
    /// it found at least one usable result, which is what decides whether a fallback wave runs.
    #[allow(clippy::too_many_arguments)]
    async fn run_engine(
        &self,
        engine: &str,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> Result<bool, Raised> {
        match engine {
            ids::IQDB => Ok(self
                .collect_iqdb(job, settings, candidates, errors, on_tick, false)
                .await),
            ids::IQDB3D => Ok(self
                .collect_iqdb(job, settings, candidates, errors, on_tick, true)
                .await),
            ids::SAUCENAO => Ok(self
                .collect_saucenao(job, settings, candidates, errors, on_tick, should_stop)
                .await),
            ids::ASCII2D => Ok(self
                .collect_ascii2d(job, settings, candidates, errors, on_tick)
                .await),
            ids::TRACEMOE => Ok(self
                .collect_tracemoe(job, settings, candidates, errors, on_tick)
                .await),
            ids::GOOGLE_IMAGES => {
                self.collect_google_images(job, settings, candidates, errors, on_tick)
                    .await
            }
            ids::GOOGLE_LENS => {
                self.collect_google_lens(job, settings, candidates, errors, on_tick)
                    .await
            }
            ids::YANDEX => {
                self.collect_yandex(job, settings, candidates, errors, on_tick, should_stop)
                    .await
            }
            ids::PAWCHIVE => {
                self.collect_pawchive(job, settings, candidates, errors)
                    .await
            }
            ids::PAWCHIVE_INDEX => Ok(self.collect_pawchive_index(job, candidates, errors).await),
            other => {
                warn!("Unknown search engine {other:?}");
                Ok(false)
            }
        }
    }

    fn elapsed_since(&self, start: f64) -> f64 {
        self.clock.monotonic() - start
    }

    /// `_collect_iqdb`.
    async fn collect_iqdb(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
        three_d: bool,
    ) -> bool {
        let label = ids::label_for(if three_d { ids::IQDB3D } else { ids::IQDB });
        let start = self.clock.monotonic();
        match self
            .engines
            .iqdb(&job.path, settings.search_timeout, on_tick, three_d)
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "{label} returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                if results.is_empty() && elapsed >= SLOW_SEARCH_THRESHOLD_SECONDS {
                    warn!(
                        "{}: {label} took {elapsed:.0}s and returned zero results - too slow to trust as a confident NOT_FOUND, won't be cached",
                        job.filename
                    );
                    errors.push(format!(
                        "{label} took {elapsed:.0}s and found nothing (too slow to trust)"
                    ));
                }
                let found = !results.is_empty();
                for m in results {
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        label,
                        apply_tag_blacklist(&m.unnamespaced_tags, settings),
                        m.width,
                        m.height,
                    );
                }
                found
            }
            Err(exc) => {
                error!("{label} search failed for {}: {exc}", job.filename);
                errors.push(format!("{label}: {exc}"));
                false
            }
        }
    }

    /// `_collect_saucenao`.
    async fn collect_saucenao(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> bool {
        let start = self.clock.monotonic();
        match self
            .engines
            .saucenao(
                &job.path,
                &settings.saucenao,
                settings.search_timeout,
                on_tick,
                should_stop,
            )
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "SauceNAO returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let mut found_any = false;
                for m in results {
                    if m.similarity < settings.saucenao.min_similarity {
                        continue;
                    }
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::SAUCENAO),
                        apply_tag_blacklist(&apply_namespace_remap(&m.tags, settings), settings),
                        None,
                        None,
                    );
                    found_any = true;
                }
                if !found_any && elapsed >= SLOW_SEARCH_THRESHOLD_SECONDS {
                    warn!(
                        "{}: SauceNAO took {elapsed:.0}s and returned zero usable results - too slow to trust as a confident NOT_FOUND, won't be cached",
                        job.filename
                    );
                    errors.push(format!(
                        "SauceNAO took {elapsed:.0}s and found nothing (too slow to trust)"
                    ));
                }
                found_any
            }
            Err(exc) => {
                error!("SauceNAO search failed for {}: {exc}", job.filename);
                errors.push(format!("SauceNAO: {exc}"));
                false
            }
        }
    }

    /// `_collect_ascii2d`.
    async fn collect_ascii2d(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
    ) -> bool {
        if self.engines.ascii2d_is_blocked() {
            // Already established that ascii2d is challenging every upload; asking again costs a
            // request per image to be refused again.
            debug!(
                "Skipping ascii2d for {} - it is refusing uploads this session",
                job.filename
            );
            return false;
        }
        let start = self.clock.monotonic();
        match self
            .engines
            .ascii2d(&job.path, settings.search_timeout, on_tick)
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "ascii2d returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let found = !results.is_empty();
                for m in results {
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::ASCII2D),
                        Vec::new(),
                        m.width,
                        m.height,
                    );
                }
                found
            }
            Err(exc @ Ascii2dError::Blocked) => {
                // Reported once, against the image that discovered it.
                error!("ascii2d is blocking this app: {exc}");
                errors.push(format!("ascii2d: {exc}"));
                false
            }
            Err(exc @ Ascii2dError::Failed(_)) => {
                error!("ascii2d search failed for {}: {exc}", job.filename);
                errors.push(format!("ascii2d: {exc}"));
                false
            }
        }
    }

    /// `_collect_tracemoe`.
    async fn collect_tracemoe(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
    ) -> bool {
        let start = self.clock.monotonic();
        match self
            .engines
            .tracemoe(
                &job.path,
                settings.search_timeout,
                on_tick,
                settings.tracemoe_min_similarity,
            )
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "trace.moe returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let found = !results.is_empty();
                for m in results {
                    append_candidate(
                        candidates,
                        &m.url,
                        m.title.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::TRACEMOE),
                        apply_tag_blacklist(&apply_namespace_remap(&m.tags, settings), settings),
                        None,
                        None,
                    );
                }
                found
            }
            Err(exc) => {
                error!("trace.moe search failed for {}: {exc}", job.filename);
                errors.push(format!("trace.moe: {exc}"));
                false
            }
        }
    }

    /// `_collect_google_images`.
    async fn collect_google_images(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
    ) -> Result<bool, Raised> {
        if self.engines.google_images_is_blocked() {
            // Not asked again, but a short note still goes on the row: an engine that was enabled
            // and never queried must not leave the row looking like a confident "found nothing".
            debug!(
                "Skipping Google Images for {} - it is refusing this app this session",
                job.filename
            );
            errors.push("Google Images: not searched (it stood down earlier this run)".to_owned());
            return Ok(false);
        }
        let start = self.clock.monotonic();
        match self
            .engines
            .google_images(
                &job.path,
                settings.search_timeout,
                on_tick,
                &settings.google_images_api_key,
            )
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "Google Images returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let found = !results.is_empty();
                for m in results {
                    // No tags: Google indexes pages, not a tag vocabulary.
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::GOOGLE_IMAGES),
                        Vec::new(),
                        None,
                        None,
                    );
                }
                Ok(found)
            }
            Err(unexpected @ GoogleImagesError::Unexpected(_)) => {
                Err(Raised(unexpected.to_string()))
            }
            Err(exc @ (GoogleImagesError::Blocked(_) | GoogleImagesError::JavaScript)) => {
                error!("Google is blocking this app: {exc}");
                errors.push(format!("Google Images: {exc}"));
                Ok(false)
            }
            Err(exc @ GoogleImagesError::Failed(_)) => {
                error!("Google Images search failed for {}: {exc}", job.filename);
                errors.push(format!("Google Images: {exc}"));
                Ok(false)
            }
        }
    }

    /// `_collect_pawchive`: exact-copy lookup on pawchive.pw.
    async fn collect_pawchive(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
    ) -> Result<bool, Raised> {
        let sha256 = match job.hydrus_hash.as_deref().filter(|h| !h.is_empty()) {
            Some(known) => known.to_owned(),
            None => {
                // Normally already known: the hash worker fills it in when the file is added.
                // Reading the file here is the fallback, not the rule.
                match self.engines.file_sha256(&job.path).await {
                    Ok(hash) => hash,
                    Err(exc) => {
                        errors.push(format!(
                            "Pawchive: could not read the file to hash it ({exc})"
                        ));
                        return Ok(false);
                    }
                }
            }
        };
        let matches = match self
            .engines
            .pawchive_lookup(&sha256, settings.search_timeout)
            .await
        {
            Ok(matches) => matches,
            Err(PawchiveError::Lookup(exc)) => {
                warn!("Pawchive lookup failed for {}: {exc}", job.filename);
                errors.push(format!("Pawchive: {exc}"));
                return Ok(false);
            }
            // `lookup` does not catch `HardTimeoutError`, and nor does the collector.
            Err(PawchiveError::HardTimeout(exc)) => return Err(Raised(exc.to_string())),
        };
        if matches.is_empty() {
            debug!("{}: not on pawchive.pw", job.filename);
        } else {
            info!(
                "{}: an exact copy is on pawchive.pw ({})",
                job.filename,
                matches
                    .iter()
                    .map(|m| m.url.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let label = ids::label_for(ids::PAWCHIVE);
        let found = !matches.is_empty();
        for m in matches {
            // 100 and meant: the lookup is by the file's own SHA-256, so every result holds these
            // exact bytes.
            append_candidate(
                candidates,
                &m.url,
                Some("Pawchive"),
                m.thumb_url,
                100.0,
                label,
                Vec::new(),
                None,
                None,
            );
        }
        Ok(found)
    }

    /// `_collect_pawchive_index`: resized or re-saved copies of indexed artists' images. Local: no
    /// request is made.
    async fn collect_pawchive_index(
        &self,
        job: &Job,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
    ) -> bool {
        let answer = match self
            .engines
            .pawchive_index(&job.path, job.hydrus_hash.as_deref())
            .await
        {
            Ok(IndexAnswer::Empty) => return false, // nothing indexed yet - and no file read for it
            Ok(IndexAnswer::Unreadable) => {
                errors.push("Pawchive index: could not read the local file".to_owned());
                return false;
            }
            Ok(IndexAnswer::Matches(matches)) => matches,
            Err(IndexError(exc)) => {
                warn!("Pawchive index could not be searched: {exc}");
                errors.push(format!("Pawchive index: {exc}"));
                return false;
            }
        };
        for m in &answer {
            // Same SHA-256 means the very same file - labelled as the exact engine, which is what
            // lets it be sent as the original.
            let label = ids::label_for(if m.exact {
                ids::PAWCHIVE
            } else {
                ids::PAWCHIVE_INDEX
            });
            append_candidate(
                candidates,
                &m.url,
                Some("Pawchive"),
                m.thumb_url.clone(),
                m.similarity,
                label,
                Vec::new(),
                None,
                None,
            );
        }
        if !answer.is_empty() {
            info!(
                "{}: the pawchive index has {}",
                job.filename,
                answer
                    .iter()
                    .map(|m| format!(
                        "{} ({:.0}%{})",
                        m.url,
                        m.similarity,
                        if m.exact { ", exact" } else { "" }
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        !answer.is_empty()
    }

    /// `_collect_google_lens`.
    async fn collect_google_lens(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
    ) -> Result<bool, Raised> {
        if self.engines.google_lens_is_blocked() {
            // Google challenged and the check went unanswered, or there is no browser engine to
            // render with. The row still says so, or an image whose other engines also found
            // nothing would be recorded and cached as a confident NOT_FOUND.
            let why = if self.engines.google_lens_is_resting() {
                format!(
                    "resting after an unanswered robot check, until {}",
                    self.engines.google_lens_resting_until()
                )
            } else {
                "it stood down earlier this run".to_owned()
            };
            debug!("Skipping Google Lens for {} - {why}", job.filename);
            errors.push(format!("Google Lens: not searched ({why})"));
            return Ok(false);
        }

        self.engines
            .google_lens_wait_for_turn(on_tick.clone())
            .await;
        let start = self.clock.monotonic();
        match self
            .engines
            .google_lens(
                &job.path,
                settings.search_timeout,
                on_tick,
                settings.lens_search_whole_image,
            )
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "Google Lens returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let found = !results.is_empty();
                for m in results {
                    // No tags, for the same reason as Google Images.
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::GOOGLE_LENS),
                        Vec::new(),
                        m.width,
                        m.height,
                    );
                }
                Ok(found)
            }
            Err(LensError::Unavailable(exc)) => {
                error!("Google Lens cannot run: {exc}");
                errors.push(format!("Google Lens: {exc}"));
                Ok(false)
            }
            Err(LensError::Blocked(exc)) => {
                error!("Google Lens was challenged and stood down: {exc}");
                errors.push(format!("Google Lens: {exc}"));
                Ok(false)
            }
            Err(LensError::Failed(exc)) => {
                error!("Google Lens search failed for {}: {exc}", job.filename);
                errors.push(format!("Google Lens: {exc}"));
                Ok(false)
            }
            // Not an error of the engine's own: Python's `except GoogleLensError` does not catch
            // it, so it leaves the collector.
            Err(LensError::Unexpected(exc)) => Err(Raised(exc)),
        }
    }

    /// `_collect_yandex`.
    async fn collect_yandex(
        &self,
        job: &Job,
        settings: &Settings,
        candidates: &mut Vec<Candidate>,
        errors: &mut Vec<String>,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> Result<bool, Raised> {
        if self.engines.yandex_is_blocked() {
            // Same reasoning as Google Lens: not asked again this run, but the row says so.
            let why = if self.engines.yandex_is_resting() {
                format!(
                    "resting after a captcha, until {}",
                    self.engines.yandex_resting_until()
                )
            } else {
                "it stood down earlier this run".to_owned()
            };
            debug!("Skipping Yandex for {} - {why}", job.filename);
            errors.push(format!("Yandex: not searched ({why})"));
            return Ok(false);
        }

        self.engines
            .yandex_wait_for_turn(on_tick.clone(), should_stop)
            .await;
        let start = self.clock.monotonic();
        match self
            .engines
            .yandex(&job.path, settings.search_timeout, on_tick)
            .await
        {
            Ok(results) => {
                let elapsed = self.elapsed_since(start);
                debug!(
                    "Yandex returned {} result(s) for {} ({elapsed:.1}s)",
                    results.len(),
                    job.filename
                );
                let found = !results.is_empty();
                for m in results {
                    // No tags: only post pages boorus/ can parse are kept.
                    append_candidate(
                        candidates,
                        &m.url,
                        m.source_name.as_deref(),
                        m.thumb_url,
                        m.similarity,
                        ids::label_for(ids::YANDEX),
                        Vec::new(),
                        m.width,
                        m.height,
                    );
                }
                Ok(found)
            }
            Err(unexpected @ YandexError::Unexpected(_)) => Err(Raised(unexpected.to_string())),
            Err(exc @ YandexError::Blocked(_)) => {
                error!("Yandex stood down: {exc}");
                errors.push(format!("Yandex: {exc}"));
                Ok(false)
            }
            Err(exc @ YandexError::Failed(_)) => {
                error!("Yandex search failed for {}: {exc}", job.filename);
                errors.push(format!("Yandex: {exc}"));
                Ok(false)
            }
        }
    }

    /// `retry_could_help`: whether a second attempt at a failed image could turn out different.
    ///
    /// Not when every failure came from an engine that has stood down for this run: the retry
    /// would skip that engine and fail again, having spent the pacing delay to report "not
    /// searched". Anything that cannot be attributed to such an engine counts as worth retrying.
    pub fn retry_could_help(&self, error_message: Option<&str>) -> bool {
        let parts: Vec<&str> = error_message
            .unwrap_or("")
            .split(';')
            .map(pycompat::strip)
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return true;
        }
        for part in parts {
            let stood_down = (part.starts_with("Google Lens:")
                && self.engines.google_lens_is_blocked())
                || (part.starts_with("Google Images:") && self.engines.google_images_is_blocked())
                || (part.starts_with("ascii2d:") && self.engines.ascii2d_is_blocked())
                || (part.starts_with("Yandex:") && self.engines.yandex_is_blocked());
            if !stood_down {
                return true;
            }
        }
        false
    }
}

/// `_append_candidate`. A URL that can no longer resolve never becomes a candidate at all.
#[allow(clippy::too_many_arguments)]
pub fn append_candidate(
    candidates: &mut Vec<Candidate>,
    url: &str,
    source_name: Option<&str>,
    thumb_url: Option<String>,
    similarity: f64,
    engine_label: &str,
    engine_tags: Vec<Tag>,
    width: Option<i64>,
    height: Option<i64>,
) {
    let canonical = canonicalize_url(url);
    if url_is_structurally_dead(&canonical) {
        // No request can reach it, so offering it would only waste the user's click.
        debug!("Skipping {canonical} - its URL form no longer resolves");
        return;
    }
    let source = classify_site(source_name, &canonical);
    let mut candidate = Candidate::new(canonical);
    // `classify_site(...) or _guess_host(...)`: classify_site always answers (its last word is
    // "Other"), so the guess is only reached for an answer that is empty.
    candidate.source_name = if source.is_empty() {
        guess_host(&candidate.url)
    } else {
        Some(source)
    };
    candidate.thumb_url = thumb_url;
    candidate.similarity = similarity;
    candidate.width = width;
    candidate.height = height;
    candidate.engine = engine_label.to_owned();
    candidate.engine_tags = engine_tags
        .into_iter()
        .map(|t| CandidateTag {
            name: t.name,
            namespace: t.namespace,
        })
        .collect();
    candidates.push(candidate);
}

/// `_guess_host`: the URL's `netloc`, or `None` when it has none or the URL does not parse.
pub fn guess_host(url: &str) -> Option<String> {
    match pyurl::try_urlparse(url, "") {
        Ok(parsed) if !parsed.netloc.is_empty() => Some(parsed.netloc),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
