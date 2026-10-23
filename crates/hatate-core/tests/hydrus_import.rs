//! `core/hydrus_import.py`, the file-upload half (Phase 4.c, DAN-2130), through `HydrusClient::new`
//! (the binaries' constructor: rustls with the explicit provider) against a scripted local Hydrus,
//! plain http and https. Every answer is scripted, every access key is a synthetic placeholder,
//! and no live Hydrus is contacted.
//!
//! The transport differential against the real Python is the broad check
//! (`worktrees/DAN-2130-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/hydrus-import.md` section 6.

mod common;

use std::path::{Path, PathBuf};

use common::{Recorded, Reply, Sandbox, TestPki, TestServer};
use hatate_core::clock::{BoxFuture, Clock};
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{HydrusClient, PERMISSION_EDIT_FILE_NOTES};
use hatate_core::hydrus_import as hi;
use hatate_core::model::{Candidate, Entry, Tag, TagSource};
use hatate_core::net;
use serde_json::{Value, json};

/// A synthetic access key: 64 zeros.
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const NEW: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCAL: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 2026-10-23 12:34:56 UTC.
struct FixedClock;

impl Clock for FixedClock {
    fn wall(&self) -> f64 {
        1_792_758_896.0
    }
    fn monotonic(&self) -> f64 {
        0.0
    }
    fn sleep(&self, _: f64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

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

/// A Hydrus that answers by endpoint. `holds` is what `file_metadata` says about every hash.
#[derive(Clone)]
struct Fake {
    add_file: Value,
    holds: bool,
    permissions: Value,
    fail: Vec<&'static str>,
}

impl Fake {
    fn new() -> Self {
        Fake {
            add_file: json!({"status": 1, "hash": NEW, "note": ""}),
            holds: true,
            permissions: json!({"permits_everything": true}),
            fail: Vec::new(),
        }
    }

    fn answer(&self, q: &Recorded) -> Reply {
        let path = q.target.split('?').next().unwrap_or("");
        if self.fail.iter().any(|f| path.ends_with(f)) {
            return Reply::new(500, "boom");
        }
        match path {
            "/add_files/add_file" => json_reply(self.add_file.clone()),
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": NEW, "file_id": 7, "is_local": self.holds, "is_trashed": false, "is_deleted": !self.holds},
                {"hash": LOCAL, "file_id": 8, "is_local": self.holds, "is_trashed": false, "is_deleted": !self.holds},
            ]})),
            "/verify_access_key" => json_reply(self.permissions.clone()),
            _ => json_reply(json!({})),
        }
    }
}

async fn world(fake: Fake) -> (TestServer, HydrusClient) {
    let server = TestServer::http(move |q| fake.answer(q)).await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    (server, client)
}

fn targets(server: &TestServer) -> Vec<String> {
    server
        .recorded()
        .iter()
        .map(|q| q.target.split('?').next().unwrap().to_owned())
        .collect()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hatate-dan2130-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An entry for a real file on disk, matched to `url` by `engine`.
fn entry_for(dir: &Path, url: Option<&str>) -> Entry {
    let path = dir.join("a.png");
    std::fs::write(
        &path,
        png_bytes(40, 40, |x, y| [(x * 6) as u8, (y * 6) as u8, 90]),
    )
    .unwrap();
    let mut e = Entry::new(path.to_str().unwrap());
    if let Some(url) = url {
        let mut c = Candidate::new(url);
        c.engine = "SauceNAO".to_owned();
        c.similarity = 91.0;
        c.source_name = Some("Danbooru".to_owned());
        e.candidates.push(c);
        e.select_candidate(0);
    }
    e
}

fn png_bytes(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb(f(x, y));
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn tag(ns: Option<&str>, name: &str, source: TagSource) -> Tag {
    Tag {
        name: name.to_owned(),
        namespace: ns.map(str::to_owned),
        source,
    }
}

fn settings_with(f: impl FnOnce(&mut Settings)) -> Settings {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    f(&mut s);
    s
}

// ---- the two schemes, through the binaries' own constructor (DAN-1909) ----

#[tokio::test]
async fn an_upload_over_the_default_plain_http_scheme_sends_file_url_and_tags_in_order() {
    let (server, client) = world(Fake::new()).await;
    let dir = scratch("http");
    let mut entry = entry_for(&dir, Some("https://danbooru.donmai.us/posts/1"));
    entry
        .tags
        .push(tag(Some("creator"), "someone", TagSource::Booru));
    entry.tags.push(tag(None, "tree", TagSource::User));

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert_eq!(result, hi::ImportResult::ok());
    assert_eq!(
        targets(&server),
        [
            "/add_files/add_file",
            "/add_urls/associate_url",
            "/add_tags/add_tags"
        ]
    );
    let seen = server.recorded();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].header("hydrus-client-api-access-key"), Some(KEY));
    assert_eq!(
        seen[0].body,
        std::fs::read(&entry.path).unwrap(),
        "the file's own bytes"
    );
    let url_body: Value = serde_json::from_slice(&seen[1].body).unwrap();
    assert_eq!(
        url_body,
        json!({"url_to_add": "https://danbooru.donmai.us/posts/1", "hashes": [NEW]})
    );
    let tag_body: Value = serde_json::from_slice(&seen[2].body).unwrap();
    assert_eq!(tag_body["hash"], NEW);
    assert_eq!(
        tag_body["service_names_to_tags"]["my tags"],
        json!(["creator:someone", "tree"])
    );
    assert_eq!(entry.hydrus_hash.as_deref(), Some(NEW));
    assert!(entry.sent_to_hydrus && entry.hydrus_import_confirmed);
    assert_eq!(entry.error_message, None);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_over_https_with_certificate_checks_on_is_refused_and_fails_the_import() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"status": 1, "hash": NEW}))).await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let dir = scratch("https-on");
    let mut entry = entry_for(&dir, None);

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert!(!result.success);
    let fault = result
        .fault
        .as_ref()
        .expect("the typed error behind `error`");
    assert_eq!(fault.kind(), ErrorKind::Net(net::ErrorKind::Tls), "{fault}");
    assert_eq!(server.request_count(), 0, "the handshake was refused");
    assert!(
        result
            .error
            .as_deref()
            .unwrap()
            .starts_with("Could not reach Hydrus at ")
    );
    assert_eq!(entry.error_message, result.error);
    assert!(!entry.sent_to_hydrus);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_over_https_with_certificate_checks_off_reaches_a_self_signed_server() {
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| json_reply(json!({"status": 1, "hash": NEW}))).await;
    let mut hs = hydrus_settings(&server.url(""));
    hs.verify_ssl = false;
    let client = HydrusClient::new(&hs).unwrap();
    let dir = scratch("https-off");
    let mut entry = entry_for(&dir, Some("https://e.example/p/1"));

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert_eq!(result, hi::ImportResult::ok());
    assert_eq!(server.request_count(), 2, "upload and URL; no tags to send");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- failures: one timeout, one network error, the user text ----

#[tokio::test]
async fn an_upload_that_times_out_fails_the_import_with_a_timeout_kind() {
    let server =
        TestServer::http(|_| json_reply(json!({})).delayed(std::time::Duration::from_secs(3)))
            .await;
    let mut hs = hydrus_settings(&server.url(""));
    hs.timeout = 0.3;
    let client = HydrusClient::new(&hs).unwrap();
    let dir = scratch("timeout");
    let mut entry = entry_for(&dir, None);

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert!(!result.success);
    let fault = result.fault.unwrap();
    assert_eq!(
        fault.kind(),
        ErrorKind::Net(net::ErrorKind::Timeout),
        "{fault}"
    );
    let user = fault.user_message();
    assert!(
        !user.contains("os error") && !user.contains(KEY) && !user.contains('?'),
        "{user}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_to_a_server_that_drops_the_connection_fails_with_a_connection_error() {
    let server = TestServer::http(|_| Reply::aborted()).await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();
    let dir = scratch("network");
    let mut entry = entry_for(&dir, None);

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert!(!result.success);
    let fault = result.fault.unwrap();
    assert!(matches!(fault.kind(), ErrorKind::Net(_)), "{fault}");
    let user = fault.user_message();
    assert!(!user.contains("os error") && !user.contains(KEY), "{user}");
    assert!(
        entry
            .error_message
            .as_deref()
            .unwrap()
            .starts_with("Could not reach Hydrus at ")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_missing_local_file_fails_the_import_and_the_user_message_has_neither_path_nor_os_text() {
    let (server, client) = world(Fake::new()).await;
    let mut entry = Entry::new("/nonexistent-dan2130/secret-name.png");

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert!(!result.success);
    let error = result.error.as_deref().unwrap();
    assert!(
        error.starts_with("Could not read local file /nonexistent-dan2130/secret-name.png: "),
        "{error}"
    );
    assert_eq!(
        entry.error_message.as_deref(),
        Some(error),
        "Python's text, byte for byte"
    );
    let fault = result.fault.unwrap();
    assert_eq!(fault.kind(), ErrorKind::FileUnreadable);
    let user = fault.user_message();
    assert!(
        !user.contains("secret-name") && !user.contains("/nonexistent"),
        "{user}"
    );
    assert!(!user.to_lowercase().contains("os error"), "{user}");
    assert!(
        fault.detail().contains("secret-name"),
        "the detail keeps the path"
    );
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn hydrus_answering_no_hash_fails_the_import_but_keeps_what_the_entry_had() {
    let mut fake = Fake::new();
    fake.add_file = json!({"status": 1});
    let (server, client) = world(fake).await;
    let dir = scratch("nohash");
    let mut entry = entry_for(&dir, None);

    // Nothing in the answer and nothing on the entry.
    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();
    assert_eq!(
        result.error.as_deref(),
        Some("Hydrus didn't return a file hash for this import")
    );
    assert_eq!(entry.hydrus_hash, None);
    assert!(!entry.sent_to_hydrus);

    // The hash the entry already had is used, as Python's `result.get("hash") or entry.hydrus_hash`.
    entry.hydrus_hash = Some(LOCAL.to_owned());
    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(entry.hydrus_hash.as_deref(), Some(LOCAL));
    assert_eq!(server.request_count(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_import_hydrus_reports_as_failed_is_a_failure_with_its_note() {
    let mut fake = Fake::new();
    fake.add_file = json!({"status": 4, "note": "unsupported filetype"});
    let (_server, client) = world(fake).await;
    let dir = scratch("rejected");
    let mut entry = entry_for(&dir, None);

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        result.error.as_deref(),
        Some("Hydrus rejected the file: unsupported filetype")
    );
    assert_eq!(result.fault.unwrap().kind(), ErrorKind::ImportRejected);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failed_url_step_and_a_failed_tag_step_are_warnings_that_keep_the_import() {
    let mut fake = Fake::new();
    fake.fail = vec!["/add_urls/associate_url", "/add_tags/add_tags"];
    let (_server, client) = world(fake).await;
    let dir = scratch("warnings");
    let mut entry = entry_for(&dir, Some("https://e.example/p/1"));
    entry.tags.push(tag(None, "tree", TagSource::User));

    let result = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap();

    assert!(result.success);
    let warning = result.warning.unwrap();
    assert!(warning.starts_with("URL not added: "), "{warning}");
    assert!(warning.contains("; Tags not added: "), "{warning}");
    assert_eq!(entry.error_message.as_deref(), Some(warning.as_str()));
    assert!(entry.sent_to_hydrus && entry.hydrus_import_confirmed);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_answer_that_is_not_the_shape_the_code_reads_propagates_like_python_s_attribute_error() {
    let mut fake = Fake::new();
    fake.add_file = json!(["not", "a", "dict"]);
    let (_server, client) = world(fake).await;
    let dir = scratch("shape");
    let mut entry = entry_for(&dir, None);

    let err = hi::send_file_upload(&mut entry, &client, None, &FixedClock)
        .await
        .unwrap_err();

    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    assert_eq!(
        entry.error_message, None,
        "Python's `except HydrusError` does not catch it"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- which tags go with the file ----

#[tokio::test]
async fn the_source_filter_decides_which_tags_are_sent_only_when_it_is_on() {
    let dir = scratch("filter");
    let mut entry = entry_for(&dir, None);
    entry.tags.push(tag(None, "from-booru", TagSource::Booru));
    entry
        .tags
        .push(tag(None, "from-engine", TagSource::SearchEngine));

    for (filter, sources, want) in [
        (false, vec!["Booru"], json!(["from-booru", "from-engine"])),
        (true, vec!["Booru"], json!(["from-booru"])),
        (true, vec![], json!(null)),
    ] {
        let (server, client) = world(Fake::new()).await;
        let settings = settings_with(|s| {
            s.filter_sent_tags_by_source = filter;
            s.enabled_tag_sources = sources.iter().map(|x| (*x).to_owned()).collect();
        });
        let mut e = entry.clone();
        hi::send_file_upload(&mut e, &client, Some(&settings), &FixedClock)
            .await
            .unwrap();
        let tag_call = server
            .recorded()
            .into_iter()
            .find(|q| q.target.starts_with("/add_tags"));
        let got = tag_call
            .map(|q| serde_json::from_slice::<Value>(&q.body).unwrap()["service_names_to_tags"]["my tags"].clone())
            .unwrap_or(Value::Null);
        assert_eq!(got, want, "filter {filter}, sources {sources:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the provenance note ----

#[tokio::test]
async fn the_note_is_off_by_default_and_then_costs_no_request() {
    let (server, client) = world(Fake::new()).await;
    let dir = scratch("note-off");
    let entry = entry_for(&dir, Some("https://e.example/p/1"));
    let off = settings_with(|s| s.write_hydrus_provenance_note = false);

    for settings in [None, Some(&off)] {
        let got = hi::write_provenance_note(&entry, &client, settings, NEW, &FixedClock)
            .await
            .unwrap();
        assert_eq!(got, None);
    }
    assert_eq!(server.request_count(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_note_text_name_and_date_are_written_after_the_hash_is_confirmed_held() {
    let (server, client) = world(Fake::new()).await;
    let dir = scratch("note-on");
    let entry = entry_for(&dir, Some("https://e.example/p/1"));
    let on = settings_with(|s| {
        s.write_hydrus_provenance_note = true;
        s.hydrus_provenance_note_name = "  my note ".to_owned();
    });

    let got = hi::write_provenance_note(&entry, &client, Some(&on), NEW, &FixedClock)
        .await
        .unwrap();

    assert_eq!(got, None);
    assert_eq!(
        targets(&server),
        [
            "/get_files/file_metadata",
            "/verify_access_key",
            "/add_notes/set_notes"
        ]
    );
    let body: Value = serde_json::from_slice(&server.recorded()[2].body).unwrap();
    assert_eq!(body["hash"], NEW);
    let text = body["notes"]["my note"]
        .as_str()
        .expect("named by the trimmed setting");
    assert!(text.starts_with("Match found by Hatate-Linux-Redux.\nEngine: SauceNAO\nSite: Danbooru\nSimilarity: 91% - measured"), "{text}");
    assert!(
        text.ends_with("Match: https://e.example/p/1\nSent to Hydrus: 2026-10-23 12:34"),
        "{text}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn no_note_is_written_for_a_file_hydrus_is_not_holding_or_an_entry_with_no_match() {
    let dir = scratch("note-skip");
    let on = settings_with(|s| s.write_hydrus_provenance_note = true);

    let mut fake = Fake::new();
    fake.holds = false;
    let (server, client) = world(fake).await;
    let matched = entry_for(&dir, Some("https://e.example/p/1"));
    let got = hi::write_provenance_note(&matched, &client, Some(&on), NEW, &FixedClock)
        .await
        .unwrap();
    assert_eq!(
        got, None,
        "previously deleted: add_file still reports a hash"
    );
    assert_eq!(targets(&server), ["/get_files/file_metadata"]);

    let (server, client) = world(Fake::new()).await;
    let unmatched = entry_for(&dir, None);
    let got = hi::write_provenance_note(&unmatched, &client, Some(&on), NEW, &FixedClock)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        server.request_count(),
        0,
        "no match means no provenance, and no request"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_missing_notes_permission_and_a_failed_write_come_back_as_warnings() {
    let dir = scratch("note-warn");
    let on = settings_with(|s| s.write_hydrus_provenance_note = true);
    let entry = entry_for(&dir, Some("https://e.example/p/1"));

    let mut fake = Fake::new();
    fake.permissions = json!({"permits_everything": false, "basic_permissions": [1, 2]});
    let (_s, client) = world(fake).await;
    let got = hi::write_provenance_note(&entry, &client, Some(&on), NEW, &FixedClock)
        .await
        .unwrap();
    let warning = got.expect("a warning");
    assert!(warning.starts_with("Imported, but no source note was written: your Hydrus access key does not have the \"Edit File Notes\" permission"), "{warning}");
    let _ = PERMISSION_EDIT_FILE_NOTES;

    let mut fake = Fake::new();
    fake.fail = vec!["/add_notes/set_notes"];
    let (_s, client) = world(fake).await;
    let got = hi::write_provenance_note(&entry, &client, Some(&on), NEW, &FixedClock)
        .await
        .unwrap();
    assert!(
        got.unwrap()
            .starts_with("Imported, but no source note was written: ")
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the duplicate relationship ----

fn noisy(seed: u32) -> impl Fn(u32, u32) -> [u8; 3] {
    move |x, y| {
        let h = x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ seed.wrapping_mul(83492791);
        [(h >> 3) as u8, (h >> 11) as u8, (h >> 19) as u8]
    }
}

#[tokio::test]
async fn the_same_picture_is_marked_better_and_made_king_but_only_when_the_user_asked() {
    let dir = scratch("relate-same");
    let mut entry = entry_for(&dir, None);
    let pic = png_bytes(64, 64, noisy(1));
    std::fs::write(&entry.path, &pic).unwrap();
    entry.hydrus_hash = Some(LOCAL.to_owned());

    let (server, client) = world(Fake::new()).await;
    let off = settings_with(|s| s.set_hydrus_duplicate_relationships = false);
    let got =
        hi::record_duplicate_relationship(&entry, &client, Some(&off), Some(LOCAL), &pic, NEW)
            .await
            .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        server.request_count(),
        0,
        "off: no comparison and no request"
    );

    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let got = hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &pic, NEW)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        targets(&server),
        [
            "/get_files/file_metadata",
            "/verify_access_key",
            "/manage_file_relationships/set_file_relationships",
            "/manage_file_relationships/set_kings"
        ]
    );
    let rel: Value = serde_json::from_slice(&server.recorded()[2].body).unwrap();
    assert_eq!(rel["relationships"][0]["hash_a"], NEW);
    assert_eq!(rel["relationships"][0]["hash_b"], LOCAL);
    assert_eq!(rel["relationships"][0]["relationship"], 4);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_different_picture_the_same_hash_and_a_file_hydrus_does_not_hold_write_nothing() {
    let dir = scratch("relate-none");
    let mut entry = entry_for(&dir, None);
    let local_pic = png_bytes(64, 64, noisy(1));
    std::fs::write(&entry.path, &local_pic).unwrap();
    let other = png_bytes(64, 64, noisy(2));
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);

    let (server, client) = world(Fake::new()).await;
    let got =
        hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &other, NEW)
            .await
            .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        server.request_count(),
        0,
        "not confident: Hydrus is not touched"
    );

    let got =
        hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(NEW), &local_pic, NEW)
            .await
            .unwrap();
    assert_eq!(got, None);
    let got = hi::record_duplicate_relationship(&entry, &client, Some(&on), None, &local_pic, NEW)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        server.request_count(),
        0,
        "one file, or nothing to pair with"
    );

    let mut fake = Fake::new();
    fake.holds = false;
    let (server, client) = world(fake).await;
    entry.hydrus_hash = Some(LOCAL.to_owned());
    let got =
        hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &local_pic, NEW)
            .await
            .unwrap();
    assert_eq!(got, None);
    assert_eq!(
        targets(&server),
        ["/get_files/file_metadata"],
        "the local copy is not held: no pairing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failed_relationship_write_is_a_warning_and_a_missing_permission_names_it() {
    let dir = scratch("relate-warn");
    let entry = entry_for(&dir, None);
    let pic = png_bytes(64, 64, noisy(1));
    std::fs::write(&entry.path, &pic).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);

    let mut fake = Fake::new();
    fake.fail = vec!["/manage_file_relationships/set_file_relationships"];
    let (_s, client) = world(fake).await;
    let got = hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &pic, NEW)
        .await
        .unwrap();
    assert!(
        got.unwrap()
            .starts_with("Imported, but not marked as a duplicate: ")
    );

    let mut fake = Fake::new();
    fake.permissions = json!({"permits_everything": false, "basic_permissions": []});
    let (server, client) = world(fake).await;
    let got = hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &pic, NEW)
        .await
        .unwrap();
    assert!(
        got.unwrap()
            .contains("\"Edit File Relationships\" permission")
    );
    assert_eq!(server.request_count(), 2, "no write was attempted");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- reading the imported file back ----

#[tokio::test]
async fn the_imported_file_is_read_back_through_its_id_and_the_temp_file_is_gone() {
    let server = TestServer::http(|q| {
        if q.target.starts_with("/get_files/file_metadata") {
            json_reply(json!({"metadata": [{"hash": NEW, "file_id": 7}]}))
        } else if q.target.starts_with("/get_files/file?file_id=7") {
            Reply::ok(b"the original bytes".to_vec())
        } else {
            Reply::new(404, "no")
        }
    })
    .await;
    let client = HydrusClient::new(&hydrus_settings(&server.url(""))).unwrap();

    let got = hi::imported_bytes(&client, NEW).await.unwrap();
    assert_eq!(got.as_deref(), Some(&b"the original bytes"[..]));
    let left: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("hatate-hydrus-relate-")
        })
        .collect();
    assert!(left.is_empty(), "removed once read: {left:?}");

    // No id: nothing to download.
    let server2 = TestServer::http(|_| json_reply(json!({"metadata": []}))).await;
    let client2 = HydrusClient::new(&hydrus_settings(&server2.url(""))).unwrap();
    assert_eq!(hi::imported_bytes(&client2, NEW).await.unwrap(), None);
    assert_eq!(server2.request_count(), 1);
    // A download that fails is "could not read it back", not an error.
    let server3 = TestServer::http(|q| {
        if q.target.starts_with("/get_files/file_metadata") {
            json_reply(json!({"metadata": [{"hash": NEW, "file_id": 7}]}))
        } else {
            Reply::new(500, "boom")
        }
    })
    .await;
    let client3 = HydrusClient::new(&hydrus_settings(&server3.url(""))).unwrap();
    assert_eq!(hi::imported_bytes(&client3, NEW).await.unwrap(), None);
}

// ---- the pure parts ----

#[test]
fn the_web_page_check_reads_the_first_512_bytes_after_leading_whitespace_case_blind() {
    for yes in [
        &b"<!DOCTYPE html>"[..],
        b"  \n\t<HTML>",
        b"<head>",
        b"\x0b\x0c<?xml version",
    ] {
        assert!(hi::looks_like_a_web_page(yes), "{yes:?}");
    }
    let mut late = vec![b' '; 600];
    late.extend_from_slice(b"<html>");
    for no in [&b""[..], b"\x89PNG\r\n", b"<htm", b"<div><html>", &late[..]] {
        assert!(!hi::looks_like_a_web_page(no), "{no:?}");
    }
}

#[test]
fn magic_bytes_follow_the_claimed_format_then_the_extension_then_a_sniff() {
    let png = {
        let mut d = b"\x89PNG\r\n\x1a\n".to_vec();
        d.resize(200, 0);
        d
    };
    let webp = {
        let mut d = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        d.resize(200, 0);
        d
    };
    let mp4 = {
        let mut d = b"\0\0\0\x18ftypmp42".to_vec();
        d.resize(200, 0);
        d
    };
    assert_eq!(hi::validate_magic_bytes(&png, Some("png"), ""), None);
    assert_eq!(
        hi::validate_magic_bytes(&png, Some("JPEG"), ".png"),
        Some("Magic bytes do not match expected format JPEG".to_owned())
    );
    assert_eq!(hi::validate_magic_bytes(&png, None, ".PNG"), None);
    assert_eq!(
        hi::validate_magic_bytes(&png, Some(""), ".jpg").unwrap(),
        "Magic bytes do not match expected format JPEG"
    );
    assert_eq!(hi::validate_magic_bytes(&webp, Some("webp"), ""), None);
    assert_eq!(
        hi::validate_magic_bytes(&png, Some("jxl"), ""),
        None,
        "an unknown format is not failed closed"
    );
    assert_eq!(
        hi::validate_magic_bytes(&png, None, ".xyz"),
        None,
        "sniffed"
    );
    // `mp42` is not one of the brands the table lists.
    assert!(
        hi::validate_magic_bytes(&mp4, None, "")
            .unwrap()
            .ends_with("(magic bytes unrecognized)")
    );
    assert_eq!(
        hi::validate_magic_bytes(b"", Some("png"), ""),
        Some("Downloaded file is empty (0 bytes)".to_owned())
    );
    assert_eq!(
        hi::validate_magic_bytes(&png[..99], Some("png"), ""),
        Some("Downloaded file is implausibly small (99 bytes)".to_owned())
    );
}

#[test]
fn a_decode_check_passes_real_pictures_and_names_html_and_truncation() {
    let png = png_bytes(40, 40, noisy(3));
    assert_eq!(hi::validate_image_decodes(&png, Some("PNG")), None);
    assert_eq!(
        hi::validate_image_decodes(&png, None),
        None,
        "no format claimed: nothing to check"
    );
    assert_eq!(
        hi::validate_image_decodes(b"anything", Some("MP4")),
        None,
        "video is not decoded"
    );
    let html = hi::validate_image_decodes(b"<html>nope</html>", Some("png")).unwrap();
    assert!(
        html.starts_with("Downloaded data does not decode as a valid image: cannot identify image file <_io.BytesIO object at 0x"),
        "{html}"
    );
    let cut = hi::validate_image_decodes(&png[..png.len() / 2], Some("png")).unwrap();
    assert!(
        cut.starts_with("Downloaded data does not decode as a valid image: "),
        "{cut}"
    );
}

#[test]
fn an_exact_copy_is_a_pawchive_match_and_joined_drops_the_empty_ones() {
    let mut c = Candidate::new("https://e.example/1");
    assert!(!hi::is_exact_copy(&c));
    c.engine = "Pawchive".to_owned();
    assert!(hi::is_exact_copy(&c));
    assert_eq!(hi::joined(&[None, None]), None);
    assert_eq!(
        hi::joined(&[Some("a"), None, Some("b")]).as_deref(),
        Some("a; b")
    );
    assert_eq!(hi::joined(&[Some(""), Some("b")]).as_deref(), Some("b"));
}

#[test]
fn repointing_moves_the_selected_candidate_and_the_entry_together() {
    let dir = scratch("repoint");
    let mut e = entry_for(&dir, Some("https://danbooru.donmai.us/post/show/5"));
    let mut other = Candidate::new("https://other.example/9");
    other.engine = "IQDB".to_owned();
    e.candidates.push(other);
    hi::repoint(&mut e, "https://danbooru.donmai.us/posts/5");
    assert_eq!(e.matched_url(), Some("https://danbooru.donmai.us/posts/5"));
    assert_eq!(e.candidates[0].url, "https://danbooru.donmai.us/posts/5");
    assert_eq!(
        e.candidates[1].url, "https://other.example/9",
        "the others are not touched"
    );

    let mut bare = Entry::new("/x.png");
    hi::repoint(&mut bare, "https://e.example/1");
    assert_eq!(bare.matched_url(), Some("https://e.example/1"));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the verdict decision table (test_hydrus_duplicate_relationships.TestVerdictDecisionTable) ----

fn measured(
    distance: Option<u32>,
    mirrored: bool,
    fine: Option<u32>,
) -> hatate_core::image_compare::Comparison {
    hatate_core::image_compare::Comparison {
        distance,
        mirrored,
        fine_distance: fine,
    }
}

#[test]
fn the_verdict_table_says_nothing_without_a_distance_a_fine_hash_or_when_mirrored() {
    use hatate_core::image_compare::{DHASH_SAME_MAX, DHASH_SIMILAR_MAX, FINE_CONFIRM_MAX};
    assert_eq!(
        hi::verdict_of(&measured(None, false, Some(0))),
        None,
        "no distance at all"
    );
    assert_eq!(
        hi::verdict_of(&measured(Some(0), false, None)),
        None,
        "no fine distance"
    );
    assert_eq!(
        hi::verdict_of(&measured(
            Some(DHASH_SAME_MAX),
            false,
            Some(FINE_CONFIRM_MAX + 1)
        )),
        None,
        "64 bits say identical and 256 do not: evidence that disagrees with itself"
    );
    assert_eq!(
        hi::verdict_of(&measured(Some(0), true, Some(0))),
        None,
        "mirrored, however close"
    );
    // The boundaries are inclusive.
    assert_eq!(
        hi::verdict_of(&measured(
            Some(DHASH_SAME_MAX),
            false,
            Some(FINE_CONFIRM_MAX)
        )),
        Some(hi::Verdict::SameImage)
    );
    assert_eq!(
        hi::verdict_of(&measured(Some(DHASH_SIMILAR_MAX), false, Some(60))),
        Some(hi::Verdict::Alternate)
    );
    assert_eq!(
        hi::verdict_of(&measured(Some(DHASH_SIMILAR_MAX + 1), false, Some(60))),
        None
    );
    assert_eq!(hi::Verdict::SameImage.as_str(), "same");
    assert_eq!(hi::Verdict::Alternate.as_str(), "alternate");
}

#[test]
fn the_verdict_reads_real_files_and_says_nothing_for_unreadable_ones() {
    let dir = scratch("verdict-files");
    let local = dir.join("l.png");
    let pic = png_bytes(64, 64, noisy(1));
    std::fs::write(&local, &pic).unwrap();
    assert_eq!(
        hi::duplicate_verdict(&local, &pic),
        Some(hi::Verdict::SameImage)
    );
    // A bigger copy of the same picture is still the same picture.
    let bigger = png_bytes(128, 128, |x, y| noisy(1)(x / 2, y / 2));
    assert_eq!(
        hi::duplicate_verdict(&local, &bigger),
        Some(hi::Verdict::SameImage)
    );
    assert_eq!(
        hi::duplicate_verdict(&local, &png_bytes(64, 64, noisy(2))),
        None,
        "a different picture"
    );
    assert_eq!(
        hi::duplicate_verdict(&local, b"not a picture"),
        None,
        "unreadable bytes"
    );
    assert_eq!(
        hi::duplicate_verdict(&dir.join("missing.png"), &pic),
        None,
        "an unreadable local file"
    );
    let mirrored = png_bytes(64, 64, |x, y| noisy(1)(63 - x, y));
    assert_eq!(
        hi::duplicate_verdict(&local, &mirrored),
        None,
        "a mirrored copy says nothing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- which tags leave the app (test_sent_tag_filter.TestTheHelper) ----

#[test]
fn the_tag_filter_is_off_by_default_and_literal_when_on() {
    use hatate_core::model::{enabled_tag_sources, tags_to_send};
    let mut e = Entry::new("/x.png");
    e.tags = vec![
        tag(None, "u", TagSource::User),
        tag(None, "b", TagSource::Booru),
        tag(Some("ns"), "h", TagSource::Hydrus),
    ];
    let names = |s: Option<&Settings>| -> Vec<String> {
        tags_to_send(&e, s).iter().map(|t| t.display()).collect()
    };
    assert_eq!(names(None), ["u", "b", "ns:h"], "no settings: no filtering");
    let off = settings_with(|s| {
        s.filter_sent_tags_by_source = false;
        s.enabled_tag_sources = vec![];
    });
    assert_eq!(
        names(Some(&off)),
        ["u", "b", "ns:h"],
        "off sends everything even with nothing ticked"
    );
    let on = settings_with(|s| {
        s.filter_sent_tags_by_source = true;
        s.enabled_tag_sources = vec!["Booru".into(), "Stale name".into()];
    });
    assert_eq!(
        names(Some(&on)),
        ["b"],
        "a stale source name is ignored, not fatal"
    );
    assert_eq!(enabled_tag_sources(&on), [TagSource::Booru]);
    let none = settings_with(|s| {
        s.filter_sent_tags_by_source = true;
        s.enabled_tag_sources = vec![];
    });
    assert!(
        names(Some(&none)).is_empty(),
        "on with nothing ticked sends nothing"
    );
}

// ---- the real-picture verdicts of test_hydrus_duplicate_relationships.TestVerdict ----

/// `_pattern(size)`: a picture with enough structure to hash meaningfully.
fn pattern(size: u32) -> impl Fn(u32, u32) -> [u8; 3] {
    move |x, y| {
        let _ = size;
        [
            ((x * 3 + y) % 256) as u8,
            ((x * x + y * 7) % 256) as u8,
            ((x + y * 5) % 256) as u8,
        ]
    }
}

#[test]
fn an_edited_variant_is_an_alternate_and_its_measured_distances_are_the_ones_the_python_suite_states()
 {
    use hatate_core::image_compare::{Source, compare_to_local, local_prints};
    let dir = scratch("alternate");
    let local = dir.join("l.png");
    std::fs::write(&local, png_bytes(240, 240, pattern(240))).unwrap();
    // A third of the picture papered over.
    let edited = png_bytes(240, 240, |x, y| {
        if y >= 168 {
            [250, 250, 250]
        } else {
            pattern(240)(x, y)
        }
    });

    let measured = compare_to_local(
        local_prints(Source::Path(&local)).as_ref(),
        Source::Bytes(&edited),
    );
    assert_eq!(
        (measured.distance, measured.fine_distance),
        (Some(7), Some(21)),
        "64-bit 7, 256-bit 21"
    );
    assert_eq!(
        hi::duplicate_verdict(&local, &edited),
        Some(hi::Verdict::Alternate)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_alternate_is_filed_as_an_alternate_and_crowns_nobody() {
    let dir = scratch("relate-alt");
    let mut entry = entry_for(&dir, None);
    std::fs::write(&entry.path, png_bytes(240, 240, pattern(240))).unwrap();
    entry.hydrus_hash = Some(LOCAL.to_owned());
    let edited = png_bytes(240, 240, |x, y| {
        if y >= 168 {
            [250, 250, 250]
        } else {
            pattern(240)(x, y)
        }
    });
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let (server, client) = world(Fake::new()).await;

    let got =
        hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &edited, NEW)
            .await
            .unwrap();

    assert_eq!(got, None);
    assert_eq!(
        targets(&server),
        [
            "/get_files/file_metadata",
            "/verify_access_key",
            "/manage_file_relationships/set_file_relationships"
        ],
        "no set_kings for an alternate"
    );
    let rel: Value = serde_json::from_slice(&server.recorded()[2].body).unwrap();
    assert_eq!(rel["relationships"][0]["relationship"], 3);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failure_crowning_the_new_file_is_reported_not_raised() {
    let dir = scratch("relate-kings");
    let entry = entry_for(&dir, None);
    let pic = png_bytes(64, 64, noisy(1));
    std::fs::write(&entry.path, &pic).unwrap();
    let on = settings_with(|s| s.set_hydrus_duplicate_relationships = true);
    let mut fake = Fake::new();
    fake.fail = vec!["/manage_file_relationships/set_kings"];
    let (_server, client) = world(fake).await;

    let got = hi::record_duplicate_relationship(&entry, &client, Some(&on), Some(LOCAL), &pic, NEW)
        .await
        .unwrap();

    assert!(
        got.unwrap()
            .starts_with("Imported, but not marked as a duplicate: ")
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the note, end to end through the upload (test_hydrus_provenance_note.TestFeatureOn/Degrades) ----

#[tokio::test]
async fn an_upload_writes_the_note_last_and_reports_a_failed_note_beside_its_own_warnings() {
    let dir = scratch("upload-note");
    let on = settings_with(|s| s.write_hydrus_provenance_note = true);

    let (server, client) = world(Fake::new()).await;
    let mut entry = entry_for(&dir, Some("https://e.example/p/1"));
    let result = hi::send_file_upload(&mut entry, &client, Some(&on), &FixedClock)
        .await
        .unwrap();
    assert_eq!(result, hi::ImportResult::ok());
    assert_eq!(
        targets(&server),
        [
            "/add_files/add_file",
            "/add_urls/associate_url",
            "/get_files/file_metadata",
            "/verify_access_key",
            "/add_notes/set_notes"
        ]
    );

    let mut fake = Fake::new();
    fake.fail = vec!["/add_urls/associate_url", "/add_notes/set_notes"];
    let (_server, client) = world(fake).await;
    let mut entry = entry_for(&dir, Some("https://e.example/p/1"));
    let result = hi::send_file_upload(&mut entry, &client, Some(&on), &FixedClock)
        .await
        .unwrap();
    let warning = result.warning.unwrap();
    assert!(warning.starts_with("URL not added: "), "{warning}");
    assert!(
        warning.contains("; Imported, but no source note was written: "),
        "{warning}"
    );
    assert!(result.success && entry.sent_to_hydrus);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn no_hash_writes_no_note_and_the_note_settings_default_to_off_and_blank() {
    let (server, client) = world(Fake::new()).await;
    let dir = scratch("note-nohash");
    let entry = entry_for(&dir, Some("https://e.example/p/1"));
    let on = settings_with(|s| s.write_hydrus_provenance_note = true);
    assert_eq!(
        hi::write_provenance_note(&entry, &client, Some(&on), "", &FixedClock)
            .await
            .unwrap(),
        None
    );
    assert_eq!(server.request_count(), 0);

    let defaults = settings_with(|_| {});
    assert!(!defaults.write_hydrus_provenance_note && !defaults.set_hydrus_duplicate_relationships);
    assert_eq!(defaults.hydrus_provenance_note_name, "");
    let _ = std::fs::remove_dir_all(dir);
}
