//! The runner's own tests, over scripted engines. The Python tests they stand for are listed in
//! the PR body (the mapping table); each section names the Python tests it covers.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::clock::ManualClock;
use crate::config::OrderedMap;
use crate::engines::UnexpectedInput;
use crate::engines::pawchive_lookup::PawchiveLookupError;
use crate::hard_timeout::HardTimeoutError;
use crate::model::TagSource;

// ---- scripted engines ----------------------------------------------------------------------

/// One scripted answer: what to return, how much clock to spend first, or a panic.
enum Step<T> {
    Value(T),
    After(f64, T),
    Panic(&'static str),
}

struct Queue<T>(Mutex<VecDeque<Step<T>>>);

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Queue(Mutex::new(VecDeque::new()))
    }
}

impl<T> Queue<T> {
    fn push(&self, step: Step<T>) {
        self.0.lock().unwrap().push_back(step);
    }

    fn take(&self, clock: &ManualClock, empty: impl FnOnce() -> T) -> T {
        let step = self.0.lock().unwrap().pop_front();
        match step {
            None => empty(),
            Some(Step::Value(v)) => v,
            Some(Step::After(secs, v)) => {
                clock.advance(secs);
                v
            }
            Some(Step::Panic(message)) => panic!("{message}"),
        }
    }
}

struct SharedClock(Arc<ManualClock>);

impl Default for SharedClock {
    fn default() -> Self {
        SharedClock(Arc::new(ManualClock::new(1_000.0, 0.0)))
    }
}

#[derive(Default)]
struct Scripted {
    clock: SharedClock,
    iqdb: Queue<Result<Vec<IqdbMatch>, IqdbError>>,
    saucenao: Queue<Result<Vec<SauceNaoMatch>, SauceNaoError>>,
    ascii2d: Queue<Result<Vec<Ascii2dMatch>, Ascii2dError>>,
    tracemoe: Queue<Result<Vec<TraceMoeMatch>, TraceMoeError>>,
    yandex: Queue<Result<Vec<YandexMatch>, YandexError>>,
    google_images: Queue<Result<Vec<GoogleImagesMatch>, GoogleImagesError>>,
    pawchive: Queue<Result<Vec<PawchiveMatch>, PawchiveError>>,
    index: Queue<Result<IndexAnswer, IndexError>>,
    sha: Queue<Result<String, String>>,
    asked: Mutex<Vec<String>>,
    ascii2d_blocked: AtomicBool,
    google_images_blocked: AtomicBool,
    lens_blocked: AtomicBool,
    lens_resting: AtomicBool,
    yandex_blocked: AtomicBool,
    yandex_resting: AtomicBool,
}

impl Scripted {
    fn new() -> Arc<Self> {
        Arc::new(Scripted::default())
    }

    fn note(&self, what: &str) {
        self.asked.lock().unwrap().push(what.to_owned());
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    fn runner(self: &Arc<Self>) -> EngineRunner {
        EngineRunner::new(self.clone(), self.clock.0.clone())
    }
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(std::future::ready(value))
}

impl Engines for Scripted {
    fn iqdb<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        three_d: bool,
    ) -> BoxFuture<'a, Result<Vec<IqdbMatch>, IqdbError>> {
        self.note(if three_d { "iqdb3d" } else { "iqdb" });
        ready(self.iqdb.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn saucenao<'a>(
        &'a self,
        _path: &'a Path,
        _settings: &'a SauceNaoSettings,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, Result<Vec<SauceNaoMatch>, SauceNaoError>> {
        self.note("saucenao");
        ready(self.saucenao.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn ascii2d_is_blocked(&self) -> bool {
        self.ascii2d_blocked.load(Ordering::SeqCst)
    }

    fn ascii2d<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<Ascii2dMatch>, Ascii2dError>> {
        self.note("ascii2d");
        ready(self.ascii2d.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn tracemoe<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _min_similarity: f64,
    ) -> BoxFuture<'a, Result<Vec<TraceMoeMatch>, TraceMoeError>> {
        self.note("tracemoe");
        ready(self.tracemoe.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn google_images_is_blocked(&self) -> bool {
        self.google_images_blocked.load(Ordering::SeqCst)
    }

    fn google_images<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _api_key: &'a Secret,
    ) -> BoxFuture<'a, Result<Vec<GoogleImagesMatch>, GoogleImagesError>> {
        self.note("googleimages");
        ready(self.google_images.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn google_lens_is_blocked(&self) -> bool {
        self.lens_blocked.load(Ordering::SeqCst)
    }

    fn google_lens_is_resting(&self) -> bool {
        self.lens_resting.load(Ordering::SeqCst)
    }

    fn google_lens_resting_until(&self) -> String {
        "21:41".to_owned()
    }

    fn google_lens_wait_for_turn<'a>(&'a self, _on_tick: Option<OnTick>) -> BoxFuture<'a, ()> {
        self.note("lens-wait");
        ready(())
    }

    fn google_lens<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _whole_image: bool,
    ) -> BoxFuture<'a, Result<Vec<GoogleLensMatch>, LensError>> {
        self.note("googlelens");
        ready(Err(LensError::Unavailable("no browser".to_owned())))
    }

    fn yandex_is_blocked(&self) -> bool {
        self.yandex_blocked.load(Ordering::SeqCst)
    }

    fn yandex_is_resting(&self) -> bool {
        self.yandex_resting.load(Ordering::SeqCst)
    }

    fn yandex_resting_until(&self) -> String {
        "09:05".to_owned()
    }

    fn yandex_wait_for_turn<'a>(
        &'a self,
        _on_tick: Option<OnTick>,
        _should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, ()> {
        self.note("yandex-wait");
        ready(())
    }

    fn yandex<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<YandexMatch>, YandexError>> {
        self.note("yandex");
        ready(self.yandex.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn file_sha256<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<String, String>> {
        self.note("sha256");
        ready(self.sha.take(&self.clock.0, || Ok("ab".repeat(32))))
    }

    fn pawchive_lookup<'a>(
        &'a self,
        sha256: &'a str,
        _timeout: f64,
    ) -> BoxFuture<'a, Result<Vec<PawchiveMatch>, PawchiveError>> {
        self.note(&format!("pawchive:{sha256}"));
        ready(self.pawchive.take(&self.clock.0, || Ok(Vec::new())))
    }

    fn pawchive_index<'a>(
        &'a self,
        _path: &'a Path,
        _hydrus_hash: Option<&'a str>,
    ) -> BoxFuture<'a, Result<IndexAnswer, IndexError>> {
        self.note("pawchiveindex");
        ready(self.index.take(&self.clock.0, || Ok(IndexAnswer::Empty)))
    }
}

// ---- builders ------------------------------------------------------------------------------

fn settings() -> Settings {
    let paths =
        crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| "/home/test".into())).unwrap();
    let mut s = Settings::with_defaults(&paths);
    // Everything optional off; each test turns on what it needs.
    s.enable_ascii2d = false;
    s.enable_tracemoe = false;
    s.enable_iqdb3d = false;
    s.enable_google_images = false;
    s.enable_google_lens = false;
    s.enable_yandex = false;
    s.enable_pawchive = false;
    s.enable_pawchive_index = false;
    s.primary_engine = "iqdb".to_owned();
    s.secondary_engine_mode = "always".to_owned();
    s.extras_only_as_fallback = false;
    s
}

fn engine_tag(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        namespace: None,
        source: TagSource::SearchEngine,
    }
}

fn iqdb_match(url: &str, similarity: f64) -> IqdbMatch {
    IqdbMatch {
        url: url.to_owned(),
        thumb_url: Some("https://t.invalid/x.jpg".to_owned()),
        similarity,
        width: Some(100),
        height: Some(200),
        source_name: Some("Danbooru".to_owned()),
        is_best_match: true,
        unnamespaced_tags: vec![engine_tag("kept"), engine_tag("highres")],
    }
}

fn sauce_match(url: &str, similarity: f64, tags: Vec<Tag>) -> SauceNaoMatch {
    SauceNaoMatch {
        url: url.to_owned(),
        similarity,
        title: None,
        source_name: Some("Danbooru".to_owned()),
        thumb_url: None,
        tags,
    }
}

fn entry() -> Entry {
    Entry::new("/pics/a.png")
}

type Wave = Result<HashMap<String, bool>, Raised>;

struct WaveRun {
    found: Wave,
    candidates: Vec<Candidate>,
    errors: Vec<String>,
    answered: HashSet<String>,
    finished: Vec<String>,
}

/// Runs one wave against `scripted`, returning everything the runner reports.
async fn run_wave_for(
    scripted: &Arc<Scripted>,
    wave: &[&str],
    settings: Settings,
    entry: &Entry,
) -> WaveRun {
    let finished: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut answered = HashSet::new();
    let notify = |name: &str| finished.lock().unwrap().push(name.to_owned());
    let found = scripted
        .runner()
        .run_engines_parallel(
            wave,
            entry,
            &Arc::new(settings),
            &mut candidates,
            &mut errors,
            None,
            Some(&notify),
            None,
            Some(&mut answered),
        )
        .await;
    WaveRun {
        found,
        candidates,
        errors,
        answered,
        finished: finished.into_inner().unwrap(),
    }
}

async fn run_wave(scripted: &Arc<Scripted>, wave: &[&str], settings: Settings) -> WaveRun {
    run_wave_for(scripted, wave, settings, &entry()).await
}

// ---- waves (test_search_quality.py::TestEngineWaves, test_core_utils.py::TestContinuingWithoutSauceNao,
// ---- test_pawchive_index.py::TestTheIndexAsASearchEngine, test_yandex.py::TestTakingPart,
// ---- test_search_quality.py::TestPawchiveExactLookup waves tests) -------------------------

#[test]
fn always_mode_runs_both_primary_engines_in_one_wave() {
    let s = settings();
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["iqdb", "saucenao"]]
    );
    let mut s = settings();
    s.primary_engine = "saucenao".to_owned();
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["saucenao", "iqdb"]]
    );
}

#[test]
fn fallback_mode_puts_the_secondary_in_a_second_wave() {
    let mut s = settings();
    s.secondary_engine_mode = "fallback".to_owned();
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["iqdb"], vec!["saucenao"]]
    );
    s.secondary_engine_mode = "disabled".to_owned();
    assert_eq!(engine_waves(&s, None, false), vec![vec!["iqdb"]]);
    // An unknown mode is "always".
    s.secondary_engine_mode = "nonsense".to_owned();
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["iqdb", "saucenao"]]
    );
    // An unknown primary is IQDB.
    s.primary_engine = "ascii2d".to_owned();
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["iqdb", "saucenao"]]
    );
}

#[test]
fn extras_join_wave_one_or_are_held_back() {
    let mut s = settings();
    s.enable_ascii2d = true;
    s.enable_tracemoe = true;
    s.enable_iqdb3d = true;
    s.enable_google_images = true;
    s.enable_google_lens = true;
    s.enable_yandex = true;
    let extras = [
        "ascii2d",
        "tracemoe",
        "iqdb3d",
        "googleimages",
        "googlelens",
        "yandex",
    ];
    let mut all = vec!["iqdb", "saucenao"];
    all.extend(extras);
    assert_eq!(engine_waves(&s, None, false), vec![all]);
    s.extras_only_as_fallback = true;
    assert_eq!(
        engine_waves(&s, None, false),
        vec![vec!["iqdb", "saucenao"], extras.to_vec()]
    );
    // Held-back extras join the secondary in fallback mode.
    s.secondary_engine_mode = "fallback".to_owned();
    let mut second = vec!["saucenao"];
    second.extend(extras);
    assert_eq!(engine_waves(&s, None, false)[1], second);
    // A held-back engine is still planned, for pacing.
    assert!(planned_engines(&s, None, false).contains(&"yandex"));
    // With none enabled, holding back leaves one wave.
    let mut none = settings();
    none.extras_only_as_fallback = true;
    assert_eq!(
        engine_waves(&none, None, false),
        vec![vec!["iqdb", "saucenao"]]
    );
}

#[test]
fn pawchive_runs_up_front_after_the_primary_even_with_extras_held_back() {
    let mut s = settings();
    s.enable_pawchive = true;
    s.enable_pawchive_index = true;
    s.enable_ascii2d = true;
    s.extras_only_as_fallback = true;
    assert_eq!(
        engine_waves(&s, None, false),
        vec![
            vec!["iqdb", "saucenao", "pawchive", "pawchiveindex"],
            vec!["ascii2d"]
        ]
    );
    s.enable_pawchive = false;
    s.enable_pawchive_index = false;
    assert_eq!(engine_waves(&s, None, false)[0], vec!["iqdb", "saucenao"]);
}

#[test]
fn an_override_is_one_engine_and_an_unknown_one_is_ignored() {
    let mut s = settings();
    s.enable_ascii2d = true;
    assert_eq!(
        engine_waves(&s, Some("googlelens"), false),
        vec![vec!["googlelens"]]
    );
    assert_eq!(
        engine_waves(&s, Some("pawchive"), true),
        vec![vec!["pawchive"]]
    );
    assert_eq!(
        engine_waves(&s, Some("nonsense"), false),
        vec![vec!["iqdb", "saucenao", "ascii2d"]]
    );
    assert_eq!(
        engine_waves(&s, Some(""), false),
        vec![vec!["iqdb", "saucenao", "ascii2d"]]
    );
    // An override beats skip_saucenao: it returns before the filter, as Python does.
    assert_eq!(
        engine_waves(&s, Some("saucenao"), true),
        vec![vec!["saucenao"]]
    );
}

#[test]
fn skipping_saucenao_removes_it_and_never_leaves_an_empty_wave() {
    let mut s = settings();
    s.enable_ascii2d = true;
    assert_eq!(engine_waves(&s, None, true), vec![vec!["iqdb", "ascii2d"]]);
    assert_eq!(planned_engines(&s, None, true), vec!["iqdb", "ascii2d"]);
    // SauceNAO as the primary, secondary held for fallback: its wave keeps IQDB and the extras.
    s.primary_engine = "saucenao".to_owned();
    s.secondary_engine_mode = "fallback".to_owned();
    s.extras_only_as_fallback = true;
    assert_eq!(engine_waves(&s, None, true), vec![vec!["iqdb", "ascii2d"]]);
    // Disabled with nothing else: the only wave empties and goes.
    s.secondary_engine_mode = "disabled".to_owned();
    s.enable_ascii2d = false;
    assert!(engine_waves(&s, None, true).is_empty());
    // Planned engines list each engine once.
    let mut d = settings();
    d.secondary_engine_mode = "fallback".to_owned();
    d.extras_only_as_fallback = true;
    d.enable_tracemoe = true;
    assert_eq!(
        planned_engines(&d, None, false),
        vec!["iqdb", "saucenao", "tracemoe"]
    );
}

// ---- fallback (TestMeasuredSimilarityReplacesOrdinalScores, TestWeakResultFallback,
// ---- TestPawchiveExactLookup::test_an_exact_hit_makes_the_slow_fallback_unnecessary) -------

fn candidate(engine: &str, similarity: f64, measured: bool, url: &str) -> Candidate {
    let mut c = Candidate::new(url);
    c.engine = engine.to_owned();
    c.similarity = similarity;
    c.similarity_measured = measured;
    c.source_name = Some(classify_site(None, url));
    c
}

#[test]
fn at_zero_only_the_primary_finding_something_skips_the_fallback() {
    let s = settings();
    let waves = vec![vec!["iqdb", "ascii2d"], vec!["saucenao"]];
    let mut found = HashMap::new();
    found.insert("ascii2d".to_owned(), true);
    let (skip, why) = fallback_is_unnecessary(&s, &waves, &found, &[]);
    assert!(!skip);
    assert_eq!(why, "the primary engine (iqdb) found nothing");
    found.insert("iqdb".to_owned(), true);
    let (skip, why) = fallback_is_unnecessary(&s, &waves, &found, &[]);
    assert!(skip);
    assert_eq!(why, "the primary engine (iqdb) found results");
}

#[test]
fn a_threshold_counts_only_measured_results_on_kept_sites() {
    let mut s = settings();
    s.fallback_below_similarity = 75.0;
    let waves = vec![vec!["iqdb"], vec!["saucenao"]];
    let found = HashMap::new();
    let lens = candidate(
        "Google Lens",
        80.0,
        false,
        "https://danbooru.donmai.us/posts/1",
    );
    let (skip, why) = fallback_is_unnecessary(&s, &waves, &found, std::slice::from_ref(&lens));
    assert!(!skip);
    assert_eq!(
        why,
        "best measured match is 0%, under the 75% threshold (1 result(s) from engines that don't measure similarity)"
    );
    // A result that was measured afterwards counts, whichever engine found it.
    let mut measured = lens.clone();
    measured.similarity_measured = true;
    measured.similarity = 79.5;
    let (skip, why) = fallback_is_unnecessary(&s, &waves, &found, std::slice::from_ref(&measured));
    assert!(skip);
    // Python's `%.0f` rounds half to even: 79.5 -> 80, 80.5 -> 80.
    assert_eq!(why, "best match is 80%, at or above the 75% threshold");
    measured.similarity = 80.5;
    let (_, why) = fallback_is_unnecessary(&s, &waves, &found, std::slice::from_ref(&measured));
    assert_eq!(why, "best match is 80%, at or above the 75% threshold");
    // A site the user unticked does not count.
    s.enabled_sites.retain(|site| site != "Danbooru");
    let iqdb = candidate("IQDB", 99.0, false, "https://danbooru.donmai.us/posts/2");
    let (skip, _) = fallback_is_unnecessary(&s, &waves, &found, &[iqdb]);
    assert!(!skip);
}

// ---- no_match (TestAPartialSearchIsNotAnError) --------------------------------------------

#[test]
fn no_match_tells_an_error_from_an_answer() {
    let none: HashSet<String> = HashSet::new();
    assert_eq!(no_match(&[], &none), (MatchStatus::NotFound, None, true));
    let errors = vec!["IQDB: down".to_owned(), "SauceNAO: slow".to_owned()];
    let message = Some("IQDB: down; SauceNAO: slow".to_owned());
    assert_eq!(
        no_match(&errors, &none),
        (MatchStatus::Error, message.clone(), true)
    );
    let one: HashSet<String> = ["saucenao".to_owned()].into();
    assert_eq!(
        no_match(&errors, &one),
        (MatchStatus::NotFound, message, false)
    );
    // A hash lookup or the local index is not an answer.
    let exact: HashSet<String> = ["pawchive".to_owned(), "pawchiveindex".to_owned()].into();
    assert_eq!(no_match(&errors, &exact).0, MatchStatus::Error);
    let mixed: HashSet<String> = ["pawchive".to_owned(), "iqdb".to_owned()].into();
    assert_eq!(no_match(&errors, &mixed).0, MatchStatus::NotFound);
}

// ---- running a wave (TestAPartialSearchIsNotAnError::test_the_runner_records_who_answered) ---

#[tokio::test]
async fn a_wave_merges_what_each_engine_found_and_records_who_answered() {
    let scripted = Scripted::new();
    scripted.iqdb.push(Step::Value(Ok(vec![iqdb_match(
        "https://danbooru.donmai.us/posts/1",
        97.0,
    )])));
    scripted
        .saucenao
        .push(Step::Value(Err(SauceNaoError("quota".to_owned()))));
    let mut s = settings();
    s.enable_tag_blacklist = true;
    s.tag_blacklist = vec!["highres".to_owned()];
    let run = run_wave(&scripted, &["iqdb", "saucenao"], s).await;
    let found = run.found.unwrap();
    assert!(found["iqdb"]);
    assert!(!found["saucenao"]);
    assert_eq!(run.errors, vec!["SauceNAO: quota"]);
    assert_eq!(run.answered, HashSet::from(["iqdb".to_owned()]));
    assert_eq!(run.candidates.len(), 1);
    let c = &run.candidates[0];
    assert_eq!(c.engine, "IQDB");
    assert_eq!(c.source_name.as_deref(), Some("Danbooru"));
    assert_eq!((c.width, c.height), (Some(100), Some(200)));
    // The blacklist ran over the engine's own tags.
    assert_eq!(
        c.engine_tags
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        vec!["kept"]
    );
    let mut finished = run.finished;
    finished.sort();
    assert_eq!(finished, vec!["iqdb", "saucenao"]);
}

#[tokio::test]
async fn an_empty_wave_does_nothing_and_an_unknown_engine_finds_nothing() {
    let scripted = Scripted::new();
    let run = run_wave(&scripted, &[], settings()).await;
    assert!(run.found.unwrap().is_empty());
    assert!(run.candidates.is_empty() && run.errors.is_empty() && run.answered.is_empty());
    assert!(run.finished.is_empty());
    let run = run_wave(&scripted, &["nonsense"], settings()).await;
    assert!(!run.found.unwrap()["nonsense"]);
    assert!(run.errors.is_empty());
    assert!(run.answered.contains("nonsense"));
}

#[tokio::test]
async fn one_engine_panicking_is_that_engines_failure_in_a_wave_of_two() {
    let scripted = Scripted::new();
    scripted.iqdb.push(Step::Panic("boom"));
    scripted.saucenao.push(Step::Value(Ok(vec![sauce_match(
        "https://danbooru.donmai.us/posts/9",
        90.0,
        Vec::new(),
    )])));
    let run = run_wave(&scripted, &["iqdb", "saucenao"], settings()).await;
    let found = run.found.unwrap();
    assert!(!found["iqdb"]);
    assert!(found["saucenao"]);
    assert_eq!(run.errors, vec!["IQDB: boom"]);
    assert_eq!(run.candidates.len(), 1);
    assert_eq!(run.answered, HashSet::from(["saucenao".to_owned()]));
}

#[tokio::test]
async fn a_wave_of_one_propagates_a_raise_after_telling_the_pacer() {
    let scripted = Scripted::new();
    scripted.iqdb.push(Step::Panic("boom"));
    let run = run_wave(&scripted, &["iqdb"], settings()).await;
    assert_eq!(run.found, Err(Raised("boom".to_owned())));
    assert!(run.candidates.is_empty() && run.errors.is_empty() && run.answered.is_empty());
    assert_eq!(run.finished, vec!["iqdb"]);
}

#[tokio::test]
async fn a_single_engine_that_fails_cleanly_is_not_answered() {
    let scripted = Scripted::new();
    scripted.tracemoe.push(Step::Value(Err(TraceMoeError(
        "trace.moe request failed".to_owned(),
    ))));
    let run = run_wave(&scripted, &["tracemoe"], settings()).await;
    assert!(!run.found.unwrap()["tracemoe"]);
    assert_eq!(run.errors, vec!["trace.moe: trace.moe request failed"]);
    assert!(run.answered.is_empty());
    assert_eq!(run.finished, vec!["tracemoe"]);
}

// ---- the slow-search rule -----------------------------------------------------------------

#[tokio::test]
async fn a_slow_search_with_nothing_is_not_trusted_as_not_found() {
    let scripted = Scripted::new();
    scripted.iqdb.push(Step::After(45.0, Ok(Vec::new())));
    let run = run_wave(&scripted, &["iqdb"], settings()).await;
    assert!(run.candidates.is_empty());
    assert_eq!(
        run.errors,
        vec!["IQDB took 45s and found nothing (too slow to trust)"]
    );
    assert!(run.answered.is_empty());

    // Just under the line is a real answer.
    let scripted = Scripted::new();
    scripted.iqdb.push(Step::After(44.9, Ok(Vec::new())));
    let run = run_wave(&scripted, &["iqdb"], settings()).await;
    assert!(run.errors.is_empty());
    assert!(run.answered.contains("iqdb"));

    // SauceNAO counts "no usable results": one under its minimum is still nothing.
    let scripted = Scripted::new();
    let mut s = settings();
    s.saucenao.min_similarity = 60.0;
    scripted.saucenao.push(Step::After(
        50.4,
        Ok(vec![sauce_match(
            "https://danbooru.donmai.us/posts/1",
            59.9,
            Vec::new(),
        )]),
    ));
    let run = run_wave(&scripted, &["saucenao"], s).await;
    assert!(run.candidates.is_empty());
    assert!(!run.found.unwrap()["saucenao"]);
    assert_eq!(
        run.errors,
        vec!["SauceNAO took 50s and found nothing (too slow to trust)"]
    );
}

// ---- SauceNAO collector: remap runs before the blacklist
// ---- (test_tag_rules.py::TestRemapRunsBeforeBlacklistOnTheSauceNaoPath) --------------------

#[tokio::test]
async fn the_saucenao_collector_remaps_then_blacklists() {
    let creator = Tag {
        name: "someone".to_owned(),
        namespace: Some("creator".to_owned()),
        source: TagSource::SearchEngine,
    };
    let cases: [(&str, Vec<&str>); 2] =
        [("creator:*", vec!["artist:someone"]), ("artist:*", vec![])];
    for (pattern, expected) in cases {
        let scripted = Scripted::new();
        scripted.saucenao.push(Step::Value(Ok(vec![sauce_match(
            "https://danbooru.donmai.us/posts/1",
            95.0,
            vec![creator.clone()],
        )])));
        let mut s = settings();
        s.enable_tag_namespace_remap = true;
        s.tag_namespace_remap = OrderedMap(vec![("creator".to_owned(), "artist".to_owned())]);
        s.enable_tag_blacklist = true;
        s.tag_blacklist = vec![pattern.to_owned()];
        let run = run_wave(&scripted, &["saucenao"], s).await;
        let shown: Vec<String> = run.candidates[0]
            .engine_tags
            .iter()
            .map(|t| format!("{}:{}", t.namespace.as_deref().unwrap_or(""), t.name))
            .collect();
        assert_eq!(shown, expected, "pattern {pattern}");
    }
}

// ---- blocked engines leave a note (TestGoogleImagesBlocked, TestAscii2dBotChallenge,
// ---- TestGoogleLensWithoutABrowser, test_lens_browser.py::test_a_resting_lens_says_so_on_the_row,
// ---- test_yandex.py::TestStandingDown) ----------------------------------------------------

#[tokio::test]
async fn a_stood_down_engine_is_not_asked_and_the_row_says_so() {
    let scripted = Scripted::new();
    scripted.ascii2d_blocked.store(true, Ordering::SeqCst);
    scripted.google_images_blocked.store(true, Ordering::SeqCst);
    scripted.lens_blocked.store(true, Ordering::SeqCst);
    scripted.yandex_blocked.store(true, Ordering::SeqCst);
    let run = run_wave(
        &scripted,
        &["ascii2d", "googleimages", "googlelens", "yandex"],
        settings(),
    )
    .await;
    assert!(run.found.unwrap().values().all(|f| !f));
    let mut errors = run.errors;
    errors.sort();
    assert_eq!(
        errors,
        vec![
            "Google Images: not searched (it stood down earlier this run)",
            "Google Lens: not searched (it stood down earlier this run)",
            "Yandex: not searched (it stood down earlier this run)",
        ]
    );
    // ascii2d says nothing (only the first image gets the explanation), so it counts as an
    // engine that finished without an error.
    assert!(run.answered.contains("ascii2d"));
    assert!(
        scripted.asked().is_empty(),
        "nothing is asked of a stood-down engine"
    );

    // Resting says when it ends.
    let scripted = Scripted::new();
    scripted.lens_blocked.store(true, Ordering::SeqCst);
    scripted.lens_resting.store(true, Ordering::SeqCst);
    scripted.yandex_blocked.store(true, Ordering::SeqCst);
    scripted.yandex_resting.store(true, Ordering::SeqCst);
    let run = run_wave(&scripted, &["googlelens", "yandex"], settings()).await;
    let mut errors = run.errors;
    errors.sort();
    assert_eq!(
        errors,
        vec![
            "Google Lens: not searched (resting after an unanswered robot check, until 21:41)",
            "Yandex: not searched (resting after a captcha, until 09:05)",
        ]
    );
}

#[tokio::test]
async fn lens_waits_its_turn_then_reports_why_it_cannot_run() {
    let scripted = Scripted::new();
    let run = run_wave(&scripted, &["googlelens"], settings()).await;
    assert!(!run.found.unwrap()["googlelens"]);
    assert_eq!(run.errors, vec!["Google Lens: no browser"]);
    assert!(run.answered.is_empty());
    assert_eq!(scripted.asked(), vec!["lens-wait", "googlelens"]);
}

#[tokio::test]
async fn google_images_and_yandex_keep_their_own_errors_apart() {
    let scripted = Scripted::new();
    scripted
        .google_images
        .push(Step::Value(Err(GoogleImagesError::Blocked(
            "Google wants a consent click".to_owned(),
        ))));
    scripted.yandex.push(Step::Value(Err(YandexError::Failed(
        "Yandex did not answer".to_owned(),
    ))));
    let run = run_wave(&scripted, &["googleimages", "yandex"], settings()).await;
    let mut errors = run.errors;
    errors.sort();
    assert_eq!(
        errors,
        vec![
            "Google Images: Google wants a consent click",
            "Yandex: Yandex did not answer"
        ]
    );
    assert!(run.answered.is_empty());
    assert_eq!(
        scripted
            .asked()
            .iter()
            .filter(|a| *a == "yandex-wait")
            .count(),
        1
    );
}

#[tokio::test]
async fn an_exception_that_is_not_the_engines_own_leaves_the_collector() {
    let raised = || {
        let scripted = Scripted::new();
        scripted
            .yandex
            .push(Step::Value(Err(YandexError::Unexpected(UnexpectedInput))));
        scripted
    };
    // In a wave of two it is the engine's failure, with the exception's text.
    let run = run_wave(&raised(), &["yandex", "googleimages"], settings()).await;
    assert!(!run.found.unwrap()["yandex"]);
    assert_eq!(
        run.errors,
        vec!["Yandex: an input on which the Python parser raises"]
    );
    // In a wave of one it propagates.
    let run = run_wave(&raised(), &["yandex"], settings()).await;
    assert_eq!(
        run.found,
        Err(Raised(
            "an input on which the Python parser raises".to_owned()
        ))
    );
    assert_eq!(run.finished, vec!["yandex"]);
    // Google Images likewise.
    let scripted = Scripted::new();
    scripted
        .google_images
        .push(Step::Value(Err(GoogleImagesError::Unexpected(
            UnexpectedInput,
        ))));
    let run = run_wave(&scripted, &["googleimages"], settings()).await;
    assert!(run.found.is_err());
}

// ---- pawchive (TestPawchiveExactLookup) and the index --------------------------------------

fn pawchive_match(url: &str) -> PawchiveMatch {
    PawchiveMatch {
        url: url.to_owned(),
        title: None,
        thumb_url: Some("https://img.invalid/t".to_owned()),
        page_index: 0,
        page_count: 1,
    }
}

#[tokio::test]
async fn a_pawchive_hit_is_a_certain_hundred() {
    let scripted = Scripted::new();
    scripted.pawchive.push(Step::Value(Ok(vec![pawchive_match(
        "https://pawchive.pw/post/1",
    )])));
    let mut known = entry();
    known.hydrus_hash = Some("cd".repeat(32));
    let run = run_wave_for(&scripted, &["pawchive"], settings(), &known).await;
    assert!(run.found.unwrap()["pawchive"]);
    assert!(run.errors.is_empty());
    assert_eq!(run.candidates[0].similarity, 100.0);
    assert_eq!(run.candidates[0].engine, "Pawchive");
    assert_eq!(run.candidates[0].source_name.as_deref(), Some("Pawchive"));
    // The known hash is used: the file is not read.
    assert_eq!(
        scripted.asked(),
        vec![format!("pawchive:{}", "cd".repeat(32))]
    );
}

#[tokio::test]
async fn pawchive_hashes_the_file_when_the_hash_is_not_known() {
    let scripted = Scripted::new();
    let run = run_wave(&scripted, &["pawchive"], settings()).await;
    assert!(run.errors.is_empty() && run.answered.contains("pawchive"));
    assert_eq!(
        scripted.asked(),
        vec!["sha256".to_owned(), format!("pawchive:{}", "ab".repeat(32))]
    );

    let scripted = Scripted::new();
    scripted.sha.push(Step::Value(Err(
        "[Errno 2] No such file or directory: '/pics/a.png'".to_owned(),
    )));
    let run = run_wave(&scripted, &["pawchive"], settings()).await;
    assert!(!run.found.unwrap()["pawchive"]);
    assert_eq!(
        run.errors,
        vec![
            "Pawchive: could not read the file to hash it ([Errno 2] No such file or directory: '/pics/a.png')"
        ]
    );
}

#[tokio::test]
async fn a_failed_pawchive_lookup_is_reported_and_a_deadline_is_not_caught() {
    let scripted = Scripted::new();
    scripted
        .pawchive
        .push(Step::Value(Err(PawchiveError::Lookup(
            PawchiveLookupError("pawchive.pw is rate limiting".to_owned()),
        ))));
    let run = run_wave(&scripted, &["pawchive"], settings()).await;
    assert_eq!(run.errors, vec!["Pawchive: pawchive.pw is rate limiting"]);
    assert!(run.answered.is_empty());

    let scripted = Scripted::new();
    scripted
        .pawchive
        .push(Step::Value(Err(PawchiveError::HardTimeout(
            HardTimeoutError("took too long".to_owned()),
        ))));
    let run = run_wave(&scripted, &["pawchive"], settings()).await;
    assert_eq!(run.found, Err(Raised("took too long".to_owned())));
}

#[tokio::test]
async fn the_pawchive_index_is_local_and_labelled_by_exactness() {
    // Empty: false, no error, and the answer counts as having been given.
    let scripted = Scripted::new();
    let run = run_wave(&scripted, &["pawchiveindex"], settings()).await;
    assert!(!run.found.unwrap()["pawchiveindex"]);
    assert!(run.candidates.is_empty() && run.errors.is_empty());
    assert!(run.answered.contains("pawchiveindex"));

    let scripted = Scripted::new();
    scripted
        .index
        .push(Step::Value(Ok(IndexAnswer::Matches(vec![
            IndexMatch {
                url: "https://pawchive.pw/post/1".to_owned(),
                thumb_url: None,
                similarity: 100.0,
                exact: true,
            },
            IndexMatch {
                url: "https://pawchive.pw/post/2".to_owned(),
                thumb_url: None,
                similarity: 91.0,
                exact: false,
            },
        ]))));
    let run = run_wave(&scripted, &["pawchiveindex"], settings()).await;
    assert!(run.found.unwrap()["pawchiveindex"]);
    assert_eq!(run.candidates[0].engine, "Pawchive");
    assert_eq!(run.candidates[1].engine, "Pawchive index");

    let scripted = Scripted::new();
    scripted
        .index
        .push(Step::Value(Ok(IndexAnswer::Unreadable)));
    let run = run_wave(&scripted, &["pawchiveindex"], settings()).await;
    assert_eq!(
        run.errors,
        vec!["Pawchive index: could not read the local file"]
    );
    assert!(run.answered.is_empty());

    let scripted = Scripted::new();
    scripted.index.push(Step::Value(Err(IndexError(
        "database is locked".to_owned(),
    ))));
    let run = run_wave(&scripted, &["pawchiveindex"], settings()).await;
    assert_eq!(run.errors, vec!["Pawchive index: database is locked"]);
}

// ---- retry_could_help (TestRetryOnlyWhenItCouldHelp) --------------------------------------

#[test]
fn a_retry_helps_unless_every_failure_is_a_stood_down_engine() {
    let scripted = Scripted::new();
    let runner = scripted.runner();
    let lens = "Google Lens: not searched (it stood down earlier this run)";
    assert!(runner.retry_could_help(None));
    assert!(runner.retry_could_help(Some("")));
    assert!(runner.retry_could_help(Some(" ; ;")));
    // Not stood down yet: worth a retry.
    assert!(runner.retry_could_help(Some(lens)));
    scripted.lens_blocked.store(true, Ordering::SeqCst);
    assert!(!runner.retry_could_help(Some(lens)));
    // A network fault alongside it still is.
    assert!(runner.retry_could_help(Some(&format!("{lens}; IQDB: connection reset"))));
    // Unattributable messages are.
    assert!(runner.retry_could_help(Some("something odd")));
    scripted.yandex_blocked.store(true, Ordering::SeqCst);
    scripted.ascii2d_blocked.store(true, Ordering::SeqCst);
    scripted.google_images_blocked.store(true, Ordering::SeqCst);
    assert!(!runner.retry_could_help(Some(
        "Yandex: a;  ascii2d: b ; Google Images: c;Google Lens: d"
    )));
}

// ---- candidates (test_urls_and_availability.py::TestStructurallyDeadUrls) -----------------

#[test]
fn a_structurally_dead_url_never_becomes_a_candidate() {
    let mut candidates = Vec::new();
    let mut add = |url: &str, name: Option<&str>| {
        append_candidate(
            &mut candidates,
            url,
            name,
            None,
            90.0,
            "IQDB",
            Vec::new(),
            None,
            None,
        );
    };
    add(
        "https://chan.sankakucomplex.com/post/show/12345",
        Some("Sankaku"),
    );
    add("https://danbooru.donmai.us/posts/12345", Some("Sankaku"));
    add("https://example.invalid/x", None);
    assert_eq!(candidates.len(), 2);
    // The URL's host wins over the engine's label; an unknown host is "Other".
    assert_eq!(candidates[0].source_name.as_deref(), Some("Danbooru"));
    assert_eq!(candidates[1].source_name.as_deref(), Some("Other"));
}

#[test]
fn guess_host_is_the_netloc_or_nothing() {
    assert_eq!(
        guess_host("https://a.example:8080/x"),
        Some("a.example:8080".to_owned())
    );
    assert_eq!(guess_host("/relative"), None);
    assert_eq!(guess_host(""), None);
    assert_eq!(guess_host("http://[bad"), None);
}
