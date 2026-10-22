//! The booru fetch half, the remote fetches, the ranged size read and the Reddit feed GET
//! (DAN-2058, Phase 3.b), through `NetClient::new` (rustls with the explicit provider) against a
//! local https server. Site hosts are reached through `NetConfig::with_host_override`, so the
//! server sees the real `Host` and the real path. No live network is touched.
//!
//! The transport differential against the real Python (`worktrees/DAN-2058-differential.log`) is
//! the broad check; these are the named, readable ones: each request a function makes, in order,
//! and what it does with the answer. The Python test each one maps to is in
//! `docs/python-test-map-3b.md`.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::boorus::dapi::Family;
use hatate_core::boorus::fetch::{self, BooruSession, FetchOptions, SettingsSink};
use hatate_core::boorus::{BooruError, find_parser};
use hatate_core::config::Settings;
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::reddit::{self, FeedCache};
use hatate_core::remote::{self, Raised};

struct World {
    server: TestServer,
    client: NetClient,
}

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
}

/// A served answer by request-target substring, first match wins; anything else is a 404.
fn routes(table: Vec<(&'static str, Reply)>) -> impl Fn(&Recorded) -> Reply + Send + Sync {
    move |req| {
        table
            .iter()
            .find(|(needle, _)| req.target.contains(needle))
            .map_or_else(|| Reply::new(404, "no route"), |(_, reply)| reply.clone())
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out
}

/// A PNG that states `w` x `h` (its pixel data is an empty zlib stream: only the header is read).
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0, 0, 0, 0, 1]));
    out.extend(chunk(b"IEND", &[]));
    out
}

const UA: &str = "hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)";
const DANBOORU_POST: &str = r#"{"id": 5, "image_width": 3, "image_height": 4, "rating": "q",
    "file_url": "https://cdn.donmai.us/f.png", "large_file_url": "https://cdn.donmai.us/l.jpg",
    "tag_string_general": "x y", "tag_string_artist": "someone"}"#;

fn json_reply(body: &str) -> Reply {
    Reply::ok(body).header("Content-Type", "application/json")
}

// -- fetch_page_info -----------------------------------------------------------------------------

/// The verifier's PASS names this test: the whole booru fetch through the binary's own
/// constructor, over https, to a site host, with the headers and the parsed result checked.
#[tokio::test]
async fn fetch_page_info_over_https_through_the_binaries_constructor() {
    let w = world(
        &["danbooru.donmai.us"],
        routes(vec![("/posts/5.json", json_reply(DANBOORU_POST))]),
    )
    .await;
    let session = BooruSession::new();
    let info = fetch::fetch_page_info(
        &w.client,
        &session,
        "https://danbooru.donmai.us/posts/5",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(w.seen(), ["danbooru.donmai.us /posts/5.json"]);
    assert_eq!(w.server.recorded()[0].header("user-agent"), Some(UA));
    assert_eq!(info.tags.len(), 3);
    assert_eq!(
        info.file_url.as_deref(),
        Some("https://cdn.donmai.us/f.png")
    );
    assert_eq!((info.width, info.height), (Some(3), Some(4)));
    assert_eq!(info.rating.as_deref(), Some("questionable"));
    assert_eq!(
        info.fetched_url.as_deref(),
        Some("https://danbooru.donmai.us/posts/5.json")
    );
    assert!(info.fetched);
    let health = &session.health.snapshot()[0];
    assert_eq!((health.site.as_str(), health.ok), ("danbooru", 1));
}

#[tokio::test]
async fn a_url_no_parser_claims_makes_no_request() {
    let w = world(&["example.test"], |_| Reply::ok("x")).await;
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://example.test/x",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert!(!info.fetched);
    assert!(w.seen().is_empty());
}

#[tokio::test]
async fn http_404_and_410_are_the_post_being_gone_and_other_statuses_are_fetch_errors() {
    for (status, gone) in [
        (404, true),
        (410, true),
        (500, false),
        (403, false),
        (204, false),
    ] {
        let w = world(&["danbooru.donmai.us"], move |_| Reply::new(status, "x")).await;
        let err = fetch::fetch_page_info(
            &w.client,
            &BooruSession::new(),
            "https://danbooru.donmai.us/posts/5",
            FetchOptions::default(),
        )
        .await
        .unwrap_err();
        let url = "https://danbooru.donmai.us/posts/5.json";
        match (gone, err) {
            (true, BooruError::ContentGone(m)) => {
                assert_eq!(
                    m,
                    format!("{url} returned HTTP {status} - the post is gone")
                );
            }
            (false, BooruError::Fetch(m)) => assert_eq!(m, format!("{url} returned HTTP {status}")),
            (_, other) => panic!("{status}: {other:?}"),
        }
    }
}

/// An e621 or Anime-Pictures account wall is a status the parser recognises, not a failure.
#[tokio::test]
async fn a_status_the_parser_reads_as_an_account_wall_is_restricted_not_an_error() {
    let w = world(&["e621.net"], |_| Reply::new(401, "{}")).await;
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://e621.net/posts/219935",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert!(info.fetched);
    assert!(info.restricted.is_some());
    assert!(info.tags.is_empty());
}

/// Network error: the fetch is a `BooruError` and the site is counted as failed, not empty.
#[tokio::test]
async fn a_network_error_is_a_booru_error_and_counts_as_a_failure() {
    let w = world(&["danbooru.donmai.us"], |_| Reply::aborted()).await;
    let session = BooruSession::new();
    let err = fetch::fetch_page_info(
        &w.client,
        &session,
        "https://danbooru.donmai.us/posts/5",
        FetchOptions::default(),
    )
    .await
    .unwrap_err();
    let BooruError::Fetch(message) = err else {
        panic!("{err:?}")
    };
    assert!(
        message.starts_with("Could not fetch https://danbooru.donmai.us/posts/5.json: "),
        "{message}"
    );
    let health = &session.health.snapshot()[0];
    assert_eq!((health.failed, health.empty, health.ok), (1, 0, 0));
}

/// Deadline: a body that keeps arriving past `timeout` is the hard deadline, not a plain network
/// error, and it is not counted against the parser.
#[tokio::test]
async fn a_page_that_trickles_past_the_deadline_is_a_hard_timeout() {
    let w = world(&["danbooru.donmai.us"], |_| {
        Reply::ok(vec![b'x'; 60]).trickled(1, Duration::from_millis(60))
    })
    .await;
    let session = BooruSession::new();
    let opts = FetchOptions {
        timeout: 0.5,
        ..FetchOptions::default()
    };
    let err = fetch::fetch_page_info(
        &w.client,
        &session,
        "https://danbooru.donmai.us/posts/5",
        opts,
    )
    .await
    .unwrap_err();
    let BooruError::Fetch(message) = err else {
        panic!("{err:?}")
    };
    assert!(
        message.starts_with(
            "Fetching https://danbooru.donmai.us/posts/5.json timed out: Timed out after 0s"
        ),
        "{message}"
    );
    assert!(
        session.health.snapshot().is_empty(),
        "a deadline is not a parser failure"
    );
}

#[tokio::test]
async fn a_page_with_no_charset_is_read_as_utf8_not_latin1() {
    let w = world(&["anime-pictures.net", "e-shuushuu.net"], |_| {
        Reply::ok("<html>\u{d7}</html>").header("Content-Type", "text/html")
    })
    .await;
    let session = BooruSession::new();
    let info = fetch::fetch_page_info(
        &w.client,
        &session,
        "https://e-shuushuu.net/image/12345/",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(info.body.as_deref(), Some("<html>\u{d7}</html>"));
    // A declared charset is honoured: the same bytes read as Latin-1 give two characters.
    let w = world(&["e-shuushuu.net"], |_| {
        Reply::ok("<html>\u{d7}</html>").header("Content-Type", "text/html; charset=ISO-8859-1")
    })
    .await;
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://e-shuushuu.net/image/12345/",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(info.body.as_deref(), Some("<html>\u{c3}\u{97}</html>"));
}

#[tokio::test]
async fn a_deletion_notice_or_a_redirect_to_the_listing_marks_the_post_gone() {
    let w = world(
        &["gelbooru.com"],
        routes(vec![
            ("dapi", Reply::new(404, "")),
            ("s=view", Reply::ok("<p>This Post Was Deleted.</p>")),
        ]),
    )
    .await;
    let opts = FetchOptions {
        gone_markers: &["was deleted"],
        ..FetchOptions::default()
    };
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://gelbooru.com/index.php?page=post&s=view&id=7",
        opts,
    )
    .await
    .unwrap();
    assert_eq!(info.gone_reason.as_deref(), Some("was deleted"));

    let listing = "https://gelbooru.com/index.php?page=post&s=list&tags=all";
    let w = world(
        &["gelbooru.com"],
        routes(vec![
            ("s=view", Reply::new(302, "").header("Location", listing)),
            ("s=list", Reply::ok("<html></html>")),
        ]),
    )
    .await;
    let redirect = |_url: &str, landed: Option<&str>| {
        landed.filter(|l| l.contains("s=list")).map(str::to_string)
    };
    let opts = FetchOptions {
        gone_redirect: Some(&redirect),
        ..FetchOptions::default()
    };
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://gelbooru.com/index.php?page=post&s=view&id=7",
        opts,
    )
    .await
    .unwrap();
    assert_eq!(
        info.gone_reason.as_deref(),
        Some(format!("redirected to {listing}").as_str())
    );
}

#[tokio::test]
async fn sankaku_is_looked_up_by_the_local_files_md5() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.bin");
    std::fs::write(&file, b"abc").unwrap();
    let w = world(&["sankakuapi.com"], |_| json_reply("[]")).await;
    let session = BooruSession::new();
    let opts = FetchOptions {
        local_path: Some(&file),
        ..FetchOptions::default()
    };
    fetch::fetch_page_info(
        &w.client,
        &session,
        "https://chan.sankakucomplex.com/en/posts/4604859",
        opts,
    )
    .await
    .unwrap();
    assert_eq!(
        w.seen(),
        ["sankakuapi.com /posts?tags=md5%3A900150983cd24fb0d6963f7d28e17f72&limit=1"]
    );
    // The hash is remembered by the file's identity, not asked for twice.
    assert_eq!(session.md5_cache_len(), 1);
    assert_eq!(
        session.local_md5(Some(&file)).as_deref(),
        Some("900150983cd24fb0d6963f7d28e17f72")
    );
    assert_eq!(session.md5_cache_len(), 1);
    // A file that is not there has no hash, and nothing is cached for it.
    assert_eq!(session.local_md5(Some(&dir.path().join("missing"))), None);
    assert_eq!(session.local_md5(None), None);
    assert_eq!(session.md5_cache_len(), 1);
}

#[tokio::test]
async fn a_deviantart_session_cookie_that_rotated_is_saved_and_a_challenge_pages_is_not() {
    let rotated = |status: u16| {
        Reply::new(status, "<html>x</html>")
            .header("Content-Type", "text/html; charset=utf-8")
            .header(
                "Set-Cookie",
                "auth=new; Path=/; Domain=.deviantart.com; Secure",
            )
            .header("Set-Cookie", "other=zz; Path=/")
    };
    for (status, saved) in [(200, true), (403, false)] {
        let w = world(
            &["backend.deviantart.com", "www.deviantart.com"],
            move |_| rotated(status),
        )
        .await;
        let sandbox = Sandbox::new();
        let mut settings = Settings::with_defaults(&sandbox.paths);
        settings.deviantart_cookies = "auth=old; userinfo=u".into();
        let sink = SettingsSink {
            settings: &mut settings,
            paths: &sandbox.paths,
        };
        let opts = FetchOptions {
            settings: Some(sink),
            ..FetchOptions::default()
        };
        let _ = fetch::fetch_page_info(
            &w.client,
            &BooruSession::new(),
            "https://www.deviantart.com/someone/art/title-1234",
            opts,
        )
        .await;
        let want = if saved {
            "auth=new; userinfo=u"
        } else {
            "auth=old; userinfo=u"
        };
        assert_eq!(settings.deviantart_cookies.expose(), want, "{status}");
        assert_eq!(sandbox.paths.config_file().exists(), saved, "{status}");
    }
}

// -- the Gelbooru family's Data API --------------------------------------------------------------

const DAPI_POST: &str = r#"{"post": [{"file_url": "https://img/f.png", "sample_url": "https://img/s.jpg",
    "width": 1152, "height": 1440}]}"#;
const LISTING_PAGE: &str =
    r#"<ul id="tag-list"><li class="tag-type-artist"><a href="x?page=post">somebody</a></li></ul>"#;

#[tokio::test]
async fn the_data_api_is_asked_once_for_the_file_the_preview_and_the_size() {
    let w = world(
        &["gelbooru.com"],
        routes(vec![
            ("page=dapi", json_reply(DAPI_POST)),
            (
                "page=post",
                Reply::ok(LISTING_PAGE).header("Content-Type", "text/html; charset=utf-8"),
            ),
        ]),
    )
    .await;
    let session = BooruSession::new();
    let url = "https://gelbooru.com/index.php?page=post&s=view&id=7";
    let info = fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(
        w.seen(),
        [
            "gelbooru.com /index.php?page=post&s=view&id=7",
            "gelbooru.com /index.php?page=dapi&s=post&q=index&json=1&id=7",
        ]
    );
    assert_eq!(info.file_url.as_deref(), Some("https://img/f.png"));
    assert_eq!(info.preview_url.as_deref(), Some("https://img/s.jpg"));
    assert_eq!((info.width, info.height), (Some(1152), Some(1440)));
    // The same post again: the memo answers, no second API call.
    fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    let api_calls = w.seen().iter().filter(|s| s.contains("dapi")).count();
    assert_eq!(api_calls, 1);
    assert_eq!(
        session.dapi.memo(Family::Gelbooru),
        Some((url.to_string(), true))
    );
}

#[tokio::test]
async fn a_401_latches_gelbooru_and_safebooru_shares_it_but_rule34_and_xbooru_do_not() {
    let w = world(
        &[
            "gelbooru.com",
            "safebooru.org",
            "api.rule34.xxx",
            "rule34.xxx",
            "xbooru.com",
        ],
        |req: &Recorded| {
            let host = req.header("host").unwrap_or("");
            if req.target.contains("page=dapi") {
                // Gelbooru's Data API wants credentials; rule34's and Xbooru's still answer.
                return if host == "gelbooru.com" {
                    Reply::new(401, "{}")
                } else {
                    json_reply(DAPI_POST)
                };
            }
            Reply::ok(LISTING_PAGE).header("Content-Type", "text/html; charset=utf-8")
        },
    )
    .await;
    let session = BooruSession::new();
    let get = |url: &'static str| {
        fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
    };
    get("https://gelbooru.com/index.php?page=post&s=view&id=1")
        .await
        .unwrap();
    assert!(session.dapi.gelbooru_latched());
    let before = w.seen().len();
    // Gelbooru again, another post: page only. Safebooru's file hook is Gelbooru's, so: page only.
    get("https://gelbooru.com/index.php?page=post&s=view&id=2")
        .await
        .unwrap();
    get("https://safebooru.org/index.php?page=post&s=view&id=3")
        .await
        .unwrap();
    assert_eq!(w.seen().len(), before + 2, "{:?}", w.seen());
    // rule34 and Xbooru keep their own: their API is still asked, and still answers.
    let info = get("https://rule34.xxx/index.php?page=post&s=view&id=4")
        .await
        .unwrap();
    assert_eq!(info.file_url.as_deref(), Some("https://img/f.png"));
    get("https://xbooru.com/index.php?page=post&s=view&id=5")
        .await
        .unwrap();
    let seen = w.seen();
    assert!(
        seen.contains(
            &"api.rule34.xxx /index.php?page=dapi&s=post&q=index&json=1&id=4".to_string()
        )
    );
    assert!(
        seen.contains(&"xbooru.com /index.php?page=dapi&s=post&q=index&json=1&id=5".to_string())
    );
    assert_eq!(session.dapi.memo(Family::Rule34).map(|m| m.1), Some(true));
    assert_eq!(session.dapi.memo(Family::Xbooru).map(|m| m.1), Some(true));
}

// -- Pawchive -------------------------------------------------------------------------------------

fn pawchive_post(has_full: bool) -> String {
    let path = format!("/aa/bb/{}.png", "0123456789abcdef".repeat(4));
    format!(
        r#"{{"id": 154701291, "service": "pixiv", "user": "6714576", "tags": "{{Bondage,\"Total Versext\"}}",
        "has_full": {has_full}, "file": {{"path": "{path}", "name": "art.png"}}}}"#
    )
}

#[tokio::test]
async fn pawchive_asks_for_the_creator_once_and_reads_the_originals_size_by_range() {
    let hosts = ["pawchive.pw", "file.pawchive.pw"];
    let w = world(
        &hosts,
        routes(vec![
            ("/profile", json_reply(r#"{"name": "  Some Artist "}"#)),
            ("/data/", Reply::new(206, png(321, 123))),
            ("/api/v1/", json_reply(&pawchive_post(true))),
        ]),
    )
    .await;
    let session = BooruSession::new();
    let url = "https://pawchive.pw/pixiv/user/6714576/post/154701291";
    let info = fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    let seen = w.seen();
    assert_eq!(
        seen[0],
        "pawchive.pw /api/v1/pixiv/user/6714576/post/154701291"
    );
    assert_eq!(seen[1], "pawchive.pw /api/v1/pixiv/user/6714576/profile");
    assert!(
        seen[2].starts_with("file.pawchive.pw /data/aa/bb/"),
        "{seen:?}"
    );
    let ranged = &w.server.recorded()[2];
    assert_eq!(ranged.header("range"), Some("bytes=0-524287"));
    assert_eq!(ranged.header("user-agent"), Some(UA));
    let names: Vec<_> = info
        .tags
        .iter()
        .map(|t| (t.namespace.clone().unwrap(), t.name.clone()))
        .collect();
    assert_eq!(
        names,
        [
            ("general".to_string(), "Bondage".to_string()),
            ("general".to_string(), "Total_Versext".to_string()),
            ("artist".to_string(), "Some_Artist".to_string()),
        ]
    );
    assert_eq!((info.width, info.height), (Some(321), Some(123)));
    // The same creator and the same file again: neither is asked for a second time.
    fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(w.seen().len(), 4, "only the post page: {:?}", w.seen());
    assert_eq!(
        session.artist_cache(),
        [("pixiv/6714576".to_string(), "Some Artist".to_string())]
    );
}

#[tokio::test]
async fn pawchive_makes_no_size_request_for_a_post_the_archive_does_not_hold() {
    let w = world(
        &["pawchive.pw", "file.pawchive.pw"],
        routes(vec![
            ("/profile", json_reply(r#"{"name": "A"}"#)),
            ("/api/v1/", json_reply(&pawchive_post(false))),
        ]),
    )
    .await;
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://pawchive.pw/pixiv/user/6714576/post/154701291",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert!(w.seen().iter().all(|s| !s.starts_with("file.pawchive.pw")));
    assert_eq!((info.width, info.height, info.file_url), (None, None, None));
}

#[tokio::test]
async fn a_failed_creator_lookup_is_remembered_as_a_miss_and_not_asked_again() {
    let w = world(
        &["pawchive.pw", "file.pawchive.pw"],
        routes(vec![
            ("/profile", Reply::aborted()),
            ("/api/v1/", json_reply(&pawchive_post(false))),
        ]),
    )
    .await;
    let session = BooruSession::new();
    let url = "https://pawchive.pw/pixiv/user/6714576/post/154701291";
    let info = fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(info.tags.len(), 2, "the post's own tags, no artist");
    assert_eq!(
        session.artist_cache(),
        [("pixiv/6714576".to_string(), String::new())]
    );
    fetch::fetch_page_info(&w.client, &session, url, FetchOptions::default())
        .await
        .unwrap();
    let profile_calls = w.seen().iter().filter(|s| s.contains("/profile")).count();
    assert_eq!(profile_calls, 1);
}

/// `(resp.json() or {}).get("name")` on a JSON list is an `AttributeError`, out of `parse`.
#[tokio::test]
async fn a_creator_answer_that_is_not_an_object_costs_the_post_all_its_tags() {
    let w = world(
        &["pawchive.pw", "file.pawchive.pw"],
        routes(vec![
            ("/profile", json_reply("[1]")),
            ("/api/v1/", json_reply(&pawchive_post(false))),
        ]),
    )
    .await;
    let session = BooruSession::new();
    let info = fetch::fetch_page_info(
        &w.client,
        &session,
        "https://pawchive.pw/pixiv/user/6714576/post/154701291",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert!(info.tags.is_empty());
    assert!(session.artist_cache().is_empty(), "nothing is remembered");
}

// -- Reddit posts and feeds -------------------------------------------------------------------------

const POST_FEED: &str = concat!(
    r#"<feed><entry><content type="html">&lt;a href=&quot;https://i.redd.it/m42sxjdyus7e1.jpeg&quot;&gt;[link]&lt;/a&gt;</content>"#,
    r#"<id>t3_1hhrx0v</id><media:thumbnail url="https://preview.redd.it/m42sxjdyus7e1.jpeg?width=640" />"#,
    r#"<category term="sub" /><title>A title</title></entry></feed>"#
);

#[tokio::test]
async fn a_reddit_post_reads_its_originals_size_with_reddits_own_user_agent() {
    let w = world(
        &["www.reddit.com", "i.redd.it"],
        routes(vec![
            (
                ".rss",
                Reply::ok(POST_FEED).header("Content-Type", "application/atom+xml; charset=UTF-8"),
            ),
            ("m42sxjdyus7e1", Reply::new(206, png(800, 600))),
        ]),
    )
    .await;
    let info = fetch::fetch_page_info(
        &w.client,
        &BooruSession::new(),
        "https://www.reddit.com/r/sub/comments/1hhrx0v/a_title/",
        FetchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        w.seen(),
        [
            "www.reddit.com /comments/1hhrx0v/.rss",
            "i.redd.it /m42sxjdyus7e1.jpeg"
        ]
    );
    let ranged = &w.server.recorded()[1];
    assert_eq!(ranged.header("user-agent"), Some(reddit::USER_AGENT));
    assert_eq!(ranged.header("range"), Some("bytes=0-524287"));
    assert_eq!((info.width, info.height), (Some(800), Some(600)));
}

#[tokio::test]
async fn a_feed_is_fetched_once_per_session_and_only_a_200_is_remembered() {
    let hits = Arc::new(Mutex::new(0u32));
    let counter = hits.clone();
    let w = world(&["www.reddit.com"], move |_| {
        let mut n = counter.lock().unwrap();
        *n += 1;
        if *n == 1 {
            Reply::new(429, "")
                .header("Retry-After", "0")
                .header("X-Test", "twice")
        } else if *n == 2 {
            Reply::new(429, "")
        } else {
            Reply::ok("<feed><entry>one</entry><entry>two</entry></feed>")
                .header("Content-Type", "application/atom+xml; charset=UTF-8")
        }
    })
    .await;
    let cache = FeedCache::new();
    let get = || reddit::fetch_feed_entries(&w.client, &cache, "Manga", "new/.rss?limit=100", 15.0);
    // 429 on the first try, 429 on the one retry: nothing, and nothing remembered.
    assert_eq!(get().await.unwrap(), None);
    assert!(cache.keys().is_empty());
    let entries = get().await.unwrap().unwrap();
    assert_eq!(entries, ["one", "two"]);
    assert_eq!(cache.keys(), ["manga/new/.rss?limit=100"]);
    // Cached under the lower-cased name: another spelling of the subreddit does not ask again.
    let before = w.server.request_count();
    reddit::fetch_feed_entries(&w.client, &cache, "MANGA", "new/.rss?limit=100", 15.0)
        .await
        .unwrap();
    assert_eq!(w.server.request_count(), before);
    assert_eq!(w.server.recorded()[0].target, "/r/Manga/new/.rss?limit=100");
    assert_eq!(
        w.server.recorded()[0].header("user-agent"),
        Some(reddit::USER_AGENT)
    );
}

#[tokio::test]
async fn find_post_tries_the_newest_feed_then_the_top_and_resolve_listing_falls_back_to_the_image()
{
    let post = "https://www.reddit.com/r/Sub/comments/1vz8vo8/manga_cover/";
    let entry = format!(
        r#"<entry><content>https://preview.redd.it/ogpxyscp4slh1.png</content><link href="{post}" /></entry>"#
    );
    let top = format!("<feed>{entry}</feed>");
    let w = world(
        &["www.reddit.com"],
        routes(vec![
            (
                "/top/",
                Reply::ok(top).header("Content-Type", "application/atom+xml; charset=UTF-8"),
            ),
            (
                "/new/",
                Reply::ok("<feed></feed>")
                    .header("Content-Type", "application/atom+xml; charset=UTF-8"),
            ),
        ]),
    )
    .await;
    let cache = FeedCache::new();
    let got = reddit::resolve_listing_online(
        &w.client,
        &cache,
        "https://www.reddit.com/r/Sub/",
        Some("https://i.redd.it/ogpxyscp4slh1.png"),
        15.0,
    )
    .await
    .unwrap();
    assert_eq!(got.as_deref(), Some(post));
    assert_eq!(
        w.seen().len(),
        2,
        "both feeds, newest first: {:?}",
        w.seen()
    );
    // Not found anywhere: the image's own link, never the whole subreddit.
    let got = reddit::resolve_listing_online(
        &w.client,
        &cache,
        "https://www.reddit.com/r/Sub/",
        Some("https://i.redd.it/zzz999.png"),
        15.0,
    )
    .await
    .unwrap();
    assert_eq!(got.as_deref(), Some("https://i.redd.it/zzz999.png"));
    // No image to find the post by: nothing safe to offer.
    let got = reddit::resolve_listing_online(
        &w.client,
        &cache,
        "https://www.reddit.com/r/Sub/",
        None,
        15.0,
    )
    .await
    .unwrap();
    assert_eq!(got, None);
    // Not a listing: unchanged, and no request.
    let before = w.server.request_count();
    let got =
        reddit::resolve_listing_online(&w.client, &cache, "https://example.com/x", None, 15.0)
            .await
            .unwrap();
    assert_eq!(got.as_deref(), Some("https://example.com/x"));
    assert_eq!(w.server.request_count(), before);
}

/// Network error and deadline for the feed GET: a network error is "no entries" and is not
/// remembered; the hard deadline leaves the function, as `HardTimeoutError` does in Python.
#[tokio::test]
async fn a_feed_network_error_is_nothing_and_a_hard_deadline_is_raised() {
    let w = world(&["www.reddit.com"], |_| Reply::aborted()).await;
    let cache = FeedCache::new();
    let got =
        reddit::fetch_feed_entries(&w.client, &cache, "Sub", "new/.rss?limit=100", 15.0).await;
    assert_eq!(got.unwrap(), None);
    assert!(cache.keys().is_empty());

    let w = world(&["www.reddit.com"], |_| {
        Reply::ok(vec![b'x'; 60]).trickled(1, Duration::from_millis(60))
    })
    .await;
    let err = reddit::fetch_feed_entries(&w.client, &cache, "Sub", "new/.rss?limit=100", 0.5)
        .await
        .unwrap_err();
    let Raised::HardTimeout(message) = err else {
        panic!("{err:?}")
    };
    assert!(message.starts_with("Timed out after 0s"), "{message}");
}

// -- the ranged size read ---------------------------------------------------------------------------

#[tokio::test]
async fn the_ranged_read_stops_at_the_header_and_a_failure_is_not_cached_but_a_miss_is() {
    use hatate_core::boorus::remote_size::RemoteSizes;
    let big = {
        let mut data = png(10, 20);
        data.extend(vec![0u8; 400_000]);
        data
    };
    let body = Arc::new(big);
    let served = body.clone();
    let w = world(&["img.example.test"], move |req| {
        if req.target.contains("miss") {
            Reply::new(404, "nope")
        } else if req.target.contains("down") {
            Reply::aborted()
        } else {
            Reply::new(200, served.as_ref().clone()) // a server that ignores Range
        }
    })
    .await;
    let sizes = RemoteSizes::new();
    let size = |path: &str| {
        let url = format!("https://img.example.test/{path}");
        let sizes = &sizes;
        let client = &w.client;
        async move { sizes.image_size(client, &url, Some("UA/1")).await.unwrap() }
    };
    assert_eq!(size("ok.png").await, (Some(10), Some(20)));
    assert_eq!(size("ok.png").await, (Some(10), Some(20)));
    assert_eq!(w.server.request_count(), 1, "cached by URL");
    assert_eq!(
        w.server.recorded()[0].header("range"),
        Some("bytes=0-524287")
    );
    assert_eq!(w.server.recorded()[0].header("user-agent"), Some("UA/1"));
    // A 404 says the file's start cannot be read: remembered. A dropped connection says nothing
    // about the file: not remembered.
    assert_eq!(size("miss.png").await, (None, None));
    assert_eq!(size("down.png").await, (None, None));
    let cached: Vec<String> = sizes.entries().into_iter().map(|(url, _)| url).collect();
    assert_eq!(
        cached,
        [
            "https://img.example.test/ok.png",
            "https://img.example.test/miss.png"
        ]
    );
}

// -- remote.rs --------------------------------------------------------------------------------------

#[tokio::test]
async fn download_bytes_sends_the_referer_and_gives_the_body_of_a_200_only() {
    let w = world(
        &["img.example.test"],
        routes(vec![
            ("/gone", Reply::new(404, "x")),
            ("/ok", Reply::ok("pixels")),
        ]),
    )
    .await;
    let got = remote::download_bytes(
        &w.client,
        Some("https://img.example.test/ok"),
        5.0,
        Some("https://www.pixiv.net/"),
        None,
        None,
    )
    .await;
    assert_eq!(got.as_deref(), Some(&b"pixels"[..]));
    let first = &w.server.recorded()[0];
    assert_eq!(first.header("referer"), Some("https://www.pixiv.net/"));
    assert_eq!(first.header("user-agent"), Some(UA));
    let got = remote::download_bytes(
        &w.client,
        Some("https://img.example.test/gone"),
        5.0,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(got, None);
    assert_eq!(w.server.recorded()[1].header("referer"), None);
    // A data: URI is decoded where it stands: no request, and an empty one is a failure.
    let before = w.server.request_count();
    let got = remote::download_bytes(
        &w.client,
        Some("data:image/png;base64,aGk="),
        5.0,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(got.as_deref(), Some(&b"hi"[..]));
    assert_eq!(
        remote::download_bytes(&w.client, Some("data:;base64,"), 5.0, None, None, None).await,
        None
    );
    assert_eq!(
        remote::download_bytes(&w.client, None, 5.0, None, None, None).await,
        None
    );
    assert_eq!(w.server.request_count(), before);
}

#[tokio::test]
async fn download_bytes_network_error_and_deadline_are_both_nothing() {
    let w = world(
        &["img.example.test"],
        routes(vec![
            (
                "/trickle",
                Reply::ok(vec![b'x'; 60]).trickled(1, Duration::from_millis(60)),
            ),
            ("/down", Reply::aborted()),
        ]),
    )
    .await;
    let get = |path: &str, timeout| {
        let url = format!("https://img.example.test/{path}");
        let client = w.client.clone();
        async move { remote::download_bytes(&client, Some(&url), timeout, None, None, None).await }
    };
    assert_eq!(get("down", 5.0).await, None);
    assert_eq!(get("trickle", 0.5).await, None);
}

#[tokio::test]
async fn fetch_text_uses_the_sites_headers_and_a_hard_deadline_escapes_it() {
    let w = world(
        &["www.pixiv.net"],
        routes(vec![
            (
                "/trickle",
                Reply::ok(vec![b'x'; 60]).trickled(1, Duration::from_millis(60)),
            ),
            ("/down", Reply::aborted()),
            ("/missing", Reply::new(404, "x")),
            ("/pages", Reply::ok("{}")),
        ]),
    )
    .await;
    let sandbox = Sandbox::new();
    let mut settings = Settings::with_defaults(&sandbox.paths);
    settings.pixiv_session_cookie = " tok ".into();
    let text = remote::fetch_text(
        &w.client,
        "https://www.pixiv.net/ajax/illust/1/pages",
        &settings,
        Some("https://r.test/"),
    )
    .await
    .unwrap();
    assert_eq!(text.as_deref(), Some("{}"));
    let first = &w.server.recorded()[0];
    assert!(
        first
            .header("user-agent")
            .unwrap()
            .starts_with("Mozilla/5.0")
    );
    assert_eq!(first.header("referer"), Some("https://r.test/"));
    assert_eq!(first.header("cookie"), Some("PHPSESSID=tok"));
    for path in ["/missing", "/down"] {
        let url = format!("https://www.pixiv.net{path}");
        assert_eq!(
            remote::fetch_text(&w.client, &url, &settings, None)
                .await
                .unwrap(),
            None,
            "{path}"
        );
    }
    settings.search_timeout = 0.5;
    let err = remote::fetch_text(&w.client, "https://www.pixiv.net/trickle", &settings, None)
        .await
        .unwrap_err();
    assert!(matches!(err, Raised::HardTimeout(_)), "{err:?}");
}

#[tokio::test]
async fn fetch_remote_info_reads_the_head_and_a_failure_leaves_it_blank() {
    let w = world(
        &["img.example.test"],
        routes(vec![
            (
                "/png",
                Reply::new(200, vec![0u8; 1234]).header("Content-Type", "IMAGE/PNG; x=y"),
            ),
            ("/down", Reply::aborted()),
            ("/slow", Reply::ok("").delayed(Duration::from_secs(3))),
            ("/redirect", Reply::new(302, "").header("Location", "/png")),
        ]),
    )
    .await;
    let info = |path: &str, timeout: f64| {
        let url = format!("https://img.example.test{path}");
        let client = w.client.clone();
        async move {
            remote::fetch_remote_info(
                &client,
                Some(&url),
                timeout,
                Some("https://post.test/1"),
                None,
            )
            .await
        }
    };
    assert_eq!(
        info("/png", 5.0).await.unwrap(),
        (Some("PNG".to_string()), Some(1234))
    );
    let head = &w.server.recorded()[0];
    assert_eq!(head.method, "HEAD");
    assert_eq!(head.header("referer"), Some("https://post.test/1"));
    // allow_redirects=True: the HEAD follows.
    assert_eq!(
        info("/redirect", 5.0).await.unwrap(),
        (Some("PNG".to_string()), Some(1234))
    );
    assert_eq!(info("/down", 5.0).await.unwrap(), (None, None));
    assert_eq!(info("/slow", 0.4).await.unwrap(), (None, None));
}

/// Names the Python function that this ticket's test of the remote half stands in for:
/// `fetch_remote_info(url=None)` and an empty URL make no request.
#[tokio::test]
async fn an_absent_url_makes_no_request_anywhere() {
    let w = world(&["img.example.test"], |_| Reply::ok("x")).await;
    assert_eq!(
        remote::fetch_remote_info(&w.client, None, 5.0, None, None)
            .await
            .unwrap(),
        (None, None)
    );
    assert_eq!(
        remote::fetch_remote_info(&w.client, Some(""), 5.0, None, None)
            .await
            .unwrap(),
        (None, None)
    );
    let sandbox = Sandbox::new();
    let settings = Settings::with_defaults(&sandbox.paths);
    assert_eq!(
        remote::fetch_text(&w.client, "", &settings, None)
            .await
            .unwrap(),
        None
    );
    assert_eq!(w.server.request_count(), 0);
    assert!(find_parser("https://example.test/").is_none());
}
