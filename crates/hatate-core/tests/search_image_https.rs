//! `search_image` end to end (DAN-2064, Phase 3.h), through `NetClient::new` (rustls with the
//! explicit provider) against a local https server. The engines answer from a script
//! (`common::ScriptedEngines`); the booru page, the cache and the status decision are the real
//! ones. Site hosts are reached through `NetConfig::with_host_override`. No live network.
//!
//! The transport differential against the real Python (`worktrees/DAN-2064-differential.log`) is
//! the broad check; these are the named, readable ones. The Python test each maps to is in
//! `docs/search-engine.md`.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, Reply, ScriptedEngines, Step, TestPki, TestServer, iqdb_match};
use hatate_core::clock::ManualClock;
use hatate_core::config::Settings;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::engines::iqdb::IqdbError;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::search_engine::{SearchEngine, SearchOptions};

const NOW: f64 = 1_792_725_790.0;
const UA: &str = "hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)";
const POST: &str = r#"{"id": 5, "image_width": 800, "image_height": 600, "rating": "g",
    "file_url": "https://cdn.donmai.us/f.png",
    "tag_string_general": "one two three four five six", "tag_string_artist": "someone"}"#;
const URL: &str = "https://danbooru.donmai.us/posts/5";

/// A profile in a temp dir and a small PNG to search for.
struct SandboxHome {
    dir: tempfile::TempDir,
    paths: Paths,
}

impl SandboxHome {
    fn new() -> SandboxHome {
        let dir = tempfile::Builder::new()
            .prefix("dan2064-search-")
            .tempdir()
            .unwrap();
        let home: std::ffi::OsString = dir.path().join("home").into();
        let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
        SandboxHome { dir, paths }
    }

    /// A 4x3 PNG whose pixels are fixed, so its hash (the cache key) is the same on every run.
    fn image(&self, name: &str) -> std::path::PathBuf {
        let path = self.dir.path().join(name);
        if !path.exists() {
            let img =
                image::RgbImage::from_fn(4, 3, |x, y| image::Rgb([x as u8 * 40, y as u8 * 60, 7]));
            img.save(&path).unwrap();
        }
        path
    }

    fn cache_files(&self) -> Vec<String> {
        let dir = self.paths.search_cache_dir();
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|it| {
                it.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

struct World {
    server: TestServer,
    engine: SearchEngine,
    engines: Arc<ScriptedEngines>,
    settings: Mutex<Settings>,
    home: SandboxHome,
}

async fn world(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> World {
    world_for_hosts(&["danbooru.donmai.us"], handler).await
}

/// `world`, with more hosts routed to the local server (the CDN a post's file lives on).
async fn world_for_hosts(
    hosts: &[&str],
    handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
) -> World {
    let pki = TestPki::generate_for(hosts);
    let server = TestServer::https(&pki, handler).await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, server.addr);
    }
    let net = NetClient::new(cfg).unwrap();
    let home = SandboxHome::new();
    let engines = Arc::new(ScriptedEngines::default());
    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = SearchEngine::with_recorder(net, runner, home.paths.clone(), clock, None);
    let mut settings = Settings::with_defaults(&home.paths);
    settings.primary_engine = "iqdb".to_owned();
    settings.secondary_engine_mode = "disabled".to_owned();
    settings.enable_ascii2d = false;
    settings.enable_tracemoe = false;
    settings.enable_pawchive_index = false;
    settings.match_conditions.require_larger_than_local = false;
    World {
        server,
        engine,
        engines,
        settings: Mutex::new(settings),
        home,
    }
}

impl World {
    fn entry(&self) -> Entry {
        Entry::new(self.home.image("a.png").to_str().unwrap())
    }

    async fn search(&self, entry: &mut Entry) {
        self.engine
            .search_image(entry, &self.settings, SearchOptions::default())
            .await
            .unwrap();
    }

    fn seen(&self) -> Vec<String> {
        self.server
            .recorded()
            .iter()
            .map(|r| {
                format!(
                    "{} {} {}",
                    r.method,
                    r.header("host").unwrap_or(""),
                    r.target
                )
            })
            .collect()
    }
}

fn json_reply(body: &str) -> Reply {
    Reply::ok(body).header("Content-Type", "application/json")
}

fn routes(table: Vec<(&'static str, Reply)>) -> impl Fn(&Recorded) -> Reply + Send + Sync {
    move |req| {
        table
            .iter()
            .find(|(needle, _)| req.target.contains(needle))
            .map_or_else(|| Reply::new(404, "no route"), |(_, reply)| reply.clone())
    }
}

/// The verifier's PASS names this test: one image searched through the binary's own constructor,
/// over https, from the engine's answer to the cache entry written.
#[tokio::test]
async fn search_image_over_https_through_the_binaries_constructor() {
    let w = world(routes(vec![("/posts/5.json", json_reply(POST))])).await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    w.search(&mut entry).await;

    assert_eq!(entry.status, MatchStatus::Good);
    assert_eq!(entry.candidates.len(), 1);
    let c = &entry.candidates[0];
    assert_eq!(c.url, URL);
    assert_eq!((c.width, c.height), (Some(800), Some(600)));
    assert_eq!(c.booru_tags.len(), 7, "six general tags and the artist");
    assert!(c.booru_tags_fetched);
    assert_eq!(entry.selected_candidate_index, 0);
    assert!(
        entry
            .tags
            .iter()
            .any(|t| t.name == "someone" && t.namespace.as_deref() == Some("artist"))
    );
    assert_eq!(w.engines.calls(), ["iqdb"]);
    assert_eq!(w.seen(), ["GET danbooru.donmai.us /posts/5.json"]);
    assert_eq!(w.server.recorded()[0].header("user-agent"), Some(UA));

    let written = w.home.cache_files();
    assert_eq!(written.len(), 1, "one cache entry for the one image");
    let hash = entry.hydrus_hash.as_deref().unwrap();
    assert_eq!(written[0], format!("{hash}.json"));
}

/// A second search of the same file is the cache's answer: no engine is asked, the page is not
/// fetched again, and the entry says where the result came from.
#[tokio::test]
async fn a_second_search_of_the_same_file_is_a_cache_hit() {
    let w = world(routes(vec![("/posts/5.json", json_reply(POST))])).await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut first = w.entry();
    w.search(&mut first).await;
    let requests_after_first = w.server.request_count();

    let mut second = w.entry();
    w.search(&mut second).await;
    assert_eq!(second.result_source.as_deref(), Some("cached"));
    assert_eq!(second.status, MatchStatus::Good);
    assert_eq!(second.candidates.len(), 1);
    assert_eq!(second.candidates[0].url, URL);
    assert_eq!(w.engines.calls(), ["iqdb"], "the engine was asked once");
    assert_eq!(
        w.server.request_count(),
        requests_after_first,
        "a cache hit fetches nothing"
    );
}

/// `use_cache = false` searches again even though the cache holds the image.
#[tokio::test]
async fn use_cache_false_searches_again() {
    let w = world(routes(vec![("/posts/5.json", json_reply(POST))])).await;
    for _ in 0..2 {
        w.engines
            .iqdb
            .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    }
    let mut first = w.entry();
    w.search(&mut first).await;
    let mut second = w.entry();
    w.engine
        .search_image(
            &mut second,
            &w.settings,
            SearchOptions {
                use_cache: false,
                ..SearchOptions::default()
            },
        )
        .await
        .unwrap();
    assert_ne!(second.result_source.as_deref(), Some("cached"));
    assert_eq!(w.engines.calls(), ["iqdb", "iqdb"]);
}

/// Network error: the booru page cannot be read, the match stays but has no tags, so the entry
/// is not `good`; the search itself does not fail.
#[tokio::test]
async fn a_network_error_on_the_booru_page_leaves_the_match_without_tags() {
    let w = world(|_| Reply::aborted()).await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    w.search(&mut entry).await;
    assert_eq!(entry.candidates.len(), 1);
    assert!(entry.candidates[0].booru_tags.is_empty());
    assert!(
        !entry.candidates[0].booru_tags_fetched,
        "a failed fetch says nothing about the post, so the page stays to be retried"
    );
    assert_ne!(entry.status, MatchStatus::Good);
    assert_ne!(entry.status, MatchStatus::Searching);
}

/// The flag the test above pins is what makes a later selection of the same candidate retry: once
/// the site answers, `fetch_candidate_details` reads the page it could not read the first time.
#[tokio::test]
async fn a_candidate_whose_booru_page_failed_is_retried_on_the_next_selection() {
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handler_up = up.clone();
    let w = world(move |_| {
        if handler_up.load(std::sync::atomic::Ordering::SeqCst) {
            json_reply(POST)
        } else {
            Reply::aborted()
        }
    })
    .await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    w.search(&mut entry).await;
    let mut candidate = entry.candidates[0].clone();
    assert!(candidate.booru_tags.is_empty());
    assert!(!candidate.booru_tags_fetched);

    up.store(true, std::sync::atomic::Ordering::SeqCst);
    w.engine
        .fetch_candidate_details(&mut candidate, &w.settings, None, None)
        .await
        .unwrap();
    assert!(candidate.booru_tags_fetched);
    assert_eq!(
        candidate.booru_tags.len(),
        7,
        "six general tags and the artist"
    );
}

/// Deadline: a booru page that trickles in past the settings' timeout does not hang the search.
#[tokio::test]
async fn a_booru_page_that_trickles_past_the_deadline_does_not_hang_the_search() {
    let w = world(|_| Reply::ok(vec![b'x'; 600]).trickled(1, Duration::from_millis(50))).await;
    w.settings.lock().unwrap().search_timeout = 0.5;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    let started = std::time::Instant::now();
    w.search(&mut entry).await;
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(entry.candidates[0].booru_tags.is_empty());
}

/// A page the site says is gone (404): the match is dropped as dead when the setting is on.
#[tokio::test]
async fn a_gone_page_is_dropped_and_the_image_is_not_found() {
    let w = world(|_| Reply::new(404, "gone")).await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    w.search(&mut entry).await;
    assert_eq!(entry.status, MatchStatus::NotFound);
    assert!(entry.candidates.is_empty());
}

/// Every engine failing is an `error` entry carrying the failure, and nothing is cached.
#[tokio::test]
async fn every_engine_failing_is_an_error_entry_and_is_not_cached() {
    let w = world(|_| Reply::new(500, "unused")).await;
    w.engines
        .iqdb
        .push(Step::Return(Err(IqdbError("iqdb down".to_owned()))));
    let mut entry = w.entry();
    w.search(&mut entry).await;
    assert_eq!(entry.status, MatchStatus::Error);
    assert!(entry.candidates.is_empty());
    assert!(
        entry
            .error_message
            .as_deref()
            .is_some_and(|m| m.contains("iqdb down")),
        "{:?}",
        entry.error_message
    );
    assert!(w.home.cache_files().is_empty());
    assert_eq!(w.server.request_count(), 0);
}

const CDN: &str = "cdn.donmai.us";

/// A post that states no file size, with a sample image: the match's format and size are not
/// known from the page, so the top pick is HEADed, and its preview is downloaded.
const POST_WITHOUT_SIZE: &str = r#"{"id": 5, "image_width": 800, "image_height": 600, "rating": "g",
    "file_url": "https://cdn.donmai.us/f.png", "large_file_url": "https://cdn.donmai.us/sample.jpg",
    "tag_string_general": "one two three four five six", "tag_string_artist": "someone"}"#;

fn cdn_world_routes() -> impl Fn(&Recorded) -> Reply + Send + Sync {
    |req| {
        if req.target.contains("/posts/5.json") {
            json_reply(POST_WITHOUT_SIZE)
        } else if req.method == "HEAD" {
            Reply::ok("")
                .header("Content-Type", "image/png")
                .content_length("4321")
        } else {
            Reply::ok(b"preview-bytes".to_vec()).header("Content-Type", "image/jpeg")
        }
    }
}

/// The format/size HEAD and the preview download go to hotlink-protected CDNs, so each carries
/// the post page as its `Referer` (`referer_for_candidate`). The out-of-repo differential saw 36
/// HEADs; nothing in the repo asserted either header until this.
#[tokio::test]
async fn the_format_head_and_the_preview_download_carry_the_post_page_as_referer() {
    let w = world_for_hosts(&["danbooru.donmai.us", CDN], cdn_world_routes()).await;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(URL, 96.5)])));
    let mut entry = w.entry();
    w.search(&mut entry).await;

    let c = &entry.candidates[0];
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://cdn.donmai.us/f.png")
    );
    assert_eq!(
        c.preview_url.as_deref(),
        Some("https://cdn.donmai.us/sample.jpg")
    );
    assert_eq!(
        (c.remote_format.as_deref(), c.remote_size_bytes),
        (Some("PNG"), Some(4321)),
        "the HEAD's answer reached the candidate"
    );
    assert_eq!(c.thumb_bytes.as_deref(), Some(&b"preview-bytes"[..]));

    let seen = w.seen();
    assert_eq!(
        seen,
        [
            "GET danbooru.donmai.us /posts/5.json",
            "GET cdn.donmai.us /sample.jpg",
            "HEAD cdn.donmai.us /f.png",
        ]
    );
    let recorded = w.server.recorded();
    let preview = &recorded[1];
    let head = &recorded[2];
    assert_eq!(preview.header("referer"), Some(URL), "the preview download");
    assert_eq!(head.header("referer"), Some(URL), "the format HEAD");
}
