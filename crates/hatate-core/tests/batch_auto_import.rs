//! Hydrus auto-import in `hatate batch` (Phase 4.g, DAN-2132): `_maybe_auto_import`,
//! `_switch_to_importable_match`, the staging directory with its `owner.pid`, the sweep of the
//! ones an earlier run left, and the rows `remove_after_import` drops. The Hydrus client is the
//! binary's own constructor (`HydrusClient::new`: rustls with the explicit provider), plain http
//! and https; every Hydrus answer comes from a scripted local server, every access key is a
//! placeholder, and nothing leaves the machine.
//!
//! The end-to-end session differential against the real Python worker is the broad check
//! (`worktrees/DAN-2132-differential.log`); these are the named, readable ones. The Python test
//! each maps to is in `docs/batch.md`.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use common::{Recorded, Reply, ScriptedEngines, Step, TestPki, TestServer, iqdb_match};
use hatate_core::batch::{
    BatchEnv, Ending, Outcome, Reporter, RunFault, SessionSink, maybe_auto_import, run_batch,
    switch_to_importable_match,
};
use hatate_core::clock::ManualClock;
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::engine_runner::EngineRunner;
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{HydrusClient, HydrusError};
use hatate_core::hydrus_import::ImportResult;
use hatate_core::model::{Candidate, Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;
use hatate_core::session::{LoadOptions, SessionStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const NOW: f64 = 1_792_725_790.0;
/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const NEW: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const POST: &str = r#"{"id": 5, "image_width": 800, "image_height": 600, "rating": "g",
    "file_url": "https://cdn.donmai.us/f.png",
    "tag_string_general": "one two three four five six", "tag_string_artist": "someone"}"#;
const URL: &str = "https://danbooru.donmai.us/posts/5";
const OTHER_URL: &str = "https://gelbooru.com/index.php?page=post&s=view&id=9";
const PAGE_URL: &str = "https://hdporncomics.com/some-comic/";

fn json_reply(body: Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn path_of(q: &Recorded) -> &str {
    q.target.split('?').next().unwrap_or("")
}

fn url_param(q: &Recorded) -> String {
    let query = q.target.split_once('?').map_or("", |(_, q)| q);
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// A scripted Hydrus: `get_url_info` answers by URL (anything else is an unknown URL), an upload
/// comes back under its own hash, and the URL import is confirmed with `NEW` unless told not to.
#[derive(Clone)]
struct Fake {
    info: HashMap<String, Value>,
    confirms: bool,
    fail: Vec<&'static str>,
    wrong_shape: Vec<&'static str>,
}

impl Fake {
    fn new() -> Self {
        Fake {
            info: HashMap::new(),
            confirms: true,
            fail: Vec::new(),
            wrong_shape: Vec::new(),
        }
    }

    fn knows(mut self, url: &str, answer: Value) -> Self {
        self.info.insert(url.to_owned(), answer);
        self
    }

    fn answer(&self, q: &Recorded) -> Reply {
        let path = path_of(q);
        if self.fail.iter().any(|f| path.ends_with(f)) {
            return Reply::new(500, "boom");
        }
        if self.wrong_shape.iter().any(|f| path.ends_with(f)) {
            return json_reply(json!([1, 2, 3]));
        }
        match path {
            "/add_urls/get_url_info" => json_reply(
                self.info
                    .get(&url_param(q))
                    .cloned()
                    .unwrap_or(json!({"url_type": 5, "url_type_string": "unknown url",
                       "match_name": "unknown url", "can_parse": false})),
            ),
            "/add_urls/add_url" => {
                json_reply(json!({"human_result_text": "URL added successfully."}))
            }
            "/add_urls/get_url_files" => {
                if self.confirms {
                    json_reply(json!({"url_file_statuses": [{"status": 2, "hash": NEW}]}))
                } else {
                    json_reply(json!({"url_file_statuses": []}))
                }
            }
            // The imported file read back out, for the duplicate comparison: the local picture.
            "/get_files/file" => Reply::ok(png_bytes(1)).header("Content-Type", "image/png"),
            "/add_files/add_file" => json_reply(json!({"status": 1, "hash": hex_sha256(&q.body)})),
            "/verify_access_key" => json_reply(json!({"permits_everything": true})),
            // Every hash asked about is held, under a file id of its own.
            "/get_files/file_metadata" => {
                let query = q.target.split_once('?').map_or("", |(_, q)| q);
                let hashes: Vec<String> = url::form_urlencoded::parse(query.as_bytes())
                    .find(|(k, _)| k == "hashes")
                    .and_then(|(_, v)| serde_json::from_str(&v).ok())
                    .unwrap_or_default();
                let metadata: Vec<Value> = hashes
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        json!({"hash": h, "file_id": 7 + i, "is_local": true,
                                         "is_trashed": false, "is_deleted": false})
                    })
                    .collect();
                json_reply(json!({ "metadata": metadata }))
            }
            _ => json_reply(json!({})),
        }
    }
}

/// What a Hydrus request saw when it arrived, including the staging directory beside it.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    staging: Vec<String>,
}

struct Hydrus {
    server: TestServer,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Hydrus {
    async fn http(fake: Fake, staging_root: PathBuf) -> Hydrus {
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let log = seen.clone();
        let server = TestServer::http(move |q| {
            log.lock().unwrap().push(Seen {
                path: path_of(q).to_owned(),
                staging: listing(&staging_root),
            });
            fake.answer(q)
        })
        .await;
        Hydrus { server, seen }
    }

    fn paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.path.clone())
            .collect()
    }

    fn count(&self, path: &str) -> usize {
        self.paths().iter().filter(|p| *p == path).count()
    }
}

/// The names of the staging directories under `root`, each with its files: `dir/file, ...`.
fn listing(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(root) else {
        return out;
    };
    for dir in dirs.flatten() {
        let mut files: Vec<String> = std::fs::read_dir(dir.path())
            .map(|rd| {
                rd.flatten()
                    .map(|f| f.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        out.push(format!(
            "{}/{}",
            dir.file_name().to_string_lossy(),
            files.join(",")
        ));
    }
    out.sort();
    out
}

fn png_bytes(seed: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(40, 40);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let v = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503) ^ seed.wrapping_mul(97);
        *p = image::Rgb([(v >> 3) as u8, (v >> 9) as u8, (v >> 15) as u8]);
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[derive(Default)]
struct Recording {
    lines: Mutex<Vec<String>>,
    imported: Mutex<Vec<(String, bool)>>,
}

impl Reporter for Recording {
    fn line(&self, text: &str) {
        self.lines.lock().unwrap().push(text.to_owned());
    }
    fn countdown(&self, _: &str, _: f64, _: f64) {}
    fn auto_imported(&self, entry: &Entry, result: &ImportResult) {
        self.imported
            .lock()
            .unwrap()
            .push((entry.filename().to_owned(), result.success));
    }
}

struct Sink(hatate_core::session::SessionWriter);

impl SessionSink for Sink {
    fn save(&self, entries: &[Entry], now: f64) -> Result<(), String> {
        self.0.save(entries, now).map_err(|e| e.to_string())
    }
}

/// A profile, a booru and a CDN on one local https server, scripted engines, a Hydrus on http.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
    _sites: TestServer,
    engine: SearchEngine,
    engines: Arc<ScriptedEngines>,
    hydrus: Hydrus,
    settings: Mutex<Settings>,
    reporter: Arc<Recording>,
    stop: Arc<AtomicBool>,
    net_config: NetConfig,
}

async fn world(fake: Fake) -> World {
    let hosts = ["danbooru.donmai.us", "cdn.donmai.us"];
    let pki = TestPki::generate_for(&hosts);
    let cdn_bytes = png_bytes(99);
    let sites = TestServer::https(&pki, move |req: &Recorded| {
        if req.target.contains("/posts/5.json") {
            Reply::ok(POST).header("Content-Type", "application/json")
        } else if req.target.contains("/f.png") {
            Reply::ok(cdn_bytes.clone()).header("Content-Type", "image/png")
        } else {
            Reply::new(404, "no route")
        }
    })
    .await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, sites.addr);
    }
    let net = NetClient::new(cfg.clone()).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("dan2132-")
        .tempdir()
        .unwrap();
    let root = dir.path().to_path_buf();
    let home: std::ffi::OsString = root.join("home").into();
    let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
    let hydrus = Hydrus::http(fake, paths.auto_import_staging_dir()).await;
    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let engines = Arc::new(ScriptedEngines::default());
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = SearchEngine::with_recorder(net, runner, paths.clone(), clock, None);
    let mut settings = Settings::with_defaults(&paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive_index = false;
    settings.match_conditions.require_larger_than_local = false;
    settings.delay_min_seconds = 0.0;
    settings.delay_max_seconds = 0.0;
    settings.log_matched_urls = false;
    settings.auto_import_enabled = true;
    settings.auto_import_method = "upload".to_owned();
    settings.auto_import_min_similarity = 90.0;
    settings.hydrus = HydrusSettings {
        api_url: hydrus.server.url(""),
        access_key: Secret::new(KEY),
        timeout: 5.0,
        ..HydrusSettings::default()
    };
    World {
        _dir: dir,
        root,
        paths,
        _sites: sites,
        engine,
        engines,
        hydrus,
        settings: Mutex::new(settings),
        reporter: Arc::new(Recording::default()),
        stop: Arc::new(AtomicBool::new(false)),
        net_config: cfg,
    }
}

impl World {
    fn set(&self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings.lock().unwrap());
    }

    fn staging(&self) -> PathBuf {
        self.paths.auto_import_staging_dir()
    }

    fn pictures(&self, n: usize) -> PathBuf {
        let dir = self.root.join("pics");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("p{i}.png")), png_bytes(i as u32 + 1)).unwrap();
        }
        dir
    }

    fn match_every_image(&self, n: usize, similarity: f64) {
        for _ in 0..n {
            self.engines
                .iqdb
                .push(Step::Return(Ok(vec![iqdb_match(URL, similarity)])));
        }
    }

    /// The batch's own constructor for the client, as the binary passes it.
    async fn run(&self, folder: &Path, entries: &mut Vec<Entry>) -> Outcome {
        let writer = SessionStore::new(&self.paths).open_writer().unwrap();
        let sink = Sink(writer);
        let quota_file = QuotaPauseFile::new(&self.paths);
        let (quota, random, reset) = (|| false, || 0.5, || {});
        let env = BatchEnv {
            engine: &self.engine,
            settings: &self.settings,
            stop: self.stop.clone(),
            quota_exhausted: &quota,
            quota_file: &quota_file,
            sink: &sink,
            reporter: self.reporter.clone(),
            unit_random: &random,
            reset_run_state: &reset,
            hydrus_client: &|s: &HydrusSettings| HydrusClient::new(s),
        };
        run_batch(&env, folder, entries).await
    }

    /// `worker._maybe_auto_import(entry, client, tmp)` called directly, as the Python tests do.
    async fn import(
        &self,
        entry: &mut Entry,
        tmp: Option<&Path>,
    ) -> Result<Option<ImportResult>, RunFault> {
        let writer = SessionStore::new(&self.paths).open_writer().unwrap();
        let sink = Sink(writer);
        let quota_file = QuotaPauseFile::new(&self.paths);
        let (quota, random, reset) = (|| false, || 0.5, || {});
        let env = BatchEnv {
            engine: &self.engine,
            settings: &self.settings,
            stop: self.stop.clone(),
            quota_exhausted: &quota,
            quota_file: &quota_file,
            sink: &sink,
            reporter: self.reporter.clone(),
            unit_random: &random,
            reset_run_state: &reset,
            hydrus_client: &|s: &HydrusSettings| HydrusClient::new(s),
        };
        let client = HydrusClient::new(&self.settings.lock().unwrap().hydrus).unwrap();
        maybe_auto_import(&env, entry, &client, tmp).await
    }

    fn saved(&self) -> Vec<Entry> {
        SessionStore::new(&self.paths)
            .load(&LoadOptions::at(NOW))
            .entries
    }

    fn lines(&self) -> Vec<String> {
        self.reporter.lines.lock().unwrap().clone()
    }

    /// An entry for a real file, searched, selected on `URL` at `similarity`.
    fn entry(&self, similarity: f64) -> Entry {
        let path = self.root.join("local.png");
        std::fs::write(&path, png_bytes(1)).unwrap();
        let mut e = Entry::new(path.to_str().unwrap());
        e.hydrus_hash = Some(hex_sha256(&png_bytes(1)));
        e.status = MatchStatus::Good;
        let mut c = Candidate::new(URL);
        c.similarity = similarity;
        c.similarity_measured = true;
        c.booru_tags_fetched = true;
        e.candidates.push(c);
        e.select_candidate(0);
        e
    }
}

fn candidate(url: &str, similarity: f64) -> Candidate {
    let mut c = Candidate::new(url);
    c.similarity = similarity;
    c.similarity_measured = true;
    c.booru_tags_fetched = true;
    c
}

fn knows_post(fake: Fake, url: &str) -> Fake {
    fake.knows(
        url,
        json!({"url_type": 0, "url_type_string": "post url", "match_name": "a file page",
               "can_parse": true}),
    )
}

// ---- the early returns of _maybe_auto_import ----

#[tokio::test]
async fn nothing_is_sent_for_a_row_already_sent_not_matched_or_below_the_threshold() {
    let w = world(Fake::new()).await;
    let mut sent = w.entry(99.0);
    sent.sent_to_hydrus = true;
    let mut not_found = w.entry(99.0);
    not_found.status = MatchStatus::NotFound;
    let mut weak = w.entry(89.9);
    let mut no_match = Entry::new(w.entry(1.0).path);
    no_match.status = MatchStatus::Good;
    for e in [&mut sent, &mut not_found, &mut weak, &mut no_match] {
        assert!(w.import(e, None).await.unwrap().is_none(), "{}", e.path);
    }
    assert!(!weak.sent_to_hydrus);
    assert_eq!(w.hydrus.paths(), Vec::<String>::new(), "no request at all");
}

#[tokio::test]
async fn a_score_exactly_at_the_threshold_is_sent() {
    let w = world(Fake::new()).await;
    let mut e = w.entry(90.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(result.success && e.sent_to_hydrus);
}

#[tokio::test]
async fn a_match_whose_source_is_gone_is_skipped_without_a_request() {
    let w = world(Fake::new()).await;
    w.set(|s| s.auto_import_method = "url_importer".to_owned());
    let mut e = w.entry(99.0);
    e.candidates[0].remote_available = Some(false);
    assert!(w.import(&mut e, None).await.unwrap().is_none());
    assert_eq!(w.hydrus.paths(), Vec::<String>::new());
    // Unknown is not gone.
    e.candidates[0].remote_available = None;
    assert!(w.import(&mut e, None).await.unwrap().is_some());
}

// ---- the three methods ----

#[tokio::test]
async fn the_upload_method_sends_the_local_file_and_marks_the_row() {
    let w = world(Fake::new()).await;
    let mut e = w.entry(97.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(result.success, "{result:?}");
    assert!(e.sent_to_hydrus && e.hydrus_import_confirmed);
    assert_eq!(w.hydrus.count("/add_files/add_file"), 1);
    let sent = w.hydrus.server.recorded();
    assert!(!sent.is_empty());
    assert!(
        sent.iter()
            .all(|q| q.header("hydrus-client-api-access-key") == Some(KEY)),
        "the key goes out as the header on every request"
    );
    assert!(
        !w.staging().exists(),
        "the upload method makes no staging directory"
    );
}

#[tokio::test]
async fn an_unknown_method_name_uploads_like_python_s_else_branch() {
    let w = world(Fake::new()).await;
    w.set(|s| s.auto_import_method = "something-else".to_owned());
    let mut e = w.entry(97.0);
    assert!(w.import(&mut e, None).await.unwrap().unwrap().success);
    assert_eq!(w.hydrus.count("/add_files/add_file"), 1);
}

#[tokio::test]
async fn download_send_downloads_into_the_staging_directory_and_sends_those_bytes() {
    let w = world(Fake::new()).await;
    w.set(|s| s.auto_import_method = "download_send".to_owned());
    let mut e = w.entry(97.0);
    e.candidates[0].direct_file_url = Some("https://cdn.donmai.us/f.png".to_owned());
    let tmp = tempfile::tempdir().unwrap();
    let result = w.import(&mut e, Some(tmp.path())).await.unwrap().unwrap();
    assert!(result.success, "{result:?}");
    let sent = &w.hydrus.server.recorded()[0];
    assert_eq!(
        hex_sha256(&sent.body),
        hex_sha256(&png_bytes(99)),
        "the downloaded bytes, not the local file"
    );
    assert!(e.sent_to_hydrus);
}

#[tokio::test]
async fn download_send_without_a_staging_directory_does_nothing() {
    let w = world(Fake::new()).await;
    w.set(|s| s.auto_import_method = "download_send".to_owned());
    let mut e = w.entry(97.0);
    assert!(w.import(&mut e, None).await.unwrap().is_none());
    assert_eq!(w.hydrus.paths(), Vec::<String>::new());
}

// ---- url_importer: the confirmation poll and what follows it ----

fn url_importer(w: &World, relationships: bool, note: bool, remove: bool) {
    w.set(|s| {
        s.auto_import_method = "url_importer".to_owned();
        s.auto_import_min_similarity = 80.0;
        s.set_hydrus_duplicate_relationships = relationships;
        s.write_hydrus_provenance_note = note;
        s.remove_after_import = remove;
        s.url_import_confirm_timeout = 0.0;
        s.url_import_confirm_interval = 0.0;
    });
}

#[tokio::test]
async fn with_every_setting_that_needs_a_hash_off_the_import_is_not_confirmed() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer(&w, false, false, false);
    let mut e = w.entry(97.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(result.success);
    assert_eq!(result.confirmed, None);
    assert_eq!(
        w.hydrus.count("/add_urls/get_url_files"),
        0,
        "no poll was paid for"
    );
    assert_eq!(w.hydrus.count("/add_urls/add_url"), 1);
}

#[tokio::test]
async fn each_of_the_three_settings_alone_is_enough_to_confirm_the_import() {
    for (relationships, note, remove) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let w = world(knows_post(Fake::new(), URL)).await;
        url_importer(&w, relationships, note, remove);
        let mut e = w.entry(97.0);
        let local = e.hydrus_hash.clone();
        let result = w.import(&mut e, None).await.unwrap().unwrap();
        assert_eq!(
            result.confirmed,
            Some(true),
            "{relationships} {note} {remove}"
        );
        assert_eq!(
            e.hydrus_hash.as_deref(),
            Some(NEW),
            "the confirmed hash replaces the local one"
        );
        assert_ne!(local.as_deref(), Some(NEW));
        assert_eq!(
            w.hydrus
                .count("/manage_file_relationships/set_file_relationships")
                > 0,
            relationships
        );
        assert_eq!(w.hydrus.count("/add_notes/set_notes") > 0, note);
    }
}

#[tokio::test]
async fn an_unconfirmed_import_relates_nothing_and_writes_no_note() {
    let mut fake = knows_post(Fake::new(), URL);
    fake.confirms = false;
    let w = world(fake).await;
    url_importer(&w, true, true, true);
    let mut e = w.entry(97.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(result.success);
    assert_eq!(result.confirmed, Some(false));
    assert_eq!(
        w.hydrus
            .count("/manage_file_relationships/set_file_relationships"),
        0
    );
    assert_eq!(w.hydrus.count("/add_notes/set_notes"), 0);
    assert_ne!(e.hydrus_hash.as_deref(), Some(NEW));
}

#[tokio::test]
async fn a_raised_stop_flag_ends_the_poll_without_a_confirmation() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer(&w, false, false, true);
    w.stop.store(true, Ordering::SeqCst);
    let mut e = w.entry(97.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert_eq!(result.confirmed, Some(false));
    assert_eq!(w.hydrus.count("/add_urls/get_url_files"), 0);
}

// ---- a refused URL: another match, then Hatate's own download ----

/// `test_search_quality::TestHydrusUrlImporterGuard::test_auto_import_falls_back_to_a_match_hydrus_can_take`
#[tokio::test]
async fn auto_import_falls_back_to_a_match_hydrus_can_take() {
    let w = world(knows_post(Fake::new(), OTHER_URL)).await;
    url_importer(&w, false, false, false);
    let mut e = w.entry(100.0);
    e.candidates[0].url = PAGE_URL.to_owned();
    e.candidates.push(candidate(OTHER_URL, 100.0));
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(result.success, "{result:?}");
    assert_eq!(e.selected_candidate_index, 1, "the importable one is selected now");
    assert_eq!(e.matched_url(), Some(OTHER_URL));
    assert_eq!(e.error_message, None);
    assert!(e.sent_to_hydrus);
    let added: Vec<String> = w
        .hydrus
        .server
        .recorded()
        .iter()
        .filter(|q| path_of(q) == "/add_urls/add_url")
        .map(|q| String::from_utf8_lossy(&q.body).into_owned())
        .collect();
    assert_eq!(added.len(), 1, "import_url ran once");
    assert!(added[0].contains(OTHER_URL), "{}", added[0]);
    let asked: Vec<String> = w.hydrus.server.recorded().iter().filter(|q| path_of(q) == "/add_urls/get_url_info").map(url_param).collect();
    assert_eq!(asked[0], PAGE_URL, "the chosen match was asked first");
}

/// `...::test_the_fallback_never_goes_below_the_threshold`
#[tokio::test]
async fn the_fallback_never_goes_below_the_threshold() {
    let w = world(knows_post(Fake::new(), OTHER_URL)).await;
    url_importer(&w, false, false, false);
    let mut e = w.entry(100.0);
    e.candidates[0].url = PAGE_URL.to_owned();
    e.candidates.push(candidate(OTHER_URL, 60.0));
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(!result.success && result.refused);
    assert_eq!(w.hydrus.count("/add_urls/add_url"), 0, "import_url never ran");
    assert_eq!(e.selected_candidate_index, 0);
    assert!(!e.sent_to_hydrus);
}

#[tokio::test]
async fn only_matches_that_clear_the_bar_on_a_measured_score_and_are_web_links_are_tried() {
    let w = world(Fake::new()).await;
    let writer = SessionStore::new(&w.paths).open_writer().unwrap();
    let _ = writer;
    let settings = w.settings.lock().unwrap().clone();
    let client = HydrusClient::new(&settings.hydrus).unwrap();
    let mut e = w.entry(97.0);
    e.candidates[0].url = PAGE_URL.to_owned();
    let mut below = candidate("https://example.com/below", 89.0);
    below.similarity_measured = true;
    let mut unmeasured = candidate("https://example.com/unmeasured", 99.0);
    unmeasured.similarity_measured = false;
    unmeasured.engine = "ascii2d".to_owned();
    let mut gone = candidate("https://example.com/gone", 99.0);
    gone.remote_available = Some(false);
    let not_web = candidate("ftp://example.com/x", 99.0);
    e.candidates.extend([below, unmeasured, gone, not_web]);
    // None of the four is asked about, and nothing is switched.
    assert!(
        !switch_to_importable_match(&mut e, &client, &settings)
            .await
            .unwrap()
    );
    assert_eq!(w.hydrus.paths(), Vec::<String>::new());
    assert_eq!(e.selected_candidate_index, 0);
}

#[tokio::test]
async fn at_most_five_alternatives_are_asked_about() {
    let w = world(Fake::new()).await;
    let settings = w.settings.lock().unwrap().clone();
    let client = HydrusClient::new(&settings.hydrus).unwrap();
    let mut e = w.entry(97.0);
    for i in 0..8 {
        e.candidates
            .push(candidate(&format!("https://example.com/{i}"), 99.0));
    }
    assert!(
        !switch_to_importable_match(&mut e, &client, &settings)
            .await
            .unwrap()
    );
    assert_eq!(w.hydrus.count("/add_urls/get_url_info"), 5);
}

/// `test_search_quality::TestSendUrlFallsBackToHatatesDownload::test_auto_import_downloads_when_no_match_hydrus_can_take`
#[tokio::test]
async fn auto_import_downloads_when_no_match_hydrus_can_take() {
    let w = world(Fake::new()).await;
    url_importer(&w, false, false, true);
    let mut e = w.entry(97.0);
    e.candidates[0].url = PAGE_URL.to_owned();
    e.candidates[0].direct_file_url = Some("https://cdn.donmai.us/f.png".to_owned());
    e.candidates[0].engine = "Google Lens".to_owned();
    let tmp = w.staging().join("hatate-linux-auto-test");
    std::fs::create_dir_all(&tmp).unwrap();
    let result = w.import(&mut e, Some(&tmp)).await.unwrap().unwrap();
    assert!(result.success, "{result:?}");
    assert_eq!(result.confirmed, Some(true), "the upload path is confirmed as it goes");
    assert!(result.note.is_some());
    assert_eq!(w.hydrus.count("/add_files/add_file"), 1);
    assert_eq!(w.hydrus.count("/add_urls/add_url"), 0, "Hydrus's importer was refused");
    assert_eq!(w.hydrus.count("/add_urls/get_url_files"), 0, "no poll: the download path needs none");
}

#[tokio::test]
async fn a_refusal_with_no_staging_directory_is_the_result() {
    let w = world(Fake::new()).await;
    url_importer(&w, false, false, false);
    let mut e = w.entry(97.0);
    e.candidates[0].url = PAGE_URL.to_owned();
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(!result.success && result.refused);
    assert!(!e.sent_to_hydrus);
}

// ---- errors ----

#[tokio::test]
async fn a_hydrus_answer_of_the_wrong_shape_is_an_error_the_worker_does_not_catch() {
    let mut fake = Fake::new();
    fake.wrong_shape = vec!["/add_urls/get_url_info"];
    let w = world(fake).await;
    url_importer(&w, false, false, false);
    let mut e = w.entry(97.0);
    let fault = w.import(&mut e, None).await.unwrap_err();
    assert_eq!(fault.kind(), ErrorKind::UnexpectedResponse);
    let shown = fault.user_message();
    assert!(
        !shown.contains(KEY) && !shown.contains("127.0.0.1") && !shown.contains('?'),
        "{shown}"
    );
    assert!(!shown.contains("os error"), "{shown}");
}

#[tokio::test]
async fn a_failed_request_is_a_failed_result_on_the_row_not_an_error() {
    let mut fake = Fake::new();
    fake.fail = vec!["/add_files/add_file"];
    let w = world(fake).await;
    let mut e = w.entry(97.0);
    let result = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(!result.success);
    assert!(result.error.is_some());
    assert!(!e.sent_to_hydrus);
}

#[tokio::test]
async fn a_hydrus_that_does_not_answer_in_time_fails_the_import_and_the_run_goes_on() {
    let seen = Arc::new(Mutex::new(0usize));
    let counter = seen.clone();
    let slow = TestServer::http(move |_| {
        *counter.lock().unwrap() += 1;
        json_reply(json!({})).delayed(std::time::Duration::from_secs(3))
    })
    .await;
    let w = world(Fake::new()).await;
    w.set(|s| {
        s.hydrus.api_url = slow.url("");
        s.hydrus.timeout = 0.3;
    });
    let dir = w.pictures(2);
    w.match_every_image(2, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(outcome.searched, 2);
    assert_eq!(outcome.auto_imported, 0);
    assert!(entries.iter().all(|e| !e.sent_to_hydrus));
    assert_eq!(*seen.lock().unwrap(), 2, "each image tried once");
}

#[tokio::test]
async fn a_hydrus_that_cannot_be_reached_fails_the_import_and_the_run_goes_on() {
    let w = world(Fake::new()).await;
    // A port nothing listens on.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    w.set(|s| s.hydrus.api_url = format!("http://127.0.0.1:{closed}"));
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert!(!entries[0].sent_to_hydrus);
    assert!(entries[0].error_message.is_some());
}

// ---- https, through the binary's own constructor ----

#[tokio::test]
async fn over_https_the_certificate_is_checked_unless_the_setting_says_not_to() {
    let pki = TestPki::generate();
    let hits = Arc::new(Mutex::new(0usize));
    let counter = hits.clone();
    let fake = Fake::new();
    let secure = TestServer::https(&pki, move |q| {
        *counter.lock().unwrap() += 1;
        fake.answer(q)
    })
    .await;
    let w = world(Fake::new()).await;
    w.set(|s| {
        s.hydrus.api_url = secure.url("");
        s.hydrus.verify_ssl = true;
    });
    let mut e = w.entry(97.0);
    let refused = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(
        !refused.success,
        "a self-signed certificate is refused with verify_ssl on"
    );
    assert_eq!(
        *hits.lock().unwrap(),
        0,
        "no request got through the handshake"
    );
    assert_eq!(
        refused.fault.as_ref().map(HydrusError::kind),
        Some(ErrorKind::Net(hatate_core::net::ErrorKind::Tls))
    );
    assert!(!e.sent_to_hydrus);

    w.set(|s| s.hydrus.verify_ssl = false);
    let accepted = w.import(&mut e, None).await.unwrap().unwrap();
    assert!(accepted.success, "{accepted:?}");
    assert!(*hits.lock().unwrap() >= 1);
    assert!(e.sent_to_hydrus);
}

// ---- the batch: staging directory, sweep, removal, failure ----

fn url_importer_batch(w: &World) {
    url_importer(w, false, false, false);
    w.set(|s| s.auto_import_min_similarity = 90.0);
}

#[tokio::test]
async fn the_staging_directory_exists_during_the_run_with_this_pid_and_is_gone_after() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer_batch(&w);
    let dir = w.pictures(2);
    w.match_every_image(2, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Finished);
    assert_eq!(outcome.auto_imported, 2);
    assert!(entries.iter().all(|e| e.sent_to_hydrus));
    let seen = w.hydrus.seen.lock().unwrap().clone();
    assert!(!seen.is_empty());
    for s in &seen {
        assert_eq!(s.staging.len(), 1, "{s:?}");
        let (name, files) = s.staging[0].split_once('/').unwrap();
        assert!(
            name.starts_with("hatate-linux-auto-") && name.len() == 18 + 8,
            "{name}"
        );
        assert_eq!(files, "owner.pid");
    }
    assert_eq!(
        listing(&w.staging()),
        Vec::<String>::new(),
        "removed with its contents"
    );
    assert!(w.staging().is_dir(), "the root stays");
}

#[tokio::test]
async fn the_upload_method_makes_no_staging_directory() {
    let w = world(Fake::new()).await;
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    assert!(entries[0].sent_to_hydrus);
    assert!(
        w.hydrus
            .seen
            .lock()
            .unwrap()
            .iter()
            .all(|s| s.staging.is_empty())
    );
}

#[tokio::test]
async fn the_staging_directory_is_removed_when_the_run_is_stopped() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer_batch(&w);
    let dir = w.pictures(2);
    w.match_every_image(2, 97.0);
    // Stop as soon as the first import has happened.
    struct StopAfterImport(Arc<AtomicBool>);
    impl Reporter for StopAfterImport {
        fn line(&self, _: &str) {}
        fn countdown(&self, _: &str, _: f64, _: f64) {}
        fn auto_imported(&self, _: &Entry, _: &ImportResult) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let writer = SessionStore::new(&w.paths).open_writer().unwrap();
    let sink = Sink(writer);
    let quota_file = QuotaPauseFile::new(&w.paths);
    let (quota, random, reset) = (|| false, || 0.5, || {});
    let env = BatchEnv {
        engine: &w.engine,
        settings: &w.settings,
        stop: w.stop.clone(),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &sink,
        reporter: Arc::new(StopAfterImport(w.stop.clone())),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &|s: &HydrusSettings| HydrusClient::new(s),
    };
    let mut entries = Vec::new();
    let outcome = run_batch(&env, &dir, &mut entries).await;
    assert_eq!(outcome.ending, Ending::Stopped);
    assert_eq!(outcome.searched, 1);
    assert_eq!(listing(&w.staging()), Vec::<String>::new());
}

#[tokio::test]
async fn the_staging_directory_is_removed_when_the_run_ends_on_an_error() {
    let mut fake = Fake::new();
    fake.wrong_shape = vec!["/add_urls/get_url_info"];
    let w = world(fake).await;
    url_importer_batch(&w);
    let dir = w.pictures(3);
    w.match_every_image(3, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    let Ending::Failed(fault) = &outcome.ending else {
        panic!("{:?}", outcome.ending);
    };
    assert_eq!(fault.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(
        outcome.searched, 1,
        "the first image was searched, then the worker died"
    );
    assert_eq!(entries[0].status, MatchStatus::Good);
    assert_eq!(entries[1].status, MatchStatus::NotSearched);
    assert_eq!(entries[2].status, MatchStatus::NotSearched);
    assert_eq!(listing(&w.staging()), Vec::<String>::new());
    assert!(w.lines().iter().any(|l| l.contains("stopped on an error")));
    assert_eq!(w.saved().len(), 3, "the session still has every row");
}

#[tokio::test]
async fn a_run_sweeps_the_directories_earlier_runs_left_and_spares_a_live_owner() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer_batch(&w);
    let root = w.staging();
    let mk = |name: &str, pid: Option<&str>| {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("download.png"), b"x").unwrap();
        if let Some(pid) = pid {
            std::fs::write(d.join("owner.pid"), pid).unwrap();
        }
        d
    };
    let mut dead = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = dead.id().to_string();
    dead.wait().unwrap();
    let stale = mk("hatate-linux-auto-dead0001", Some(&dead_pid));
    let nopid = mk("hatate-linux-auto-nopid002", None);
    let junk = mk("hatate-linux-auto-junk0003", Some("not a number"));
    let zero = mk("hatate-linux-auto-zero0004", Some("0"));
    let live = mk(
        "hatate-linux-auto-live0005",
        Some(&std::process::id().to_string()),
    );
    let other = mk("someone-elses-dir", Some(&dead_pid));
    let file = root.join("hatate-linux-auto-afile006");
    std::fs::write(&file, b"x").unwrap();

    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    for gone in [&stale, &nopid, &junk, &zero] {
        assert!(!gone.exists(), "{}", gone.display());
    }
    for kept in [&live, &other, &file] {
        assert!(kept.exists(), "{}", kept.display());
    }
}

#[tokio::test]
async fn an_owner_pid_too_large_for_a_c_int_ends_the_run_before_any_search() {
    let w = world(Fake::new()).await;
    let d = w.staging().join("hatate-linux-auto-huge0001");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("owner.pid"), "99999999999").unwrap();
    let dir = w.pictures(2);
    w.match_every_image(2, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    let Ending::Failed(fault) = &outcome.ending else {
        panic!("{:?}", outcome.ending);
    };
    assert_eq!(fault.kind(), ErrorKind::InvalidInput);
    assert_eq!(outcome.searched, 0);
    assert!(entries.iter().all(|e| e.status == MatchStatus::NotSearched));
    assert!(w.engines.calls().is_empty());
    assert!(d.exists(), "the sweep stopped where Python's did");
}

#[tokio::test]
async fn a_staging_root_that_cannot_be_made_ends_the_run_and_says_so_without_the_path() {
    let w = world(Fake::new()).await;
    url_importer_batch(&w);
    // The cache folder is a file.
    let cache = w.staging().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, b"x").unwrap();
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    let Ending::Failed(fault) = &outcome.ending else {
        panic!("{:?}", outcome.ending);
    };
    assert_eq!(fault.kind(), ErrorKind::Io);
    let shown = fault.user_message();
    assert!(!shown.contains(cache.to_str().unwrap()), "{shown}");
    assert!(
        !shown.contains("os error") && !shown.contains("Errno"),
        "{shown}"
    );
    assert_eq!(outcome.searched, 0);
}

#[tokio::test]
async fn with_no_access_key_nothing_is_imported_and_no_staging_directory_is_made() {
    let w = world(Fake::new()).await;
    url_importer_batch(&w);
    w.set(|s| s.hydrus.access_key = Secret::new(""));
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!(outcome.searched, 1);
    assert_eq!(w.hydrus.paths(), Vec::<String>::new());
    assert!(!w.staging().join("x").exists());
    assert_eq!(listing(&w.staging()), Vec::<String>::new());
    assert!(w.lines().iter().any(|l| l.contains("no Hydrus access key")));
}

#[tokio::test]
async fn with_auto_import_off_the_hydrus_is_not_touched_and_no_directory_is_made() {
    let w = world(Fake::new()).await;
    w.set(|s| s.auto_import_enabled = false);
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    w.run(&dir, &mut entries).await;
    assert_eq!(w.hydrus.paths(), Vec::<String>::new());
    assert!(!entries[0].sent_to_hydrus);
}

#[tokio::test]
async fn remove_after_import_drops_the_imported_rows_from_the_list_and_the_session() {
    let w = world(knows_post(Fake::new(), URL)).await;
    url_importer(&w, false, false, true);
    w.set(|s| s.auto_import_min_similarity = 90.0);
    let dir = w.pictures(3);
    // The middle one is below the bar and stays.
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 97.0)])));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 50.0)])));
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 97.0)])));
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!((outcome.auto_imported, outcome.removed), (2, 2));
    assert_eq!(
        entries.iter().map(|e| e.filename()).collect::<Vec<_>>(),
        ["p1.png"]
    );
    assert_eq!(w.saved().len(), 1);
    assert_eq!(w.reporter.imported.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn an_import_hydrus_never_confirmed_is_not_removed_and_a_failed_one_is_kept() {
    let mut fake = knows_post(Fake::new(), URL);
    fake.confirms = false;
    let w = world(fake).await;
    url_importer(&w, false, false, true);
    let dir = w.pictures(1);
    w.match_every_image(1, 97.0);
    let mut entries = Vec::new();
    let outcome = w.run(&dir, &mut entries).await;
    assert_eq!((outcome.auto_imported, outcome.removed), (1, 0));
    assert_eq!(entries.len(), 1, "the file may still be downloading");
    assert!(entries[0].sent_to_hydrus && !entries[0].hydrus_import_confirmed);
}

#[tokio::test]
async fn the_net_config_the_test_adds_is_not_how_the_binary_builds_its_client() {
    // The binary passes `HydrusClient::new`: no host override, no extra root, and `verify_ssl`
    // straight from the settings. The test world's config is what reaches the sites.
    let w = world(Fake::new()).await;
    let client = HydrusClient::new(&w.settings.lock().unwrap().hydrus).unwrap();
    assert!(
        client.settings().verify_ssl,
        "verification is on by default"
    );
    let _ = &w.net_config;
}
