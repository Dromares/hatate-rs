//! `core/hydrus_import_poll.py` and `workers/hydrus_import_poll_worker.py` without Qt (Phase 4.d,
//! DAN-2131), through `HydrusClient::new` against a scripted local Hydrus, plain http and https.
//! Time is a `ManualClock`, so the intervals and the give-up are read back as the sleeps asked
//! for. Every access key is a synthetic placeholder; no live Hydrus is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2131-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-import-url.md`.

mod common;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::clock::ManualClock;
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::HydrusClient;
use hatate_core::hydrus_import_poll::{
    DEFAULT_INTERVAL, DEFAULT_TIMEOUT, MAX_CONCURRENT_CHECKS, PollEvents, PollItem, PollOptions,
    PollWorkerOptions, RESOLVED_STATUSES, extract_confirmed_hash, poll_single_url_import,
    run_import_poll,
};
use hatate_core::model::Entry;
use hatate_core::net;
use serde_json::{Value, json};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const URL: &str = "https://www.deviantart.com/a/art/b-1";

fn hydrus_settings(url: &str) -> HydrusSettings {
    HydrusSettings {
        api_url: url.to_owned(),
        access_key: Secret::new(KEY),
        timeout: 5.0,
        ..HydrusSettings::default()
    }
}

fn json_reply(body: Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

fn in_progress() -> Reply {
    json_reply(json!({"url_file_statuses": []}))
}

fn confirmed(status: i64) -> Reply {
    json_reply(json!({"url_file_statuses": [{"status": status, "hash": HASH}]}))
}

fn error_503() -> Reply {
    Reply::new(503, "busy")
}

fn url_of(q: &Recorded) -> String {
    let query = q.target.split_once('?').map_or("", |(_, q)| q);
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// A Hydrus whose `get_url_files` answers from a script, one answer per call, the last one
/// repeating. Everything else is `{}`.
async fn scripted(script: Vec<Reply>) -> (TestServer, HydrusClient) {
    let calls = AtomicUsize::new(0);
    let server = TestServer::http(move |q| {
        if q.target.starts_with("/add_urls/get_url_files") {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            script[n.min(script.len() - 1)].clone()
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    (server, client)
}

fn polls(server: &TestServer) -> usize {
    server
        .recorded()
        .iter()
        .filter(|q| q.target.starts_with("/add_urls/get_url_files"))
        .count()
}

async fn poll(
    script: Vec<Reply>,
    timeout: f64,
    interval: f64,
) -> (Option<String>, TestServer, ManualClock) {
    let (server, client) = scripted(script).await;
    let clock = ManualClock::new(0.0, 1000.0);
    let options = PollOptions {
        timeout,
        interval,
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &clock)
        .await
        .unwrap();
    (got, server, clock)
}

// ---- extract_confirmed_hash ----

#[test]
fn the_confirmed_hash_is_read_from_the_first_resolved_status() {
    // test_urls_and_availability: the four cases.
    assert_eq!(
        extract_confirmed_hash(&json!({"url_file_statuses": []})).unwrap(),
        None
    );
    assert_eq!(
        extract_confirmed_hash(&json!({"url_file_statuses": [{"status": 2, "hash": "abc"}]}))
            .unwrap(),
        Some("abc".to_owned())
    );
    assert_eq!(
        extract_confirmed_hash(&json!({"url_file_statuses": [{"status": 0, "hash": null}]}))
            .unwrap(),
        None
    );
    assert_eq!(extract_confirmed_hash(&json!({})).unwrap(), None);
}

#[test]
fn a_hash_that_is_not_a_string_is_refused_not_passed_on() {
    // DAN-2169 condition 2 (differential class D1): Python would hand `5`, a list or `true` on as
    // the confirmed hash. A real Hydrus never sends one; here it fails closed.
    for bad in [
        json!(5),
        json!([1]),
        json!(true),
        json!({"a": 1}),
        json!(1.5),
    ] {
        let answer = json!({"url_file_statuses": [{"status": 2, "hash": bad}]});
        let err = extract_confirmed_hash(&answer).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedResponse, "{bad}");
    }
    // A falsy non-string (0, an empty list) is "no hash yet", as in Python.
    for quiet in [json!(0), json!([]), json!(false), json!("")] {
        let answer = json!({"url_file_statuses": [{"status": 2, "hash": quiet}]});
        assert_eq!(extract_confirmed_hash(&answer).unwrap(), None, "{quiet}");
    }
}

// ---- poll_single_url_import ----

#[tokio::test]
async fn an_already_confirmed_import_returns_at_once_without_sleeping() {
    let (got, server, clock) = poll(vec![confirmed(2)], 60.0, 2.0).await;
    assert_eq!(got.as_deref(), Some(HASH));
    assert_eq!(polls(&server), 1);
    assert_eq!(url_of(&server.recorded()[0]), URL);
    assert!(clock.sleeps().is_empty());
}

#[tokio::test]
async fn it_keeps_checking_until_hydrus_confirms_and_both_resolved_statuses_count() {
    let (got, server, clock) =
        poll(vec![in_progress(), in_progress(), confirmed(3)], 60.0, 2.0).await;
    assert_eq!(got.as_deref(), Some(HASH));
    assert_eq!(polls(&server), 3);
    assert_eq!(clock.sleeps(), [2.0, 2.0]);
    assert_eq!(RESOLVED_STATUSES, [2, 3]);
    assert_eq!((DEFAULT_TIMEOUT, DEFAULT_INTERVAL), (60.0, 2.0));
    // Status 0 ("ready for import") is not a confirmation, however it carries a hash.
    let zero = json_reply(json!({"url_file_statuses": [{"status": 0, "hash": HASH}]}));
    let (got, server, _) = poll(vec![zero, confirmed(2)], 60.0, 0.5).await;
    assert_eq!(got.as_deref(), Some(HASH));
    assert_eq!(polls(&server), 2);
    // A resolved status with no hash is not one either.
    let no_hash = json_reply(json!({"url_file_statuses": [{"status": 2, "hash": null}]}));
    let (got, _, _) = poll(vec![no_hash], 0.0, 2.0).await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn it_gives_up_at_the_timeout_and_the_last_sleep_never_overshoots_it() {
    // t=0 check, sleep 2, t=2 check, sleep 2, t=4 check, remaining 0: three checks, two sleeps.
    let (got, server, clock) = poll(vec![in_progress()], 4.0, 2.0).await;
    assert_eq!(got, None);
    assert_eq!(polls(&server), 3);
    assert_eq!(clock.sleeps(), [2.0, 2.0]);

    let (got, _, clock) = poll(vec![in_progress()], 5.0, 2.0).await;
    assert_eq!(got, None);
    assert_eq!(clock.sleeps(), [2.0, 2.0, 1.0]);

    // A zero timeout still checks once.
    let (got, server, clock) = poll(vec![in_progress()], 0.0, 2.0).await;
    assert_eq!(got, None);
    assert_eq!(polls(&server), 1);
    assert!(clock.sleeps().is_empty());

    // A confirmation on the very last check is still returned.
    let (got, _, _) = poll(vec![in_progress(), confirmed(2)], 2.0, 2.0).await;
    assert_eq!(got.as_deref(), Some(HASH));

    // The defaults: sixty seconds in two-second sleeps.
    let (got, _, clock) = poll(vec![in_progress()], DEFAULT_TIMEOUT, DEFAULT_INTERVAL).await;
    assert_eq!(got, None);
    assert_eq!(clock.sleeps().iter().sum::<f64>(), DEFAULT_TIMEOUT);
    assert!(clock.sleeps().iter().all(|s| *s == DEFAULT_INTERVAL));
}

#[tokio::test]
async fn progress_reports_the_remaining_seconds_and_never_goes_negative_or_runs_after_a_hit() {
    let (_server, client) = scripted(vec![in_progress()]).await;
    let clock = ManualClock::new(0.0, 1000.0);
    let reported = Mutex::new(Vec::new());
    let on_progress =
        |remaining: f64, timeout: f64| reported.lock().unwrap().push((remaining, timeout));
    let options = PollOptions {
        timeout: 6.0,
        interval: 2.0,
        on_progress: Some(&on_progress),
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &clock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        *reported.lock().unwrap(),
        [(6.0, 6.0), (4.0, 6.0), (2.0, 6.0), (0.0, 6.0)]
    );

    // An interval longer than the timeout: the remaining time is clamped at zero.
    reported.lock().unwrap().clear();
    let options = PollOptions {
        timeout: 1.0,
        interval: 10.0,
        on_progress: Some(&on_progress),
        ..PollOptions::default()
    };
    poll_single_url_import(&client, URL, &options, &ManualClock::new(0.0, 0.0))
        .await
        .unwrap();
    assert!(reported.lock().unwrap().iter().all(|(r, _)| *r >= 0.0));

    // Nothing is reported once the import is confirmed.
    let (_server, client) = scripted(vec![confirmed(2)]).await;
    reported.lock().unwrap().clear();
    let options = PollOptions {
        on_progress: Some(&on_progress),
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &clock)
        .await
        .unwrap();
    assert_eq!(got.as_deref(), Some(HASH));
    assert!(reported.lock().unwrap().is_empty());
}

#[tokio::test]
async fn progress_is_clamped_at_zero_when_a_slow_check_runs_past_the_timeout() {
    // The first check takes 100 s of the clock against a 10 s timeout: the time left is negative
    // before the clamp, and the person sees 0, not -90 (Python's `max(0.0, ...)`).
    let clock = std::sync::Arc::new(ManualClock::new(0.0, 0.0));
    let slow = clock.clone();
    let server = TestServer::http(move |_| {
        slow.advance(100.0);
        in_progress()
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let reported = Mutex::new(Vec::new());
    let on_progress =
        |remaining: f64, timeout: f64| reported.lock().unwrap().push((remaining, timeout));
    let options = PollOptions {
        timeout: 10.0,
        interval: 2.0,
        on_progress: Some(&on_progress),
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &*clock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(*reported.lock().unwrap(), [(0.0, 10.0)]);
    assert!(clock.sleeps().is_empty());
}

#[tokio::test]
async fn a_stop_ends_the_poll_without_a_request_or_part_way_and_is_not_a_failure() {
    let (server, client) = scripted(vec![confirmed(2)]).await;
    let clock = ManualClock::new(0.0, 0.0);
    let stop = || true;
    let options = PollOptions {
        stop_check: Some(&stop),
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &clock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(polls(&server), 0);
    assert!(clock.sleeps().is_empty());

    // Part way: the flag goes up during the first request.
    let (server, client) = scripted(vec![in_progress()]).await;
    let flag = AtomicBool::new(false);
    let server_ref = &server;
    let stop = || {
        let up = server_ref.request_count() >= 1;
        flag.store(up, Ordering::SeqCst);
        up
    };
    let options = PollOptions {
        timeout: 60.0,
        interval: 1.0,
        stop_check: Some(&stop),
        ..PollOptions::default()
    };
    let got = poll_single_url_import(&client, URL, &options, &clock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(polls(&server), 1);
}

#[tokio::test]
async fn a_hydrus_error_is_retried_and_errors_to_the_timeout_answer_none() {
    let (got, server, _) = poll(vec![error_503(), error_503(), confirmed(2)], 60.0, 0.1).await;
    assert_eq!(got.as_deref(), Some(HASH));
    assert_eq!(polls(&server), 3);

    let (got, _, clock) = poll(vec![error_503()], 2.0, 1.0).await;
    assert_eq!(got, None);
    assert_eq!(clock.sleeps(), [1.0, 1.0]);
}

#[tokio::test]
async fn an_answer_of_the_wrong_shape_is_not_swallowed() {
    // Only HydrusError is "busy, try again". A list where a dict belongs is Python's
    // AttributeError, and retrying it for sixty seconds would hide a real bug.
    let (_server, client) = scripted(vec![json_reply(json!([1]))]).await;
    let err = poll_single_url_import(
        &client,
        URL,
        &PollOptions::default(),
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
}

// ---- the two schemes, a timeout, a network error ----

#[tokio::test]
async fn the_poll_over_https_with_certificate_checks_on_never_confirms_and_off_it_does() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| confirmed(2)).await;

    // On: every request is refused at the handshake, which is a HydrusError the poll retries until
    // the timeout, so the answer is `None` and the server never saw a request.
    let on = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let clock = ManualClock::new(0.0, 0.0);
    let options = PollOptions {
        timeout: 2.0,
        interval: 1.0,
        ..PollOptions::default()
    };
    assert_eq!(
        poll_single_url_import(&on, URL, &options, &clock)
            .await
            .unwrap(),
        None
    );
    assert_eq!(server.request_count(), 0);
    assert_eq!(clock.sleeps(), [1.0, 1.0]);

    // Off: the self-signed server is reached.
    let mut hs = hydrus_settings(&server.url(""));
    hs.verify_ssl = false;
    let off = HydrusClient::new(&hs).unwrap();
    let got = poll_single_url_import(&off, URL, &options, &clock)
        .await
        .unwrap();
    assert_eq!(got.as_deref(), Some(HASH));
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn a_poll_that_times_out_or_loses_the_connection_is_retried_like_any_hydrus_error() {
    let slow = TestServer::http(|_| confirmed(2).delayed(std::time::Duration::from_secs(3))).await;
    let mut hs = hydrus_settings(&slow.url(""));
    hs.timeout = 0.3;
    let client = HydrusClient::new(&hs).unwrap();
    let clock = ManualClock::new(0.0, 0.0);
    let options = PollOptions {
        timeout: 1.0,
        interval: 1.0,
        ..PollOptions::default()
    };
    assert_eq!(
        poll_single_url_import(&client, URL, &options, &clock)
            .await
            .unwrap(),
        None
    );
    // The typed error behind that retry: a timeout, with a user message that leaks nothing.
    let err = client.get_url_files(URL).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Timeout));
    let user = err.user_message();
    assert!(!user.contains(KEY) && !user.contains("deviantart") && !user.contains("os error"));

    let dropped = TestServer::http(|_| Reply::aborted()).await;
    let client = HydrusClient::new(&hydrus_settings(&dropped.url(""))).unwrap();
    let err = client.get_url_files(URL).await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Net(_)));
    assert_eq!(
        poll_single_url_import(&client, URL, &options, &ManualClock::new(0.0, 0.0))
            .await
            .unwrap(),
        None
    );
}

// ---- the worker ----

#[derive(Default)]
struct Events {
    resolved: Vec<(usize, Option<String>, Option<String>)>,
    countdowns: Vec<(String, f64, f64)>,
    finished: usize,
}

impl PollEvents for Events {
    fn entry_resolved(&mut self, index: usize, hash: Option<&str>, warning: Option<&str>) {
        self.resolved
            .push((index, hash.map(str::to_owned), warning.map(str::to_owned)));
    }
    fn wait_countdown(&mut self, label: &str, remaining: f64, total: f64) {
        self.countdowns.push((label.to_owned(), remaining, total));
    }
    fn finished_all(&mut self) {
        self.finished += 1;
    }
}

/// A Hydrus that answers `get_url_files` by URL: `answers[url]` is the round it confirms in
/// (0-based), absent means never.
async fn hydrus_by_url(
    confirms_in_round: Vec<(&'static str, usize)>,
) -> (TestServer, HydrusClient) {
    let seen: Mutex<std::collections::HashMap<String, usize>> = Mutex::default();
    let server = TestServer::http(move |q| {
        if !q.target.starts_with("/add_urls/get_url_files") {
            return json_reply(json!({}));
        }
        let url = url_of(q);
        let mut seen = seen.lock().unwrap();
        let round = *seen.entry(url.clone()).and_modify(|n| *n += 1).or_insert(0);
        match confirms_in_round.iter().find(|(u, _)| *u == url) {
            Some((_, at)) if round >= *at => confirmed(2),
            _ => in_progress(),
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    (server, client)
}

fn entry(name: &str) -> Entry {
    let mut e = Entry::new(format!("/nonexistent-dan2131/{name}.png"));
    e.hydrus_hash = Some("b".repeat(64));
    e.sent_to_hydrus = true;
    e
}

fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    f(&mut s);
    s
}

#[tokio::test]
async fn the_worker_resolves_each_entry_when_it_confirms_or_when_its_own_timeout_passes() {
    let a = entry("a");
    let b = entry("b");
    let c = entry("c");
    let items = [
        PollItem {
            entry: &a,
            url: "https://x.test/a".into(),
        },
        PollItem {
            entry: &b,
            url: "https://x.test/b".into(),
        },
        PollItem {
            entry: &c,
            url: "https://x.test/c".into(),
        },
    ];
    // a confirms in the first round, b in the third, c never.
    let (server, client) =
        hydrus_by_url(vec![("https://x.test/a", 0), ("https://x.test/b", 2)]).await;
    let clock = ManualClock::new(0.0, 0.0);
    let mut events = Events::default();
    let options = PollWorkerOptions {
        timeout: 4.0,
        interval: 2.0,
        settings: None,
    };

    run_import_poll(&client, &items, &options, &|| false, &mut events, &clock)
        .await
        .unwrap();

    assert_eq!(
        events.resolved,
        [
            (0, Some(HASH.to_owned()), None),
            (1, Some(HASH.to_owned()), None),
            (2, None, None)
        ]
    );
    // Round 1 (t=0): a. Sleep 2. Round 2 (t=2): b and c still pending. Sleep 2. Round 3 (t=4):
    // b confirms; c has run out of time.
    assert_eq!(
        events.countdowns,
        [
            ("Confirming 2 Hydrus import(s)".to_owned(), 2.0, 2.0),
            ("Confirming 2 Hydrus import(s)".to_owned(), 2.0, 2.0),
            (String::new(), 0.0, 0.0),
        ]
    );
    assert_eq!(clock.sleeps(), [2.0, 2.0]);
    assert_eq!(events.finished, 1);
    // 3 + 2 + 2 checks.
    assert_eq!(
        server
            .recorded()
            .iter()
            .filter(|q| q.target.starts_with("/add_urls/get_url_files"))
            .count(),
        7
    );
}

#[tokio::test]
async fn a_stop_emits_nothing_more_and_still_clears_the_countdown_and_finishes() {
    let a = entry("a");
    let items = [PollItem {
        entry: &a,
        url: "https://x.test/a".into(),
    }];
    let (server, client) = hydrus_by_url(vec![("https://x.test/a", 0)]).await;
    let mut events = Events::default();
    run_import_poll(
        &client,
        &items,
        &PollWorkerOptions::default(),
        &|| true,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap();
    assert!(events.resolved.is_empty());
    assert_eq!(events.countdowns, [(String::new(), 0.0, 0.0)]);
    assert_eq!(events.finished, 1);
    assert_eq!(server.request_count(), 0);

    // Stop raised while the checks are in flight: their results are ignored.
    let stop_after_first = AtomicBool::new(false);
    let server_seen = &server;
    let stop = || {
        let up = server_seen.request_count() >= 1;
        stop_after_first.store(up, Ordering::SeqCst);
        up
    };
    let mut events = Events::default();
    run_import_poll(
        &client,
        &items,
        &PollWorkerOptions::default(),
        &stop,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap();
    assert!(events.resolved.is_empty(), "{:?}", events.resolved);
    assert_eq!(events.finished, 1);
}

#[tokio::test]
async fn at_most_eight_checks_are_in_flight_at_once() {
    assert_eq!(MAX_CONCURRENT_CHECKS, 8);
    let entries: Vec<Entry> = (0..20).map(|i| entry(&format!("e{i}"))).collect();
    let items: Vec<PollItem> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| PollItem {
            entry: e,
            url: format!("https://x.test/{i}"),
        })
        .collect();
    let live = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let server = TestServer::http(move |q| {
        if q.target.starts_with("/add_urls/get_url_files") {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            live.fetch_sub(1, Ordering::SeqCst);
            // Report the peak through the body of the last answer.
            return json_reply(json!({"url_file_statuses": [{"status": 2, "hash": HASH}],
                                      "peak": peak.load(Ordering::SeqCst)}));
        }
        json_reply(json!({}))
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let mut events = Events::default();
    run_import_poll(
        &client,
        &items,
        &PollWorkerOptions::default(),
        &|| false,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap();
    // All twenty resolved, in the order given.
    let order: Vec<usize> = events.resolved.iter().map(|r| r.0).collect();
    assert_eq!(order, (0..20).collect::<Vec<_>>());
    assert_eq!(server.request_count(), 20);
}

#[tokio::test]
async fn a_confirmed_import_is_related_before_it_is_reported_and_off_costs_nothing() {
    // test_url_importer_duplicate_relationships.TestBatchPollWorkerWiring: the settings decide.
    let a = entry("a");
    let items = [PollItem {
        entry: &a,
        url: "https://x.test/a".into(),
    }];
    let (server, client) = hydrus_by_url(vec![("https://x.test/a", 0)]).await;
    let mut events = Events::default();
    let off = settings_with(|s| s.set_hydrus_duplicate_relationships = false);
    for settings in [None, Some(&off)] {
        run_import_poll(
            &client,
            &items,
            &PollWorkerOptions {
                settings,
                ..PollWorkerOptions::default()
            },
            &|| false,
            &mut events,
            &ManualClock::new(0.0, 0.0),
        )
        .await
        .unwrap();
    }
    assert_eq!(server.request_count(), 2, "only the two status checks");
    assert!(events.resolved.iter().all(|r| r.2.is_none()));

    // On, with a local file that cannot be read back: the warning rides on the signal.
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let mut events = Events::default();
    run_import_poll(
        &client,
        &items,
        &PollWorkerOptions {
            settings: Some(&on),
            ..PollWorkerOptions::default()
        },
        &|| false,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap();
    let (_, hash, warning) = &events.resolved[0];
    assert_eq!(hash.as_deref(), Some(HASH));
    assert!(
        warning
            .as_deref()
            .unwrap()
            .starts_with("Imported, but not marked as a duplicate"),
        "{warning:?}"
    );
}

#[tokio::test]
async fn a_non_string_hash_in_the_worker_writes_nothing_and_reports_nothing() {
    // DAN-2169 condition 2, through the worker: the error leaves `run_import_poll` before the
    // relationship bookkeeping, the entry's signal or the finish signal, and Hydrus is asked
    // nothing but the one status check.
    let a = entry("a");
    let items = [PollItem {
        entry: &a,
        url: "https://x.test/a".into(),
    }];
    let server = TestServer::http(|q| {
        if q.target.starts_with("/add_urls/get_url_files") {
            json_reply(json!({"url_file_statuses": [{"status": 2, "hash": 5}]}))
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let mut events = Events::default();
    let err = run_import_poll(
        &client,
        &items,
        &PollWorkerOptions {
            settings: Some(&on),
            ..PollWorkerOptions::default()
        },
        &|| false,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert!(events.resolved.is_empty());
    assert_eq!(events.finished, 0);
    let targets: Vec<String> = server.recorded().iter().map(|q| q.target.clone()).collect();
    assert_eq!(targets.len(), 1, "{targets:?}");
    assert!(targets[0].starts_with("/add_urls/get_url_files"));
    assert_eq!(
        a.hydrus_hash,
        Some("b".repeat(64)),
        "the entry is untouched"
    );
    assert!(a.sent_to_hydrus);
}

#[tokio::test]
async fn bookkeeping_that_goes_wrong_does_not_take_the_confirmation_down() {
    // test_unexpected_failure_does_not_take_the_confirmation_down: an answer of the wrong shape
    // from the relationship lookup is Python's AttributeError, caught by `_relate`.
    let a = entry("a");
    let items = [PollItem {
        entry: &a,
        url: "https://x.test/a".into(),
    }];
    let server = TestServer::http(|q| {
        if q.target.starts_with("/add_urls/get_url_files") {
            confirmed(2)
        } else {
            json_reply(json!([1]))
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let mut events = Events::default();
    run_import_poll(
        &client,
        &items,
        &PollWorkerOptions {
            settings: Some(&on),
            ..PollWorkerOptions::default()
        },
        &|| false,
        &mut events,
        &ManualClock::new(0.0, 0.0),
    )
    .await
    .unwrap();
    assert_eq!(events.resolved, [(0, Some(HASH.to_owned()), None)]);
}
