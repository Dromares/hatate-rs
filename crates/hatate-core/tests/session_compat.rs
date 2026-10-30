//! Session store compatibility: `docs/file-formats.md` section 3 and rules 6.2.
//!
//! Test names start with `ruleN_` for the rule they prove. Every test runs in a temp profile
//! (`common::Sandbox`) and never resolves to the real one; the "is the Python app running"
//! check is pointed at a fake `/proc` except in the one test that spawns a real process.

mod common;

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use common::{Sandbox, corpus, mode};
use hatate_core::model::{Candidate, CandidateTag, Entry, MatchStatus, Tag, TagSource};
use hatate_core::session::{
    GuardConfig, LoadOptions, Notice, SaveError, SessionStore, SessionWriter, Source, named,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

/// A moment inside 30 days of the availability verdicts in the samples (1792639907).
const NOW: f64 = 1_792_700_000.0;

fn opts() -> LoadOptions {
    LoadOptions::at(NOW)
}

fn unclean() -> LoadOptions {
    LoadOptions {
        unclean_shutdown: true,
        ..opts()
    }
}

fn sample_db() -> PathBuf {
    corpus("fresh-profile/xdg-config/hatate-linux/session.db")
}

fn put(from: &Path, to: PathBuf) -> PathBuf {
    fs::create_dir_all(to.parent().unwrap()).unwrap();
    fs::copy(from, &to).unwrap();
    to
}

fn store(sb: &Sandbox) -> SessionStore {
    SessionStore::new(&sb.paths)
}

/// A writer whose "is the Python app running" probe looks at a `/proc` that does not exist.
fn writer(sb: &Sandbox) -> SessionWriter {
    store(sb)
        .open_writer_with(GuardConfig {
            proc_root: sb.dir.path().join("no-proc"),
            own_pid: 1,
        })
        .unwrap()
}

fn raw(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

/// `(path, ordinal, data)` for every row, `ORDER BY ordinal`.
fn rows(path: &Path) -> Vec<(String, i64, String)> {
    let conn = raw(path);
    let mut stmt = conn
        .prepare("SELECT path, ordinal, data FROM entries ORDER BY ordinal")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn meta(path: &Path, key: &str) -> Option<String> {
    raw(path)
        .query_row("SELECT value FROM meta WHERE key = ?", [key], |r| r.get(0))
        .ok()
}

fn schema_sql(path: &Path) -> String {
    let conn = raw(path);
    let mut stmt = conn
        .prepare("SELECT sql FROM sqlite_master WHERE type = 'table' ORDER BY rowid")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|s| format!("{};\n", s.unwrap()))
        .collect()
}

fn load_sample(sb: &Sandbox) -> Vec<Entry> {
    put(&sample_db(), sb.paths.session_db());
    let loaded = store(sb).load(&opts());
    assert!(loaded.report.skipped.is_empty());
    loaded.entries
}

/// A database created by hand: the real DDL, then whatever `setup` adds, then `rows`.
fn hostile_db(sb: &Sandbox, setup: &str, rows: &[(&str, i64, &str)]) -> PathBuf {
    let path = sb.paths.session_db();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE entries (
            path    TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            data    TEXT NOT NULL
        );
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .unwrap();
    conn.execute_batch(setup).unwrap();
    for (p, o, d) in rows {
        conn.execute(
            "INSERT INTO entries(path, ordinal, data) VALUES(?, ?, ?)",
            rusqlite::params![p, o, d],
        )
        .unwrap();
    }
    path
}

fn good_row(path: &str) -> String {
    json!({"path": path, "status": "good", "candidates": [{"url": "https://x.invalid/1"}]})
        .to_string()
}

fn entry(path: &str) -> Entry {
    Entry::new(path)
}

// ---- Rule 6.2.1: open session.db unchanged, in ordinal order, tolerate the unexpected ----

#[test]
fn rule1_the_sample_session_loads_in_ordinal_order_without_a_corrupt_row() {
    let sb = Sandbox::new();
    put(&sample_db(), sb.paths.session_db());
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.report.source, Source::Database);
    assert!(loaded.report.skipped.is_empty());
    assert_eq!(loaded.report.ignored, 0);
    assert!(
        loaded.report.notices.is_empty(),
        "{:?}",
        loaded.report.notices
    );
    let paths: Vec<_> = loaded.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/data/hatate-sample/images/example-1.png",
            "/data/hatate-sample/images/example-2.jpg",
            "/data/hatate-sample/images/ünïcode-3.webp",
            "/data/hatate-sample/images/example-4.gif",
            "/data/hatate-sample/images/example-5.png",
            "/data/hatate-sample/images/example-6.png",
        ]
    );
    assert_eq!(store(&sb).entry_count(), 6);
    let first = &loaded.entries[0];
    assert_eq!(first.status, MatchStatus::Good);
    assert_eq!(first.candidates.len(), 2);
    assert_eq!(first.candidates[0].remote_available, Some(true));
    assert!(first.sent_to_hydrus && first.hydrus_import_confirmed);
    assert_eq!(first.tags.len(), 7);
}

#[test]
fn rule1_order_follows_ordinal_not_insertion_or_path() {
    let sb = Sandbox::new();
    hostile_db(
        &sb,
        "",
        &[
            ("/c", 2, &good_row("/c")),
            ("/a", 7, &good_row("/a")),
            ("/b", 0, &good_row("/b")),
        ],
    );
    let paths: Vec<_> = store(&sb)
        .load(&opts())
        .entries
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(paths, ["/b", "/c", "/a"]);
}

#[test]
fn rule1_extra_columns_and_tables_are_tolerated_and_survive_a_save() {
    let sb = Sandbox::new();
    let path = hostile_db(
        &sb,
        "ALTER TABLE entries ADD COLUMN note TEXT;
         CREATE TABLE other_app (k TEXT, v TEXT);
         INSERT INTO other_app VALUES ('keep', 'me');",
        &[("/a", 0, &good_row("/a")), ("/b", 1, &good_row("/b"))],
    );
    raw_write(&path, "UPDATE entries SET note = 'hello' WHERE path = '/a'");
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 2);
    assert!(loaded.report.skipped.is_empty());

    writer(&sb).save(&loaded.entries, NOW).unwrap();

    let conn = raw(&path);
    let note: Option<String> = conn
        .query_row("SELECT note FROM entries WHERE path = '/a'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(note.as_deref(), Some("hello"), "an extra column is kept");
    let other: String = conn
        .query_row("SELECT v FROM other_app WHERE k = 'keep'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(other, "me", "an extra table is kept");
}

fn raw_write(path: &Path, sql: &str) {
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

#[test]
fn rule1_meta_and_user_version_are_ignored_and_left_alone() {
    // 3.8: schema_version 99, user_version 7, a row that is not JSON.
    let sb = Sandbox::new();
    let path = hostile_db(
        &sb,
        "INSERT INTO meta VALUES ('schema_version', '99'); PRAGMA user_version = 7;",
        &[
            ("/a/ok.png", 0, &good_row("/a/ok.png")),
            ("/a/b.png", 1, "this is not json"),
        ],
    );
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.report.skipped.len(), 1);

    writer(&sb).save(&loaded.entries, NOW).unwrap();
    let version: i64 = raw(&path)
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 7, "user_version is not touched");
    assert_eq!(meta(&path, "schema_version").as_deref(), Some("1"));
}

#[test]
fn rule1_a_database_without_meta_loads_and_load_does_not_create_it() {
    let sb = Sandbox::new();
    let path = hostile_db(&sb, "DROP TABLE meta;", &[("/a", 0, &good_row("/a"))]);
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 1);
    let has_meta: i64 = raw(&path)
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'meta'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_meta, 0, "reading never modifies the file");
    // A save creates it (the Python open does, 3.8).
    writer(&sb).save(&loaded.entries, NOW).unwrap();
    assert_eq!(meta(&path, "schema_version").as_deref(), Some("1"));
}

#[test]
fn rule1_load_leaves_no_wal_or_shm_behind() {
    let sb = Sandbox::new();
    put(&sample_db(), sb.paths.session_db());
    store(&sb).load(&opts());
    assert_eq!(sb.listing(), ["session.db"]);
}

// ---- Rule 6.2.2: the DDL, WAL, no new NOT NULL column ----

#[test]
fn rule2_a_rust_written_file_has_exactly_the_documented_schema() {
    let sb = Sandbox::new();
    writer(&sb).save(&[entry("/a"), entry("/b")], NOW).unwrap();
    let want = fs::read_to_string(corpus("schemas/session.db.schema.txt")).unwrap();
    assert_eq!(schema_sql(&sb.paths.session_db()), want);
    // No indexes beyond the two implicit ones, no views, no triggers.
    let extra: i64 = raw(&sb.paths.session_db())
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('view', 'trigger')
                OR (type = 'index' AND name NOT LIKE 'sqlite_autoindex_%')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(extra, 0);
}

#[test]
fn rule2_the_file_is_wal_utf8_4096_like_the_sample() {
    let sb = Sandbox::new();
    writer(&sb).save(&[entry("/a")], NOW).unwrap();
    let ours = fs::read(sb.paths.session_db()).unwrap();
    let theirs = fs::read(sample_db()).unwrap();
    // File-format write and read version (WAL = 2), page size, text encoding.
    for range in [18..20, 16..18, 56..60] {
        assert_eq!(
            ours[range.clone()],
            theirs[range.clone()],
            "header bytes {range:?}"
        );
    }
    assert_eq!(&ours[18..20], &[2, 2]);
    let mode: String = raw(&sb.paths.session_db())
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn rule2_after_a_save_no_wal_or_shm_is_left_and_the_file_is_private() {
    let sb = Sandbox::new();
    writer(&sb).save(&[entry("/a")], NOW).unwrap();
    assert_eq!(sb.listing(), ["session.db"]);
    assert_eq!(mode(&sb.paths.session_db()), 0o600);
    assert_eq!(mode(sb.config_dir()), 0o700);
}

#[test]
fn rule2_a_save_does_not_add_a_column_or_drop_one() {
    let sb = Sandbox::new();
    put(&sample_db(), sb.paths.session_db());
    let before = schema_sql(&sb.paths.session_db());
    let loaded = store(&sb).load(&opts());
    writer(&sb).save(&loaded.entries, NOW).unwrap();
    assert_eq!(schema_sql(&sb.paths.session_db()), before);
}

// ---- Rule 6.2.3: data is the 16-key object with the invariants Python's loader needs ----

const ENTRY_KEYS: [&str; 16] = [
    "path",
    "hydrus_hash",
    "status",
    "error_message",
    "result_source",
    "searched_without_saucenao",
    "sent_to_hydrus",
    "hydrus_import_confirmed",
    "reviewed",
    "upscale_verdict",
    "upscale_check_detail",
    "local_width",
    "local_height",
    "selected_candidate_index",
    "candidates",
    "tags",
];

const CANDIDATE_KEYS: [&str; 20] = [
    "url",
    "source_name",
    "thumb_url",
    "similarity",
    "similarity_measured",
    "width",
    "height",
    "engine",
    "engine_tags",
    "booru_tags",
    "direct_file_url",
    "preview_url",
    "remote_format",
    "remote_size_bytes",
    "incomplete_reason",
    "tags_borrowed_from",
    "remote_available",
    "availability_checked_at",
    "restricted",
    "tag_version",
];

/// What 6.2.3 lists as the things that keep Python's loader from raising.
fn assert_loader_safe(data: &str) {
    let v: Value = serde_json::from_str(data).expect("data is JSON");
    let o = v.as_object().expect("data is an object");
    assert_eq!(o.keys().map(String::as_str).collect::<Vec<_>>(), ENTRY_KEYS);
    assert!(o["path"].as_str().is_some_and(|p| !p.is_empty()));
    assert!(
        [
            "not_searched",
            "good",
            "poor",
            "not_found",
            "error",
            "searching"
        ]
        .contains(&o["status"].as_str().unwrap())
    );
    assert!(o["selected_candidate_index"].is_u64());
    for c in o["candidates"].as_array().unwrap() {
        let c = c.as_object().unwrap();
        assert_eq!(
            c.keys().map(String::as_str).collect::<Vec<_>>(),
            CANDIDATE_KEYS
        );
        assert!(c["url"].is_string());
        match (&c["remote_available"], &c["availability_checked_at"]) {
            (Value::Bool(_), Value::Number(_)) | (Value::Null, Value::Null) => {}
            other => panic!("remote_available/availability_checked_at: {other:?}"),
        }
        for list in ["engine_tags", "booru_tags"] {
            for t in c[list].as_array().unwrap() {
                assert!(t["name"].is_string());
                assert_eq!(
                    t.as_object().unwrap().len(),
                    2,
                    "no source inside a candidate"
                );
            }
        }
    }
    for t in o["tags"].as_array().unwrap() {
        assert!(t["name"].is_string());
        assert!(TagSource::parse(t["source"].as_str().unwrap()).is_some());
        assert_eq!(t.as_object().unwrap().len(), 3);
    }
}

#[test]
fn rule3_every_sample_row_is_reproduced_byte_for_byte() {
    // Load, then encode: the Rust encoder is `json.dumps` for these rows, so a Rust-written
    // file is indistinguishable from the Python-written one (spec 3.4).
    let sb = Sandbox::new();
    let entries = load_sample(&sb);
    let original = rows(&sb.paths.session_db());
    let target = Sandbox::new();
    writer(&target).save(&entries, NOW).unwrap();
    let ours = rows(&target.paths.session_db());
    assert_eq!(ours.len(), original.len());
    for (a, b) in original.iter().zip(&ours) {
        assert_eq!(a.0, b.0, "path");
        assert_eq!(a.1, b.1, "ordinal");
        assert_eq!(a.2, b.2, "data of {}", a.0);
        assert_loader_safe(&b.2);
    }
}

#[test]
fn rule3_hostile_content_is_written_in_the_safe_shape() {
    let sb = Sandbox::new();
    let mut e = entry("/ünï/\u{7f}/😀 \"quoted\" \\ back\nslash.png");
    e.status = MatchStatus::Error;
    e.error_message = Some("boom \u{1} \u{2028} ü".to_owned());
    let mut c = Candidate::new("https://example.invalid/chapter");
    c.similarity = f64::NAN;
    c.remote_available = Some(false);
    c.availability_checked_at = Some(NOW);
    c.engine_tags = vec![CandidateTag {
        name: "t".to_owned(),
        namespace: Some("ns".to_owned()),
    }];
    e.candidates = vec![c];
    e.tags = vec![Tag {
        name: "x".to_owned(),
        namespace: None,
        source: TagSource::Hydrus,
    }];
    writer(&sb)
        .save(&[e.clone(), entry("/second")], NOW)
        .unwrap();
    for (_, _, data) in rows(&sb.paths.session_db()) {
        assert_loader_safe(&data);
        assert!(
            data.is_ascii(),
            "json.dumps escapes everything outside ASCII"
        );
    }
    // And it comes back (NaN, which JSON cannot hold, comes back as 0.0).
    let back = store(&sb).load(&opts()).entries;
    e.candidates[0].similarity = 0.0;
    assert_eq!(back[0], e);
}

#[test]
fn rule3_the_mangadex_tag_version_is_written() {
    let sb = Sandbox::new();
    let mut e = entry("/a");
    e.candidates = vec![
        Candidate::new("https://mangadex.org/chapter/0123abcd-4567-89ab-cdef-0123456789ab"),
        Candidate::new("https://danbooru.donmai.us/posts/1"),
    ];
    writer(&sb).save(&[e], NOW).unwrap();
    let data: Value = serde_json::from_str(&rows(&sb.paths.session_db())[0].2).unwrap();
    assert_eq!(data["candidates"][0]["tag_version"], 1);
    assert_eq!(data["candidates"][1]["tag_version"], 0);
}

// ---- Rule 6.2.4: meta rows on every save ----

#[test]
fn rule4_every_save_rewrites_both_meta_rows() {
    let sb = Sandbox::new();
    let w = writer(&sb);
    let db = sb.paths.session_db();
    w.save(&[entry("/a")], 1_792_639_849.160799).unwrap();
    assert_eq!(meta(&db, "schema_version").as_deref(), Some("1"));
    assert_eq!(meta(&db, "saved_at").as_deref(), Some("1792639849.160799"));
    w.save(&[entry("/a")], 1_792_639_900.0).unwrap();
    assert_eq!(
        meta(&db, "saved_at").as_deref(),
        Some("1792639900.0"),
        "str(float)"
    );
    // A foreign value is put back.
    raw_write(
        &db,
        "UPDATE meta SET value = '99' WHERE key = 'schema_version'",
    );
    w.save(&[entry("/a")], 1_792_639_901.5).unwrap();
    assert_eq!(meta(&db, "schema_version").as_deref(), Some("1"));
    let n: i64 = raw(&db)
        .query_row("SELECT COUNT(*) FROM meta", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

// ---- Rule 6.2.5: restore order ----

#[test]
fn rule5_saved_tags_overwrite_what_selecting_the_candidate_derived() {
    let sb = Sandbox::new();
    let entries = load_sample(&sb);
    // example-1: engine and booru tags on its candidates, and a saved list that adds User,
    // Hydrus and Hatate tags. The saved list wins whole.
    let first = &entries[0];
    let sources: Vec<_> = first.tags.iter().map(|t| t.source).collect();
    for s in TagSource::ALL {
        assert!(sources.contains(&s), "{s:?} survives the restore");
    }
    assert_eq!(first.tags.len(), 7);
}

#[test]
fn rule5_without_saved_tags_the_selected_candidates_own_tags_are_derived() {
    let sb = Sandbox::new();
    let row = json!({
        "path": "/a", "status": "good", "selected_candidate_index": 1,
        "candidates": [
            {"url": "u0", "engine_tags": [{"name": "e0", "namespace": null}], "booru_tags": []},
            {"url": "u1", "engine_tags": [{"name": "e1", "namespace": null}],
             "booru_tags": [{"name": "b1", "namespace": "artist"}]},
        ],
        "tags": []
    })
    .to_string();
    hostile_db(&sb, "", &[("/a", 0, &row)]);
    let e = store(&sb).load(&opts()).entries.remove(0);
    assert_eq!(e.selected_candidate_index, 1);
    let tags: Vec<_> = e.tags.iter().map(|t| (t.display(), t.source)).collect();
    assert_eq!(
        tags,
        [
            ("e1".to_owned(), TagSource::SearchEngine),
            ("artist:b1".to_owned(), TagSource::Booru)
        ]
    );
}

#[test]
fn rule5_booru_tags_written_by_the_saved_list_are_not_lost() {
    // The bug 3.4 warns about: apply the saved tags first, select second, and the booru
    // tags vanish. Here the saved list holds a booru tag the candidate does not have.
    let sb = Sandbox::new();
    let row = json!({
        "path": "/a", "candidates": [{"url": "u0", "booru_tags": []}],
        "tags": [{"name": "hand_picked", "namespace": null, "source": "Booru"}]
    })
    .to_string();
    hostile_db(&sb, "", &[("/a", 0, &row)]);
    let e = store(&sb).load(&opts()).entries.remove(0);
    assert_eq!(e.tags.len(), 1);
    assert_eq!(e.tags[0].source, TagSource::Booru);
}

// ---- Rule 6.2.6: sticky states ----

#[test]
fn rule6_searching_survives_a_clean_restart() {
    let sb = Sandbox::new();
    let entries = load_sample(&sb);
    assert_eq!(entries[4].status, MatchStatus::Searching);
    assert!(!entries[4].interrupted_mid_search);
}

#[test]
fn rule6_searching_is_reset_after_an_unclean_shutdown() {
    let sb = Sandbox::new();
    put(&sample_db(), sb.paths.session_db());
    let loaded = store(&sb).load(&unclean());
    assert_eq!(loaded.report.reset_mid_search, 1);
    assert_eq!(loaded.entries[4].status, MatchStatus::NotSearched);
    assert!(loaded.entries[4].interrupted_mid_search);
    // Every other row is untouched, and the flag is not stored.
    assert_eq!(loaded.entries[0].status, MatchStatus::Good);
    writer(&sb).save(&loaded.entries, NOW).unwrap();
    let data: Value = serde_json::from_str(&rows(&sb.paths.session_db())[4].2).unwrap();
    assert_eq!(data["status"], "not_searched");
    assert!(data.get("interrupted_mid_search").is_none());
}

#[test]
fn rule6_unknown_status_and_unknown_tag_source_fall_back() {
    let sb = Sandbox::new();
    let row = json!({
        "path": "/a", "status": "banana",
        "tags": [{"name": "x", "namespace": null, "source": "Nowhere"}, {"name": "y"}]
    })
    .to_string();
    hostile_db(&sb, "", &[("/a", 0, &row)]);
    let loaded = store(&sb).load(&opts());
    assert!(loaded.report.skipped.is_empty());
    let e = &loaded.entries[0];
    assert_eq!(e.status, MatchStatus::NotSearched);
    assert!(e.tags.iter().all(|t| t.source == TagSource::User));
}

// ---- Rule 6.2.7: contiguous ordinals, one transaction ----

#[test]
fn rule7_a_full_save_writes_ordinals_zero_to_n_minus_one_and_drops_removed_rows() {
    let sb = Sandbox::new();
    let w = writer(&sb);
    let mut list: Vec<Entry> = (0..50)
        .map(|i| entry(&format!("/img/{i:03}.png")))
        .collect();
    w.save(&list, NOW).unwrap();
    // Remove some, reverse the rest, add one: the file must follow the list exactly.
    list.retain(|e| !e.path.ends_with('3'));
    list.reverse();
    list.push(entry("/img/new.png"));
    w.save(&list, NOW).unwrap();
    let got = rows(&sb.paths.session_db());
    assert_eq!(got.len(), list.len());
    for (i, ((path, ordinal, _), e)) in got.iter().zip(&list).enumerate() {
        assert_eq!(*ordinal, i as i64);
        assert_eq!(path, &e.path);
    }
    let again = store(&sb).load(&opts()).entries;
    assert_eq!(again, list);
}

#[test]
fn rule7_a_row_removed_from_the_list_does_not_come_back() {
    // The bug in `core/session_db.py:101-114`'s history: deletions decided from the caller's
    // revision map instead of the file.
    let sb = Sandbox::new();
    let w = writer(&sb);
    w.save(&[entry("/a"), entry("/b"), entry("/c")], NOW)
        .unwrap();
    w.save(&[entry("/a"), entry("/c")], NOW).unwrap();
    let paths: Vec<_> = store(&sb)
        .load(&opts())
        .entries
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(paths, ["/a", "/c"]);
}

#[test]
fn rule7_a_save_is_one_transaction() {
    // An extra NOT NULL column makes every row insert fail (3.8). The save has by then
    // rewritten `meta`; all of it must be rolled back, rows and meta alike.
    let sb = Sandbox::new();
    let path = hostile_db(
        &sb,
        "ALTER TABLE entries ADD COLUMN extra TEXT NOT NULL DEFAULT 'x';
         INSERT INTO meta VALUES ('saved_at', 'before');",
        &[("/keep", 0, &good_row("/keep"))],
    );
    // Now make the column mandatory without a default, like a foreign writer would have it.
    raw_write(
        &path,
        "PRAGMA writable_schema = ON;
         UPDATE sqlite_master SET sql = replace(sql, 'extra TEXT NOT NULL DEFAULT ''x''', 'extra TEXT NOT NULL')
           WHERE name = 'entries';
         PRAGMA writable_schema = OFF;",
    );
    let before = rows(&path);
    let err = writer(&sb).save(&[entry("/new")], NOW).unwrap_err();
    assert!(
        matches!(&err, SaveError::Database(m) if m.contains("NOT NULL")),
        "{err}"
    );
    assert_eq!(rows(&path), before, "the row that was there is still there");
    assert_eq!(meta(&path, "saved_at").as_deref(), Some("before"));
}

// ---- Rule 6.2.8: legacy session.json ----

fn legacy_input() -> PathBuf {
    corpus("legacy-migration/input/session.json")
}

#[test]
fn rule8_session_json_alone_is_read_migrated_and_kept() {
    let sb = Sandbox::new();
    put(&legacy_input(), sb.paths.session_json());
    let loaded = writer(&sb).restore(&opts());
    assert_eq!(loaded.report.source, Source::LegacyJson);
    assert_eq!(loaded.entries.len(), 6);
    assert!(
        loaded
            .report
            .notices
            .contains(&Notice::LegacyMigrated { entries: 6 })
    );
    assert!(sb.paths.session_json().exists(), "kept as a backup");
    assert_eq!(rows(&sb.paths.session_db()).len(), 6);
    // The second start reads the database.
    let again = writer(&sb).restore(&opts());
    assert_eq!(again.report.source, Source::Database);
    assert_eq!(again.entries, loaded.entries);
}

#[test]
fn rule8_the_migration_matches_what_the_python_app_wrote_from_the_same_json() {
    let sb = Sandbox::new();
    put(&legacy_input(), sb.paths.session_json());
    let ours = writer(&sb).restore(&opts()).entries;
    let python = Sandbox::new();
    put(
        &corpus("legacy-migration/after-load/session.db"),
        python.paths.session_db(),
    );
    let theirs = store(&python).load(&opts()).entries;
    assert_eq!(ours, theirs);
    let a = rows(&sb.paths.session_db());
    let b = rows(&python.paths.session_db());
    assert_eq!(a, b, "same rows, same ordinals, same data");
}

#[test]
fn rule8_session_db_wins_even_when_it_is_empty() {
    let sb = Sandbox::new();
    hostile_db(&sb, "", &[]);
    put(&legacy_input(), sb.paths.session_json());
    let loaded = writer(&sb).restore(&opts());
    assert_eq!(loaded.report.source, Source::Database);
    assert!(loaded.entries.is_empty());
    assert!(sb.paths.session_json().exists());
}

fn set_age(path: &Path, now: f64, days: f64) {
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs_f64(now - days * 86400.0);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// A profile with a database holding one entry and a `session.json`, then `restore`.
fn retirement(json_days_old: f64, db_days_old: f64) -> (Sandbox, bool) {
    let sb = Sandbox::new();
    let w = writer(&sb);
    w.save(&[entry("/a")], NOW).unwrap();
    put(&legacy_input(), sb.paths.session_json());
    // `now` for the age maths is the real clock here: the files' mtimes are set against it.
    let now = hatate_core::session::now_epoch();
    set_age(&sb.paths.session_json(), now, json_days_old);
    set_age(&sb.paths.session_db(), now, db_days_old);
    let loaded = w.restore(&LoadOptions::at(now));
    assert_eq!(loaded.entries.len(), 1);
    let retired = loaded
        .report
        .notices
        .iter()
        .any(|n| matches!(n, Notice::LegacyRetired { .. }));
    assert_eq!(retired, !sb.paths.session_json().exists());
    (sb, retired)
}

#[test]
fn rule8_a_backup_older_than_the_database_and_30_days_is_retired() {
    let (_sb, retired) = retirement(31.0, 1.0);
    assert!(retired);
}

#[test]
fn rule8_a_backup_under_30_days_old_is_kept() {
    assert!(!retirement(29.0, 0.5).1);
}

#[test]
fn rule8_a_backup_newer_than_the_database_is_kept_however_old() {
    assert!(!retirement(40.0, 50.0).1);
}

#[test]
fn rule8_an_empty_session_db_never_retires_the_backup() {
    let sb = Sandbox::new();
    hostile_db(&sb, "", &[]);
    put(&legacy_input(), sb.paths.session_json());
    let now = hatate_core::session::now_epoch();
    set_age(&sb.paths.session_json(), now, 90.0);
    writer(&sb).restore(&LoadOptions::at(now));
    assert!(sb.paths.session_json().exists());
}

#[test]
fn rule8_a_plain_load_never_migrates_or_retires() {
    let sb = Sandbox::new();
    put(&legacy_input(), sb.paths.session_json());
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 6);
    assert_eq!(
        sb.listing(),
        ["session.json"],
        "no session.db was made by reading"
    );
}

#[test]
fn rule8_clear_empties_the_database_and_removes_the_legacy_file() {
    let sb = Sandbox::new();
    let w = writer(&sb);
    w.save(&[entry("/a"), entry("/b")], NOW).unwrap();
    put(&legacy_input(), sb.paths.session_json());
    assert!(w.clear().unwrap());
    assert!(sb.paths.session_db().exists(), "the file stays");
    assert!(!sb.paths.session_json().exists());
    assert_eq!(store(&sb).entry_count(), 0);
    assert!(!w.clear().unwrap(), "nothing left to clear");
}

/// DAN-2223: `tests/test_nothing_survived.py::TestSavedSessionAge::test_age_is_read_from_the_store_mtime`,
/// with the same numbers: a store last written three days ago is three days old (within a minute),
/// and when both stores exist the newer one decides.
#[test]
fn saved_session_age_is_the_stores_mtime() {
    let sb = Sandbox::new();
    let now = SystemTime::now();
    let db = sb.paths.session_db();
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let stamp = |p: &Path, ago: u64| {
        let f = File::options().write(true).create(true).open(p).unwrap();
        f.set_modified(now - Duration::from_secs(ago)).unwrap();
    };
    stamp(&db, 3 * 86400);
    let now_s = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let age = store(&sb).saved_session_age_seconds(now_s).unwrap();
    assert!((age - 3.0 * 86400.0).abs() < 60.0, "{age}");
    // The older `session.json` does not drag the age back.
    stamp(&sb.paths.session_json(), 10 * 86400);
    let age = store(&sb).saved_session_age_seconds(now_s).unwrap();
    assert!((age - 3.0 * 86400.0).abs() < 60.0, "{age}");
}

#[test]
fn rule8_nothing_saved_is_an_empty_list_and_says_so() {
    let sb = Sandbox::new();
    let loaded = store(&sb).load(&opts());
    assert!(loaded.entries.is_empty());
    assert_eq!(loaded.report.notices, [Notice::NothingSaved]);
    assert!(!store(&sb).has_saved_session());
    assert_eq!(store(&sb).saved_session_age_seconds(NOW), None);
    writer(&sb).save(&[entry("/a")], NOW).unwrap();
    assert!(store(&sb).has_saved_session());
    assert!(store(&sb).saved_session_age_seconds(NOW + 1e9).unwrap() > 1e8);
}

// ---- Rule 6.2.9: named sessions ----

fn named_sample() -> PathBuf {
    corpus("named-session/named-session.json")
}

#[test]
fn rule9_the_sample_named_session_loads() {
    let loaded = named::load(&named_sample(), &opts());
    assert_eq!(loaded.report.source, Source::Named);
    assert_eq!(loaded.entries.len(), 6);
    assert!(loaded.report.skipped.is_empty());
    assert_eq!(named::entry_count(&named_sample()), Some(6));
    // The same six entries as the automatic session sample.
    let sb = Sandbox::new();
    assert_eq!(loaded.entries, load_sample(&sb));
}

#[test]
fn rule9_a_rust_written_named_session_is_the_pythons_bytes() {
    let sb = Sandbox::new();
    let entries = named::load(&named_sample(), &opts()).entries;
    let target = sb.dir.path().join("out/named.json");
    named::save(&target, &entries, 1792639907.5574033).unwrap();
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(named_sample()).unwrap()
    );
    assert_eq!(mode(&target), 0o600);
    let names: Vec<_> = fs::read_dir(target.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["named.json"], "no temp file is left behind");
}

#[test]
fn rule9_save_load_save_is_lossless_and_replaces_the_target() {
    let sb = Sandbox::new();
    let target = sb.dir.path().join("s.json");
    fs::write(&target, "old contents").unwrap();
    let mut list = vec![entry("/a"), entry("/ü")];
    list[0].reviewed = true;
    named::save(&target, &list, NOW).unwrap();
    let back = named::load(&target, &opts()).entries;
    assert_eq!(back, list);
    named::save(&target, &back, NOW).unwrap();
    assert_eq!(named::load(&target, &opts()).entries, list);
}

#[test]
fn rule9_only_version_one_is_read() {
    let sb = Sandbox::new();
    let write = |name: &str, body: &str| {
        let p = sb.dir.path().join(name);
        fs::write(&p, body).unwrap();
        p
    };
    let doc = |version: &str| {
        format!(
            r#"{{"version": {version}, "entries": [{}]}}"#,
            json!({"path": "/a"})
        )
    };
    for bad in ["2", "0", "\"1\"", "null", "[1]", "1.5"] {
        let loaded = named::load(&write("v.json", &doc(bad)), &opts());
        assert!(loaded.entries.is_empty(), "version {bad}");
        assert_eq!(
            loaded.report.notices,
            [Notice::UnsupportedVersion],
            "version {bad}"
        );
        assert_eq!(named::entry_count(&sb.dir.path().join("v.json")), None);
    }
    // Python compares with `!=`: 1.0 and true equal 1 and are accepted.
    for good in ["1", "1.0", "true"] {
        let loaded = named::load(&write("v.json", &doc(good)), &opts());
        assert_eq!(loaded.entries.len(), 1, "version {good}");
    }
    let no_version = write("n.json", r#"{"entries": []}"#);
    assert!(named::load(&no_version, &opts()).entries.is_empty());
}

#[test]
fn rule9_unreadable_and_foreign_files_give_an_empty_list_not_a_panic() {
    let sb = Sandbox::new();
    let write = |name: &str, body: &[u8]| {
        let p = sb.dir.path().join(name);
        fs::write(&p, body).unwrap();
        p
    };
    let cases: [(&str, &[u8]); 6] = [
        ("empty", b""),
        ("array", b"[1, 2]"),
        ("scalar", b"7"),
        ("truncated", br#"{"version": 1, "entries": [{"path": "/a""#),
        ("bom", b"\xef\xbb\xbf{\"version\": 1, \"entries\": []}"),
        ("binary", b"\xff\xfe\x00\x01"),
    ];
    for (name, body) in cases {
        let loaded = named::load(&write(name, body), &opts());
        assert!(loaded.entries.is_empty(), "{name}");
        assert!(!loaded.report.notices.is_empty(), "{name}");
    }
    let missing = named::load(&sb.dir.path().join("missing.json"), &opts());
    assert!(missing.entries.is_empty());
    assert_eq!(
        named::entry_count(&sb.dir.path().join("missing.json")),
        None
    );
}

#[test]
fn rule9_a_malformed_element_is_skipped_and_the_others_load() {
    let sb = Sandbox::new();
    let p = sb.dir.path().join("m.json");
    let body = json!({"version": 1, "saved_at": 1.0, "entries": [
        {"path": "/ok1"}, "not an object", {"path": "/bad", "candidates": [{"no": "url"}]},
        {"status": "good"}, {"path": "/ok2"}, 5]});
    fs::write(&p, body.to_string()).unwrap();
    let loaded = named::load(&p, &opts());
    let paths: Vec<_> = loaded.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["/ok1", "/ok2"]);
    assert_eq!(loaded.report.skipped.len(), 3);
    assert_eq!(loaded.report.ignored, 1);
}

#[test]
fn rule9_a_failed_save_leaves_the_target_and_no_temp_file() {
    if is_root() {
        return; // permissions do not stop root
    }
    let sb = Sandbox::new();
    let dir = sb.dir.path().join("ro");
    fs::create_dir(&dir).unwrap();
    let target = dir.join("s.json");
    fs::write(&target, "precious").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let err = named::save(&target, &[entry("/a")], NOW);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(err, Err(SaveError::Io(_))));
    assert_eq!(fs::read_to_string(&target).unwrap(), "precious");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

fn is_root() -> bool {
    fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
        .unwrap_or(false)
}

// ---- Rule 6.2.10: single writer ----

/// A fake `/proc` where `pid` has `target` open as fd 3.
fn fake_proc(root: &Path, pid: u32, target: &Path) {
    let fd = root.join(pid.to_string()).join("fd");
    fs::create_dir_all(&fd).unwrap();
    std::os::unix::fs::symlink(target, fd.join("3")).unwrap();
    // Noise a real /proc has too.
    fs::create_dir_all(root.join("self")).unwrap();
    fs::write(root.join("cpuinfo"), "").unwrap();
}

fn crash_log(sb: &Sandbox) -> PathBuf {
    fs::create_dir_all(sb.config_dir()).unwrap();
    let p = sb.paths.crash_log();
    fs::write(&p, "--- session started, faulthandler armed ---\n").unwrap();
    fs::canonicalize(p).unwrap()
}

#[test]
fn rule10_a_process_holding_crash_log_open_refuses_the_writer_and_says_why() {
    let sb = Sandbox::new();
    let log = crash_log(&sb);
    let proc_root = sb.dir.path().join("proc");
    fake_proc(&proc_root, 4242, &log);
    let err = store(&sb)
        .open_writer_with(GuardConfig {
            proc_root,
            own_pid: 1,
        })
        .unwrap_err();
    assert_eq!(err.pid, 4242);
    let msg = err.to_string();
    assert!(
        msg.contains("Python app is running") && msg.contains("4242"),
        "{msg}"
    );
    assert!(msg.contains("overwrites"), "it says why: {msg}");
    assert!(!sb.paths.session_db().exists(), "nothing was written");
}

#[test]
fn rule10_our_own_process_does_not_count() {
    let sb = Sandbox::new();
    let log = crash_log(&sb);
    let proc_root = sb.dir.path().join("proc");
    fake_proc(&proc_root, 77, &log);
    let w = store(&sb).open_writer_with(GuardConfig {
        proc_root,
        own_pid: 77,
    });
    assert!(w.is_ok());
}

#[test]
fn rule10_a_stale_running_marker_without_a_process_is_not_a_second_writer() {
    // An unclean shutdown leaves crash.log.running behind (5.3); that is the banner's
    // business, not a reason to refuse.
    let sb = Sandbox::new();
    let log = crash_log(&sb);
    fs::write(sb.paths.crash_running_marker(), "1792639907.5").unwrap();
    let proc_root = sb.dir.path().join("proc");
    fake_proc(&proc_root, 9, &sb.dir.path().join("something-else"));
    let _ = log;
    assert!(
        store(&sb)
            .open_writer_with(GuardConfig {
                proc_root,
                own_pid: 1
            })
            .is_ok()
    );
}

#[test]
fn rule10_a_python_app_that_starts_later_stops_the_next_save() {
    let sb = Sandbox::new();
    let log = crash_log(&sb);
    let proc_root = sb.dir.path().join("proc");
    fs::create_dir_all(&proc_root).unwrap();
    let w = store(&sb)
        .open_writer_with(GuardConfig {
            proc_root: proc_root.clone(),
            own_pid: 1,
        })
        .unwrap();
    w.save(&[entry("/a")], NOW).unwrap();
    fake_proc(&proc_root, 31337, &log);
    let err = w.save(&[entry("/a"), entry("/b")], NOW).unwrap_err();
    assert!(
        matches!(err, SaveError::Refused(ref p) if p.pid == 31337),
        "{err}"
    );
    assert!(w.clear().is_err());
    assert_eq!(
        rows(&sb.paths.session_db()).len(),
        1,
        "the file was not touched"
    );
}

#[test]
fn rule10_a_real_process_holding_crash_log_is_found_through_proc() {
    let sb = Sandbox::new();
    let log = crash_log(&sb);
    let mut child = Command::new("sleep")
        .arg("60")
        .stdin(Stdio::from(File::open(&log).unwrap()))
        .spawn()
        .expect("sleep");
    let found = store(&sb).open_writer();
    let child_pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(found.unwrap_err().pid, child_pid);
    // Gone again: allowed.
    assert!(store(&sb).open_writer().is_ok());
}

#[test]
fn rule10_reading_is_never_refused() {
    let sb = Sandbox::new();
    put(&sample_db(), sb.paths.session_db());
    let log = crash_log(&sb);
    let proc_root = sb.dir.path().join("proc");
    fake_proc(&proc_root, 4242, &log);
    assert_eq!(store(&sb).load(&opts()).entries.len(), 6);
}

// ---- Hostile rows (6.5) ----

#[test]
fn hostile_the_documented_probe_rows() {
    let sb = Sandbox::new();
    let bad_candidate =
        json!({"path": "/x", "status": "banana", "candidates": [{"engine": "IQDB"}]}).to_string();
    hostile_db(
        &sb,
        "",
        &[
            ("/a/b.png", 0, &bad_candidate),
            ("/c", 1, ""),
            ("/d", 2, "{"),
            ("/e", 3, "[]"),
            ("/f", 4, "null"),
            ("/g", 5, "12"),
            ("/h", 6, &json!({"status": "good"}).to_string()),
            ("/i", 7, &json!({"path": ""}).to_string()),
            ("/ok", 8, &good_row("/ok")),
        ],
    );
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.entries[0].path, "/ok");
    assert_eq!(
        loaded.report.skipped.len(),
        6,
        "{:?}",
        loaded.report.skipped
    );
    assert_eq!(loaded.report.ignored, 2);
}

#[test]
fn hostile_a_data_value_that_is_not_text() {
    let sb = Sandbox::new();
    let path = hostile_db(&sb, "", &[("/ok", 0, &good_row("/ok"))]);
    raw_write(
        &path,
        "INSERT INTO entries VALUES ('/blob', 1, x'ff00fe');
         INSERT INTO entries VALUES ('/jsonblob', 2, CAST('{\"path\": \"/jb\"}' AS BLOB));",
    );
    let loaded = store(&sb).load(&opts());
    let paths: Vec<_> = loaded.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["/ok", "/jb"]);
    assert_eq!(loaded.report.skipped.len(), 1);
}

#[test]
fn hostile_non_finite_numbers_and_lone_surrogates_do_not_cost_the_row() {
    // json.dumps writes NaN and unpaired surrogates; json.loads reads them back.
    let sb = Sandbox::new();
    let row = r#"{"path": "/a", "error_message": "bad \udcff name", "candidates": [{"url": "u", "similarity": NaN, "width": Infinity}]}"#;
    hostile_db(&sb, "", &[("/a", 0, row)]);
    let loaded = store(&sb).load(&opts());
    assert!(loaded.report.skipped.is_empty());
    let e = &loaded.entries[0];
    assert_eq!(e.error_message.as_deref(), Some("bad \u{fffd} name"));
    assert_eq!(e.candidates[0].similarity, 0.0);
    assert_eq!(e.candidates[0].width, None);
}

#[test]
fn hostile_wrong_typed_values_load_with_defaults_and_a_notice() {
    let sb = Sandbox::new();
    let row = json!({"path": "/a", "hydrus_hash": 12, "local_width": "wide", "reviewed": "yes",
                     "candidates": [{"url": "u", "similarity": "high"}]})
    .to_string();
    hostile_db(&sb, "", &[("/a", 0, &row)]);
    let loaded = store(&sb).load(&opts());
    let e = &loaded.entries[0];
    assert_eq!(e.hydrus_hash, None);
    assert_eq!(e.local_width, None);
    assert!(e.reviewed, "bool(...) of a non-empty string");
    let fields: Vec<_> = loaded
        .report
        .notices
        .iter()
        .filter_map(|n| match n {
            Notice::FieldCoerced { row: 0, field } => Some(*field),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["hydrus_hash", "local_width", "similarity"]);
}

#[test]
fn hostile_an_extra_not_null_column_reads_but_never_saves() {
    // 3.8: reads work, every save fails and the session stops persisting. Rust says so.
    let sb = Sandbox::new();
    let path = hostile_db(&sb, "", &[("/a", 0, &good_row("/a"))]);
    raw_write(
        &path,
        "PRAGMA writable_schema = ON;
         UPDATE sqlite_master SET sql = replace(sql, 'data    TEXT NOT NULL', 'data    TEXT NOT NULL, extra TEXT NOT NULL') WHERE name = 'entries';
         PRAGMA writable_schema = OFF;",
    );
    // The edited schema applies to a fresh connection.
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 1);
    let err = writer(&sb).save(&loaded.entries, NOW).unwrap_err();
    assert!(err.to_string().contains("NOT NULL"), "{err}");
}

#[test]
fn hostile_a_file_that_is_not_a_database_is_left_alone() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.config_dir()).unwrap();
    let junk = b"definitely not an sqlite database, just text that is long enough to have a header"
        .repeat(8);
    fs::write(sb.paths.session_db(), &junk).unwrap();
    let loaded = store(&sb).load(&opts());
    assert!(loaded.entries.is_empty());
    assert!(
        matches!(loaded.report.notices.as_slice(), [Notice::DatabaseUnusable { message }] if message.contains("not a database")),
        "{:?}",
        loaded.report.notices
    );
    assert!(writer(&sb).save(&[entry("/a")], NOW).is_err());
    assert_eq!(
        fs::read(sb.paths.session_db()).unwrap(),
        junk,
        "not replaced"
    );
    assert_eq!(store(&sb).entry_count(), 0);
}

#[test]
fn hostile_an_empty_file_and_a_database_without_tables_are_an_empty_session() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.config_dir()).unwrap();
    fs::write(sb.paths.session_db(), b"").unwrap();
    let loaded = store(&sb).load(&opts());
    assert!(loaded.entries.is_empty());
    assert!(loaded.report.notices.is_empty());
    // ...and a save into it works.
    writer(&sb).save(&[entry("/a")], NOW).unwrap();
    assert_eq!(store(&sb).load(&opts()).entries.len(), 1);
}

#[test]
fn hostile_two_rows_with_the_same_inner_path_do_not_break_a_save() {
    let sb = Sandbox::new();
    hostile_db(
        &sb,
        "",
        &[
            ("/p1", 0, &good_row("/same")),
            ("/p2", 1, &good_row("/same")),
        ],
    );
    let loaded = store(&sb).load(&opts());
    assert_eq!(loaded.entries.len(), 2);
    writer(&sb).save(&loaded.entries, NOW).unwrap();
    // Both rows are replaced by the one row the path column can hold.
    assert_eq!(rows(&sb.paths.session_db()).len(), 1);
}

#[test]
fn hostile_a_large_session_round_trips() {
    let sb = Sandbox::new();
    let list: Vec<Entry> = (0..3000)
        .map(|i| {
            let mut e = entry(&format!("/lib/{i:05}.png"));
            e.status = MatchStatus::ALL[i % 6];
            e.candidates = vec![Candidate::new(format!("https://x.invalid/{i}"))];
            e
        })
        .collect();
    writer(&sb).save(&list, NOW).unwrap();
    let back = store(&sb).load(&opts());
    assert!(back.report.skipped.is_empty());
    assert_eq!(back.entries, list);
}

#[test]
fn everything_stays_inside_the_sandbox() {
    let sb = Sandbox::new();
    writer(&sb).save(&[entry("/a")], NOW).unwrap();
    sb.assert_sandboxed();
    assert!(sb.paths.session_db().starts_with(sb.dir.path()));
}
