//! DAN-2501: the shipped probe fetch ([`LiveFetch`]) over https, to a local server, through the
//! binary's own constructor (`NetClient::new`, rustls with the explicit provider). Site hosts are
//! reached through `NetConfig::with_host_override`; no real site is touched. The verifier names
//! `the_live_probe_reads_a_post_over_https` (the network-client rule in `CLAUDE.md`).

mod common;

use std::cell::RefCell;

use common::{Reply, TestPki, TestServer};
use hatate_core::boorus::fetch::BooruSession;
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::parser_probe::{self, LiveFetch};

const DANBOORU_POST: &str = r#"{"id": 1000000, "image_width": 3, "image_height": 4, "rating": "q",
    "file_url": "https://cdn.donmai.us/f.png", "large_file_url": "https://cdn.donmai.us/l.jpg",
    "tag_string_general": "x y", "tag_string_artist": "someone"}"#;

async fn client_for(host: &str, server: &TestServer, pki: &TestPki) -> NetClient {
    let cfg = NetConfig::default()
        .with_root_certificate_der(pki.ca_der.clone())
        .with_host_override(host, server.addr);
    NetClient::new(cfg).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_live_probe_reads_a_post_over_https() {
    let pki = TestPki::generate_for(&["danbooru.donmai.us"]);
    let server = TestServer::https(&pki, |req| {
        if req.target.contains("/posts/1000000.json") {
            Reply::ok(DANBOORU_POST).header("Content-Type", "application/json")
        } else {
            Reply::new(404, "no route")
        }
    })
    .await;
    let net = client_for("danbooru.donmai.us", &server, &pki).await;
    // The probe blocks its thread (it runs on a worker thread in the app), so run it off the
    // test's runtime.
    let results = tokio::task::spawn_blocking(move || {
        let fetch = LiveFetch::new(net, BooruSession::new()).unwrap();
        let seen = RefCell::new(Vec::new());
        let results = parser_probe::run(
            &fetch,
            10.0,
            &|d, t, s| seen.borrow_mut().push(format!("{d}/{t} {s}")),
            &["danbooru"],
        );
        (results, seen.into_inner())
    })
    .await
    .unwrap();
    let (results, seen) = results;
    assert_eq!(seen, ["0/1 danbooru", "1/1 "]);
    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(r.ok, "{:?}", r.problem);
    assert_eq!((r.tags, r.width, r.height), (3, Some(3), Some(4)));
    assert!(r.has_file_url);
    // The probe asked the site the way a search does: the data API, once.
    let seen = server.recorded();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].header("host"), Some("danbooru.donmai.us"));
    assert_eq!(seen[0].target, "/posts/1000000.json");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_that_answers_500_is_a_failed_probe_not_a_panic() {
    let pki = TestPki::generate_for(&["danbooru.donmai.us"]);
    let server = TestServer::https(&pki, |_| Reply::new(500, "down")).await;
    let net = client_for("danbooru.donmai.us", &server, &pki).await;
    let results = tokio::task::spawn_blocking(move || {
        let fetch = LiveFetch::new(net, BooruSession::new()).unwrap();
        parser_probe::run(&fetch, 10.0, &|_, _, _| {}, &["danbooru"])
    })
    .await
    .unwrap();
    assert!(!results[0].ok);
    assert!(!results[0].summary().is_empty());
}
