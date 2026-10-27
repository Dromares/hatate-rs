//! `core/availability.py` (DAN-2060, Phase 3.d), through `NetClient::new` (rustls with the
//! explicit provider) against a local https server. Site hosts are reached through
//! `NetConfig::with_host_override`, so the server sees the real `Host` and the real path. No live
//! network is touched.
//!
//! The transport differential against the real Python (`worktrees/DAN-2060-differential.log`) is
//! the broad check; these are the named, readable ones. The Python test each one maps to is in
//! `docs/availability.md` section 6.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::availability::{self, SettingsAccess};
use hatate_core::config::Settings;
use hatate_core::model::Candidate;
use hatate_core::net::site;
use hatate_core::net::{Cookies, NetClient, NetConfig, USER_AGENT};
use hatate_core::remote::{self, Raised};
use tracing_subscriber::fmt::MakeWriter;

const SANKAKU: &str = "chan.sankakucomplex.com";
const BROWSER_UA: &str = "Mozilla/5.0";

struct World {
    server: TestServer,
    client: NetClient,
}

/// One https server for every host in `hosts`; `handler` sees the request and its `Host`.
async fn world(
    hosts: &[&str],
    handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
) -> World {
    let pki = TestPki::generate_for(hosts);
    let server = TestServer::https(&pki, handler).await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, server.addr);
    }
    let client = NetClient::new(cfg).unwrap();
    World { server, client }
}

impl World {
    /// `host target` of every request, in order.
    fn seen(&self) -> Vec<String> {
        self.server
            .recorded()
            .iter()
            .map(|r| format!("{} {}", r.header("host").unwrap_or(""), r.target))
            .collect()
    }

    async fn check(&self, url: &str) -> Option<bool> {
        availability::check_url_available(&self.client, Some(url), 5.0, None, None, None, None)
            .await
            .unwrap()
    }

    async fn page(&self, url: &str) -> Option<bool> {
        availability::page_says_available(&self.client, Some(url), 5.0, None, None, None, None)
            .await
            .unwrap()
    }
}

/// The same answer to every request.
fn always(reply: Reply) -> impl Fn(&Recorded) -> Reply + Send + Sync {
    move |_| reply.clone()
}

/// A redirect to `to`, then the page at `to`.
fn bounced_to(
    to: &'static str,
    final_body: &'static [u8],
) -> impl Fn(&Recorded) -> Reply + Send + Sync {
    move |req| {
        let landed = to.split_once("://").map_or(to, |(_, rest)| rest);
        let here = format!("{}{}", req.header("host").unwrap_or(""), req.target);
        if here == landed {
            Reply::ok(final_body)
        } else {
            Reply::new(302, "").header("Location", to)
        }
    }
}

// ---- the central rule (TestAvailabilityDetection) ----------------------------------------

#[tokio::test]
async fn only_a_definite_404_or_410_is_gone() {
    for code in [404u16, 410] {
        let w = world(&["x.test"], always(Reply::new(code, ""))).await;
        assert_eq!(w.check("https://x.test/1").await, Some(false), "{code}");
    }
}

#[tokio::test]
async fn success_is_available() {
    for code in [200u16, 301, 302] {
        let w = world(&["x.test"], always(Reply::new(code, ""))).await;
        assert_eq!(w.check("https://x.test/1").await, Some(true), "{code}");
    }
}

#[tokio::test]
async fn a_403_and_server_errors_are_unknown_not_gone() {
    for code in [403u16, 500, 502, 503] {
        let w = world(&["x.test"], always(Reply::new(code, ""))).await;
        assert_eq!(w.check("https://x.test/1").await, None, "{code}");
    }
}

#[tokio::test]
async fn a_network_failure_is_unknown() {
    let w = world(&["x.test"], always(Reply::aborted())).await;
    assert_eq!(w.check("https://x.test/1").await, None);
}

#[tokio::test]
async fn a_missing_url_is_safe() {
    let w = world(&["x.test"], always(Reply::ok(""))).await;
    let got = availability::check_url_available(&w.client, None, 5.0, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert!(w.server.recorded().is_empty());
}

// ---- the shipped scheme, and the wall-clock deadline --------------------------------------

/// The verifier's PASS names this test (DAN-1909): the check runs through the binary's own
/// constructor, `NetClient::new`, over real TLS to a local https server.
#[tokio::test]
async fn availability_check_runs_over_tls_through_the_binaries_constructor() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |req| {
        if req.target == "/gone" {
            Reply::new(404, "")
        } else {
            Reply::ok("<html></html>")
        }
    })
    .await;
    let client =
        NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap();
    let live = server.url("/post/1");
    let dead = server.url("/gone");
    let got = availability::check_url_available(&client, Some(&live), 5.0, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(got, Some(true));
    let got = availability::check_url_available(&client, Some(&dead), 5.0, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(got, Some(false));
    let seen = server.recorded();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].header("user-agent"), Some(USER_AGENT));
}

#[tokio::test]
async fn a_head_that_never_arrives_is_unknown_at_the_deadline() {
    let w = world(
        &["x.test"],
        always(Reply::ok("late").delayed(Duration::from_secs(5))),
    )
    .await;
    let t0 = Instant::now();
    let got = availability::check_url_available(
        &w.client,
        Some("https://x.test/1"),
        0.4,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let took = t0.elapsed();
    assert_eq!(got, None);
    assert!(took >= Duration::from_millis(350), "{took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[tokio::test]
async fn the_deadline_covers_the_body_not_each_read() {
    // The head comes at once and the body trickles: every read is quick, the whole is not.
    let page = vec![b'x'; 4000];
    let w = world(
        &[SANKAKU, "sankakuapi.com"],
        always(Reply::ok(page).trickled(100, Duration::from_millis(100))),
    )
    .await;
    let t0 = Instant::now();
    let got = availability::page_says_available(
        &w.client,
        Some("https://chan.sankakucomplex.com/en/posts/abc"),
        0.5,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let took = t0.elapsed();
    assert_eq!(
        got, None,
        "a body that outlasts the deadline settles nothing"
    );
    assert!(took < Duration::from_secs(2), "{took:?}");
}

// ---- structurally dead URLs ---------------------------------------------------------------

#[test]
fn a_numeric_sankaku_id_is_structurally_dead() {
    for url in [
        "https://chan.sankakucomplex.com/en/posts/4704392",
        "https://chan.sankakucomplex.com/en/posts/4541326",
        "https://chan.sankakucomplex.com/post/show/371628",
    ] {
        assert!(availability::url_is_structurally_dead(url), "{url}");
    }
}

#[test]
fn a_modern_sankaku_id_is_not_structurally_dead() {
    assert!(!availability::url_is_structurally_dead(
        "https://www.sankakucomplex.com/posts/09azv5E4Dak"
    ));
}

#[test]
fn other_sites_are_never_called_structurally_dead() {
    for url in [
        "https://danbooru.donmai.us/posts/123",
        "https://gelbooru.com/index.php?page=post&s=view&id=1",
        "https://rule34.xxx/index.php?page=post&s=view&id=4585855",
        "https://rule34.paheal.net/post/view/5674224",
        "",
    ] {
        assert!(!availability::url_is_structurally_dead(url), "{url:?}");
    }
}

#[test]
fn a_cached_result_is_cleaned_of_structurally_dead_matches() {
    let cached = vec![
        Candidate::new("https://chan.sankakucomplex.com/en/posts/4541326"),
        Candidate::new("https://danbooru.donmai.us/posts/1"),
    ];
    let kept = availability::drop_structurally_dead(cached, "x.png");
    let urls: Vec<&str> = kept.iter().map(|c| c.url.as_str()).collect();
    assert_eq!(urls, ["https://danbooru.donmai.us/posts/1"]);
}

// ---- Sankaku's API (TestSankakuPostExists) ------------------------------------------------

async fn api_world(status: u16, body: &'static str) -> World {
    world(&["sankakuapi.com"], always(Reply::new(status, body))).await
}

async fn post_exists(w: &World, url: &str) -> Option<bool> {
    availability::sankaku_post_exists(&w.client, url, 5.0, None).await
}

#[tokio::test]
async fn a_numeric_id_is_dead_whatever_became_of_the_post() {
    let w = api_world(400, r#"{"success": false, "code": "invalid id"}"#).await;
    let got = post_exists(&w, "https://chan.sankakucomplex.com/en/posts/3934951").await;
    assert_eq!(got, Some(false));
}

#[tokio::test]
async fn a_live_post_with_a_picture_is_available() {
    let w = api_world(
        200,
        r#"{"id": "y0abg167r2o", "status": "active", "file_url": "https://v.sankakucomplex.com/x.jpg"}"#,
    )
    .await;
    let got = post_exists(&w, "https://www.sankakucomplex.com/posts/y0abg167r2o").await;
    assert_eq!(got, Some(true));
}

#[tokio::test]
async fn a_post_whose_picture_is_withheld_counts_as_gone() {
    let w = api_world(
        200,
        r#"{"id": "y0abg167r2o", "status": "active", "file_url": null, "sample_url": null,
            "preview_url": null, "redirect_to_signup": true}"#,
    )
    .await;
    let got = post_exists(&w, "https://www.sankakucomplex.com/posts/y0abg167r2o").await;
    assert_eq!(got, Some(false));
}

#[tokio::test]
async fn a_deleted_status_counts_as_gone() {
    let w = api_world(
        200,
        r#"{"id": "abc", "status": "deleted", "file_url": "https://v.sankakucomplex.com/x.jpg"}"#,
    )
    .await;
    let got = post_exists(&w, "https://www.sankakucomplex.com/posts/abc").await;
    assert_eq!(got, Some(false));
}

#[tokio::test]
async fn a_network_failure_is_unknown_never_gone_for_the_api() {
    let w = world(&["sankakuapi.com"], always(Reply::aborted())).await;
    let got = post_exists(&w, "https://www.sankakucomplex.com/posts/abc").await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn rate_limiting_is_unknown_too() {
    let w = api_world(429, "{}").await;
    let got = post_exists(&w, "https://www.sankakucomplex.com/posts/abc").await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn other_sites_are_left_to_the_ordinary_check() {
    let w = api_world(200, "{}").await;
    for url in [
        "https://danbooru.donmai.us/posts/1",
        "https://gelbooru.com/index.php?page=post&s=view&id=1",
        "https://rule34.paheal.net/post/view/5674224",
        "",
    ] {
        assert_eq!(post_exists(&w, url).await, None, "{url:?}");
    }
    assert!(
        w.server.recorded().is_empty(),
        "not a Sankaku URL: no request at all"
    );
}

#[tokio::test]
async fn both_sankaku_url_shapes_are_recognised() {
    let w = api_world(
        200,
        r#"{"status": "active", "file_url": "https://v/x.jpg"}"#,
    )
    .await;
    for (url, id) in [
        (
            "https://www.sankakucomplex.com/posts/y0abg167r2o",
            "y0abg167r2o",
        ),
        (
            "https://chan.sankakucomplex.com/en/posts/3934951",
            "3934951",
        ),
        ("https://chan.sankakucomplex.com/post/show/1234", "1234"),
    ] {
        w.server.requests.lock().unwrap().clear();
        post_exists(&w, url).await;
        assert_eq!(w.seen(), [format!("sankakuapi.com /posts/{id}")], "{url}");
    }
}

#[tokio::test]
async fn the_sankaku_login_goes_to_the_sankaku_api_host_and_no_other() {
    let w = api_world(
        200,
        r#"{"status": "active", "file_url": "https://v/x.jpg"}"#,
    )
    .await;
    let mut cookies = Cookies::new();
    cookies.set("sid", "v1");
    availability::sankaku_post_exists(
        &w.client,
        "https://www.sankakucomplex.com/posts/abc",
        5.0,
        Some(cookies),
    )
    .await;
    assert_eq!(w.server.recorded()[0].header("cookie"), Some("sid=v1"));
}

// ---- soft redirects (TestSoftRedirectDetection) -------------------------------------------

#[tokio::test]
async fn a_sankaku_empty_page_redirect_is_gone() {
    let w = world(
        &[SANKAKU],
        bounced_to("https://chan.sankakucomplex.com/en/posts/show_empty", b""),
    )
    .await;
    assert_eq!(
        w.check("https://chan.sankakucomplex.com/en/posts/abc123")
            .await,
        Some(false)
    );
}

#[tokio::test]
async fn a_sankaku_not_found_redirect_on_another_subdomain_is_gone() {
    let w = world(
        &[SANKAKU, "www.sankakucomplex.com"],
        bounced_to("https://www.sankakucomplex.com/errors/not_found", b""),
    )
    .await;
    assert_eq!(
        w.check("https://chan.sankakucomplex.com/post/show/4762937")
            .await,
        Some(false)
    );
}

#[tokio::test]
async fn a_live_sankaku_post_survives_the_not_found_rule() {
    for url in [
        "https://chan.sankakucomplex.com/post/show/4762937",
        "https://chan.sankakucomplex.com/en/posts/abc123",
    ] {
        let w = world(&[SANKAKU], always(Reply::ok(""))).await;
        assert_eq!(w.page(url).await, Some(true), "{url}");
    }
}

#[tokio::test]
async fn the_landing_matches_whatever_the_locale_prefix_or_trailing_slash() {
    for landing in [
        "https://chan.sankakucomplex.com/posts/show_empty",
        "https://chan.sankakucomplex.com/en/posts/show_empty/",
        "https://chan.sankakucomplex.com/ja/posts/show_empty",
    ] {
        let w = world(&[SANKAKU], bounced_to(landing, b"")).await;
        assert_eq!(
            w.check("https://chan.sankakucomplex.com/posts/abc123")
                .await,
            Some(false),
            "{landing}"
        );
    }
}

#[tokio::test]
async fn the_rule_does_not_leak_to_other_sites() {
    let w = world(
        &["danbooru.donmai.us"],
        bounced_to("https://danbooru.donmai.us/posts/show_empty", b""),
    )
    .await;
    assert_eq!(
        w.check("https://danbooru.donmai.us/posts/1").await,
        Some(true)
    );
}

#[tokio::test]
async fn a_network_failure_is_still_unknown_for_sankaku() {
    let w = world(&[SANKAKU, "sankakuapi.com"], always(Reply::aborted())).await;
    assert_eq!(
        w.check("https://chan.sankakucomplex.com/en/posts/x").await,
        None
    );
}

#[test]
fn sankaku_and_the_moebooru_hosts_are_enrolled_for_checking() {
    assert!(availability::should_availability_check(
        "https://chan.sankakucomplex.com/en/posts/x"
    ));
    assert!(availability::should_availability_check(
        "https://sankakucomplex.com/post/show/1"
    ));
    for url in [
        "https://yande.re/post/show/1",
        "https://konachan.com/post/show/1",
        "https://konachan.net/post/show/1",
    ] {
        assert!(availability::should_availability_check(url), "{url}");
    }
}

// ---- Gelbooru and the other `s=list` sites (TestGelbooruDeletedPosts, TestRule34Soft404) ---

#[tokio::test]
async fn a_deleted_post_redirecting_to_the_list_is_gone() {
    for (host, post) in [
        (
            "gelbooru.com",
            "https://gelbooru.com/index.php?page=post&s=view&id=4154084",
        ),
        (
            "safebooru.org",
            "https://safebooru.org/index.php?page=post&s=view&id=1",
        ),
        (
            "rule34.xxx",
            "https://rule34.xxx/index.php?page=post&s=view&id=4585855",
        ),
    ] {
        let list: &'static str = Box::leak(
            format!("https://{host}/index.php?page=post&s=list&tags=all").into_boxed_str(),
        );
        let w = world(&[host], bounced_to(list, b"")).await;
        assert_eq!(w.check(post).await, Some(false), "{host}");
    }
}

#[tokio::test]
async fn a_live_gelbooru_post_is_available() {
    let w = world(&["gelbooru.com"], always(Reply::ok(""))).await;
    assert_eq!(
        w.check("https://gelbooru.com/index.php?page=post&s=view&id=14748461")
            .await,
        Some(true)
    );
}

#[tokio::test]
async fn a_list_url_does_not_report_itself_gone() {
    // The fragment must appear in where the request LANDED and not in where it started.
    let listing = "https://gelbooru.com/index.php?page=post&s=list&tags=all";
    let w = world(&["gelbooru.com"], always(Reply::ok(""))).await;
    assert_eq!(w.check(listing).await, Some(true));
    assert_eq!(
        availability::redirect_means_gone(listing, Some(listing)),
        None
    );
    let post = "https://gelbooru.com/index.php?page=post&s=view&id=4154084";
    assert!(availability::redirect_means_gone(post, Some(listing)).is_some());
}

#[test]
fn gelbooru_has_no_wording_to_match_on() {
    let post = "https://gelbooru.com/index.php?page=post&s=view&id=4154084";
    assert!(availability::soft_404_markers_for(post).is_empty());
}

#[tokio::test]
async fn a_gelbooru_network_failure_is_still_unknown() {
    let w = world(&["gelbooru.com"], always(Reply::aborted())).await;
    assert_eq!(
        w.check("https://gelbooru.com/index.php?page=post&s=view&id=1")
            .await,
        None
    );
}

// ---- the body markers (TestSankakuEmptyBody) ----------------------------------------------

const SANKAKU_EMPTY_PAGE: &[u8] = br#"<!DOCTYPE html>
<html lang="en" data-current-user-theme="dark">
  <head>
    <title>No Content | Sankaku</title>
    <link rel="icon" href="//chan.sankakucomplex.com/images/favicon.png">
    <link rel="alternate" hreflang="en" href="https://chan.sankakucomplex.com/en/posts/show_empty" />
    <link rel="alternate" hreflang="ja" href="https://chan.sankakucomplex.com/ja/en/posts/show_empty" />
  </head>
  <body class="en">
    <h2 id="page-title"><a href="/">Sankaku</a>/<a href="https://chan.sankakucomplex.com/en/posts/show_empty">No Content</a></h2>
    <div><h3>Nothing is visible to you here.</h3></div>
  </body>
</html>"#;

const SANKAKU_LIVE_PAGE: &[u8] = br#"<!DOCTYPE html><html><head>
<title>rainbow dash spitfire | Sankaku</title>
<link rel="alternate" hreflang="en" href="https://chan.sankakucomplex.com/en/posts/5960112" />
</head><body><div id="post-content">
<img src="//is.sankakucomplex.com/data/ab/cd/abcd.jpg"></div></body></html>"#;

const EMPTY_URL: &str = "https://chan.sankakucomplex.com/post/show/5960112";

#[tokio::test]
async fn a_real_empty_sankaku_page_is_detected() {
    let w = world(
        &[SANKAKU, "sankakuapi.com"],
        always(Reply::ok(SANKAKU_EMPTY_PAGE)),
    )
    .await;
    assert_eq!(w.check(EMPTY_URL).await, Some(false));
}

#[tokio::test]
async fn a_real_post_page_is_not_flagged() {
    let w = world(&[SANKAKU], always(Reply::ok(SANKAKU_LIVE_PAGE))).await;
    assert_eq!(w.page(EMPTY_URL).await, Some(true));
}

#[tokio::test]
async fn the_head_alone_is_enough() {
    let cut = SANKAKU_EMPTY_PAGE
        .windows(7)
        .position(|w| w == b"</head>")
        .unwrap();
    let head_only = &SANKAKU_EMPTY_PAGE[..cut];
    assert!(!head_only.windows(18).any(|w| w == b"Nothing is visible"));
    let w = world(&[SANKAKU, "sankakuapi.com"], always(Reply::ok(head_only))).await;
    assert_eq!(w.check(EMPTY_URL).await, Some(false));
}

#[test]
fn the_first_sankaku_markers_appear_early_in_the_document() {
    let lowered = String::from_utf8(SANKAKU_EMPTY_PAGE.to_vec())
        .unwrap()
        .to_lowercase();
    let markers = availability::soft_404_markers_for(EMPTY_URL);
    for marker in &markers[..2] {
        let at = lowered.find(marker).unwrap_or_else(|| panic!("{marker}"));
        assert!(at < availability::SOFT_404_SCAN_BYTES, "{marker}");
    }
}

#[tokio::test]
async fn an_unrecognised_body_leaves_the_match_alone() {
    let w = world(&[SANKAKU], always(Reply::ok("<html>something else</html>"))).await;
    assert_eq!(w.page(EMPTY_URL).await, Some(true));
}

// ---- logging (TestAvailabilityLoggingIsDiagnosable) ---------------------------------------

// One process-global subscriber, installed once, writing into a per-thread buffer. A scoped
// `set_default` per test is racy under the parallel test runner: tracing caches each callsite's
// interest process-wide, so a callsite first hit by a thread with no subscriber stays disabled
// for the test that does capture (the hosted run lost `a_marker_miss_reports_the_page_title`
// that way). The tests are `current_thread` runtimes, so the code under test logs on the
// test's own thread.
thread_local! {
    static LOG_BUFFER: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Default)]
struct Capture;

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        LOG_BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        Capture
    }
}

fn capture_logs() -> (Capture, ()) {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(Capture)
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other subscriber in this test binary");
    });
    LOG_BUFFER.with(|b| b.borrow_mut().clear());
    (Capture, ())
}

fn logged(_capture: &Capture) -> String {
    LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap())
}

#[tokio::test]
async fn success_says_the_body_was_scanned() {
    let (capture, _guard) = capture_logs();
    let w = world(
        &[SANKAKU],
        always(Reply::ok("<title>art | Sankaku</title>")),
    )
    .await;
    assert_eq!(
        w.page("https://chan.sankakucomplex.com/post/show/4762937")
            .await,
        Some(true)
    );
    let text = logged(&capture);
    assert!(text.contains("scanned"), "{text}");
    assert!(text.contains("marker"), "{text}");
}

#[tokio::test]
async fn a_marker_miss_reports_the_page_title() {
    let (capture, _guard) = capture_logs();
    let w = world(
        &[SANKAKU],
        always(Reply::ok("<html><head><title>Just a moment...</title>")),
    )
    .await;
    assert_eq!(
        w.page("https://chan.sankakucomplex.com/post/show/4762937")
            .await,
        Some(true)
    );
    assert!(logged(&capture).contains("Just a moment"));
}

#[test]
fn a_body_that_is_not_html_reports_no_title() {
    assert!(availability::page_title("\u{1f}\u{8b}\u{8}\u{0}binary").contains("no title"));
}

#[tokio::test]
async fn success_says_when_a_host_has_no_markers() {
    let (capture, _guard) = capture_logs();
    let w = world(&["danbooru.donmai.us"], always(Reply::ok(""))).await;
    assert_eq!(
        w.check("https://danbooru.donmai.us/posts/1").await,
        Some(true)
    );
    assert!(logged(&capture).contains("no body markers"));
}

// ---- Moebooru (TestMoebooruDeletedPosts) --------------------------------------------------

const YANDERE_DELETED: &[u8] = b"<html><head><title>... | #516284 | yande.re</title>\
<meta property=\"og:image\" content=\"https://assets.yande.re/assets/stubs/explicit-caa.png\">\
</head><body>\
This user name doesn't exist. If you want to create a new account, just verify...\
<div>This post was deleted. Reason: . MD5: ffa93b4481cfbaffdee9406bd572d20b</div>\
<div>Size: 4960x3507</div></body></html>";

const YANDERE_LIVE: &[u8] = b"<html><head><title>slyvia | #516285 | yande.re</title>\
<meta property=\"og:image\" content=\"https://files.yande.re/sample/7748/sample.jpg\">\
</head><body>\
This user name doesn't exist. If you want to create a new account, just verify...\
<div>This image has been resized. Click on the View larger version link...</div>\
<div>Size: 2700x4800</div></body></html>";

#[tokio::test]
async fn a_deleted_moebooru_post_is_detected() {
    let w = world(
        &["yande.re"],
        always(Reply::ok(YANDERE_DELETED).header("Content-Type", "text/html")),
    )
    .await;
    assert_eq!(
        w.check("https://yande.re/post/show/516284").await,
        Some(false)
    );
}

#[tokio::test]
async fn a_live_moebooru_post_is_not_condemned_by_the_hidden_login_template() {
    let w = world(
        &["yande.re"],
        always(Reply::ok(YANDERE_LIVE).header("Content-Type", "text/html")),
    )
    .await;
    assert_eq!(
        w.check("https://yande.re/post/show/516285").await,
        Some(true)
    );
}

#[tokio::test]
async fn the_placeholder_image_is_a_second_signal() {
    let text = String::from_utf8(YANDERE_DELETED.to_vec())
        .unwrap()
        .replace("This post was deleted.", "");
    assert!(!text.contains("This post was deleted"));
    let w = world(
        &["yande.re"],
        always(Reply::ok(text).header("Content-Type", "text/html")),
    )
    .await;
    assert_eq!(
        w.check("https://yande.re/post/show/516284").await,
        Some(false)
    );
}

#[test]
fn both_moebooru_signals_are_registered_for_every_host() {
    for url in [
        "https://yande.re/post/show/1",
        "https://konachan.com/post/show/1",
        "https://konachan.net/post/show/1",
    ] {
        let markers = availability::soft_404_markers_for(url);
        assert!(markers.contains(&"this post was deleted"), "{url}");
        assert!(markers.contains(&"/assets/stubs/"), "{url}");
    }
}

#[tokio::test]
async fn a_moebooru_network_failure_is_still_unknown() {
    let w = world(&["yande.re"], always(Reply::aborted())).await;
    assert_eq!(w.check("https://yande.re/post/show/1").await, None);
}

// ---- JoyReactor's full-res fallback (TestReactorFullResFallback) --------------------------

const FULL: &str = "https://img10.reactor.cc/pics/post/full/egoswans-8192103.jpeg";
const PLAIN: &str = "https://img10.reactor.cc/pics/post/egoswans-8192103.jpeg";

#[test]
fn a_confirmed_dead_full_url_falls_back_to_plain() {
    let mut candidate = Candidate::new(FULL);
    let result = availability::reactor_full_fallback(&mut candidate, Some(false));
    assert_eq!(
        result, None,
        "unknown: the fallback URL itself has not been checked"
    );
    assert_eq!(candidate.url, PLAIN);
}

#[test]
fn available_and_unknown_are_left_alone() {
    let mut candidate = Candidate::new(FULL);
    assert_eq!(
        availability::reactor_full_fallback(&mut candidate, Some(true)),
        Some(true)
    );
    assert_eq!(candidate.url, FULL);
    assert_eq!(
        availability::reactor_full_fallback(&mut candidate, None),
        None
    );
    assert_eq!(candidate.url, FULL);
}

#[test]
fn a_dead_non_reactor_or_plain_reactor_candidate_is_reported_dead_as_normal() {
    for url in ["https://danbooru.donmai.us/posts/1", PLAIN] {
        let mut candidate = Candidate::new(url);
        assert_eq!(
            availability::reactor_full_fallback(&mut candidate, Some(false)),
            Some(false)
        );
        assert_eq!(candidate.url, url);
    }
}

// ---- the index bounce (TestIndexRedirectDetection) ----------------------------------------

const LEGACY: &str = "https://chan.sankakucomplex.com/post/show/4100422";

#[test]
fn a_bounce_to_the_index_is_detected() {
    for landing in [
        "https://chan.sankakucomplex.com/en/posts",
        "https://chan.sankakucomplex.com/en/posts/",
        "https://chan.sankakucomplex.com/posts",
        "https://chan.sankakucomplex.com/",
    ] {
        assert!(
            availability::redirect_to_index(LEGACY, Some(landing)).is_some(),
            "{landing}"
        );
    }
}

#[test]
fn a_real_post_is_never_flagged_as_an_index_bounce() {
    assert_eq!(
        availability::redirect_to_index(
            LEGACY,
            Some("https://chan.sankakucomplex.com/en/posts/4100422")
        ),
        None
    );
}

#[test]
fn the_index_rule_is_scoped_to_the_originating_host() {
    assert_eq!(
        availability::redirect_to_index(
            "https://danbooru.donmai.us/posts/1",
            Some("https://danbooru.donmai.us/posts")
        ),
        None
    );
}

#[tokio::test]
async fn an_index_bounce_is_unverifiable_rather_than_gone() {
    let w = world(
        &[SANKAKU],
        bounced_to("https://chan.sankakucomplex.com/en/posts", b""),
    )
    .await;
    assert_eq!(w.page(LEGACY).await, None);
}

#[test]
fn sankaku_and_pixiv_are_checked_on_their_post_page() {
    assert!(availability::has_soft_404_detection(
        "https://chan.sankakucomplex.com/en/posts/x"
    ));
    assert!(availability::has_soft_404_detection(
        "https://www.pixiv.net/artworks/1"
    ));
    assert!(!availability::has_soft_404_detection(
        "https://danbooru.donmai.us/posts/1"
    ));
}

// ---- site headers (TestAvailabilityChecksUseSiteHeaders) ----------------------------------

#[tokio::test]
async fn sankaku_is_checked_with_a_browser_user_agent() {
    let w = world(&[SANKAKU], always(Reply::ok(""))).await;
    w.page("https://chan.sankakucomplex.com/en/posts/3797441")
        .await;
    assert!(
        w.server.recorded()[0]
            .header("user-agent")
            .unwrap()
            .contains(BROWSER_UA)
    );
}

#[tokio::test]
async fn other_sites_keep_the_honest_descriptive_agent() {
    let w = world(&["danbooru.donmai.us"], always(Reply::ok(""))).await;
    w.check("https://danbooru.donmai.us/posts/1").await;
    assert_eq!(
        w.server.recorded()[0].header("user-agent"),
        Some(USER_AGENT)
    );
}

#[tokio::test]
async fn an_explicit_referer_still_wins() {
    let w = world(&[SANKAKU], always(Reply::ok(""))).await;
    availability::page_says_available(
        &w.client,
        Some("https://chan.sankakucomplex.com/en/posts/1"),
        5.0,
        Some("https://example.test/from"),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        w.server.recorded()[0].header("referer"),
        Some("https://example.test/from")
    );
}

#[test]
fn headers_for_url_covers_the_right_sites() {
    for url in [
        "https://chan.sankakucomplex.com/en/posts/1",
        "https://www.pixiv.net/artworks/1",
        "https://www.deviantart.com/artist/art/title-1",
        "https://www.fav.me/d123456",
    ] {
        let headers = site::headers_for_url(url, None).unwrap_or_else(|| panic!("{url}"));
        assert!(
            headers
                .get("user-agent")
                .unwrap()
                .to_str()
                .unwrap()
                .contains(BROWSER_UA),
            "{url}"
        );
    }
    assert!(site::headers_for_url("https://danbooru.donmai.us/posts/1", None).is_none());
    assert!(site::headers_for_url("", None).is_none());
}

#[tokio::test]
async fn deviantart_is_fetched_with_a_browser_user_agent_and_its_own_referer() {
    let w = world(&["www.deviantart.com"], always(Reply::ok(""))).await;
    w.check("https://www.deviantart.com/artist/art/title-1")
        .await;
    let seen = &w.server.recorded()[0];
    assert!(seen.header("user-agent").unwrap().contains(BROWSER_UA));
    assert_eq!(seen.header("referer"), Some("https://www.deviantart.com/"));
}

#[tokio::test]
async fn a_renewed_session_cookie_is_captured_during_an_availability_check() {
    let w = world(
        &["www.deviantart.com"],
        always(Reply::ok("").header(
            "Set-Cookie",
            "auth=new-token; Domain=deviantart.com; Path=/",
        )),
    )
    .await;
    let sandbox = Sandbox::new();
    let mut settings = Settings::with_defaults(&sandbox.paths);
    settings.deviantart_cookies = "auth=old-token; auth_secure=old-secure".into();
    let settings = Mutex::new(settings);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let url = "https://www.deviantart.com/artist/art/title-1";
    let cookies = site::cookies_for_url(url, &remote::credentials(&settings.lock().unwrap()));
    availability::check_url_available(
        &w.client,
        Some(url),
        5.0,
        None,
        None,
        cookies,
        Some(&access),
    )
    .await
    .unwrap();
    assert_eq!(
        settings.lock().unwrap().deviantart_cookies.expose(),
        "auth=new-token; auth_secure=old-secure"
    );
    // The check sent the pasted cookies, and the renewal was saved.
    assert!(
        w.server.recorded()[0]
            .header("cookie")
            .unwrap()
            .contains("auth=old-token")
    );
    assert!(sandbox.paths.config_file().exists());
}

#[tokio::test]
async fn the_no_content_page_is_still_read_as_gone() {
    let w = world(
        &[SANKAKU],
        bounced_to("https://chan.sankakucomplex.com/en/posts/show_empty", b""),
    )
    .await;
    assert_eq!(
        w.check("https://chan.sankakucomplex.com/en/posts/3797441")
            .await,
        Some(false)
    );
}

// ---- the sweep and the cached re-check (search_quality's TestCachedResultsAreRechecked) ---

fn sweep_settings(sandbox: &Sandbox) -> Mutex<Settings> {
    let mut settings = Settings::with_defaults(&sandbox.paths);
    settings.search_timeout = 5.0;
    Mutex::new(settings)
}

#[tokio::test]
async fn a_cached_match_that_has_since_gone_is_dropped() {
    let w = world(&["gelbooru.com", "danbooru.donmai.us"], |req| {
        if req.header("host") == Some("gelbooru.com") {
            Reply::new(404, "")
        } else {
            Reply::ok("")
        }
    })
    .await;
    let sandbox = Sandbox::new();
    let settings = sweep_settings(&sandbox);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let cached = vec![
        Candidate::new("https://gelbooru.com/index.php?page=post&s=view&id=1"),
        Candidate::new("https://danbooru.donmai.us/posts/2"),
    ];
    let kept =
        availability::recheck_cached_candidates(&w.client, cached, &access, "x.png", None).await;
    let urls: Vec<&str> = kept.iter().map(|c| c.url.as_str()).collect();
    assert_eq!(urls, ["https://danbooru.donmai.us/posts/2"]);
}

#[tokio::test]
async fn an_unknown_answer_keeps_the_cached_match() {
    let w = world(&["danbooru.donmai.us"], always(Reply::new(503, ""))).await;
    let sandbox = Sandbox::new();
    let settings = sweep_settings(&sandbox);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let cached = vec![Candidate::new("https://danbooru.donmai.us/posts/2")];
    let kept =
        availability::recheck_cached_candidates(&w.client, cached, &access, "x.png", None).await;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].remote_available, None);
}

#[tokio::test]
async fn the_sweep_marks_every_verdict_and_falls_back_for_a_dead_reactor_full_file() {
    let w = world(
        &["danbooru.donmai.us", "gelbooru.com", "img10.reactor.cc"],
        |req| match req.header("host").unwrap() {
            "gelbooru.com" | "img10.reactor.cc" => Reply::new(410, ""),
            _ => Reply::ok(""),
        },
    )
    .await;
    let sandbox = Sandbox::new();
    let settings = sweep_settings(&sandbox);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let mut candidates = vec![
        Candidate::new("https://danbooru.donmai.us/posts/1"),
        Candidate::new("https://gelbooru.com/index.php?page=post&s=view&id=1"),
        Candidate::new(FULL),
    ];
    availability::sweep_candidate_availability(&w.client, &mut candidates, &access, "x.png", None)
        .await;
    assert_eq!(candidates[0].remote_available, Some(true));
    assert_eq!(candidates[1].remote_available, Some(false));
    assert_eq!(
        candidates[2].url, PLAIN,
        "the full/ file is gone: the plain file is offered"
    );
    assert_eq!(candidates[2].remote_available, None);
    // A verdict is dated; an inconclusive one is not (`record_availability` clears the date).
    assert!(candidates[0].availability_checked_at.is_some());
    assert!(candidates[1].availability_checked_at.is_some());
    assert!(candidates[2].availability_checked_at.is_none());
}

#[tokio::test]
async fn the_sweep_skips_candidates_that_already_have_a_verdict_and_ticks_around_the_work() {
    let w = world(&["danbooru.donmai.us"], always(Reply::ok(""))).await;
    let sandbox = Sandbox::new();
    let settings = sweep_settings(&sandbox);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let mut known = Candidate::new("https://danbooru.donmai.us/posts/1");
    known.remote_available = Some(true);
    let mut candidates = vec![known, Candidate::new("https://danbooru.donmai.us/posts/2")];
    let ticks: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let cb = |label: &str, _r: f64, _t: f64| ticks.lock().unwrap().push(label.to_string());
    availability::sweep_candidate_availability(
        &w.client,
        &mut candidates,
        &access,
        "x.png",
        Some(&cb),
    )
    .await;
    assert_eq!(w.seen(), ["danbooru.donmai.us /posts/2"]);
    assert_eq!(*ticks.lock().unwrap(), ["Checking 1 match(es)", ""]);
}

#[tokio::test]
async fn a_sweep_never_runs_more_than_five_checks_at_once() {
    let live = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (l, p) = (live.clone(), peak.clone());
    let w = world(&["danbooru.donmai.us"], move |_| {
        use std::sync::atomic::Ordering::SeqCst;
        let now = l.fetch_add(1, SeqCst) + 1;
        p.fetch_max(now, SeqCst);
        std::thread::sleep(Duration::from_millis(60));
        l.fetch_sub(1, SeqCst);
        Reply::ok("")
    })
    .await;
    let sandbox = Sandbox::new();
    let settings = sweep_settings(&sandbox);
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let mut candidates: Vec<Candidate> = (1..=12)
        .map(|n| Candidate::new(format!("https://danbooru.donmai.us/posts/{n}")))
        .collect();
    availability::sweep_candidate_availability(&w.client, &mut candidates, &access, "x.png", None)
        .await;
    assert_eq!(w.server.recorded().len(), 12);
    assert!(
        peak.load(std::sync::atomic::Ordering::SeqCst) <= availability::AVAILABILITY_SWEEP_WORKERS
    );
}

// ---- errors Python lets escape --------------------------------------------------------------

#[tokio::test]
async fn a_url_urlparse_refuses_raises_a_value_error_not_an_unknown() {
    let w = world(&["x.test"], always(Reply::ok(""))).await;
    let got = availability::page_says_available(
        &w.client,
        Some("http://[x.test/"),
        5.0,
        None,
        None,
        None,
        None,
    )
    .await;
    match got {
        Err(Raised::ValueError(message)) => assert_eq!(message, "Invalid IPv6 URL"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_definite_verdict_is_dated_and_an_inconclusive_one_clears_the_date() {
    for verdict in [true, false] {
        let mut candidate = Candidate::new("https://danbooru.donmai.us/posts/1");
        candidate.record_availability(Some(verdict), 1_700_000_000.0);
        assert_eq!(candidate.remote_available, Some(verdict));
        assert_eq!(candidate.availability_checked_at, Some(1_700_000_000.0));
    }
    let mut candidate = Candidate::new("https://danbooru.donmai.us/posts/1");
    candidate.record_availability(Some(true), 1_700_000_000.0);
    candidate.record_availability(None, 1_700_000_100.0);
    assert_eq!(candidate.remote_available, None);
    assert_eq!(candidate.availability_checked_at, None);
}

// ---- the manual check of one image's matches (AvailabilityWorker, DAN-2218) ----------------

fn entry_with(urls: &[&str]) -> hatate_core::model::Entry {
    let mut entry = hatate_core::model::Entry::new("/pics/one.png");
    entry.candidates = urls.iter().map(|u| Candidate::new(*u)).collect();
    entry
}

/// Runs the manual check with the sandbox profile's settings.
async fn manual_check(
    w: &World,
    entry: &mut hatate_core::model::Entry,
    stop: &std::sync::atomic::AtomicBool,
) -> (availability::EntryCheck, Vec<(usize, usize)>) {
    let sandbox = Sandbox::new();
    let settings = Mutex::new(Settings::with_defaults(&sandbox.paths));
    let access = SettingsAccess {
        settings: &settings,
        paths: &sandbox.paths,
    };
    let progress = Mutex::new(Vec::new());
    let on_progress = |d: usize, t: usize| progress.lock().unwrap().push((d, t));
    let result =
        availability::check_entry_candidates(&w.client, entry, &access, &on_progress, stop).await;
    (result, progress.into_inner().unwrap())
}

/// `AvailabilityWorker.run`: a 404 is gone, a 200 is available, a 403 stays unknown; each
/// verdict is recorded on its candidate (overwriting what was there), progress counts every
/// check, and the gone count is only the definite ones.
#[tokio::test]
async fn the_manual_check_records_each_verdict_and_counts_only_the_gone() {
    let w = world(&["x.test"], |req: &Recorded| match req.target.as_str() {
        "/gone" => Reply::new(404, ""),
        "/there" => Reply::ok("<html><title>fine</title></html>"),
        _ => Reply::new(403, ""),
    })
    .await;
    let mut entry = entry_with(&[
        "https://x.test/gone",
        "https://x.test/there",
        "https://x.test/blocked",
    ]);
    // A saved verdict is overwritten: this is the user asking again.
    entry.candidates[1].record_availability(Some(false), 1.0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (result, progress) = manual_check(&w, &mut entry, &stop).await;
    assert_eq!((result.gone, result.checked), (1, 3));
    let verdicts: Vec<Option<bool>> = entry
        .candidates
        .iter()
        .map(|c| c.remote_available)
        .collect();
    assert_eq!(verdicts, [Some(false), Some(true), None]);
    let counts: Vec<usize> = progress.iter().map(|p| p.0).collect();
    assert_eq!(counts, [1, 2, 3]);
    assert!(progress.iter().all(|p| p.1 == 3));
}

/// A raised stop flag: nothing is started, nothing is recorded.
#[tokio::test]
async fn a_stopped_manual_check_starts_nothing() {
    let w = world(&["x.test"], always(Reply::new(404, ""))).await;
    let mut entry = entry_with(&["https://x.test/a", "https://x.test/b"]);
    let stop = std::sync::atomic::AtomicBool::new(true);
    let (result, progress) = manual_check(&w, &mut entry, &stop).await;
    assert_eq!((result.gone, result.checked), (0, 0));
    assert!(progress.is_empty());
    assert!(w.seen().is_empty());
    assert!(
        entry
            .candidates
            .iter()
            .all(|c| c.remote_available.is_none())
    );
}

/// `ImageEntry.drop_unavailable_candidates`: only `Some(false)` goes, the selection follows the
/// same match, and an entry with nothing left is a no-match.
#[test]
fn dropping_the_gone_keeps_the_selection_on_the_same_match() {
    let mut entry = entry_with(&["https://a.test/1", "https://a.test/2", "https://a.test/3"]);
    entry.candidates[0].remote_available = Some(false);
    entry.candidates[2].remote_available = None;
    entry.selected_candidate_index = 1;
    assert_eq!(entry.drop_unavailable_candidates(), 1);
    assert_eq!(entry.candidates.len(), 2);
    assert_eq!(entry.selected_candidate_index, 0);
    assert_eq!(entry.selected_candidate().unwrap().url, "https://a.test/2");

    // The selected one is the one that went: back to the best remaining.
    let mut entry = entry_with(&["https://a.test/1", "https://a.test/2"]);
    entry.candidates[1].remote_available = Some(false);
    entry.selected_candidate_index = 1;
    assert_eq!(entry.drop_unavailable_candidates(), 1);
    assert_eq!(entry.selected_candidate().unwrap().url, "https://a.test/1");

    // Everything went.
    let mut entry = entry_with(&["https://a.test/1"]);
    entry.status = hatate_core::model::MatchStatus::Good;
    entry.candidates[0].remote_available = Some(false);
    assert_eq!(entry.drop_unavailable_candidates(), 1);
    assert!(entry.candidates.is_empty());
    assert_eq!(entry.status, hatate_core::model::MatchStatus::NotFound);
    assert_eq!(entry.selected_candidate_index, 0);
    assert_eq!(entry.similarity(), None);
}
