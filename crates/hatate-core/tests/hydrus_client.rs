//! `core/hydrus_client.py`, the read half (Phase 4.a, DAN-2126), through `HydrusClient::new` (the
//! binaries' constructor: rustls with the explicit provider) against a local server, plain http
//! and https. Every answer is scripted and every access key is a synthetic placeholder; no live
//! Hydrus is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2126-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-client.md` section 6.

mod common;

use std::time::Duration;

use common::{Recorded, Reply, TestPki, TestServer};
use hatate_core::config::{HydrusSettings, Secret};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{
    HydrusClient, HydrusError, HydrusFile, PERMISSION_EDIT_FILE_RELATIONSHIPS,
};
use hatate_core::net::{self, NetConfig};
use serde_json::{Value, json};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const H1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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

/// A plain-http server and a client for it, built by the binaries' constructor.
async fn world(
    handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
) -> (TestServer, HydrusClient) {
    let server = TestServer::http(handler).await;
    let client = HydrusClient::new(&settings(&server.url(""))).unwrap();
    (server, client)
}

fn target(server: &TestServer, i: usize) -> String {
    server.recorded()[i].target.clone()
}

// ---- the two schemes, through the binaries' own constructor (DAN-1909) ----

#[tokio::test]
async fn the_default_scheme_is_plain_http_and_the_key_goes_in_a_header() {
    let (server, client) = world(|_| json_reply(json!({"basic_permissions": [0, 3]}))).await;
    assert!(client.test_connection().await.unwrap());
    let seen = server.recorded();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].target, "/verify_access_key");
    assert_eq!(
        seen[0].header("hydrus-client-api-access-key"),
        Some(KEY),
        "the key is a header"
    );
    assert!(!seen[0].target.contains(KEY), "never in the URL");
}

#[tokio::test]
async fn https_with_certificate_checks_on_refuses_a_self_signed_server() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"ok": 1}))).await;
    let client = HydrusClient::new(&settings(&server.url(""))).unwrap();
    let err = client.test_connection().await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Tls), "{err}");
    assert_eq!(server.request_count(), 0, "the handshake was refused");
    assert!(
        err.message()
            .starts_with(&format!("Could not reach Hydrus at {}: ", server.url("")))
    );
}

#[tokio::test]
async fn https_with_certificate_checks_off_accepts_a_self_signed_server() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"ok": 1}))).await;
    let mut s = settings(&server.url(""));
    s.verify_ssl = false;
    let client = HydrusClient::new(&s).unwrap();
    assert!(client.test_connection().await.unwrap());
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn https_with_checks_on_trusts_a_root_the_caller_adds() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"ok": 1}))).await;
    let cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    let client = HydrusClient::with_net_config(&settings(&server.url("")), cfg).unwrap();
    assert!(client.test_connection().await.unwrap());
}

#[tokio::test]
async fn the_settings_switch_wins_over_the_net_config() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"ok": 1}))).await;
    // `verify_tls: false` in the config is overridden by `verify_ssl: true` in the settings.
    let cfg = NetConfig::default().without_tls_verification();
    let client = HydrusClient::with_net_config(&settings(&server.url("")), cfg).unwrap();
    assert!(client.test_connection().await.is_err());
}

// ---- errors: one timeout, one network error, the status mapping ----

#[tokio::test]
async fn a_timeout_is_a_typed_net_error_after_the_configured_wait() {
    let server = TestServer::http(|_| json_reply(json!({})).delayed(Duration::from_secs(3))).await;
    let mut s = settings(&server.url(""));
    s.timeout = 0.3;
    let client = HydrusClient::new(&s).unwrap();
    let started = std::time::Instant::now();
    let err = client.test_connection().await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Net(net::ErrorKind::Timeout), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(err.status_code(), None);
}

#[tokio::test]
async fn a_network_error_names_the_api_url_and_carries_no_status() {
    let (server, client) = world(|_| Reply::aborted()).await;
    let err = client.get_services().await.unwrap_err();
    assert!(matches!(err.kind(), ErrorKind::Net(_)), "{err}");
    assert_eq!(err.status_code(), None);
    assert!(
        err.message()
            .starts_with(&format!("Could not reach Hydrus at {}: ", server.url(""))),
        "{err}"
    );
}

#[tokio::test]
async fn an_error_for_a_person_has_no_url_query_key_or_os_text() {
    // A request with a query, an access key, and a refused connection.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let url = format!("http://127.0.0.1:{port}");
    let client = HydrusClient::new(&settings(&url)).unwrap();
    let errors: Vec<HydrusError> = vec![
        client
            .search_files(&["secret_tag".to_owned()])
            .await
            .unwrap_err(),
        client
            .get_url_info("http://example.com/p?token=abc")
            .await
            .unwrap_err(),
    ];
    for err in errors {
        let shown = err.user_message();
        for forbidden in [
            KEY,
            "secret_tag",
            "token",
            "?",
            "os error",
            "Connection refused",
            "127.0.0.1",
        ] {
            assert!(!shown.contains(forbidden), "{forbidden} in {shown:?}");
        }
        assert!(!shown.is_empty());
        // The raw text is for the log, and holds no key either.
        assert!(!err.detail().contains(KEY));
        assert!(!err.detail().contains("secret_tag"), "{}", err.detail());
        assert!(!err.detail().contains("token=abc"), "{}", err.detail());
    }
}

#[tokio::test]
async fn a_401_a_403_and_other_statuses_are_told_apart_by_status() {
    let (_s, client) = world(|r| match r.target.as_str() {
        "/get_services" => Reply::new(401, "no"),
        "/verify_access_key" => Reply::new(403, "x".repeat(300)),
        _ => Reply::new(500, "Q".repeat(400)),
    })
    .await;
    let e401 = client.get_services().await.unwrap_err();
    assert_eq!(e401.kind(), ErrorKind::AccessKeyRejected);
    assert_eq!(e401.status_code(), Some(401));
    assert_eq!(
        e401.message(),
        "Hydrus rejected the access key (401) - check Settings > Hydrus"
    );
    let e403 = client.test_connection().await.unwrap_err();
    assert_eq!(e403.kind(), ErrorKind::PermissionDenied);
    assert_eq!(e403.status_code(), Some(403));
    assert_eq!(
        e403.message(),
        format!(
            "Access key lacks permission for this action (403): {}",
            "x".repeat(200)
        )
    );
    let e500 = client.get_url_info("u").await.unwrap_err();
    assert_eq!(e500.kind(), ErrorKind::HttpStatus);
    assert_eq!(e500.status_code(), Some(500));
    assert_eq!(
        e500.message(),
        format!("Hydrus returned HTTP 500: {}", "Q".repeat(300))
    );
    assert!(!e500.user_message().contains('Q'));
}

#[tokio::test]
async fn a_429_is_not_retried_because_hydrus_calls_bypass_the_net_retry() {
    let (server, client) = world(|_| Reply::new(429, "slow down").header("Retry-After", "1")).await;
    let err = client.get_services().await.unwrap_err();
    assert_eq!(err.status_code(), Some(429));
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn a_200_with_an_unparseable_body_is_an_empty_object_not_a_crash() {
    let (_s, client) = world(|_| Reply::ok("not json at all")).await;
    assert!(!client.test_connection().await.unwrap());
    assert_eq!(client.get_services().await.unwrap(), json!({}));
}

#[tokio::test]
async fn no_access_key_means_no_access_key_header() {
    let server = TestServer::http(|_| json_reply(json!({}))).await;
    let mut s = settings(&server.url(""));
    s.access_key = Secret::new("");
    let client = HydrusClient::new(&s).unwrap();
    client.test_connection().await.unwrap();
    assert_eq!(
        server.recorded()[0].header("hydrus-client-api-access-key"),
        None
    );
}

// ---- the readers ----

#[tokio::test]
async fn tag_services_are_filtered_by_hydrus_own_type_label() {
    let (_s, client) = world(|_| {
        json_reply(json!({"services": {
            "aa": {"name": "my tags", "type_pretty": "local tag service"},
            "bb": {"name": "public tag repo", "type_pretty": "tag repository"},
            "cc": {"name": "my files", "type_pretty": "local file domain"},
            "dd": {"name": "favourites", "type_pretty": "local rating like service"},
            "ee": {"type_pretty": "local tag service"},
        }}))
    })
    .await;
    assert_eq!(
        client.list_tag_services().await.unwrap(),
        vec![
            ("aa".to_owned(), "my tags".to_owned()),
            ("bb".to_owned(), "public tag repo".to_owned()),
            ("ee".to_owned(), "ee".to_owned()),
        ]
    );
}

#[tokio::test]
async fn a_response_with_no_services_key_is_an_empty_mapping() {
    let (_s, client) = world(|_| json_reply(json!({}))).await;
    assert_eq!(client.get_services().await.unwrap(), json!({}));
    assert!(client.list_tag_services().await.unwrap().is_empty());
}

#[tokio::test]
async fn search_files_json_encodes_the_tag_list_the_way_requests_does() {
    let (server, client) = world(|_| json_reply(json!({"file_ids": [1, 2]}))).await;
    let ids = client
        .search_files(&[
            "cat".to_owned(),
            "dog \"x\"".to_owned(),
            "caf\u{e9}".to_owned(),
        ])
        .await
        .unwrap();
    assert_eq!(ids, vec![1, 2]);
    // json.dumps(["cat", "dog \"x\"", "café"]) with quote_plus, then requote_uri.
    assert_eq!(
        target(&server, 0),
        "/get_files/search_files?tags=%5B%22cat%22%2C+%22dog+%5C%22x%5C%22%22%2C+%22caf%5Cu00e9%22%5D&file_sort_type=0"
    );
}

#[tokio::test]
async fn file_metadata_becomes_rows_with_their_current_tags_only() {
    let (server, client) = world(|_| {
        json_reply(json!({"metadata": [{
            "file_id": 4, "hash": H1, "mime": "image/png", "width": 10, "height": 20,
            "tags": {
                "aa": {"display_tags": {"0": ["cat", "blue"], "1": ["pending"]}},
                "bb": {"storage_tags": {"0": ["blue", "ns:dog"]}},
                "cc": "not a dict",
            },
        }, {"file_id": 5}]}))
    })
    .await;
    let rows = client.get_file_metadata(&[4, 5]).await.unwrap();
    assert_eq!(
        target(&server, 0),
        "/get_files/file_metadata?file_ids=%5B4%2C+5%5D"
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].file_id,
            rows[0].hash.as_str(),
            rows[0].mime.as_deref()
        ),
        (Some(4), H1, Some("image/png"))
    );
    assert_eq!((rows[0].width, rows[0].height), (Some(10), Some(20)));
    // Only status "0" (current) tags; "1" is pending, not on the file. Sorted, de-duplicated.
    assert_eq!(rows[0].tags, vec!["blue", "cat", "ns:dog"]);
    assert_eq!(rows[1].hash, "");
    assert!(rows[1].tags.is_empty());
    assert!(HydrusFile::new(Some(1), "aa").tags.is_empty());
}

#[tokio::test]
async fn tags_and_sizes_come_back_by_hash_in_order_and_only_for_known_hashes() {
    let (server, client) = world(|_| {
        json_reply(json!({"metadata": [
            {"hash": H2, "size": 20, "tags": {"a": {"display_tags": {"0": ["z"]}}}},
            {"hash": H1, "size": 10, "tags": {"a": {"display_tags": {"0": ["x", "y"]}}}},
            {"hash": H2, "size": 21, "tags": {}},
            {"file_id": 9},
        ]}))
    })
    .await;
    let found = client
        .get_tags_and_sizes_for_hashes(&[H1.to_owned(), H2.to_owned()])
        .await
        .unwrap();
    assert_eq!(
        target(&server, 0),
        format!("/get_files/file_metadata?hashes=%5B%22{H1}%22%2C+%22{H2}%22%5D")
    );
    // A hash seen twice keeps its first place and takes its last value, as a dict does.
    let order: Vec<&str> = found.iter().map(|(h, _)| h).collect();
    assert_eq!(order, vec![H2, H1]);
    assert_eq!(found.get(H2).unwrap().size, Some(21));
    assert!(found.get(H2).unwrap().tags.is_empty());
    assert_eq!(found.get(H1).unwrap().tags, vec!["x", "y"]);
    let plain = client
        .get_tags_for_hashes(&[H1.to_owned(), H2.to_owned()])
        .await
        .unwrap();
    assert_eq!(
        plain[1],
        (H1.to_owned(), vec!["x".to_owned(), "y".to_owned()])
    );
}

#[tokio::test]
async fn an_empty_hash_list_makes_no_request() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    assert!(
        client
            .get_tags_and_sizes_for_hashes(&[])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(client.get_tags_for_hashes(&[]).await.unwrap().is_empty());
    assert!(client.filter_known_hashes(&[]).await.unwrap().is_empty());
    assert_eq!(client.file_id_for_hash("").await.unwrap(), None);
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn known_hashes_asks_for_identifiers_only_and_a_null_file_id_is_not_known() {
    let (server, client) = world(|_| {
        json_reply(json!({"metadata": [
            {"hash": H1, "file_id": 1234},
            {"hash": H2, "file_id": null},
            {"file_id": 7},
        ]}))
    })
    .await;
    let known = client
        .filter_known_hashes(&[H1.to_owned(), H2.to_owned()])
        .await
        .unwrap();
    assert_eq!(known.into_iter().collect::<Vec<_>>(), vec![H1.to_owned()]);
    assert!(target(&server, 0).ends_with("&only_return_identifiers=true"));
}

#[tokio::test]
async fn a_failed_known_hashes_call_is_an_empty_set_but_a_misshapen_answer_is_not() {
    let (_s, client) = world(|_| Reply::new(500, "boom")).await;
    assert!(
        client
            .filter_known_hashes(&[H1.to_owned()])
            .await
            .unwrap()
            .is_empty()
    );
    let (_s, client) = world(|_| json_reply(json!([1, 2]))).await;
    let err = client
        .filter_known_hashes(&[H1.to_owned()])
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(err.message(), "'list' object has no attribute 'get'");
}

#[tokio::test]
async fn missing_permission_names_the_checkbox_and_says_where() {
    let (_s, client) = world(|_| json_reply(json!({"basic_permissions": [0, 1, 3]}))).await;
    let message = client
        .missing_permission(PERMISSION_EDIT_FILE_RELATIONSHIPS)
        .await
        .unwrap()
        .unwrap();
    assert!(message.contains("\"Edit File Relationships\""), "{message}");
    assert!(message.contains("review services"));
    assert!(message.starts_with("your Hydrus access key does not have the"));
    assert_eq!(client.missing_permission(3).await.unwrap(), None);
    assert!(
        client
            .missing_permission(99)
            .await
            .unwrap()
            .unwrap()
            .contains("\"permission 99\"")
    );
}

#[tokio::test]
async fn a_catch_all_key_and_an_inconclusive_probe_are_not_missing() {
    let (_s, client) =
        world(|_| json_reply(json!({"permits_everything": true, "basic_permissions": []}))).await;
    assert_eq!(client.missing_permission(8).await.unwrap(), None);
    let (_s, client) = world(|_| json_reply(json!({}))).await;
    assert_eq!(client.missing_permission(8).await.unwrap(), None);
    let (_s, client) = world(|_| json_reply(json!({"basic_permissions": "all"}))).await;
    assert_eq!(client.missing_permission(8).await.unwrap(), None);
    // Hydrus unreachable or refusing: inconclusive, so nothing is reported missing.
    let (_s, client) = world(|_| Reply::new(403, "no")).await;
    assert_eq!(client.missing_permission(8).await.unwrap(), None);
    let (_s, client) = world(|_| Reply::aborted()).await;
    assert_eq!(client.missing_permission(8).await.unwrap(), None);
}

#[tokio::test]
async fn a_thumbnail_is_bytes_and_every_failure_is_a_typed_error() {
    let (server, client) = world(|r| match r.target.as_str() {
        t if t.ends_with(H1) => Reply::ok(b"JPEGDATA".to_vec()),
        t if t.ends_with(H2) => Reply::new(500, "no"),
        _ => Reply::new(403, "denied"),
    })
    .await;
    assert_eq!(client.fetch_thumbnail(H1).await.unwrap(), b"JPEGDATA");
    assert_eq!(
        target(&server, 0),
        format!("/get_files/thumbnail?hash={H1}")
    );
    let err = client.fetch_thumbnail(H2).await.unwrap_err();
    assert_eq!(
        (err.kind(), err.status_code()),
        (ErrorKind::HttpStatus, Some(500))
    );
    let err = client.fetch_thumbnail("cc").await.unwrap_err();
    assert_eq!(
        (err.kind(), err.status_code()),
        (ErrorKind::PermissionDenied, Some(403))
    );
    // A transport failure is an Err too, where Python answered None.
    let (_s, client) = world(|_| Reply::aborted()).await;
    assert!(matches!(
        client.fetch_thumbnail(H1).await.unwrap_err().kind(),
        ErrorKind::Net(_)
    ));
}

#[tokio::test]
async fn file_id_for_hash_matches_case_insensitively_and_a_null_id_is_none() {
    let (server, client) = world(|_| {
        json_reply(json!({"metadata": [
            {"hash": H2, "file_id": 5},
            {"hash": H1.to_uppercase(), "file_id": 41},
        ]}))
    })
    .await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), Some(41));
    assert!(target(&server, 0).ends_with("&only_return_identifiers=true"));
    let (_s, client) =
        world(|_| json_reply(json!({"metadata": [{"hash": H1, "file_id": null}]}))).await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), None);
    let (_s, client) =
        world(|_| json_reply(json!({"metadata": [{"hash": H2, "file_id": 5}]}))).await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), None);
    let (_s, client) = world(|_| json_reply(json!({}))).await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), None);
    let (_s, client) = world(|_| Reply::new(500, "x")).await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), None);
    // `int("41")` is 41 and `int("x")` is a ValueError that nothing catches.
    let (_s, client) =
        world(|_| json_reply(json!({"metadata": [{"hash": H1, "file_id": "41"}]}))).await;
    assert_eq!(client.file_id_for_hash(H1).await.unwrap(), Some(41));
    let (_s, client) =
        world(|_| json_reply(json!({"metadata": [{"hash": H1, "file_id": "x"}]}))).await;
    let err = client.file_id_for_hash(H1).await.unwrap_err();
    assert_eq!(err.message(), "invalid literal for int() with base 10: 'x'");
}

#[tokio::test]
async fn url_info_and_url_files_ask_their_endpoints_with_the_url_as_a_parameter() {
    let (server, client) = world(|_| json_reply(json!({"url_type": 0}))).await;
    assert_eq!(
        client
            .get_url_info("http://example.com/post/1?a=b c")
            .await
            .unwrap(),
        json!({"url_type": 0})
    );
    assert_eq!(
        client
            .get_url_files("http://example.com/post/1")
            .await
            .unwrap(),
        json!({"url_type": 0})
    );
    assert_eq!(
        target(&server, 0),
        "/add_urls/get_url_info?url=http%3A%2F%2Fexample.com%2Fpost%2F1%3Fa%3Db+c"
    );
    assert_eq!(
        target(&server, 1),
        "/add_urls/get_url_files?url=http%3A%2F%2Fexample.com%2Fpost%2F1"
    );
    let (_s, client) = world(|_| Reply::new(500, "boom")).await;
    assert!(client.get_url_info("u").await.is_err());
    assert!(client.get_url_files("u").await.is_err());
}

#[tokio::test]
async fn a_service_name_resolves_to_its_key_and_a_key_is_passed_through() {
    let (server, client) = world(|_| {
        json_reply(json!({"services": {
            "aabb": {"name": " My Tags ", "type_pretty": "local tag service"},
            "ccdd": {"name": "downloader tags", "type_pretty": "tag repository"},
        }}))
    })
    .await;
    let key = "6c6f63616c2074616773";
    assert_eq!(
        client.resolve_to_service_key(key).await.unwrap(),
        Some(key.to_owned())
    );
    assert_eq!(server.request_count(), 0, "a key needs no lookup");
    assert_eq!(
        client.resolve_to_service_key("  my tags").await.unwrap(),
        Some("aabb".to_owned())
    );
    assert_eq!(
        client
            .resolve_to_service_key("downloader tags")
            .await
            .unwrap(),
        Some("ccdd".to_owned())
    );
    assert_eq!(client.resolve_to_service_key("nope").await.unwrap(), None);
    assert_eq!(client.resolve_to_service_key("").await.unwrap(), None);
    let (_s, client) = world(|_| Reply::aborted()).await;
    assert_eq!(
        client.resolve_to_service_key("my tags").await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_download_is_written_whole_and_the_key_stays_out_of_the_url() {
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let served = body.clone();
    let (server, client) = world(move |_| Reply::ok(served.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    client.download_file(77, &dest).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
    assert_eq!(target(&server, 0), "/get_files/file?file_id=77");
    assert_eq!(
        server.recorded()[0].header("hydrus-client-api-access-key"),
        Some(KEY)
    );
    assert_eq!(
        client.get_file_path_url(3),
        format!("{}/get_files/file?file_id=3", server.url(""))
    );
}

#[tokio::test]
async fn a_failed_download_leaves_no_file_and_says_which_one() {
    let (_s, client) = world(|_| Reply::new(404, "gone")).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let err = client.download_file(5, &dest).await.unwrap_err();
    // Python's message names the status; its exception carries none.
    assert_eq!(err.status_code(), None);
    assert_eq!(err.kind(), ErrorKind::HttpStatus);
    assert_eq!(err.message(), "Hydrus returned HTTP 404 for file 5");
    assert!(!dest.exists());
    let (_s, client) = world(|_| Reply::aborted()).await;
    let err = client.download_file(9, &dest).await.unwrap_err();
    assert!(
        err.message().starts_with("Could not download file 9: "),
        "{err}"
    );
    assert_eq!(err.status_code(), None);
    assert!(!dest.exists());
}

#[tokio::test]
async fn an_unwritable_destination_is_an_io_error_that_names_no_path_for_a_person() {
    let (_s, client) = world(|_| Reply::ok("data")).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("missing-folder").join("out.bin");
    let err = client.download_file(1, &dest).await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Io);
    let shown = err.user_message();
    assert!(
        !shown.contains("missing-folder") && !shown.contains("os error"),
        "{shown}"
    );
    assert!(
        err.message()
            .starts_with("[Errno 2] No such file or directory: '"),
        "{err}"
    );
}

#[tokio::test]
async fn a_download_over_https_checks_the_certificate_like_every_other_call() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| Reply::ok("data")).await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let client = HydrusClient::new(&settings(&server.url(""))).unwrap();
    assert_eq!(
        client.download_file(1, &dest).await.unwrap_err().kind(),
        ErrorKind::Net(net::ErrorKind::Tls)
    );
    let mut s = settings(&server.url(""));
    s.verify_ssl = false;
    HydrusClient::new(&s)
        .unwrap()
        .download_file(1, &dest)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"data");
}

// ---- `_post`, which the write half is built on ----

#[tokio::test]
async fn a_json_post_sends_python_json_and_an_absent_body_is_an_empty_object() {
    let (server, client) = world(|_| json_reply(json!({"page_key": "k"}))).await;
    let body = json!({"hashes": ["a", "b"], "n": 1, "f": 1.5, "t": true, "z": null, "s": "caf\u{e9}", "e": {}});
    let answer = client
        .post_json("/manage_pages/new_page", Some(&body), &[])
        .await
        .unwrap();
    assert_eq!(answer["page_key"], "k");
    client.post_json("/x", None, &[]).await.unwrap();
    let seen = server.recorded();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        String::from_utf8(seen[0].body.clone()).unwrap(),
        "{\"hashes\": [\"a\", \"b\"], \"n\": 1, \"f\": 1.5, \"t\": true, \"z\": null, \"s\": \"caf\\u00e9\", \"e\": {}}"
    );
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert_eq!(seen[0].header("hydrus-client-api-access-key"), Some(KEY));
    assert_eq!(seen[1].body, b"{}");
}

#[tokio::test]
async fn a_raw_post_sends_bytes_as_they_are_with_the_callers_headers() {
    let (server, client) = world(|_| json_reply(json!({"status": 1}))).await;
    client
        .post_raw(
            "/add_files/add_file",
            bytes::Bytes::from_static(b"\x00\x01\xff"),
            &[("Content-Type", "application/octet-stream")],
        )
        .await
        .unwrap();
    let seen = &server.recorded()[0];
    assert_eq!(seen.body, b"\x00\x01\xff");
    assert_eq!(
        seen.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(seen.header("hydrus-client-api-access-key"), Some(KEY));
}

#[tokio::test]
async fn a_caller_header_replaces_the_default_one_of_the_same_name() {
    let (server, client) = world(|_| json_reply(json!({}))).await;
    let body = json!({"a": 1});
    client
        .post_json("/x", Some(&body), &[("content-type", "text/plain")])
        .await
        .unwrap();
    assert_eq!(
        server.recorded()[0].header("content-type"),
        Some("text/plain")
    );
}

// ---- answers shaped the way Python would raise on ----

#[tokio::test]
async fn an_answer_python_would_raise_an_attribute_error_on_is_an_unexpected_response() {
    let cases: Vec<(Value, &str)> = vec![
        (json!([1]), "'list' object has no attribute 'get'"),
        (json!("text"), "'str' object has no attribute 'get'"),
        (json!(5), "'int' object has no attribute 'get'"),
        (json!(1.5), "'float' object has no attribute 'get'"),
        (json!(true), "'bool' object has no attribute 'get'"),
        (json!(null), "'NoneType' object has no attribute 'get'"),
    ];
    for (body, text) in cases {
        let b = body.clone();
        let (_s, client) = world(move |_| json_reply(b.clone())).await;
        let err = client.get_services().await.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnexpectedResponse, "{body}");
        assert_eq!(err.message(), text, "{body}");
        assert!(err.user_message().contains("cannot read"));
    }
}

#[tokio::test]
async fn metadata_rows_are_read_as_python_iterates_them() {
    let (_s, client) = world(|_| json_reply(json!({"metadata": 5}))).await;
    let err = client.get_file_metadata(&[1]).await.unwrap_err();
    assert_eq!(err.message(), "'int' object is not iterable");
    let (_s, client) = world(|_| json_reply(json!({"metadata": ["row"]}))).await;
    let err = client.get_file_metadata(&[1]).await.unwrap_err();
    assert_eq!(err.message(), "'str' object has no attribute 'get'");
    let (_s, client) = world(|_| json_reply(json!({"metadata": [{"tags": [1]}]}))).await;
    let err = client.get_file_metadata(&[1]).await.unwrap_err();
    assert_eq!(err.message(), "'list' object has no attribute 'values'");
}
