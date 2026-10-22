//! The corpus items whose Python run needed a socket, run end to end (DAN-2058, Phase 3.b): the
//! Rust client, through `NetClient::new` (rustls with the explicit provider), against a local
//! https server that serves the item's scripted answers.
//!
//! * `core.reddit._feed_entries`: the four `mocked_transport` items. The server serves each item's
//!   recorded responses; the request it sees is the recorded one, and the result and the session
//!   cache are `expected.json`'s.
//! * The 14 items that list `skipped_network_callables` (`pawchive.parse` x5,
//!   `pawchive.parse_dimensions` x10 and `redditpost.parse_dimensions` x2: 17 callable runs). The
//!   corpus has no recorded answer for these, so `tests/data/net_replay_pawchive_reddit.json`
//!   holds two synthetic answer sets per run (`ok`, `down`) and, for each, the request the real
//!   Python (`76c85c4`) made and the value it returned with that answer. The answers are a
//!   generated 321x123 PNG and a made-up creator name; nothing comes from a live site.
//!
//! The Phase 2 corpus runner (`corpus_diff.rs`) is unchanged and still skips these callables.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{Recorded, Reply, TestPki, TestServer};
use hatate_core::boorus::fetch::{self, BooruSession};
use hatate_core::boorus::{Context, PARSERS};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::reddit::{self, FeedCache};
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

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

fn host_of(url: &str) -> &str {
    url.trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap()
}

/// What the server saw of a request, in the shape the oracle recorded: the headers Python sets
/// explicitly (`requests` adds `Accept`, `Accept-Encoding`, `Connection`, which are not compared).
fn seen(r: &Recorded) -> Value {
    let mut headers = serde_json::Map::new();
    for name in ["user-agent", "range", "referer"] {
        if let Some(v) = r.header(name) {
            headers.insert(name.into(), v.into());
        }
    }
    json!({
        "method": r.method,
        "url": format!("https://{}{}", r.header("host").unwrap_or(""), r.target),
        "headers": headers,
    })
}

#[tokio::test]
async fn the_four_feed_entries_items_replay_with_the_recorded_request_and_result() {
    let dir = repo_root().join("corpus/engines/reddit.feed_entries");
    let mut ran = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let item = entry.unwrap().path();
        let call = read_json(&item.join("call.json"));
        let expected = read_json(&item.join("expected.json"));
        let want = &expected["results"]["core.reddit._feed_entries"];
        let responses = call["responses"].as_array().unwrap();
        assert_eq!(responses.len(), 1, "{item:?}");
        let status = responses[0]["status"].as_u64().unwrap() as u16;
        let body = std::fs::read(item.join("input.xml")).unwrap();
        let w = world(&["www.reddit.com"], move |_| {
            Reply::new(status, body.clone())
        })
        .await;
        let cache = FeedCache::new();
        let (subreddit, feed) = (
            call["args"]["subreddit"].as_str().unwrap(),
            call["args"]["feed"].as_str().unwrap(),
        );
        let got = reddit::fetch_feed_entries(
            &w.client,
            &cache,
            subreddit,
            feed,
            call["args"]["timeout"].as_f64().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(json!(got), want["returns"], "{item:?}");
        assert_eq!(
            json!(cache.keys()),
            want["effects"]["state_after"]["cached_keys"],
            "{item:?}"
        );
        // The recorded request is the first one on the wire. The item was recorded with
        // `core.net.get` replaced, above the net layer's one retry of a 429 (`sleeps` is empty
        // there); the Rust client has its real net layer, so a 429 is asked for once more.
        let recorded = &call["request"][0];
        let wire = w.server.recorded();
        assert_eq!(seen(&wire[0])["url"], recorded["url"], "{item:?}");
        assert_eq!(seen(&wire[0])["method"], recorded["method"], "{item:?}");
        assert!(wire[0].header("user-agent").is_some(), "{item:?}");
        let expected_wire = if status == 429 { 2 } else { 1 };
        assert_eq!(wire.len(), expected_wire, "{item:?}");
        assert!(wire.iter().all(|r| r.target == wire[0].target), "{item:?}");
        ran += 1;
    }
    assert_eq!(ran, 4);
}

#[tokio::test]
async fn the_14_skipped_items_run_end_to_end_and_make_the_request_python_makes() {
    let cases = read_json(
        &repo_root().join("crates/hatate-core/tests/data/net_replay_pawchive_reddit.json"),
    );
    let cases = cases.as_array().unwrap();
    let items: std::collections::BTreeSet<_> =
        cases.iter().map(|c| c["item"].as_str().unwrap()).collect();
    assert_eq!(
        items.len(),
        14,
        "the corpus has 14 items with skipped_network_callables"
    );
    let runs: std::collections::BTreeSet<_> = cases
        .iter()
        .map(|c| (c["item"].as_str().unwrap(), c["callable"].as_str().unwrap()))
        .collect();
    assert_eq!(
        runs.len(),
        17,
        "pawchive.parse x5, pawchive.parse_dimensions x10, redditpost.parse_dimensions x2"
    );
    for case in cases {
        let label = format!("{} {} {}", case["item"], case["callable"], case["variant"]);
        let item = repo_root().join(case["dir"].as_str().unwrap());
        let call = read_json(&item.join("call.json"));
        let input = std::fs::read_dir(&item)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("input.")
            })
            .unwrap();
        let body = String::from_utf8(std::fs::read(input).unwrap()).unwrap();
        let url = call["extra_args"][0].as_str().unwrap();

        let answers: Arc<Vec<(String, Reply)>> = Arc::new(
            case["answers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    let mut reply = Reply::new(
                        a["status"].as_u64().unwrap() as u16,
                        unhex(a["body_hex"].as_str().unwrap()),
                    );
                    for (k, v) in a["headers"].as_object().unwrap() {
                        reply = reply.header(k, v.as_str().unwrap());
                    }
                    (a["when"].as_str().unwrap().to_string(), reply)
                })
                .collect(),
        );
        let mut hosts: Vec<&str> = case["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| host_of(r["url"].as_str().unwrap()))
            .collect();
        hosts.sort_unstable();
        hosts.dedup();
        let table = Arc::clone(&answers);
        let w = world(&hosts, move |req| {
            let full = format!("https://{}{}", req.header("host").unwrap_or(""), req.target);
            table
                .iter()
                .find(|(when, _)| full.contains(when.as_str()))
                .map_or_else(|| Reply::new(404, "unscripted"), |(_, r)| r.clone())
        })
        .await;

        let session = BooruSession::new();
        for (key, name) in call["context"]["artist_cache"]
            .as_object()
            .into_iter()
            .flatten()
        {
            session.seed_artist(key, name.as_str().unwrap());
        }
        let name = case["callable"].as_str().unwrap();
        // By the callable's module: an item may carry a URL no parser claims (pawchive-017).
        let module = name.split('.').nth(2).unwrap();
        let parser = PARSERS
            .iter()
            .copied()
            .find(|p| p.name == module)
            .unwrap_or_else(|| panic!("{label}: no parser named {module}"));
        let ctx: Context = fetch::network_context(&w.client, &session, parser, &body, url).await;
        let got = if name.ends_with(".parse") {
            json!((parser.parse)(&body, url, &ctx))
        } else {
            let (width, height) = (parser.parse_dimensions.expect("registered"))(&body, url, &ctx);
            json!([width, height])
        };
        assert_eq!(json!({"returns": got}), case["returns"], "{label}");

        let wire: Vec<Value> = w.server.recorded().iter().map(seen).collect();
        assert_eq!(Value::Array(wire), case["requests"], "{label}");
    }
}
