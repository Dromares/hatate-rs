//! Config-supplied secrets never reach `Debug`, logs or errors (docs/net.md section 9).
//!
//! Every secret here is synthetic. Each check also proves it is looking at
//! something: the secret really was sent, and the log really has lines.

mod common;

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Reply, TestPki, TestServer};
use hatate_core::net::site::{SiteCredentials, cookies_for_url, headers_for_url};
use hatate_core::net::{
    Cookies, ErrorKind, NetClient, NetConfig, NetError, NetRequest, RetryMode, Secret,
};
use tracing_subscriber::fmt::MakeWriter;

const API_KEY: &str = "SYNTH-api-key-4f1c9a";
const COOKIE: &str = "SYNTH-cookie-9d2e71";
const BASIC: &str = "SYNTH-basic-77aa03";
const HYDRUS: &str = "SYNTH-hydrus-31bcd5";
const ALL: [&str; 4] = [API_KEY, COOKIE, BASIC, HYDRUS];

const T: Duration = Duration::from_secs(3);

fn assert_clean(what: &str, text: &str) {
    for secret in ALL {
        assert!(!text.contains(secret), "{what} leaked {secret}: {text}");
    }
}

/// A request carrying every kind of secret the layer handles.
fn loaded(url: &str) -> NetRequest {
    let mut jar = Cookies::new();
    jar.set("PHPSESSID", COOKIE);
    NetRequest::post(url, T)
        .unwrap()
        .secret_query("api_key", &Secret::new(API_KEY))
        .secret_header("Authorization", &Secret::new(BASIC))
        .unwrap()
        .secret_header("Hydrus-Client-API-Access-Key", &Secret::new(HYDRUS))
        .unwrap()
        .with_cookies(jar)
}

fn assert_error_clean(what: &str, err: &NetError) {
    assert_clean(what, &format!("{err}"));
    assert_clean(what, &format!("{err:?}"));
    assert_clean(what, &format!("{err:#?}"));
    if let Some(url) = err.url() {
        assert!(
            !url.contains('?'),
            "{what}: the error URL kept its query: {url}"
        );
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[test]
fn debug_of_every_secret_carrying_type_is_clean() {
    let mut jar = Cookies::new();
    jar.set("PHPSESSID", COOKIE);
    let creds = SiteCredentials {
        pixiv_session_cookie: COOKIE.into(),
        sankaku_cookies: format!("login=you; sid={COOKIE}").as_str().into(),
        animepictures_cookies: COOKIE.into(),
        deviantart_cookies: format!("auth={COOKIE}").as_str().into(),
        e621_username: BASIC.into(),
        e621_api_key: API_KEY.into(),
    };
    let req = loaded("https://saucenao.com/search.php?db=999");
    let client = NetClient::new(NetConfig::default()).unwrap();
    let dumps = [
        format!("{req:?}"),
        format!("{req:#?}"),
        format!("{jar:?}"),
        format!("{creds:?}"),
        format!("{creds:#?}"),
        format!("{:?}", Secret::new(API_KEY)),
        format!("{client:?}"),
        format!("{:?}", NetConfig::default()),
        format!("{:?}", cookies_for_url("https://www.pixiv.net/x", &creds)),
        format!(
            "{:?}",
            headers_for_url("https://e621.net/posts/1", Some(&creds))
        ),
        format!(
            "{:?}",
            headers_for_url("https://e621.net/posts/1", Some(&creds)).map(|h| h.len())
        ),
    ];
    for dump in &dumps {
        assert_clean("Debug", dump);
    }
    // The request dump still says something useful: names, never values.
    assert!(dumps[0].contains("api_key") && dumps[0].contains("PHPSESSID"));
    assert!(dumps[0].contains("authorization: [redacted]"));
    assert!(dumps[0].contains("https://saucenao.com/search.php"));
    // And the HeaderMap Debug path of http itself hides sensitive values.
    let headers = headers_for_url("https://e621.net/posts/1", Some(&creds)).unwrap();
    assert_clean("HeaderMap Debug", &format!("{headers:?}"));
    // Positive control: the secrets really are in there to be sent.
    assert_eq!(
        req.url
            .query_pairs()
            .find(|(k, _)| k == "api_key")
            .unwrap()
            .1,
        API_KEY
    );
}

#[tokio::test]
async fn a_response_dump_hides_set_cookie_values() {
    let server =
        TestServer::http(|_| Reply::ok("").header("Set-Cookie", &format!("auth={COOKIE}; Path=/")))
            .await;
    let client = NetClient::new(NetConfig::default()).unwrap();
    let resp = client
        .send(loaded(&server.url("/p")).with_retry(RetryMode::Never))
        .await
        .unwrap();
    assert_eq!(
        resp.set_cookie_pairs(),
        vec![("auth".to_owned(), COOKIE.to_owned())],
        "positive control"
    );
    assert_clean("NetResponse Debug", &format!("{resp:?}"));
    assert_clean("NetResponse Debug", &format!("{resp:#?}"));
    // The server really did get every secret.
    let seen = &server.recorded()[0];
    assert_eq!(seen.header("authorization"), Some(BASIC));
    assert_eq!(seen.header("hydrus-client-api-access-key"), Some(HYDRUS));
    assert_eq!(
        seen.header("cookie"),
        Some(format!("PHPSESSID={COOKIE}").as_str())
    );
    assert!(seen.target.contains(API_KEY));
}

#[tokio::test]
async fn errors_carry_no_secret_whatever_went_wrong() {
    let client = NetClient::new(NetConfig::default()).unwrap();

    // Connection refused: a port nothing listens on.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let err = client
        .send(loaded(&format!("http://127.0.0.1:{closed}/search.php")))
        .await
        .unwrap_err();
    assert!(
        matches!(err.kind(), ErrorKind::Connect | ErrorKind::Other),
        "{err}"
    );
    assert_error_clean("connection refused", &err);

    // Per-read timeout.
    let slow = TestServer::http(|_| Reply::ok("").delayed(Duration::from_secs(2))).await;
    let mut req = loaded(&slow.url("/search.php"));
    req.timeout = Duration::from_millis(150);
    let err = client.send(req).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Timeout);
    assert_error_clean("timeout", &err);

    // Deadline.
    let trickle =
        TestServer::http(|_| Reply::ok(vec![b'x'; 40]).trickled(1, Duration::from_millis(60)))
            .await;
    let err = client
        .send(loaded(&trickle.url("/search.php")).with_deadline(Duration::from_millis(250)))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DeadlineExceeded);
    assert_error_clean("deadline", &err);

    // Redirect loop, with the secret in every Location.
    let looping = TestServer::http(|_| {
        Reply::new(302, "").header("Location", &format!("/loop?api_key={API_KEY}"))
    })
    .await;
    let cfg = NetConfig {
        max_redirects: 2,
        ..NetConfig::default()
    };
    let err = NetClient::new(cfg)
        .unwrap()
        .send(loaded(&looping.url("/loop")))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TooManyRedirects);
    assert_error_clean("redirect loop", &err);

    // A redirect to something that is not a URL the layer will follow.
    let bad = TestServer::http(|_| {
        Reply::new(302, "").header(
            "Location",
            &format!("ftp://example.invalid/?api_key={API_KEY}"),
        )
    })
    .await;
    let err = client.send(loaded(&bad.url("/x"))).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidRequest);
    assert_error_clean("bad redirect", &err);

    // Body too large.
    let big = TestServer::http(|_| Reply::ok(vec![0u8; 4096])).await;
    let small = NetClient::new(NetConfig {
        max_body_bytes: 10,
        ..NetConfig::default()
    })
    .unwrap();
    let err = small.send(loaded(&big.url("/x"))).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::BodyTooLarge);
    assert_error_clean("body too large", &err);

    // TLS failure: an untrusted certificate.
    let pki = TestPki::generate();
    let tls = TestServer::https(&pki, |_| Reply::ok("")).await;
    let err = client
        .send(loaded(&tls.url("/search.php")))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Tls);
    assert_error_clean("tls", &err);

    // A host that is closed for too long.
    let closed_host = TestServer::http(|_| {
        Reply::ok("")
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "60")
    })
    .await;
    client.send(loaded(&closed_host.url("/x"))).await.unwrap();
    let err = client
        .send(loaded(&closed_host.url("/x")))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::RateLimited);
    assert_error_clean("rate limited", &err);

    // Request building: a bad URL and a bad header value never echo their input.
    let err =
        NetRequest::get(&format!("http://exa mple.invalid/?api_key={API_KEY}"), T).unwrap_err();
    assert_error_clean("bad url", &err);
    let err = NetRequest::get("http://example.invalid/", T)
        .unwrap()
        .secret_header("Authorization", &Secret::new(format!("{BASIC}\n")))
        .unwrap_err();
    assert_error_clean("bad header", &err);
}

#[tokio::test]
async fn log_lines_carry_no_secret() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls2 = calls.clone();
    let server = TestServer::http(move |_| {
        // 429 once (a retry log), and a closed-host header (a wait log).
        match calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
            0 => Reply::new(429, "").header("Retry-After", "0.05"),
            1 => Reply::ok("")
                .header("x-ratelimit-remaining", "0")
                .header("x-ratelimit-reset", "0.05")
                .header("Set-Cookie", &format!("auth={COOKIE}")),
            _ => Reply::ok(""),
        }
    })
    .await;
    let client = NetClient::new(NetConfig::default()).unwrap();
    let req = || {
        let mut r = loaded(&server.url("/search.php"));
        r.method = hatate_core::net::Method::Get;
        r
    };
    client.send(req()).await.unwrap();
    client.send(req()).await.unwrap();

    let logged = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(
        logged.contains("retrying once"),
        "positive control: the retry was logged\n{logged}"
    );
    assert!(
        logged.contains("waiting for it to reset"),
        "positive control: the wait was logged\n{logged}"
    );
    assert_clean("log output", &logged);
    assert!(
        !logged.contains("api_key"),
        "not even the parameter name is logged\n{logged}"
    );
}
