//! DAN-2063 (Phase 3.f): the engine runner over the shipped engines (`LiveEngines`), against a
//! local **https** server, through the constructor the binaries use (`NetClient::new`: rustls with
//! the explicit CryptoProvider). No live network.
//!
//! The verifier's PASS names `the_runner_reaches_local_https_engines_through_the_binaries_constructor`.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use common::{ReplayServer, Reply, TestPki};
use hatate_core::clock::ManualClock;
use hatate_core::config::Settings;
use hatate_core::engine_backends::{LENS_UNAVAILABLE_MESSAGE, LiveEngines};
use hatate_core::engine_runner::EngineRunner;
use hatate_core::engines::iqdb::IqdbClient;
use hatate_core::engines::tracemoe::TraceMoeClient;
use hatate_core::model::Entry;
use hatate_core::net::{NetClient, NetConfig};
use serde_json::json;

const IQDB_PAGE: &str = "<html><head><title>iqdb</title></head><body><div id=\"pages\">\
<table><tr><th>Best match</th></tr><tr>\
<td class=\"image\"><a href=\"https://danbooru.donmai.us/posts/77\"><img src=\"/thumbs/a.jpg\"></a></td>\
<td>1200x1600</td><td>97% similarity</td></tr></table></div></body></html>";

fn net(pki: &TestPki) -> NetClient {
    NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap()
}

fn settings() -> Settings {
    let paths = hatate_core::paths::Paths::from_lookup(|k| {
        (k == "HOME").then(|| "/home/test".into())
    })
    .unwrap();
    let mut s = Settings::with_defaults(&paths);
    s.enable_pawchive = false;
    s
}

struct Image {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn image() -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2063-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("shot.png");
    std::fs::write(&path, b"a small picture").unwrap();
    Image { _dir: dir, path }
}

#[tokio::test]
async fn the_runner_reaches_local_https_engines_through_the_binaries_constructor() {
    let pki = TestPki::generate();
    let file = image();
    let entry = Entry::new(file.path.to_str().unwrap());

    let iqdb = ReplayServer::https(
        &pki,
        vec![Reply::ok(IQDB_PAGE).header("Content-Type", "text/html; charset=UTF-8")],
    )
    .await;
    let body = json!({"result": [{
        "anilist": {"id": 21, "title": {"romaji": "One Piece"}},
        "similarity": 0.97, "episode": 3, "image": "https://example.test/t.jpg"}]});
    let moe = ReplayServer::https(
        &pki,
        vec![Reply::ok(body.to_string()).header("Content-Type", "application/json")],
    )
    .await;

    let clock = Arc::new(ManualClock::new(1_000.0, 0.0));
    let mut live = LiveEngines::new(net(&pki), clock.clone());
    live.iqdb = IqdbClient::with_hosts(net(&pki), &iqdb.url("/"), &iqdb.url("/3d/"));
    live.tracemoe = TraceMoeClient::with_host(net(&pki), &moe.url("/search"));
    let live = Arc::new(live);
    let runner = EngineRunner::new(live.clone(), clock.clone());
    let settings = Arc::new(settings());

    // One wave of IQDB, trace.moe, Google Lens and the Pawchive index.
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut answered = HashSet::new();
    let found = runner
        .run_engines_parallel(
            &["iqdb", "tracemoe", "googlelens", "pawchiveindex"],
            &entry,
            &settings,
            &mut candidates,
            &mut errors,
            None,
            None,
            None,
            Some(&mut answered),
        )
        .await
        .unwrap();

    assert!(found["iqdb"] && found["tracemoe"]);
    assert!(!found["googlelens"] && !found["pawchiveindex"]);
    let mut urls: Vec<(&str, &str)> = candidates
        .iter()
        .map(|c| (c.engine.as_str(), c.url.as_str()))
        .collect();
    urls.sort();
    assert_eq!(
        urls,
        vec![
            ("IQDB", "https://danbooru.donmai.us/posts/77"),
            ("trace.moe", "https://anilist.co/anime/21"),
        ]
    );
    // Lens could not run: the error says so, and the engine latches.
    assert_eq!(
        errors,
        vec![format!("Google Lens: {LENS_UNAVAILABLE_MESSAGE}")]
    );
    assert!(live.lens.is_blocked());
    assert_eq!(
        answered,
        HashSet::from([
            "iqdb".to_owned(),
            "tracemoe".to_owned(),
            "pawchiveindex".to_owned()
        ])
    );
    assert_eq!(iqdb.recorded()[0].method, "POST");
    assert_eq!(moe.recorded().len(), 1);

    // The next image: Lens is blocked, so it is not asked, and the row says so.
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    runner
        .run_engines_parallel(
            &["googlelens"],
            &entry,
            &settings,
            &mut candidates,
            &mut errors,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        errors,
        vec!["Google Lens: not searched (it stood down earlier this run)"]
    );
    assert_eq!(clock.slept(), 0.0, "a blocked Lens does not wait for its turn");

    // A new search clears the stand-down; the pacing then asks the clock for the 45 s spacing.
    live.lens.reset_blocked_flag();
    let mut errors = Vec::new();
    runner
        .run_engines_parallel(
            &["googlelens"],
            &entry,
            &settings,
            &mut Vec::new(),
            &mut errors,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(clock.slept(), 45.0);
    assert_eq!(
        errors,
        vec![format!("Google Lens: {LENS_UNAVAILABLE_MESSAGE}")]
    );
}

#[tokio::test]
async fn an_engine_that_cannot_reach_its_host_is_that_engines_failure() {
    let pki = TestPki::generate();
    let file = image();
    let entry = Entry::new(file.path.to_str().unwrap());
    // IQDB answers; trace.moe is pointed at a closed port.
    let iqdb = ReplayServer::https(
        &pki,
        vec![Reply::ok(IQDB_PAGE).header("Content-Type", "text/html; charset=UTF-8")],
    )
    .await;
    let clock = Arc::new(ManualClock::new(1_000.0, 0.0));
    let mut live = LiveEngines::new(net(&pki), clock.clone());
    live.iqdb = IqdbClient::with_hosts(net(&pki), &iqdb.url("/"), &iqdb.url("/3d/"));
    live.tracemoe = TraceMoeClient::with_host(net(&pki), "https://127.0.0.1:1/search");
    let runner = EngineRunner::new(Arc::new(live), clock);

    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut answered = HashSet::new();
    let found = runner
        .run_engines_parallel(
            &["iqdb", "tracemoe"],
            &entry,
            &Arc::new(settings()),
            &mut candidates,
            &mut errors,
            None,
            None,
            None,
            Some(&mut answered),
        )
        .await
        .unwrap();
    assert!(found["iqdb"]);
    assert!(!found["tracemoe"]);
    assert_eq!(candidates.len(), 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("trace.moe: "), "{errors:?}");
    assert_eq!(answered, HashSet::from(["iqdb".to_owned()]));
}
