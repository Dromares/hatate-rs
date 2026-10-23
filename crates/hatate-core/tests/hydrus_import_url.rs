//! `core/hydrus_import.py`, the URL-import half (Phase 4.d, DAN-2131), through `HydrusClient::new`
//! (the binaries' constructor: rustls with the explicit provider) against a scripted local Hydrus,
//! plain http and https. The sites a match points at (the redirect probe, the file download) are a
//! second local https server reached through `NetConfig::with_host_override`. Every answer is
//! scripted, every access key is a synthetic placeholder, and no live service is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2131-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-import-url.md`.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Recorded, Reply, Sandbox, ScriptedEngines, TestPki, TestServer};
use hatate_core::clock::ManualClock;
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::engine_runner::EngineRunner;
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::HydrusClient;
use hatate_core::hydrus_import as hi;
use hatate_core::model::{Candidate, Entry};
use hatate_core::net::{self, NetClient, NetConfig};
use hatate_core::search_engine::SearchEngine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const NEW: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCAL: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn hydrus_settings(url: &str) -> HydrusSettings {
    HydrusSettings {
        api_url: url.to_owned(),
        access_key: Secret::new(KEY),
        timeout: 5.0,
        ..HydrusSettings::default()
    }
}

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

/// The `url` query value of a request, percent-decoded.
fn url_param(q: &Recorded) -> String {
    let query = q.target.split_once('?').map_or("", |(_, q)| q);
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

const POST: fn() -> Value = || json!({"url_type": 0, "url_type_string": "post url", "match_name": "a file page", "can_parse": true});
const UNKNOWN: fn() -> Value = || json!({"url_type": 5, "url_type_string": "unknown url", "match_name": "unknown url", "can_parse": false});

/// A Hydrus that answers by endpoint. `info` is what `get_url_info` says about a URL (anything not
/// listed is an unknown URL).
#[derive(Clone)]
struct Fake {
    info: HashMap<String, Value>,
    fail: Vec<&'static str>,
    holds: bool,
}

impl Fake {
    fn new() -> Self {
        Fake {
            info: HashMap::new(),
            fail: Vec::new(),
            holds: true,
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
        match path {
            "/add_urls/get_url_info" => json_reply(
                self.info
                    .get(&url_param(q))
                    .cloned()
                    .unwrap_or_else(UNKNOWN),
            ),
            "/add_urls/add_url" => {
                json_reply(json!({"human_result_text": "URL added successfully."}))
            }
            // The file Hydrus is handed comes back under its own hash, like a real, non-transcoding
            // Hydrus does (DAN-17).
            "/add_files/add_file" => json_reply(json!({"status": 1, "hash": hex_sha256(&q.body)})),
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": NEW, "file_id": 7, "is_local": self.holds, "is_trashed": false, "is_deleted": !self.holds},
                {"hash": LOCAL, "file_id": 8, "is_local": self.holds, "is_trashed": false, "is_deleted": !self.holds},
            ]})),
            "/verify_access_key" => json_reply(json!({"permits_everything": true})),
            _ => json_reply(json!({})),
        }
    }
}

async fn hydrus(fake: Fake) -> (TestServer, HydrusClient) {
    let server = TestServer::http(move |q| fake.answer(q)).await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    (server, client)
}

fn targets(server: &TestServer) -> Vec<String> {
    server
        .recorded()
        .iter()
        .map(|q| path_of(q).to_owned())
        .collect()
}

/// The sites a match points at: one https server answering for each of `hosts`, and a search engine
/// whose net client reaches them.
struct Sites {
    server: TestServer,
    engine: SearchEngine,
    sandbox: Sandbox,
}

impl Sites {
    async fn new(
        hosts: &[&str],
        handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
    ) -> Sites {
        let pki = TestPki::generate_for(hosts);
        let server = TestServer::https(&pki, handler).await;
        let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
        for host in hosts {
            cfg = cfg.with_host_override(host, server.addr);
        }
        let net = NetClient::new(cfg).unwrap();
        let sandbox = Sandbox::new();
        let clock = Arc::new(ManualClock::new(1_792_758_896.0, 0.0));
        let runner = EngineRunner::new(Arc::new(ScriptedEngines::default()), clock.clone());
        let engine = SearchEngine::with_recorder(net, runner, sandbox.paths.clone(), clock, None);
        Sites {
            server,
            engine,
            sandbox,
        }
    }

    fn settings(&self) -> Mutex<Settings> {
        Mutex::new(Settings::with_defaults(&self.sandbox.paths))
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hatate-dan2131-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_bytes(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let v = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503) ^ seed.wrapping_mul(97);
        *p = image::Rgb([(v >> 3) as u8, (v >> 9) as u8, (v >> 15) as u8]);
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

/// An entry for a real file on disk, selected on a candidate for `url`.
fn entry_for(dir: &Path, url: &str, direct: Option<&str>) -> Entry {
    let path = dir.join("a.png");
    std::fs::write(&path, png_bytes(40, 40, 1)).unwrap();
    let mut e = Entry::new(path.to_str().unwrap());
    let mut c = Candidate::new(url);
    c.engine = "Google Lens".to_owned();
    c.similarity = 100.0;
    c.direct_file_url = direct.map(str::to_owned);
    c.booru_tags_fetched = true;
    e.candidates.push(c);
    e.select_candidate(0);
    e
}

// ---- url_import_refusal ----

#[tokio::test]
async fn hydrus_s_own_answers_decide_which_urls_the_importer_is_not_handed() {
    // The answers a live Hydrus returned (test_search_quality.TestHydrusUrlImporterGuard).
    let fake = Fake::new()
        .knows(
            "https://hdporncomics.com/tramp-gang-tekuho-sex-comic/",
            json!({"url_type": 5, "url_type_string": "unknown url", "match_name": "unknown url",
                   "can_parse": false, "cannot_parse_reason": "unknown url class"}),
        )
        .knows(
            "https://www.reddit.com/r/YanchaGalAnjouSan/",
            json!({"url_type": 3, "url_type_string": "gallery url",
                   "match_name": "Reddit | Subreddit - Sort hot", "can_parse": true}),
        )
        .knows(
            "https://www.reddit.com/r/YanchaGalAnjouSan/comments/1vz8vo8/manga_cover/",
            json!({"url_type": 0, "url_type_string": "post url", "match_name": "Reddit | Post", "can_parse": true}),
        )
        .knows(
            "https://i.redd.it/ogpxyscp4slh1.png",
            json!({"url_type": 5, "url_type_string": "unknown url", "match_name": "unknown url",
                   "can_parse": false, "cannot_parse_reason": "unknown url class"}),
        )
        .knows(
            "https://example.test/watch",
            json!({"url_type": 4, "url_type_string": "watchable url", "match_name": "A thread"}),
        )
        .knows(
            "https://example.test/post/9",
            json!({"url_type": 0, "url_type_string": "post url", "match_name": "A post",
                   "can_parse": false, "cannot_parse_reason": "the parser is broken"}),
        )
        .knows(
            "https://example.test/post/10",
            json!({"url_type": 0, "url_type_string": "post url", "match_name": "A post", "can_parse": false}),
        );
    let (server, client) = hydrus(fake).await;
    let refusal = |url: &'static str| {
        let client = &client;
        async move { hi::url_import_refusal(client, url).await.unwrap() }
    };

    let page = refusal("https://hdporncomics.com/tramp-gang-tekuho-sex-comic/").await;
    assert!(page.unwrap().contains("no downloader for hdporncomics.com"));
    let gallery = refusal("https://www.reddit.com/r/YanchaGalAnjouSan/").await;
    assert_eq!(
        gallery.unwrap(),
        "Hydrus sees this as a gallery url (Reddit | Subreddit - Sort hot) - importing it would \
         download everything there, not this one image"
    );
    assert!(
        refusal("https://example.test/watch")
            .await
            .unwrap()
            .contains("watchable url")
    );
    // A post and a direct file link go through.
    assert_eq!(
        refusal("https://www.reddit.com/r/YanchaGalAnjouSan/comments/1vz8vo8/manga_cover/").await,
        None
    );
    assert_eq!(refusal("https://i.redd.it/ogpxyscp4slh1.png").await, None);
    // A post Hydrus recognises but cannot parse says why, and "no parser" when it does not.
    assert_eq!(
        refusal("https://example.test/post/9").await.unwrap(),
        "Hydrus recognises this as A post but cannot parse it (the parser is broken)"
    );
    assert_eq!(
        refusal("https://example.test/post/10").await.unwrap(),
        "Hydrus recognises this as A post but cannot parse it (no parser)"
    );
    assert_eq!(server.request_count(), 7);
}

#[tokio::test]
async fn hydrus_not_answering_does_not_block_imports() {
    let fake = Fake {
        fail: vec!["/add_urls/get_url_info"],
        ..Fake::new()
    };
    let (_server, client) = hydrus(fake).await;
    assert_eq!(
        hi::url_import_refusal(&client, "https://example.com/page")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn an_answer_that_is_not_an_object_is_pythons_attribute_error() {
    let server = TestServer::http(|_| json_reply(json!([1, 2]))).await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let err = hi::url_import_refusal(&client, "https://example.com/page")
        .await
        .unwrap_err();
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
}

#[tokio::test]
async fn an_address_python_cannot_parse_is_told_to_the_person_without_library_words() {
    // DAN-2169 condition 3. Python's `urlparse` raises `ValueError("Invalid IPv6 URL")`; the Rust
    // error carries the class, and what a person is shown is Hatate's own text (differential
    // class D2: only the internal `message()` differs).
    let net = NetClient::new(NetConfig::default()).unwrap();
    let err = hi::forwarded_url(&net, "https://[::1/x").await.unwrap_err();
    assert!(matches!(err, hi::UrlImportError::Url(_)), "{err:?}");
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    let shown = err.user_message();
    assert_eq!(
        shown,
        "The address of this match is not one Hatate can read, so it was not sent."
    );
    for word in ["IPv6", "ValueError", "urlparse", "[::1", "Invalid"] {
        assert!(!shown.contains(word), "{word} in {shown:?}");
    }
}

// ---- send_url_to_importer ----

#[tokio::test]
async fn a_refused_url_is_not_sent_or_marked_sent() {
    let url = "https://hdporncomics.com/tramp-gang-tekuho-sex-comic/";
    let (server, client) = hydrus(Fake::new()).await;
    // The redirect probe the refusal triggers: a host that does not move.
    let sites = Sites::new(&["hdporncomics.com"], |_| Reply::ok("page")).await;
    let dir = scratch("refused");
    let mut entry = entry_for(&dir, url, None);

    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();

    assert!(!result.success && result.refused);
    assert!(result.error.as_deref().unwrap().contains("no downloader"));
    assert_eq!(
        entry.error_message.as_deref(),
        Some(format!("Not sent: {}", result.error.as_ref().unwrap()).as_str())
    );
    assert!(!targets(&server).contains(&"/add_urls/add_url".to_owned()));
    assert!(!entry.sent_to_hydrus);
    let probes = sites.server.recorded();
    assert_eq!(probes.len(), 1);
    assert_eq!(probes[0].method, "HEAD");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_rewritten_url_is_what_hydrus_is_asked_about_and_sent() {
    let fake = Fake::new().knows("https://danbooru.donmai.us/posts/7727191", POST());
    let (server, client) = hydrus(fake).await;
    let sites = Sites::new(&[], |_| Reply::ok("")).await;
    let dir = scratch("rewrite");
    let mut entry = entry_for(&dir, "https://danbooru.donmai.us/post/show/7727191", None);

    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();

    assert_eq!(result, hi::ImportResult::ok());
    let seen = server.recorded();
    assert_eq!(path_of(&seen[0]), "/add_urls/get_url_info");
    assert_eq!(
        url_param(&seen[0]),
        "https://danbooru.donmai.us/posts/7727191"
    );
    let body: Value = serde_json::from_slice(&seen[1].body).unwrap();
    assert_eq!(body["url"], "https://danbooru.donmai.us/posts/7727191");
    assert_eq!(body["show_destination_page"], false);
    // Queued, not confirmed, and no file was uploaded (test_session
    // `test_url_importer_is_sent_but_not_confirmed`, `a_hydrus_that_accepts_the_url_is_used_as_before`).
    assert_eq!(result.confirmed, None);
    assert!(!targets(&server).contains(&"/add_files/add_file".to_owned()));
    assert!(entry.sent_to_hydrus && !entry.hydrus_import_confirmed);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn nothing_to_send_is_skipped_not_failed() {
    let (server, client) = hydrus(Fake::new()).await;
    let sites = Sites::new(&[], |_| Reply::ok("")).await;
    let mut none = Entry::new("/x/a.png");
    let r = hi::send_url_to_importer(&mut none, &client, &sites.engine.net, None)
        .await
        .unwrap();
    assert_eq!(r.skipped_reason.as_deref(), Some("no matched URL"));
    assert!(!r.success && !r.refused && r.error.is_none());

    let dir = scratch("skip");
    let mut odd = entry_for(&dir, "ftp://example.test/a.png", None);
    let r = hi::send_url_to_importer(&mut odd, &client, &sites.engine.net, None)
        .await
        .unwrap();
    assert_eq!(
        r.skipped_reason.as_deref(),
        Some("matched URL isn't http(s)")
    );
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_forwarding_legacy_link_is_followed_and_repointed() {
    let view = "https://deviantart.com/view/654484001";
    let art = "https://www.deviantart.com/chierru/art/2016-%7C-Year-Ender-Illustration-654484001";
    let fake = Fake::new().knows(art, POST());
    let (server, client) = hydrus(fake).await;
    let sites = Sites::new(&["deviantart.com", "www.deviantart.com"], move |q| {
        if q.header("host") == Some("deviantart.com") {
            Reply::new(302, "").header("Location", art)
        } else {
            Reply::ok("")
        }
    })
    .await;
    let dir = scratch("forward");
    let mut entry = entry_for(&dir, view, None);

    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();

    assert!(result.success, "{result:?}");
    let seen = server.recorded();
    let sent: Value = serde_json::from_slice(&seen.last().unwrap().body).unwrap();
    assert_eq!(sent["url"], art);
    assert_eq!(entry.matched_url(), Some(art));
    assert_eq!(entry.selected_candidate().unwrap().url, art);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failed_request_is_not_mistaken_for_a_refusal() {
    let url = "https://www.deviantart.com/a/art/b-1";
    let fake = Fake {
        fail: vec!["/add_urls/add_url"],
        ..Fake::new().knows(url, POST())
    };
    let (_server, client) = hydrus(fake).await;
    let sites = Sites::new(&[], |_| Reply::ok("")).await;
    let dir = scratch("failed");
    let mut entry = entry_for(&dir, url, Some("https://images.example.test/b.png"));

    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();

    assert!(!result.success && !result.refused);
    assert_eq!(result.fault.as_ref().unwrap().kind(), ErrorKind::HttpStatus);
    assert_eq!(entry.error_message, result.error);
    assert!(!entry.sent_to_hydrus);
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the two schemes, timeouts, errors ----

#[tokio::test]
async fn the_importer_over_https_with_certificate_checks_on_is_refused_and_off_it_succeeds() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |q| match path_of(q) {
        "/add_urls/get_url_info" => json_reply(POST()),
        _ => json_reply(json!({"human_result_text": "URL added successfully."})),
    })
    .await;
    let sites = Sites::new(&[], |_| Reply::ok("")).await;
    let dir = scratch("https");
    let url = "https://www.deviantart.com/a/art/b-1";

    // On (the default): the handshake is refused, so nothing is asked of the server, and the
    // refusal check (which is allowed to fail) lets the URL through to the send, which fails.
    let on = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let mut entry = entry_for(&dir, url, None);
    let result = hi::send_url_to_importer(&mut entry, &on, &sites.engine.net, None)
        .await
        .unwrap();
    assert!(!result.success && !result.refused);
    let fault = result.fault.as_ref().unwrap();
    assert_eq!(fault.kind(), ErrorKind::Net(net::ErrorKind::Tls), "{fault}");
    assert_eq!(server.request_count(), 0);

    // Off: a self-signed server is reached.
    let mut hs = hydrus_settings(&server.url(""));
    hs.verify_ssl = false;
    let off = HydrusClient::new(&hs).unwrap();
    let mut entry = entry_for(&dir, url, None);
    let result = hi::send_url_to_importer(&mut entry, &off, &sites.engine.net, None)
        .await
        .unwrap();
    assert_eq!(result, hi::ImportResult::ok());
    assert_eq!(server.request_count(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_timeout_and_a_dropped_connection_fail_the_send_with_a_user_message_that_leaks_nothing() {
    let sites = Sites::new(&[], |_| Reply::ok("")).await;
    let dir = scratch("errors");
    let url = "https://www.deviantart.com/a/art/b-1?token=SECRET";

    let slow =
        TestServer::http(|_| json_reply(json!({})).delayed(std::time::Duration::from_secs(3)))
            .await;
    let mut hs = hydrus_settings(&slow.url(""));
    hs.timeout = 0.3;
    let client = HydrusClient::new(&hs).unwrap();
    let mut entry = entry_for(&dir, url, None);
    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();
    let fault = result.fault.as_ref().unwrap();
    assert_eq!(
        fault.kind(),
        ErrorKind::Net(net::ErrorKind::Timeout),
        "{fault}"
    );
    let user = fault.user_message();
    assert!(
        !user.contains("os error")
            && !user.contains(KEY)
            && !user.contains("SECRET")
            && !user.contains('?'),
        "{user}"
    );

    let dropped = TestServer::http(|_| Reply::aborted()).await;
    let client = HydrusClient::new(&hydrus_settings(&dropped.url(""))).unwrap();
    let mut entry = entry_for(&dir, url, None);
    let result = hi::send_url_to_importer(&mut entry, &client, &sites.engine.net, None)
        .await
        .unwrap();
    let fault = result.fault.as_ref().unwrap();
    assert!(matches!(fault.kind(), ErrorKind::Net(_)), "{fault}");
    let user = fault.user_message();
    assert!(
        !user.contains("os error") && !user.contains(KEY) && !user.contains("SECRET"),
        "{user}"
    );
    assert!(
        entry
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("Could not reach Hydrus at ")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_dropped_connection_in_the_redirect_probe_is_not_a_redirect_and_not_an_error() {
    // Python's `_forwarded_url` catches `requests.RequestException`: a connection that goes away
    // is one, so the probe says "nowhere else" (`None`) and raises nothing.
    let sites = Sites::new(&["gone.example.test"], |_| Reply::aborted()).await;
    assert_eq!(
        hi::forwarded_url(&sites.engine.net, "https://gone.example.test/x")
            .await
            .unwrap(),
        None,
        "a dropped connection is a RequestException: not a redirect, not an error"
    );
}

#[tokio::test]
async fn a_probe_with_no_time_left_leaves_the_function_like_hard_timeout_error() {
    // `HardTimeoutError` is not a `requests.RequestException`, so Python's `_forwarded_url` lets it
    // propagate. The net layer raises it when no time is left before a request starts (waiting for
    // the response to begin is a plain timeout instead, below), so a probe given a spent deadline
    // reaches that branch without waiting the shipped 15 s, and without asking the server.
    let sites = Sites::new(&["slow.example.test"], |_| Reply::ok("")).await;
    let err = hi::forwarded_url_within(&sites.engine.net, "https://slow.example.test/x", 0.0)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            hi::UrlImportError::Remote(hatate_core::remote::Raised::HardTimeout(_))
        ),
        "{err:?}"
    );
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::DeadlineExceeded));
    assert_eq!(sites.server.request_count(), 0);
}

#[tokio::test]
async fn a_server_that_does_not_answer_the_probe_in_time_is_a_timeout_not_a_hard_deadline() {
    // Waiting for the response to begin ends as a plain `requests` timeout, which `_forwarded_url`
    // catches: "nowhere else", no error.
    let sites = Sites::new(&["slow.example.test"], |_| {
        Reply::ok("").delayed(Duration::from_secs(5))
    })
    .await;
    let started = Instant::now();
    assert_eq!(
        hi::forwarded_url_within(&sites.engine.net, "https://slow.example.test/x", 0.3)
            .await
            .unwrap(),
        None
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn the_redirect_probe_ships_with_python_s_fifteen_second_deadline() {
    // Python: `net.request("HEAD", url, timeout=15, deadline=15, ...)`.
    assert_eq!(hi::FORWARD_CHECK_TIMEOUT, 15.0);
}

// ---- download_and_send, send_url_or_download ----

fn site_serving(file: Vec<u8>) -> impl Fn(&Recorded) -> Reply + Send + Sync + 'static {
    move |_| Reply::ok(file.clone()).header("Content-Type", "image/png")
}

#[tokio::test]
async fn hatate_downloads_what_hydrus_cannot() {
    let file = png_bytes(64, 64, 5);
    let sites = Sites::new(&["e-shuushuu.net"], site_serving(file.clone())).await;
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("fallback");
    let mut entry = entry_for(
        &dir,
        "http://e-shuushuu.net/image/976339/",
        Some("https://e-shuushuu.net/images/2019-01-01-976339.png"),
    );
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();

    let result = hi::send_url_or_download(
        &mut entry,
        &client,
        &sites.engine,
        Some(&sites.settings()),
        &tmp,
    )
    .await
    .unwrap();

    assert!(result.success, "{result:?}");
    assert_eq!(result.confirmed, Some(true));
    assert!(
        result
            .note
            .as_deref()
            .unwrap()
            .contains("Hatate downloaded it")
    );
    let t = targets(&server);
    assert!(!t.contains(&"/add_urls/add_url".to_owned()));
    let upload = server
        .recorded()
        .into_iter()
        .find(|q| path_of(q) == "/add_files/add_file")
        .unwrap();
    assert_eq!(
        upload.body, file,
        "the downloaded bytes, not the local file"
    );
    assert_eq!(
        entry.hydrus_hash.as_deref(),
        Some(hex_sha256(&file).as_str())
    );
    assert!(entry.sent_to_hydrus && entry.hydrus_import_confirmed);
    assert_eq!(entry.error_message, None);
    // The file was written under the temp folder, named after the entry.
    assert!(tmp.join("a_matched.png").exists());
    // The download carried the match page as its Referer.
    let get = &sites.server.recorded()[0];
    assert_eq!(get.method, "GET");
    assert_eq!(
        get.header("referer"),
        Some("http://e-shuushuu.net/image/976339/")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn nothing_that_can_reach_the_original_says_so() {
    let sites = Sites::new(&["anime-pictures.net"], |_| Reply::ok("")).await;
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("nothing");
    let mut entry = entry_for(
        &dir,
        "https://anime-pictures.net/pictures/view_post/772314",
        None,
    );

    let result = hi::send_url_or_download(
        &mut entry,
        &client,
        &sites.engine,
        Some(&sites.settings()),
        &dir,
    )
    .await
    .unwrap();

    assert!(!result.success && result.refused);
    let error = result.error.unwrap();
    assert!(error.contains("Neither Hydrus nor Hatate"), "{error}");
    assert!(error.contains("Send (upload)"));
    assert!(error.contains("anime-pictures.net"));
    assert!(!targets(&server).contains(&"/add_files/add_file".to_owned()));
    assert!(!entry.sent_to_hydrus);
    assert_eq!(entry.error_message, Some(format!("Not sent: {error}")));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_exact_copy_is_uploaded_from_disk_when_hydrus_cannot_take_the_url() {
    // Pawchive found this file by its own hash, so the local copy is the original.
    let sites = Sites::new(&["pawchive.pw"], |_| Reply::ok("never asked")).await;
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("exact");
    let mut entry = entry_for(&dir, "https://pawchive.pw/fanbox/user/1/post/2", None);
    entry.candidates[0].engine = "Pawchive".to_owned();

    let result = hi::send_url_or_download(
        &mut entry,
        &client,
        &sites.engine,
        Some(&sites.settings()),
        &dir,
    )
    .await
    .unwrap();

    assert!(result.success, "{result:?}");
    assert_eq!(result.confirmed, Some(true));
    assert!(
        result
            .note
            .as_deref()
            .unwrap()
            .starts_with("pawchive.pw has this exact file")
    );
    let upload = server
        .recorded()
        .into_iter()
        .find(|q| path_of(q) == "/add_files/add_file")
        .unwrap();
    assert_eq!(upload.body, std::fs::read(&entry.path).unwrap());
    // Only the redirect probe the refusal triggers (a HEAD); no GET for the file.
    let seen = sites.server.recorded();
    assert!(
        seen.iter().all(|q| q.method == "HEAD"),
        "nothing was downloaded"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_web_page_in_place_of_the_image_is_not_uploaded() {
    let sites = Sites::new(&["e-shuushuu.net"], |_| {
        Reply::ok("\n  <!DOCTYPE html><html><body>Log in</body>")
    })
    .await;
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("webpage");
    let mut entry = entry_for(
        &dir,
        "http://e-shuushuu.net/image/1/",
        Some("https://e-shuushuu.net/images/x.jpeg"),
    );

    let result = hi::download_and_send(
        &mut entry,
        &client,
        &sites.engine.net,
        30.0,
        &dir,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap();

    assert!(!result.success);
    assert_eq!(
        result.error.as_deref(),
        Some(
            "e-shuushuu.net answered with a web page instead of the image - it may only serve \
             originals to logged-in accounts"
        )
    );
    let fault = result.fault.unwrap();
    assert_eq!(fault.kind(), ErrorKind::DownloadInvalid);
    assert!(!fault.user_message().contains("e-shuushuu"));
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_downloaded_size_must_match_the_size_the_head_reported() {
    // BA-02: Content-Length from the HEAD must match what arrived.
    let file = png_bytes(64, 64, 5);
    let sites = Sites::new(&["e-shuushuu.net"], site_serving(file.clone())).await;
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("size");
    let mut entry = entry_for(
        &dir,
        "http://e-shuushuu.net/image/1/",
        Some("https://e-shuushuu.net/images/x.png"),
    );
    entry.candidates[0].remote_size_bytes = Some(1000);

    let result = hi::download_and_send(
        &mut entry,
        &client,
        &sites.engine.net,
        30.0,
        &dir,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap();

    assert!(!result.success);
    assert_eq!(
        result.error,
        Some(format!(
            "Downloaded {} bytes but expected 1000 bytes (Content-Length from e-shuushuu.net) - \
             transfer may be truncated or corrupted",
            file.len()
        ))
    );
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn bytes_that_are_not_the_claimed_format_or_do_not_decode_are_not_uploaded() {
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("validate");
    let png = png_bytes(64, 64, 5);
    let mut cut = png.clone();
    cut.truncate(png.len() / 2);
    for (body, claimed, expect) in [
        (
            png.clone(),
            Some("JPEG"),
            "Downloaded file validation failed: ",
        ),
        (
            cut,
            Some("PNG"), // the decode check only runs when a format is claimed
            "Downloaded file validation failed: Downloaded data does not decode",
        ),
        (
            vec![7u8; 40],
            None,
            "Downloaded file validation failed: Downloaded file is implausibly small (40 bytes)",
        ),
    ] {
        let sites = Sites::new(&["cdn.example.test"], site_serving(body)).await;
        let mut entry = entry_for(
            &dir,
            "https://cdn.example.test/p/1",
            Some("https://cdn.example.test/f/a.png"),
        );
        entry.candidates[0].remote_format = claimed.map(str::to_owned);
        let result = hi::download_and_send(
            &mut entry,
            &client,
            &sites.engine.net,
            30.0,
            &dir,
            None,
            &*sites.engine.clock,
        )
        .await
        .unwrap();
        assert!(!result.success, "{expect}: {result:?}");
        assert!(
            result.error.as_deref().unwrap().starts_with(expect),
            "{:?}",
            result.error
        );
    }
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_hash_hydrus_returns_that_is_not_the_files_is_refused_before_the_url_and_tags() {
    let file = png_bytes(64, 64, 5);
    let sites = Sites::new(&["cdn.example.test"], site_serving(file.clone())).await;
    let server = TestServer::http(|q| match path_of(q) {
        "/add_files/add_file" => json_reply(json!({"status": 1, "hash": NEW})),
        _ => json_reply(json!({})),
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let dir = scratch("mismatch");
    let mut entry = entry_for(
        &dir,
        "https://cdn.example.test/p/1",
        Some("https://cdn.example.test/f/a.png"),
    );

    let result = hi::download_and_send(
        &mut entry,
        &client,
        &sites.engine.net,
        30.0,
        &dir,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap();

    assert!(!result.success);
    let error = result.error.unwrap();
    assert!(error.starts_with("Hash mismatch for a.png ("), "{error}");
    assert!(error.contains(&format!("downloaded file hash={}", hex_sha256(&file))));
    assert!(error.contains(&format!("Hydrus returned hash={NEW}")));
    assert_eq!(result.fault.unwrap().kind(), ErrorKind::HashMismatch);
    assert_eq!(targets(&server), ["/add_files/add_file"]);
    assert!(!entry.sent_to_hydrus);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_download_that_fails_and_one_that_cannot_be_written_are_told_apart() {
    let (_server, client) = hydrus(Fake::new()).await;
    let dir = scratch("fails");
    let sites = Sites::new(&["cdn.example.test"], |_| Reply::new(404, "gone")).await;
    let mut entry = entry_for(
        &dir,
        "https://cdn.example.test/p/1",
        Some("https://cdn.example.test/f/a.png"),
    );
    let result = hi::download_and_send(
        &mut entry,
        &client,
        &sites.engine.net,
        30.0,
        &dir,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap();
    assert_eq!(
        result.error.as_deref(),
        Some("Could not download the image from https://cdn.example.test/f/a.png")
    );
    assert_eq!(result.fault.unwrap().kind(), ErrorKind::DownloadFailed);

    // A folder that does not exist: Python's OSError leaves the function.
    let sites = Sites::new(&["cdn.example.test"], site_serving(png_bytes(64, 64, 5))).await;
    let mut entry = entry_for(
        &dir,
        "https://cdn.example.test/p/1",
        Some("https://cdn.example.test/f/a.png"),
    );
    let missing = dir.join("no-such-folder");
    let err = hi::download_and_send(
        &mut entry,
        &client,
        &sites.engine.net,
        30.0,
        &missing,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert!(
        err.message()
            .starts_with("[Errno 2] No such file or directory: '"),
        "{err}"
    );
    let user = err.user_message();
    assert!(
        !user.contains("no-such-folder") && !user.contains("Errno") && !user.contains("os error"),
        "{user}"
    );

    // Nothing to download.
    let mut bare = entry_for(&dir, "https://cdn.example.test/p/1", None);
    let r = hi::download_and_send(
        &mut bare,
        &client,
        &sites.engine.net,
        30.0,
        &dir,
        None,
        &*sites.engine.clock,
    )
    .await
    .unwrap();
    assert_eq!(
        r.skipped_reason.as_deref(),
        Some("no direct file URL available")
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- relate_confirmed_url_import / finish_url_import ----

struct FixedClock;
impl hatate_core::clock::Clock for FixedClock {
    fn wall(&self) -> f64 {
        1_792_758_896.0
    }
    fn monotonic(&self) -> f64 {
        0.0
    }
    fn sleep(&self, _: f64) -> hatate_core::clock::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    f(&mut s);
    s
}

#[tokio::test]
async fn with_the_setting_off_a_confirmed_url_import_costs_nothing() {
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("relate-off");
    let entry = entry_for(&dir, "https://example.test/p/1", None);
    let off = settings_with(|s| s.set_hydrus_duplicate_relationships = false);
    for settings in [None, Some(&off)] {
        let got = hi::relate_confirmed_url_import(&entry, &client, settings, Some(LOCAL), NEW)
            .await
            .unwrap();
        assert_eq!(got, None);
    }
    let got = hi::finish_url_import(&entry, &client, Some(&off), Some(LOCAL), NEW, &FixedClock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn nothing_is_related_without_a_local_hash_or_when_hydrus_landed_on_the_same_file() {
    let (server, client) = hydrus(Fake::new()).await;
    let dir = scratch("relate-none");
    let entry = entry_for(&dir, "https://example.test/p/1", None);
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    for local in [None, Some(""), Some(NEW), Some(&NEW.to_uppercase()[..])] {
        let got = hi::relate_confirmed_url_import(&entry, &client, Some(&on), local, NEW)
            .await
            .unwrap();
        assert_eq!(got, None, "{local:?}");
    }
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_confirmed_file_is_read_back_and_paired_as_the_better_copy() {
    let dir = scratch("relate-on");
    let mut entry = entry_for(&dir, "https://example.test/p/1", None);
    let pic = png_bytes(64, 64, 9);
    std::fs::write(&entry.path, &pic).unwrap();
    entry.hydrus_hash = Some(LOCAL.to_owned());
    let served = pic.clone();
    let fake = Fake::new();
    let server = TestServer::http(move |q| {
        if q.target.starts_with("/get_files/file_metadata")
            && q.target.contains("only_return_identifiers")
        {
            json_reply(json!({"metadata": [{"hash": NEW, "file_id": 7}]}))
        } else if q.target.starts_with("/get_files/file?file_id=7") {
            Reply::ok(served.clone())
        } else {
            fake.answer(q)
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);

    let got = hi::relate_confirmed_url_import(&entry, &client, Some(&on), Some(LOCAL), NEW)
        .await
        .unwrap();

    assert_eq!(got, None);
    let t = targets(&server);
    assert!(t.contains(&"/get_files/file".to_owned()), "{t:?}");
    let rel = server
        .recorded()
        .into_iter()
        .find(|q| path_of(q) == "/manage_file_relationships/set_file_relationships")
        .expect("the relationship was written");
    let body: Value = serde_json::from_slice(&rel.body).unwrap();
    assert_eq!(body["relationships"][0]["hash_a"], NEW);
    assert_eq!(body["relationships"][0]["hash_b"], LOCAL);
    assert_eq!(body["relationships"][0]["relationship"], 4);
    assert!(t.contains(&"/manage_file_relationships/set_kings".to_owned()));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_read_back_failure_becomes_a_warning_not_a_failed_import() {
    // test_url_importer_duplicate_relationships.TestReadBackFailures
    let dir = scratch("relate-fail");
    let entry = entry_for(&dir, "https://example.test/p/1", None);
    let server = TestServer::http(|q| {
        if q.target.starts_with("/get_files/file_metadata") {
            json_reply(json!({"metadata": [{"hash": NEW, "file_id": null}]}))
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);

    let got = hi::finish_url_import(&entry, &client, Some(&on), Some(LOCAL), NEW, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        got.as_deref(),
        Some(
            "Imported, but not marked as a duplicate: could not read the imported file back out \
             of Hydrus to compare it against your copy"
        )
    );
    let _ = std::fs::remove_dir_all(dir);
}
