//! `core/similarity_check.py` (DAN-2062, Phase 3.g): the Python tests that exercise it, ported from
//! `tests/test_search_quality.py` (`TestMeasuredSimilarityReplacesOrdinalScores`,
//! `TestSkippedMeasurementIsLogged`, `TestBorrowedThumbnails`,
//! `TestNeverMeasuredOrdinalGetsARealScore`) and `tests/test_similarity_marking.py` at `76c85c4`.
//! Python patches `remote.download_bytes`, `local_prints`, `aligned_similarity` and
//! `_worth_hashing`; here the thumbnails are served by a local `https://` server answering for the
//! real hostnames, through `NetClient::new`, and the hashing is the real thing on real pictures.
//! Where Python patched a distance in (100.0, 20.0), the Rust test serves a picture that really
//! is that far from the local one and asserts the band. `docs/image-compare.md` maps each test.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::art::{Canvas, page_art, picture};
use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::boorus::BooruPageInfo;
use hatate_core::config::Settings;
use hatate_core::model::{Candidate, Entry};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::similarity_check::{
    measure_ordinal_similarities, recheck_borrowed_measurement, worth_hashing,
};
use tracing_subscriber::fmt::MakeWriter;

// One process-global subscriber, installed once, writing into a per-thread buffer (see
// `availability_https.rs` for why a scoped one is racy). The tests are `current_thread`.
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

fn capture_logs() {
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
}

fn logged() -> String {
    LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap())
}

type Routes = Arc<Mutex<HashMap<String, Reply>>>;

struct World {
    server: TestServer,
    net: NetClient,
    routes: Routes,
    dir: tempfile::TempDir,
    settings: Settings,
    _sandbox: Sandbox,
}

const HOSTS: [&str; 6] = [
    "t.test",
    "www.google.com",
    "s1.zerochan.net",
    "rule34.paheal.net",
    "e.test",
    "img.example.test",
];

async fn world() -> World {
    let pki = TestPki::generate_for(&HOSTS);
    let routes: Routes = Arc::default();
    let r = routes.clone();
    let server = TestServer::https(&pki, move |req: &Recorded| {
        let key = format!("https://{}{}", req.header("host").unwrap_or(""), req.target);
        r.lock()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or_else(|| Reply::new(404, "not found"))
    })
    .await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for h in HOSTS {
        cfg = cfg.with_host_override(h, server.addr);
    }
    let sandbox = Sandbox::new();
    let settings = Settings::with_defaults(&sandbox.paths);
    World {
        server,
        net: NetClient::new(cfg).unwrap(),
        routes,
        dir: tempfile::tempdir().unwrap(),
        settings,
        _sandbox: sandbox,
    }
}

impl World {
    fn serve(&self, url: &str, reply: Reply) {
        self.routes.lock().unwrap().insert(url.to_owned(), reply);
    }

    fn requests(&self) -> usize {
        self.server.recorded().len()
    }

    fn write(&self, name: &str, bytes: Vec<u8>) -> String {
        let path = self.dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }
}

/// The local picture and a thumbnail an engine would hand back for it: a JPEG with enough bytes
/// per pixel to be worth hashing.
fn local_art() -> Canvas {
    picture(1, 300, 400)
}

fn thumb_of(c: &Canvas) -> Vec<u8> {
    c.thumbnail(220).jpeg(95)
}

fn candidates(specs: &[(&str, f64, Option<&str>)]) -> Vec<Candidate> {
    specs
        .iter()
        .enumerate()
        .map(|(i, (engine, similarity, thumb))| {
            let mut c = Candidate::new(format!("https://e.test/{i}"));
            c.source_name = Some("Other".into());
            c.thumb_url = thumb.map(str::to_owned);
            c.similarity = *similarity;
            c.engine = (*engine).to_owned();
            c
        })
        .collect()
}

async fn measure(w: &World, entry_path: &str, cs: &mut [Candidate]) {
    let entry = Entry::new(entry_path);
    measure_ordinal_similarities(&w.net, &entry, cs, &w.settings)
        .await
        .unwrap();
}

/// Serves `art`'s thumbnail at `url`, writes `art` as the local file, runs the measurement.
async fn run(w: &World, local: &Canvas, served: &[(&str, Reply)], cs: &mut [Candidate]) {
    let path = w.write("local.png", local.png());
    for (url, reply) in served {
        w.serve(url, reply.clone());
    }
    measure(w, &path, cs).await;
}

// -- TestMeasuredSimilarityReplacesOrdinalScores

#[tokio::test]
async fn test_an_ordinal_score_is_replaced_by_a_measurement() {
    let w = world().await;
    let l = local_art();
    let mut cs = candidates(&[("Google Lens", 80.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &l,
        &[("https://t.test/1.jpg", Reply::ok(thumb_of(&l)))],
        &mut cs,
    )
    .await;
    assert!(cs[0].similarity >= 90.0, "{}", cs[0].similarity);
    assert!(cs[0].similarity_measured);
}

#[tokio::test]
async fn test_a_measuring_engine_is_left_alone() {
    let w = world().await;
    let l = local_art();
    let mut cs = candidates(&[("IQDB", 93.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &l,
        &[("https://t.test/1.jpg", Reply::ok(thumb_of(&l)))],
        &mut cs,
    )
    .await;
    assert_eq!(cs[0].similarity, 93.0);
    assert!(!cs[0].similarity_measured);
    assert_eq!(
        w.requests(),
        0,
        "its own number is better than one taken from a thumbnail"
    );
}

#[tokio::test]
async fn test_a_candidate_with_no_thumbnail_keeps_its_ordinal_score() {
    let w = world().await;
    let mut cs = candidates(&[("Google Lens", 80.0, None)]);
    run(&w, &local_art(), &[], &mut cs).await;
    assert_eq!(cs[0].similarity, 80.0);
    assert!(!cs[0].similarity_measured);
}

#[tokio::test]
async fn test_a_thumbnail_that_will_not_download_keeps_its_score() {
    let w = world().await;
    let mut cs = candidates(&[("ascii2d", 86.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &local_art(),
        &[("https://t.test/1.jpg", Reply::new(404, "gone"))],
        &mut cs,
    )
    .await;
    assert_eq!(cs[0].similarity, 86.0);
    assert!(!cs[0].similarity_measured);
    assert_eq!(w.requests(), 1);
}

#[tokio::test]
async fn test_a_different_picture_scores_far_below_its_ordinal_score() {
    let w = world().await;
    let mut cs = candidates(&[("Google Lens", 78.0, Some("https://t.test/1.jpg"))]);
    let other = picture(3, 300, 400);
    run(
        &w,
        &local_art(),
        &[("https://t.test/1.jpg", Reply::ok(thumb_of(&other)))],
        &mut cs,
    )
    .await;
    assert!(cs[0].similarity < 50.0, "{}", cs[0].similarity);
    assert!(cs[0].similarity_measured);
}

/// 221x228 of noise, so the sharp version compresses like a photograph.
fn noise(w: u32, h: u32) -> Canvas {
    let mut c = Canvas::new(w, h, [0, 0, 0]);
    let mut state = 7u64;
    for y in 0..h {
        for x in 0..w {
            let mut px = [0u8; 3];
            for v in &mut px {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *v = (state % 256) as u8;
            }
            c.img.put_pixel(x, y, image::Rgb(px));
        }
    }
    c
}

#[test]
fn test_a_blurred_placeholder_is_never_measured_from() {
    let source = noise(221, 228);
    let blurred = Canvas {
        img: image::imageops::blur(&source.img, 12.0),
    }
    .jpeg(20);
    let sharp = source.jpeg(80);
    assert!(
        !worth_hashing(&blurred).unwrap(),
        "a blur at a few bytes per pixel carries no signal"
    );
    assert!(worth_hashing(&sharp).unwrap());
}

#[test]
fn test_a_tiny_image_is_not_measured_from_either() {
    assert!(!worth_hashing(&Canvas::new(16, 16, [200, 40, 40]).png()).unwrap());
}

#[test]
fn test_junk_bytes_are_not_measured_from() {
    assert!(!worth_hashing(b"not an image").unwrap());
}

// -- TestSkippedMeasurementIsLogged

#[tokio::test]
async fn test_a_candidate_with_no_thumbnail_at_all_says_so() {
    capture_logs();
    let w = world().await;
    let mut cs = candidates(&[("Google Lens", 80.0, None)]);
    cs[0].url = "https://e.test/post/1".into();
    run(&w, &local_art(), &[], &mut cs).await;
    let text = logged();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("not measuring"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    for needle in ["no thumbnail", "Google Lens", "https://e.test/post/1"] {
        assert!(lines[0].contains(needle), "{}", lines[0]);
    }
    assert!(!cs[0].similarity_measured);
}

#[tokio::test]
async fn test_a_thumbnail_too_poor_to_hash_says_so() {
    capture_logs();
    let w = world().await;
    let mut cs = candidates(&[("Google Lens", 80.0, Some("https://t.test/1.jpg"))]);
    cs[0].url = "https://e.test/post/1".into();
    run(
        &w,
        &local_art(),
        &[("https://t.test/1.jpg", Reply::ok("not an image"))],
        &mut cs,
    )
    .await;
    let text = logged();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("not measuring"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    for needle in [
        "too little",
        "Google Lens",
        "https://e.test/post/1",
        "https://t.test/1.jpg",
    ] {
        assert!(lines[0].contains(needle), "{}", lines[0]);
    }
    assert!(!cs[0].similarity_measured);
}

#[tokio::test]
async fn test_a_thumbnail_that_would_not_download_says_so() {
    capture_logs();
    let w = world().await;
    let mut cs = candidates(&[("Google Lens", 80.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &local_art(),
        &[("https://t.test/1.jpg", Reply::new(500, "no"))],
        &mut cs,
    )
    .await;
    let text = logged();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("not measuring"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("came back empty"), "{}", lines[0]);
    assert!(lines[0].contains("https://t.test/1.jpg"), "{}", lines[0]);
    assert!(!cs[0].similarity_measured);
}

#[tokio::test]
async fn test_nothing_is_logged_when_the_measurement_proceeds() {
    capture_logs();
    let w = world().await;
    let l = local_art();
    let mut cs = candidates(&[("Google Lens", 80.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &l,
        &[("https://t.test/1.jpg", Reply::ok(thumb_of(&l)))],
        &mut cs,
    )
    .await;
    let text = logged();
    assert!(!text.lines().any(|l| l.contains("not measuring")), "{text}");
    assert!(cs[0].similarity_measured);
    assert!(text.contains("measured 1/1"), "{text}");
}

#[tokio::test]
async fn test_a_local_file_that_could_not_be_hashed_says_so_at_info() {
    capture_logs();
    let w = world().await;
    let mut cs: Vec<Candidate> = (0..3)
        .map(|i| {
            let mut c = Candidate::new(format!("https://e.test/post/{i}"));
            c.thumb_url = Some(format!("https://t.test/{i}.jpg"));
            c.similarity = 80.0;
            c.engine = "Google Lens".into();
            c
        })
        .collect();
    // A local file that is not a picture.
    let path = w.write("local.png", b"not an image".to_vec());
    measure(&w, &path, &mut cs).await;
    let text = logged();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("could not hash the local file"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(
        lines[0].contains("INFO"),
        "the level is what this test is for: {}",
        lines[0]
    );
    assert!(lines[0].contains("3 candidate(s)"), "{}", lines[0]);
    assert!(!cs.iter().any(|c| c.similarity_measured));
    assert_eq!(w.requests(), 0);
}

/// DAN-2083 (C1): a file Pillow reads but the port does not - an arithmetic-coded JPEG - is logged
/// with the file and the reason, so a real hit can be diagnosed from the log.
#[tokio::test]
async fn test_a_jpeg_the_port_cannot_decode_says_why_at_info() {
    capture_logs();
    let w = world().await;
    let mut cs: Vec<Candidate> = (0..2)
        .map(|i| {
            let mut c = Candidate::new(format!("https://e.test/post/{i}"));
            c.thumb_url = Some(format!("https://t.test/{i}.jpg"));
            c.similarity = 80.0;
            c.engine = "Google Lens".into();
            c
        })
        .collect();
    // SOI, an SOF9 (extended sequential, arithmetic coded) 16x16 one-component frame, SOS, EOI.
    let jpeg = [
        0xFF, 0xD8, 0xFF, 0xC9, 0x00, 0x0B, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
        0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x00, 0xFF, 0xD9,
    ];
    let path = w.write("arith.jpg", jpeg.to_vec());
    measure(&w, &path, &mut cs).await;
    let text = logged();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("could not hash the local file"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("INFO"), "{}", lines[0]);
    assert!(lines[0].contains("arith.jpg"), "{}", lines[0]);
    assert!(lines[0].contains("arithmetic"), "{}", lines[0]);
    assert!(!cs.iter().any(|c| c.similarity_measured));
    assert_eq!(w.requests(), 0);
}

// -- test_similarity_marking.TestTheFlagSurvivesARestart

#[tokio::test]
async fn test_a_restored_measured_candidate_is_not_queued_for_re_measuring() {
    let w = world().await;
    let l = local_art();
    let mut cs = candidates(&[("Google Lens", 91.0, Some("https://t.test/1.jpg"))]);
    cs[0].similarity_measured = true;
    run(
        &w,
        &l,
        &[("https://t.test/1.jpg", Reply::ok(thumb_of(&l)))],
        &mut cs,
    )
    .await;
    assert_eq!(cs[0].similarity, 91.0);
    assert_eq!(w.requests(), 0, "a measured score is not measured again");
}

// -- inline thumbnails and the ticket's network checks

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[tokio::test]
async fn a_data_uri_thumbnail_is_measured_without_a_request() {
    let w = world().await;
    let l = local_art();
    let uri = format!("data:image/jpeg;base64,{}", base64(&thumb_of(&l)));
    let mut cs = candidates(&[("Google Lens", 80.0, Some(uri.as_str()))]);
    run(&w, &l, &[], &mut cs).await;
    assert!(cs[0].similarity_measured);
    assert!(cs[0].similarity >= 90.0);
    assert_eq!(w.requests(), 0);
}

#[tokio::test]
async fn an_inline_thumbnail_is_measured_and_the_url_is_not_fetched() {
    let w = world().await;
    let l = local_art();
    let mut cs = candidates(&[("Google Lens", 80.0, Some("https://t.test/1.jpg"))]);
    cs[0].thumb_bytes = Some(thumb_of(&l));
    run(&w, &l, &[], &mut cs).await;
    assert!(cs[0].similarity_measured);
    assert_eq!(w.requests(), 0);
}

#[tokio::test]
async fn a_download_deadline_leaves_the_candidate_unmeasured() {
    let pki = TestPki::generate_for(&["t.test"]);
    // A body that trickles: every read is quick, the whole is not.
    let server = TestServer::https(&pki, |_| {
        Reply::ok(vec![0xFFu8; 60]).trickled(1, Duration::from_millis(60))
    })
    .await;
    let net = NetClient::new(
        NetConfig::default()
            .with_root_certificate_der(pki.ca_der.clone())
            .with_host_override("t.test", server.addr),
    )
    .unwrap();
    let sandbox = Sandbox::new();
    let mut settings = Settings::with_defaults(&sandbox.paths);
    settings.search_timeout = 0.4;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.png");
    std::fs::write(&path, local_art().png()).unwrap();
    let mut cs = candidates(&[("ascii2d", 86.0, Some("https://t.test/1.jpg"))]);
    let started = std::time::Instant::now();
    let entry = Entry::new(path.to_string_lossy());
    measure_ordinal_similarities(&net, &entry, &mut cs, &settings)
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the deadline is over the whole download"
    );
    assert_eq!(cs[0].similarity, 86.0);
    assert!(!cs[0].similarity_measured);
}

#[tokio::test]
async fn a_dropped_connection_leaves_the_candidate_unmeasured() {
    let w = world().await;
    let mut cs = candidates(&[("ascii2d", 86.0, Some("https://t.test/1.jpg"))]);
    run(
        &w,
        &local_art(),
        &[("https://t.test/1.jpg", Reply::aborted())],
        &mut cs,
    )
    .await;
    assert_eq!(cs[0].similarity, 86.0);
    assert!(!cs[0].similarity_measured);
}

// -- TestBorrowedThumbnails and TestNeverMeasuredOrdinalGetsARealScore

fn page(width: Option<i64>, height: Option<i64>, preview: Option<&str>) -> BooruPageInfo {
    BooruPageInfo {
        width,
        height,
        preview_url: preview.map(str::to_owned),
        ..BooruPageInfo::default()
    }
}

fn flat_png(size: (u32, u32), color: [u8; 3]) -> Vec<u8> {
    Canvas::new(size.0, size.1, color).png()
}

#[tokio::test]
async fn test_a_wrong_shape_is_remeasured_against_the_posts_own_image() {
    let w = world().await;
    let mut c = Candidate::new("https://www.zerochan.net/3700079");
    c.similarity = 100.0;
    c.similarity_measured = true;
    c.engine = "Google Lens".into();
    let local = w.write("local.png", flat_png((1105, 1565), [90, 120, 200]));
    let preview = "https://s1.zerochan.net/x.600.3700079.jpg";
    // The post's own image: nothing like the (flat) local file.
    w.serve(preview, Reply::ok(thumb_of(&page_art(1))));
    recheck_borrowed_measurement(
        &w.net,
        &mut c,
        &page(Some(4093), Some(2894), Some(preview)),
        &w.settings,
        &local,
    )
    .await
    .unwrap();
    assert!(c.similarity < 50.0, "{}", c.similarity);
    assert!(c.similarity_measured);
    assert_eq!(w.requests(), 1);
}

#[tokio::test]
async fn test_the_right_shape_is_left_alone() {
    let w = world().await;
    let mut c = Candidate::new("https://www.zerochan.net/3700079");
    c.similarity = 100.0;
    c.similarity_measured = true;
    c.engine = "Google Lens".into();
    let local = w.write("local.png", flat_png((1105, 1565), [90, 120, 200]));
    recheck_borrowed_measurement(
        &w.net,
        &mut c,
        &page(
            Some(1105),
            Some(1565),
            Some("https://s1.zerochan.net/x.600.3700079.jpg"),
        ),
        &w.settings,
        &local,
    )
    .await
    .unwrap();
    assert_eq!(w.requests(), 0);
    assert_eq!(c.similarity, 100.0);
}

#[tokio::test]
async fn test_no_image_of_its_own_means_unmeasured() {
    let w = world().await;
    let mut c = Candidate::new("https://www.zerochan.net/3700079");
    c.similarity = 100.0;
    c.similarity_measured = true;
    c.engine = "Google Lens".into();
    let local = w.write("local.png", flat_png((1105, 1565), [90, 120, 200]));
    recheck_borrowed_measurement(
        &w.net,
        &mut c,
        &page(
            Some(4093),
            Some(2894),
            Some("https://s1.zerochan.net/x.600.3700079.jpg"),
        ),
        &w.settings,
        &local,
    )
    .await
    .unwrap();
    assert!(!c.similarity_measured);
}

#[tokio::test]
async fn test_a_lens_exact_match_gets_measured_instead_of_the_ordinal_80() {
    let w = world().await;
    let mut c = Candidate::new("https://rule34.paheal.net/post/view/6611704");
    c.similarity = 80.0;
    c.engine = "Google Lens".into();
    let flat = [10, 200, 30];
    let local = w.write("local.png", flat_png((1200, 1600), flat));
    let preview = "https://rule34.paheal.net/x.jpg";
    w.serve(preview, Reply::ok(flat_png((300, 400), flat)));
    recheck_borrowed_measurement(
        &w.net,
        &mut c,
        &page(Some(1200), Some(1600), Some(preview)),
        &w.settings,
        &local,
    )
    .await
    .unwrap();
    assert_eq!(c.similarity, 100.0);
    assert!(c.similarity_measured);
}

#[tokio::test]
async fn test_a_real_similarity_engine_is_left_alone() {
    let w = world().await;
    let mut c = Candidate::new("https://danbooru.donmai.us/posts/1");
    c.similarity = 92.0;
    c.engine = "IQDB".into();
    let local = w.write("local.png", flat_png((1200, 1600), [10, 200, 30]));
    recheck_borrowed_measurement(
        &w.net,
        &mut c,
        &page(
            Some(1200),
            Some(1600),
            Some("https://rule34.paheal.net/x.jpg"),
        ),
        &w.settings,
        &local,
    )
    .await
    .unwrap();
    assert_eq!(w.requests(), 0);
    assert_eq!(c.similarity, 92.0);
    assert!(!c.similarity_measured);
}

#[tokio::test]
async fn test_no_page_image_leaves_the_ordinal_score_but_marked_unmeasured() {
    let w = world().await;
    let mut c = Candidate::new("https://rule34.paheal.net/post/view/1");
    c.similarity = 78.0;
    c.engine = "Google Lens".into();
    let local = w.write("local.png", flat_png((1200, 1600), [10, 200, 30]));
    recheck_borrowed_measurement(&w.net, &mut c, &page(None, None, None), &w.settings, &local)
        .await
        .unwrap();
    assert_eq!(c.similarity, 78.0);
    assert!(!c.similarity_measured);
}
