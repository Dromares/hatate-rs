//! DAN-2057 (Phase 3.a): the request halves of IQDB, trace.moe and the Pawchive lookup, against a
//! local **https** replay server (`tests/common/replay.rs`), through the constructor the binaries
//! use (`NetClient::new`: rustls with the explicit CryptoProvider). No live network.
//!
//! - `mocked_transport` corpus items: the client makes the item's recorded requests in order and
//!   gives `expected.json` (the returned matches, or the error and its message).
//! - the DAN-1909 rule: `engine_clients_reach_a_local_https_server_through_the_binaries_constructor`.
//! - per client: one wall-clock deadline and one network error.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    MockedItem, Recorded, RecordedExt, ReplayServer, Reply, TestPki, mocked_items, same_json,
};
use hatate_core::engines::client::OnTick;
use hatate_core::engines::iqdb::IqdbClient;
use hatate_core::engines::pawchive_lookup::{PawchiveError, PawchiveLookupClient};
use hatate_core::engines::tracemoe::TraceMoeClient;
use hatate_core::net::{NetClient, NetConfig};
use serde_json::{Value, json};

const SHA: &str = "3fac89759d85abce1ebcbff0c27d838d9a52c47ffc5a6fdcf31c6986064bbefb";

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
fn image(name: &str, bytes: &[u8]) -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2057-")
        .tempdir()
        .unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    Image { _dir: dir, path }
}

const IQDB_PAGE: &str = "<html><head><title>iqdb</title></head><body><div id=\"pages\">\
<table><tr><th>Best match</th></tr><tr>\
<td class=\"image\"><a href=\"https://example.test/post/1\"><img src=\"/thumbs/a.jpg\"></a></td>\
<td>1200x1600</td><td>97% similarity</td></tr></table></div></body></html>";

// ---- the corpus items ---------------------------------------------------------------------

/// What a client call came to, in the corpus's own terms.
enum Got {
    Returned(Value),
    Raised(&'static str, String),
}

fn check(item: &MockedItem, got: Got, origin: &str, recorded_origin: &str) {
    let id = &item.id;
    match (&item.expected["returns"], &item.expected["raises"], got) {
        (returns, Value::Null, Got::Returned(value)) => {
            // Thumbnail paths are joined against the host asked; the item recorded the real one.
            let value: Value =
                serde_json::from_str(&value.to_string().replace(origin, recorded_origin)).unwrap();
            assert!(
                same_json(returns, &value),
                "{id}: expected {returns}, got {value}"
            );
        }
        (Value::Null, class, Got::Raised(got_class, message)) => {
            assert_eq!(class, got_class, "{id}");
            assert_eq!(item.expected["message"], message.as_str(), "{id}");
        }
        (_, _, Got::Returned(value)) => {
            panic!("{id}: expected {}, returned {value}", item.expected)
        }
        (_, _, Got::Raised(class, message)) => {
            panic!(
                "{id}: expected {}, raised {class}: {message}",
                item.expected
            )
        }
    }
}

/// The script an item's client call needs. A GET answered 429 is retried once by the net layer
/// (`core/net.py`), below the layer the item recorded (`layer: net`): the server serves the item's
/// 429 to the retry as well, and the test checks the retry is the same request.
fn script_for(item: &MockedItem) -> (Vec<Reply>, usize) {
    let mut script = item.script();
    let retried =
        item.call["request"][0]["method"] == "GET" && item.call["responses"][0]["status"] == 429;
    if retried {
        script.push(script[0].clone());
    }
    (script, if retried { 2 } else { 1 })
}

/// The requests the server saw are the recorded ones, in order (the net layer's retry repeats the
/// recorded request). Returns them.
fn check_requests(
    item: &MockedItem,
    server: &ReplayServer,
    expected_count: usize,
) -> Vec<Recorded> {
    server.assert_script_consumed();
    let seen = server.recorded();
    assert_eq!(seen.len(), expected_count, "{}", item.id);
    let wanted = item.recorded_targets();
    for (i, request) in seen.iter().enumerate() {
        let (method, target) = &wanted[i.min(wanted.len() - 1)];
        assert_eq!(&request.method, method, "{}", item.id);
        assert_eq!(&request.target, target, "{}", item.id);
        assert_eq!(
            request.header("user-agent"),
            Some("hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)"),
            "{}",
            item.id
        );
    }
    seen
}

/// The form fields a recorded request carried, by name (`file_fields`), and no others.
fn check_upload(item: &MockedItem, request: &Recorded, filename: &str, bytes: &[u8]) {
    let fields: Vec<&str> = item.call["request"][0]["file_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let parts = request.form_parts();
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, fields, "{}", item.id);
    assert_eq!(parts[0].filename.as_deref(), Some(filename), "{}", item.id);
    assert_eq!(
        parts[0].content_type.as_deref(),
        Some("application/octet-stream"),
        "{}",
        item.id
    );
    assert_eq!(parts[0].data, bytes, "{}", item.id);
}

async fn iqdb_items(callable: &str, three_d: bool) -> usize {
    let items = mocked_items(callable);
    for item in &items {
        let pki = TestPki::generate();
        let (script, count) = script_for(item);
        let server = ReplayServer::https(&pki, script).await;
        let origin = server.url("/");
        // Two servers would be needed to tell the instances apart; one address serves whichever
        // is asked for, and the instance asked for is named in the error text.
        let client = IqdbClient::with_hosts(net(&pki), &origin, &origin);
        let timeout = item.call["args"]["timeout"].as_f64().unwrap();
        let file = image("pic.png", b"a small picture, sent as it is");

        let result = if three_d {
            client.search_3d(&file.path, timeout, None).await
        } else {
            client.search(&file.path, timeout, None).await
        };

        let recorded = item.call["request"][0]["url"].as_str().unwrap();
        let got = match result {
            Ok(matches) => Got::Returned(serde_json::to_value(matches).unwrap()),
            Err(e) => Got::Raised("IqdbError", e.to_string()),
        };
        check(item, got, &origin, recorded);
        let seen = check_requests(item, &server, count);
        check_upload(item, &seen[0], "pic.png", b"a small picture, sent as it is");
        assert!(
            seen[0]
                .header("content-type")
                .unwrap()
                .starts_with("multipart/form-data")
        );
    }
    items.len()
}

#[tokio::test]
async fn iqdb_search_replays_its_corpus_item() {
    assert_eq!(iqdb_items("core.iqdb.search", false).await, 1);
}

#[tokio::test]
async fn iqdb_search_3d_replays_its_corpus_item() {
    assert_eq!(iqdb_items("core.iqdb.search_3d", true).await, 1);
}

#[tokio::test]
async fn pawchive_lookup_replays_its_corpus_items() {
    let items = mocked_items("core.pawchive_lookup.lookup");
    assert_eq!(items.len(), 5);
    for item in &items {
        let pki = TestPki::generate();
        let (script, count) = script_for(item);
        let server = ReplayServer::https(&pki, script).await;
        let origin = server.url("");
        let client =
            PawchiveLookupClient::with_host(net(&pki), &server.url("/api/v1/search_hash/"));
        let sha = item.call["args"]["sha256"].as_str().unwrap();
        let timeout = item.call["args"]["timeout"].as_f64().unwrap_or(15.0);

        let got = match client.lookup(sha, timeout).await {
            Ok(matches) => Got::Returned(serde_json::to_value(matches).unwrap()),
            Err(e) => Got::Raised("PawchiveLookupError", e.to_string()),
        };
        check(item, got, &origin, &origin);
        check_requests(item, &server, count);
    }
}

// ---- the DAN-1909 rule: the shipped scheme, the shipped constructor ----------------------------

/// The verifier's PASS names this test.
#[tokio::test]
async fn engine_clients_reach_a_local_https_server_through_the_binaries_constructor() {
    let pki = TestPki::generate();
    let file = image("shot.png", b"a small picture");

    // IQDB: the page parses, and the thumbnail is joined against the host asked.
    let server = ReplayServer::https(
        &pki,
        vec![Reply::ok(IQDB_PAGE).header("Content-Type", "text/html; charset=UTF-8")],
    )
    .await;
    let client = IqdbClient::with_hosts(net(&pki), &server.url("/"), &server.url("/3d/"));
    let matches = client.search(&file.path, 30.0, None).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].url, "https://example.test/post/1");
    assert_eq!(
        matches[0].thumb_url.as_deref(),
        Some(server.url("/thumbs/a.jpg").as_str())
    );
    assert_eq!(
        (matches[0].similarity, matches[0].width),
        (97.0, Some(1200))
    );
    assert_eq!(server.recorded()[0].method, "POST");

    // trace.moe: the query, the field name, and a JSON answer.
    let body = json!({"result": [{
        "anilist": {"id": 21, "title": {"romaji": "One Piece"}},
        "similarity": 0.97, "episode": 3, "image": "https://example.test/t.jpg"}]});
    let server = ReplayServer::https(
        &pki,
        vec![Reply::ok(body.to_string()).header("Content-Type", "application/json")],
    )
    .await;
    let client = TraceMoeClient::with_host(net(&pki), &server.url("/search"));
    let matches = client.search(&file.path, 30.0, None, 85.0).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].url, "https://anilist.co/anime/21");
    let seen = &server.recorded()[0];
    assert_eq!(seen.path(), "/search");
    assert_eq!(
        seen.query(),
        [
            ("cutBorders".to_owned(), "1".to_owned()),
            ("anilistInfo".to_owned(), "1".to_owned())
        ]
    );
    assert_eq!(seen.form_parts()[0].name, "image");

    // Pawchive: a GET of /<hash>, no upload.
    let body = json!({"posts": [{"service": "fanbox", "user": "1", "id": "2", "title": "t",
        "file": {"path": format!("/3f/ac/{SHA}.jpeg")}}]});
    let server = ReplayServer::https(&pki, vec![Reply::ok(body.to_string())]).await;
    let client = PawchiveLookupClient::with_host(net(&pki), &server.url("/api/v1/search_hash/"));
    let matches = client.lookup(SHA, 15.0).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].url, "https://pawchive.pw/fanbox/user/1/post/2");
    assert_eq!(
        server.recorded()[0].path(),
        format!("/api/v1/search_hash/{SHA}")
    );
}

// ---- `search_3d` posts to the 3D host (test_search_quality.py::test_search_3d_posts_to_the_3d_host) ---

#[tokio::test]
async fn search_3d_posts_to_the_3d_host_and_search_to_the_2d_host() {
    let pki = TestPki::generate();
    let empty = "<html><head><title>iqdb</title></head><body><div id='pages'></div></body></html>";
    let two_d = ReplayServer::https(&pki, vec![Reply::ok(empty)]).await;
    let three_d = ReplayServer::https(&pki, vec![Reply::ok(empty)]).await;
    let client = IqdbClient::with_hosts(net(&pki), &two_d.url("/"), &three_d.url("/"));
    let file = image("x.png", b"x");

    client.search_3d(&file.path, 5.0, None).await.unwrap();
    assert_eq!((two_d.recorded().len(), three_d.recorded().len()), (0, 1));
    client.search(&file.path, 5.0, None).await.unwrap();
    assert_eq!((two_d.recorded().len(), three_d.recorded().len()), (1, 1));
}

// ---- one deadline and one network error per client ------------------------------------------------

/// A body that trickles in for longer than the deadline: a per-read timeout would never fire.
fn trickling() -> Reply {
    Reply::ok(IQDB_PAGE).trickled(8, Duration::from_millis(60))
}

#[tokio::test]
async fn iqdb_gives_up_at_the_deadline_on_a_body_that_trickles() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![trickling()]).await;
    let client = IqdbClient::with_hosts(net(&pki), &server.url("/"), &server.url("/"))
        .with_min_budget_seconds(1.0);
    let file = image("x.png", b"x");
    let started = std::time::Instant::now();

    let err = client.search(&file.path, 1.0, None).await.unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        err.to_string(),
        "IQDB request timed out: Timed out after 1s (the server may be trickling data, keeping \
         the connection alive without actually finishing)"
    );
}

#[tokio::test]
async fn iqdb_reports_a_network_error() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = IqdbClient::with_hosts(net(&pki), &closed, &closed);
    let file = image("x.png", b"x");
    let err = client.search(&file.path, 30.0, None).await.unwrap_err();
    assert!(
        err.to_string().starts_with("IQDB request failed: "),
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
async fn tracemoe_gives_up_at_the_deadline_on_a_body_that_trickles() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![trickling()]).await;
    let client = TraceMoeClient::with_host(net(&pki), &server.url("/search"));
    let file = image("x.png", b"x");

    let err = client
        .search(&file.path, 1.0, None, 85.0)
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .starts_with("trace.moe request timed out: Timed out after 1s"),
        "{err}"
    );
}

#[tokio::test]
async fn tracemoe_reports_a_network_error() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = TraceMoeClient::with_host(net(&pki), &closed);
    let file = image("x.png", b"x");
    let err = client
        .search(&file.path, 30.0, None, 85.0)
        .await
        .unwrap_err();
    assert!(
        err.to_string().starts_with("trace.moe request failed: "),
        "{err}"
    );
}

#[tokio::test]
async fn pawchive_lookup_gives_up_at_the_deadline_and_reports_a_network_error() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(&pki, vec![trickling()]).await;
    let client = PawchiveLookupClient::with_host(net(&pki), &server.url("/api/v1/search_hash/"));
    // Python's `lookup` does not catch `HardTimeoutError`: it is not a `PawchiveLookupError`.
    let err = client.lookup(SHA, 1.0).await.unwrap_err();
    assert!(matches!(err, PawchiveError::HardTimeout(_)), "{err}");
    assert!(err.to_string().starts_with("Timed out after 1s"), "{err}");

    let closed = closed_port().await;
    let client = PawchiveLookupClient::with_host(net(&pki), &format!("{closed}api/"));
    let err = client.lookup(SHA, 15.0).await.unwrap_err();
    assert!(
        matches!(&err, PawchiveError::Lookup(e) if e.0.starts_with("could not reach pawchive.pw: ")),
        "{err}"
    );

    // Nothing is asked for a string that is not a hash.
    let err = client.lookup("not-a-hash", 15.0).await.unwrap_err();
    assert_eq!(err.to_string(), "not a SHA-256: 'not-a-hash'");
}

// ---- the countdown reaches the caller ---------------------------------------------------------------

#[tokio::test]
async fn the_ticker_reports_while_iqdb_waits_and_clears_at_the_end() {
    let pki = TestPki::generate();
    let slow = Reply::ok(IQDB_PAGE).delayed(Duration::from_millis(1300));
    let server = ReplayServer::https(&pki, vec![slow]).await;
    let client = IqdbClient::with_hosts(net(&pki), &server.url("/"), &server.url("/"));
    let seen: Arc<Mutex<Vec<(String, f64, f64)>>> = Arc::default();
    let sink = seen.clone();
    let on_tick: OnTick = Arc::new(move |label, remaining, total| {
        sink.lock()
            .unwrap()
            .push((label.to_owned(), remaining, total));
    });
    let file = image("x.png", b"x");

    client
        .search(&file.path, 30.0, Some(on_tick))
        .await
        .unwrap();

    let seen = seen.lock().unwrap().clone();
    // The floor made the budget 45 s.
    assert_eq!(
        seen.first().map(|t| (t.0.as_str(), t.2)),
        Some(("Waiting on IQDB", 45.0))
    );
    assert_eq!(seen.last(), Some(&(String::new(), 0.0, 0.0)));
}

/// An address nothing listens on.
async fn closed_port() -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("https://127.0.0.1:{port}/")
}
