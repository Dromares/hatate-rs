//! DAN-2059 (Phase 3.c): the request halves of ascii2d and SauceNAO, against a local **https**
//! replay server (`tests/common/replay.rs`), through the constructor the binaries use
//! (`NetClient::new`: rustls with the explicit CryptoProvider). No live network.
//!
//! - `mocked_transport` corpus items: `core.ascii2d.search` (3) and `core.saucenao.search` (14).
//!   The client makes the item's recorded requests in order and gives `expected.json`: the matches
//!   or the error and its message, the sleeps it asked for, and the module state it left (the
//!   `_blocked` latch; the quota reading and the daily-limit latch).
//! - the DAN-1909 rule: `ascii2d_and_saucenao_reach_a_local_https_server_through_the_binaries_constructor`.
//! - per client: one wall-clock deadline and one network error.
//! - the Python tests that exercise these functions, ported (the mapping is in the PR body).
//!
//! The API key in every request is the synthetic `SYNTHETIC_KEY`.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    MockedItem, Recorded, RecordedExt, ReplayServer, Reply, TestPki, mocked_items, same_json,
};
use hatate_core::clock::ManualClock;
use hatate_core::config::SauceNaoSettings;
use hatate_core::engines::ascii2d::{Ascii2dClient, Ascii2dError};
use hatate_core::engines::client::OnTick;
use hatate_core::engines::saucenao::{
    SHORT_WINDOW_SECONDS, SauceNaoClient, SauceNaoError, ShouldStop,
};
use hatate_core::net::{NetClient, NetConfig};
use serde_json::{Value, json};

const SYNTHETIC_KEY: &str = "synthetic-test-key-0000";
const CORPUS_KEY: &str = "<api-key>";

fn net(pki: &TestPki) -> NetClient {
    // The one constructor: installs the provider and builds the client. The throw-away CA is the
    // only extra trust anchor.
    NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap()
}

struct Image {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

/// A small file: under the upload cap, so it is sent as it is.
fn image(name: &str) -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2059-")
        .tempdir()
        .unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, b"a small picture, sent as it is").unwrap();
    Image { _dir: dir, path }
}

const UPLOAD: &[u8] = b"a small picture, sent as it is";

/// An address nothing listens on.
async fn closed_port() -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("https://127.0.0.1:{port}/")
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_700_000_000.0, 100.0))
}

fn settings(json_api: bool) -> SauceNaoSettings {
    SauceNaoSettings {
        api_key: SYNTHETIC_KEY.into(),
        use_json_api: json_api,
        ..SauceNaoSettings::default()
    }
}

/// `SauceNaoSettings` from a recorded `args.settings`, with the placeholder key made synthetic.
fn settings_of(recorded: &Value) -> SauceNaoSettings {
    let mut settings = SauceNaoSettings::default();
    for (key, value) in recorded.as_object().expect("settings is an object") {
        match key.as_str() {
            "api_key" => {
                assert_eq!(value, CORPUS_KEY);
                settings.api_key = SYNTHETIC_KEY.into();
            }
            "use_json_api" => settings.use_json_api = value.as_bool().unwrap(),
            "min_similarity" => settings.min_similarity = value.as_f64().unwrap(),
            "tag_types" => {
                settings.tag_types = value
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap().to_owned())
                    .collect();
            }
            "namespace" => settings.namespace = value.as_str().unwrap().to_owned(),
            "dbmask" => settings.dbmask = value.as_i64().unwrap(),
            "pause_search_on_quota_exhausted" => {
                settings.pause_search_on_quota_exhausted = value.as_bool().unwrap();
            }
            "__dataclass__" => {}
            other => panic!("unknown setting {other}"),
        }
    }
    settings
}

/// The form fields a recorded request carried, by name (`file_fields`), and no others.
fn check_upload(item: &MockedItem, request: &Recorded) {
    let fields: Vec<&str> = item.call["request"][0]["file_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let parts = request.form_parts();
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, fields, "{}", item.id);
    assert_eq!(parts[0].filename.as_deref(), Some("pic.png"), "{}", item.id);
    assert_eq!(
        parts[0].content_type.as_deref(),
        Some("application/octet-stream"),
        "{}",
        item.id
    );
    assert_eq!(parts[0].data, UPLOAD, "{}", item.id);
}

// ---- ascii2d: the corpus items ------------------------------------------------------------

#[tokio::test]
async fn ascii2d_search_replays_its_corpus_items() {
    let items = mocked_items("core.ascii2d.search");
    assert_eq!(items.len(), 3);
    for item in &items {
        let pki = TestPki::generate();
        let server = ReplayServer::https(&pki, item.script()).await;
        let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
        let file = image("pic.png");
        let timeout = item.call["args"]["timeout"].as_f64().unwrap();

        let result = client.search(&file.path, timeout, None).await;

        // The recorded request: one POST of the file field to /search/file, nothing else.
        server.assert_script_consumed();
        let seen = server.recorded();
        assert_eq!(seen.len(), 1, "{}", item.id);
        assert_eq!(seen[0].method, "POST", "{}", item.id);
        assert_eq!(seen[0].path(), "/search/file", "{}", item.id);
        assert_eq!(item.recorded_targets()[0].1, "/search/file", "{}", item.id);
        check_upload(item, &seen[0]);
        assert_eq!(
            seen[0].header("referer"),
            Some(server.url("/").as_str()),
            "{}",
            item.id
        );
        assert!(
            seen[0]
                .header("user-agent")
                .unwrap()
                .starts_with("Mozilla/5.0 (X11; Linux x86_64)"),
            "{}",
            item.id
        );

        let expected = &item.expected;
        let state = &expected["effects"]["state_after"];
        assert_eq!(
            Value::Bool(client.is_blocked()),
            state["blocked"],
            "{}",
            item.id
        );
        match (&expected["raises"], result) {
            (Value::String(class), Err(err)) => {
                let class_got = match err {
                    Ascii2dError::Blocked => "Ascii2dBlockedError",
                    Ascii2dError::Failed(_) => "Ascii2dError",
                };
                assert_eq!(class, class_got, "{}", item.id);
                assert_eq!(expected["message"], err.to_string().as_str(), "{}", item.id);
            }
            (Value::Null, Ok(matches)) => {
                let got = serde_json::to_value(matches).unwrap();
                assert!(same_json(&expected["returns"], &got), "{}", item.id);
            }
            (want, got) => panic!("{}: expected {want}, got {got:?}", item.id),
        }
    }
}

// ---- SauceNAO: the corpus items -----------------------------------------------------------

#[tokio::test]
async fn saucenao_search_replays_its_corpus_items() {
    let items = mocked_items("core.saucenao.search");
    assert_eq!(items.len(), 14);
    for item in &items {
        let pki = TestPki::generate();
        let server = ReplayServer::https(&pki, item.script()).await;
        let clock = clock();
        let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
            .with_clock(clock.clone());
        let settings = settings_of(&item.call["args"]["settings"]);
        let timeout = item.call["args"]["timeout"].as_f64().unwrap_or(30.0);
        let file = image("pic.png");

        let result = client
            .search(&file.path, &settings, timeout, None, None)
            .await;

        // The recorded requests, in order: POST to the one path, the recorded parameters (the key
        // synthetic), the recorded form field.
        server.assert_script_consumed();
        let seen = server.recorded();
        let recorded = item.call["request"].as_array().unwrap();
        assert_eq!(seen.len(), recorded.len(), "{}", item.id);
        for (request, want) in seen.iter().zip(recorded) {
            assert_eq!(request.method, "POST", "{}", item.id);
            assert_eq!(request.path(), "/search.php", "{}", item.id);
            let mut query: Vec<(String, String)> = request.query();
            query.sort();
            let mut params: Vec<(String, String)> = want["params"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let text = match v {
                        Value::String(s) if s == CORPUS_KEY => SYNTHETIC_KEY.to_owned(),
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (k.clone(), text)
                })
                .collect();
            params.sort();
            assert_eq!(query, params, "{}", item.id);
            check_upload(item, request);
        }

        let expected = &item.expected;
        let effects = &expected["effects"];
        // `sleeps`: the burst-window waits asked for. The client waits in quarter-second steps
        // (`_sleep_interruptibly`); the sum of the steps is the wait.
        let want_sleep: f64 = effects["sleeps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_f64().unwrap())
            .sum();
        assert_eq!(clock.slept(), want_sleep, "{}", item.id);
        // `state_after`: the daily-limit latch and the quota reading.
        let state = &effects["state_after"];
        assert_eq!(
            Value::Bool(client.is_daily_quota_exhausted()),
            state["daily_limit_reported"],
            "{}",
            item.id
        );
        let quota = client
            .get_last_quota()
            .map_or(Value::Null, |q| serde_json::to_value(q).unwrap());
        // Python keeps a quota object of four `None`s after an answer whose header held none, and
        // `None` before any; the port keeps the same two.
        assert!(same_json(&quota, &state["last_quota"]), "{}", item.id);

        match (&expected["raises"], result) {
            (Value::String(class), Err(err)) => {
                assert_eq!(class, "SauceNaoError", "{}", item.id);
                assert_eq!(expected["message"], err.0.as_str(), "{}", item.id);
            }
            (Value::Null, Ok(matches)) => {
                let got = serde_json::to_value(matches).unwrap();
                assert!(
                    same_json(&expected["returns"], &got),
                    "{}: expected {}, got {got}",
                    item.id,
                    expected["returns"]
                );
            }
            (want, got) => panic!("{}: expected {want}, got {got:?}", item.id),
        }
    }
}

// ---- the DAN-1909 rule: the shipped scheme, the shipped constructor ------------------------------

const COLOR_PAGE: &str = "<html><head><title>ascii2d</title></head><body>\
<div class=\"item-box\"><div class=\"image-box\"><img src=\"/thumbnail/query.jpg\"></div></div>\
<div class=\"item-box\"><div class=\"image-box\"><img src=\"/thumbnail/hit.jpg\"></div>\
<div class=\"detail-box\"><h6><a href=\"https://www.pixiv.net/artworks/12345\">pixiv</a></h6>\
<small>1200x1600 PNG</small></div></div></body></html>";

const BOVW_PAGE: &str = "<html><head><title>ascii2d</title></head><body>\
<div class=\"item-box\"><div class=\"detail-box\">\
<a href=\"https://www.pixiv.net/artworks/12345\">pixiv</a>\
<a href=\"https://twitter.com/i/web/status/99\">twitter</a></div></div></body></html>";

/// The verifier's PASS names this test.
#[tokio::test]
async fn ascii2d_and_saucenao_reach_a_local_https_server_through_the_binaries_constructor() {
    let pki = TestPki::generate();
    let file = image("shot.png");

    // ascii2d: the upload is answered with a redirect to the colour search and a cookie; the
    // colour page sets a second cookie; the feature search carries both, most specific path first.
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(302, "")
                .header("Location", "/search/color/aabbcc")
                .header("Set-Cookie", "_session_id=s1; Path=/; HttpOnly"),
            Reply::ok(COLOR_PAGE)
                .header("Content-Type", "text/html; charset=UTF-8")
                .header("Set-Cookie", "csrf=c2; Path=/search"),
            Reply::ok(BOVW_PAGE).header("Content-Type", "text/html; charset=UTF-8"),
        ],
    )
    .await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let matches = client.search(&file.path, 30.0, None).await.unwrap();
    let urls: Vec<_> = matches.iter().map(|m| m.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://www.pixiv.net/artworks/12345",
            "https://twitter.com/i/web/status/99"
        ]
    );
    // The colour hit keeps its higher score; the feature-only one is new, 80.
    assert_eq!(
        matches.iter().map(|m| m.similarity).collect::<Vec<_>>(),
        [86.0, 80.0]
    );
    assert_eq!(
        matches[0].thumb_url.as_deref(),
        Some(server.url("/thumbnail/hit.jpg").as_str())
    );
    let seen = server.recorded();
    let line = |r: &Recorded| (r.method.clone(), r.path());
    assert_eq!(
        seen.iter().map(line).collect::<Vec<_>>(),
        [
            ("POST".to_owned(), "/search/file".to_owned()),
            ("GET".to_owned(), "/search/color/aabbcc".to_owned()),
            ("GET".to_owned(), "/search/bovw/aabbcc".to_owned()),
        ]
    );
    // The redirected GET has no body and no content type; the session's headers go on every hop.
    assert!(
        seen[0]
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data")
    );
    assert_eq!(seen[1].header("content-type"), None);
    assert!(seen[1].body.is_empty());
    for request in &seen {
        assert_eq!(request.header("referer"), Some(server.url("/").as_str()));
    }
    assert_eq!(seen[0].header("cookie"), None);
    assert_eq!(seen[1].header("cookie"), Some("_session_id=s1"));
    assert_eq!(seen[2].header("cookie"), Some("csrf=c2; _session_id=s1"));
    server.assert_script_consumed();

    // SauceNAO: the key, the output type and the database in the query, in Python's order; the
    // image as the `file` field; a JSON answer with the quota.
    let body = json!({
        "header": {"status": 0, "short_remaining": 5, "short_limit": 6,
                   "long_remaining": 99, "long_limit": 100},
        "results": [{"header": {"similarity": "91.5", "index_name": "Index #5: Pixiv"},
                     "data": {"ext_urls": ["https://www.pixiv.net/artworks/1"], "title": "t"}}],
    });
    let server = ReplayServer::https(&pki, vec![Reply::ok(body.to_string())]).await;
    let client =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    let matches = client
        .search(&file.path, &settings(true), 30.0, None, None)
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].similarity, 91.5);
    let seen = &server.recorded()[0];
    assert_eq!(
        seen.query(),
        [
            ("output_type".to_owned(), "2".to_owned()),
            ("api_key".to_owned(), SYNTHETIC_KEY.to_owned()),
            ("db".to_owned(), "999".to_owned()),
        ]
    );
    assert_eq!(seen.form_parts()[0].name, "file");
    assert_eq!(client.get_last_quota().unwrap().long_remaining, Some(99));

    // The HTML route: no key, no output type.
    let server = ReplayServer::https(&pki, vec![Reply::ok("<html></html>")]).await;
    let client =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    let none = SauceNaoSettings {
        api_key: "".into(),
        ..SauceNaoSettings::default()
    };
    assert_eq!(
        client
            .search(&file.path, &none, 30.0, None, None)
            .await
            .unwrap(),
        []
    );
    assert_eq!(
        server.recorded()[0].query(),
        [("db".to_owned(), "999".to_owned())]
    );
}

// ---- one deadline and one network error per client ----------------------------------------------

/// A body that trickles in for longer than the deadline: a per-read timeout would never fire.
fn trickling() -> Reply {
    Reply::ok(COLOR_PAGE).trickled(8, Duration::from_millis(60))
}

const TRICKLE_MESSAGE: &str = "Timed out after 1s (the server may be trickling data, keeping the \
                               connection alive without actually finishing)";

#[tokio::test]
async fn ascii2d_gives_up_at_the_deadline_on_a_body_that_trickles() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![trickling()]).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");
    let started = std::time::Instant::now();

    let err = client.search(&file.path, 1.0, None).await.unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        err,
        Ascii2dError::Failed(format!("ascii2d request timed out: {TRICKLE_MESSAGE}"))
    );
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn ascii2d_reports_a_network_error_and_a_missing_file() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = Ascii2dClient::with_base(net(&pki), &closed);
    let file = image("x.png");
    let err = client.search(&file.path, 30.0, None).await.unwrap_err();
    assert!(
        matches!(&err, Ascii2dError::Failed(m) if m.starts_with("ascii2d request failed: ")),
        "{err}"
    );
    // The upload is read before anything is sent: a missing file is its own error.
    let gone = file.path.with_file_name("missing.png");
    let err = client.search(&gone, 30.0, None).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "Could not read image: [Errno 2] No such file or directory: '{}'",
            gone.display()
        )
    );
}

#[tokio::test]
async fn saucenao_gives_up_at_the_deadline_on_a_body_that_trickles_on_both_routes() {
    let pki = TestPki::generate();
    for json_api in [true, false] {
        let server = ReplayServer::https(&pki, vec![trickling()]).await;
        let client =
            SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
        let file = image("x.png");
        let err = client
            .search(&file.path, &settings(json_api), 1.0, None, None)
            .await
            .unwrap_err();
        assert_eq!(
            err.0,
            format!("SauceNAO request timed out: {TRICKLE_MESSAGE}"),
            "json_api={json_api}"
        );
    }
}

#[tokio::test]
async fn saucenao_reports_a_network_error_and_a_missing_file() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = SauceNaoClient::with_api_url(net(&pki), &closed).with_clock(clock());
    let file = image("x.png");
    for json_api in [true, false] {
        let err = client
            .search(&file.path, &settings(json_api), 30.0, None, None)
            .await
            .unwrap_err();
        assert!(err.0.starts_with("SauceNAO request failed: "), "{err}");
        // The key is in the query string and in no error text.
        assert!(!err.0.contains(SYNTHETIC_KEY), "{err}");
    }
    let gone = file.path.with_file_name("missing.png");
    let err = client
        .search(&gone, &settings(true), 30.0, None, None)
        .await
        .unwrap_err();
    assert_eq!(
        err.0,
        format!(
            "Could not read image: [Errno 2] No such file or directory: '{}'",
            gone.display()
        )
    );
}

// ---- tests/test_search_quality.py::TestAscii2dBotChallenge -----------------------------------------

const CHALLENGE: &str = "<!DOCTYPE html><html><head><title>Just a moment...</title></head>\
<body>challenge-platform</body></html>";

#[tokio::test]
async fn a_challenge_is_recognised_and_latched_and_a_new_search_clears_it() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::new(403, CHALLENGE)]).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");

    let err = client.search(&file.path, 5.0, None).await.unwrap_err();

    assert_eq!(err, Ascii2dError::Blocked);
    assert!(client.is_blocked());
    assert!(err.to_string().contains("bot check"));
    assert!(err.to_string().contains("Settings > Engine"));
    // A clone shares the latch (the one batch has one client); starting a search clears it.
    assert!(client.clone().is_blocked());
    client.reset_blocked_flag();
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn a_plain_403_is_not_called_a_bot_check() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::new(403, "nope")]).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");

    let err = client.search(&file.path, 5.0, None).await.unwrap_err();

    assert_eq!(
        err,
        Ascii2dError::Failed("ascii2d returned HTTP 403".into())
    );
    assert!(!client.is_blocked());
}

// ---- ascii2d: what the session does ------------------------------------------------------------------

#[tokio::test]
async fn a_failed_feature_search_keeps_the_colour_results() {
    let pki = TestPki::generate();
    for feature in [Reply::new(500, "down"), Reply::aborted()] {
        let server = ReplayServer::https(
            &pki,
            vec![
                Reply::new(302, "").header("Location", "/search/color/ff00"),
                Reply::ok(COLOR_PAGE),
                feature,
            ],
        )
        .await;
        let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
        let file = image("x.png");

        let matches = client.search(&file.path, 5.0, None).await.unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].search_kind, "color");
        assert_eq!(server.recorded()[2].path(), "/search/bovw/ff00");
    }
}

#[tokio::test]
async fn no_feature_search_is_made_when_the_colour_url_names_no_hash() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::ok(COLOR_PAGE)]).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");

    let matches = client.search(&file.path, 5.0, None).await.unwrap();

    assert_eq!(matches.len(), 1);
    server.assert_script_consumed();
    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test]
async fn a_page_that_is_not_ascii2d_asks_no_second_question() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::ok("<html>hello</html>")]).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");

    let err = client.search(&file.path, 5.0, None).await.unwrap_err();

    assert!(
        err.to_string()
            .starts_with("ascii2d's response didn't look like a real results page")
    );
    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test]
async fn a_307_repeats_the_upload_and_a_loop_of_redirects_ends_at_thirty() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(307, "").header("Location", "/elsewhere"),
            Reply::ok(COLOR_PAGE),
        ],
    )
    .await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");
    client.search(&file.path, 5.0, None).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen[1].method, "POST");
    assert_eq!(seen[1].path(), "/elsewhere");
    assert_eq!(seen[1].body, seen[0].body);
    assert_eq!(
        seen[1].header("content-type"),
        seen[0].header("content-type")
    );

    let hops = (0..31)
        .map(|_| Reply::new(302, "").header("Location", "/again"))
        .collect();
    let server = ReplayServer::https(&pki, hops).await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert_eq!(
        err,
        Ascii2dError::Failed("ascii2d request failed: Exceeded 30 redirects.".into())
    );
    assert_eq!(server.recorded().len(), 31);
}

#[tokio::test]
async fn a_cookie_that_is_expired_secure_or_for_another_path_is_not_sent() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(302, "")
                .header("Location", "/search/color/aa")
                .header("Set-Cookie", "kept=1")
                .header("Set-Cookie", "gone=1; Max-Age=0")
                .header("Set-Cookie", "elsewhere=1; Path=/other")
                .header(
                    "Set-Cookie",
                    "past=1; Expires=Thu, 01 Jan 1970 00:00:01 GMT",
                )
                .header("Set-Cookie", "secure=1; Secure; Path=/")
                .header("Set-Cookie", "other_host=1; Domain=example.org"),
            Reply::ok(COLOR_PAGE),
            Reply::ok(BOVW_PAGE),
        ],
    )
    .await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");
    client.search(&file.path, 5.0, None).await.unwrap();
    let seen = server.recorded();
    // `kept` has the default path of the POST (`/search`); the server is https, so `secure` goes.
    assert_eq!(seen[1].header("cookie"), Some("kept=1; secure=1"));
}

#[tokio::test]
async fn the_countdown_reports_for_both_requests_and_clears() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(302, "").header("Location", "/search/color/aa"),
            Reply::ok(COLOR_PAGE).delayed(Duration::from_millis(1300)),
            Reply::ok(BOVW_PAGE),
        ],
    )
    .await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let seen: Arc<Mutex<Vec<(String, f64, f64)>>> = Arc::default();
    let sink = seen.clone();
    let on_tick: OnTick = Arc::new(move |label, remaining, total| {
        sink.lock()
            .unwrap()
            .push((label.to_owned(), remaining, total));
    });
    let file = image("x.png");

    client
        .search(&file.path, 30.0, Some(on_tick))
        .await
        .unwrap();

    let seen = seen.lock().unwrap().clone();
    let labels: Vec<_> = seen.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(labels.first(), Some(&"Waiting on ascii2d"));
    // Each wait ends with the clearing tick.
    assert_eq!(labels.iter().filter(|l| l.is_empty()).count(), 2);
    assert_eq!(seen.last(), Some(&(String::new(), 0.0, 0.0)));
}

// ---- tests/test_core_utils.py::TestSauceNaoRetriesA429 -------------------------------------------------

fn quota_body(short_remaining: i64) -> String {
    json!({
        "header": {"status": 0, "short_remaining": short_remaining, "short_limit": 17,
                   "long_remaining": 100, "long_limit": 5000},
        "results": [],
    })
    .to_string()
}

#[tokio::test]
async fn a_429_is_retried_once_after_waiting_one_burst_window() {
    let pki = TestPki::generate();
    let server =
        ReplayServer::https(&pki, vec![Reply::new(429, ""), Reply::ok(quota_body(5))]).await;
    let clock = clock();
    let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
        .with_clock(clock.clone());
    let file = image("x.png");

    let got = client
        .search(&file.path, &settings(true), 30.0, None, None)
        .await;

    assert_eq!(got.unwrap(), []); // got through on the retry
    server.assert_script_consumed();
    assert_eq!(clock.slept(), SHORT_WINDOW_SECONDS);
    // The wait is in quarter-second steps, 120 of them.
    assert_eq!(clock.sleeps().len(), 120);
    assert!(clock.sleeps().iter().all(|s| *s == 0.25));
}

#[tokio::test]
async fn a_second_429_gives_up_rather_than_looping() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::new(429, ""), Reply::new(429, "")]).await;
    let client =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    let file = image("x.png");

    let err = client
        .search(&file.path, &settings(true), 30.0, None, None)
        .await
        .unwrap_err();

    assert_eq!(
        err,
        SauceNaoError("SauceNAO rate limit reached, slow down the search delay".into())
    );
    server.assert_script_consumed();
}

#[tokio::test]
async fn a_successful_search_never_sleeps_and_its_quota_reading_is_timestamped() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::ok(quota_body(5))]).await;
    let clock = clock();
    let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
        .with_clock(clock.clone());
    let file = image("x.png");

    client
        .search(&file.path, &settings(true), 30.0, None, None)
        .await
        .unwrap();

    assert_eq!(clock.sleeps(), Vec::<f64>::new());
    assert_eq!(client.get_last_quota().unwrap().short_remaining, Some(5));
}

#[tokio::test]
async fn an_exhausted_window_is_waited_out_before_asking() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![Reply::ok(quota_body(0)), Reply::ok(quota_body(5))],
    )
    .await;
    let clock = clock();
    let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
        .with_clock(clock.clone());
    let file = image("x.png");
    let s = settings(true);

    // The first answer leaves the window spent; ten seconds on, the next search holds off for
    // the 20 that remain of it before it asks.
    client
        .search(&file.path, &s, 30.0, None, None)
        .await
        .unwrap();
    assert_eq!(clock.sleeps(), Vec::<f64>::new());
    clock.advance(10.0);
    client
        .search(&file.path, &s, 30.0, None, None)
        .await
        .unwrap();

    assert_eq!(clock.slept(), 20.0);
}

// ---- tests/test_core_utils.py::TestBurstWaitAnswersStop -------------------------------------------------

fn stop_now() -> Option<ShouldStop> {
    Some(Arc::new(|| true))
}

#[tokio::test]
async fn a_stop_ends_the_burst_wait_at_once() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![Reply::ok(quota_body(0)), Reply::ok(quota_body(5))],
    )
    .await;
    let clock = clock();
    let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
        .with_clock(clock.clone());
    let file = image("x.png");
    let s = settings(true);
    client
        .search(&file.path, &s, 30.0, None, None)
        .await
        .unwrap();

    // The stop flag is up: the wait ends without sleeping, and the request is still made (Python
    // does not check the flag between the wait and the request).
    client
        .search(&file.path, &s, 30.0, None, stop_now())
        .await
        .unwrap();

    assert_eq!(clock.sleeps(), Vec::<f64>::new());
    server.assert_script_consumed();
}

#[tokio::test]
async fn it_waits_when_nothing_asks_it_to_stop() {
    let pki = TestPki::generate();
    let server =
        ReplayServer::https(&pki, vec![Reply::new(429, ""), Reply::ok(quota_body(5))]).await;
    let clock = clock();
    let client = SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php"))
        .with_clock(clock.clone());
    let file = image("x.png");
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let never: Option<ShouldStop> = Some(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        false
    }));

    client
        .search(&file.path, &settings(true), 30.0, None, never)
        .await
        .unwrap();

    assert_eq!(clock.slept(), 30.0);
    // Once before each of the 120 steps and once more after the wait ended.
    assert!(asked.load(Ordering::SeqCst) >= 120);
}

#[tokio::test]
async fn stopping_mid_retry_does_not_fire_a_second_request() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![Reply::new(429, "")]).await;
    let client =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    let file = image("x.png");

    let err = client
        .search(&file.path, &settings(true), 30.0, None, stop_now())
        .await
        .unwrap_err();

    assert_eq!(
        err.0,
        "Search stopped while waiting out SauceNAO's rate limit"
    );
    assert_eq!(
        server.recorded().len(),
        1,
        "the retry must not run after a stop"
    );
}

// ---- tests/test_core_utils.py::TestSauceNaoQuotaExhaustion (the latch, through the client) --------------------

#[tokio::test]
async fn the_daily_latch_follows_the_answers_and_the_reset_clears_it() {
    let pki = TestPki::generate();
    let answers = |header: Value| Reply::ok(json!({"header": header, "results": []}).to_string());
    let server = ReplayServer::https(
        &pki,
        vec![
            answers(json!({"status": 0, "short_remaining": 0, "long_remaining": 82})),
            answers(json!({"status": 0, "long_remaining": 0})),
            answers(json!({"status": -1, "message": "Invalid API key."})),
            answers(json!({"status": -1, "message": "Daily Search Limit Exceeded."})),
        ],
    )
    .await;
    let client =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    let file = image("x.png");
    let s = settings(true);
    let mut flags = Vec::new();
    for _ in 0..4 {
        let _ = client.search(&file.path, &s, 30.0, None, None).await;
        flags.push(client.is_daily_quota_exhausted());
        if flags.len() == 2 {
            client.reset_daily_limit_flag();
            // An answer whose quota still says 0 left reads as spent without the latch.
            assert!(client.is_daily_quota_exhausted());
        }
    }
    // Burst empty and a daily 82 left: not exhausted. Daily 0: exhausted. The bad-key answer has
    // a header and no quota numbers, so it replaces the reading with an empty one (as Python's
    // `_last_quota = SauceNaoQuota(...)` does) and, the latch having been reset, nothing says
    // spent. A message naming the daily limit latches.
    assert_eq!(flags, [false, true, false, true]);
}

/// The redirect's `Location` is read as UTF-8, as `requests` does, so a non-ASCII query on the
/// colour page's address is followed rather than ending the search at the 302.
#[tokio::test]
async fn a_redirect_whose_location_is_not_ascii_is_followed() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(302, "").header("Location", "/search/color/ff00?q=caf\u{e9}"),
            Reply::ok(COLOR_PAGE),
            Reply::new(500, "down"),
        ],
    )
    .await;
    let client = Ascii2dClient::with_base(net(&pki), &server.url("/"));
    let file = image("x.png");

    let matches = client.search(&file.path, 5.0, None).await.unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(server.recorded()[1].path(), "/search/color/ff00");
}
