//! DAN-2165 (Phase 5.c): `hatate batch <dir>` with Google Lens on, end to end: the folder scan,
//! the hash pass, the search through the runner and the real `LiveEngines` Lens client with a
//! scripted `LensFetch` in place of the browser, rule34.xxx and Reddit answered from a local https
//! server (`NetClient::new`, rustls with the explicit provider), and the session the store writes.
//! IQDB answers nothing (scripted), so Lens is the only engine that finds anything. No live
//! network and no browser.
//!
//! The shipped binary cannot be run here: its primary engine is IQDB on the real host, and it has
//! no flag that redirects one (the rule). `hatate-cli` builds its engines with
//! `LiveEngines::for_profile`, which `lens_wiring.rs` pins; the rest is this library path. The
//! differential against the real Python worker over the same folders and the same scripted
//! fetches is the broad check (`worktrees/DAN-2165-differential.log`): Python opens the session
//! this writes and compares it entry for entry with its own.

mod common;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use common::{Recorded, Reply, ScriptedEngines, TestPki, TestServer};
use hatate_core::batch::{BatchEnv, Outcome, Silent, run_batch};
use hatate_core::clock::{BoxFuture, ManualClock};
use hatate_core::config::Settings;
use hatate_core::engine_backends::LiveEngines;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::engines::lens_search::{
    LensClient, LensFetch, LensFetchError, LensMissing, LensPayloads,
};
use hatate_core::engines::lens_sites::OnlineSites;
use hatate_core::image_compare::{Source, dhash};
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;
use hatate_core::session::{LoadOptions, SessionStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const NOW: f64 = 1_792_725_790.0;
pub const HOSTS: [&str; 7] = [
    "rule34.xxx",
    "wimg.rule34.xxx",
    "www.reddit.com",
    "api.rule34.xxx",
    "danbooru.donmai.us",
    "www.pinterest.com",
    "www.example.org",
];

/// One scripted answer by `host` + request target.
#[derive(Clone)]
pub struct Answer {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// What `fetch_results_payloads` does for one image.
#[derive(Clone)]
pub enum Fetch {
    Pages { payloads: Vec<String>, tiles: Value },
    Unavailable(String),
    Challenge(String),
    Browser(String),
}

/// What the fetch was asked.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchCall {
    pub sha256: String,
    pub filename: String,
    pub timeout: f64,
    pub whole_image: bool,
    pub dhash16: Option<String>,
}

/// A browser that answers from a script, in the order the images are searched.
pub struct ScriptedFetch {
    script: Mutex<VecDeque<Fetch>>,
    pub calls: Mutex<Vec<FetchCall>>,
}

impl ScriptedFetch {
    pub fn new(script: Vec<Fetch>) -> Arc<Self> {
        Arc::new(ScriptedFetch {
            script: Mutex::new(script.into()),
            calls: Mutex::default(),
        })
    }
}

impl LensFetch for ScriptedFetch {
    fn fetch_results_payloads<'a>(
        &'a self,
        image: &'a [u8],
        filename: &'a str,
        timeout: f64,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        let hash = dhash(Source::Bytes(image), 16)
            .ok()
            .flatten()
            .map(|h| h.to_hex());
        self.calls.lock().unwrap().push(FetchCall {
            sha256: format!("{:x}", Sha256::digest(image)),
            filename: filename.to_owned(),
            timeout,
            whole_image,
            dhash16: hash,
        });
        let step = self.script.lock().unwrap().pop_front();
        Box::pin(std::future::ready(match step {
            Some(Fetch::Pages { payloads, tiles }) => Ok(LensPayloads { payloads, tiles }),
            Some(Fetch::Unavailable(message)) => Err(LensFetchError::Unavailable {
                message,
                missing: Some(LensMissing::Chromium),
            }),
            Some(Fetch::Challenge(text)) => Err(LensFetchError::ChallengeUnanswered(text)),
            Some(Fetch::Browser(text)) => Err(LensFetchError::Browser(text)),
            None => Ok(LensPayloads {
                payloads: Vec::new(),
                tiles: json!([]),
            }),
        }))
    }
}

pub struct Scenario {
    /// File name in the folder -> bytes.
    pub files: Vec<(String, Vec<u8>)>,
    pub fetches: Vec<Fetch>,
    pub routes: HashMap<String, Answer>,
}

pub struct Ran {
    pub entries: Vec<Entry>,
    pub outcome: Outcome,
    pub session_db: PathBuf,
    pub calls: Vec<FetchCall>,
    pub requests: Vec<String>,
    /// Every sleep the run asked of the clock.
    pub sleeps: Vec<f64>,
    _dir: tempfile::TempDir,
}

/// Runs `hatate batch` over `scenario` in a throw-away profile.
pub async fn run_scenario(scenario: &Scenario) -> Ran {
    let pki = TestPki::generate_for(&HOSTS);
    let routes = Arc::new(scenario.routes.clone());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (handler_routes, handler_seen) = (routes.clone(), seen.clone());
    let server = TestServer::https(&pki, move |req: &Recorded| {
        let key = format!("{}{}", req.header("host").unwrap_or("?"), req.target);
        handler_seen
            .lock()
            .unwrap()
            .push(format!("{} {key}", req.method));
        match handler_routes.get(&key) {
            Some(a) => Reply::new(a.status, a.body.clone()).header("Content-Type", &a.content_type),
            None => Reply::new(404, "no route"),
        }
    })
    .await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in HOSTS {
        cfg = cfg.with_host_override(host, server.addr);
    }
    let net = NetClient::new(cfg).unwrap();

    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2165-batch-")
        .tempdir()
        .unwrap();
    let root = dir.path().to_path_buf();
    let home: std::ffi::OsString = root.join("home").into();
    let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
    let pics = root.join("pics");
    std::fs::create_dir_all(&pics).unwrap();
    for (name, bytes) in &scenario.files {
        std::fs::write(pics.join(name), bytes).unwrap();
    }

    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let fetch = ScriptedFetch::new(scenario.fetches.clone());
    let mut live = LiveEngines::new(net.clone(), clock.clone());
    live.lens = LensClient::with_fetch(fetch.clone())
        .with_clock(clock.clone())
        .with_sites(Arc::new(OnlineSites::new(net.clone())));
    let engines = Arc::new(ScriptedEngines::default());
    *engines.lens_backend.lock().unwrap() = Some(Arc::new(live));
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = SearchEngine::with_recorder(net, runner, paths.clone(), clock.clone(), None);
    let mut settings = Settings::with_defaults(&paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive = false;
    settings.enable_pawchive_index = false;
    settings.enable_google_lens = true;
    settings.match_conditions.require_larger_than_local = false;
    settings.delay_min_seconds = 0.0;
    settings.delay_max_seconds = 0.0;
    let settings = Mutex::new(settings);

    let writer = SessionStore::new(&paths).open_writer().unwrap();
    let quota_file = QuotaPauseFile::new(&paths);
    let quota = || false;
    let random = || 0.5;
    let reset = || {};
    let env = BatchEnv {
        engine: &engine,
        settings: &settings,
        stop: Arc::new(AtomicBool::new(false)),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &writer,
        reporter: Arc::new(Silent),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &|s| hatate_core::hydrus_client::HydrusClient::new(s),
    };
    let mut entries = Vec::new();
    let outcome = run_batch(&env, &pics, &mut entries).await;
    drop(writer);
    let session_db = paths.session_db();
    let saved = SessionStore::new(&paths)
        .load(&LoadOptions::at(NOW))
        .entries;
    let requests = seen.lock().unwrap().clone();
    let calls = fetch.calls.lock().unwrap().clone();
    Ran {
        entries: saved,
        outcome,
        session_db,
        calls,
        requests,
        sleeps: clock.sleeps(),
        _dir: dir,
    }
}

// ---- pictures: any image with structure, unique per seed ----

pub fn picture(seed: u64, w: u32, h: u32) -> image::RgbImage {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    let mut small = image::RgbImage::new(12, 16);
    for p in small.pixels_mut() {
        *p = image::Rgb([next(), next(), next()]);
    }
    image::imageops::resize(&small, w, h, image::imageops::FilterType::CatmullRom)
}

pub fn png(img: &image::RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(img: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(img)
        .unwrap();
    out
}

fn html(body: String) -> Answer {
    Answer {
        status: 200,
        content_type: "text/html; charset=UTF-8".to_owned(),
        body: body.into_bytes(),
    }
}

/// A Lens response body the 2.j parser reads two visual matches out of.
fn payload() -> String {
    r#"["https:\/\/www.pinterest.com\/pin\/0\/","Pin number 0 of many","https:\/\/www.pinterest.com\/pin\/1\/","Pin number 1 of many"]"#.to_owned()
}

fn pages(tiles: Value) -> Fetch {
    Fetch::Pages {
        payloads: vec![payload()],
        tiles,
    }
}

fn one_picture() -> (String, Vec<u8>) {
    ("a.png".to_owned(), png(&picture(40, 96, 72)))
}

fn row(entry: &Entry) -> String {
    format!(
        "{:?} {:?} {}",
        entry.status,
        entry.error_message,
        entry
            .candidates
            .iter()
            .map(|c| c.url.clone())
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[tokio::test]
async fn lens_finds_matches_and_a_rule34_search_tile_is_recovered_through_the_listing_page() {
    let (name, bytes) = one_picture();
    let original = picture(40, 96, 72);
    let mut routes = HashMap::new();
    routes.insert(
        "rule34.xxx/index.php?page=post&s=list&tags=parent%3A5".to_owned(),
        html(
            "<span><a id=\"p5\" href=\"/index.php?page=post&s=view&id=5\">\
             <img src=\"https://wimg.rule34.xxx/t/5.jpg\"/></a></span>"
                .to_owned(),
        ),
    );
    routes.insert(
        "rule34.xxx/index.php?page=post&s=view&id=5".to_owned(),
        html(
            "<html><body><ul id=\"tag-sidebar\"><li class=\"tag-type-general tag\">\
             <a href=\"x\">red_hair</a></li></ul></body></html>"
                .to_owned(),
        ),
    );
    routes.insert(
        "wimg.rule34.xxx/t/5.jpg".to_owned(),
        Answer {
            status: 200,
            content_type: "image/jpeg".to_owned(),
            body: jpeg(&original, 80),
        },
    );
    let ran = run_scenario(&Scenario {
        files: vec![(name, bytes)],
        fetches: vec![pages(json!([{
            "text": "Rule 34 / parent:5 250x177 Rule 34",
            "thumb": "data:image/jpeg;base64,AAAA"
        }]))],
        routes,
    })
    .await;
    assert_eq!(ran.entries.len(), 1, "{:?}", ran.outcome);
    let urls: Vec<&str> = ran.entries[0]
        .candidates
        .iter()
        .map(|c| c.url.as_str())
        .collect();
    assert_eq!(
        urls[0], "https://rule34.xxx/index.php?page=post&s=view&id=5",
        "the recovered post is the first exact match: {urls:?}"
    );
    assert!(
        urls.contains(&"https://www.pinterest.com/pin/0/"),
        "{urls:?}"
    );
    assert_eq!(ran.calls.len(), 1);
    assert_eq!(ran.calls[0].filename, "a.png");
    assert!(ran.calls[0].whole_image);
    assert!(ran.requests[0].starts_with("GET rule34.xxx/index.php?page=post&s=list"));
}

#[tokio::test]
async fn an_unavailable_browser_latches_lens_for_the_run() {
    let (name, bytes) = one_picture();
    let second = ("b.png".to_owned(), png(&picture(41, 96, 72)));
    let ran = run_scenario(&Scenario {
        files: vec![(name, bytes), second],
        fetches: vec![Fetch::Unavailable("no browser here".to_owned())],
        routes: HashMap::new(),
    })
    .await;
    assert_eq!(ran.entries.len(), 2);
    assert_eq!(
        ran.calls.len(),
        1,
        "the second image does not reach the browser"
    );
    let first = row(&ran.entries[0]);
    let second = row(&ran.entries[1]);
    assert!(first.contains("Google Lens: no browser here"), "{first}");
    assert!(second.contains("Google Lens: not searched"), "{second}");
    assert!(ran.entries.iter().all(|e| e.status != MatchStatus::Good));
}

#[tokio::test]
async fn an_unanswered_robot_check_rests_lens_and_says_so() {
    let (name, bytes) = one_picture();
    let second = ("b.png".to_owned(), png(&picture(41, 96, 72)));
    let ran = run_scenario(&Scenario {
        files: vec![(name, bytes), second],
        fetches: vec![Fetch::Challenge(
            "the robot check in the browser window went unanswered".to_owned(),
        )],
        routes: HashMap::new(),
    })
    .await;
    assert_eq!(ran.calls.len(), 1);
    let first = row(&ran.entries[0]);
    let second = row(&ran.entries[1]);
    assert!(
        first.contains("Google asked to check you aren't a robot"),
        "{first}"
    );
    assert!(
        second.contains("resting after an unanswered robot check"),
        "{second}"
    );
}

#[tokio::test]
async fn a_browser_failure_is_counted_and_a_page_with_nothing_in_it_is_a_failure_too() {
    let files: Vec<(String, Vec<u8>)> = (0..4)
        .map(|i| (format!("p{i}.png"), png(&picture(50 + i, 96, 72))))
        .collect();
    let ran = run_scenario(&Scenario {
        files,
        fetches: vec![
            Fetch::Browser("Page.goto: Timeout 90000ms exceeded.".to_owned()),
            Fetch::Pages {
                payloads: vec![],
                tiles: json!([]),
            },
            Fetch::Browser("Page.evaluate: boom".to_owned()),
            pages(json!([])),
        ],
        routes: HashMap::new(),
    })
    .await;
    // Three failures in a row stand Lens down; the fourth image is never searched.
    assert_eq!(
        ran.calls.len(),
        3,
        "{:#?}",
        ran.entries.iter().map(row).collect::<Vec<_>>()
    );
    let rows: Vec<String> = ran.entries.iter().map(row).collect();
    assert!(
        rows[0].contains(
            "the browser could not read the results (Page.goto: Timeout 90000ms exceeded.)"
        ),
        "{rows:?}"
    );
    assert!(rows[1].contains("nothing in it"), "{rows:?}");
    assert!(rows[3].contains("not searched"), "{rows:?}");
}
