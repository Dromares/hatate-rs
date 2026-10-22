//! `NetConfig::with_host_override` and `NetRequest::with_probe` (DAN-2058): what the Phase 3
//! network halves stand on. A request for a site's real URL reaches a local https server, which
//! sees the real `Host` and the real path; and a probed body read stops where Python's
//! `iter_content` loop would.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Reply, TestPki, TestServer};
use hatate_core::net::{BodyProbe, NetClient, NetConfig, NetRequest};

const T: Duration = Duration::from_secs(10);

fn client_for(pki: &TestPki, server: &TestServer, hosts: &[&str]) -> NetClient {
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, server.addr);
    }
    NetClient::new(cfg).unwrap()
}

#[tokio::test]
async fn a_real_url_reaches_the_local_server_with_its_own_host_and_path() {
    let pki = TestPki::generate_for(&["www.reddit.com", "file.pawchive.pw"]);
    let server = TestServer::https(&pki, |_| Reply::ok("feed")).await;
    let client = client_for(&pki, &server, &["www.reddit.com", "file.pawchive.pw"]);

    let resp = client
        .send(NetRequest::get("https://www.reddit.com/r/x/new/.rss?limit=100", T).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.text(), "feed");
    assert_eq!(
        resp.url.as_str(),
        "https://www.reddit.com/r/x/new/.rss?limit=100"
    );
    client
        .send(NetRequest::get("https://file.pawchive.pw/data/a.png", T).unwrap())
        .await
        .unwrap();

    let seen = server.recorded();
    assert_eq!(seen[0].header("host"), Some("www.reddit.com"));
    assert_eq!(seen[0].target, "/r/x/new/.rss?limit=100");
    assert_eq!(seen[1].header("host"), Some("file.pawchive.pw"));
    // Each name has its own pooled connection: the handshake names the host it is for.
    assert_eq!(
        server.connections.load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

#[tokio::test]
async fn a_host_with_no_override_is_not_redirected() {
    let pki = TestPki::generate_for(&["www.reddit.com"]);
    let server = TestServer::https(&pki, |_| Reply::ok("never")).await;
    let client = client_for(&pki, &server, &["www.reddit.com"]);
    // `localhost` resolves to this machine but is not overridden: it keeps its own port.
    let resp = client
        .send(NetRequest::get(&server.url_localhost("/p"), T).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.text(), "never");
    assert_eq!(
        server.recorded()[0]
            .header("host")
            .unwrap()
            .split(':')
            .next(),
        Some("localhost")
    );
}

fn probe(chunk: usize, cap: usize, stop_at: usize) -> BodyProbe {
    BodyProbe {
        chunk,
        cap,
        done: Arc::new(move |taken: &[u8]| taken.len() >= stop_at),
    }
}

#[tokio::test]
async fn a_probed_read_takes_whole_blocks_and_stops_at_the_first_one_that_satisfies_it() {
    let pki = TestPki::generate_for(&["f.example"]);
    let body: Vec<u8> = (0..100u32).map(|i| i as u8).collect();
    let served = body.clone();
    let server = TestServer::https(&pki, move |_| Reply::ok(served.clone())).await;
    let client = client_for(&pki, &server, &["f.example"]);

    // Blocks of 16; satisfied once 20 bytes are in: the second block is the one that does it.
    let req = NetRequest::get("https://f.example/x", T)
        .unwrap()
        .with_probe(probe(16, 64, 20));
    let resp = client.send(req).await.unwrap();
    assert_eq!(resp.body.as_ref(), &body[..32]);

    // Never satisfied: stops at the cap, a whole number of blocks.
    let req = NetRequest::get("https://f.example/x", T)
        .unwrap()
        .with_probe(probe(16, 64, 1000));
    assert_eq!(client.send(req).await.unwrap().body.as_ref(), &body[..64]);

    // Shorter than a block: the remainder is taken at the end of the body.
    let req = NetRequest::get("https://f.example/x", T)
        .unwrap()
        .with_probe(probe(400, 4000, 1000));
    assert_eq!(client.send(req).await.unwrap().body.as_ref(), &body[..]);
}

#[tokio::test]
async fn a_probed_read_waits_for_a_whole_block_as_iter_content_does() {
    // A server that sends 10 bytes and stalls: with a block of 16, nothing is taken until the
    // timeout. (Python: `iter_content(16)` blocks for the other 6 bytes and raises ReadTimeout.)
    let pki = TestPki::generate_for(&["f.example"]);
    let server = TestServer::https(&pki, |_| {
        Reply::ok(vec![7u8; 20]).trickled(10, Duration::from_secs(5))
    })
    .await;
    let client = client_for(&pki, &server, &["f.example"]);
    let req = NetRequest::get("https://f.example/x", Duration::from_millis(300))
        .unwrap()
        .with_probe(probe(16, 64, 1));
    let err = client.send(req).await.unwrap_err();
    assert_eq!(err.kind(), hatate_core::net::ErrorKind::Timeout, "{err}");
}
