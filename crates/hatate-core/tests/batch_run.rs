//! `hatate batch` end to end (DAN-2065, Phase 3.i): the folder scan, the hash pass, the
//! duplicate filter, the search loop with its waits, retries, quota pause and stop, and the
//! session it leaves behind. The engines answer from a script (`common::ScriptedEngines`), the
//! booru page comes from a local https server through `NetClient::new` (rustls with the explicit
//! provider), the clock is manual so a 60 s wait costs nothing. No live network.
//!
//! The differential against the Python worker loop and `search_image` is the broad check
//! (`worktrees/DAN-2065-differential.log`); these are the named, readable ones. The Python test
//! each maps to is in `docs/batch.md`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{Recorded, Reply, ScriptedEngines, Step, TestPki, TestServer, iqdb_match};
use hatate_core::batch::{BatchEnv, Ending, Outcome, Reporter, SessionSink, run_batch};
use hatate_core::clock::{Clock, ManualClock};
use hatate_core::config::Settings;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::engines::iqdb::IqdbError;
use hatate_core::engines::saucenao::SauceNaoError;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::run_state::{RunChannel, RunEnd, RunEvent, RunSink, RunState};
use hatate_core::search_engine::SearchEngine;
use hatate_core::session::{LoadOptions, SessionStore};
use hatate_core::stop_record::{Read as StopRead, StopRecordFile};

const NOW: f64 = 1_792_725_790.0;
const POST: &str = r#"{"id": 5, "image_width": 800, "image_height": 600, "rating": "g",
    "file_url": "https://cdn.donmai.us/f.png",
    "tag_string_general": "one two three four five six", "tag_string_artist": "someone"}"#;
const URL: &str = "https://danbooru.donmai.us/posts/5";

/// Everything a batch is given, in a temp profile.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
    server: TestServer,
    engine: SearchEngine,
    engines: Arc<ScriptedEngines>,
    clock: Arc<ManualClock>,
    settings: Mutex<Settings>,
    reporter: Arc<Recording>,
    stop: Arc<AtomicBool>,
    quota_after_calls: AtomicUsize,
    /// Seconds the manual clock moves on each time the booru page is served: non-engine work.
    booru_cost: Arc<Mutex<f64>>,
}

/// A reporter that keeps what it is told and can raise the stop flag on a line.
#[derive(Default)]
struct Recording {
    lines: Mutex<Vec<String>>,
    countdowns: Mutex<Vec<(String, f64, f64)>>,
    stop_on: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

impl Reporter for Recording {
    fn line(&self, text: &str) {
        self.lines.lock().unwrap().push(text.to_owned());
        if let Some((needle, flag)) = self.stop_on.lock().unwrap().as_ref() {
            if text.contains(needle.as_str()) {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
    fn countdown(&self, label: &str, remaining: f64, total: f64) {
        self.countdowns
            .lock()
            .unwrap()
            .push((label.to_owned(), remaining, total));
    }
}

/// Counts saves, and writes the session the way the binary does.
struct CountingSink {
    inner: hatate_core::session::SessionWriter,
    saves: AtomicUsize,
}

impl SessionSink for CountingSink {
    fn save(&self, entries: &[Entry], now: f64) -> Result<(), String> {
        self.saves.fetch_add(1, Ordering::SeqCst);
        self.inner.save(entries, now).map_err(|e| e.to_string())
    }
}

async fn world() -> World {
    let hosts = ["danbooru.donmai.us"];
    let pki = TestPki::generate_for(&hosts);
    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let booru_cost = Arc::new(Mutex::new(0.0));
    let (handler_clock, handler_cost) = (clock.clone(), booru_cost.clone());
    let server = TestServer::https(&pki, move |req: &Recorded| {
        if req.target.contains("/posts/5.json") {
            handler_clock.advance(*handler_cost.lock().unwrap());
            Reply::ok(POST).header("Content-Type", "application/json")
        } else {
            Reply::new(404, "no route")
        }
    })
    .await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, server.addr);
    }
    let net = NetClient::new(cfg).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("dan2065-batch-")
        .tempdir()
        .unwrap();
    let root = dir.path().to_path_buf();
    let home: std::ffi::OsString = root.join("home").into();
    let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
    let engines = Arc::new(ScriptedEngines::default());
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = SearchEngine::with_recorder(net, runner, paths.clone(), clock.clone(), None);
    let mut settings = Settings::with_defaults(&paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive_index = false;
    settings.match_conditions.require_larger_than_local = false;
    // Zero gap unless a test asks for one.
    settings.delay_min_seconds = 0.0;
    settings.delay_max_seconds = 0.0;
    World {
        _dir: dir,
        root,
        paths,
        server,
        engine,
        engines,
        clock,
        settings: Mutex::new(settings),
        reporter: Arc::new(Recording::default()),
        stop: Arc::new(AtomicBool::new(false)),
        quota_after_calls: AtomicUsize::new(usize::MAX),
        booru_cost,
    }
}

impl World {
    /// `n` pictures with different pixels in `pics/`, plus whatever the test adds.
    fn pictures(&self, names: &[&str]) -> PathBuf {
        let dir = self.root.join("pics");
        std::fs::create_dir_all(&dir).unwrap();
        for (i, name) in names.iter().enumerate() {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let img = image::RgbImage::from_fn(4, 3, |x, y| {
                image::Rgb([x as u8 * 40, y as u8 * 60, 7 + i as u8])
            });
            img.save(&p).unwrap();
        }
        dir
    }

    fn sink(&self) -> CountingSink {
        let writer = SessionStore::new(&self.paths).open_writer().unwrap();
        CountingSink {
            inner: writer,
            saves: AtomicUsize::new(0),
        }
    }

    async fn run(&self, folder: &Path, entries: &mut Vec<Entry>) -> (Outcome, CountingSink) {
        self.run_with(self.reporter.clone(), folder, entries).await
    }

    /// As [`World::run`] with another reporter in the loop's hands.
    async fn run_with(
        &self,
        reporter: Arc<dyn Reporter>,
        folder: &Path,
        entries: &mut Vec<Entry>,
    ) -> (Outcome, CountingSink) {
        let sink = self.sink();
        let quota_file = QuotaPauseFile::new(&self.paths);
        let engines = self.engines.clone();
        let threshold = &self.quota_after_calls;
        let quota = move || engines.calls().len() >= threshold.load(Ordering::SeqCst);
        let random = || 0.5;
        let reset = || {};
        let env = BatchEnv {
            engine: &self.engine,
            settings: &self.settings,
            stop: self.stop.clone(),
            quota_exhausted: &quota,
            quota_file: &quota_file,
            sink: &sink,
            reporter,
            unit_random: &random,
            reset_run_state: &reset,
            hydrus_client: &|s| hatate_core::hydrus_client::HydrusClient::new(s),
        };
        let outcome = run_batch(&env, folder, entries).await;
        (outcome, sink)
    }

    fn saved(&self) -> Vec<Entry> {
        SessionStore::new(&self.paths)
            .load(&LoadOptions::at(NOW))
            .entries
    }

    fn lines(&self) -> Vec<String> {
        self.reporter.lines.lock().unwrap().clone()
    }

    fn match_every_image(&self, n: usize) {
        for _ in 0..n {
            self.engines
                .iqdb
                .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
        }
    }
}

/// Equal as stored: the fields a session does not keep (`last_searched`, the fetch flags) are
/// compared as the store sees them.
fn same_session(a: &[Entry], b: &[Entry]) -> bool {
    let json = |e: &[Entry]| serde_json::to_string(e).unwrap();
    json(a) == json(b)
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.filename()).collect()
}

/// The verifier's PASS names this test: a folder through `run_batch` over https, scan, hash,
/// duplicate drop, search, the matched-URL line, and a session the store reads back.
#[tokio::test]
async fn a_folder_is_scanned_hashed_searched_and_saved_over_https() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png", "sub/c.png"]);
    std::fs::write(dir.join("notes.txt"), b"not an image").unwrap();
    // A copy under another name: same bytes, so the same hash.
    std::fs::copy(dir.join("a.png"), dir.join("a_copy.png")).unwrap();
    w.settings.lock().unwrap().log_file_path =
        w.root.join("matched.log").to_string_lossy().into_owned();
    w.match_every_image(3);

    let mut entries = Vec::new();
    let (outcome, sink) = w.run(&dir, &mut entries).await;

    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(
        (outcome.found, outcome.added, outcome.duplicates),
        (4, 3, 1)
    );
    assert_eq!(outcome.searched, 3);
    assert!(!outcome.save_failed);
    assert_eq!(names(&entries), ["a.png", "b.png", "c.png"]);
    for e in &entries {
        assert_eq!(e.status, MatchStatus::Good, "{}", e.path);
        assert_eq!(e.hydrus_hash.as_deref().map(str::len), Some(64));
        assert_eq!(e.candidates[0].url, URL);
    }
    assert_eq!(w.engines.calls(), ["iqdb", "iqdb", "iqdb"]);
    assert!(
        sink.saves.load(Ordering::SeqCst) >= 2,
        "once when added, once at the end"
    );
    assert!(
        w.server
            .recorded()
            .iter()
            .all(|r| r.header("host") == Some("danbooru.donmai.us"))
    );

    // The session on disk is the list in memory.
    let saved = w.saved();
    assert_eq!(saved.len(), 3);
    assert!(
        same_session(&saved, &entries),
        "the saved list is the list in memory"
    );

    let matched = std::fs::read_to_string(w.root.join("matched.log")).unwrap();
    assert_eq!(matched.lines().count(), 3);
    assert!(
        matched
            .lines()
            .all(|l| l.starts_with("[") && l.ends_with(&format!("-> {URL}"))),
        "{matched}"
    );
}

#[tokio::test]
async fn a_second_run_over_the_same_folder_adds_and_searches_nothing() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;

    let mut again = w.saved();
    let (outcome, _) = w.run(&dir, &mut again).await;
    assert_eq!(
        (
            outcome.found,
            outcome.added,
            outcome.duplicates,
            outcome.searched
        ),
        (2, 0, 2, 0)
    );
    assert!(outcome.nothing_to_search);
    assert!(same_session(&again, &entries));
    assert_eq!(w.engines.calls().len(), 2);
}

#[tokio::test]
async fn files_already_in_the_session_that_are_not_searched_are_searched_too() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    // The first run stops after hashing/adding: searching is cut off at once.
    w.stop.store(true, Ordering::SeqCst);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    // A stop raised before hashing gives no files at all.
    assert!(entries.is_empty());

    w.stop.store(false, Ordering::SeqCst);
    let mut seeded = vec![Entry::new(dir.join("a.png").to_string_lossy())];
    w.match_every_image(2);
    let (outcome, _) = w.run(&dir, &mut seeded).await;
    assert_eq!(
        (outcome.added, outcome.duplicates, outcome.searched),
        (1, 1, 2)
    );
    assert_eq!(seeded.len(), 2);
    assert!(seeded.iter().all(|e| e.status == MatchStatus::Good));
    assert!(
        seeded[0].hydrus_hash.is_some(),
        "the entry that had no hash was hashed in the same pass"
    );
}

#[tokio::test]
async fn an_empty_folder_says_so_and_saves_nothing() {
    let w = world().await;
    let dir = w.root.join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    let mut entries = Vec::new();
    let (outcome, sink) = w.run(&dir, &mut entries).await;
    assert!(outcome.nothing_to_search);
    assert_eq!(
        sink.saves.load(Ordering::SeqCst),
        0,
        "an empty list is never written over a session"
    );
    assert!(w.lines().iter().any(|l| l.contains("no unsearched images")));
}

#[tokio::test]
async fn a_network_fault_is_retried_once_after_the_first_pass() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("timeout".to_owned()))));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    // The retry of the first image.
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 3, "two images and one retry");
    assert!(entries.iter().all(|e| e.status == MatchStatus::Good));
    assert!(w.lines().iter().any(|l| l.contains("retrying 1 image(s)")));
}

#[tokio::test]
async fn a_persistent_failure_is_retried_exactly_once() {
    let w = world().await;
    let dir = w.pictures(&["a.png"]);
    for _ in 0..3 {
        w.engines
            .iqdb
            .push(Step::Return(Err(IqdbError("down".to_owned()))));
    }
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 2);
    assert_eq!(entries[0].status, MatchStatus::Error);
    assert_eq!(w.engines.calls(), ["iqdb", "iqdb"]);
}

/// `test_several_failures_are_all_retried`: every failed image gets its one extra attempt after
/// the first pass, and the image that succeeded is not searched again.
#[tokio::test]
async fn several_failures_are_all_retried_and_the_image_that_succeeded_is_not() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    let fault = || Step::Return(Err(IqdbError("timeout".to_owned())));
    let found = || Step::Return(Ok(vec![iqdb_match(URL, 96.5)]));
    // First pass: a fails, b is found, c fails. Retry pass: a, then c.
    w.engines.iqdb.push(fault());
    w.engines.iqdb.push(found());
    w.engines.iqdb.push(fault());
    w.engines.iqdb.push(found());
    w.engines.iqdb.push(found());
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 5, "three images and two retries");
    assert_eq!(w.engines.calls(), ["iqdb"; 5]);
    assert!(entries.iter().all(|e| e.status == MatchStatus::Good));
    assert!(w.lines().iter().any(|l| l.contains("retrying 2 image(s)")));
}

/// `test_a_not_found_is_not_a_failure`: a verdict of "no match" is final and is not retried.
#[tokio::test]
async fn a_not_found_is_not_a_failure_and_is_not_retried() {
    let w = world().await;
    let dir = w.pictures(&["a.png"]);
    w.engines.iqdb.push(Step::Return(Ok(Vec::new())));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 1);
    assert_eq!(entries[0].status, MatchStatus::NotFound);
    assert_eq!(w.engines.calls(), ["iqdb"]);
}

#[tokio::test]
async fn retries_off_means_one_attempt() {
    let w = world().await;
    w.settings.lock().unwrap().retry_failed_searches = false;
    let dir = w.pictures(&["a.png"]);
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("down".to_owned()))));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 1);
}

#[tokio::test]
async fn a_panic_in_one_engine_marks_that_image_and_the_batch_goes_on() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.settings.lock().unwrap().retry_failed_searches = false;
    w.engines.iqdb.push(Step::Panic("boom"));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 2);
    assert_eq!(entries[0].status, MatchStatus::Error);
    assert!(
        entries[0]
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("Unexpected error: "),
        "{:?}",
        entries[0].error_message
    );
    assert_eq!(entries[1].status, MatchStatus::Good);
}

#[tokio::test]
async fn the_gap_between_images_is_waited_out_with_a_countdown_and_not_after_the_last() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 40.0;
        s.delay_max_seconds = 80.0; // random 0.5 -> 60 s
    }
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    let slept = w.clock.slept();
    assert!(
        (slept - 120.0).abs() < 1.0,
        "two gaps of 60 s, not three: slept {slept}"
    );
    let countdowns = w.reporter.countdowns.lock().unwrap().clone();
    assert!(
        countdowns
            .iter()
            .any(|(label, _, total)| label == "Rate limit" && (*total - 60.0).abs() < 1.0)
    );
    assert_eq!(
        countdowns.last().unwrap().0,
        "",
        "the last call clears the countdown"
    );
}

/// Python's `round()` sends halves to the even number: a 2.25 s wait shows at 2.25, 1.25 and 0.5
/// (`round(0.5)` is 0, which is a change from 1), where rounding halves away from zero showed
/// 2.25, 1.25 and 0.25. Found by the DAN-2132 differential against the real worker.
#[tokio::test]
async fn the_countdown_reports_when_the_displayed_second_changes_with_halves_to_even() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 2.25;
        s.delay_max_seconds = 2.25;
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    let shown: Vec<f64> = w
        .reporter
        .countdowns
        .lock()
        .unwrap()
        .iter()
        .filter(|(label, _, _)| label == "Rate limit")
        .map(|(_, remaining, _)| *remaining)
        .collect();
    assert_eq!(shown, [2.25, 1.25, 0.5]);
}

#[tokio::test]
async fn an_image_served_from_the_cache_costs_no_gap() {
    let w = world().await;
    w.settings.lock().unwrap().delay_min_seconds = 60.0;
    w.settings.lock().unwrap().delay_max_seconds = 60.0;
    let dir = w.pictures(&["a.png"]);
    w.match_every_image(1);
    let mut first = Vec::new();
    w.run(&dir, &mut first).await;
    let before = w.clock.slept();

    // The same bytes under another name, in another folder: a cache hit, and not the last image.
    let dir2 = w.root.join("more");
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::copy(dir.join("a.png"), dir2.join("a2.png")).unwrap();
    let img = image::RgbImage::from_fn(4, 3, |x, _| image::Rgb([x as u8, 1, 2]));
    img.save(dir2.join("b2.png")).unwrap();
    w.match_every_image(1);
    let mut second = Vec::new();
    // `a2.png` sorts first and is a duplicate of nothing in `second`.
    w.run(&dir2, &mut second).await;
    let cached = second.iter().find(|e| e.filename() == "a2.png").unwrap();
    assert_eq!(cached.result_source.as_deref(), Some("cached"));
    assert_eq!(w.clock.slept(), before, "no gap after a cached image");
}

/// `gui_smoke::TestRateLimitIsMeasuredEngineToEngine`, the wiring half: time spent on the booru
/// page after the engines answered counts toward the gap, and doing so never shortens the
/// interval between two engine requests.
#[tokio::test]
async fn booru_time_counts_toward_the_gap_and_the_engine_to_engine_interval_stays_whole() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 60.0;
        s.delay_max_seconds = 60.0;
    }
    *w.booru_cost.lock().unwrap() = 20.0;
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    let start = w.clock.monotonic();
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    let fetches = w
        .server
        .recorded()
        .iter()
        .filter(|r| r.target.contains("/posts/5.json"))
        .count();
    assert_eq!(fetches, 3, "every image fetched its booru page");
    // Each gap is 60 s from one engine answer to the next: 20 s of it the booru page, 40 s waited.
    let slept = w.clock.slept();
    assert!(
        (slept - 80.0).abs() < 1.0,
        "two waits of 40 s, not 60: slept {slept}"
    );
    // Engine answer 1 -> 2 -> 3 is 120 s however the time was spent, plus the last page's 20 s.
    let elapsed = w.clock.monotonic() - start;
    assert!(
        (elapsed - 140.0).abs() < 1.0,
        "the interval the engines see is not shortened: {elapsed}"
    );
}

/// `gui_smoke::TestPerHostPacing`: each engine is paced on its own clock, from its own override,
/// and an engine that is not queried imposes nothing.
#[tokio::test]
async fn a_hosts_own_interval_replaces_the_global_gap_for_that_host_only() {
    let set = |w: &World, global: f64, iqdb: f64| {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = global;
        s.delay_max_seconds = global;
        s.engine_delays =
            hatate_core::config::OrderedMap(vec![("iqdb".to_owned(), vec![iqdb, iqdb])]);
    };
    // A laxer host is not held to the global delay (`test_a_skipped_host_imposes_no_wait`: only
    // IQDB is queried, so SauceNAO's pace is never in it).
    let w = world().await;
    set(&w, 60.0, 5.0);
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    w.run(&dir, &mut Vec::new()).await;
    assert!(
        (w.clock.slept() - 10.0).abs() < 1.0,
        "two gaps of 5 s: {}",
        w.clock.slept()
    );
    assert!(!w.engines.calls().contains(&"saucenao"));

    // A stricter host stretches the gap (`test_a_longer_override_extends_the_wait`).
    let w = world().await;
    set(&w, 10.0, 90.0);
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    w.run(&dir, &mut Vec::new()).await;
    assert!(
        (w.clock.slept() - 180.0).abs() < 1.0,
        "two gaps of 90 s: {}",
        w.clock.slept()
    );
}

/// `test_a_stricter_host_paces_the_whole_wave`: engines of one wave are queried together, so the
/// wave waits for the slowest of them.
#[tokio::test]
async fn a_stricter_host_paces_the_whole_wave() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.secondary_engine_mode = "always".to_owned();
        s.delay_min_seconds = 10.0;
        s.delay_max_seconds = 10.0;
        s.engine_delays =
            hatate_core::config::OrderedMap(vec![("saucenao".to_owned(), vec![70.0, 70.0])]);
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    w.run(&dir, &mut Vec::new()).await;
    assert!(
        w.engines.calls().contains(&"saucenao") && w.engines.calls().contains(&"iqdb"),
        "{:?}",
        w.engines.calls()
    );
    assert!(
        (w.clock.slept() - 70.0).abs() < 1.0,
        "IQDB waited for SauceNAO's 70 s: {}",
        w.clock.slept()
    );
}

/// `test_a_broken_engine_report_falls_back_instead_of_hammering`: when a fresh search ends with no
/// engine having reported (SauceNAO is the only engine and has been left out because its quota is
/// spent, so none is asked), the wait falls back to the global delay and not to zero.
#[tokio::test]
async fn no_engine_report_falls_back_to_the_global_delay() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.primary_engine = "saucenao".to_owned();
        s.continue_without_saucenao_on_quota = true;
        s.delay_min_seconds = 60.0;
        s.delay_max_seconds = 60.0;
        s.engine_delays =
            hatate_core::config::OrderedMap(vec![("saucenao".to_owned(), vec![1.0, 1.0])]);
    }
    w.quota_after_calls.store(0, Ordering::SeqCst);
    let dir = w.pictures(&["a.png", "b.png"]);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    assert!(w.engines.calls().is_empty(), "{:?}", w.engines.calls());
    assert!(
        (w.clock.slept() - 60.0).abs() < 1.0,
        "the global 60 s, not SauceNAO's 1 s and not nothing: {}",
        w.clock.slept()
    );
}

/// Python logs `entry.matched_url`, a stored field, and a cached result whose matches have all
/// died since is left holding the dead URL (`select_candidate(-1)` changes nothing). The log line
/// is for a row with no candidates; the differential found it (DAN-2065, 1 of 294 entries).
#[tokio::test]
async fn a_cached_result_whose_matches_all_died_still_logs_the_url_python_kept() {
    let w = world().await;
    let dir = w.pictures(&["a.png"]);
    w.settings.lock().unwrap().log_file_path =
        w.root.join("matched.log").to_string_lossy().into_owned();
    w.match_every_image(1);
    let mut first = Vec::new();
    w.run(&dir, &mut first).await;
    assert_eq!(first[0].status, MatchStatus::Good);

    // The cached match is now known to be gone (what an earlier availability sweep leaves).
    let cache = w.paths.search_cache_dir();
    for f in std::fs::read_dir(&cache).unwrap() {
        let f = f.unwrap().path();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains(URL));
        assert!(text.contains("\"remote_available\": true"), "{text}");
        let text = text
            .replace(URL, "https://danbooru.donmai.us/posts/6")
            .replace("\"remote_available\": true", "\"remote_available\": false");
        std::fs::write(&f, text).unwrap();
    }
    std::fs::remove_file(w.root.join("matched.log")).unwrap();
    let mut second = Vec::new();
    w.run(&dir, &mut second).await;

    let e = &second[0];
    assert_eq!(e.result_source.as_deref(), Some("cached"));
    assert!(e.candidates.is_empty(), "the dead match was dropped");
    assert_eq!(e.status, MatchStatus::Good, "the cached verdict stays");
    let matched = std::fs::read_to_string(w.root.join("matched.log")).unwrap();
    assert!(
        matched
            .trim_end()
            .ends_with("a.png -> https://danbooru.donmai.us/posts/6"),
        "{matched}"
    );
}

#[tokio::test]
async fn stop_saves_the_session_and_leaves_the_rest_unsearched() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    *w.reporter.stop_on.lock().unwrap() = Some(("(1/3)".to_owned(), w.stop.clone()));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    assert_eq!(outcome.searched, 1);
    let saved = w.saved();
    assert_eq!(saved.len(), 3, "every file is in the session");
    assert_eq!(
        saved
            .iter()
            .filter(|e| e.status == MatchStatus::Good)
            .count(),
        1
    );
    assert_eq!(
        saved
            .iter()
            .filter(|e| e.status == MatchStatus::NotSearched)
            .count(),
        2
    );
}

#[tokio::test]
async fn stop_wakes_a_wait_at_once() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 600.0;
        s.delay_max_seconds = 600.0;
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    // Raised on the first countdown line of the wait.
    struct StopOnCountdown(Arc<AtomicBool>);
    impl Reporter for StopOnCountdown {
        fn line(&self, _: &str) {}
        fn countdown(&self, label: &str, _: f64, _: f64) {
            if label == "Rate limit" {
                self.0.store(true, Ordering::SeqCst);
            }
        }
    }
    let sink = w.sink();
    let quota_file = QuotaPauseFile::new(&w.paths);
    let (quota, random, reset) = (|| false, || 0.5, || {});
    let env = BatchEnv {
        engine: &w.engine,
        settings: &w.settings,
        stop: w.stop.clone(),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &sink,
        reporter: Arc::new(StopOnCountdown(w.stop.clone())),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &|s| hatate_core::hydrus_client::HydrusClient::new(s),
    };
    let mut entries = Vec::new();
    let outcome = run_batch(&env, &dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    assert!(
        w.clock.slept() < 1.0,
        "woke within a step, not after 600 s: {}",
        w.clock.slept()
    );
    assert_eq!(w.saved().len(), 2);
}

#[tokio::test]
async fn the_session_is_autosaved_on_its_interval_during_a_long_wait() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 60.0;
        s.delay_max_seconds = 60.0;
        s.autosave_interval_seconds = 10;
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let mut entries = Vec::new();
    let (_, sink) = w.run(&dir, &mut entries).await;
    // Added (1) + about every 10 s of a 60 s wait (5) + the end (1).
    let saves = sink.saves.load(Ordering::SeqCst);
    assert!((6..=9).contains(&saves), "{saves}");
}

#[tokio::test]
async fn autosave_off_saves_when_files_are_added_and_at_the_end_only() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 60.0;
        s.delay_max_seconds = 60.0;
        s.autosave_session = false;
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let mut entries = Vec::new();
    let (_, sink) = w.run(&dir, &mut entries).await;
    assert_eq!(sink.saves.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_interval_under_ten_seconds_is_ten() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 60.0;
        s.delay_max_seconds = 60.0;
        s.autosave_interval_seconds = 1;
    }
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let mut entries = Vec::new();
    let (_, sink) = w.run(&dir, &mut entries).await;
    assert!(
        sink.saves.load(Ordering::SeqCst) <= 9,
        "{}",
        sink.saves.load(Ordering::SeqCst)
    );
}

/// SauceNAO's daily allowance runs out after the first image while it is in the engine order:
/// the run pauses, says how many are left, records the pause file, and the rest stay unsearched.
#[tokio::test]
async fn an_exhausted_saucenao_quota_pauses_the_run_and_records_it() {
    let w = world().await;
    w.settings.lock().unwrap().secondary_engine_mode = "always".to_owned();
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    w.quota_after_calls.store(1, Ordering::SeqCst);
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(
        outcome.ending,
        Ending::PausedForQuota {
            searched: 1,
            remaining: 2
        }
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.status == MatchStatus::NotSearched)
            .count(),
        2
    );
    let pause = QuotaPauseFile::new(&w.paths)
        .read()
        .expect("the pause is on disk");
    assert_eq!((pause.searched, pause.remaining), (1, 2));
    assert_eq!(w.saved().len(), 3);
}

#[tokio::test]
async fn a_new_run_forgets_the_old_pause_file() {
    let w = world().await;
    let file = QuotaPauseFile::new(&w.paths);
    file.record(&hatate_core::quota_pause::QuotaPause::starting_at(
        NOW, 5, 5,
    ))
    .unwrap();
    let dir = w.pictures(&["a.png"]);
    w.match_every_image(1);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    assert!(file.read().is_none());
}

#[tokio::test]
async fn with_saucenao_out_of_the_order_its_quota_is_irrelevant() {
    let w = world().await; // iqdb only, no secondary
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    w.quota_after_calls.store(0, Ordering::SeqCst);
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(outcome.searched, 2);
}

#[tokio::test]
async fn continue_without_saucenao_carries_on_and_says_so_once() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.secondary_engine_mode = "always".to_owned();
        s.continue_without_saucenao_on_quota = true;
    }
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    w.quota_after_calls.store(1, Ordering::SeqCst);
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(outcome.searched, 3);
    let said = w
        .lines()
        .iter()
        .filter(|l| l.contains("carrying on without it"))
        .count();
    assert_eq!(said, 1);
    // `test_the_later_images_are_searched_without_saucenao`: asked once, while the allowance was
    // still there, and not again for the rest of the batch.
    assert_eq!(
        w.engines.calls().iter().filter(|c| **c == "saucenao").count(),
        1,
        "{:?}",
        w.engines.calls()
    );
    assert!(QuotaPauseFile::new(&w.paths).read().is_none());
    // `test_those_results_are_marked_provisional`: the images searched without SauceNAO say so,
    // and are kept out of the cache so a later run searches them in full.
    assert_eq!(
        entries
            .iter()
            .map(|e| e.searched_without_saucenao)
            .collect::<Vec<_>>(),
        [false, true, true]
    );
    assert_eq!(
        std::fs::read_dir(w.paths.search_cache_dir())
            .unwrap()
            .count(),
        1,
        "only the complete result is cached"
    );
}

/// `test_stopping_cancels_the_retry_pass`: a retry after Stop would spend the pacing delay to
/// fail the same way.
#[tokio::test]
async fn stopping_cancels_the_retry_pass() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("timeout".to_owned()))));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    *w.reporter.stop_on.lock().unwrap() = Some(("(2/2)".to_owned(), w.stop.clone()));
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    assert_eq!(outcome.searched, 2, "no third attempt");
    assert_eq!(entries[0].status, MatchStatus::Error);
}

/// `test_an_exhausted_daily_quota_cancels_the_retry_pass`: with the allowance spent a retry fails
/// the same way, so none is queued.
#[tokio::test]
async fn an_exhausted_quota_cancels_the_retry_pass() {
    let w = world().await;
    w.settings.lock().unwrap().secondary_engine_mode = "always".to_owned();
    let dir = w.pictures(&["a.png"]);
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("timeout".to_owned()))));
    w.engines
        .saucenao
        .push(Step::Return(Err(SauceNaoError("timeout".to_owned()))));
    // Both engines have been asked once when the first pass ends: the allowance is spent.
    w.quota_after_calls.store(2, Ordering::SeqCst);
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(entries[0].status, MatchStatus::Error);
    assert_eq!(outcome.searched, 1, "not retried");
}

/// `test_progress_accounts_for_the_extra_attempts`: the total grows when retries are queued, so
/// the count stays honest.
#[tokio::test]
async fn progress_accounts_for_the_extra_attempts() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("timeout".to_owned()))));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    let lines = w.lines();
    assert!(
        lines.iter().any(|l| l.starts_with("(1/2) a.png")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("(2/2) b.png")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("(3/3) a.png")),
        "{lines:?}"
    );
}

/// `test_the_search_worker_spends_the_flag`: a row the last run died on is searched again and
/// the note about it is spent.
#[tokio::test]
async fn a_search_spends_the_interrupted_note() {
    let w = world().await;
    let dir = w.pictures(&["a.png"]);
    let mut row = Entry::new(dir.join("a.png").to_string_lossy());
    row.interrupted_mid_search = true;
    let mut entries = vec![row];
    w.match_every_image(1);
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(
        (outcome.added, outcome.duplicates, outcome.searched),
        (0, 1, 1)
    );
    assert!(!entries[0].interrupted_mid_search);
    assert_eq!(entries[0].status, MatchStatus::Good);
}

#[tokio::test]
async fn hash_source_local_reads_every_file_and_a_lying_name_does_not_fool_it() {
    let w = world().await;
    w.settings.lock().unwrap().hash_source = "local".to_owned();
    let dir = w.pictures(&["a.png"]);
    let liar = dir.join(format!("{}.png", "ab".repeat(32)));
    std::fs::copy(dir.join("a.png"), &liar).unwrap();
    let mut entries = Vec::new();
    let (outcome, _) = w.run(&dir, &mut entries).await;
    assert_eq!(
        (outcome.added, outcome.duplicates),
        (1, 1),
        "same bytes, so a duplicate by real hash"
    );
}

// ---------------------------------------------------------------------------------------------
// The RunState channel (DAN-2216): one run per ending, through the real loop.
// ---------------------------------------------------------------------------------------------

/// A channel over the world's clock, and everything it says, in order.
fn channel(w: &World) -> (RunChannel, Arc<Mutex<Vec<RunEvent>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink: RunSink = {
        let seen = Arc::clone(&seen);
        Arc::new(move |e| seen.lock().unwrap().push(e))
    };
    let ch = RunChannel::new(sink, w.clock.clone() as Arc<dyn Clock>)
        .with_stop_record(StopRecordFile::new(&w.paths))
        .with_next(w.reporter.clone());
    (ch, seen)
}

/// The states only (the Lens changes are filtered out), as `(done, total, current)` for a
/// running state and a tag for the rest.
fn shape(events: &[RunEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            RunEvent::State(RunState::Running(p)) => Some(format!(
                "running {}/{} {}",
                p.done,
                p.total,
                p.current.as_deref().unwrap_or("-")
            )),
            RunEvent::State(RunState::Waiting(w)) => Some(format!(
                "waiting {} {}/{}",
                w.label, w.progress.done, w.progress.total
            )),
            RunEvent::State(RunState::PausedForQuota {
                searched,
                remaining,
            }) => Some(format!("paused {searched} {remaining}")),
            RunEvent::State(RunState::Ended(end)) => Some(format!("ended {end:?}")),
            RunEvent::State(RunState::Idle) => Some("idle".to_owned()),
            RunEvent::Lens(_) => None,
        })
        .collect()
}

/// B1 and the Python worker's signals in order: `image_updated(searching)`, `progress`, ... and
/// `finished_all`, for a run that completes.
#[tokio::test]
async fn a_completed_run_says_running_for_each_image_then_completed() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    let (outcome, _) = w.run_with(Arc::new(ch.clone()), &dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(
        shape(&seen.lock().unwrap()),
        [
            "running 0/3 a.png",
            "running 1/3 a.png",
            "running 1/3 b.png",
            "running 2/3 b.png",
            "running 2/3 c.png",
            "running 3/3 c.png",
            "ended Completed",
        ]
    );
    assert_eq!(ch.state(), RunState::Ended(RunEnd::Completed));
    assert!(
        !StopRecordFile::new(&w.paths).path().exists(),
        "a completed run leaves no stop record"
    );
}

#[tokio::test]
async fn the_gap_between_images_is_a_waiting_state_that_goes_back_to_running_and_the_eta_follows_the_pace()
 {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.delay_min_seconds = 40.0;
        s.delay_max_seconds = 80.0; // random 0.5 -> 60 s
    }
    let dir = w.pictures(&["a.png", "b.png", "c.png", "d.png", "e.png"]);
    w.match_every_image(5);
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    w.run_with(Arc::new(ch), &dir, &mut entries).await;
    let events = seen.lock().unwrap().clone();
    let waits: Vec<&hatate_core::run_state::Wait> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::State(RunState::Waiting(w)) => Some(w),
            _ => None,
        })
        .collect();
    assert!(!waits.is_empty());
    assert!(
        waits
            .iter()
            .all(|w| w.label == "Rate limit" && (w.total - 60.0).abs() < 1.0)
    );
    assert_eq!(waits[0].progress.done, 1);
    // The wait ends with the clearing call, which is a running state again before the next image.
    let shapes = shape(&events);
    let last_wait = shapes
        .iter()
        .rposition(|s| s.starts_with("waiting"))
        .unwrap();
    assert!(shapes[last_wait + 1].starts_with("running"), "{shapes:?}");
    // The pace is unknown for the first intervals (`eta_seconds` is `None`), then it is one gap of
    // about 60 s per image: after the fourth image one is left.
    let eta_at = |done: i64| {
        events.iter().rev().find_map(|e| match e {
            RunEvent::State(RunState::Running(p)) if p.done == done => Some(p.eta_seconds),
            _ => None,
        })
    };
    assert_eq!(eta_at(1), Some(None), "one signal is no pace");
    let eta = eta_at(4)
        .unwrap()
        .expect("three intervals give an estimate");
    assert!((eta - 60.0).abs() < 2.0, "eta {eta}");
}

#[tokio::test]
async fn a_stop_says_interrupted_and_leaves_a_record_in_hatate_rs_json_only() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    *w.reporter.stop_on.lock().unwrap() = Some(("(1/3)".to_owned(), w.stop.clone()));
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    let (outcome, _) = w.run_with(Arc::new(ch), &dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    let shapes = shape(&seen.lock().unwrap());
    assert_eq!(shapes.last().unwrap(), "ended Interrupted", "{shapes:?}");
    let StopRead::Valid(record) = StopRecordFile::new(&w.paths).read() else {
        panic!("a stop writes the record");
    };
    assert_eq!(record.unsearched, 2);
    assert_eq!(record.at, NOW);

    // Rust-only: the Python files are what the run left them, byte for byte, after the record is
    // written and again after it is cleared.
    let python_files = |w: &World| {
        [
            w.paths.session_db(),
            w.paths.session_json(),
            w.paths.config_file(),
        ]
        .map(|p| std::fs::read(p).ok())
    };
    let before = python_files(&w);
    let rec = StopRecordFile::new(&w.paths);
    rec.write(&hatate_core::stop_record::StopRecord {
        at: 1.0,
        unsearched: 9,
    })
    .unwrap();
    rec.clear().unwrap();
    assert_eq!(python_files(&w), before);
    assert!(!rec.path().exists());
}

#[tokio::test]
async fn starting_a_run_clears_the_stop_record() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    let rec = StopRecordFile::new(&w.paths);
    rec.write(&hatate_core::stop_record::StopRecord {
        at: 5.0,
        unsearched: 1,
    })
    .unwrap();
    let (ch, _) = channel(&w);
    let mut entries = Vec::new();
    w.run_with(Arc::new(ch), &dir, &mut entries).await;
    assert_eq!(
        rec.read(),
        StopRead::Absent,
        "the first image of a run clears it"
    );
}

#[tokio::test]
async fn a_stop_that_cannot_be_recorded_is_still_a_stop() {
    let w = world().await;
    let dir = w.pictures(&["a.png", "b.png"]);
    w.match_every_image(2);
    // `hatate-rs.json` is a directory: the write fails, and the run neither fails nor changes state.
    std::fs::create_dir_all(w.paths.rust_settings_file()).unwrap();
    *w.reporter.stop_on.lock().unwrap() = Some(("(1/2)".to_owned(), w.stop.clone()));
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    let (outcome, _) = w.run_with(Arc::new(ch), &dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    assert_eq!(
        shape(&seen.lock().unwrap()).last().unwrap(),
        "ended Interrupted"
    );
    assert!(!outcome.save_failed);
}

#[tokio::test]
async fn an_exhausted_quota_is_a_paused_state_not_an_ending() {
    let w = world().await;
    w.settings.lock().unwrap().secondary_engine_mode = "always".to_owned();
    let dir = w.pictures(&["a.png", "b.png", "c.png"]);
    w.match_every_image(3);
    w.quota_after_calls.store(1, Ordering::SeqCst);
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    let (outcome, _) = w.run_with(Arc::new(ch.clone()), &dir, &mut entries).await;
    assert!(matches!(outcome.ending, Ending::PausedForQuota { .. }));
    let shapes = shape(&seen.lock().unwrap());
    assert_eq!(shapes.last().unwrap(), "paused 1 2", "{shapes:?}");
    assert_eq!(
        shapes.iter().filter(|s| s.starts_with("paused")).count(),
        1,
        "said once, however many hooks heard it: {shapes:?}"
    );
    assert_eq!(
        ch.state(),
        RunState::PausedForQuota {
            searched: 1,
            remaining: 2
        }
    );
    assert_eq!(StopRecordFile::new(&w.paths).read(), StopRead::Absent);
}

/// Failed: the staging directory cannot be made (a file is where it should be), so the worker
/// dies before the first image. The state carries the path-free message and nothing is recorded.
#[tokio::test]
async fn a_worker_that_dies_is_failed_with_a_message_that_names_no_path() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.auto_import_enabled = true;
        s.auto_import_method = "download_send".to_owned();
        s.hydrus.access_key = hatate_core::config::Secret::new("0123456789abcdef");
    }
    let staging = w.paths.auto_import_staging_dir();
    std::fs::create_dir_all(staging.parent().unwrap()).unwrap();
    std::fs::write(&staging, b"a file where the directory should be").unwrap();
    let dir = w.pictures(&["a.png"]);
    w.match_every_image(1);
    let (ch, seen) = channel(&w);
    let mut entries = Vec::new();
    let (outcome, _) = w.run_with(Arc::new(ch.clone()), &dir, &mut entries).await;
    let Ending::Failed(fault) = &outcome.ending else {
        panic!("{:?}", outcome.ending);
    };
    let RunState::Ended(RunEnd::Failed(message)) = ch.state() else {
        panic!("{:?}", ch.state());
    };
    assert_eq!(message, fault.user_message());
    let path = staging.to_string_lossy().into_owned();
    assert!(
        !message.contains(&path) && !message.contains('/') && !message.contains("os error"),
        "{message}"
    );
    assert!(
        !shape(&seen.lock().unwrap())
            .iter()
            .any(|s| s.contains("Interrupted"))
    );
    assert_eq!(
        StopRecordFile::new(&w.paths).read(),
        StopRead::Absent,
        "Failed is never persisted"
    );
}

// ---------------------------------------------------------------------------------------------
// What a window launch can set (DAN-2218): `search_pending_with`.
// ---------------------------------------------------------------------------------------------

use hatate_core::batch::{PendingOptions, search_pending_with};

/// Runs `search_pending_with` over `entries` (all of them pending) with `reporter` in the loop's
/// hands, the world's quota flag and a manual clock.
async fn pending_with(
    w: &World,
    entries: &mut [Entry],
    reporter: Arc<dyn Reporter>,
    opts: &PendingOptions,
) -> hatate_core::batch::SearchEnd {
    let quota_file = QuotaPauseFile::new(&w.paths);
    let engines = w.engines.clone();
    let threshold = &w.quota_after_calls;
    let quota = move || engines.calls().len() >= threshold.load(Ordering::SeqCst);
    let random = || 0.5;
    let reset = || {};
    let env = BatchEnv {
        engine: &w.engine,
        settings: &w.settings,
        stop: w.stop.clone(),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &w.sink(),
        reporter: reporter.clone(),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &|s| hatate_core::hydrus_client::HydrusClient::new(s),
    };
    let mut autosave = hatate_core::batch::Autosave::new(env.sink, &*reporter, 0.0);
    let pending = (0..entries.len()).collect();
    search_pending_with(&env, entries, pending, &mut autosave, opts).await
}

/// Keeps `updated`, `progress` and `continuing_without_saucenao` in the order they arrive.
#[derive(Default)]
struct Hooks(Mutex<Vec<String>>);

impl Reporter for Hooks {
    fn line(&self, _text: &str) {}
    fn countdown(&self, _label: &str, _remaining: f64, _total: f64) {}
    fn updated(&self, index: usize, entry: &Entry) {
        self.0
            .lock()
            .unwrap()
            .push(format!("updated {index} {}", entry.status.as_str()));
    }
    fn progress(&self, done: usize, total: usize) {
        self.0
            .lock()
            .unwrap()
            .push(format!("progress {done}/{total}"));
    }
    fn continuing_without_saucenao(&self, searched: i64, remaining: i64) {
        self.0
            .lock()
            .unwrap()
            .push(format!("continuing {searched} {remaining}"));
    }
}

fn rows(w: &World, names: &[&str]) -> Vec<Entry> {
    let dir = w.pictures(names);
    names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut e = Entry::new(dir.join(n).to_string_lossy());
            e.hydrus_hash = Some(format!("{:064x}", i + 1));
            e
        })
        .collect()
}

/// `SearchWorker.image_updated` after a search comes before `progress`, once per image, with the
/// searched entry and its place in the slice.
#[tokio::test]
async fn updated_comes_after_each_search_and_before_its_progress() {
    let w = world().await;
    let mut entries = rows(&w, &["a.png", "b.png"]);
    let hooks = Arc::new(Hooks::default());
    let end = pending_with(&w, &mut entries, hooks.clone(), &PendingOptions::default()).await;
    assert_eq!(end.ending, Ending::Finished);
    assert_eq!(
        *hooks.0.lock().unwrap(),
        [
            "updated 0 not_found",
            "progress 1/2",
            "updated 1 not_found",
            "progress 2/2"
        ]
    );
}

/// `override_engines`: the named engine runs for that row whatever the configured order is.
#[tokio::test]
async fn an_override_engine_replaces_the_configured_order_for_its_row_only() {
    let w = world().await;
    let mut entries = rows(&w, &["a.png", "b.png"]);
    let opts = PendingOptions {
        override_engines: [(1usize, "saucenao".to_owned())].into_iter().collect(),
        ..PendingOptions::default()
    };
    pending_with(&w, &mut entries, Arc::new(Hooks::default()), &opts).await;
    assert_eq!(w.engines.calls(), ["iqdb", "saucenao"]);
}

/// `bypass_cache`: a re-search does not read the cache the first search wrote.
#[tokio::test]
async fn bypass_cache_searches_again_what_the_cache_holds() {
    let w = world().await;
    w.settings.lock().unwrap().use_search_cache = true;
    let mut entries = rows(&w, &["a.png"]);
    pending_with(
        &w,
        &mut entries,
        Arc::new(Hooks::default()),
        &PendingOptions::default(),
    )
    .await;
    assert_eq!(w.engines.calls().len(), 1);
    pending_with(
        &w,
        &mut entries,
        Arc::new(Hooks::default()),
        &PendingOptions::default(),
    )
    .await;
    assert_eq!(w.engines.calls().len(), 1, "the second search was cached");
    pending_with(
        &w,
        &mut entries,
        Arc::new(Hooks::default()),
        &PendingOptions {
            bypass_cache: true,
            ..PendingOptions::default()
        },
    )
    .await;
    assert_eq!(w.engines.calls().len(), 2, "a bypassing search asks again");
}

/// `force_continue_without_saucenao`: the one-shot override finishes the batch despite the saved
/// setting being off, says so once, and without it the run pauses.
#[tokio::test]
async fn the_one_shot_override_continues_without_saucenao_and_without_it_the_run_pauses() {
    let w = world().await;
    {
        let mut s = w.settings.lock().unwrap();
        s.secondary_engine_mode = "always".to_owned();
        s.continue_without_saucenao_on_quota = false;
    }
    w.quota_after_calls.store(1, Ordering::SeqCst);
    let mut entries = rows(&w, &["a.png", "b.png", "c.png"]);
    let end = pending_with(
        &w,
        &mut entries,
        Arc::new(Hooks::default()),
        &PendingOptions::default(),
    )
    .await;
    assert_eq!(
        end.ending,
        Ending::PausedForQuota {
            searched: 1,
            remaining: 2
        }
    );

    let mut entries = rows(&w, &["a.png", "b.png", "c.png"]);
    let hooks = Arc::new(Hooks::default());
    let opts = PendingOptions {
        force_continue_without_saucenao: true,
        ..PendingOptions::default()
    };
    let end = pending_with(&w, &mut entries, hooks.clone(), &opts).await;
    assert_eq!(end.ending, Ending::Finished);
    assert_eq!(end.searched, 3);
    assert_eq!(
        hooks
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("continuing"))
            .collect::<Vec<_>>(),
        ["continuing 1 2"]
    );
    assert!(
        !w.settings
            .lock()
            .unwrap()
            .continue_without_saucenao_on_quota
    );
}
