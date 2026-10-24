//! Lens recovery's two site lookups (DAN-2165, Phase 5.c): `boorus.rule34.find_post_by_search`
//! and `reddit.find_post` behind `LensSites`, through `NetClient::new` (rustls with the explicit
//! provider) against a local https server. Site hosts are reached through
//! `NetConfig::with_host_override`, so the server sees the real `Host` and the real path. No live
//! network is touched.
//!
//! The transport differential against the real Python (`worktrees/DAN-2165-differential.log`) is
//! the broad check; these are the named, readable ones. The Python test each one maps to is in
//! `docs/lens-search.md` section 5.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, Reply, TestPki, TestServer};
use hatate_core::boorus::rule34;
use hatate_core::engines::lens_rank::{LensSites, Unexpected};
use hatate_core::engines::lens_sites::OnlineSites;
use hatate_core::image_compare::{Hash, Source, dhash};
use hatate_core::net::{NetClient, NetConfig};

const HOSTS: [&str; 4] = [
    "rule34.xxx",
    "wimg.rule34.xxx",
    "www.reddit.com",
    "img.rule34.xxx",
];
const UA: &str = "hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)";

struct World {
    server: TestServer,
    client: NetClient,
}

type Handler = Box<dyn Fn(&Recorded) -> Reply + Send + Sync>;

async fn world(handler: Handler) -> World {
    let pki = TestPki::generate_for(&HOSTS);
    let server = TestServer::https(&pki, handler).await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in HOSTS {
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

/// A picture with structure, unique per seed.
fn picture(seed: u64) -> image::RgbImage {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    let mut small = image::RgbImage::new(10, 12);
    for p in small.pixels_mut() {
        *p = image::Rgb([next(), next(), next()]);
    }
    image::imageops::resize(&small, 120, 90, image::imageops::FilterType::CatmullRom)
}

fn png(img: &image::RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(img: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(img)
        .unwrap();
    out
}

fn local_hash(img: &image::RgbImage) -> Hash {
    dhash(Source::Bytes(&png(img)), rule34::LISTING_HASH_SIZE)
        .unwrap()
        .unwrap()
}

/// A listing page of `(post id, thumbnail path)` posts, thumbnails on `wimg.rule34.xxx`.
fn listing(posts: &[(&str, &str)]) -> String {
    let mut html = String::from("<html><body>");
    for (id, thumb) in posts {
        html.push_str(&format!(
            "<span class=\"thumb\"><a id=\"p{id}\" href=\"/index.php?page=post&s=view&id={id}\">\
             <img src=\"https://wimg.rule34.xxx{thumb}\" alt=\"x\"/></a></span>"
        ));
    }
    html.push_str("</body></html>");
    html
}

/// A server whose listing is `listing_reply` and whose thumbnails are looked up by path.
fn site(listing_reply: Reply, thumbs: Vec<(&'static str, Reply)>) -> Handler {
    Box::new(move |req| {
        if req.target.starts_with("/index.php?page=post&s=list") {
            return listing_reply.clone();
        }
        thumbs
            .iter()
            .find(|(path, _)| req.target == *path)
            .map_or_else(|| Reply::new(404, "no route"), |(_, reply)| reply.clone())
    })
}

fn html(body: String) -> Reply {
    Reply::ok(body).header("Content-Type", "text/html; charset=UTF-8")
}

/// The verifier's PASS names this test: the listing GET and the thumbnail GETs through the
/// binary's own constructor, over https, to a site host, with the headers, the request order and
/// the answer checked.
#[tokio::test]
async fn the_closest_post_within_the_limit_is_found_over_https() {
    let original = picture(7);
    let w = world(site(
        html(listing(&[
            ("111", "/t/a.jpg?1"),
            ("222", "/t/b.jpg?2"),
            ("333", "/t/c.jpg?3"),
        ])),
        vec![
            // An edit of the same scene, the same scene, and a different picture.
            ("/t/a.jpg?1", Reply::ok(jpeg(&picture(99), 80))),
            ("/t/b.jpg?2", Reply::ok(jpeg(&original, 85))),
            ("/t/c.jpg?3", Reply::ok(jpeg(&picture(98), 80))),
        ],
    ))
    .await;
    let found = rule34::find_post_by_search(
        &w.client,
        "parent:10755759",
        Some(&local_hash(&original)),
        15.0,
    )
    .await
    .unwrap();
    let (url, thumb, distance) = found.expect("the scene is on the page");
    assert_eq!(url, "https://rule34.xxx/index.php?page=post&s=view&id=222");
    assert_eq!(thumb, "https://wimg.rule34.xxx/t/b.jpg?2");
    assert!(distance <= rule34::LISTING_MAX_DISTANCE, "{distance}");

    let seen = w.seen();
    assert_eq!(
        seen[0],
        "rule34.xxx /index.php?page=post&s=list&tags=parent%3A10755759"
    );
    let mut thumbs: Vec<_> = seen[1..].to_vec();
    thumbs.sort();
    assert_eq!(
        thumbs,
        [
            "wimg.rule34.xxx /t/a.jpg?1",
            "wimg.rule34.xxx /t/b.jpg?2",
            "wimg.rule34.xxx /t/c.jpg?3"
        ]
    );
    for request in w.server.recorded() {
        assert_eq!(request.header("user-agent"), Some(UA));
    }
}

#[tokio::test]
async fn the_nearest_post_past_the_limit_is_not_a_match() {
    let original = picture(7);
    let w = world(site(
        html(listing(&[("1", "/t/a.jpg"), ("2", "/t/b.jpg")])),
        vec![
            ("/t/a.jpg", Reply::ok(jpeg(&picture(50), 80))),
            ("/t/b.jpg", Reply::ok(jpeg(&picture(51), 80))),
        ],
    ))
    .await;
    let found = rule34::find_post_by_search(&w.client, "x", Some(&local_hash(&original)), 15.0)
        .await
        .unwrap();
    assert_eq!(found, None);
    assert_eq!(w.seen().len(), 3, "the thumbnails were all read first");
}

#[tokio::test]
async fn an_empty_listing_costs_one_request() {
    let w = world(site(
        html("<html><body>Nothing found</body></html>".to_owned()),
        vec![],
    ))
    .await;
    let hash = local_hash(&picture(1));
    assert_eq!(
        rule34::find_post_by_search(&w.client, "tag", Some(&hash), 15.0)
            .await
            .unwrap(),
        None
    );
    assert_eq!(w.seen().len(), 1);
}

#[tokio::test]
async fn a_listing_that_answers_with_an_error_status_finds_nothing() {
    let hash = local_hash(&picture(1));
    for status in [404, 500, 403] {
        let w = world(site(
            Reply::new(status, listing(&[("1", "/t/a.jpg")])),
            vec![("/t/a.jpg", Reply::ok(jpeg(&picture(1), 80)))],
        ))
        .await;
        assert_eq!(
            rule34::find_post_by_search(&w.client, "tag", Some(&hash), 15.0)
                .await
                .unwrap(),
            None,
            "{status}"
        );
        assert_eq!(w.seen().len(), 1, "{status}: no thumbnail is fetched");
    }
}

#[tokio::test]
async fn a_dropped_connection_or_a_slow_listing_finds_nothing() {
    let hash = local_hash(&picture(1));
    let w = world(site(Reply::aborted(), vec![])).await;
    assert_eq!(
        rule34::find_post_by_search(&w.client, "tag", Some(&hash), 15.0)
            .await
            .unwrap(),
        None
    );
    // The timeout is the caller's: one second here, 15 in the shipped lookup.
    let slow = html(listing(&[("1", "/t/a.jpg")])).delayed(Duration::from_secs(3));
    let w = world(site(slow, vec![])).await;
    let started = std::time::Instant::now();
    assert_eq!(
        rule34::find_post_by_search(&w.client, "tag", Some(&hash), 1.0)
            .await
            .unwrap(),
        None
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn a_thumbnail_that_cannot_be_read_is_left_out_not_fatal() {
    let original = picture(7);
    let w = world(site(
        html(listing(&[
            ("1", "/t/gone.jpg"),
            ("2", "/t/junk.jpg"),
            ("3", "/t/ok.jpg"),
            ("4", "/t/drop.jpg"),
        ])),
        vec![
            ("/t/gone.jpg", Reply::new(404, png(&original))),
            ("/t/junk.jpg", Reply::ok("not a picture")),
            ("/t/ok.jpg", Reply::ok(jpeg(&picture(60), 80))),
            ("/t/drop.jpg", Reply::aborted()),
        ],
    ))
    .await;
    let found = rule34::find_post_by_search(&w.client, "tag", Some(&local_hash(&original)), 15.0)
        .await
        .unwrap();
    // A 404 with a picture in its body is still read: `.content` does not look at the status.
    let (url, _, distance) = found.unwrap();
    assert_eq!(distance, 0);
    assert!(url.ends_with("id=1"), "{url}");
}

#[tokio::test]
async fn a_decompression_bomb_fails_the_lookup_and_the_other_thumbnails_were_still_asked_for() {
    // A PNG that states 30000 x 30000.
    let mut bomb = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = 30000u32.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&30000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        let mut crc = 0xffff_ffffu32;
        for byte in &body {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        out.extend_from_slice(&(!crc).to_be_bytes());
        out
    };
    bomb.extend(chunk(b"IHDR", &ihdr));
    bomb.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0, 0, 0, 0, 1]));
    bomb.extend(chunk(b"IEND", &[]));
    let w = world(site(
        html(listing(&[("1", "/t/bomb.png"), ("2", "/t/a.jpg")])),
        vec![
            ("/t/bomb.png", Reply::ok(bomb)),
            ("/t/a.jpg", Reply::ok(jpeg(&picture(1), 80))),
        ],
    ))
    .await;
    let hash = local_hash(&picture(1));
    let err = rule34::find_post_by_search(&w.client, "tag", Some(&hash), 15.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("decompression bomb"), "{err}");
    assert_eq!(
        w.seen().len(),
        3,
        "Python's pool finishes every thumbnail first"
    );
}

#[tokio::test]
async fn nothing_is_asked_without_a_hash_or_without_tags() {
    let w = world(site(html(listing(&[("1", "/t/a.jpg")])), vec![])).await;
    let hash = local_hash(&picture(1));
    assert_eq!(
        rule34::find_post_by_search(&w.client, "tag", None, 15.0)
            .await
            .unwrap(),
        None
    );
    for tags in ["", "   ", "\t\n"] {
        assert_eq!(
            rule34::find_post_by_search(&w.client, tags, Some(&hash), 15.0)
                .await
                .unwrap(),
            None
        );
    }
    assert_eq!(w.seen().len(), 0);
}

#[tokio::test]
async fn the_search_is_stripped_and_written_as_a_query_string() {
    let w = world(site(html(listing(&[])), vec![])).await;
    let hash = local_hash(&picture(1));
    rule34::find_post_by_search(&w.client, "  a&b=c#d+e%f tâg ", Some(&hash), 15.0)
        .await
        .unwrap();
    assert_eq!(
        w.seen(),
        ["rule34.xxx /index.php?page=post&s=list&tags=a%26b%3Dc%23d%2Be%25f+t%C3%A2g"]
    );
}

// -- Reddit, through `OnlineSites` -------------------------------------------------------------

fn feed(entries: &[(&str, bool)]) -> String {
    let mut out = String::from("<feed>");
    for (media, with_link) in entries {
        let link = if *with_link {
            format!("<link href=\"https://www.reddit.com/r/Pics/comments/{media}/title/\"/>")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "<entry>{link}<content>&lt;img src=\"https://i.redd.it/{media}.jpg\"&gt;</content></entry>"
        ));
    }
    out.push_str("</feed>");
    out
}

/// `OnlineSites` runs its lookups from a blocking thread, as `search` does.
async fn reddit_lookup(
    sites: Arc<OnlineSites>,
    sub: &str,
    media: &str,
) -> Result<Option<String>, Unexpected> {
    let (sub, media) = (sub.to_owned(), media.to_owned());
    tokio::task::spawn_blocking(move || sites.find_reddit_post(&sub, &media))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_found_post_a_missing_post_and_a_removed_post() {
    let seen_feeds: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&seen_feeds);
    let w = world(Box::new(move |req| {
        log.lock().unwrap().push(req.target.clone());
        let body = if req.target.starts_with("/r/Pics/new/") {
            // `removed1` is named by an entry with no link to its post (a removed post).
            feed(&[("other1", true), ("removed1", false)])
        } else {
            feed(&[("top1", true), ("removed1", false)])
        };
        Reply::ok(body).header("Content-Type", "application/atom+xml")
    }))
    .await;
    let sites = Arc::new(OnlineSites::new(w.client.clone()));
    // Found: the second feed names it.
    assert_eq!(
        reddit_lookup(sites.clone(), "Pics", "top1")
            .await
            .unwrap()
            .as_deref(),
        Some("https://www.reddit.com/r/Pics/comments/top1/title/")
    );
    // Missing: neither feed names it.
    assert_eq!(
        reddit_lookup(sites.clone(), "Pics", "nothere")
            .await
            .unwrap(),
        None
    );
    // Removed: both feeds name it, neither links to the post.
    assert_eq!(
        reddit_lookup(sites.clone(), "Pics", "removed1")
            .await
            .unwrap(),
        None
    );
    // The feeds are read once per session.
    let asked = seen_feeds.lock().unwrap().clone();
    assert_eq!(
        asked,
        [
            "/r/Pics/new/.rss?limit=100",
            "/r/Pics/top/.rss?t=all&limit=100"
        ]
    );
    for request in w.server.recorded() {
        assert_eq!(
            request.header("user-agent"),
            Some("linux:hatate-linux:1.0 (reverse image tagger)")
        );
    }
}

#[tokio::test]
async fn a_feed_that_fails_counts_as_empty_and_is_asked_again_next_time() {
    let calls = Arc::new(Mutex::new(0usize));
    let counted = Arc::clone(&calls);
    let w = world(Box::new(move |_| {
        *counted.lock().unwrap() += 1;
        Reply::new(500, "down")
    }))
    .await;
    let sites = Arc::new(OnlineSites::new(w.client.clone()));
    assert_eq!(
        reddit_lookup(sites.clone(), "Pics", "abc").await.unwrap(),
        None
    );
    assert_eq!(*calls.lock().unwrap(), 2);
    assert_eq!(reddit_lookup(sites, "Pics", "abc").await.unwrap(), None);
    assert_eq!(*calls.lock().unwrap(), 4, "a failure is not remembered");
}

#[tokio::test]
async fn the_rule34_lookup_through_the_seam_hands_back_the_thumbnail_as_an_option() {
    let original = picture(3);
    let w = world(site(
        html(listing(&[("5", "/t/a.jpg")])),
        vec![("/t/a.jpg", Reply::ok(png(&original)))],
    ))
    .await;
    let sites = Arc::new(OnlineSites::new(w.client.clone()));
    let hash = local_hash(&original);
    let found =
        tokio::task::spawn_blocking(move || sites.find_rule34_post("parent:5", Some(&hash)))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        found,
        Some((
            "https://rule34.xxx/index.php?page=post&s=view&id=5".to_owned(),
            Some("https://wimg.rule34.xxx/t/a.jpg".to_owned()),
            0
        ))
    );
}
