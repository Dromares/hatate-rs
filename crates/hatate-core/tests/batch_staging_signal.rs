//! The second-signal path of `hatate batch` (Phase 4.g, DAN-2132): the process is about to exit
//! without unwinding, so the staging directory of the run in progress is removed by
//! `cleanup_current_staging` (the `atexit`/signal `cleanup()` of `register_cleanup_handlers`).
//! Its own test binary: the tracker is process-wide, as Python's `_current_staging_dir` is, and
//! the other batch tests run side by side in theirs.

mod common;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use common::{Recorded, Reply, ScriptedEngines, Step, TestServer, iqdb_match};
use hatate_core::batch::{BatchEnv, SessionSink, Silent, run_batch};
use hatate_core::clock::ManualClock;
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::engine_runner::EngineRunner;
use hatate_core::hydrus_client::HydrusClient;
use hatate_core::model::Entry;
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;

struct NoSink;
impl SessionSink for NoSink {
    fn save(&self, _: &[Entry], _: f64) -> Result<(), String> {
        Ok(())
    }
}

fn listing(root: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn the_signal_path_cleanup_removes_the_directory_of_a_run_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = dir.path().to_path_buf();
    let home: std::ffi::OsString = root.join("home").into();
    let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
    let staging = paths.auto_import_staging_dir();

    // At the first Hydrus request the run is under way: the staging directory exists, then it is
    // cleaned up as the second-signal handler does, and looked at again.
    let (before, after) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(vec![])));
    let (b2, a2, s2) = (before.clone(), after.clone(), staging.clone());
    let hydrus = TestServer::http(move |q: &Recorded| {
        if q.target.starts_with("/add_urls/get_url_info") {
            *b2.lock().unwrap() = listing(&s2);
            hatate_core::batch::cleanup_current_staging();
            *a2.lock().unwrap() = listing(&s2);
        }
        Reply::ok(r#"{"url_type": 0, "url_type_string": "post url", "match_name": "x", "can_parse": true}"#)
    })
    .await;

    let pics = root.join("pics");
    std::fs::create_dir_all(&pics).unwrap();
    std::fs::write(pics.join("a.png"), {
        let mut out = Vec::new();
        image::RgbImage::from_fn(8, 8, |x, y| image::Rgb([x as u8 * 9, y as u8 * 9, 3]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    })
    .unwrap();

    let clock = Arc::new(ManualClock::new(1_792_725_790.0, 0.0));
    let engines = Arc::new(ScriptedEngines::default());
    engines.iqdb.push(Step::Return(Ok(vec![iqdb_match(
        "https://example.com/p/1",
        97.0,
    )])));
    let runner = EngineRunner::new(engines, clock.clone());
    let net = NetClient::new(NetConfig::default()).unwrap();
    let engine = SearchEngine::with_recorder(net, runner, paths.clone(), clock, None);
    let mut settings = Settings::with_defaults(&paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive_index = false;
    settings.match_conditions.require_larger_than_local = false;
    settings.auto_import_enabled = true;
    settings.auto_import_method = "url_importer".to_owned();
    settings.hydrus = HydrusSettings {
        api_url: hydrus.url(""),
        access_key: Secret::new("0".repeat(64)),
        timeout: 5.0,
        ..HydrusSettings::default()
    };
    let settings = Mutex::new(settings);
    let quota_file = QuotaPauseFile::new(&paths);
    let (quota, random, reset) = (|| false, || 0.5, || {});
    let env = BatchEnv {
        engine: &engine,
        settings: &settings,
        stop: Arc::new(AtomicBool::new(false)),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &NoSink,
        reporter: Arc::new(Silent),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &|s: &HydrusSettings| HydrusClient::new(s),
    };
    let mut entries = Vec::new();
    run_batch(&env, &pics, &mut entries).await;

    assert_eq!(
        before.lock().unwrap().len(),
        1,
        "the run's directory existed"
    );
    assert!(before.lock().unwrap()[0].starts_with("hatate-linux-auto-"));
    assert_eq!(
        *after.lock().unwrap(),
        Vec::<String>::new(),
        "gone while the run was still going"
    );
}
