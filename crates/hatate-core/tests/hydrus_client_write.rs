//! `core/hydrus_client.py`, the write half (Phase 4.b, DAN-2129), through `HydrusClient::new`
//! (the binaries' constructor: rustls with the explicit provider) against a local server, plain
//! http and https. Every answer is scripted and every access key is a synthetic placeholder; no
//! live Hydrus is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2129-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-client-write.md` section 6.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{Recorded, Reply, TestPki, TestServer};
use hatate_core::config::{HydrusSettings, Secret};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{
    DUPLICATE_ALTERNATE, DUPLICATE_BETTER, DeletionState, HydrusClient, HydrusError,
};
use hatate_core::net;
use serde_json::{Value, json};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const H1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const H3: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn settings(url: &str) -> HydrusSettings {
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

async fn world(
    handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
) -> (TestServer, HydrusClient) {
    let server = TestServer::http(handler).await;
    let client = HydrusClient::new(&settings(&server.url(""))).unwrap();
    (server, client)
}

/// As [`world`] with the tag service the settings name.
async fn world_with_service(
    service: &str,
    handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
) -> (TestServer, HydrusClient) {
    let server = TestServer::http(handler).await;
    let mut s = settings(&server.url(""));
    s.tag_service_key = service.to_owned();
    let client = HydrusClient::new(&s).unwrap();
    (server, client)
}

fn body_text(server: &TestServer, i: usize) -> String {
    String::from_utf8(server.recorded()[i].body.clone()).unwrap()
}

fn body_json(server: &TestServer, i: usize) -> Value {
    serde_json::from_slice(&server.recorded()[i].body).unwrap()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// A temp dir that a test owns.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hatate-dan2129-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- the two schemes, through the binaries' own constructor (DAN-1909) ----

#[tokio::test]
async fn a_write_over_the_default_plain_http_scheme_carries_the_key_and_json_headers() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client.set_kings(&strings(&[H1])).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].target, "/manage_file_relationships/set_kings");
    assert_eq!(seen[0].header("hydrus-client-api-access-key"), Some(KEY));
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert!(!seen[0].target.contains(KEY), "never in the URL");
    assert_eq!(body_text(&server, 0), format!("{{\"hashes\": [\"{H1}\"]}}"));
}

#[tokio::test]
async fn a_write_over_https_with_certificate_checks_on_is_refused_before_anything_is_sent() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({}))).await;
    let client = HydrusClient::new(&settings(&server.url(""))).unwrap();
    let err = client.set_note(H1, "n", "t").await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Tls), "{err}");
    assert_eq!(server.request_count(), 0, "the handshake was refused");
    // Not a relationship or notes failure: a connection failure is passed through as it is.
    assert!(err.message().starts_with("Could not reach Hydrus at "));
}

#[tokio::test]
async fn a_write_over_https_with_certificate_checks_off_reaches_a_self_signed_server() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"status": 1}))).await;
    let mut s = settings(&server.url(""));
    s.verify_ssl = false;
    let client = HydrusClient::new(&s).unwrap();
    client.set_note(H1, "n", "t").await.unwrap();
    // A raw upload takes the same path.
    let dir = scratch("https");
    let file = dir.join("a.bin");
    std::fs::write(&file, b"payload").unwrap();
    let got = client.import_file(&file).await.unwrap();
    assert_eq!(got, json!({"status": 1}));
    assert_eq!(server.request_count(), 2);
    assert_eq!(server.recorded()[1].body, b"payload");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- errors: one timeout, one network error, the user text ----

#[tokio::test]
async fn a_write_that_times_out_is_a_typed_net_error_after_the_configured_wait() {
    let server = TestServer::http(|_| json_reply(json!({})).delayed(Duration::from_secs(3))).await;
    let mut s = settings(&server.url(""));
    s.timeout = 0.3;
    let client = HydrusClient::new(&s).unwrap();
    let started = std::time::Instant::now();
    let err = client.undelete_files(&strings(&[H1])).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Timeout), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(err.status_code(), None);
}

#[tokio::test]
async fn a_write_to_an_unreachable_hydrus_names_the_api_url_and_carries_no_status() {
    let (server, client) = world(|_| Reply::aborted()).await;
    let err = client
        .set_file_relationship(H1, H2, DUPLICATE_BETTER, true)
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Net(_)), "{err}");
    assert_eq!(err.status_code(), None);
    assert!(
        err.message()
            .starts_with(&format!("Could not reach Hydrus at {}: ", server.url(""))),
        "{err}"
    );
}

#[tokio::test]
async fn an_error_for_a_person_has_no_url_query_key_hash_or_os_text() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let client = HydrusClient::new(&settings(&format!("http://127.0.0.1:{port}"))).unwrap();
    let missing = std::env::temp_dir().join("hatate-dan2129-no-such-dir/secret_name.png");
    let errors: Vec<HydrusError> = vec![
        client.set_kings(&strings(&[H1])).await.unwrap_err(),
        client
            .add_tags(H1, &strings(&["secret_tag"]), None)
            .await
            .unwrap_err(),
        client
            .associate_url("http://example.com/p?token=abc", Some(H1))
            .await
            .unwrap_err(),
        client.import_file(&missing).await.unwrap_err(),
        client.show_files_in_client(&[], "p").await.unwrap_err(),
    ];
    assert_eq!(errors[3].kind(), ErrorKind::FileUnreadable);
    assert_eq!(errors[4].kind(), ErrorKind::InvalidInput);
    for err in &errors {
        let shown = err.user_message();
        for forbidden in [
            KEY,
            H1,
            "secret_tag",
            "secret_name",
            "hatate-dan2129",
            "token",
            "?",
            "os error",
            "Errno",
            "No such file",
            "Connection refused",
            "127.0.0.1",
        ] {
            assert!(!shown.contains(forbidden), "{forbidden} in {shown:?}");
        }
        assert!(!shown.is_empty());
        assert!(!err.detail().contains(KEY));
        assert!(!err.detail().contains("secret_tag"), "{}", err.detail());
        assert!(!err.detail().contains("token=abc"), "{}", err.detail());
    }
    // The raw detail is for the log and does carry the path.
    assert!(errors[3].detail().contains("secret_name.png"));
}

#[tokio::test]
async fn the_two_new_failures_and_the_named_statuses_each_read_as_what_to_do() {
    let (_s, client) = world(|r| match r.target.as_str() {
        "/manage_file_relationships/set_kings" => Reply::new(403, "nope"),
        "/add_notes/set_notes" => Reply::new(404, "nope"),
        _ => json_reply(json!({"status": 7, "note": "vetoed: SECRET-NOTE"})),
    })
    .await;
    let e403 = client.set_kings(&strings(&[H1])).await.unwrap_err();
    assert_eq!(e403.kind(), ErrorKind::PermissionDenied);
    assert!(e403.user_message().contains("lacks a permission"));
    let e404 = client.set_note(H1, "n", "t").await.unwrap_err();
    assert_eq!(e404.kind(), ErrorKind::HttpStatus);
    assert_eq!(e404.status_code(), Some(404));
    let dir = scratch("usermsg");
    let file = dir.join("a.bin");
    std::fs::write(&file, b"x").unwrap();
    let rejected = client.import_file(&file).await.unwrap_err();
    assert_eq!(rejected.kind(), ErrorKind::ImportRejected);
    assert!(rejected.message().contains("SECRET-NOTE"));
    assert!(!rejected.user_message().contains("SECRET-NOTE"));
    assert!(rejected.user_message().contains("import log"));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- show_files_in_client ----

#[tokio::test]
async fn show_files_asks_for_a_file_search_page_holding_the_hashes_and_focuses_it() {
    let (server, client) = world(|_| json_reply(json!({"page_key": "abc123"}))).await;
    let key = client
        .show_files_in_client(&strings(&[H1, H2]), "From Hatate")
        .await
        .unwrap();
    assert_eq!(key, "abc123");
    let seen = &server.recorded()[0];
    assert_eq!(
        (seen.method.as_str(), seen.target.as_str()),
        ("POST", "/manage_pages/new_page")
    );
    // json.dumps order: page_type, page_name, hashes, focus_page; nothing locks the page.
    assert_eq!(
        body_text(&server, 0),
        format!(
            "{{\"page_type\": 6, \"page_name\": \"From Hatate\", \"hashes\": [\"{H1}\", \"{H2}\"], \"focus_page\": true}}"
        )
    );
    assert!(!body_text(&server, 0).contains("system_hash_locked"));
}

#[tokio::test]
async fn show_files_with_no_hashes_never_reaches_the_network() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    let err = client.show_files_in_client(&[], "p").await.unwrap_err();
    assert_eq!(err.message(), "No files to show.");
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn show_files_surfaces_a_missing_permission_as_a_403_and_an_old_client_as_a_404() {
    for (status, kind) in [
        (403, ErrorKind::PermissionDenied),
        (404, ErrorKind::HttpStatus),
    ] {
        let (_s, client) = world(move |_| Reply::new(status, "x")).await;
        let err = client
            .show_files_in_client(&strings(&[H1]), "p")
            .await
            .unwrap_err();
        assert_eq!((err.status_code(), err.kind()), (Some(status), kind));
        // The page call has no rewording: the message is the plain `_handle` one.
        assert!(
            err.message()
                .starts_with("Access key lacks permission for this action (403)")
                || err.message().starts_with("Hydrus returned HTTP 404"),
            "{err}"
        );
    }
}

#[tokio::test]
async fn show_files_with_an_answer_that_has_no_page_key_is_not_fatal() {
    for body in [json!({}), json!({"other": 1}), json!([])] {
        let (_s, client) = world(move |_| json_reply(body.clone())).await;
        assert_eq!(
            client
                .show_files_in_client(&strings(&[H1]), "p")
                .await
                .unwrap(),
            ""
        );
    }
    let (_s, client) = world(|_| Reply::ok("not json")).await;
    assert_eq!(
        client
            .show_files_in_client(&strings(&[H1]), "p")
            .await
            .unwrap(),
        ""
    );
    // Python reads the answer as untyped JSON: a list that is not empty has no `.get`.
    let (_s, client) = world(|_| json_reply(json!([1]))).await;
    let err = client
        .show_files_in_client(&strings(&[H1]), "p")
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
}

// ---- relationships and kings ----

#[tokio::test]
async fn the_relationship_request_is_the_documented_shape_and_never_asks_for_a_deletion() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client
        .set_file_relationship(H1, H2, DUPLICATE_BETTER, true)
        .await
        .unwrap();
    client
        .set_file_relationship(H1, H2, DUPLICATE_ALTERNATE, false)
        .await
        .unwrap();
    assert_eq!(
        server.recorded()[0].target,
        "/manage_file_relationships/set_file_relationships"
    );
    assert_eq!(
        body_text(&server, 0),
        format!(
            "{{\"relationships\": [{{\"hash_a\": \"{H1}\", \"hash_b\": \"{H2}\", \"relationship\": 4, \"do_default_content_merge\": true}}]}}"
        )
    );
    let second = body_json(&server, 1);
    assert_eq!(second["relationships"][0]["relationship"], 3);
    assert_eq!(
        second["relationships"][0]["do_default_content_merge"],
        false
    );
    for i in 0..2 {
        let text = body_text(&server, i);
        assert!(
            !text.contains("delete_a") && !text.contains("delete_b"),
            "{text}"
        );
    }
    assert_eq!((DUPLICATE_BETTER, DUPLICATE_ALTERNATE), (4, 3));
}

#[tokio::test]
async fn a_file_cannot_be_paired_with_itself_and_a_missing_hash_is_refused_before_the_request() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    let same = client
        .set_file_relationship(H1, &H1.to_uppercase(), DUPLICATE_BETTER, true)
        .await
        .unwrap_err();
    assert_eq!(
        same.message(),
        "A file cannot be set as a duplicate of itself"
    );
    for (a, b) in [("", H2), (H1, ""), ("", "")] {
        let err = client
            .set_file_relationship(a, b, DUPLICATE_BETTER, true)
            .await
            .unwrap_err();
        assert_eq!(
            err.message(),
            "Both file hashes are needed to set a duplicate relationship"
        );
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn a_relationship_403_and_404_become_the_two_messages_a_user_can_act_on() {
    const WANT_403: &str = "your Hydrus access key does not have the \"Edit File Relationships\" permission, which is needed to record duplicate relationships. Add it in Hydrus: services \u{2192} review services \u{2192} local \u{2192} client api, then edit your access key and tick it";
    const WANT_404: &str = "this Hydrus is too old to record duplicate relationships over the Client API (it has no /manage_file_relationships endpoint). Update Hydrus, or turn the setting off in Settings > Hydrus";
    // `set_file_relationship` and `set_kings` share the one rewording.
    for use_kings in [false, true] {
        for (status, want, kind) in [
            (403u16, WANT_403, ErrorKind::PermissionDenied),
            (404, WANT_404, ErrorKind::HttpStatus),
        ] {
            let (_s, client) = world(move |_| Reply::new(status, "raw hydrus text")).await;
            let err = if use_kings {
                client.set_kings(&strings(&[H1])).await.unwrap_err()
            } else {
                client
                    .set_file_relationship(H1, H2, DUPLICATE_BETTER, true)
                    .await
                    .unwrap_err()
            };
            assert_eq!((err.status_code(), err.kind()), (Some(status), kind));
            assert_eq!(err.message(), want);
            assert!(!err.message().contains("raw hydrus text"));
        }
    }
}

#[tokio::test]
async fn any_other_relationship_failure_is_passed_through_unchanged() {
    for status in [401u16, 409, 500, 503] {
        let (_s, client) = world(move |_| Reply::new(status, "boom")).await;
        let err = client
            .set_file_relationship(H1, H2, DUPLICATE_BETTER, true)
            .await
            .unwrap_err();
        assert_eq!(err.status_code(), Some(status));
        assert!(
            err.message().starts_with("Hydrus returned HTTP")
                || err.message().starts_with("Hydrus rejected the access key"),
            "{err}"
        );
        assert!(!err.message().contains("Edit File Relationships"));
    }
}

#[tokio::test]
async fn set_kings_sends_the_hashes_and_does_nothing_for_an_empty_list() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client.set_kings(&[]).await.unwrap();
    assert_eq!(server.request_count(), 0);
    client.set_kings(&strings(&[H1, H2])).await.unwrap();
    assert_eq!(
        server.recorded()[0].target,
        "/manage_file_relationships/set_kings"
    );
    assert_eq!(body_json(&server, 0), json!({"hashes": [H1, H2]}));
}

// ---- notes ----

#[tokio::test]
async fn the_note_request_is_the_documented_shape_and_replaces_rather_than_merges() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client
        .set_note(H1, "hatate: source", "Found via \u{e9}\n\"x\"")
        .await
        .unwrap();
    assert_eq!(server.recorded()[0].target, "/add_notes/set_notes");
    assert_eq!(
        body_text(&server, 0),
        format!(
            "{{\"hash\": \"{H1}\", \"notes\": {{\"hatate: source\": \"Found via \\u00e9\\n\\\"x\\\"\"}}}}"
        )
    );
    assert!(!body_text(&server, 0).contains("merge_cleverly"));
}

#[tokio::test]
async fn a_missing_note_hash_or_name_is_refused_before_the_request() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    let no_hash = client.set_note("", "n", "t").await.unwrap_err();
    assert_eq!(no_hash.message(), "A file hash is needed to write a note");
    let no_name = client.set_note(H1, "", "t").await.unwrap_err();
    assert_eq!(no_name.message(), "A note name is needed to write a note");
    // An empty text is a note; only the hash and the name are required.
    client.set_note(H1, "n", "").await.unwrap();
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn a_note_403_and_404_become_the_two_messages_a_user_can_act_on() {
    let (_s, client) = world(|_| Reply::new(403, "x")).await;
    let err = client.set_note(H1, "n", "t").await.unwrap_err();
    assert_eq!(err.status_code(), Some(403));
    assert_eq!(
        err.message(),
        "your Hydrus access key does not have the \"Edit File Notes\" permission, which is needed to write a note onto the file. Add it in Hydrus: services \u{2192} review services \u{2192} local \u{2192} client api, then edit your access key and tick it"
    );
    let (_s, client) = world(|_| Reply::new(404, "x")).await;
    let err = client.set_note(H1, "n", "t").await.unwrap_err();
    assert_eq!(err.status_code(), Some(404));
    assert_eq!(
        err.message(),
        "this Hydrus is too old to write file notes over the Client API (it has no /add_notes/set_notes endpoint). Update Hydrus, or turn the setting off in Settings > Hydrus"
    );
    let (_s, client) = world(|_| Reply::new(500, "bang")).await;
    let err = client.set_note(H1, "n", "t").await.unwrap_err();
    assert_eq!(err.message(), "Hydrus returned HTTP 500: bang");
}

// ---- deletes ----

/// A file_metadata answer built from `(hash, file_id, is_trashed, is_deleted, is_local)`.
fn metadata(rows: &[(&str, Value, bool, bool, bool)]) -> Value {
    json!({"metadata": rows.iter().map(|(h, id, t, d, l)| json!({
        "hash": h, "file_id": id, "is_trashed": t, "is_deleted": d, "is_local": l,
    })).collect::<Vec<_>>()})
}

fn held() -> Value {
    metadata(&[(H1, json!(1), false, false, true)])
}

#[tokio::test]
async fn a_held_file_is_deleted_with_a_reason_after_the_state_check() {
    let (server, client) = world(|r| {
        if r.method == "GET" {
            json_reply(held())
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let gone = client
        .delete_files(&strings(&[H1]), Some("duplicate of better"))
        .await
        .unwrap();
    assert_eq!(gone, vec![H1]);
    let seen = server.recorded();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0].target,
        format!("/get_files/file_metadata?hashes=%5B%22{H1}%22%5D")
    );
    assert_eq!(seen[1].target, "/add_files/delete_files");
    assert_eq!(
        body_text(&server, 1),
        format!("{{\"hashes\": [\"{H1}\"], \"reason\": \"duplicate of better\"}}")
    );
    // No reason, no `reason` key.
    client.delete_files(&strings(&[H1]), None).await.unwrap();
    client
        .delete_files(&strings(&[H1]), Some(""))
        .await
        .unwrap();
    assert_eq!(body_text(&server, 3), format!("{{\"hashes\": [\"{H1}\"]}}"));
    assert_eq!(body_text(&server, 5), format!("{{\"hashes\": [\"{H1}\"]}}"));
}

#[tokio::test]
async fn a_hash_hydrus_never_had_is_never_sent_and_nor_is_one_already_deleted() {
    for row in [
        // Echoed back with a null id: never seen.
        (H1, Value::Null, false, false, false),
        // Already gone: keeps its id but is_deleted.
        (H1, json!(5), false, true, false),
        // In the trash.
        (H1, json!(5), true, false, true),
        // Not local, not flagged deleted.
        (H1, json!(5), false, false, false),
    ] {
        let (server, client) =
            world(move |_| json_reply(metadata(std::slice::from_ref(&row)))).await;
        let gone = client.delete_files(&strings(&[H1]), None).await.unwrap();
        assert!(gone.is_empty());
        assert_eq!(server.request_count(), 1, "only the state check, no POST");
    }
}

#[tokio::test]
async fn only_the_held_files_of_a_mixed_selection_are_sent_in_the_order_given() {
    let (server, client) = world(|r| {
        if r.method == "GET" {
            json_reply(metadata(&[
                (H3, json!(3), false, false, true),
                (H1, json!(1), false, false, true),
                (H2, json!(2), true, false, true),
            ]))
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let gone = client
        .delete_files(&strings(&[H1, H2, "dd", H3, H1]), None)
        .await
        .unwrap();
    // Duplicates in the request stay duplicates, as Python's list comprehension keeps them.
    assert_eq!(gone, vec![H1, H3, H1]);
    assert_eq!(body_json(&server, 1), json!({"hashes": [H1, H3, H1]}));
}

#[tokio::test]
async fn an_empty_selection_makes_no_request_and_a_failed_state_check_deletes_nothing() {
    let (server, client) = world(|_| Reply::new(500, "down")).await;
    assert!(client.delete_files(&[], None).await.unwrap().is_empty());
    assert_eq!(server.request_count(), 0);
    // The check fails as a HydrusError: every hash is "unknown", so nothing is sent.
    assert!(
        client
            .delete_files(&strings(&[H1]), None)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.request_count(), 1);
    // An answer Python raises a raw exception on is not swallowed.
    let (_s, client) = world(|_| json_reply(json!({"metadata": [1]}))).await;
    let err = client
        .delete_files(&strings(&[H1]), None)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(err.message(), "'int' object has no attribute 'get'");
}

#[tokio::test]
async fn a_failed_delete_post_is_the_error_and_a_lacking_permission_is_a_403() {
    let (server, client) = world(|r| {
        if r.method == "GET" {
            json_reply(held())
        } else {
            Reply::new(403, "needs Import and Delete Files")
        }
    })
    .await;
    let err = client
        .delete_files(&strings(&[H1]), None)
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), Some(403));
    assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    assert_eq!(server.request_count(), 2);
}

#[tokio::test]
async fn deletion_states_tell_every_case_apart() {
    let (server, client) = world(|_| {
        json_reply(json!({"metadata": [
            {"hash": H1, "file_id": 1, "is_trashed": false, "is_deleted": false, "is_local": true},
            {"hash": H2, "file_id": 2, "is_trashed": true, "is_deleted": false, "is_local": true},
            {"hash": H3, "file_id": 3, "is_trashed": false, "is_deleted": true, "is_local": false},
            {"hash": "dd", "file_id": null},
            {"hash": "ee", "file_id": 9},
            {"hash": "", "file_id": 9},
            {"file_id": 9},
            {"hash": "ff", "file_id": 4, "is_local": true, "is_deleted": 0},
            {"hash": "novel", "file_id": 4, "is_local": true},
        ]}))
    })
    .await;
    let states = client
        .deletion_states(&strings(&[H1, H2, H3, "dd", "ee", "gg", H1]))
        .await
        .unwrap();
    let seen: Vec<(&str, &str)> = states.iter().map(|(h, s)| (h, s.as_str())).collect();
    assert_eq!(
        seen,
        vec![
            (H1, "present"),
            (H2, "trashed"),
            (H3, "deleted"),
            ("dd", "unknown"),
            // No is_local at all counts as not local.
            ("ee", "deleted"),
            ("gg", "unknown"),
            // A hash the answer names that was not asked about is added, last.
            ("ff", "present"),
            ("novel", "present"),
        ]
    );
    assert_eq!(states.get(H2), Some(DeletionState::Trashed));
    assert_eq!(states.len(), 8);
    assert_eq!(
        server.recorded()[0].target,
        format!(
            "/get_files/file_metadata?hashes=%5B%22{H1}%22%2C+%22{H2}%22%2C+%22{H3}%22%2C+%22dd%22%2C+%22ee%22%2C+%22gg%22%2C+%22{H1}%22%5D"
        )
    );
    assert!(client.deletion_states(&[]).await.unwrap().is_empty());
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn deletion_states_of_an_odd_answer_fail_the_way_python_does() {
    let (_s, client) =
        world(|_| json_reply(json!({"metadata": [{"hash": [1], "file_id": 1}]}))).await;
    let err = client.deletion_states(&strings(&[H1])).await.unwrap_err();
    assert_eq!(
        err.message(),
        "cannot use 'list' as a dict key (unhashable type: 'list')"
    );
    let (_s, client) = world(|_| json_reply(json!([]))).await;
    let err = client.deletion_states(&strings(&[H1])).await.unwrap_err();
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
    let (_s, client) = world(|_| json_reply(json!({"metadata": null}))).await;
    let err = client.deletion_states(&strings(&[H1])).await.unwrap_err();
    assert_eq!(err.message(), "'NoneType' object is not iterable");
    // A failed call is every hash unknown.
    let (_s, client) = world(|_| Reply::new(500, "x")).await;
    let states = client.deletion_states(&strings(&[H1, H2])).await.unwrap();
    assert!(states.iter().all(|(_, s)| s == DeletionState::Unknown));
    assert_eq!(states.len(), 2);
}

#[tokio::test]
async fn a_row_whose_hash_is_not_a_string_is_left_out_and_does_not_stop_a_delete() {
    let (server, client) = world(|r| {
        if r.method == "GET" {
            json_reply(json!({"metadata": [
                {"hash": 5, "file_id": 1, "is_local": true},
                {"hash": 2.5, "file_id": 1, "is_local": true},
                {"hash": true, "file_id": 1, "is_local": true},
                {"hash": H1, "file_id": 2, "is_local": true},
            ]}))
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let states = client.deletion_states(&strings(&[H1])).await.unwrap();
    assert_eq!(states.len(), 1);
    let gone = client.delete_files(&strings(&[H1]), None).await.unwrap();
    assert_eq!(gone, vec![H1]);
    assert_eq!(server.request_count(), 3);
}

#[tokio::test]
async fn undelete_posts_the_hashes_and_an_empty_list_does_nothing() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client.undelete_files(&[]).await.unwrap();
    assert_eq!(server.request_count(), 0);
    client.undelete_files(&strings(&[H1, H2])).await.unwrap();
    assert_eq!(server.recorded()[0].target, "/add_files/undelete_files");
    assert_eq!(body_json(&server, 0), json!({"hashes": [H1, H2]}));
}

// ---- import_file ----

#[tokio::test]
async fn the_file_bytes_are_the_whole_request_body_and_the_answer_comes_back_verbatim() {
    let dir = scratch("import-bytes");
    let file = dir.join("pic.png");
    let bytes: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    std::fs::write(&file, &bytes).unwrap();
    let answer = json!({"status": 1, "hash": H1, "note": "", "extra": [1, 2]});
    let reply = answer.clone();
    let (server, client) = world(move |_| json_reply(reply.clone())).await;
    let got = client.import_file(&file).await.unwrap();
    assert_eq!(got, answer);
    let seen = &server.recorded()[0];
    assert_eq!(
        (seen.method.as_str(), seen.target.as_str()),
        ("POST", "/add_files/add_file")
    );
    assert_eq!(seen.body, bytes);
    assert_eq!(
        seen.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(seen.header("hydrus-client-api-access-key"), Some(KEY));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_unreadable_local_file_is_refused_before_any_request() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    let dir = scratch("import-missing");
    let missing = dir.join("nope.png");
    let err = client.import_file(&missing).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FileUnreadable);
    assert_eq!(err.status_code(), None);
    assert_eq!(
        err.message(),
        format!(
            "Could not read local file {}: [Errno 2] No such file or directory: '{}'",
            missing.display(),
            missing.display()
        )
    );
    // A directory is unreadable too, with its own errno.
    let err = client.import_file(&dir).await.unwrap_err();
    assert!(err.message().contains("[Errno 21] Is a directory"), "{err}");
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn hydrus_answering_200_with_a_refusal_status_is_still_a_failure() {
    let dir = scratch("import-refused");
    let file = dir.join("a.bin");
    std::fs::write(&file, b"x").unwrap();
    for (answer, want) in [
        (
            json!({"status": 4, "note": "bad file"}),
            "Hydrus rejected the file: bad file",
        ),
        (
            json!({"status": 7, "note": "vetoed: too small"}),
            "Hydrus rejected the file: vetoed: too small",
        ),
        (
            json!({"status": 4}),
            "Hydrus rejected the file: no details given",
        ),
        (
            json!({"status": 4.0, "note": null}),
            "Hydrus rejected the file: None",
        ),
        (
            json!({"status": 7, "note": 5}),
            "Hydrus rejected the file: 5",
        ),
        (
            json!({"status": 7, "note": ["a", 1]}),
            "Hydrus rejected the file: ['a', 1]",
        ),
        (
            json!({"status": 7, "note": ""}),
            "Hydrus rejected the file: ",
        ),
    ] {
        let (_s, client) = world(move |_| json_reply(answer.clone())).await;
        let err = client.import_file(&file).await.unwrap_err();
        assert_eq!(err.message(), want);
        assert_eq!(err.kind(), ErrorKind::ImportRejected);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_success_statuses_and_an_answer_with_no_status_are_passed_through() {
    let dir = scratch("import-ok");
    let file = dir.join("a.bin");
    std::fs::write(&file, b"x").unwrap();
    for answer in [
        json!({"status": 1, "hash": H1}),
        json!({"status": 2}),
        json!({"status": 3, "note": "previously deleted"}),
        json!({"status": 5}),
        json!({"status": true}),
        json!({"status": "4"}),
        json!({"hash": H1}),
        json!({}),
    ] {
        let reply = answer.clone();
        let (_s, client) = world(move |_| json_reply(reply.clone())).await;
        assert_eq!(client.import_file(&file).await.unwrap(), answer);
    }
    // A body that is not JSON is `{}`, which has no status.
    let (_s, client) = world(|_| Reply::ok("<html>")).await;
    assert_eq!(client.import_file(&file).await.unwrap(), json!({}));
    // A JSON body that is not an object has no `.get` in Python.
    let (_s, client) = world(|_| json_reply(json!([1]))).await;
    let err = client.import_file(&file).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- associate_url and import_url ----

#[tokio::test]
async fn the_url_field_is_url_to_add_and_a_hash_is_a_one_element_list() {
    let (server, client) = world(|_| json_reply(json!({"ok": 1}))).await;
    let got = client
        .associate_url("https://e.example/p/1?a=b&c=d", Some(H1))
        .await
        .unwrap();
    assert_eq!(got, json!({"ok": 1}));
    assert_eq!(server.recorded()[0].target, "/add_urls/associate_url");
    assert_eq!(
        body_text(&server, 0),
        format!("{{\"url_to_add\": \"https://e.example/p/1?a=b&c=d\", \"hashes\": [\"{H1}\"]}}")
    );
    for none in [None, Some("")] {
        client.associate_url("u", none).await.unwrap();
    }
    assert_eq!(body_text(&server, 1), "{\"url_to_add\": \"u\"}");
    assert_eq!(body_text(&server, 2), "{\"url_to_add\": \"u\"}");
}

#[tokio::test]
async fn an_associate_failure_reaches_the_caller() {
    let (_s, client) = world(|_| Reply::new(500, "bang")).await;
    let err = client.associate_url("u", Some(H1)).await.unwrap_err();
    assert_eq!(err.status_code(), Some(500));
}

#[tokio::test]
async fn the_minimal_import_sends_the_url_and_no_page_side_effects() {
    let (server, client) = world(|_| json_reply(json!({"human_result_text": "queued"}))).await;
    let got = client
        .import_url("https://e.example/p/1", &[], None, false, None)
        .await
        .unwrap();
    assert_eq!(got, json!({"human_result_text": "queued"}));
    assert_eq!(server.recorded()[0].target, "/add_urls/add_url");
    assert_eq!(
        body_text(&server, 0),
        "{\"url\": \"https://e.example/p/1\", \"show_destination_page\": false}"
    );
    assert_eq!(
        server.request_count(),
        1,
        "no tags, so no tag service lookup"
    );
}

#[tokio::test]
async fn a_destination_page_is_named_only_when_asked_for() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client
        .import_url("u", &[], None, true, Some("My page"))
        .await
        .unwrap();
    client
        .import_url("u", &[], None, true, Some(""))
        .await
        .unwrap();
    client.import_url("u", &[], None, true, None).await.unwrap();
    assert_eq!(
        body_text(&server, 0),
        "{\"url\": \"u\", \"show_destination_page\": true, \"destination_page_name\": \"My page\"}"
    );
    assert_eq!(
        body_text(&server, 1),
        "{\"url\": \"u\", \"show_destination_page\": true}"
    );
    assert_eq!(
        body_text(&server, 2),
        "{\"url\": \"u\", \"show_destination_page\": true}"
    );
}

fn services() -> Value {
    json!({"services": {
        "6c6f63616c2074616773": {"name": "my tags", "type_pretty": "local tag service"},
        "abcd": {"name": "Public Repo", "type_pretty": "tag repository"},
        "ff00": {"name": "files", "type_pretty": "local file domain"},
    }})
}

#[tokio::test]
async fn tags_go_under_the_resolved_hex_key_and_a_name_is_looked_up() {
    let (server, client) = world(|r| {
        if r.target == "/get_services" {
            json_reply(services())
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    // A hex service key is used as it is, with no lookup.
    client
        .import_url("u", &strings(&["a", "b:c"]), Some("abcd"), false, None)
        .await
        .unwrap();
    assert_eq!(server.request_count(), 1);
    assert_eq!(
        body_text(&server, 0),
        "{\"url\": \"u\", \"show_destination_page\": false, \"service_keys_to_additional_tags\": {\"abcd\": [\"a\", \"b:c\"]}}"
    );
    // A name is resolved through the services Hydrus lists, ignoring case and padding.
    client
        .import_url("u", &strings(&["a"]), Some("  public REPO "), false, None)
        .await
        .unwrap();
    assert_eq!(server.recorded()[1].target, "/get_services");
    assert_eq!(
        body_json(&server, 2)["service_keys_to_additional_tags"],
        json!({"abcd": ["a"]})
    );
    // `names are never sent under the keys field`.
    assert!(!body_text(&server, 2).contains("service_names_to_tags"));
}

#[tokio::test]
async fn the_explicit_key_wins_over_the_setting_and_the_setting_over_my_tags() {
    let answer = |r: &Recorded| {
        if r.target == "/get_services" {
            json_reply(services())
        } else {
            json_reply(json!({}))
        }
    };
    let (server, client) = world_with_service("ff00", answer).await;
    client
        .import_url("u", &strings(&["a"]), Some("abcd"), false, None)
        .await
        .unwrap();
    assert_eq!(
        body_json(&server, 0)["service_keys_to_additional_tags"],
        json!({"abcd": ["a"]})
    );
    client
        .import_url("u", &strings(&["a"]), None, false, None)
        .await
        .unwrap();
    assert_eq!(
        body_json(&server, 1)["service_keys_to_additional_tags"],
        json!({"ff00": ["a"]})
    );
    // The empty default setting falls back to Hydrus's own default name, resolved to its key.
    let (server, client) = world(answer).await;
    client
        .import_url("u", &strings(&["a"]), None, false, None)
        .await
        .unwrap();
    assert_eq!(server.recorded()[0].target, "/get_services");
    assert_eq!(
        body_json(&server, 1)["service_keys_to_additional_tags"],
        json!({"6c6f63616c2074616773": ["a"]})
    );
}

#[tokio::test]
async fn an_unresolvable_service_imports_the_file_without_the_tags() {
    // An unknown name, a failing listing and a dead connection all import without the tags.
    for list in [
        json_reply(services()),
        Reply::new(500, "x"),
        Reply::aborted(),
    ] {
        let (server, client) = world(move |r| {
            if r.target == "/get_services" {
                list.clone()
            } else {
                json_reply(json!({"ok": 1}))
            }
        })
        .await;
        let got = client
            .import_url("u", &strings(&["a"]), Some("no such service"), false, None)
            .await
            .unwrap();
        assert_eq!(got, json!({"ok": 1}));
        let posted = server.recorded().len() - 1;
        assert_eq!(
            body_text(&server, posted),
            "{\"url\": \"u\", \"show_destination_page\": false}"
        );
    }
    let (_s, client) = world(|r| {
        if r.target == "/get_services" {
            json_reply(json!([1]))
        } else {
            json_reply(json!({}))
        }
    })
    .await;
    let err = client
        .import_url("u", &strings(&["a"]), Some("no such service"), false, None)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
}

// ---- add_tags ----

fn tag_posts(server: &TestServer) -> Vec<Value> {
    (0..server.request_count())
        .filter(|i| server.recorded()[*i].target == "/add_tags/add_tags")
        .map(|i| body_json(server, i))
        .collect()
}

#[tokio::test]
async fn a_hex_looking_service_tries_the_keys_field_first_and_a_name_the_names_field() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client
        .add_tags(H1, &strings(&["a", "b"]), Some("6c6f63616c2074616773"))
        .await
        .unwrap();
    client
        .add_tags(H1, &strings(&["a"]), Some("my tags"))
        .await
        .unwrap();
    assert_eq!(
        body_text(&server, 0),
        format!(
            "{{\"hash\": \"{H1}\", \"service_keys_to_tags\": {{\"6c6f63616c2074616773\": [\"a\", \"b\"]}}}}"
        )
    );
    assert_eq!(
        body_text(&server, 1),
        format!("{{\"hash\": \"{H1}\", \"service_names_to_tags\": {{\"my tags\": [\"a\"]}}}}")
    );
    assert_eq!(server.request_count(), 2, "a success is one request");
}

#[tokio::test]
async fn the_empty_setting_writes_to_my_tags_and_an_explicit_key_wins_over_the_setting() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client.add_tags(H1, &strings(&["a"]), None).await.unwrap();
    client
        .add_tags(H1, &strings(&["a"]), Some(""))
        .await
        .unwrap();
    for i in 0..2 {
        assert_eq!(
            body_json(&server, i)["service_names_to_tags"],
            json!({"my tags": ["a"]})
        );
    }
    let (server, client) = world_with_service("Favourites", |_| json_reply(json!({}))).await;
    client.add_tags(H1, &strings(&["a"]), None).await.unwrap();
    client
        .add_tags(H1, &strings(&["a"]), Some("abcd"))
        .await
        .unwrap();
    assert_eq!(
        body_json(&server, 0)["service_names_to_tags"],
        json!({"Favourites": ["a"]})
    );
    assert_eq!(
        body_json(&server, 1)["service_keys_to_tags"],
        json!({"abcd": ["a"]})
    );
}

#[tokio::test]
async fn a_wrong_first_guess_is_retried_with_the_other_field_in_both_directions() {
    for service in ["abcd", "my tags"] {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let (server, client) = world(move |_| {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                Reply::new(400, "could not parse")
            } else {
                json_reply(json!({"ok": true}))
            }
        })
        .await;
        let got = client
            .add_tags(H1, &strings(&["a"]), Some(service))
            .await
            .unwrap();
        assert_eq!(got, json!({"ok": true}));
        let posts = tag_posts(&server);
        assert_eq!(posts.len(), 2);
        let (first, second) = if service == "abcd" {
            ("service_keys_to_tags", "service_names_to_tags")
        } else {
            ("service_names_to_tags", "service_keys_to_tags")
        };
        assert!(posts[0].get(first).is_some() && posts[0].get(second).is_none());
        assert!(posts[1].get(second).is_some() && posts[1].get(first).is_none());
    }
}

#[tokio::test]
async fn both_attempts_failing_returns_the_second_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let (server, client) = world(move |_| match seen.fetch_add(1, Ordering::SeqCst) {
        0 => Reply::new(400, "first"),
        _ => Reply::new(500, "second"),
    })
    .await;
    let err = client
        .add_tags(H1, &strings(&["a"]), Some("my tags"))
        .await
        .unwrap_err();
    assert_eq!(err.status_code(), Some(500));
    assert_eq!(err.message(), "Hydrus returned HTTP 500: second");
    assert_eq!(server.request_count(), 2, "one retry, not more");
}

#[tokio::test]
async fn an_empty_tag_list_is_still_sent_rather_than_silently_dropped() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    client.add_tags(H1, &[], Some("abcd")).await.unwrap();
    assert_eq!(
        body_json(&server, 0)["service_keys_to_tags"],
        json!({"abcd": []})
    );
}

#[tokio::test]
async fn a_connection_failure_is_retried_once_too_as_python_retries_any_hydrus_error() {
    let (server, client) = world(|_| Reply::aborted()).await;
    let err = client
        .add_tags(H1, &strings(&["a"]), Some("my tags"))
        .await
        .unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Net(_)));
    assert_eq!(server.request_count(), 2);
}
