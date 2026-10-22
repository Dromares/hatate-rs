//! `core/multipage.py` and `core/multipage_resolve.py` (DAN-2062, Phase 3.g): the Python tests in
//! `tests/test_multipage.py` at `76c85c4`, ported, plus the three network checks the ticket asks
//! for. The resolver tests go through `NetClient::new` (rustls with the explicit provider) against
//! a local `https://` server that answers for the real hostnames (`NetConfig::with_host_override`),
//! so the page list is fetched with the site's real headers and the thumbnails come over TLS.
//!
//! Python patches `remote.fetch_text`, `remote.download_bytes` and `image_dimensions`; here the
//! server serves the answers and each local file really has the size the Python test patched in.
//! The pictures are drawn here (`common::art`), so each test asserts the outcome its Python
//! original asserts. `docs/image-compare.md` maps every Python test to its Rust name.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::art::{Canvas, page_art, thumbnail_bytes};
use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::boorus::{BooruPageInfo, pixiv};
use hatate_core::config::Settings;
use hatate_core::image_compare::Source;
use hatate_core::model::Candidate;
use hatate_core::multipage::{
    MAX_PLAUSIBLE_DISTANCE, NEAR_EXACT_DISTANCE, PageSize, check_page, choose_matching_page,
    dimensions_contradict_page, dimensions_identify_page, page_index_from_thumbnail,
    page_url_for_index,
};
use hatate_core::multipage_resolve::{ResolveError, resolve_multipage_candidate};
use hatate_core::net::{NetClient, NetConfig, USER_AGENT};
use hatate_core::pyraster::{Image, Mode};
use hatate_core::remote::{self, Raised};
use serde_json::json;

fn image_of(c: &Canvas) -> Image {
    let (w, h) = c.size();
    Image {
        width: w as usize,
        height: h as usize,
        mode: Mode::Rgb,
        data: c.img.as_raw().clone(),
    }
}

fn pages_of(arts: &[Canvas]) -> Vec<(usize, Vec<u8>)> {
    arts.iter()
        .enumerate()
        .map(|(i, a)| (i, thumbnail_bytes(a)))
        .collect()
}

fn art_pages(n: usize) -> Vec<Canvas> {
    (0..n).map(page_art).collect()
}

// -- TestChoosingThePage

#[test]
fn test_identifies_each_page_in_turn() {
    let pages = pages_of(&art_pages(5));
    for target in 0..5 {
        let local = image_of(&page_art(target));
        let m = choose_matching_page(Source::Image(&local), &pages)
            .unwrap()
            .unwrap();
        assert!(m.confident, "page {target}: {m:?}");
        assert_eq!(m.index, target);
    }
}

#[test]
fn test_a_later_page_is_found_not_just_the_first() {
    let pages = pages_of(&art_pages(5));
    let local = image_of(&page_art(3));
    let m = choose_matching_page(Source::Image(&local), &pages)
        .unwrap()
        .unwrap();
    assert_eq!(m.index, 3);
}

#[test]
fn test_single_page_post_is_not_considered() {
    let local = image_of(&page_art(0));
    let one = [(0usize, thumbnail_bytes(&page_art(0)))];
    assert!(
        choose_matching_page(Source::Image(&local), &one)
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_empty_input() {
    let local = image_of(&page_art(0));
    assert!(
        choose_matching_page(Source::Image(&local), &[])
            .unwrap()
            .is_none()
    );
}

// -- TestRefusingToGuess

fn stranger() -> Canvas {
    let mut c = Canvas::new(400, 600, [255, 255, 255]);
    c.polygon(&[(0.0, 0.0), (400.0, 0.0), (0.0, 600.0)], [10, 10, 10]);
    c
}

#[test]
fn test_indistinguishable_pages_are_not_chosen_between() {
    let identical: Vec<_> = (0..4).map(|i| (i, thumbnail_bytes(&page_art(0)))).collect();
    let local = image_of(&page_art(0));
    let m = choose_matching_page(Source::Image(&local), &identical)
        .unwrap()
        .unwrap();
    assert!(!m.confident);
    assert!(m.reason.contains("indistinguishable"), "{}", m.reason);
}

#[test]
fn test_local_image_absent_from_the_post() {
    let pages = pages_of(&art_pages(5));
    let mut unrelated = Canvas::new(400, 600, [250, 245, 240]);
    unrelated.ellipse([30.0, 30.0, 370.0, 370.0], [80, 140, 200]);
    unrelated.rectangle([60.0, 400.0, 340.0, 560.0], [220, 120, 40]);
    unrelated.line((30.0, 390.0), (370.0, 390.0), [0, 0, 0], 6);
    let local = image_of(&unrelated);
    let m = choose_matching_page(Source::Image(&local), &pages)
        .unwrap()
        .unwrap();
    assert!(!m.confident);
}

#[test]
fn test_a_featureless_local_image_is_refused() {
    let pages = pages_of(&art_pages(3));
    let flat = image_of(&Canvas::new(400, 600, [0, 0, 0]));
    assert!(
        choose_matching_page(Source::Image(&flat), &pages)
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_unreadable_local_image() {
    let pages = pages_of(&art_pages(3));
    assert!(
        choose_matching_page(Source::Bytes(b"not an image"), &pages)
            .unwrap()
            .is_none()
    );
}

#[test]
fn test_unreadable_thumbnails() {
    let local = image_of(&page_art(0));
    let junk = [(0usize, b"junk".to_vec()), (1, b"junk".to_vec())];
    assert!(
        choose_matching_page(Source::Image(&local), &junk)
            .unwrap()
            .is_none()
    );
}

// -- TestPageUrlRewriting

const ORIGINAL: &str = "https://i.pximg.net/img-original/img/2020/01/02/03/04/05/12345678_p0.jpg";

#[test]
fn test_rewrites_to_another_page() {
    assert!(
        page_url_for_index(ORIGINAL, 3)
            .unwrap()
            .ends_with("12345678_p3.jpg")
    );
}

#[test]
fn test_page_zero_is_unchanged() {
    assert_eq!(page_url_for_index(ORIGINAL, 0).as_deref(), Some(ORIGINAL));
}

#[test]
fn test_only_the_last_marker_is_replaced() {
    let tricky = "https://i.pximg.net/img-original/_p0_dir/999_p0.jpg";
    assert_eq!(
        page_url_for_index(tricky, 2).as_deref(),
        Some("https://i.pximg.net/img-original/_p0_dir/999_p2.jpg")
    );
}

#[test]
fn test_a_url_without_the_marker_is_refused() {
    assert_eq!(page_url_for_index("https://example.com/image.jpg", 2), None);
    assert_eq!(page_url_for_index("", 2), None);
}

// -- TestPixivPageMetadata

#[test]
fn test_page_count() {
    let single = json!({"error": false, "body": {"pageCount": 1, "urls": {"original": "x"}}});
    let multi = json!({"error": false, "body": {"pageCount": 7, "urls": {"original": "x"}}});
    assert_eq!(pixiv::parse_page_count(&single.to_string(), "u"), 1);
    assert_eq!(pixiv::parse_page_count(&multi.to_string(), "u"), 7);
}

#[test]
fn test_missing_page_count_means_one() {
    let body = json!({"error": false, "body": {"urls": {"original": "x"}}});
    assert_eq!(pixiv::parse_page_count(&body.to_string(), "u"), 1);
}

#[test]
fn test_pages_api_url_for_both_url_forms() {
    assert_eq!(
        pixiv::pages_api_url("https://www.pixiv.net/artworks/12345678").as_deref(),
        Some("https://www.pixiv.net/ajax/illust/12345678/pages")
    );
    assert_eq!(
        pixiv::pages_api_url("https://www.pixiv.net/member_illust.php?mode=medium&illust_id=999")
            .as_deref(),
        Some("https://www.pixiv.net/ajax/illust/999/pages")
    );
}

#[test]
fn test_parses_the_page_list() {
    let body = json!({"error": false, "body": [
        {"urls": {"small": "s0", "original": "o0"}},
        {"urls": {"small": "s1", "original": "o1"}}]});
    let pages = pixiv::parse_pages(&body.to_string(), "u");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["urls"]["original"], "o1");
}

#[test]
fn test_error_response_yields_no_pages() {
    assert!(pixiv::parse_pages(&json!({"error": true}).to_string(), "u").is_empty());
}

// -- TestPageIndexFromSauceNaoThumbnail

const MANGA: &str =
    "https://img1.saucenao.com/res/pixiv/8986/manga/89861006_p1.jpg?auth=abc&exp=1787684400";

fn index(url: Option<&str>, id: Option<&str>) -> Option<i128> {
    page_index_from_thumbnail(url, id).unwrap()
}

#[test]
fn test_reads_the_page_off_a_manga_thumbnail() {
    assert_eq!(index(Some(MANGA), Some("89861006")), Some(1));
}

#[test]
fn test_double_digit_pages() {
    let url = "https://img1.saucenao.com/res/pixiv/10475/manga/104759687_p32.jpg";
    assert_eq!(index(Some(url), Some("104759687")), Some(32));
}

#[test]
fn test_single_page_thumbnail_form() {
    let url = "https://img1.saucenao.com/res/pixiv/7305/73053388_p0_master1200.jpg";
    assert_eq!(index(Some(url), Some("73053388")), Some(0));
}

#[test]
fn test_historical_index_is_read_too() {
    let url = "https://img1.saucenao.com/res/pixiv_historical/460/manga/4600491_p2.jpg";
    assert_eq!(index(Some(url), Some("4600491")), Some(2));
}

#[test]
fn test_a_different_illust_id_is_refused() {
    assert_eq!(index(Some(MANGA), Some("12345678")), None);
}

#[test]
fn test_old_thumbnails_carry_no_page() {
    assert_eq!(
        index(
            Some("https://img1.saucenao.com/res/pixiv/972/9725787_s.jpg"),
            Some("9725787")
        ),
        None
    );
    assert_eq!(
        index(
            Some("https://img1.saucenao.com/res/pixiv/3446/34466861_m.jpg"),
            Some("34466861")
        ),
        None
    );
}

#[test]
fn test_only_saucenao_pixiv_thumbnails() {
    assert_eq!(
        index(Some("https://iqdb.org/thumbs/1_p3.jpg"), Some("1")),
        None
    );
    assert_eq!(
        index(
            Some("https://img3.saucenao.com/booru/f/f/ffa93b4481cfbaffdee9406bd572d20b_2.jpg"),
            Some("1")
        ),
        None
    );
    assert_eq!(
        index(
            Some("https://evil.example.com/res/pixiv/1/manga/89861006_p1.jpg"),
            Some("89861006")
        ),
        None
    );
}

#[test]
fn test_missing_inputs() {
    assert_eq!(index(None, Some("89861006")), None);
    assert_eq!(index(Some(""), Some("89861006")), None);
    assert_eq!(index(Some(MANGA), None), None);
}

// -- TestDimensionsAsConfirmation

fn sizes(v: &[(i64, i64)]) -> Vec<PageSize> {
    v.iter().map(|&(w, h)| Some((Some(w), Some(h)))).collect()
}

#[test]
fn test_a_unique_exact_size_identifies_the_page() {
    let pages = sizes(&[(3474, 6642), (1986, 2938), (800, 600)]);
    assert_eq!(
        dimensions_identify_page(Some((1986, 2938)), &pages),
        Some(1)
    );
}

#[test]
fn test_a_size_shared_by_two_pages_identifies_nothing() {
    let pages = sizes(&[(1000, 1000), (1000, 1000), (800, 600)]);
    assert_eq!(dimensions_identify_page(Some((1000, 1000)), &pages), None);
}

#[test]
fn test_a_resized_local_file_identifies_nothing() {
    let pages = sizes(&[(3474, 6642), (1986, 2938), (800, 600)]);
    assert_eq!(dimensions_identify_page(Some((993, 1469)), &pages), None);
}

#[test]
fn test_unknown_local_size() {
    let pages = sizes(&[(3474, 6642), (1986, 2938), (800, 600)]);
    assert_eq!(dimensions_identify_page(None, &pages), None);
}

// -- TestDimensionsAsRefutation

#[test]
fn test_a_resize_of_the_named_page_is_not_contradicted() {
    let pages = sizes(&[(3474, 6642), (1986, 2938)]);
    assert!(!dimensions_contradict_page(Some((993, 1469)), &pages, 1));
}

#[test]
fn test_a_portrait_local_file_contradicts_a_landscape_page() {
    let pages = sizes(&[(800, 1000), (574, 1000)]);
    assert!(dimensions_contradict_page(Some((1157, 1000)), &pages, 1));
}

#[test]
fn test_the_widest_confirmed_real_case_still_passes() {
    let pages = sizes(&[(1000, 1000), (676, 1000)]);
    assert!(!dimensions_contradict_page(Some((707, 1000)), &pages, 1));
}

#[test]
fn test_unknown_sizes_never_contradict() {
    assert!(!dimensions_contradict_page(
        None,
        &sizes(&[(1, 1), (2, 2)]),
        1
    ));
    let unknown: Vec<PageSize> = vec![Some((Some(1), Some(1))), Some((None, None))];
    assert!(!dimensions_contradict_page(Some((10, 10)), &unknown, 1));
    assert!(!dimensions_contradict_page(
        Some((10, 10)),
        &sizes(&[(1, 1)]),
        5
    ));
}

// -- TestTiesAreMarked

#[test]
fn test_indistinguishable_pages_are_flagged() {
    let art = page_art(0);
    let pages = [(0usize, thumbnail_bytes(&art)), (1, thumbnail_bytes(&art))];
    let local = image_of(&art);
    let m = choose_matching_page(Source::Image(&local), &pages)
        .unwrap()
        .unwrap();
    assert!(!m.confident);
    assert!(m.indistinguishable);
}

#[test]
fn test_a_post_without_the_image_is_not_flagged() {
    let pages = pages_of(&art_pages(3));
    let local = image_of(&stranger());
    let m = choose_matching_page(Source::Image(&local), &pages)
        .unwrap()
        .unwrap();
    assert!(!m.confident);
    assert!(!m.indistinguishable);
}

#[test]
fn test_a_confident_match_is_not_flagged() {
    let pages = pages_of(&art_pages(5));
    let local = image_of(&page_art(3));
    let m = choose_matching_page(Source::Image(&local), &pages)
        .unwrap()
        .unwrap();
    assert!(m.confident);
    assert!(!m.indistinguishable);
}

// -- TestCheckingOnePage

#[test]
fn test_the_right_page_is_confirmed() {
    let art = page_art(2);
    let check = check_page(Source::Image(&image_of(&art)), &thumbnail_bytes(&art)).unwrap();
    assert!(check.confirmed);
    assert!(check.distance.unwrap() <= 5);
}

#[test]
fn test_a_different_page_is_refused() {
    let local = image_of(&page_art(2));
    let check = check_page(Source::Image(&local), &thumbnail_bytes(&page_art(4))).unwrap();
    assert!(!check.confirmed);
    assert!(check.distance.unwrap() > 5);
}

#[test]
fn test_plausible_is_not_good_enough() {
    // Python patches `hamming_distance` to NEAR_EXACT_DISTANCE + 1. Here a real picture and a
    // real edit of it (a third papered over) read 7 apart: plausible, and not confirmed.
    assert!(MAX_PLAUSIBLE_DISTANCE > NEAR_EXACT_DISTANCE);
    let mut base = Canvas::new(240, 240, [0, 0, 0]);
    for x in 0..240u32 {
        for y in 0..240u32 {
            base.img.put_pixel(
                x,
                y,
                image::Rgb([
                    ((x * 3 + y) % 256) as u8,
                    ((x * x + y * 7) % 256) as u8,
                    ((x + y * 5) % 256) as u8,
                ]),
            );
        }
    }
    let mut edited = base.clone();
    edited.rectangle([0.0, 168.0, 240.0, 240.0], [250, 250, 250]);
    let check = check_page(Source::Image(&image_of(&base)), &edited.png()).unwrap();
    assert_eq!(check.distance, Some(NEAR_EXACT_DISTANCE + 2));
    assert!(!check.confirmed);
}

#[test]
fn test_a_featureless_local_image_confirms_nothing() {
    let flat = Canvas::new(400, 600, [128, 128, 128]);
    let check = check_page(Source::Image(&image_of(&flat)), &thumbnail_bytes(&flat)).unwrap();
    assert!(!check.confirmed);
    assert!(check.reason.contains("featureless"), "{}", check.reason);
}

#[test]
fn test_unreadable_images() {
    let art = image_of(&page_art(0));
    assert!(
        !check_page(
            Source::Bytes(b"not an image"),
            &thumbnail_bytes(&page_art(0))
        )
        .unwrap()
        .confirmed
    );
    assert!(!check_page(Source::Image(&art), b"junk").unwrap().confirmed);
}

// -- the resolver, over https

type Routes = Arc<Mutex<HashMap<String, Vec<u8>>>>;

struct World {
    server: TestServer,
    net: NetClient,
    routes: Routes,
    dir: tempfile::TempDir,
    settings: Settings,
    _sandbox: Sandbox,
}

const HOSTS: [&str; 5] = [
    "www.pixiv.net",
    "i.pximg.net",
    "pbs.twimg.com",
    "api.fxtwitter.com",
    "danbooru.donmai.us",
];

async fn world() -> World {
    let pki = TestPki::generate_for(&HOSTS);
    let routes: Routes = Arc::default();
    let r = routes.clone();
    let server = TestServer::https(&pki, move |req: &Recorded| {
        let key = format!("https://{}{}", req.header("host").unwrap_or(""), req.target);
        match r.lock().unwrap().get(&key) {
            Some(body) => Reply::ok(body.clone()),
            None => Reply::new(404, "not found"),
        }
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
    fn serve(&self, url: &str, body: Vec<u8>) {
        self.routes.lock().unwrap().insert(url.to_owned(), body);
    }

    /// The requests that went to `host`.
    fn requests_to(&self, host: &str) -> usize {
        self.server
            .recorded()
            .iter()
            .filter(|r| r.header("host") == Some(host))
            .count()
    }

    /// A local file: `art` drawn at `size` (resized if it is not that already), as a PNG.
    fn local(&self, art: &Canvas, size: (u32, u32)) -> String {
        let canvas = if art.size() == size {
            art.clone()
        } else {
            art.resized(size.0, size.1)
        };
        self.write(canvas.png())
    }

    /// A local file that is one flat grey, `size` big: for the tests whose Python original only
    /// ever asks for the file's dimensions.
    fn flat(&self, size: (u32, u32)) -> String {
        let img = image::GrayImage::from_pixel(size.0, size.1, image::Luma([120]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        self.write(out)
    }

    fn write(&self, bytes: Vec<u8>) -> String {
        let n = std::fs::read_dir(self.dir.path()).unwrap().count();
        let path = self.dir.path().join(format!("local{n}.png"));
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }
}

// -- TestResolvingAMultipageCandidate

const ARTWORK: &str = "https://www.pixiv.net/artworks/89861006";

fn small(i: usize) -> String {
    format!("https://i.pximg.net/c/540x540_70/89861006_p{i}_master1200.jpg")
}

fn pages_json(sizes: &[(i64, i64)]) -> String {
    let body: Vec<_> = sizes
        .iter()
        .enumerate()
        .map(|(i, (w, h))| {
            json!({"urls": {
                "small": small(i),
                "regular": format!("https://i.pximg.net/img-master/89861006_p{i}_master1200.jpg"),
                "original": format!("https://i.pximg.net/img-original/89861006_p{i}.jpg")},
                "width": w, "height": h})
        })
        .collect();
    json!({"error": false, "body": body}).to_string()
}

fn pixiv_candidate(thumb_page: Option<usize>) -> Candidate {
    let mut c = Candidate::new(ARTWORK);
    c.source_name = Some("Pixiv".into());
    c.engine = "SauceNAO".into();
    c.thumb_url = Some(match thumb_page {
        Some(p) => format!("https://img1.saucenao.com/res/pixiv/8986/manga/89861006_p{p}.jpg"),
        None => "https://iqdb.org/thumbs/2021/nope.jpg".into(),
    });
    c.direct_file_url = Some("https://i.pximg.net/img-original/89861006_p0.jpg".into());
    c.preview_url = Some("https://i.pximg.net/img-master/89861006_p0_master1200.jpg".into());
    c.width = Some(3474);
    c.height = Some(6642);
    c
}

/// Serves the page list and the thumbnails of `thumbs` (by page index), then resolves.
async fn resolve(
    w: &World,
    candidate: &mut Candidate,
    sizes: &[(i64, i64)],
    local: &str,
    thumbs: &[(usize, Vec<u8>)],
    page_count: Option<i64>,
) {
    w.serve(
        "https://www.pixiv.net/ajax/illust/89861006/pages",
        pages_json(sizes).into_bytes(),
    );
    for (i, bytes) in thumbs {
        w.serve(&small(*i), bytes.clone());
    }
    resolve_multipage_candidate(
        &w.net,
        candidate,
        page_count.unwrap_or(sizes.len() as i64),
        &w.settings,
        local,
        None,
    )
    .await
    .unwrap();
}

fn thumbs_of(arts: &[Canvas]) -> Vec<(usize, Vec<u8>)> {
    pages_of(arts)
}

#[tokio::test]
async fn test_dimensions_confirm_saucenaos_page_without_downloading_anything() {
    let w = world().await;
    let local = w.flat((1986, 2938));
    let mut c = pixiv_candidate(Some(1));
    resolve(&w, &mut c, &[(3474, 6642), (1986, 2938)], &local, &[], None).await;
    assert_eq!(c.page_index, 1);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://i.pximg.net/img-original/89861006_p1.jpg")
    );
    assert_eq!(w.requests_to("i.pximg.net"), 0);
    assert_eq!(
        w.requests_to("www.pixiv.net"),
        1,
        "the page list is fetched once"
    );
}

#[tokio::test]
async fn test_the_chosen_pages_own_dimensions_are_kept() {
    let w = world().await;
    let local = w.flat((1986, 2938));
    let mut c = pixiv_candidate(Some(1));
    resolve(&w, &mut c, &[(3474, 6642), (1986, 2938)], &local, &[], None).await;
    assert_eq!((c.width, c.height), (Some(1986), Some(2938)));
}

#[tokio::test]
async fn test_hashing_overrules_a_stale_saucenao_index() {
    let w = world().await;
    let arts = art_pages(3);
    let local = w.local(&arts[1], (450, 300));
    let mut c = pixiv_candidate(Some(2));
    resolve(
        &w,
        &mut c,
        &[(900, 600); 3],
        &local,
        &thumbs_of(&arts),
        None,
    )
    .await;
    assert_eq!(c.page_index, 1);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://i.pximg.net/img-original/89861006_p1.jpg")
    );
}

#[tokio::test]
async fn test_an_unhashable_local_file_falls_back_to_saucenao() {
    let w = world().await;
    let mut c = pixiv_candidate(Some(2));
    resolve(
        &w,
        &mut c,
        &[(900, 600); 3],
        "/does/not/exist.jpg",
        &[],
        None,
    )
    .await;
    assert_eq!(c.page_index, 2);
}

#[tokio::test]
async fn test_a_page_beyond_the_post_is_ignored() {
    let w = world().await;
    let mut c = pixiv_candidate(Some(4));
    resolve(&w, &mut c, &[(900, 600); 4], "/local/file.jpg", &[], None).await;
    assert_eq!(c.page_index, 0);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://i.pximg.net/img-original/89861006_p0.jpg")
    );
}

#[tokio::test]
async fn test_a_page_of_the_wrong_shape_is_ignored() {
    let w = world().await;
    let local = w.flat((1157, 1000));
    let mut c = pixiv_candidate(Some(1));
    resolve(&w, &mut c, &[(1000, 1000), (574, 1000)], &local, &[], None).await;
    assert_eq!(c.page_index, 0);
}

#[tokio::test]
async fn test_saucenao_breaks_a_tie_hashing_refuses() {
    let w = world().await;
    let art = page_art(0);
    let local = w.local(&art, (450, 300));
    let mut c = pixiv_candidate(Some(1));
    resolve(
        &w,
        &mut c,
        &[(900, 600); 2],
        &local,
        &thumbs_of(&[art.clone(), art]),
        None,
    )
    .await;
    assert_eq!(c.page_index, 1);
}

#[tokio::test]
async fn test_a_post_that_doesnt_contain_the_image_is_left_alone() {
    let w = world().await;
    let local = w.local(&stranger(), (450, 300));
    let mut c = pixiv_candidate(Some(2));
    resolve(
        &w,
        &mut c,
        &[(900, 600); 3],
        &local,
        &thumbs_of(&art_pages(3)),
        None,
    )
    .await;
    assert_eq!(c.page_index, 0);
}

#[tokio::test]
async fn test_a_huge_post_is_resolvable_when_saucenao_names_the_page() {
    let w = world().await;
    let mut sizes = vec![(3474, 6642); 40];
    sizes[32] = (1986, 2938);
    let local = w.flat((1986, 2938));
    let mut c = pixiv_candidate(Some(32));
    resolve(&w, &mut c, &sizes, &local, &[], Some(40)).await;
    assert_eq!(c.page_index, 32);
    assert_eq!(w.requests_to("i.pximg.net"), 0);
}

#[tokio::test]
async fn test_a_huge_post_with_nothing_naming_a_page_keeps_the_first() {
    let w = world().await;
    let local = w.flat((999, 888));
    let mut c = pixiv_candidate(None);
    resolve(&w, &mut c, &vec![(3474, 6642); 40], &local, &[], Some(40)).await;
    assert_eq!(c.page_index, 0);
    assert_eq!(w.requests_to("i.pximg.net"), 0);
    assert_eq!(
        w.requests_to("www.pixiv.net"),
        1,
        "the page list is still fetched"
    );
}

#[tokio::test]
async fn test_a_huge_posts_named_page_is_checked_on_its_own() {
    let w = world().await;
    let art = page_art(3);
    let local = w.local(&art, (800, 1122));
    let mut c = pixiv_candidate(Some(54));
    resolve(
        &w,
        &mut c,
        &vec![(800, 1122); 111],
        &local,
        &[(54, thumbnail_bytes(&art))],
        Some(111),
    )
    .await;
    assert_eq!(c.page_index, 54);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://i.pximg.net/img-original/89861006_p54.jpg")
    );
    assert_eq!(w.requests_to("i.pximg.net"), 1);
}

#[tokio::test]
async fn test_a_huge_posts_named_page_can_be_refuted() {
    let w = world().await;
    let local = w.local(&page_art(3), (800, 1122));
    let mut c = pixiv_candidate(Some(54));
    resolve(
        &w,
        &mut c,
        &vec![(800, 1122); 111],
        &local,
        &[(54, thumbnail_bytes(&page_art(1)))],
        Some(111),
    )
    .await;
    assert_eq!(c.page_index, 0);
    assert_eq!(w.requests_to("i.pximg.net"), 1);
}

#[tokio::test]
async fn test_a_huge_post_falls_back_to_a_page_singled_out_by_size() {
    let w = world().await;
    let art = page_art(2);
    let local = w.local(&art, (1986, 2938));
    let mut sizes = vec![(3474, 6642); 60];
    sizes[41] = (1986, 2938);
    let mut c = pixiv_candidate(None);
    resolve(
        &w,
        &mut c,
        &sizes,
        &local,
        &[(41, thumbnail_bytes(&art))],
        Some(60),
    )
    .await;
    assert_eq!(c.page_index, 41);
}

#[tokio::test]
async fn test_a_huge_post_checks_the_named_page_before_the_size_match() {
    let w = world().await;
    let art = page_art(4);
    let local = w.local(&art, (800, 1122));
    let mut sizes = vec![(800, 1122); 111];
    sizes[7] = (1986, 2938);
    let mut c = pixiv_candidate(Some(54));
    let thumbs = [
        (54, thumbnail_bytes(&art)),
        (7, thumbnail_bytes(&page_art(1))),
    ];
    resolve(&w, &mut c, &sizes, &local, &thumbs, Some(111)).await;
    assert_eq!(c.page_index, 54);
    assert_eq!(w.requests_to("i.pximg.net"), 1);
}

#[tokio::test]
async fn test_a_featureless_local_image_confirms_nothing_in_a_huge_post() {
    let w = world().await;
    let flat = Canvas::new(400, 600, [128, 128, 128]);
    let local = w.local(&flat, (800, 1122));
    let mut c = pixiv_candidate(Some(54));
    resolve(
        &w,
        &mut c,
        &vec![(800, 1122); 111],
        &local,
        &[(54, thumbnail_bytes(&flat))],
        Some(111),
    )
    .await;
    assert_eq!(c.page_index, 0);
}

#[tokio::test]
async fn test_an_iqdb_candidate_is_unaffected() {
    let w = world().await;
    let mut c = pixiv_candidate(None);
    resolve(&w, &mut c, &[(900, 600); 2], "/local/file.jpg", &[], None).await;
    assert_eq!(c.page_index, 0);
}

// -- TestResolvingAMultiPhotoTweet

const TWEET: &str = "https://x.com/DARKMETAKNIGH12/status/2011168979038142698";
const API: &str = "https://api.fxtwitter.com/status/2011168979038142698";
const MEDIA: [(&str, i64, i64); 4] = [
    ("G-kbgPfa0AAXsZh", 1216, 832),
    ("G-kbgizbQAAakdJ", 1365, 2048),
    ("G-kbhHJaoAAgMek", 2048, 2048),
    ("G-kbhl1bAAAxpzn", 1171, 2048),
];

fn api_body(media: &[(&str, i64, i64)]) -> String {
    let photos: Vec<_> = media
        .iter()
        .map(|(name, w, h)| {
            json!({"type": "photo", "width": w, "height": h,
            "url": format!("https://pbs.twimg.com/media/{name}.jpg?name=orig")})
        })
        .collect();
    json!({"code": 200, "message": "OK", "tweet": {"id": "2011168979038142698",
        "author": {"screen_name": "DARKMETAKNIGH12"}, "media": {"photos": photos}}})
    .to_string()
}

fn tweet_candidate() -> Candidate {
    let (first, width, height) = MEDIA[0];
    let mut c = Candidate::new(TWEET);
    c.source_name = Some("Twitter".into());
    c.engine = "SauceNAO".into();
    c.thumb_url = Some("https://img1.saucenao.com/userdata/12345.jpg".into());
    c.direct_file_url = Some(format!("https://pbs.twimg.com/media/{first}.jpg?name=orig"));
    c.preview_url = Some(format!(
        "https://pbs.twimg.com/media/{first}.jpg?name=large"
    ));
    c.width = Some(width);
    c.height = Some(height);
    c
}

async fn resolve_tweet(
    w: &World,
    candidate: &mut Candidate,
    local: &str,
    thumbs: &[(usize, Vec<u8>)],
    media: &[(&str, i64, i64)],
    page_info: Option<&BooruPageInfo>,
) {
    w.serve(API, api_body(media).into_bytes());
    for (i, bytes) in thumbs {
        w.serve(
            &format!("https://pbs.twimg.com/media/{}.jpg?name=small", MEDIA[*i].0),
            bytes.clone(),
        );
    }
    resolve_multipage_candidate(
        &w.net,
        candidate,
        media.len() as i64,
        &w.settings,
        local,
        page_info,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_the_fourth_photo_is_used_when_it_is_the_one_that_matched() {
    let w = world().await;
    let local = w.local(&page_art(3), (1171, 2048));
    let mut c = tweet_candidate();
    resolve_tweet(&w, &mut c, &local, &thumbs_of(&art_pages(4)), &MEDIA, None).await;
    assert_eq!(c.page_index, 3);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://pbs.twimg.com/media/G-kbhl1bAAAxpzn.jpg?name=orig")
    );
    assert_eq!(
        c.preview_url.as_deref(),
        Some("https://pbs.twimg.com/media/G-kbhl1bAAAxpzn.jpg?name=large")
    );
    assert_eq!((c.width, c.height), (Some(1171), Some(2048)));
}

#[tokio::test]
async fn test_each_photo_is_found_in_turn() {
    let w = world().await;
    let thumbs = thumbs_of(&art_pages(4));
    for (index, (name, width, height)) in MEDIA.iter().enumerate() {
        let local = w.local(&page_art(index), (*width as u32, *height as u32));
        let mut c = tweet_candidate();
        resolve_tweet(&w, &mut c, &local, &thumbs, &MEDIA, None).await;
        assert_eq!(c.page_index, index as i64, "photo {index}");
        assert_eq!(
            c.direct_file_url.as_deref(),
            Some(format!("https://pbs.twimg.com/media/{name}.jpg?name=orig").as_str())
        );
    }
}

#[tokio::test]
async fn test_a_single_photo_tweet_is_left_alone() {
    let w = world().await;
    let local = w.local(&page_art(0), (1216, 832));
    let mut c = tweet_candidate();
    let before = (
        c.direct_file_url.clone(),
        c.preview_url.clone(),
        c.width,
        c.height,
    );
    resolve_tweet(&w, &mut c, &local, &[], &MEDIA[..1], None).await;
    assert_eq!(c.page_index, 0);
    assert_eq!(
        (
            c.direct_file_url.clone(),
            c.preview_url.clone(),
            c.width,
            c.height
        ),
        before
    );
    assert_eq!(
        w.requests_to("pbs.twimg.com"),
        0,
        "not one thumbnail is compared"
    );
}

#[tokio::test]
async fn test_a_tweet_none_of_whose_photos_match_keeps_the_first() {
    let w = world().await;
    let local = w.local(&page_art(4), (1216, 832));
    let mut c = tweet_candidate();
    resolve_tweet(&w, &mut c, &local, &thumbs_of(&art_pages(4)), &MEDIA, None).await;
    assert_eq!(c.page_index, 0);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://pbs.twimg.com/media/G-kbgPfa0AAXsZh.jpg?name=orig")
    );
}

#[tokio::test]
async fn test_a_url_naming_a_photo_is_overruled_by_the_images() {
    let w = world().await;
    let local = w.local(&page_art(0), (1216, 832));
    let mut c = tweet_candidate();
    let (third, width, height) = MEDIA[2];
    c.direct_file_url = Some(format!("https://pbs.twimg.com/media/{third}.jpg?name=orig"));
    c.preview_url = Some(format!(
        "https://pbs.twimg.com/media/{third}.jpg?name=large"
    ));
    c.width = Some(width);
    c.height = Some(height);
    resolve_tweet(&w, &mut c, &local, &thumbs_of(&art_pages(4)), &MEDIA, None).await;
    assert_eq!(c.page_index, 0);
    assert_eq!(
        c.direct_file_url.as_deref(),
        Some("https://pbs.twimg.com/media/G-kbgPfa0AAXsZh.jpg?name=orig")
    );
    assert_eq!((c.width, c.height), (Some(1216), Some(832)));
}

#[tokio::test]
async fn test_the_api_record_already_read_is_not_fetched_again() {
    let w = world().await;
    let local = w.local(&page_art(3), (1171, 2048));
    let mut c = tweet_candidate();
    let info = BooruPageInfo {
        fetched_url: Some(API.into()),
        body: Some(api_body(&MEDIA)),
        page_count: 4,
        ..BooruPageInfo::default()
    };
    resolve_tweet(
        &w,
        &mut c,
        &local,
        &thumbs_of(&art_pages(4)),
        &MEDIA,
        Some(&info),
    )
    .await;
    assert_eq!(c.page_index, 3);
    assert_eq!(w.requests_to("api.fxtwitter.com"), 0);
}

// -- TestPageListRequestHeaders (`remote::fetch_text`, 3.b: re-checked here because the page
//    list is the one fetch the resolver makes itself)

async fn fetch_headers(url: &str, referer: Option<&str>) -> Recorded {
    let w = world().await;
    w.serve(
        "https://www.pixiv.net/ajax/illust/89861006/pages",
        b"{}".to_vec(),
    );
    w.serve("https://danbooru.donmai.us/posts/1.json", b"{}".to_vec());
    remote::fetch_text(&w.net, url, &w.settings, referer)
        .await
        .unwrap();
    w.server.recorded().remove(0)
}

#[tokio::test]
async fn test_pixiv_page_list_is_fetched_with_the_browser_user_agent() {
    let r = fetch_headers("https://www.pixiv.net/ajax/illust/89861006/pages", None).await;
    assert!(r.header("user-agent").unwrap().contains("Mozilla/5.0"));
    assert_eq!(r.header("referer"), Some("https://www.pixiv.net/"));
}

#[tokio::test]
async fn test_an_explicit_referer_still_wins() {
    let r = fetch_headers(
        "https://www.pixiv.net/ajax/illust/89861006/pages",
        Some("https://www.pixiv.net/artworks/1"),
    )
    .await;
    assert_eq!(
        r.header("referer"),
        Some("https://www.pixiv.net/artworks/1")
    );
}

#[tokio::test]
async fn test_other_sites_keep_the_honest_user_agent() {
    let r = fetch_headers("https://danbooru.donmai.us/posts/1.json", None).await;
    assert_eq!(r.header("user-agent"), Some(USER_AGENT));
}

// -- the ticket's network checks: one deadline and one network error

#[tokio::test]
async fn the_page_list_deadline_is_a_hard_timeout_that_leaves_the_candidate_alone() {
    let pki = TestPki::generate_for(&["www.pixiv.net"]);
    // A body that trickles in a byte at a time: every read is quick, the whole is not, which is what
    // a deadline over the whole request (and not a per-read timeout) catches.
    let server = TestServer::https(&pki, |_| {
        Reply::ok(vec![b'x'; 60]).trickled(1, Duration::from_millis(60))
    })
    .await;
    let net = NetClient::new(
        NetConfig::default()
            .with_root_certificate_der(pki.ca_der.clone())
            .with_host_override("www.pixiv.net", server.addr),
    )
    .unwrap();
    let sandbox = Sandbox::new();
    let mut settings = Settings::with_defaults(&sandbox.paths);
    settings.search_timeout = 0.4;
    let mut c = pixiv_candidate(Some(1));
    let before = c.clone();
    let started = std::time::Instant::now();
    let err = resolve_multipage_candidate(&net, &mut c, 4, &settings, "/local/file.jpg", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResolveError::Raised(Raised::HardTimeout(_))),
        "{err:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the deadline is over the whole request"
    );
    assert_eq!(c, before);
}

#[tokio::test]
async fn a_dropped_connection_on_the_page_list_keeps_the_first_page() {
    let pki = TestPki::generate_for(&["www.pixiv.net"]);
    let server = TestServer::https(&pki, |_| Reply::aborted()).await;
    let net = NetClient::new(
        NetConfig::default()
            .with_root_certificate_der(pki.ca_der.clone())
            .with_host_override("www.pixiv.net", server.addr),
    )
    .unwrap();
    let sandbox = Sandbox::new();
    let settings = Settings::with_defaults(&sandbox.paths);
    let mut c = pixiv_candidate(Some(1));
    let before = c.clone();
    resolve_multipage_candidate(&net, &mut c, 4, &settings, "/local/file.jpg", None)
        .await
        .unwrap();
    assert_eq!(
        c, before,
        "a network error leaves the candidate exactly as it was"
    );
}
