//! DAN-2145 (Phase 4.h): `hatate batch <dir>` with a Pawchive index in the profile, end to end:
//! the folder scan, the hash pass, the search through the runner and the real `LiveEngines`
//! index, the Pawchive booru pages from a local https server (`NetClient::new`, rustls with the
//! explicit provider), and the session the store writes. IQDB answers nothing (scripted), so the
//! index is the only engine that finds anything. No live network.
//!
//! The shipped binary cannot be run here: its primary engine is IQDB on the real host, and it has
//! no flag that redirects one (the rule). `hatate-cli` builds its engines with
//! `LiveEngines::for_profile`, which `pawchive_index_wiring.rs` pins; the rest is this library
//! path. The differential against the real Python worker over the same folders and the same
//! index file is the broad check (`worktrees/DAN-2145-differential.log`): Python opens the
//! session this writes and compares it entry for entry with its own.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use common::{Recorded, Reply, ScriptedEngines, TestPki, TestServer};
use hatate_core::batch::{BatchEnv, Outcome, Silent, run_batch};
use hatate_core::clock::ManualClock;
use hatate_core::config::Settings;
use hatate_core::engine_backends::LiveEngines;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::image_compare::Source;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::pawchive_index::{ImageRow, PawchiveIndex, fingerprints};
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;
use hatate_core::session::{LoadOptions, SessionStore};
use sha2::{Digest, Sha256};

const NOW: f64 = 1_792_725_790.0;
const HOSTS: [&str; 2] = ["pawchive.pw", "img.pawchive.pw"];

/// One scripted answer, by the request's path and query.
#[derive(Clone)]
pub struct Answer {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub struct Scenario {
    /// File name in the folder -> bytes.
    pub files: Vec<(String, Vec<u8>)>,
    /// The index file's rows, written with the 4.f `PawchiveIndex::add_images` unless the
    /// caller hands `index_file` in.
    pub rows: Vec<ImageRow>,
    pub routes: HashMap<String, Answer>,
}

pub struct Ran {
    pub entries: Vec<Entry>,
    pub outcome: Outcome,
    pub session_db: PathBuf,
    pub index_db: PathBuf,
    /// The index file's bytes and mtime before the batch ran (the mtime is set far in the past
    /// so that any write shows).
    pub index_before: (Vec<u8>, SystemTime),
    pub requests: Vec<String>,
    _dir: tempfile::TempDir,
}

pub fn sha_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn row_for(service: &str, user: &str, post: u64, bytes: &[u8], title: &str) -> ImageRow {
    let (h8, h16) = fingerprints(Source::Bytes(bytes)).unwrap();
    let (h8, h16) = (h8.unwrap(), h16.unwrap());
    let sha = sha_of(bytes);
    ImageRow {
        service: service.to_owned(),
        user_id: user.to_owned(),
        post_id: post.to_string(),
        page_index: 0,
        path: format!("/{}/{}/{sha}.png", &sha[..2], &sha[2..4]),
        sha256: sha,
        title: Some(title.to_owned()),
        dhash8: Some(format!("{:016x}", h8.0[3])),
        dhash16: Some(format!(
            "{:016x}{:016x}{:016x}{:016x}",
            h16.0[0], h16.0[1], h16.0[2], h16.0[3]
        )),
    }
}

fn json_answer(body: serde_json::Value) -> Answer {
    Answer {
        status: 200,
        content_type: "application/json".to_owned(),
        body: body.to_string().into_bytes(),
    }
}

/// The two pages Pawchive's booru parser asks for one post, and its artist's profile.
pub fn post_routes(routes: &mut HashMap<String, Answer>, row: &ImageRow, tags: &[&str]) {
    routes.insert(
        format!(
            "/api/v1/{}/user/{}/post/{}",
            row.service, row.user_id, row.post_id
        ),
        json_answer(serde_json::json!({"post": {
            "id": row.post_id, "service": row.service, "user": row.user_id,
            "title": row.title, "tags": tags,
            "file": {"name": "a.png", "path": row.path}, "attachments": [], "has_full": false}})),
    );
    routes.insert(
        format!("/api/v1/{}/user/{}/profile", row.service, row.user_id),
        json_answer(serde_json::json!({"name": format!("Artist {}", row.user_id)})),
    );
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

/// Runs `hatate batch` over `scenario` in a throw-away profile. `index_file` is a ready-made
/// `pawchive_index.db` to copy into the profile (a Python-written one, say); without it the
/// rows are written with the Rust index. `enable_index` is the `enable_pawchive_index` setting.
pub async fn run_scenario(
    scenario: &Scenario,
    index_file: Option<&Path>,
    enable_index: bool,
) -> Ran {
    let pki = TestPki::generate_for(&HOSTS);
    let routes = Arc::new(scenario.routes.clone());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (handler_routes, handler_seen) = (routes.clone(), seen.clone());
    let server = TestServer::https(&pki, move |req: &Recorded| {
        handler_seen.lock().unwrap().push(format!(
            "{}{}",
            req.header("host").unwrap_or("?"),
            req.target
        ));
        match handler_routes.get(&req.target) {
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
        .prefix("hatate-dan2145-batch-")
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
    let index_db = paths.pawchive_index_db();
    std::fs::create_dir_all(index_db.parent().unwrap()).unwrap();
    match index_file {
        Some(src) => {
            std::fs::copy(src, &index_db).unwrap();
        }
        None => PawchiveIndex::new(&index_db)
            .add_images(&scenario.rows)
            .unwrap(),
    }

    std::fs::File::options()
        .write(true)
        .open(&index_db)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
        .unwrap();
    let index_before = (std::fs::read(&index_db).unwrap(), modified(&index_db));

    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let engines = Arc::new(ScriptedEngines::default());
    *engines.index_backend.lock().unwrap() = Some(Arc::new(LiveEngines::for_profile(
        net.clone(),
        clock.clone(),
        &paths,
    )));
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = SearchEngine::with_recorder(net, runner, paths.clone(), clock.clone(), None);
    let mut settings = Settings::with_defaults(&paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive = false;
    settings.enable_pawchive_index = enable_index;
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
    Ran {
        entries: saved,
        outcome,
        session_db,
        index_db,
        index_before,
        requests,
        _dir: dir,
    }
}

// ---- pictures: any image with structure, unique per seed ----

fn picture(seed: u64, w: u32, h: u32) -> image::RgbImage {
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

fn png(img: &image::RgbImage) -> Vec<u8> {
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

/// Three indexed posts; the folder holds the very same file as the first, a resized JPEG of the
/// second, an unrelated picture, and a file that is not a picture.
fn scenario() -> Scenario {
    let originals: Vec<image::RgbImage> = (0..3).map(|i| picture(500 + i, 128, 96)).collect();
    let bytes: Vec<Vec<u8>> = originals.iter().map(png).collect();
    let rows: Vec<ImageRow> = bytes
        .iter()
        .enumerate()
        .map(|(i, b)| row_for("fanbox", "4242", 900 + i as u64, b, &format!("post {i}")))
        .collect();
    let mut routes = HashMap::new();
    for row in &rows {
        post_routes(&mut routes, row, &["red_hair", "solo"]);
    }
    let small =
        image::imageops::resize(&originals[1], 70, 55, image::imageops::FilterType::Triangle);
    Scenario {
        files: vec![
            ("a_exact.png".to_owned(), bytes[0].clone()),
            ("b_small.jpg".to_owned(), jpeg(&small, 70)),
            ("c_unrelated.png".to_owned(), png(&picture(999, 90, 70))),
            (
                "d_notimage.png".to_owned(),
                b"definitely not a picture".to_vec(),
            ),
        ],
        rows,
        routes,
    }
}

fn entry<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
    entries
        .iter()
        .find(|e| e.filename() == name)
        .unwrap_or_else(|| panic!("{name} is not in the session"))
}

#[tokio::test]
async fn a_batch_over_a_folder_finds_the_indexed_posts_and_saves_them_in_the_session() {
    let ran = run_scenario(&scenario(), None, true).await;

    assert_eq!(ran.entries.len(), 4);
    let exact = entry(&ran.entries, "a_exact.png");
    assert_ne!(exact.status, MatchStatus::NotFound);
    let c = &exact.candidates[0];
    assert_eq!(c.url, "https://pawchive.pw/fanbox/user/4242/post/900");
    assert_eq!(c.similarity, 100.0);
    // Same SHA-256 is labelled as the exact engine; a fingerprint alone as the index engine.
    assert_eq!(c.engine, "Pawchive");
    let small = entry(&ran.entries, "b_small.jpg");
    assert_eq!(small.candidates.len(), 1);
    assert_eq!(
        small.candidates[0].url,
        "https://pawchive.pw/fanbox/user/4242/post/901"
    );
    assert_eq!(small.candidates[0].engine, "Pawchive index");
    assert!(small.candidates[0].similarity < 100.0);
    assert!(
        small.candidates[0]
            .booru_tags
            .iter()
            .any(|t| t.name == "red_hair"),
        "the post page was fetched from the local server: {:?}",
        small.candidates[0].booru_tags
    );
    let other = entry(&ran.entries, "c_unrelated.png");
    assert!(other.candidates.is_empty());
    assert_eq!(other.status, MatchStatus::NotFound);
    let junk = entry(&ran.entries, "d_notimage.png");
    assert_eq!(
        junk.error_message.as_deref(),
        Some("Pawchive index: could not read the local file")
    );
    assert!(
        ran.requests
            .iter()
            .all(|r| r.starts_with("pawchive.pw/api/v1/")
                || r.starts_with("img.pawchive.pw/thumbnail/")),
        "{:?}",
        ran.requests
    );
}

#[tokio::test]
async fn a_batch_never_writes_the_users_index() {
    let ran = run_scenario(&scenario(), None, true).await;

    assert!(ran.entries.iter().any(|e| !e.candidates.is_empty()));
    let after = (
        std::fs::read(&ran.index_db).unwrap(),
        modified(&ran.index_db),
    );
    assert!(ran.index_before.0 == after.0, "the index bytes changed");
    assert_eq!(ran.index_before.1, after.1, "the index mtime changed");
    let dir = ran.index_db.parent().unwrap();
    let mut siblings: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("pawchive_index"))
        .collect();
    siblings.sort();
    assert_eq!(
        siblings,
        ["pawchive_index.db"],
        "no journal or WAL left behind"
    );
}

#[tokio::test]
async fn with_the_setting_off_the_index_is_not_asked_and_not_touched() {
    let ran = run_scenario(&scenario(), None, false).await;

    assert_eq!(ran.entries.len(), 4);
    for e in &ran.entries {
        assert!(
            e.candidates.is_empty(),
            "{}: {:?}",
            e.filename(),
            e.candidates
        );
        assert!(e.error_message.is_none(), "{}", e.filename());
    }
    assert!(ran.requests.is_empty(), "{:?}", ran.requests);
}
