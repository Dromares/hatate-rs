//! `core/hydrus_reconcile.py` and `workers/hydrus_reconcile_worker.py` without Qt (Phase 4.d,
//! DAN-2131): settling entries stuck at "Queued" by asking Hydrus by hash. `reconcile` takes the
//! lookup as a callable (these tests pass plain closures); the worker's body is also run through
//! `HydrusClient::new` against a scripted local Hydrus, plain http and https. Every access key is
//! a synthetic placeholder; no live Hydrus is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2131-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-import-url.md`.

mod common;

use std::cell::RefCell;
use std::num::NonZeroUsize;

use common::{Reply, Sandbox, TestPki, TestServer};
use hatate_core::config::{Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{DeletionState, DeletionStates, HydrusClient, HydrusError};
use hatate_core::hydrus_reconcile::{
    BATCH_SIZE, ReconcileResult, entries_awaiting_confirmation, reconcile, run_reconcile,
    run_reconcile_with,
};
use hatate_core::model::Entry;
use serde_json::{Value, json};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn entry(name: &str, sent: bool, confirmed: bool, hash: Option<&str>) -> Entry {
    let mut e = Entry::new(format!("/tmp/{name}"));
    e.sent_to_hydrus = sent;
    e.hydrus_import_confirmed = confirmed;
    e.hydrus_hash = hash.map(str::to_owned);
    e
}

fn waiting(name: &str, hash: &str) -> Entry {
    entry(name, true, false, Some(hash))
}

fn size(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

/// `lambda hashes: {h: mapping.get(h, "unknown") for h in hashes}`.
fn lookup<'a>(
    mapping: &'a [(&'a str, DeletionState)],
) -> impl AsyncFnMut(&[String]) -> Result<DeletionStates, HydrusError> + 'a {
    async move |hashes: &[String]| {
        Ok(hashes
            .iter()
            .map(|h| {
                let state = mapping
                    .iter()
                    .find(|(k, _)| k == h)
                    .map_or(DeletionState::Unknown, |(_, s)| *s);
                (h.clone(), state)
            })
            .collect())
    }
}

async fn run(entries: &mut [Entry], mapping: &[(&str, DeletionState)]) -> ReconcileResult {
    reconcile(entries, lookup(mapping), size(BATCH_SIZE), None, None)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_file_hydrus_holds_is_confirmed_and_one_it_never_saw_stays_queued() {
    let mut entries = vec![waiting("a.png", "h1")];
    let result = run(&mut entries, &[("h1", DeletionState::Present)]).await;
    assert!(entries[0].hydrus_import_confirmed);
    assert_eq!(result.confirmed, [0]);

    // It may simply not have finished downloading yet.
    let mut entries = vec![waiting("a.png", "h1")];
    let result = run(&mut entries, &[]).await;
    assert!(!entries[0].hydrus_import_confirmed);
    assert_eq!(result.unresolved, [0]);
    assert_eq!(result.checked, 1);
    assert_eq!(
        result.summary(),
        "confirmed 0 of 1; 1 still unknown to Hydrus"
    );
}

#[tokio::test]
async fn a_file_deleted_in_hydrus_still_counts_as_imported() {
    for state in [DeletionState::Trashed, DeletionState::Deleted] {
        let mut entries = vec![waiting("a.png", "h1")];
        run(&mut entries, &[("h1", state)]).await;
        assert!(entries[0].hydrus_import_confirmed, "{state:?}");
    }
}

#[tokio::test]
async fn entries_not_sent_already_confirmed_or_without_a_hash_are_not_asked_about() {
    let mut entries = vec![
        entry("never-sent", false, false, Some("h1")),
        entry("done", true, true, Some("h2")),
        entry("no-hash", true, false, None),
        entry("empty-hash", true, false, Some("")),
    ];
    assert!(entries_awaiting_confirmation(&entries).is_empty());
    let result = run(&mut entries, &[("h1", DeletionState::Present)]).await;
    assert_eq!(result.checked, 0);
    assert_eq!(result.summary(), "Nothing was waiting on Hydrus");
    assert!(!entries[0].hydrus_import_confirmed);
}

#[tokio::test]
async fn confirmation_is_never_taken_away() {
    let mut entries = vec![entry("a.png", true, true, Some("h1"))];
    run(&mut entries, &[("h1", DeletionState::Unknown)]).await;
    assert!(entries[0].hydrus_import_confirmed);
}

#[tokio::test]
async fn two_rows_sharing_a_hash_both_get_the_answer_from_one_question() {
    let mut entries = vec![waiting("a.png", "h1"), waiting("copy.png", "h1")];
    let asked = RefCell::new(Vec::new());
    let result = reconcile(
        &mut entries,
        async |hashes: &[String]| {
            asked.borrow_mut().extend(hashes.iter().cloned());
            Ok(hashes
                .iter()
                .map(|h| (h.clone(), DeletionState::Present))
                .collect())
        },
        size(BATCH_SIZE),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(entries.iter().all(|e| e.hydrus_import_confirmed));
    assert_eq!(*asked.borrow(), ["h1"]);
    assert_eq!(result.checked, 2);
    assert_eq!(result.summary(), "confirmed 2 of 2");
}

#[tokio::test]
async fn hashes_are_asked_in_batches_and_progress_is_reported_per_batch() {
    let mut entries: Vec<Entry> = (0..10)
        .map(|i| waiting(&format!("{i}.png"), &format!("h{i}")))
        .collect();
    let sizes = RefCell::new(Vec::new());
    let progress = RefCell::new(Vec::new());
    let on_progress = |done: usize, total: usize| progress.borrow_mut().push((done, total));
    reconcile(
        &mut entries,
        async |hashes: &[String]| {
            sizes.borrow_mut().push(hashes.len());
            Ok(hashes
                .iter()
                .map(|h| (h.clone(), DeletionState::Present))
                .collect())
        },
        size(4),
        None,
        Some(&on_progress),
    )
    .await
    .unwrap();
    assert_eq!(*sizes.borrow(), [4, 4, 2]);
    assert_eq!(*progress.borrow(), [(4, 10), (8, 10), (10, 10)]);
}

#[tokio::test]
async fn it_can_be_stopped_part_way() {
    let mut entries: Vec<Entry> = (0..10)
        .map(|i| waiting(&format!("{i}.png"), &format!("h{i}")))
        .collect();
    let calls = RefCell::new(0usize);
    let stop = || *calls.borrow() >= 2;
    let result = reconcile(
        &mut entries,
        async |hashes: &[String]| {
            *calls.borrow_mut() += 1;
            Ok(hashes
                .iter()
                .map(|h| (h.clone(), DeletionState::Present))
                .collect())
        },
        size(2),
        Some(&stop),
        None,
    )
    .await
    .unwrap();
    assert_eq!(*calls.borrow(), 2);
    assert_eq!(result.checked, 4);
}

#[tokio::test]
async fn a_lookup_with_no_answer_is_survived_and_an_error_ends_the_call() {
    let mut entries = vec![waiting("a.png", "h1")];
    let result = reconcile(
        &mut entries,
        async |_: &[String]| Ok(DeletionStates::default()),
        size(BATCH_SIZE),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!entries[0].hydrus_import_confirmed);
    assert_eq!(result.checked, 1);

    // An unreachable Hydrus is not an error: `deletion_states` answers "unknown" for every hash
    // (Python's own handler), so everything stays as it was.
    let dropped = TestServer::http(|_| Reply::aborted()).await;
    let client = HydrusClient::new(&settings_for(&dropped.url(""), true).hydrus).unwrap();
    let result = reconcile(
        &mut entries,
        async |hashes: &[String]| client.deletion_states(hashes).await,
        size(BATCH_SIZE),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(!entries[0].hydrus_import_confirmed);
    assert_eq!(result.unresolved, [0]);

    // What does end the call is what Python lets propagate: an answer of the wrong shape.
    let odd = TestServer::http(|_| json_reply(json!([1]))).await;
    let client = HydrusClient::new(&settings_for(&odd.url(""), true).hydrus).unwrap();
    let err = reconcile(
        &mut entries,
        async |hashes: &[String]| client.deletion_states(hashes).await,
        size(BATCH_SIZE),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
}

#[tokio::test]
async fn nothing_waiting_asks_hydrus_nothing() {
    let called = RefCell::new(0usize);
    let result = reconcile(
        &mut [],
        async |_: &[String]| {
            *called.borrow_mut() += 1;
            Ok(DeletionStates::default())
        },
        size(BATCH_SIZE),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(*called.borrow(), 0);
    assert_eq!(result, ReconcileResult::default());
    assert!(result.summary().contains("Nothing was waiting"));
}

// ---- the worker's body, against a scripted Hydrus ----

fn json_reply(body: Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

/// A Hydrus whose `file_metadata` says each listed hash is held (`present`), trashed or deleted.
fn hydrus_answering(
    known: Vec<(&'static str, &'static str)>,
) -> impl Fn(&common::Recorded) -> Reply + Send + Sync + 'static {
    move |q| {
        if q.target.starts_with("/get_files/file_metadata") {
            let rows: Vec<Value> = known
                .iter()
                .map(|(hash, state)| {
                    json!({"hash": hash, "file_id": 1,
                           "is_local": *state == "present",
                           "is_trashed": *state == "trashed",
                           "is_deleted": *state == "deleted"})
                })
                .collect();
            json_reply(json!({"metadata": rows}))
        } else {
            json_reply(json!({}))
        }
    }
}

fn settings_for(url: &str, verify_ssl: bool) -> Settings {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    s.hydrus.api_url = url.to_owned();
    s.hydrus.access_key = Secret::new(KEY);
    s.hydrus.timeout = 5.0;
    s.hydrus.verify_ssl = verify_ssl;
    s
}

#[tokio::test]
async fn the_worker_reconciles_over_plain_http_by_hash() {
    let server =
        TestServer::http(hydrus_answering(vec![("h1", "present"), ("h2", "deleted")])).await;
    let settings = settings_for(&server.url(""), true);
    let mut entries = vec![
        waiting("a.png", "h1"),
        waiting("b.png", "h2"),
        waiting("c.png", "h3"),
    ];

    let result = run_reconcile(&mut entries, &settings, None, None).await;

    assert_eq!(
        result.summary(),
        "confirmed 2 of 3; 1 still unknown to Hydrus"
    );
    assert_eq!(result.confirmed, [0, 1]);
    assert_eq!(result.unresolved, [2]);
    assert!(entries[0].hydrus_import_confirmed && entries[1].hydrus_import_confirmed);
    assert!(!entries[2].hydrus_import_confirmed);
    // One question for the three hashes.
    let seen = server.recorded();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].header("hydrus-client-api-access-key"), Some(KEY));
}

#[tokio::test]
async fn the_worker_with_nothing_waiting_makes_no_request() {
    let server = TestServer::http(hydrus_answering(vec![])).await;
    let settings = settings_for(&server.url(""), true);
    let mut entries = vec![entry("a.png", true, true, Some("h1"))];
    let result = run_reconcile(&mut entries, &settings, None, None).await;
    assert_eq!(result, ReconcileResult::default());
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn over_https_a_self_signed_hydrus_is_refused_with_checks_on_and_reached_with_them_off() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, hydrus_answering(vec![("h1", "present")])).await;

    // On: the worker never fails the app. It answers an empty result and nothing is confirmed.
    let mut entries = vec![waiting("a.png", "h1")];
    let result = run_reconcile(
        &mut entries,
        &settings_for(&server.url(""), true),
        None,
        None,
    )
    .await;
    // The handshake is refused, `deletion_states` answers "unknown", nothing is confirmed.
    assert_eq!(result.unresolved, [0]);
    assert!(!entries[0].hydrus_import_confirmed);
    assert_eq!(server.request_count(), 0);

    // The typed error behind that, through the same constructor.
    let on = HydrusClient::new(&settings_for(&server.url(""), true).hydrus).unwrap();
    let err = on.get_services().await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(hatate_core::net::ErrorKind::Tls));

    // Off: it is reached.
    let result = run_reconcile(
        &mut entries,
        &settings_for(&server.url(""), false),
        None,
        None,
    )
    .await;
    assert_eq!(result.confirmed, [0]);
    assert!(entries[0].hydrus_import_confirmed);
}

#[tokio::test]
async fn a_hydrus_that_times_out_or_drops_the_connection_leaves_everything_as_it_was() {
    let slow =
        TestServer::http(|_| json_reply(json!({})).delayed(std::time::Duration::from_secs(3)))
            .await;
    let mut settings = settings_for(&slow.url(""), true);
    settings.hydrus.timeout = 0.3;
    let mut entries = vec![waiting("a.png", "h1")];
    let result = run_reconcile(&mut entries, &settings, None, None).await;
    assert_eq!(
        (result.checked, result.unresolved.as_slice()),
        (1, &[0][..])
    );
    assert!(!entries[0].hydrus_import_confirmed);

    let client = HydrusClient::new(&settings.hydrus).unwrap();
    let err = client.get_services().await.unwrap_err();
    assert_eq!(
        err.kind(),
        ErrorKind::Net(hatate_core::net::ErrorKind::Timeout)
    );
    let user = err.user_message();
    assert!(
        !user.contains(KEY) && !user.contains("os error") && !user.contains('?'),
        "{user}"
    );

    let dropped = TestServer::http(|_| Reply::aborted()).await;
    let client = HydrusClient::new(&settings_for(&dropped.url(""), true).hydrus).unwrap();
    let result = run_reconcile_with(&mut entries, &client, None, None).await;
    assert_eq!(
        (result.checked, result.unresolved.as_slice()),
        (1, &[0][..])
    );
    assert!(!entries[0].hydrus_import_confirmed);

    // The worker never fails the app: a wrong-shaped answer, which Python would let propagate to its
    // `except Exception`, is a warning and an empty result.
    let odd = TestServer::http(|_| json_reply(json!([1]))).await;
    let client = HydrusClient::new(&settings_for(&odd.url(""), true).hydrus).unwrap();
    let result = run_reconcile_with(&mut entries, &client, None, None).await;
    assert_eq!(result, ReconcileResult::default());
}
