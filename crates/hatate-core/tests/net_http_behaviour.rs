//! The net layer against a local plain-http server: measured intervals for pacing
//! and retries, timeouts, redirects, uploads. No live site is contacted.
//!
//! Timing assertions are lower bounds only (a loaded CI box can be slower than the
//! rule, never faster), so they cannot flake high. The exact-clock versions of the
//! same rules are in `src/net/stack_tests.rs`.

mod common;

use std::time::{Duration, Instant};

use bytes::Bytes;
use common::{Reply, TestServer};
use hatate_core::net::{
    Cookies, ErrorKind, NetClient, NetConfig, NetRequest, Part, Redirects, RetryMode, Secret,
};

const T: Duration = Duration::from_secs(5);

fn client() -> NetClient {
    NetClient::new(NetConfig::default()).unwrap()
}

fn get(server: &TestServer, path: &str) -> NetRequest {
    NetRequest::get(&server.url(path), T).unwrap()
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A valid gzip stream holding `data` in one stored deflate block.
fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(data.len()).unwrap();
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

// --- identity, pooling, cookies ----------------------------------------------

#[tokio::test]
async fn default_user_agent_is_sent_unless_the_caller_sets_one() {
    let server = TestServer::http(|_| Reply::ok("")).await;
    let c = client();
    c.send(get(&server, "/a")).await.unwrap();
    c.send(
        get(&server, "/b")
            .header("User-Agent", "Mozilla/5.0 test")
            .unwrap(),
    )
    .await
    .unwrap();
    let seen = server.recorded();
    assert_eq!(
        seen[0].header("user-agent"),
        Some("hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)")
    );
    assert_eq!(seen[1].header("user-agent"), Some("Mozilla/5.0 test"));
}

#[tokio::test]
async fn connections_are_reused_across_requests() {
    let server = TestServer::http(|_| Reply::ok("x")).await;
    let c = client();
    for _ in 0..4 {
        c.send(get(&server, "/")).await.unwrap();
    }
    assert_eq!(
        server.connections.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn set_cookie_is_not_kept_and_passed_cookies_are_sent() {
    let server =
        TestServer::http(|_| Reply::ok("").header("Set-Cookie", "anti_bot=1; Path=/")).await;
    let c = client();
    let first = c.send(get(&server, "/")).await.unwrap();
    assert_eq!(
        first.set_cookie_pairs(),
        vec![("anti_bot".to_owned(), "1".to_owned())]
    );
    c.send(get(&server, "/")).await.unwrap();
    let mut jar = Cookies::new();
    jar.set("PHPSESSID", "synthetic-session");
    jar.set("other", "a=b");
    c.send(get(&server, "/").with_cookies(jar)).await.unwrap();
    let seen = server.recorded();
    assert_eq!(
        seen[1].header("cookie"),
        None,
        "the server's cookie did not come back"
    );
    assert_eq!(
        seen[2].header("cookie"),
        Some("PHPSESSID=synthetic-session; other=a=b")
    );
}

#[tokio::test]
async fn gzip_bodies_are_decoded() {
    let server = TestServer::http(|_| {
        Reply::ok(gzip_stored(b"plain text")).header("Content-Encoding", "gzip")
    })
    .await;
    let resp = client().send(get(&server, "/")).await.unwrap();
    assert_eq!(resp.text(), "plain text");
}

#[tokio::test]
async fn an_error_status_is_a_response_not_an_error() {
    let server = TestServer::http(|_| Reply::new(404, "gone")).await;
    let resp = client().send(get(&server, "/")).await.unwrap();
    assert_eq!(resp.status.as_u16(), 404);
    assert_eq!(resp.text(), "gone");
}

#[tokio::test]
async fn head_has_headers_and_no_body() {
    let server =
        TestServer::http(|_| Reply::ok("0123456789").header("Content-Type", "image/png")).await;
    let resp = client()
        .send(NetRequest::head(&server.url("/img"), T).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.header("content-type"), Some("image/png"));
    assert_eq!(resp.header("content-length"), Some("10"));
    assert!(resp.body.is_empty());
    assert_eq!(server.recorded()[0].method, "HEAD");
}

#[tokio::test]
async fn query_values_are_encoded() {
    let server = TestServer::http(|_| Reply::ok("")).await;
    let key = Secret::new("a b&c=d");
    client()
        .send(
            get(&server, "/search.php")
                .query("db", "999")
                .secret_query("api_key", &key),
        )
        .await
        .unwrap();
    assert_eq!(
        server.recorded()[0].target,
        "/search.php?db=999&api_key=a+b%26c%3Dd"
    );
}

#[tokio::test]
async fn a_body_over_the_cap_is_refused() {
    let server = TestServer::http(|_| Reply::ok(vec![b'x'; 5000])).await;
    let cfg = NetConfig {
        max_body_bytes: 1000,
        ..NetConfig::default()
    };
    let err = NetClient::new(cfg)
        .unwrap()
        .send(get(&server, "/"))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::BodyTooLarge);
}

// --- uploads -----------------------------------------------------------------

#[tokio::test]
async fn a_multipart_upload_arrives_intact() {
    let server = TestServer::http(|_| Reply::ok("{}")).await;
    let payload: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let req = NetRequest::post(&server.url("/search.php"), T)
        .unwrap()
        .with_multipart(vec![Part {
            name: "file".into(),
            filename: Some("image.jpg".into()),
            mime: Some("application/octet-stream".into()),
            data: Bytes::from(payload.clone()),
        }]);
    client().send(req).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen[0].method, "POST");
    assert!(
        seen[0]
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let body = &seen[0].body;
    let text = String::from_utf8_lossy(body);
    assert!(text.contains("name=\"file\"") && text.contains("filename=\"image.jpg\""));
    assert!(
        body.windows(payload.len()).any(|w| w == payload.as_slice()),
        "the bytes arrived unchanged"
    );
}

#[tokio::test]
async fn a_raw_body_is_sent_as_is() {
    let server = TestServer::http(|_| Reply::ok("")).await;
    let req = NetRequest::post(&server.url("/add"), T)
        .unwrap()
        .with_body(Bytes::from_static(b"\x00\x01raw"));
    client().send(req).await.unwrap();
    assert_eq!(server.recorded()[0].body, b"\x00\x01raw");
}

// --- pacing and retry, measured -----------------------------------------------

#[tokio::test]
async fn min_gap_spaces_requests_by_at_least_the_gap() {
    let server = TestServer::http(|_| Reply::ok("")).await;
    let c = client();
    let gap = Duration::from_millis(300);
    for _ in 0..3 {
        c.send(get(&server, "/").with_min_gap(gap)).await.unwrap();
    }
    let seen = server.recorded();
    for pair in seen.windows(2) {
        let spacing = pair[1].at - pair[0].at;
        assert!(
            spacing >= gap - Duration::from_millis(15),
            "spacing was {spacing:?}"
        );
    }
}

#[tokio::test]
async fn a_429_is_retried_once_after_the_named_wait() {
    // First call 429, then 200.
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls2 = calls.clone();
    let server = TestServer::http(move |_| {
        if calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            Reply::new(429, "slow down").header("Retry-After", "0.5")
        } else {
            Reply::ok("served")
        }
    })
    .await;
    let started = Instant::now();
    let resp = client().send(get(&server, "/")).await.unwrap();
    assert_eq!(resp.text(), "served");
    let seen = server.recorded();
    assert_eq!(seen.len(), 2);
    assert!(
        seen[1].at - seen[0].at >= Duration::from_millis(480),
        "{:?}",
        seen[1].at - seen[0].at
    );
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[tokio::test]
async fn a_429_with_a_long_retry_after_is_returned_without_waiting() {
    let server = TestServer::http(|_| Reply::new(429, "no").header("Retry-After", "60")).await;
    let started = Instant::now();
    let resp = client().send(get(&server, "/")).await.unwrap();
    assert_eq!(resp.status.as_u16(), 429);
    assert_eq!(server.request_count(), 1);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_host_that_says_it_ran_out_is_waited_for() {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls2 = calls.clone();
    let server = TestServer::http(move |_| {
        let reply = Reply::ok("");
        if calls2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            reply
                .header("x-ratelimit-remaining", "0.0")
                .header("x-ratelimit-reset", "0.6")
        } else {
            reply
        }
    })
    .await;
    let c = client();
    c.send(get(&server, "/")).await.unwrap();
    let after_first = Instant::now();
    c.send(get(&server, "/")).await.unwrap();
    let seen = server.recorded();
    assert!(
        seen[1].at - after_first >= Duration::from_millis(550),
        "{:?}",
        seen[1].at - after_first
    );
}

#[tokio::test]
async fn a_host_that_is_closed_for_too_long_is_refused_at_once() {
    let server = TestServer::http(|_| {
        Reply::ok("")
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "45")
    })
    .await;
    let c = client();
    c.send(get(&server, "/")).await.unwrap();
    let started = Instant::now();
    let err = c.send(get(&server, "/")).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::RateLimited);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn polite_mode_retries_a_429_up_to_three_times() {
    let server = TestServer::http(|_| Reply::new(429, "").header("Retry-After", "0.1")).await;
    let req = get(&server, "/")
        .with_retry(RetryMode::pawchive())
        .with_min_gap(Duration::from_millis(100));
    let resp = client().send(req).await.unwrap();
    assert_eq!(resp.status.as_u16(), 429);
    assert_eq!(server.request_count(), 4);
}

// --- timeouts ------------------------------------------------------------------

#[tokio::test]
async fn a_server_that_never_answers_times_out() {
    let server = TestServer::http(|_| Reply::ok("late").delayed(Duration::from_secs(3))).await;
    let started = Instant::now();
    let err = client()
        .send(NetRequest::get(&server.url("/"), Duration::from_millis(250)).unwrap())
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Timeout, "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn the_deadline_stops_a_body_that_trickles_in() {
    // 30 bytes, one every 100 ms: each read is well inside the 1 s per-read timeout,
    // so only the deadline can stop it (the `hard_timeout` case, core/net.py:184-200).
    let server =
        TestServer::http(|_| Reply::ok(vec![b'z'; 30]).trickled(1, Duration::from_millis(100)))
            .await;
    let started = Instant::now();
    let err = client()
        .send(
            NetRequest::get(&server.url("/"), Duration::from_secs(1))
                .unwrap()
                .with_deadline(Duration::from_millis(500)),
        )
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DeadlineExceeded, "{err}");
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );

    // Without a deadline the same body is simply read to the end.
    let resp = client()
        .send(NetRequest::get(&server.url("/"), Duration::from_secs(1)).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.body.len(), 30);
}

// --- redirects -----------------------------------------------------------------

#[tokio::test]
async fn get_follows_redirects_and_reports_the_final_url() {
    let server = TestServer::http(|rec| match rec.target.as_str() {
        "/start" => Reply::new(302, "").header("Location", "/middle"),
        "/middle" => Reply::new(301, "").header("Location", "/end?x=1"),
        _ => Reply::ok("arrived"),
    })
    .await;
    let resp = client().send(get(&server, "/start")).await.unwrap();
    assert_eq!(resp.text(), "arrived");
    assert_eq!(resp.url.path(), "/end");
    assert_eq!(resp.url.query(), Some("x=1"));
}

#[tokio::test]
async fn head_does_not_follow_unless_asked() {
    let server = TestServer::http(|rec| {
        if rec.target == "/old" {
            Reply::new(301, "").header("Location", "/new")
        } else {
            Reply::ok("").header("X-Final", "yes")
        }
    })
    .await;
    let c = client();
    let stay = c
        .send(NetRequest::head(&server.url("/old"), T).unwrap())
        .await
        .unwrap();
    assert_eq!(stay.status.as_u16(), 301);
    assert_eq!(stay.url.path(), "/old");
    let follow = c
        .send(
            NetRequest::head(&server.url("/old"), T)
                .unwrap()
                .with_redirects(Redirects::Follow),
        )
        .await
        .unwrap();
    assert_eq!(follow.status.as_u16(), 200);
    assert_eq!(follow.url.path(), "/new");
    assert_eq!(follow.header("x-final"), Some("yes"));
    // And GET can be told to stop.
    let stop = c
        .send(get(&server, "/old").with_redirects(Redirects::Stop))
        .await
        .unwrap();
    assert_eq!(stop.status.as_u16(), 301);
}

#[tokio::test]
async fn post_becomes_get_on_302_and_stays_post_on_307() {
    let server = TestServer::http(|rec| match rec.target.as_str() {
        "/a" => Reply::new(302, "").header("Location", "/landed"),
        "/b" => Reply::new(307, "").header("Location", "/landed"),
        _ => Reply::ok(""),
    })
    .await;
    let c = client();
    let post = |path: &str| {
        NetRequest::post(&server.url(path), T)
            .unwrap()
            .with_body(Bytes::from_static(b"payload"))
    };
    c.send(post("/a")).await.unwrap();
    c.send(post("/b")).await.unwrap();
    let seen = server.recorded();
    assert_eq!((seen[1].method.as_str(), seen[1].body.len()), ("GET", 0));
    assert_eq!(
        (seen[3].method.as_str(), seen[3].body.as_slice()),
        ("POST", &b"payload"[..])
    );
}

#[tokio::test]
async fn a_redirect_loop_stops_at_the_limit() {
    let server = TestServer::http(|_| Reply::new(302, "").header("Location", "/loop")).await;
    let cfg = NetConfig {
        max_redirects: 4,
        ..NetConfig::default()
    };
    let err = NetClient::new(cfg)
        .unwrap()
        .send(get(&server, "/loop"))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TooManyRedirects);
    assert_eq!(
        server.request_count(),
        5,
        "the first request and four redirects"
    );
}

#[tokio::test]
async fn credentials_stay_on_their_host_when_a_redirect_leaves_it() {
    // 127.0.0.1 redirects to the same server under the name localhost: another host.
    let target = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let target2 = target.clone();
    let server = TestServer::http(move |rec| {
        if rec
            .header("host")
            .is_some_and(|h| h.starts_with("127.0.0.1"))
        {
            Reply::new(302, "").header("Location", &target2.lock().unwrap())
        } else {
            Reply::ok("landed")
        }
    })
    .await;
    *target.lock().unwrap() = server.url_localhost("/landing");

    let secret = Secret::new("synthetic-bearer");
    let mut jar = Cookies::new();
    jar.set("sid", "synthetic-sid");
    let req = || {
        get(&server, "/start")
            .secret_header("Authorization", &secret)
            .unwrap()
            .secret_header("Hydrus-Client-API-Access-Key", &secret)
            .unwrap()
            .header("Referer", "https://example.test/")
            .unwrap()
            .with_cookies(jar.clone())
    };
    client().send(req()).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].header("authorization"), Some("synthetic-bearer"));
    assert_eq!(
        seen[0].header("hydrus-client-api-access-key"),
        Some("synthetic-bearer")
    );
    assert_eq!(seen[0].header("cookie"), Some("sid=synthetic-sid"));
    // The other host sees none of it, but the ordinary header travels.
    assert_eq!(seen[1].header("authorization"), None);
    assert_eq!(seen[1].header("hydrus-client-api-access-key"), None);
    assert_eq!(seen[1].header("cookie"), None);
    assert_eq!(seen[1].header("referer"), Some("https://example.test/"));

    // Cookies scoped to both names follow; the Authorization header still does not.
    server.requests.lock().unwrap().clear();
    let scoped = jar.clone().with_scope(["127.0.0.1", "localhost"]);
    client().send(req().with_cookies(scoped)).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen[1].header("cookie"), Some("sid=synthetic-sid"));
    assert_eq!(seen[1].header("authorization"), None);
}

#[tokio::test]
async fn credentials_follow_a_redirect_on_the_same_host() {
    let server = TestServer::http(|rec| {
        if rec.target == "/start" {
            Reply::new(302, "").header("Location", "/next")
        } else {
            Reply::ok("")
        }
    })
    .await;
    let secret = Secret::new("synthetic-bearer");
    client()
        .send(
            get(&server, "/start")
                .secret_header("Authorization", &secret)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        server.recorded()[1].header("authorization"),
        Some("synthetic-bearer")
    );
}

#[tokio::test]
async fn close_all_drops_the_pool() {
    let server = TestServer::http(|_| Reply::ok("")).await;
    let c = client();
    c.send(get(&server, "/")).await.unwrap();
    c.close_all().unwrap();
    c.send(get(&server, "/")).await.unwrap();
    assert_eq!(
        server.connections.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}
