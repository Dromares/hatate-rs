//! `core/hydrus_tag_lookup.py` (`apply_existing_hydrus_tags`) and the lookup worker's run body
//! (`workers/hydrus_lookup_worker.py`), against a scripted local Hydrus (Phase 4.a, DAN-2126).
//! The access key is a synthetic placeholder. The Python test each one maps to is in
//! `docs/hydrus-client.md` section 6.

mod common;

use common::{Recorded, Reply, Sandbox, TestServer};
use hatate_core::config::{OrderedMap, Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_tag_lookup::{apply_existing_hydrus_tags, run_tag_lookup};
use hatate_core::model::{Entry, TagSource};
use serde_json::{Value, json};

const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const H1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn json_reply(body: Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

fn settings(sb: &Sandbox, url: &str) -> Settings {
    let mut s = Settings::with_defaults(&sb.paths);
    s.hydrus.api_url = url.to_owned();
    s.hydrus.access_key = Secret::new(KEY);
    s.hydrus.timeout = 5.0;
    s
}

fn entry(dir: &std::path::Path, name: &str, size: usize, hash: Option<&str>) -> Entry {
    let path = dir.join(name);
    std::fs::write(&path, vec![b'x'; size]).unwrap();
    let mut e = Entry::new(path.to_str().unwrap());
    e.hydrus_hash = hash.map(str::to_owned);
    e
}

fn answer(rows: Value) -> impl Fn(&Recorded) -> Reply + Send + Sync + 'static {
    move |_| json_reply(json!({ "metadata": rows }))
}

fn names(e: &Entry) -> Vec<String> {
    e.tags.iter().map(|t| t.display()).collect()
}

#[tokio::test]
async fn existing_hydrus_tags_are_imported_for_a_matching_size() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::http(answer(json!([
        {"hash": H1, "size": 1000, "tags": {"s": {"display_tags": {"0": ["1girl", "artist:someone"]}}}}
    ])))
    .await;
    let mut entries = vec![entry(dir.path(), "a.jpg", 1000, Some(H1))];
    let result = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url(""))).await;
    let result = result.unwrap();
    assert_eq!((result.tagged_count, result.error), (1, None));
    assert_eq!(names(&entries[0]), vec!["1girl", "artist:someone"]);
    assert!(
        entries[0]
            .tags
            .iter()
            .all(|t| t.source == TagSource::Hydrus)
    );
    assert_eq!(server.request_count(), 1);
    assert_eq!(
        server.recorded()[0].target,
        format!("/get_files/file_metadata?hashes=%5B%22{H1}%22%5D")
    );
}

#[tokio::test]
async fn a_size_that_differs_blocks_the_import_and_a_missing_one_does_not() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    for (size, tagged) in [
        (json!(9999), 0),
        (json!(1000), 1),
        (json!(null), 1),
        (json!(0), 1),
    ] {
        let server = TestServer::http(answer(json!([
            {"hash": H1, "size": size, "tags": {"s": {"display_tags": {"0": ["1girl"]}}}}
        ])))
        .await;
        let mut entries = vec![entry(dir.path(), "a.jpg", 1000, Some(H1))];
        let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
            .await
            .unwrap();
        assert_eq!(r.tagged_count, tagged, "size {size}");
        assert_eq!(entries[0].tags.len(), tagged);
    }
}

#[tokio::test]
async fn a_remapped_namespace_reaches_the_imported_tag_only_when_the_remap_is_on() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let rows = json!([{"hash": H1, "tags": {"s": {"display_tags": {"0": ["artist:someone"]}}}}]);
    for (on, want) in [(true, "creator:someone"), (false, "artist:someone")] {
        let server = TestServer::http(answer(rows.clone())).await;
        let mut s = settings(&sb, &server.url(""));
        s.enable_tag_namespace_remap = on;
        s.tag_namespace_remap = OrderedMap(vec![("artist".to_owned(), "creator".to_owned())]);
        let mut entries = vec![entry(dir.path(), "a.jpg", 10, Some(H1))];
        let r = apply_existing_hydrus_tags(&mut entries, &s).await.unwrap();
        assert_eq!(r.tagged_count, 1);
        assert_eq!(names(&entries[0]), vec![want]);
    }
}

#[tokio::test]
async fn a_lookup_failure_is_distinguishable_from_zero_tags() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    // Hydrus unreachable: the error carries the failure's own text.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    drop(listener);
    let mut entries = vec![entry(dir.path(), "a.jpg", 10, Some(H1))];
    let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &url))
        .await
        .unwrap();
    assert_eq!(r.tagged_count, 0);
    let error = r.error.expect("a failed lookup carries an error");
    assert!(
        error.starts_with(&format!("Could not reach Hydrus at {url}: ")),
        "{error}"
    );
    // A rejected key.
    let server = TestServer::http(|_| Reply::new(401, "")).await;
    let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
        .await
        .unwrap();
    assert_eq!(
        r.error.as_deref(),
        Some("Hydrus rejected the access key (401) - check Settings > Hydrus")
    );
    // A genuine zero: Hydrus knows none of the hashes, and there is no error.
    let server = TestServer::http(answer(json!([]))).await;
    let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
        .await
        .unwrap();
    assert_eq!((r.tagged_count, r.error), (0, None));
}

#[tokio::test]
async fn no_access_key_means_no_hashing_and_no_request() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::http(answer(json!([]))).await;
    let mut s = settings(&sb, &server.url(""));
    s.hydrus.access_key = Secret::new("");
    let mut entries = vec![entry(dir.path(), "a.jpg", 10, None)];
    let r = apply_existing_hydrus_tags(&mut entries, &s).await.unwrap();
    assert_eq!((r.tagged_count, r.error), (0, None));
    assert_eq!(server.request_count(), 0);
    assert_eq!(entries[0].hydrus_hash, None, "nothing was hashed");
}

#[tokio::test]
async fn files_are_hashed_once_grouped_by_hash_and_sent_in_one_request() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::http(move |r: &Recorded| {
        // Echo a tag for every hash asked about.
        let hashes: Vec<String> = r
            .target
            .split("hashes=")
            .nth(1)
            .map(|q| {
                q.replace("%5B", "")
                    .replace("%5D", "")
                    .replace("%22", "")
                    .replace("%2C+", ",")
                    .split(',')
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let rows: Vec<Value> = hashes
            .iter()
            .map(|h| json!({"hash": h, "tags": {"s": {"display_tags": {"0": ["seen"]}}}}))
            .collect();
        json_reply(json!({"metadata": rows}))
    })
    .await;
    let mut entries = vec![
        entry(dir.path(), "one.jpg", 5, None),
        entry(dir.path(), "two.jpg", 5, None),
        entry(dir.path(), "three.jpg", 6, Some(H2)),
    ];
    let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
        .await
        .unwrap();
    assert_eq!(r.tagged_count, 3);
    // Two files with the same bytes share one hash and one slot in the request.
    let want = hatate_core::hydrus_tag_lookup::hash_file(std::path::Path::new(&entries[0].path));
    assert_eq!(entries[0].hydrus_hash.as_deref(), Some(want.as_str()));
    assert_eq!(entries[0].hydrus_hash, entries[1].hydrus_hash);
    assert_eq!(entries[2].hydrus_hash.as_deref(), Some(H2));
    assert_eq!(server.request_count(), 1);
    assert!(
        server.recorded()[0]
            .target
            .contains(&format!("%22{want}%22%2C+%22{H2}%22")),
        "{}",
        server.recorded()[0].target
    );
}

#[tokio::test]
async fn an_unreadable_file_with_no_hash_is_skipped_and_nothing_is_asked_when_all_are() {
    let sb = Sandbox::new();
    let server = TestServer::http(answer(json!([]))).await;
    let mut entries = vec![Entry::new("/no/such/file.jpg")];
    let r = apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
        .await
        .unwrap();
    assert_eq!((r.tagged_count, r.error), (0, None));
    assert_eq!(server.request_count(), 0);
    assert_eq!(entries[0].hydrus_hash, None);
}

#[tokio::test]
async fn hydrus_tags_replace_the_earlier_hydrus_tags_and_leave_the_others() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::http(answer(json!([
        {"hash": H1, "tags": {"s": {"display_tags": {"0": ["new"]}}}}
    ])))
    .await;
    let mut entries = vec![entry(dir.path(), "a.jpg", 10, Some(H1))];
    let mine = hatate_core::model::Tag {
        name: "mine".to_owned(),
        namespace: None,
        source: TagSource::User,
    };
    let old = hatate_core::model::Tag {
        name: "old".to_owned(),
        namespace: None,
        source: TagSource::Hydrus,
    };
    entries[0].tags = vec![mine, old];
    apply_existing_hydrus_tags(&mut entries, &settings(&sb, &server.url("")))
        .await
        .unwrap();
    assert_eq!(names(&entries[0]), vec!["mine", "new"]);
}

#[tokio::test]
async fn the_lookup_worker_turns_an_unreadable_answer_into_an_error_result() {
    let sb = Sandbox::new();
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::http(|_| json_reply(json!([1, 2]))).await;
    let mut entries = vec![entry(dir.path(), "a.jpg", 10, Some(H1))];
    let s = settings(&sb, &server.url(""));
    // `apply_existing_hydrus_tags` lets the shape error escape, as Python's raw exception does...
    let err = apply_existing_hydrus_tags(&mut entries, &s)
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedResponse);
    // ...and the worker's blanket `except Exception` makes it a result.
    let r = run_tag_lookup(&mut entries, &s).await;
    assert_eq!(r.tagged_count, 0);
    assert_eq!(
        r.error.as_deref(),
        Some("'list' object has no attribute 'get'")
    );
}

#[tokio::test]
async fn the_lookup_worker_still_answers_zero_for_an_empty_batch() {
    let sb = Sandbox::new();
    let server = TestServer::http(answer(json!([]))).await;
    let r = run_tag_lookup(&mut [], &settings(&sb, &server.url(""))).await;
    assert_eq!((r.tagged_count, r.error), (0, None));
    assert_eq!(server.request_count(), 0);
}
