//! DAN-2061 (Phase 3.e): the request halves of Yandex and Google Images, against a local
//! **https** replay server (`tests/common/replay.rs`), through the constructor the binaries use
//! (`NetClient::new`: rustls with the explicit CryptoProvider). No live network.
//!
//! - `mocked_transport` corpus items: `core.yandex.search` (5) and `core.google_images.search`
//!   (10). The client makes the item's recorded requests in order, to the recorded hosts (the
//!   test client connects those names to the local server, `NetConfig::with_host_override`;
//!   a response whose recorded URL is a redirect's landing is served by a 303), and gives
//!   `expected.json`: the matches or the error and its message, the sleeps it asked for, and the
//!   module state it left.
//! - the DAN-1909 rule: `yandex_and_google_images_reach_a_local_https_server_through_the_binaries_constructor`.
//! - per client: one wall-clock deadline and one network error.
//! - the Python tests that exercise these functions, ported (the mapping is in the PR body).
//!
//! The Vision key in every request is the synthetic `SYNTHETIC_KEY`.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    MockedItem, Recorded, RecordedExt, Redirected, ReplayServer, Reply, TestPki, mocked_items,
    same_json,
};
use hatate_core::clock::{Clock, ManualClock};
use hatate_core::engines::client::OnTick;
use hatate_core::engines::google_images::{GoogleImagesClient, GoogleImagesError};
use hatate_core::engines::saucenao::ShouldStop;
use hatate_core::engines::yandex::{CHALLENGE_REST_SECONDS, YandexClient, YandexError};
use hatate_core::net::{NetClient, NetConfig, Secret};
use serde_json::{Value, json};

const SYNTHETIC_KEY: &str = "synthetic-test-key-0000";
const CORPUS_KEY: &str = "<api-key>";

fn net(pki: &TestPki) -> NetClient {
    // The one constructor: installs the provider and builds the client. The throw-away CA is the
    // only extra trust anchor.
    NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap()
}

/// The same, with the real hostnames connected to the local server: what a request for
/// `https://yandex.com/...` sees is the server, with `Host: yandex.com`.
fn net_for(pki: &TestPki, server: &ReplayServer, hosts: &[&str]) -> NetClient {
    let mut config = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        config = config.with_host_override(host, server.addr());
    }
    NetClient::new(config).unwrap()
}

struct Image {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

/// A small file Google gets as it is: under the upload cap, so it is sent unchanged.
fn image(name: &str) -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2061-")
        .tempdir()
        .unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, UPLOAD).unwrap();
    Image { _dir: dir, path }
}

const UPLOAD: &[u8] = b"a small picture, sent as it is";

/// A real picture (Yandex decodes and re-encodes it).
fn picture(name: &str, width: u32, height: u32) -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2061-")
        .tempdir()
        .unwrap();
    let path = dir.path().join(name);
    image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x * 255 / width.max(1)) as u8,
            (y * 255 / height.max(1)) as u8,
            128,
        ])
    })
    .save(&path)
    .unwrap();
    Image { _dir: dir, path }
}

/// An address nothing listens on.
async fn closed_port() -> String {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("https://127.0.0.1:{port}")
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_700_000_000.0, 100.0))
}

fn key() -> Secret {
    Secret::new(SYNTHETIC_KEY)
}

// ---- Yandex page builders (tests/test_yandex.py) -----------------------------------------

/// `html.escape(text, quote=True)`
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// `_site(url, domain)`: Yandex tags every outbound link with its own utm_ parameters.
fn site(url: &str, domain: &str) -> Value {
    let tagged = if url.contains('?') {
        format!("{url}&utm_medium=organic&utm_source=yandexsmartcamera")
    } else {
        format!("{url}?utm_medium=organic&utm_source=yandexsmartcamera")
    };
    json!({
        "title": "t",
        "url": tagged,
        "domain": domain,
        "thumb": {"url": "//avatars.mds.yandex.net/i?id=abc&n=13", "width": 148, "height": 90},
        "originalImage": {"url": "https://img.example/1.jpg", "width": 1000, "height": 1400},
    })
}

/// `_page(sites)`: other data-state blobs first, then the one carrying cbirSites, HTML-escaped as
/// served.
fn page(sites: &[Value]) -> String {
    let other = html_escape(&json!({"form": {"action": "/images/search"}}).to_string());
    let state = html_escape(&json!({"initialState": {"cbirSites": {"sites": sites}}}).to_string());
    format!(
        "<html><div data-state=\"{other}\"></div>\
         <div class=\"CbirSites\" data-state=\"{state}\"></div></html>"
    )
}

fn sites() -> Vec<Value> {
    vec![
        site(
            "https://ru.pinterest.com/pin/267753140320940932/",
            "ru.pinterest.com",
        ),
        site(
            "https://rule34.us/index.php?r=posts%2Findex&q=dirtynicky",
            "rule34.us",
        ),
        site(
            "https://rule34.us/index.php?r=posts%2Fview&id=757670",
            "rule34.us",
        ),
        site(
            "https://safebooru.org/index.php?page=post&s=list&tags=baku-p",
            "safebooru.org",
        ),
        site(
            "https://kagamihara.donmai.us/posts/6307169?q=1girl",
            "kagamihara.donmai.us",
        ),
        site("https://telegra.ph/Some-post-01-01", "telegra.ph"),
        site(
            "https://xbooru.com/index.php?page=post&s=view&id=855502",
            "xbooru.com",
        ),
    ]
}

const UPLOAD_OK: &str = r#"{"blocks": [{"params": {"cbirId": "1617445/abc"}}]}"#;

fn upload_ok() -> Reply {
    Reply::ok(UPLOAD_OK).header("Content-Type", "application/json")
}

fn html_page(body: impl Into<Vec<u8>>) -> Reply {
    Reply::ok(body).header("Content-Type", "text/html; charset=utf-8")
}

/// A Yandex client against a local server answering `script`.
async fn yandex(
    pki: &TestPki,
    script: Vec<Reply>,
    clock: &Arc<ManualClock>,
) -> (YandexClient, ReplayServer) {
    let server = ReplayServer::https(pki, script).await;
    let client = YandexClient::with_search_url(net(pki), &server.url("/images/search"))
        .with_clock(clock.clone());
    (client, server)
}

// ---- the corpus items --------------------------------------------------------------------

/// Query pairs of a recorded request, as the object the corpus keeps them in.
fn query_object(request: &Recorded) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in request.query() {
        map.insert(name, Value::String(value));
    }
    Value::Object(map)
}

/// The corpus `params` object with the placeholder key made synthetic.
fn params_of(recorded: &Value) -> Value {
    let mut params = recorded.clone();
    if let Some(key) = params.get_mut("key") {
        assert_eq!(key, CORPUS_KEY);
        *key = Value::String(SYNTHETIC_KEY.to_owned());
    }
    params
}

/// The requests the server saw are the item's recorded ones (and the redirect hops), in order:
/// method, host and path; the query of each recorded request; the form's field names.
fn check_requests(item: &MockedItem, server: &ReplayServer, redirected: &Redirected) {
    server.assert_script_consumed();
    let seen = server.recorded();
    let line = |r: &Recorded| {
        (
            r.method.clone(),
            r.header("host").unwrap_or("").to_owned(),
            r.path(),
        )
    };
    assert_eq!(
        seen.iter().map(line).collect::<Vec<_>>(),
        redirected.expected,
        "{}",
        item.id
    );
    let recorded = item.call["request"].as_array().unwrap();
    assert_eq!(recorded.len(), redirected.recorded_at.len(), "{}", item.id);
    for (request, &at) in recorded.iter().zip(&redirected.recorded_at) {
        let sent = &seen[at];
        assert_eq!(
            query_object(sent),
            params_of(&request["params"]),
            "{}",
            item.id
        );
        let fields: Vec<&str> = request["file_fields"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap()).collect())
            .unwrap_or_default();
        let parts = sent.form_parts();
        assert_eq!(
            parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            fields,
            "{}",
            item.id
        );
        if request["json_body"] == true {
            assert_eq!(
                sent.header("content-type"),
                Some("application/json"),
                "{}",
                item.id
            );
            let body: Value = serde_json::from_slice(&sent.body).unwrap();
            assert_eq!(
                body["requests"][0]["features"],
                json!([{"type": "WEB_DETECTION", "maxResults": 20}]),
                "{}",
                item.id
            );
        }
    }
}

/// The expected `raises` class and message against the error.
fn check_yandex(
    item: &MockedItem,
    client: &YandexClient,
    clock: &ManualClock,
    result: Result<Vec<hatate_core::engines::yandex::YandexMatch>, YandexError>,
) {
    let expected = &item.expected;
    let state = &expected["effects"]["state_after"];
    let seen = client.state();
    assert_eq!(Value::Bool(seen.blocked), state["blocked"], "{}", item.id);
    assert_eq!(
        json!(seen.consecutive_failures),
        state["consecutive_failures"],
        "{}",
        item.id
    );
    assert_eq!(
        Value::Bool(client.is_resting()),
        state["resting"],
        "{}",
        item.id
    );
    assert_eq!(
        json!(clock.sleeps()),
        expected["effects"]["sleeps"],
        "{}",
        item.id
    );
    match (&expected["raises"], result) {
        (Value::String(class), Err(err)) => {
            let class_got = match err {
                YandexError::Blocked(_) => "YandexBlockedError",
                YandexError::Failed(_) => "YandexError",
                YandexError::Unexpected(_) => "unexpected",
            };
            assert_eq!(class, class_got, "{}", item.id);
            // The message names when the rest ends, which the corpus keeps as a placeholder.
            let ends = clock.local_hhmm(clock.wall() + CHALLENGE_REST_SECONDS);
            let want = expected["message"]
                .as_str()
                .unwrap()
                .replace("<HH:MM>", &ends);
            assert_eq!(want, err.to_string(), "{}", item.id);
        }
        (Value::Null, Ok(matches)) => {
            let got = serde_json::to_value(matches).unwrap();
            assert!(same_json(&expected["returns"], &got), "{}", item.id);
        }
        (want, got) => panic!("{}: expected {want}, got {got:?}", item.id),
    }
}

#[tokio::test]
async fn yandex_search_replays_its_corpus_items() {
    let items = mocked_items("core.yandex.search");
    assert_eq!(items.len(), 5);
    for item in &items {
        let pki = TestPki::generate_for(&["yandex.com"]);
        let redirected = item.script_following_redirects();
        let server = ReplayServer::https(&pki, redirected.replies.clone()).await;
        let clock = clock();
        let client =
            YandexClient::new(net_for(&pki, &server, &["yandex.com"])).with_clock(clock.clone());
        let file = picture("pic.png", 64, 48);
        let timeout = item.call["args"]["timeout"].as_f64().unwrap();

        let result = client.search(&file.path, timeout, None).await;

        check_requests(item, &server, &redirected);
        let seen = server.recorded();
        // The upload: the picture re-encoded as a JPEG, the `upfile` part.
        let parts = seen[0].form_parts();
        assert_eq!(
            parts[0].filename.as_deref(),
            Some("image.jpg"),
            "{}",
            item.id
        );
        assert_eq!(
            parts[0].content_type.as_deref(),
            Some("image/jpeg"),
            "{}",
            item.id
        );
        assert_eq!(&parts[0].data[..2], b"\xff\xd8", "{}", item.id);
        for request in &seen {
            assert_eq!(
                request.header("referer"),
                Some("https://yandex.com/images/")
            );
            assert!(
                request
                    .header("user-agent")
                    .unwrap()
                    .starts_with("Mozilla/5.0 (X11; Linux x86_64)"),
                "{}",
                item.id
            );
        }
        check_yandex(item, &client, &clock, result);
    }
}

#[tokio::test]
async fn google_images_search_replays_its_corpus_items() {
    let items = mocked_items("core.google_images.search");
    assert_eq!(items.len(), 10);
    let hosts = [
        "www.google.com",
        "consent.google.com",
        "lens.google.com",
        "vision.googleapis.com",
    ];
    for item in &items {
        let pki = TestPki::generate_for(&hosts);
        let redirected = item.script_following_redirects();
        let server = ReplayServer::https(&pki, redirected.replies.clone()).await;
        let client = GoogleImagesClient::new(net_for(&pki, &server, &hosts));
        let file = image("pic.png");
        let timeout = item.call["args"]["timeout"].as_f64().unwrap();
        let api_key = match item.call["args"].get("api_key") {
            Some(Value::String(key)) if key == CORPUS_KEY => key_of(SYNTHETIC_KEY),
            Some(Value::String(key)) => key_of(key),
            _ => key_of(""),
        };

        let result = client.search(&file.path, timeout, None, &api_key).await;

        check_requests(item, &server, &redirected);
        let first = &server.recorded()[0];
        if api_key.is_empty() {
            // The public upload: the file as `encoded_image`, the consent cookie and the headers.
            let parts = first.form_parts();
            assert_eq!(parts[0].filename.as_deref(), Some("pic.png"), "{}", item.id);
            assert_eq!(
                parts[0].content_type.as_deref(),
                Some("application/octet-stream"),
                "{}",
                item.id
            );
            assert_eq!(parts[0].data, UPLOAD, "{}", item.id);
            assert_eq!(
                first.header("cookie"),
                Some("SOCS=CAESHAgBEhIaAB"),
                "{}",
                item.id
            );
            assert_eq!(first.header("referer"), Some("https://www.google.com/"));
            assert_eq!(first.header("accept-language"), Some("en-US,en;q=0.9"));
        } else {
            // Vision: the image in the JSON body, only the user agent, no cookie.
            let body: Value = serde_json::from_slice(&first.body).unwrap();
            assert_eq!(
                body["requests"][0]["image"]["content"], "YSBzbWFsbCBwaWN0dXJlLCBzZW50IGFzIGl0IGlz",
                "{}",
                item.id
            );
            assert_eq!(first.header("cookie"), None, "{}", item.id);
            assert_eq!(first.header("referer"), None, "{}", item.id);
        }
        assert!(
            first
                .header("user-agent")
                .unwrap()
                .starts_with("Mozilla/5.0 (X11; Linux x86_64)"),
            "{}",
            item.id
        );

        let expected = &item.expected;
        assert_eq!(
            Value::Bool(client.is_blocked()),
            expected["effects"]["state_after"]["blocked"],
            "{}",
            item.id
        );
        assert_eq!(expected["effects"]["sleeps"], json!([]), "{}", item.id);
        match (&expected["raises"], result) {
            (Value::String(class), Err(err)) => {
                let class_got = match err {
                    GoogleImagesError::Failed(_) => "GoogleImagesError",
                    GoogleImagesError::Blocked(_) => "GoogleImagesBlockedError",
                    GoogleImagesError::JavaScript => "GoogleImagesJavaScriptError",
                    GoogleImagesError::Unexpected(_) => "unexpected",
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

fn key_of(text: &str) -> Secret {
    Secret::new(text)
}

// ---- the DAN-1909 test: the binary's own constructor over https ---------------------------

/// The verifier's PASS names this test.
#[tokio::test]
async fn yandex_and_google_images_reach_a_local_https_server_through_the_binaries_constructor() {
    let pki = TestPki::generate();
    let clock = clock();

    // Yandex: the upload answers with the id and a cookie; the "sites" tab carries the cookie
    // and the id; the matches are the post pages, scored by position among the kept.
    let file = picture("shot.png", 3000, 2000);
    let (client, server) = yandex(
        &pki,
        vec![
            upload_ok().header("Set-Cookie", "yandexuid=u1; Path=/"),
            html_page(page(&sites())),
        ],
        &clock,
    )
    .await;
    let matches = client.search(&file.path, 30.0, None).await.unwrap();
    assert_eq!(
        matches.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(),
        [
            "https://rule34.us/index.php?r=posts%2Fview&id=757670",
            "https://danbooru.donmai.us/posts/6307169?q=1girl",
            "https://xbooru.com/index.php?page=post&s=view&id=855502",
        ]
    );
    assert_eq!(
        matches.iter().map(|m| m.similarity).collect::<Vec<_>>(),
        [80.0, 78.0, 76.0]
    );
    let seen = server.recorded();
    assert_eq!(
        seen.iter()
            .map(|r| (r.method.as_str(), r.path()))
            .collect::<Vec<_>>(),
        [
            ("POST", "/images/search".to_owned()),
            ("GET", "/images/search".to_owned())
        ]
    );
    assert_eq!(
        seen[0].query(),
        [
            ("rpt".to_owned(), "imageview".to_owned()),
            ("format".to_owned(), "json".to_owned()),
            (
                "request".to_owned(),
                r#"{"blocks":[{"block":"b-page_type_search-by-image__link"}]}"#.to_owned()
            ),
        ]
    );
    assert_eq!(
        seen[1].query(),
        [
            ("cbir_id".to_owned(), "1617445/abc".to_owned()),
            ("rpt".to_owned(), "imageview".to_owned()),
            ("cbir_page".to_owned(), "sites".to_owned()),
            (
                "url".to_owned(),
                "https://avatars.mds.yandex.net/get-images-cbir/1617445/abc/orig".to_owned()
            ),
        ]
    );
    assert_eq!(seen[0].header("cookie"), None);
    assert_eq!(seen[1].header("cookie"), Some("yandexuid=u1"));
    assert!(seen[1].body.is_empty());
    assert_eq!(seen[1].header("content-type"), None);
    // A 3000x2000 picture goes up as a JPEG that fits in 1600x1600 (1600x1067).
    let sent = &seen[0].form_parts()[0];
    let decoded = image::load_from_memory(&sent.data).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1600, 1067));
    server.assert_script_consumed();

    // Google, public route: the file as `encoded_image`, `hl=en`, and a local address gets no
    // consent cookie (it is for `.google.com`); the redirect to the results page is followed.
    let file = image("shot.png");
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(303, "").header("Location", "/search?tbs=sbi:x"),
            html_page(
                "<html><body><a href=\"https://www.deviantart.com/someone/art/A-Picture-123\">\
                 <h3>A Picture</h3></a></body></html>",
            ),
        ],
    )
    .await;
    let google = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let matches = google
        .search(&file.path, 30.0, None, &key_of(""))
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(
        matches[0].url,
        "https://www.deviantart.com/someone/art/A-Picture-123"
    );
    assert_eq!(matches[0].similarity, 78.0);
    let seen = server.recorded();
    assert_eq!(seen[0].query(), [("hl".to_owned(), "en".to_owned())]);
    assert_eq!(seen[0].form_parts()[0].name, "encoded_image");
    assert_eq!(seen[0].header("cookie"), None);
    assert_eq!(seen[1].method, "GET");
    assert_eq!(seen[1].path(), "/search");
    server.assert_script_consumed();

    // Google, Vision route: the key in the query, the JSON body, nothing else.
    let payload = json!({"responses": [{"webDetection": {"pagesWithMatchingImages": [
        {"url": "https://www.deviantart.com/someone/art/A-Picture-123",
         "pageTitle": "A Picture", "fullMatchingImages": [{"url": "https://images.test/full.jpg"}]}
    ]}}]});
    let server = ReplayServer::https(
        &pki,
        vec![Reply::ok(payload.to_string()).header("Content-Type", "application/json")],
    )
    .await;
    let google = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let matches = google.search(&file.path, 30.0, None, &key()).await.unwrap();
    assert_eq!(matches.len(), 1);
    let sent = &server.recorded()[0];
    assert_eq!(sent.path(), "/v1/images:annotate");
    assert_eq!(sent.query(), [("key".to_owned(), SYNTHETIC_KEY.to_owned())]);
    assert_eq!(
        String::from_utf8(sent.body.clone()).unwrap(),
        "{\"requests\": [{\"image\": {\"content\": \"YSBzbWFsbCBwaWN0dXJlLCBzZW50IGFzIGl0IGlz\"}, \
         \"features\": [{\"type\": \"WEB_DETECTION\", \"maxResults\": 20}]}]}"
    );
    server.assert_script_consumed();
}

// ---- one deadline and one network error per client ---------------------------------------

/// A body that trickles in for longer than the deadline: a per-read timeout would never fire.
fn trickling(body: &str) -> Reply {
    // Padded with white space (which every format here allows after its end) so that the body
    // takes far longer than the deadline to arrive.
    Reply::ok(format!("{body}{}", " ".repeat(400))).trickled(8, Duration::from_millis(60))
}

const TRICKLE_MESSAGE: &str = "Timed out after 1s (the server may be trickling data, keeping the \
                               connection alive without actually finishing)";

#[tokio::test]
async fn yandex_gives_up_at_the_deadline_on_a_body_that_trickles() {
    let pki = TestPki::generate();
    let clock = clock();
    let (client, _server) = yandex(&pki, vec![trickling(UPLOAD_OK)], &clock).await;
    let file = picture("x.png", 16, 16);
    let started = std::time::Instant::now();

    let err = client.search(&file.path, 1.0, None).await.unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        err,
        YandexError::Failed(format!("Yandex request timed out: {TRICKLE_MESSAGE}"))
    );
    // A request that did not finish is not a page that failed to parse: nothing is counted.
    assert_eq!(client.state().consecutive_failures, 0);
}

#[tokio::test]
async fn yandex_gives_up_at_the_deadline_on_the_second_request_too() {
    let pki = TestPki::generate();
    let clock = clock();
    let (client, _server) =
        yandex(&pki, vec![upload_ok(), trickling(&page(&sites()))], &clock).await;
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 1.0, None).await.unwrap_err();
    assert_eq!(
        err,
        YandexError::Failed(format!("Yandex request timed out: {TRICKLE_MESSAGE}"))
    );
}

#[tokio::test]
async fn yandex_reports_a_network_error_a_missing_file_and_a_file_it_cannot_send() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = YandexClient::with_search_url(net(&pki), &format!("{closed}/images/search"));
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 30.0, None).await.unwrap_err();
    assert!(
        matches!(&err, YandexError::Failed(m) if m.starts_with("Yandex request failed: ")),
        "{err:?}"
    );
    assert_eq!(client.state().consecutive_failures, 0);

    let gone = file.path.with_file_name("gone.png");
    let err = client.search(&gone, 30.0, None).await.unwrap_err();
    assert_eq!(
        err,
        YandexError::Failed(format!(
            "Could not read image: [Errno 2] No such file or directory: '{}'",
            gone.display()
        ))
    );

    // Bytes no decoder reads (a video, JPEG XL): refused with a reason, no request, not counted.
    let server = ReplayServer::https(&pki, vec![]).await;
    let client = YandexClient::with_search_url(net(&pki), &server.url("/images/search"));
    let text = image("clip.mp4");
    let err = client.search(&text.path, 30.0, None).await.unwrap_err();
    assert_eq!(
        err,
        YandexError::Failed(
            "not in a format that can be sent to Yandex (video and JPEG XL are the usual \
             reasons); the other engines can still search it"
                .to_owned()
        )
    );
    assert!(server.recorded().is_empty());
    assert_eq!(client.state().consecutive_failures, 0);
}

#[tokio::test]
async fn google_images_gives_up_at_the_deadline_on_a_body_that_trickles_on_both_routes() {
    let pki = TestPki::generate();
    let file = image("x.png");
    let server = ReplayServer::https(&pki, vec![trickling("<html></html>"), trickling("{}")]).await;
    let client = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let started = std::time::Instant::now();

    let public = client
        .search(&file.path, 1.0, None, &key_of(""))
        .await
        .unwrap_err();
    let vision = client
        .search(&file.path, 1.0, None, &key())
        .await
        .unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(8));
    assert_eq!(
        public,
        GoogleImagesError::Failed(format!("Google request timed out: {TRICKLE_MESSAGE}"))
    );
    assert_eq!(
        vision,
        GoogleImagesError::Failed(format!(
            "Google Vision request timed out: {TRICKLE_MESSAGE}"
        ))
    );
    // A timeout is about this image: nothing is latched.
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn google_images_reports_a_network_error_and_a_missing_file_and_never_prints_the_key() {
    let pki = TestPki::generate();
    let closed = closed_port().await;
    let client = GoogleImagesClient::with_urls(
        net(&pki),
        &format!("{closed}/searchbyimage/upload"),
        &format!("{closed}/v1/images:annotate"),
    );
    let file = image("x.png");
    let public = client
        .search(&file.path, 30.0, None, &key_of(""))
        .await
        .unwrap_err();
    assert!(
        matches!(&public, GoogleImagesError::Failed(m) if m.starts_with("Google request failed: ")),
        "{public:?}"
    );
    let vision = client
        .search(&file.path, 30.0, None, &key())
        .await
        .unwrap_err();
    let text = vision.to_string();
    assert!(text.starts_with("Google Vision request failed: "), "{text}");
    assert!(!text.contains(SYNTHETIC_KEY), "{text}");
    assert!(!format!("{vision:?}").contains(SYNTHETIC_KEY));
    assert!(!client.is_blocked());

    let gone = file.path.with_file_name("gone.png");
    let err = client.search(&gone, 30.0, None, &key()).await.unwrap_err();
    assert_eq!(
        err,
        GoogleImagesError::Failed(format!(
            "Could not read image: [Errno 2] No such file or directory: '{}'",
            gone.display()
        ))
    );
}

// ---- Yandex: tests/test_yandex.py::TestStandingDown, through the client ---------------------

fn captcha() -> Reply {
    Reply::new(303, "").header("Location", "/showcaptcha?cc=1&retpath=x")
}

#[tokio::test]
async fn a_good_answer() {
    let pki = TestPki::generate();
    let (client, server) =
        yandex(&pki, vec![upload_ok(), html_page(page(&sites()))], &clock()).await;
    let file = picture("x.png", 16, 16);
    assert_eq!(client.search(&file.path, 5.0, None).await.unwrap().len(), 3);
    server.assert_script_consumed();
}

#[tokio::test]
async fn a_captcha_rests_yandex_through_a_new_search() {
    let pki = TestPki::generate();
    let clock = clock();
    // The captcha answers the upload: a redirect to /showcaptcha, which is the final URL.
    let (client, _server) = yandex(
        &pki,
        vec![captcha(), html_page("<html>captcha</html>")],
        &clock,
    )
    .await;
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    let YandexError::Blocked(message) = &err else {
        panic!("{err:?}");
    };
    client.reset_blocked_flag(); // what starting a search does
    assert!(client.is_blocked());
    assert!(client.is_resting());
    // The message names when the rest ends, and `resting_until` says the same.
    assert!(message.contains(&client.resting_until()), "{message}");
    assert_eq!(
        client.resting_until(),
        clock.local_hhmm(clock.wall() + CHALLENGE_REST_SECONDS)
    );
}

#[tokio::test]
async fn a_captcha_on_the_results_page_counts_too() {
    let pki = TestPki::generate();
    let (client, _server) = yandex(
        &pki,
        vec![upload_ok(), captcha(), html_page("<html>captcha</html>")],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert!(matches!(err, YandexError::Blocked(_)), "{err:?}");
    assert!(client.is_resting());
}

#[tokio::test]
async fn the_rest_ends() {
    let pki = TestPki::generate();
    let clock = clock();
    let (client, _server) = yandex(&pki, vec![captcha(), html_page("captcha")], &clock).await;
    let file = picture("x.png", 16, 16);
    client.search(&file.path, 5.0, None).await.unwrap_err();
    assert!(client.is_blocked());
    clock.advance(CHALLENGE_REST_SECONDS - 1.0);
    assert!(client.is_blocked());
    assert_eq!(client.resting_until().len(), 5);
    clock.advance(2.0);
    assert!(!client.is_blocked());
    assert_eq!(client.resting_until(), "");
}

#[tokio::test]
async fn a_changed_page_stands_down_after_a_run_of_them() {
    let pki = TestPki::generate();
    let broken = || html_page("<html>no results block</html>");
    let (client, _server) = yandex(
        &pki,
        vec![
            upload_ok(),
            broken(),
            upload_ok(),
            broken(),
            upload_ok(),
            broken(),
        ],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    for _ in 0..2 {
        let err = client.search(&file.path, 5.0, None).await.unwrap_err();
        assert!(matches!(err, YandexError::Failed(_)), "{err:?}");
    }
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert!(matches!(err, YandexError::Blocked(_)), "{err:?}");
    assert!(client.is_blocked());
    assert!(!client.is_resting()); // not a captcha: a new search retries
    client.reset_blocked_flag();
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn a_success_clears_the_run_of_failures() {
    let pki = TestPki::generate();
    let broken = || html_page("<html></html>");
    let (client, _server) = yandex(
        &pki,
        vec![
            upload_ok(),
            broken(),
            upload_ok(),
            broken(),
            upload_ok(),
            html_page(page(&[])),
            upload_ok(),
            broken(),
        ],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    for _ in 0..2 {
        client.search(&file.path, 5.0, None).await.unwrap_err();
    }
    assert_eq!(client.search(&file.path, 5.0, None).await.unwrap(), []);
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert!(matches!(err, YandexError::Failed(_)), "{err:?}");
    assert_eq!(client.state().consecutive_failures, 1);
}

#[tokio::test]
async fn an_upload_answer_with_no_id_counts_a_failure_and_asks_no_second_question() {
    let pki = TestPki::generate();
    let (client, server) = yandex(
        &pki,
        vec![Reply::ok("not json").header("Content-Type", "text/html")],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert_eq!(
        err,
        YandexError::Failed(
            "Yandex's upload answer (HTTP 200) carried no search id - the endpoint may have \
             changed."
                .to_owned()
        )
    );
    assert_eq!(server.recorded().len(), 1);
    assert_eq!(client.state().consecutive_failures, 1);
}

#[tokio::test]
async fn a_json_answer_with_a_utf_16_body_and_no_charset_is_read_as_requests_reads_it() {
    // `requests` sniffs the UTF of a JSON body when the server declares no encoding.
    let pki = TestPki::generate();
    let utf16: Vec<u8> = UPLOAD_OK
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let utf16 = [vec![0xff, 0xfe], utf16].concat();
    let (client, server) = yandex(
        &pki,
        vec![Reply::ok(utf16), html_page(page(&sites()))],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    assert_eq!(client.search(&file.path, 5.0, None).await.unwrap().len(), 3);
    assert_eq!(server.recorded()[1].query()[0].1, "1617445/abc");
}

// ---- Yandex: the pacing and the clock ------------------------------------------------------

/// What the countdown reported: `(label, seconds left, total)`.
type Ticks = Arc<Mutex<Vec<(String, f64, f64)>>>;

fn ticks() -> (OnTick, Ticks) {
    let seen: Ticks = Arc::default();
    let sink = seen.clone();
    let on_tick: OnTick = Arc::new(move |label, left, total| {
        sink.lock().unwrap().push((label.to_owned(), left, total));
    });
    (on_tick, seen)
}

#[tokio::test]
async fn searches_start_ten_seconds_apart_and_the_wait_is_counted_down() {
    let clock = clock();
    let pki = TestPki::generate();
    let (client, _server) = yandex(&pki, vec![], &clock).await;

    // The first search starts at once.
    client.wait_for_turn(None, None).await;
    assert_eq!(clock.sleeps(), Vec::<f64>::new());

    // The second asks for the rest of ten seconds, a second at a time, saying how long is left.
    let (on_tick, seen) = ticks();
    client.wait_for_turn(Some(on_tick), None).await;
    assert_eq!(clock.sleeps(), vec![1.0; 10]);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 10);
    assert_eq!(seen[0], ("Pacing Yandex".to_owned(), 10.0, 10.0));
    assert_eq!(seen[9], ("Pacing Yandex".to_owned(), 1.0, 10.0));

    // The turn was claimed before waiting: the third queues behind the second.
    clock.advance(0.0);
    client.wait_for_turn(None, None).await;
    assert_eq!(clock.sleeps().len(), 20);
}

#[tokio::test]
async fn a_search_that_arrives_after_the_gap_is_not_held() {
    let clock = clock();
    let pki = TestPki::generate();
    let (client, _server) = yandex(&pki, vec![], &clock).await;
    client.wait_for_turn(None, None).await;
    clock.advance(10.0);
    client.wait_for_turn(None, None).await;
    assert_eq!(clock.sleeps(), Vec::<f64>::new());
}

#[tokio::test]
async fn stop_ends_the_wait_at_once_and_end_rest_forgets_the_turn() {
    let clock = clock();
    let pki = TestPki::generate();
    let (client, _server) = yandex(&pki, vec![], &clock).await;
    client.wait_for_turn(None, None).await;
    let stop: ShouldStop = Arc::new(|| true);
    client.wait_for_turn(None, Some(stop)).await;
    assert_eq!(clock.sleeps(), Vec::<f64>::new());
    // The stopped search still claimed its turn: the next one waits behind it.
    client.end_rest();
    client.wait_for_turn(None, None).await;
    assert_eq!(clock.sleeps(), Vec::<f64>::new());
}

// ---- Yandex: what is sent ------------------------------------------------------------------

#[tokio::test]
async fn the_captcha_message_names_the_local_time_the_rest_ends() {
    let pki = TestPki::generate();
    let clock = clock(); // 2023-11-14 22:13:20 UTC
    let (client, _server) = yandex(&pki, vec![captcha(), html_page("captcha")], &clock).await;
    let file = picture("x.png", 16, 16);
    let err = client.search(&file.path, 5.0, None).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "Yandex asked for a captcha, so it is resting until 22:43 rather than being asked again \
         straight away. The other engines carry on."
    );
}

#[tokio::test]
async fn the_countdown_reports_for_both_requests_and_clears() {
    let pki = TestPki::generate();
    let (client, _server) = yandex(
        &pki,
        vec![
            upload_ok().delayed(Duration::from_millis(1300)),
            html_page(page(&sites())),
        ],
        &clock(),
    )
    .await;
    let file = picture("x.png", 16, 16);
    let (on_tick, seen) = ticks();
    client
        .search(&file.path, 30.0, Some(on_tick))
        .await
        .unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.first().unwrap().0, "Waiting on Yandex");
    assert_eq!(seen.last().unwrap(), &(String::new(), 0.0, 0.0));
}

// ---- Google Images: the Python tests ------------------------------------------------------

fn vision_payload() -> Value {
    json!({"responses": [{"webDetection": {"pagesWithMatchingImages": [
        {"url": "https://www.deviantart.com/someone/art/A-Picture-123",
         "pageTitle": "A Picture", "fullMatchingImages": [{"url": "https://images.test/full.jpg"}]}
    ]}}]})
}

fn json_reply(status: u16, body: Value) -> Reply {
    Reply::new(status, body.to_string()).header("Content-Type", "application/json")
}

async fn google(
    pki: &TestPki,
    script: Vec<Reply>,
) -> (GoogleImagesClient, ReplayServer, ReplayServer) {
    let public = ReplayServer::https(pki, vec![]).await;
    let vision = ReplayServer::https(pki, script).await;
    let client = GoogleImagesClient::with_urls(
        net(pki),
        &public.url("/searchbyimage/upload"),
        &vision.url("/v1/images:annotate"),
    );
    (client, public, vision)
}

#[tokio::test]
async fn a_key_is_used_instead_of_the_public_page() {
    let pki = TestPki::generate();
    let (client, public, vision) = google(&pki, vec![json_reply(200, vision_payload())]).await;
    let file = image("x.png");
    // `api_key=" k "` is stripped.
    let matches = client
        .search(&file.path, 5.0, None, &key_of("  synthetic  "))
        .await
        .unwrap();
    assert_eq!(
        matches[0].url,
        "https://www.deviantart.com/someone/art/A-Picture-123"
    );
    assert!(public.recorded().is_empty());
    assert_eq!(
        vision.recorded()[0].query(),
        [("key".to_owned(), "synthetic".to_owned())]
    );
}

#[tokio::test]
async fn a_rejected_key_stands_down_for_the_batch() {
    let pki = TestPki::generate();
    let body = json!({"error": {"code": 403, "status": "PERMISSION_DENIED",
                                "message": "Cloud Vision API has not been used in project 1"}});
    let (client, _public, _vision) = google(&pki, vec![json_reply(403, body)]).await;
    let file = image("x.png");
    let err = client
        .search(&file.path, 5.0, None, &key())
        .await
        .unwrap_err();
    assert!(matches!(err, GoogleImagesError::Blocked(_)), "{err:?}");
    assert!(client.is_blocked());
    assert!(err.to_string().contains("Settings > Engine"));
    client.reset_blocked_flag();
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn an_exhausted_quota_stands_down_too() {
    let pki = TestPki::generate();
    let body = json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED",
                                "message": "Quota exceeded"}});
    let (client, _public, _vision) = google(&pki, vec![json_reply(429, body)]).await;
    let file = image("x.png");
    let err = client
        .search(&file.path, 5.0, None, &key())
        .await
        .unwrap_err();
    assert!(matches!(err, GoogleImagesError::Blocked(_)), "{err:?}");
    assert!(client.is_blocked());
}

#[tokio::test]
async fn a_per_image_error_is_not_latched() {
    let pki = TestPki::generate();
    let body = json!({"responses": [{"error": {"code": 3, "message": "Bad image data"}}]});
    let (client, _public, _vision) = google(&pki, vec![json_reply(200, body)]).await;
    let file = image("x.png");
    let err = client
        .search(&file.path, 5.0, None, &key())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        GoogleImagesError::Failed("Google Vision: Bad image data".to_owned())
    );
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn a_vision_answer_that_is_not_json_is_read_as_an_empty_object() {
    let pki = TestPki::generate();
    let (client, _public, _vision) = google(
        &pki,
        vec![
            Reply::ok("<html>oops</html>"),
            Reply::new(500, "<html>oops</html>"),
        ],
    )
    .await;
    let file = image("x.png");
    // A 200 with no JSON: `{}`, no matches.
    assert_eq!(
        client.search(&file.path, 5.0, None, &key()).await.unwrap(),
        []
    );
    // A 500 with no JSON: the status is the message, and it is about this image.
    let err = client
        .search(&file.path, 5.0, None, &key())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        GoogleImagesError::Failed("Google Vision: HTTP 500".to_owned())
    );
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn a_blocked_public_search_latches_and_a_new_search_clears_it() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::ok("<html>Our systems have detected unusual traffic</html>"),
            Reply::new(413, ""),
        ],
    )
    .await;
    let client = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let file = image("x.png");
    let err = client
        .search(&file.path, 5.0, None, &key_of(""))
        .await
        .unwrap_err();
    assert!(matches!(err, GoogleImagesError::Blocked(_)), "{err:?}");
    assert!(client.is_blocked());
    client.reset_blocked_flag();
    // A 413 is about this image and does not latch.
    let err = client
        .search(&file.path, 5.0, None, &key_of(""))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        GoogleImagesError::Failed(
            "Google rejected the image as too large, even after downscaling".to_owned()
        )
    );
    assert!(!client.is_blocked());
}

#[tokio::test]
async fn the_consent_cookie_goes_to_google_hosts_only() {
    let pki = TestPki::generate_for(&["www.google.com", "lens.google.com", "example.org"]);
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(303, "").header("Location", "https://lens.google.com/v3/upload"),
            Reply::new(303, "").header("Location", "https://example.org/landing"),
            html_page("<html><body></body></html>"),
        ],
    )
    .await;
    let client = GoogleImagesClient::new(net_for(
        &pki,
        &server,
        &["www.google.com", "lens.google.com", "example.org"],
    ));
    let file = image("x.png");
    client
        .search(&file.path, 5.0, None, &key_of(""))
        .await
        .unwrap();
    let seen = server.recorded();
    let line = |r: &Recorded| {
        (
            r.header("host").unwrap_or("").to_owned(),
            r.header("cookie").map(str::to_owned),
        )
    };
    assert_eq!(
        seen.iter().map(line).collect::<Vec<_>>(),
        [
            (
                "www.google.com".to_owned(),
                Some("SOCS=CAESHAgBEhIaAB".to_owned())
            ),
            (
                "lens.google.com".to_owned(),
                Some("SOCS=CAESHAgBEhIaAB".to_owned())
            ),
            ("example.org".to_owned(), None),
        ]
    );
    // The redirect to a Lens page is read by the Lens reader, and every hop keeps the headers.
    assert!(
        seen.iter()
            .all(|r| r.header("referer") == Some("https://www.google.com/"))
    );
}

#[tokio::test]
async fn google_images_counts_down_under_the_label_of_its_route_and_clears() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            html_page("<html></html>").delayed(Duration::from_millis(1300)),
            json_reply(200, json!({"responses": [{}]})).delayed(Duration::from_millis(1300)),
        ],
    )
    .await;
    let client = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let file = image("x.png");
    for (api_key, label) in [
        (key_of(""), "Waiting on Google Images"),
        (key(), "Waiting on Google (Vision)"),
    ] {
        let (on_tick, seen) = ticks();
        client
            .search(&file.path, 30.0, Some(on_tick), &api_key)
            .await
            .unwrap();
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.first().unwrap().0, label);
        assert_eq!(seen.last().unwrap(), &(String::new(), 0.0, 0.0));
    }
}

/// `requests` decodes a redirect's `Location` as UTF-8 (`Session` reads the raw bytes), so a
/// non-ASCII path is followed; reading it as ASCII-only text ended the search at the 303.
#[tokio::test]
async fn google_images_follows_a_redirect_whose_location_is_not_ascii() {
    let pki = TestPki::generate();
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(303, "").header("Location", "/search/caf\u{e9}?q=1"),
            html_page(
                "<html><body><a href=\"https://www.deviantart.com/someone/art/A-Picture-123\">\
                 <h3>A Picture</h3></a></body></html>",
            ),
        ],
    )
    .await;
    let client = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let file = image("x.png");

    let matches = client
        .search(&file.path, 30.0, None, &key_of(""))
        .await
        .unwrap();

    assert_eq!(matches.len(), 1);
    let seen = server.recorded();
    assert_eq!(seen[1].method, "GET");
    assert_eq!(seen[1].path(), "/search/caf%C3%A9");
    server.assert_script_consumed();
}
