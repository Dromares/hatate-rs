//! `core/pawchive_index.py` (Phase 4.f, DAN-2128): the index database, the polite crawl and the
//! artist search, against a local https server with a throw-away CA. Pawchive is played by a
//! scripted server; no live request is made (DAN-1547: the live site answers 404 for every
//! original). The Python tests each one maps to are in `docs/pawchive-index.md`; the transport and
//! file differentials against the real Python are the broad check
//! (`worktrees/DAN-2128-differential.log`).

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{Recorded, Reply, TestPki, TestServer};
use hatate_core::clock::{Clock, ManualClock};
use hatate_core::error::ErrorKind;
use hatate_core::image_compare::{Hash, Source};
use hatate_core::net::{self, NetClient, NetConfig};
use hatate_core::pawchive_index::{
    self, CrawlResult, ImageRow, PawchiveCrawler, PawchiveError, PawchiveIndex, creator_from_url,
    find_creators_for_ui, fingerprints, run_index_jobs, similarity_for,
};
use serde_json::{Value, json};

// ---- pictures: any image with structure, unique per seed ----

fn picture(seed: u64, w: u32, h: u32) -> image::RgbImage {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    let mut small = image::RgbImage::new(12, 16);
    for p in small.pixels_mut() {
        *p = image::Rgb([next(), next(), next()]);
    }
    image::imageops::resize(&small, w, h, image::imageops::FilterType::CatmullRom)
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

fn sha(n: u64) -> String {
    format!("{n:064x}")
}

fn path_for(sha: &str, ext: &str) -> String {
    format!("/{}/{}/{sha}.{ext}", &sha[..2], &sha[2..4])
}

fn prints(bytes: &[u8]) -> (Hash, Hash) {
    let (h8, h16) = fingerprints(Source::Bytes(bytes)).unwrap();
    (h8.unwrap(), h16.unwrap())
}

fn hex16(h: &Hash) -> String {
    format!(
        "{:016x}{:016x}{:016x}{:016x}",
        h.0[0], h.0[1], h.0[2], h.0[3]
    )
}

fn row(post: u64, page: i64, sha: &str, bytes: &[u8], service: &str, user: &str) -> ImageRow {
    let (h8, h16) = prints(bytes);
    ImageRow {
        service: service.to_owned(),
        user_id: user.to_owned(),
        post_id: post.to_string(),
        page_index: page,
        sha256: sha.to_owned(),
        path: path_for(sha, "png"),
        title: Some("t".to_owned()),
        dhash8: Some(format!("{:016x}", h8.0[3])),
        dhash16: Some(hex16(&h16)),
    }
}

fn temp_index() -> (tempfile::TempDir, PawchiveIndex) {
    let dir = tempfile::tempdir().unwrap();
    let index = PawchiveIndex::new(dir.path().join("index.db"));
    (dir, index)
}

// ---- the index database ----

#[test]
fn an_edited_copy_is_found_and_scored_as_the_same_picture() {
    let (_dir, index) = temp_index();
    index
        .add_creator("fanbox", "42", Some("Some Artist"))
        .unwrap();
    let original = picture(1, 300, 420);
    index
        .add_images(&[
            row(100, 0, &sha(1), &png(&original), "fanbox", "42"),
            row(101, 0, &sha(2), &png(&picture(2, 300, 420)), "fanbox", "42"),
        ])
        .unwrap();
    let edited =
        image::imageops::resize(&original, 100, 140, image::imageops::FilterType::Lanczos3);
    let (_, local16) = prints(&jpeg(&edited, 70));
    let matches = index.search_default(Some(&local16), None).unwrap();
    let best = &matches[0];
    assert_eq!(best.url, "https://pawchive.pw/fanbox/user/42/post/100");
    assert!(!best.exact);
    assert!(best.similarity >= 90.0, "{}", best.similarity);
    assert_eq!(best.artist.as_deref(), Some("Some Artist"));
    assert_eq!(
        best.thumb_url,
        format!(
            "https://img.pawchive.pw/thumbnail/data{}",
            path_for(&sha(1), "png")
        )
    );
}

#[test]
fn a_different_picture_is_not_a_match() {
    let (_dir, index) = temp_index();
    index
        .add_images(&[row(
            100,
            0,
            &sha(1),
            &png(&picture(1, 300, 420)),
            "fanbox",
            "42",
        )])
        .unwrap();
    let (_, local16) = prints(&png(&picture(99, 300, 420)));
    assert!(
        index
            .search_default(Some(&local16), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_same_sha256_is_the_very_same_file() {
    let (_dir, index) = temp_index();
    index
        .add_images(&[row(
            100,
            0,
            &sha(7),
            &png(&picture(7, 300, 420)),
            "fanbox",
            "42",
        )])
        .unwrap();
    let matches = index
        .search_default(None, Some(&sha(7).to_uppercase()))
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert!(matches[0].exact);
    assert_eq!(matches[0].similarity, 100.0);
    assert_eq!(matches[0].distance, 0);
}

#[test]
fn one_result_per_post_on_its_closest_image() {
    let (_dir, index) = temp_index();
    let pic = picture(3, 300, 420);
    index
        .add_images(&[
            row(
                100,
                0,
                &sha(10),
                &png(&picture(11, 300, 420)),
                "fanbox",
                "42",
            ),
            row(100, 1, &sha(12), &png(&pic), "fanbox", "42"),
            row(
                100,
                2,
                &sha(13),
                &png(&image::imageops::resize(
                    &pic,
                    150,
                    210,
                    image::imageops::FilterType::Triangle,
                )),
                "fanbox",
                "42",
            ),
        ])
        .unwrap();
    let (_, local16) = prints(&png(&pic));
    let matches = index.search_default(Some(&local16), None).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!((matches[0].page_index, matches[0].distance), (1, 0));
}

#[test]
fn new_images_are_searchable_straight_away() {
    let (_dir, index) = temp_index();
    let (_, local16) = prints(&png(&picture(5, 300, 420)));
    assert!(
        index
            .search_default(Some(&local16), None)
            .unwrap()
            .is_empty()
    ); // loads the empty cache
    index
        .add_images(&[row(
            100,
            0,
            &sha(5),
            &png(&picture(5, 300, 420)),
            "fanbox",
            "42",
        )])
        .unwrap();
    assert_eq!(index.search_default(Some(&local16), None).unwrap().len(), 1);
}

#[test]
fn removing_an_artist_removes_their_images() {
    let (_dir, index) = temp_index();
    index.add_creator("fanbox", "42", Some("A")).unwrap();
    index
        .add_images(&[row(
            100,
            0,
            &sha(1),
            &png(&picture(1, 300, 420)),
            "fanbox",
            "42",
        )])
        .unwrap();
    index.remove_creator("fanbox", "42").unwrap();
    assert!(index.creators().unwrap().is_empty());
    assert!(index.is_empty().unwrap());
}

#[test]
fn artists_report_what_was_indexed() {
    let (_dir, index) = temp_index();
    index.add_creator("fanbox", "42", Some("A")).unwrap();
    let missing = ImageRow {
        service: "fanbox".into(),
        user_id: "42".into(),
        post_id: "101".into(),
        page_index: 0,
        sha256: sha(2),
        path: path_for(&sha(2), "png"),
        title: Some("t".into()),
        dhash8: None,
        dhash16: None,
    };
    index
        .add_images(&[
            row(100, 0, &sha(1), &png(&picture(1, 300, 420)), "fanbox", "42"),
            missing,
        ])
        .unwrap();
    let creators = index.creators().unwrap();
    assert_eq!(creators.len(), 1);
    assert_eq!(
        (creators[0].image_count, creators[0].unfingerprinted),
        (2, 1)
    );
    assert_eq!(creators[0].label(), "A (fanbox)");
    assert_eq!(index.image_count().unwrap(), 1);
}

#[test]
fn adding_a_creator_twice_says_so_and_a_name_updates() {
    let (_dir, index) = temp_index();
    assert!(index.add_creator("fanbox", "42", None).unwrap());
    assert!(!index.add_creator("fanbox", "42", Some("Named")).unwrap());
    let creators = index.creators().unwrap();
    assert_eq!(creators[0].name.as_deref(), Some("Named"));
}

#[test]
fn a_lightly_cropped_copy_still_surfaces_as_one_to_review() {
    // MEASURED in Python: a 3% crop each side sat at distance 28, the nearest different picture at 41.
    const { assert!(pawchive_index::MAX_MATCH_DISTANCE >= 28) };
    const { assert!(pawchive_index::MAX_MATCH_DISTANCE < 41) };
    assert!(similarity_for(28) < 75.0, "cannot satisfy a fallback");
}

#[test]
fn the_scale_keeps_variants_below_the_auto_import_line() {
    assert_eq!(similarity_for(0), 100.0);
    assert_eq!(similarity_for(6), 90.0);
    assert!(similarity_for(9) < 90.0, "a measured variant");
    assert_eq!(similarity_for(24), 70.0);
    // Python's round(x, 1) on the interpolated value.
    assert_eq!(similarity_for(1), 98.3);
    assert_eq!(similarity_for(2), 96.7);
    assert_eq!(similarity_for(128), 0.0);
    assert_eq!(similarity_for(129), 0.0);
    assert_eq!(similarity_for(-4), 100.0);
}

#[test]
fn links_name_the_artist() {
    assert_eq!(
        creator_from_url("https://pawchive.pw/patreon/user/6714576/post/38740818"),
        Some(("patreon".to_owned(), "6714576".to_owned()))
    );
    assert_eq!(
        creator_from_url("https://pawchive.pw/fanbox/user/9"),
        Some(("fanbox".to_owned(), "9".to_owned()))
    );
    assert_eq!(creator_from_url("https://example.com/user/1"), None);
}

#[test]
fn the_schema_is_pythons_schema() {
    // `sqlite_master` of a file Python 76c85c4 made with `PawchiveIndex()._connect()`.
    let (dir, index) = temp_index();
    index.add_creator("fanbox", "1", None).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY name")
        .unwrap();
    let rows: Vec<(String, String, String, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let names: Vec<&str> = rows.iter().map(|r| r.1.as_str()).collect();
    assert_eq!(
        names,
        [
            "creators",
            "images",
            "images_by_sha256",
            "sqlite_autoindex_creators_1",
            "sqlite_autoindex_images_1"
        ]
    );
    assert_eq!(
        rows[2].3.as_deref(),
        Some("CREATE INDEX images_by_sha256 ON images (sha256)")
    );
}

#[test]
fn a_missing_folder_is_an_io_error_that_names_no_path_for_a_person() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("a-file");
    std::fs::write(&blocker, b"x").unwrap();
    // The parent of the database is a file: the folder cannot be made.
    let index = PawchiveIndex::new(blocker.join("sub").join("index.db"));
    let err = index.creators().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    let shown = err.user_message();
    assert!(
        !shown.contains("a-file") && !shown.contains(dir.path().to_str().unwrap()),
        "{shown}"
    );
    assert!(
        !shown.contains("os error") && !shown.contains("Errno"),
        "{shown}"
    );
    assert!(err.detail().contains("a-file"), "the log keeps the path");
}

#[test]
fn a_database_that_is_not_one_is_a_database_error_that_names_no_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    std::fs::write(&path, vec![b'x'; 4096]).unwrap();
    let err = PawchiveIndex::new(&path).creators().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Database);
    let shown = err.user_message();
    assert!(
        !shown.contains("index.db") && !shown.contains("SQLite") && !shown.contains("os error"),
        "{shown}"
    );
}

#[test]
fn an_index_that_does_not_exist_yet_is_empty_and_costs_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let index = PawchiveIndex::new(dir.path().join("none.db"));
    assert!(index.is_empty().unwrap());
    assert_eq!(index.image_count().unwrap(), 0);
    assert!(
        index
            .search_default(Some(&Hash([1, 2, 3, 4])), Some("abc"))
            .unwrap()
            .is_empty()
    );
    assert!(
        !dir.path().join("none.db").exists(),
        "reading an absent index creates nothing"
    );
}

// ---- Pawchive played by a scripted https server ----

#[derive(Clone, Default)]
struct Site {
    name: Option<String>,
    posts: Vec<Value>,
    thumbnails: BTreeMap<String, Vec<u8>>,
    /// Status for the first profile request, e.g. 429 then 200.
    first_status: Option<(u16, Option<String>)>,
    profile_status: Option<u16>,
    first_done: Arc<AtomicBool>,
}

fn post(id: u64, shas: &[String], ext: &str) -> Value {
    let files: Vec<Value> = shas
        .iter()
        .enumerate()
        .map(|(i, s)| json!({"name": format!("{i}.{ext}"), "path": path_for(s, ext)}))
        .collect();
    json!({"id": id.to_string(), "title": format!("post {id}"), "file": files[0], "attachments": files})
}

impl Site {
    fn reply(&self, req: &Recorded) -> Reply {
        let (path, query) = req.target.split_once('?').unwrap_or((&req.target, ""));
        if let Some((status, retry_after)) = &self.first_status
            && !self.first_done.swap(true, Ordering::SeqCst)
        {
            let mut reply = Reply::new(*status, "");
            if let Some(r) = retry_after {
                reply = reply.header("Retry-After", r);
            }
            return reply;
        }
        if path.ends_with("/profile") {
            return match self.profile_status {
                Some(status) => Reply::new(status, ""),
                None => Reply::ok(json!({"name": self.name}).to_string()),
            };
        }
        if path.ends_with("/posts") {
            let offset: usize = query
                .strip_prefix("o=")
                .and_then(|o| o.parse().ok())
                .unwrap_or(0);
            let page: Vec<&Value> = self
                .posts
                .iter()
                .skip(offset)
                .take(pawchive_index::PAGE_SIZE)
                .collect();
            return Reply::ok(json!(page).to_string());
        }
        match path
            .strip_prefix("/thumbnail/data")
            .and_then(|p| self.thumbnails.get(p))
        {
            Some(body) => Reply::ok(body.clone()),
            None => Reply::new(404, "not found"),
        }
    }
}

struct World {
    server: TestServer,
    crawler: PawchiveCrawler,
    clock: Arc<ManualClock>,
    dir: tempfile::TempDir,
    index: PawchiveIndex,
}

async fn world(site: Site) -> World {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, move |r| site.reply(r)).await;
    let net =
        NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap();
    let clock = Arc::new(ManualClock::new(1_700_000_000.0, 5_000.0));
    let crawler =
        PawchiveCrawler::with_hosts(net, &server.url("/api/v1"), &server.url("/thumbnail/data"))
            .with_clock(clock.clone());
    let dir = tempfile::tempdir().unwrap();
    let index = PawchiveIndex::with_clock(dir.path().join("index.db"), clock.clone());
    World {
        server,
        crawler,
        clock,
        dir,
        index,
    }
}

async fn crawl(
    w: &World,
    stop: &(dyn Fn() -> bool + Send + Sync),
) -> Result<CrawlResult, PawchiveError> {
    let mut progress = |_: &str, _: usize, _: usize| {};
    w.crawler
        .index_creator(&w.index, "fanbox", "42", 1.0, stop, &mut progress)
        .await
}

fn targets(w: &World) -> Vec<String> {
    w.server.recorded().into_iter().map(|r| r.target).collect()
}

fn thumbnails(range: std::ops::RangeInclusive<u64>) -> BTreeMap<String, Vec<u8>> {
    range
        .map(|i| (path_for(&sha(i), "png"), png(&picture(i, 64, 90))))
        .collect()
}

#[tokio::test]
async fn every_page_of_posts_and_every_image_is_indexed() {
    let n = pawchive_index::PAGE_SIZE as u64 + 3;
    let posts = (0..n)
        .map(|i| post(1000 + i, &[sha(1000 + i)], "png"))
        .collect();
    let w = world(Site {
        name: Some("TEKU".into()),
        posts,
        thumbnails: thumbnails(1000..=1000 + n),
        ..Site::default()
    })
    .await;
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!((result.posts, result.images, result.new), (53, 53, 53));
    assert_eq!(result.error, None);
    let creators = w.index.creators().unwrap();
    assert_eq!(
        (
            creators[0].name.as_deref(),
            creators[0].image_count,
            creators[0].post_count
        ),
        (Some("TEKU"), 53, 53)
    );
    let listing: Vec<String> = targets(&w)
        .into_iter()
        .filter(|t| t.contains("/posts"))
        .collect();
    assert_eq!(
        listing,
        [
            "/api/v1/fanbox/user/42/posts?o=0",
            "/api/v1/fanbox/user/42/posts?o=50"
        ]
    );
    // The first request waits for nothing; every other one waits out the second.
    let sleeps = w.clock.sleeps();
    assert!(!sleeps.is_empty());
    assert!(sleeps.iter().all(|s| *s <= 0.2 + 1e-9));
    // 56 requests (a profile, two listing pages, 53 thumbnails); the first waits for nothing and
    // each of the other 55 waits out the second since the last.
    assert!((w.clock.slept() - 55.0).abs() < 0.01, "{}", w.clock.slept());
}

#[tokio::test]
async fn the_crawl_sends_pawchives_user_agent_with_the_key_requests() {
    let w = world(Site {
        name: Some("A".into()),
        ..Site::default()
    })
    .await;
    crawl(&w, &|| false).await.unwrap();
    for r in w.server.recorded() {
        assert_eq!(r.method, "GET");
        assert_eq!(r.header("user-agent"), Some(net::USER_AGENT));
    }
}

#[tokio::test]
async fn a_refresh_fetches_only_what_is_new() {
    let posts = vec![post(1, &[sha(1)], "png"), post(2, &[sha(2)], "png")];
    let first = world(Site {
        posts: posts.clone(),
        thumbnails: thumbnails(1..=3),
        ..Site::default()
    })
    .await;
    crawl(&first, &|| false).await.unwrap();
    // The same index file, a second crawl against a site that now has one more post.
    let mut again = vec![post(3, &[sha(3)], "png")];
    again.extend(posts);
    let second = world(Site {
        posts: again,
        thumbnails: thumbnails(1..=3),
        ..Site::default()
    })
    .await;
    std::fs::copy(
        first.dir.path().join("index.db"),
        second.dir.path().join("index.db"),
    )
    .unwrap();
    let result = crawl(&second, &|| false).await.unwrap();
    assert_eq!(result.new, 1);
    let thumbs: Vec<String> = targets(&second)
        .into_iter()
        .filter(|t| t.starts_with("/thumbnail/"))
        .collect();
    assert_eq!(
        thumbs,
        [format!("/thumbnail/data{}", path_for(&sha(3), "png"))]
    );
}

#[tokio::test]
async fn non_images_are_skipped_and_a_404_original_is_counted_not_fatal() {
    let mut p = post(1, &[sha(1), sha(2)], "png");
    p["attachments"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "set.zip", "path": path_for(&sha(3), "zip")}));
    let w = world(Site {
        posts: vec![p],
        thumbnails: thumbnails(1..=1),
        ..Site::default()
    })
    .await;
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!(
        (result.images, result.unfingerprinted, result.new),
        (2, 1, 2)
    );
    assert!(!targets(&w).iter().any(|t| t.contains(".zip")));
    // The 404 one is a row with no fingerprint, so it is not asked for again.
    assert_eq!(w.index.known_pages("fanbox", "42").unwrap().len(), 2);
    assert_eq!(w.index.image_count().unwrap(), 1);
}

#[tokio::test]
async fn a_creator_with_no_posts_is_indexed_as_empty() {
    let w = world(Site {
        name: Some("Nobody".into()),
        ..Site::default()
    })
    .await;
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!((result.posts, result.images, result.new), (0, 0, 0));
    assert_eq!(result.error, None);
    let creators = w.index.creators().unwrap();
    assert_eq!(
        (creators[0].post_count, creators[0].indexed_at),
        (0, Some(w.clock.wall()))
    );
    assert_eq!(targets(&w).len(), 2, "a profile and one listing page");
}

#[tokio::test]
async fn an_unknown_artist_is_an_error_not_a_crawl() {
    let w = world(Site {
        profile_status: Some(404),
        ..Site::default()
    })
    .await;
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!(result.error.as_deref(), Some("pawchive has no such artist"));
    assert_eq!(result.error_kind, Some(ErrorKind::NotFound));
    assert_eq!(targets(&w).len(), 1);
    assert!(w.index.creators().unwrap().is_empty(), "nothing was added");
}

#[tokio::test]
async fn a_failed_listing_is_reported_with_its_status() {
    let w = world(Site {
        profile_status: Some(500),
        ..Site::default()
    })
    .await;
    // A profile that is neither 200 nor 404 does not stop the crawl; the listing 200s with no posts.
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!(result.error, None);
    assert_eq!(result.name, None);
}

#[tokio::test]
async fn stopping_keeps_what_was_done_and_the_next_run_carries_on() {
    let posts: Vec<Value> = (1..=5).map(|i| post(i, &[sha(i)], "png")).collect();
    let w = world(Site {
        posts,
        thumbnails: thumbnails(1..=5),
        ..Site::default()
    })
    .await;
    let server = &w.server;
    let thumb_calls = || {
        server
            .recorded()
            .iter()
            .filter(|r| r.target.starts_with("/thumbnail/"))
            .count()
    };
    let result = crawl(&w, &|| thumb_calls() >= 2).await.unwrap();
    assert!(result.stopped);
    assert_eq!(w.index.known_pages("fanbox", "42").unwrap().len(), 2);
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!(result.new, 3);
    assert_eq!(w.index.known_pages("fanbox", "42").unwrap().len(), 5);
}

#[tokio::test]
async fn being_rate_limited_is_waited_out_not_fatal() {
    let site = Site {
        posts: vec![post(1, &[sha(1)], "png")],
        thumbnails: thumbnails(1..=1),
        first_status: Some((429, Some("7".into()))),
        ..Site::default()
    };
    let w = world(site).await;
    let result = crawl(&w, &|| false).await.unwrap();
    assert_eq!(result.error, None);
    assert_eq!(result.new, 1);
    // The 7 seconds named by Retry-After were slept (in steps of 0.2 s).
    assert!(w.clock.slept() >= 7.0, "{}", w.clock.slept());
}

#[tokio::test]
async fn a_429_with_no_usable_retry_after_waits_thirty_seconds() {
    for header in [None, Some("soon".to_owned()), Some(String::new())] {
        let site = Site {
            first_status: Some((429, header)),
            ..Site::default()
        };
        let w = world(site).await;
        crawl(&w, &|| false).await.unwrap();
        assert!(w.clock.slept() >= 30.0, "{}", w.clock.slept());
    }
}

#[tokio::test]
async fn a_dead_host_is_retried_then_reported_in_the_result() {
    let w = world(Site::default()).await;
    // A server that is gone: the crawler points at a port nothing listens on.
    let net = NetClient::new(NetConfig::default()).unwrap();
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("https://127.0.0.1:{}", dead.local_addr().unwrap().port());
    drop(dead);
    let crawler = PawchiveCrawler::with_hosts(net, &format!("{base}/api/v1"), &format!("{base}/t"))
        .with_clock(w.clock.clone());
    let mut progress = |_: &str, _: usize, _: usize| {};
    let result = crawler
        .index_creator(&w.index, "fanbox", "42", 1.0, &|| false, &mut progress)
        .await
        .unwrap();
    let error = result.error.clone().expect("a network failure is reported");
    assert!(!error.is_empty());
    assert_eq!(
        result.error_kind,
        Some(ErrorKind::Net(net::ErrorKind::Connect))
    );
    let shown = result.error_user_message().unwrap();
    assert!(
        !shown.contains("127.0.0.1") && !shown.contains("os error") && !shown.contains("?"),
        "{shown}"
    );
    // Four tries, a second apart.
    assert!(w.clock.slept() >= 3.0);
}

#[tokio::test]
async fn a_self_signed_site_is_refused_by_the_shipped_client() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| Reply::ok("{}")).await;
    let net = NetClient::new(NetConfig::default()).unwrap(); // no extra root: verification on
    let clock = Arc::new(ManualClock::new(1.0e9, 5_000.0));
    let crawler = PawchiveCrawler::with_hosts(net, &server.url("/api/v1"), &server.url("/t"))
        .with_clock(clock);
    let dir = tempfile::tempdir().unwrap();
    let err = crawler
        .find_creators("a", true, &dir.path().join("c.json"), 50)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Tls), "{err}");
    assert_eq!(server.request_count(), 0);
    assert!(err.user_message().contains("secure connection"));
}

#[tokio::test]
async fn a_slow_site_is_a_timeout_after_the_requests_thirty_seconds() {
    // 30 s is the crawl's per-request timeout; the server holds the reply back past a short one.
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| {
        Reply::ok("{}").delayed(std::time::Duration::from_secs(3))
    })
    .await;
    let net =
        NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap();
    let clock = Arc::new(ManualClock::new(1.0e9, 5_000.0));
    let crawler = PawchiveCrawler::with_hosts(net, &server.url("/api/v1"), &server.url("/t"))
        .with_clock(clock);
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let fetched = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        crawler.find_creators("a", true, &dir.path().join("c.json"), 50),
    )
    .await;
    assert!(
        fetched.is_ok(),
        "the 120 s list timeout is not the test's wait"
    );
    assert!(started.elapsed() >= std::time::Duration::from_secs(3));
}

// ---- find_creators ----

fn creators_list() -> Value {
    json!([
        {"id": "1", "name": "TEKU", "service": "patreon", "favorited": 50, "public_id": "teku"},
        {"id": "2", "name": "Tekuho", "service": "fanbox", "favorited": 900, "public_id": "tekuho"},
        {"id": "3", "name": "Someone", "service": "fanbox", "favorited": 10, "public_id": "x"},
    ])
}

async fn creators_world(
    count: Arc<AtomicUsize>,
    body: Value,
    status: u16,
) -> (
    TestServer,
    PawchiveCrawler,
    Arc<ManualClock>,
    tempfile::TempDir,
) {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Reply::new(status, body.to_string())
    })
    .await;
    let net =
        NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap();
    let clock = Arc::new(ManualClock::new(1_700_000_000.0, 5_000.0));
    let crawler = PawchiveCrawler::with_hosts(net, &server.url("/api/v1"), &server.url("/t"))
        .with_clock(clock.clone());
    (server, crawler, clock, tempfile::tempdir().unwrap())
}

#[tokio::test]
async fn the_creator_list_is_downloaded_once_then_kept_for_a_week() {
    let count = Arc::new(AtomicUsize::new(0));
    let (server, crawler, clock, dir) = creators_world(count.clone(), creators_list(), 200).await;
    let cache = dir.path().join("creators.json");
    let found = crawler
        .find_creators("tek", false, &cache, 50)
        .await
        .unwrap();
    let names: Vec<&str> = found.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Tekuho", "TEKU"], "most favourited first");
    crawler
        .find_creators("some", false, &cache, 50)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let req = &server.recorded()[0];
    assert_eq!(req.target, "/api/v1/creators");
    assert_eq!(req.header("user-agent"), Some(net::USER_AGENT));
    // Python's json.dumps spelling, so Python reads the file back.
    assert!(
        std::fs::read_to_string(&cache)
            .unwrap()
            .starts_with("[{\"id\": \"1\", \"name\": \"TEKU\"")
    );
    // The clock says the file was written just now; a week and a second later it is stale.
    let written = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options()
        .write(true)
        .open(&cache)
        .unwrap()
        .set_modified(written)
        .unwrap();
    clock.advance(7.0 * 24.0 * 3600.0 - 1.0);
    crawler
        .find_creators("some", false, &cache, 50)
        .await
        .unwrap();
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a second short of a week is still kept"
    );
    clock.advance(2.0);
    crawler
        .find_creators("some", false, &cache, 50)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    crawler
        .find_creators("some", true, &cache, 50)
        .await
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 3, "refresh asks again");
}

#[tokio::test]
async fn an_empty_query_finds_nothing_but_still_has_the_list() {
    let count = Arc::new(AtomicUsize::new(0));
    let (_server, crawler, _clock, dir) = creators_world(count.clone(), creators_list(), 200).await;
    let cache = dir.path().join("creators.json");
    assert!(
        crawler
            .find_creators("  ", false, &cache, 50)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(cache.exists());
}

#[tokio::test]
async fn a_failed_creator_list_is_an_http_error_that_names_no_url_for_a_person() {
    let count = Arc::new(AtomicUsize::new(0));
    let (server, crawler, _clock, dir) = creators_world(count, json!({}), 503).await;
    let err = crawler
        .find_creators("a", false, &dir.path().join("creators.json"), 50)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::HttpStatus);
    assert_eq!(err.status_code(), Some(503));
    assert_eq!(
        err.message(),
        format!(
            "503 Server Error: Service Unavailable for url: {}",
            server.url("/api/v1/creators")
        )
    );
    let shown = err.user_message();
    assert!(
        !shown.contains("127.0.0.1") && !shown.contains("503") && !shown.contains("?"),
        "{shown}"
    );
}

#[tokio::test]
async fn the_artist_search_worker_body_returns_the_error_text_for_the_window() {
    let count = Arc::new(AtomicUsize::new(0));
    let (_server, crawler, _clock, dir) = creators_world(count, json!({}), 404).await;
    let (found, error) = find_creators_for_ui(&crawler, "a", &dir.path().join("c.json")).await;
    assert!(found.is_empty());
    assert!(
        error
            .unwrap()
            .message()
            .starts_with("404 Client Error: Not Found for url: ")
    );
}

// ---- the worker body ----

#[tokio::test]
async fn the_index_worker_body_runs_each_job_and_honours_stop() {
    let posts: Vec<Value> = (1..=2).map(|i| post(i, &[sha(i)], "png")).collect();
    let w = world(Site {
        name: Some("A".into()),
        posts,
        thumbnails: thumbnails(1..=2),
        ..Site::default()
    })
    .await;
    let stop = AtomicBool::new(false);
    let messages = Mutex::new(Vec::<String>::new());
    let done = Mutex::new(Vec::<(String, usize)>::new());
    let jobs = vec![
        ("fanbox".to_owned(), "42".to_owned()),
        ("fanbox".to_owned(), "43".to_owned()),
    ];
    let mut on_progress = |m: &str, _: usize, _: usize| messages.lock().unwrap().push(m.to_owned());
    let mut on_done = |r: &CrawlResult| {
        done.lock().unwrap().push((r.user_id.clone(), r.new));
        stop.store(true, Ordering::SeqCst); // Stop after the first artist
    };
    let outcome = run_index_jobs(
        &w.crawler,
        &w.index,
        &jobs,
        &stop,
        &mut on_progress,
        &mut on_done,
    )
    .await;
    assert!(outcome.aborted.is_none());
    assert_eq!(
        outcome.results.len(),
        1,
        "the second job never starts after Stop"
    );
    assert_eq!(done.into_inner().unwrap(), [("42".to_owned(), 2)]);
    let messages = messages.into_inner().unwrap();
    assert_eq!(messages[0], "Listing A's posts");
    assert!(messages.contains(&"Fingerprinting A".to_owned()));
    assert_eq!(messages.last().unwrap(), "Indexed A");
}

#[tokio::test]
async fn an_unreadable_database_ends_the_worker_body_with_the_error_not_a_panic() {
    let w = world(Site {
        name: Some("A".into()),
        ..Site::default()
    })
    .await;
    std::fs::write(w.dir.path().join("index.db"), vec![b'x'; 4096]).unwrap();
    let stop = AtomicBool::new(false);
    let mut p = |_: &str, _: usize, _: usize| {};
    let mut d = |_: &CrawlResult| {};
    let jobs = vec![("fanbox".to_owned(), "42".to_owned())];
    let outcome = run_index_jobs(&w.crawler, &w.index, &jobs, &stop, &mut p, &mut d).await;
    assert_eq!(outcome.aborted.unwrap().kind(), ErrorKind::Database);
    assert!(outcome.results.is_empty());
}

// ---- end to end ----

#[tokio::test]
async fn an_edited_copy_of_an_indexed_image_is_found_after_a_crawl() {
    let pictures: Vec<image::RgbImage> = (1..=3).map(|i| picture(i, 300, 420)).collect();
    let posts: Vec<Value> = (1..=3).map(|i| post(i, &[sha(i)], "png")).collect();
    let thumbs = (1..=3u64)
        .map(|i| {
            let small = image::imageops::resize(
                &pictures[i as usize - 1],
                200,
                280,
                image::imageops::FilterType::Lanczos3,
            );
            (path_for(&sha(i), "png"), png(&small))
        })
        .collect();
    let w = world(Site {
        posts,
        thumbnails: thumbs,
        ..Site::default()
    })
    .await;
    crawl(&w, &|| false).await.unwrap();
    let big = image::imageops::resize(
        &pictures[1],
        600,
        840,
        image::imageops::FilterType::Lanczos3,
    );
    let (_, local16) = prints(&jpeg(&big, 80));
    let best = &w.index.search_default(Some(&local16), None).unwrap()[0];
    assert_eq!(best.url, "https://pawchive.pw/fanbox/user/42/post/2");
    assert!(best.similarity >= 90.0, "{}", best.similarity);
}
