//! DAN-2099: what `LiveEngines` hands each client, checked at the request. Each test builds
//! `LiveEngines` over the binaries' constructor (`NetClient::new`: rustls with the explicit
//! CryptoProvider), swaps one client for one pointed at a local **https** server, calls the
//! engine through the `Engines` trait with values no default shares, and reads the value off the
//! request the server saw. No live network. The keys are synthetic.
//!
//! The wiring each test pins (a wrong value there is silent in the shipped build):
//! IQDB's 3D flag, SauceNAO's settings, Google Images' API key, Pawchive's hash, trace.moe's
//! `min_similarity`, and that ascii2d and Yandex answer from the host they were given.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{RecordedExt, ReplayServer, Reply, TestPki};
use hatate_core::clock::ManualClock;
use hatate_core::config::{SauceNaoSettings, Secret};
use hatate_core::engine_backends::LiveEngines;
use hatate_core::engine_runner::Engines;
use hatate_core::engines::ascii2d::Ascii2dClient;
use hatate_core::engines::google_images::GoogleImagesClient;
use hatate_core::engines::iqdb::IqdbClient;
use hatate_core::engines::pawchive_lookup::PawchiveLookupClient;
use hatate_core::engines::saucenao::SauceNaoClient;
use hatate_core::engines::tracemoe::TraceMoeClient;
use hatate_core::engines::yandex::YandexClient;
use hatate_core::net::{NetClient, NetConfig};
use serde_json::{Value, json};

const SYNTHETIC_KEY: &str = "synthetic-test-key-0000";
const UPLOAD: &[u8] = b"a small picture";
/// `sha256(UPLOAD)`, as `sha256sum` prints it.
const UPLOAD_SHA256: &str = "b00a9cace55a8ed87bbe3ff72f9f47d84698927284ea60ccad8cae0fc070abdc";

const IQDB_PAGE: &str = "<html><head><title>iqdb</title></head><body><div id=\"pages\">\
<table><tr><th>Best match</th></tr><tr>\
<td class=\"image\"><a href=\"https://danbooru.donmai.us/posts/77\"><img src=\"/thumbs/a.jpg\"></a></td>\
<td>1200x1600</td><td>97% similarity</td></tr></table></div></body></html>";

fn net(pki: &TestPki) -> NetClient {
    NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone())).unwrap()
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_700_000_000.0, 100.0))
}

fn live(pki: &TestPki) -> LiveEngines {
    LiveEngines::new(net(pki), clock())
}

struct Image {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn image() -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2099-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("shot.png");
    std::fs::write(&path, UPLOAD).unwrap();
    Image { _dir: dir, path }
}

/// A real picture (Yandex decodes and re-encodes it).
fn picture() -> Image {
    let dir = tempfile::Builder::new()
        .prefix("hatate-dan2099-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("shot.png");
    image::RgbImage::from_fn(300, 200, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    })
    .save(&path)
    .unwrap();
    Image { _dir: dir, path }
}

fn html(body: &str) -> Reply {
    Reply::ok(body).header("Content-Type", "text/html; charset=UTF-8")
}

fn json_reply(body: &Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

#[tokio::test]
async fn iqdb_three_d_flag_picks_the_3d_index() {
    let pki = TestPki::generate();
    let file = image();
    let server = ReplayServer::https(&pki, vec![html(IQDB_PAGE), html(IQDB_PAGE)]).await;
    let mut engines = live(&pki);
    engines.iqdb = IqdbClient::with_hosts(net(&pki), &server.url("/2d/"), &server.url("/3d/"));

    let three = engines.iqdb(&file.path, 30.0, None, true).await.unwrap();
    let two = engines.iqdb(&file.path, 30.0, None, false).await.unwrap();

    assert_eq!(three.len(), 1);
    assert_eq!(two.len(), 1);
    let paths: Vec<String> = server.recorded().iter().map(|r| r.path()).collect();
    assert_eq!(paths, ["/3d/", "/2d/"]);
}

#[tokio::test]
async fn saucenao_gets_the_key_and_database_of_the_settings_it_was_given() {
    let pki = TestPki::generate();
    let file = image();
    let body = json!({
        "header": {"status": 0, "short_remaining": 5, "short_limit": 6,
                   "long_remaining": 99, "long_limit": 100},
        "results": [{"header": {"similarity": "91.5", "index_name": "Index #5: Pixiv"},
                     "data": {"ext_urls": ["https://www.pixiv.net/artworks/1"], "title": "t"}}],
    });
    let server = ReplayServer::https(&pki, vec![json_reply(&body)]).await;
    let mut engines = live(&pki);
    engines.saucenao =
        SauceNaoClient::with_api_url(net(&pki), &server.url("/search.php")).with_clock(clock());
    // Neither the key nor the database is a default, and a default would take the HTML page.
    let settings = SauceNaoSettings {
        api_key: SYNTHETIC_KEY.into(),
        dbmask: 5,
        min_similarity: 90.0,
        ..SauceNaoSettings::default()
    };

    let matches = engines
        .saucenao(&file.path, &settings, 30.0, None, None)
        .await
        .unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(
        server.recorded()[0].query(),
        [
            ("output_type".to_owned(), "2".to_owned()),
            ("api_key".to_owned(), SYNTHETIC_KEY.to_owned()),
            ("db".to_owned(), "5".to_owned()),
        ]
    );
}

#[tokio::test]
async fn ascii2d_uploads_to_the_host_it_was_given() {
    let pki = TestPki::generate();
    let file = image();
    let result_page = "<html><head><title>ascii2d</title></head><body>\
<div class=\"item-box\"><div class=\"detail-box\">\
<a href=\"https://www.pixiv.net/artworks/12345\">pixiv</a></div></div></body></html>";
    let server = ReplayServer::https(
        &pki,
        vec![
            Reply::new(302, "")
                .header("Location", "/search/color/aabbcc")
                .header("Set-Cookie", "_session_id=s1; Path=/; HttpOnly"),
            html(result_page),
            html(result_page),
        ],
    )
    .await;
    let mut engines = live(&pki);
    engines.ascii2d = Ascii2dClient::with_base(net(&pki), &server.url("/"));

    let matches = engines.ascii2d(&file.path, 30.0, None).await.unwrap();

    assert_eq!(matches[0].url, "https://www.pixiv.net/artworks/12345");
    let first = &server.recorded()[0];
    assert_eq!(
        (first.method.as_str(), first.path().as_str()),
        ("POST", "/search/file")
    );
    assert_eq!(first.form_parts()[0].data, UPLOAD);
}

#[tokio::test]
async fn trace_moe_drops_what_is_below_the_min_similarity_it_was_given() {
    let pki = TestPki::generate();
    let file = image();
    let hit = |id: u64, similarity: f64| {
        json!({"anilist": {"id": id, "title": {"romaji": "One Piece"}},
               "similarity": similarity, "episode": 3, "image": "https://example.test/t.jpg"})
    };
    let body = json!({"result": [hit(21, 0.97), hit(22, 0.50)]});
    let server = ReplayServer::https(&pki, vec![json_reply(&body)]).await;
    let mut engines = live(&pki);
    engines.tracemoe = TraceMoeClient::with_host(net(&pki), &server.url("/search"));

    let matches = engines
        .tracemoe(&file.path, 30.0, None, 90.0)
        .await
        .unwrap();

    assert_eq!(
        matches.iter().map(|m| m.url.as_str()).collect::<Vec<_>>(),
        ["https://anilist.co/anime/21"]
    );
    assert_eq!(server.recorded().len(), 1);
}

#[tokio::test]
async fn google_images_sends_the_api_key_it_was_given() {
    let pki = TestPki::generate();
    let file = image();
    let body = json!({"responses": [{"webDetection": {}}]});
    let server = ReplayServer::https(&pki, vec![json_reply(&body)]).await;
    let mut engines = live(&pki);
    engines.google_images = GoogleImagesClient::with_urls(
        net(&pki),
        &server.url("/searchbyimage/upload"),
        &server.url("/v1/images:annotate"),
    );
    let key = Secret::from(SYNTHETIC_KEY);

    engines
        .google_images(&file.path, 30.0, None, &key)
        .await
        .unwrap();

    let seen = &server.recorded()[0];
    assert_eq!(seen.path(), "/v1/images:annotate");
    assert_eq!(seen.query(), [("key".to_owned(), SYNTHETIC_KEY.to_owned())]);
}

#[tokio::test]
async fn yandex_uploads_to_the_host_it_was_given() {
    let pki = TestPki::generate();
    let file = picture();
    let upload_ok = json_reply(&json!({"blocks": [{"params": {"cbirId": "1617445/abc"}}]}));
    let state = json!({"initialState": {"cbirSites": {"sites": [{
        "title": "t",
        "url": "https://danbooru.donmai.us/posts/6307169?utm_medium=organic",
        "domain": "danbooru.donmai.us",
        "thumb": {"url": "//avatars.mds.yandex.net/i?id=abc&n=13", "width": 148, "height": 90},
        "originalImage": {"url": "https://img.example/1.jpg", "width": 1000, "height": 1400},
    }]}}})
    .to_string()
    .replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;");
    let page = format!("<html><div class=\"CbirSites\" data-state=\"{state}\"></div></html>");
    let server = ReplayServer::https(
        &pki,
        vec![
            upload_ok.header("Set-Cookie", "yandexuid=u1; Path=/"),
            html(&page),
        ],
    )
    .await;
    let mut engines = live(&pki);
    engines.yandex =
        YandexClient::with_search_url(net(&pki), &server.url("/images/search")).with_clock(clock());

    let matches = engines.yandex(&file.path, 30.0, None).await.unwrap();

    assert_eq!(matches[0].url, "https://danbooru.donmai.us/posts/6307169");
    let first = &server.recorded()[0];
    assert_eq!(
        (first.method.as_str(), first.path().as_str()),
        ("POST", "/images/search")
    );
}

#[tokio::test]
async fn pawchive_looks_up_the_hash_it_was_given() {
    let pki = TestPki::generate();
    let file = image();
    let server = ReplayServer::https(&pki, vec![Reply::new(404, "")]).await;
    let mut engines = live(&pki);
    engines.pawchive = PawchiveLookupClient::with_host(net(&pki), &server.url("/lookup/"));

    // The hash the runner passes on is the one `file_sha256` makes of the file.
    let sha = engines.file_sha256(&file.path).await.unwrap();
    assert_eq!(sha, UPLOAD_SHA256);
    let matches = engines.pawchive_lookup(&sha, 30.0).await.unwrap();

    assert!(matches.is_empty());
    let seen = server.recorded();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path(), format!("/lookup/{UPLOAD_SHA256}"));
}
