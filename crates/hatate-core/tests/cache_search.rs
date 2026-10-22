//! Search cache, `docs/file-formats.md` section 4 and rules 6.3 (DAN-1982).
//! Every test runs in a temp profile (`common::Sandbox`) and never touches the real one.

mod common;

use std::fs;

use common::{Sandbox, corpus};
use hatate_core::cache::{
    CACHE_FORMAT_VERSION, CacheEntry, CacheKey, Candidate, Lookup, Malformed, MatchStatus,
    MissReason, SearchCache, StoreOutcome, Tag, entry_to_json,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SAMPLE_GOOD: &str = "fresh-profile/xdg-config/hatate-linux/search_cache/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.json";
const SAMPLE_NOT_FOUND: &str = "fresh-profile/xdg-config/hatate-linux/search_cache/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.json";
const DAY: f64 = 86_400.0;
const T0: f64 = 1_792_639_907.557_621; // `cached_at` of the "good" sample

fn key(c: char) -> CacheKey {
    CacheKey::parse(&c.to_string().repeat(64)).unwrap()
}

fn cache(sb: &Sandbox) -> SearchCache {
    SearchCache::new(&sb.paths)
}

fn put_raw(sb: &Sandbox, k: &CacheKey, bytes: &[u8]) {
    let c = cache(sb);
    fs::create_dir_all(c.dir()).unwrap();
    fs::write(c.dir().join(format!("{}.json", k.as_str())), bytes).unwrap();
}

fn on_disk(sb: &Sandbox, k: &CacheKey) -> Value {
    let p = cache(sb).dir().join(format!("{}.json", k.as_str()));
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}

fn good_entry(now: f64) -> CacheEntry {
    let mut c = Candidate::new("https://danbooru.donmai.us/posts/1");
    c.similarity = 91.25;
    c.engine = "SauceNAO".into();
    c.engine_tags = vec![Tag {
        name: "é日".into(),
        namespace: Some("character".into()),
    }];
    CacheEntry::new(MatchStatus::Good, vec![c], now)
}

// ---- key -------------------------------------------------------------------------

#[test]
fn key_is_lowercase_sha256_hex() {
    // SHA-256("abc"), the FIPS 180 test vector.
    let digest: [u8; 32] = Sha256::digest(b"abc").into();
    let k = CacheKey::from_digest(&digest);
    assert_eq!(
        k.as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(CacheKey::parse(k.as_str()).unwrap(), k);
}

#[test]
fn key_refuses_anything_that_is_not_64_lowercase_hex() {
    let ok = "a".repeat(64);
    assert!(CacheKey::parse(&ok).is_ok());
    for bad in [
        "A".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        String::new(),
        format!("{}/", "a".repeat(63)),
        format!("../{}", "a".repeat(61)),
        format!("{}.json", "a".repeat(64)),
        "g".repeat(64),
        format!("{}é", "a".repeat(62)),
    ] {
        assert!(CacheKey::parse(&bad).is_err(), "{bad:?}");
    }
}

// ---- corpus samples (acceptance: every sample reads as a hit) ----------------------

#[test]
fn every_corpus_sample_reads_as_a_hit() {
    let dir = corpus("fresh-profile/xdg-config/hatate-linux/search_cache");
    let mut seen = 0;
    for item in fs::read_dir(dir).unwrap() {
        let item = item.unwrap();
        let name = item.file_name().to_string_lossy().into_owned();
        let sb = Sandbox::new();
        let k = CacheKey::parse(name.strip_suffix(".json").unwrap()).unwrap();
        let bytes = fs::read(item.path()).unwrap();
        put_raw(&sb, &k, &bytes);
        // One second after the file was written: neither the TTL nor the 30-day
        // availability rule has anything to say yet.
        let at: f64 = serde_json::from_slice::<Value>(&bytes).unwrap()["cached_at"]
            .as_f64()
            .unwrap();
        let hit = cache(&sb).lookup(&k, 30.0, at + 1.0);
        assert!(matches!(hit, Lookup::Hit(_)), "{name}: {hit:?}");
        seen += 1;
    }
    assert_eq!(seen, 2, "the corpus changed: update this test");
}

#[test]
fn the_good_sample_reads_field_for_field() {
    let sb = Sandbox::new();
    let k = key('a');
    put_raw(&sb, &k, &fs::read(corpus(SAMPLE_GOOD)).unwrap());
    let e = cache(&sb).lookup(&k, 30.0, T0 + 1.0).hit().unwrap();
    assert_eq!(e.format_version, 2);
    assert_eq!(e.status, MatchStatus::Good);
    assert_eq!(e.cached_at, Some(T0));
    assert_eq!(e.selected_candidate_index, 0);
    assert_eq!(e.candidates.len(), 2);
    let (a, b) = (&e.candidates[0], &e.candidates[1]);
    assert_eq!(a.url, "https://danbooru.donmai.us/posts/1000001");
    assert_eq!(a.source_name.as_deref(), Some("Danbooru"));
    assert_eq!(
        (a.similarity, a.width, a.height),
        (96.5, Some(1600), Some(1200))
    );
    assert_eq!(
        a.engine_tags[1],
        Tag {
            name: "example_character".into(),
            namespace: Some("character".into())
        }
    );
    assert_eq!(a.booru_tags.len(), 2);
    assert_eq!(a.remote_size_bytes, Some(123_456));
    assert_eq!(a.remote_available, Some(true));
    assert_eq!(a.availability_checked_at, Some(1_792_639_907.537_248_4));
    assert!(a.booru_tags_fetched && a.remote_info_fetched);
    assert_eq!(b.engine, "SauceNAO");
    assert_eq!(b.incomplete_reason.as_deref(), Some("login required"));
    assert_eq!(b.remote_available, None);
    assert_eq!(b.thumb_url, None);
}

/// Writing what was read gives back Python's bytes: same keys, order, separators and
/// float text. The Python app wrote these samples with `json.dumps`.
#[test]
fn writing_a_sample_back_is_byte_identical_to_python() {
    for name in [SAMPLE_GOOD, SAMPLE_NOT_FOUND] {
        let sb = Sandbox::new();
        let k = key('c');
        let bytes = fs::read(corpus(name)).unwrap();
        put_raw(&sb, &k, &bytes);
        let at = serde_json::from_slice::<Value>(&bytes).unwrap()["cached_at"]
            .as_f64()
            .unwrap();
        let e = cache(&sb).lookup(&k, 30.0, at + 1.0).hit().unwrap();
        assert_eq!(entry_to_json(&e).as_bytes(), &bytes[..], "{name}");
    }
}

// ---- rule 6.3: what is written -----------------------------------------------------

#[test]
fn a_stored_entry_has_the_six_keys_and_format_version_2() {
    let sb = Sandbox::new();
    let k = key('1');
    assert_eq!(
        cache(&sb).store(&k, &good_entry(1000.5), false).unwrap(),
        StoreOutcome::Written
    );
    let v = on_disk(&sb, &k);
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "format_version",
            "cached_at",
            "status",
            "error_message",
            "selected_candidate_index",
            "candidates"
        ]
    );
    assert_eq!(v["format_version"], CACHE_FORMAT_VERSION);
    assert_eq!(v["format_version"], 2);
    assert_eq!(v["cached_at"], 1000.5);
    assert_eq!(v["status"], "good");
    let cand_keys: Vec<&str> = v["candidates"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(cand_keys.len(), 20);
    assert_eq!(cand_keys[0], "url");
    assert_eq!(cand_keys[19], "tag_version");
}

#[test]
fn the_file_is_named_by_the_key_and_the_cache_dir_is_created() {
    let sb = Sandbox::new();
    let k = key('2');
    assert!(!sb.paths.search_cache_dir().exists());
    cache(&sb).store(&k, &good_entry(1.0), false).unwrap();
    assert!(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", "2".repeat(64)))
            .is_file()
    );
    // Exactly one file, no temp file left behind.
    assert_eq!(
        fs::read_dir(sb.paths.search_cache_dir()).unwrap().count(),
        1
    );
}

#[test]
fn tag_version_is_1_for_mangadex_and_0_otherwise() {
    let sb = Sandbox::new();
    let k = key('3');
    let mut e = good_entry(1.0);
    e.candidates = [
        "https://mangadex.org/chapter/0123abcd-4567-89ab-cdef-0123456789ab",
        "https://mangadex.org/title/0123abcd-4567-89ab-cdef-0123456789ab",
        "https://danbooru.donmai.us/posts/9",
        "https://www.pixiv.net/artworks/9",
    ]
    .map(Candidate::new)
    .to_vec();
    cache(&sb).store(&k, &e, false).unwrap();
    let v = on_disk(&sb, &k);
    let tv: Vec<i64> = v["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["tag_version"].as_i64().unwrap())
        .collect();
    assert_eq!(tv, [1, 0, 0, 0]);
}

#[test]
fn error_results_are_never_cached() {
    let sb = Sandbox::new();
    let k = key('4');
    let mut e = good_entry(1.0);
    e.status = MatchStatus::Error;
    e.error_message = Some("rate limited".into());
    assert_eq!(
        cache(&sb).store(&k, &e, false).unwrap(),
        StoreOutcome::SkippedError
    );
    assert!(
        !sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str()))
            .exists()
    );
    assert_eq!(
        cache(&sb).lookup(&k, 30.0, 2.0),
        Lookup::Miss(MissReason::Absent)
    );
}

#[test]
fn provisional_results_are_never_cached() {
    let sb = Sandbox::new();
    let k = key('5');
    // Even a good-looking result found while SauceNAO was skipped.
    assert_eq!(
        cache(&sb).store(&k, &good_entry(1.0), true).unwrap(),
        StoreOutcome::SkippedProvisional
    );
    assert!(
        !sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str()))
            .exists()
    );
    // An error that is also provisional is still just "not cached".
    let mut e = good_entry(1.0);
    e.status = MatchStatus::Error;
    assert!(matches!(
        cache(&sb).store(&k, &e, true).unwrap(),
        StoreOutcome::SkippedError | StoreOutcome::SkippedProvisional
    ));
    assert!(!sb.paths.search_cache_dir().exists());
}

#[test]
fn a_skipped_store_leaves_an_existing_entry_alone() {
    let sb = Sandbox::new();
    let k = key('6');
    cache(&sb).store(&k, &good_entry(1.0), false).unwrap();
    let before = fs::read(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str())),
    )
    .unwrap();
    let mut e = good_entry(2.0);
    e.status = MatchStatus::Error;
    cache(&sb).store(&k, &e, false).unwrap();
    cache(&sb).store(&k, &good_entry(3.0), true).unwrap();
    let after = fs::read(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str())),
    )
    .unwrap();
    assert_eq!(before, after);
}

#[test]
fn what_is_stored_reads_back() {
    let sb = Sandbox::new();
    let k = key('7');
    let mut e = good_entry(1_800_000_000.25);
    e.error_message = Some("note".into());
    e.candidates
        .push(Candidate::new("https://example.invalid/2"));
    e.selected_candidate_index = 1;
    cache(&sb).store(&k, &e, false).unwrap();
    let back = cache(&sb).lookup(&k, 0.0, 1_800_000_001.0).hit().unwrap();
    assert_eq!(back.status, MatchStatus::Good);
    assert_eq!(back.cached_at, Some(1_800_000_000.25));
    assert_eq!(back.error_message.as_deref(), Some("note"));
    assert_eq!(back.selected_candidate_index, 1);
    assert_eq!(back.candidates.len(), 2);
    assert_eq!(back.candidates[0].engine_tags[0].name, "é日");
    assert_eq!(back.candidates[0].similarity, 91.25);
    // Everything stored by this version counts as fetched.
    assert!(
        back.candidates
            .iter()
            .all(|c| c.booru_tags_fetched && c.remote_info_fetched)
    );
}

#[test]
fn non_ascii_is_escaped_like_python_ensure_ascii() {
    let sb = Sandbox::new();
    let k = key('8');
    cache(&sb).store(&k, &good_entry(1.0), false).unwrap();
    let text = fs::read_to_string(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str())),
    )
    .unwrap();
    assert!(text.is_ascii());
    let bs = char::from(92u8); // a backslash, spelled out so no tool re-reads the escape
    assert!(text.contains(&format!("\"name\": \"{bs}u00e9{bs}u65e5\"")));
    assert_eq!(text.lines().count(), 1);
}

// ---- rule 6.3: what is a miss ------------------------------------------------------

#[test]
fn a_missing_file_is_a_miss() {
    let sb = Sandbox::new();
    assert_eq!(
        cache(&sb).lookup(&key('a'), 30.0, 1.0),
        Lookup::Miss(MissReason::Absent)
    );
}

/// A Python crash mid-write leaves a truncated file; every prefix of a real entry must be
/// a miss, never a panic and never a half-entry.
#[test]
fn every_truncation_of_a_real_entry_is_a_miss() {
    let sb = Sandbox::new();
    let k = key('9');
    let full = fs::read(corpus(SAMPLE_GOOD)).unwrap();
    for len in 0..full.len() {
        put_raw(&sb, &k, &full[..len]);
        let got = cache(&sb).lookup(&k, 30.0, T0 + 1.0);
        assert!(
            matches!(got, Lookup::Miss(MissReason::Unreadable)),
            "len {len}: {got:?}"
        );
    }
    put_raw(&sb, &k, &full);
    assert!(matches!(
        cache(&sb).lookup(&k, 30.0, T0 + 1.0),
        Lookup::Hit(_)
    ));
}

#[test]
fn unreadable_and_garbage_files_are_misses() {
    let sb = Sandbox::new();
    let k = key('b');
    for bytes in [
        &b""[..],
        b"\xff\xfe\x00not utf8",
        b"\xef\xbb\xbf{\"format_version\": 2}", // BOM, as json.loads(str) rejects it
        b"{",
        b"null garbage",
        b"\x00\x00\x00\x00",
    ] {
        put_raw(&sb, &k, bytes);
        assert_eq!(
            cache(&sb).lookup(&k, 30.0, 1.0),
            Lookup::Miss(MissReason::Unreadable),
            "{bytes:?}"
        );
    }
    // A directory where the file should be: read fails, still a miss.
    fs::remove_file(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str())),
    )
    .unwrap();
    fs::create_dir(
        sb.paths
            .search_cache_dir()
            .join(format!("{}.json", k.as_str())),
    )
    .unwrap();
    assert_eq!(
        cache(&sb).lookup(&k, 30.0, 1.0),
        Lookup::Miss(MissReason::Unreadable)
    );
}

#[test]
fn valid_json_that_is_not_an_entry_is_a_miss() {
    let sb = Sandbox::new();
    let k = key('c');
    let cases: [(&str, Malformed); 9] = [
        ("[]", Malformed::NotAnObject),
        ("5", Malformed::NotAnObject),
        (
            r#"{"status": "good", "candidates": [{"source_name": "x"}]}"#,
            Malformed::MissingRequired,
        ),
        (
            r#"{"status": "good", "candidates": [{"url": "u", "booru_tags": [{"namespace": null}]}]}"#,
            Malformed::MissingRequired,
        ),
        (
            r#"{"status": "good", "candidates": [5]}"#,
            Malformed::Candidates,
        ),
        (
            r#"{"status": "good", "candidates": null}"#,
            Malformed::Candidates,
        ),
        (r#"{"status": "bogus"}"#, Malformed::Status),
        (r#"{"format_version": "two"}"#, Malformed::Number),
        (
            r#"{"candidates": [{"url": "u", "tag_version": "x"}]}"#,
            Malformed::Number,
        ),
    ];
    for (text, why) in cases {
        put_raw(&sb, &k, text.as_bytes());
        assert_eq!(
            cache(&sb).lookup(&k, 30.0, 1.0),
            Lookup::Miss(MissReason::Malformed(why)),
            "{text}"
        );
        // A bad file is left where it is: Python does not delete these either.
        assert!(
            sb.paths
                .search_cache_dir()
                .join(format!("{}.json", k.as_str()))
                .exists()
        );
    }
}

#[test]
fn leniently_read_entries_follow_the_python_defaults() {
    let sb = Sandbox::new();
    let k = key('d');
    // No status -> not_found; no candidates key -> none; format_version missing -> 0.
    put_raw(&sb, &k, br#"{"cached_at": 5}"#);
    let e = cache(&sb).lookup(&k, 0.0, 6.0).hit().unwrap();
    assert_eq!(
        (e.status, e.format_version, e.candidates.len()),
        (MatchStatus::NotFound, 0, 0)
    );
    assert!(e.predates_current_format());
    // Numbers as strings and floats are coerced by int(); out-of-range selection -> 0.
    put_raw(
        &sb,
        &k,
        br#"{"format_version": "2", "status": "poor", "selected_candidate_index": 7,
        "candidates": [{"url": "u", "tag_version": null, "width": 3}]}"#,
    );
    let e = cache(&sb).lookup(&k, 0.0, 6.0).hit().unwrap();
    assert_eq!(
        (e.format_version, e.status, e.selected_candidate_index),
        (2, MatchStatus::Poor, 0)
    );
    assert_eq!(e.candidates[0].engine, "IQDB");
    assert_eq!(e.candidates[0].similarity, 0.0);
    put_raw(
        &sb,
        &k,
        br#"{"status": "good", "selected_candidate_index": 1,
        "candidates": [{"url": "a"}, {"url": "b"}]}"#,
    );
    assert_eq!(
        cache(&sb)
            .lookup(&k, 0.0, 6.0)
            .hit()
            .unwrap()
            .selected_candidate_index,
        1
    );
    put_raw(
        &sb,
        &k,
        br#"{"status": "good", "selected_candidate_index": -1, "candidates": [{"url": "a"}]}"#,
    );
    assert_eq!(
        cache(&sb)
            .lookup(&k, 0.0, 6.0)
            .hit()
            .unwrap()
            .selected_candidate_index,
        0
    );
}

// ---- spec 4: expiry ----------------------------------------------------------------

fn put_status(sb: &Sandbox, k: &CacheKey, status: &str, cached_at: Option<f64>) {
    let at = cached_at.map_or(String::new(), |t| format!(r#""cached_at": {t}, "#));
    put_raw(
        sb,
        k,
        format!(r#"{{"format_version": 2, {at}"status": "{status}", "candidates": []}}"#)
            .as_bytes(),
    );
}

fn exists(sb: &Sandbox, k: &CacheKey) -> bool {
    sb.paths
        .search_cache_dir()
        .join(format!("{}.json", k.as_str()))
        .exists()
}

#[test]
fn only_not_found_expires_and_the_read_deletes_it() {
    let sb = Sandbox::new();
    let k = key('e');
    put_status(&sb, &k, "not_found", Some(1000.0));
    // Exactly at the TTL is still fresh (`>`, not `>=`).
    assert!(matches!(
        cache(&sb).lookup(&k, 30.0, 1000.0 + 30.0 * DAY),
        Lookup::Hit(_)
    ));
    assert!(exists(&sb, &k));
    // One tick past it: a miss, and the file is gone.
    assert_eq!(
        cache(&sb).lookup(&k, 30.0, 1000.0 + 30.0 * DAY + 0.001),
        Lookup::Miss(MissReason::Expired)
    );
    assert!(!exists(&sb, &k));
    // Found results never age out, however old.
    for status in ["good", "poor"] {
        put_status(&sb, &k, status, Some(1000.0));
        assert!(
            matches!(
                cache(&sb).lookup(&k, 30.0, 1000.0 + 3650.0 * DAY),
                Lookup::Hit(_)
            ),
            "{status}"
        );
        assert!(exists(&sb, &k));
    }
}

#[test]
fn ttl_zero_or_negative_never_expires() {
    let sb = Sandbox::new();
    let k = key('f');
    put_status(&sb, &k, "not_found", Some(1000.0));
    for ttl in [0.0, -5.0] {
        assert!(matches!(
            cache(&sb).lookup(&k, ttl, 1000.0 + 9999.0 * DAY),
            Lookup::Hit(_)
        ));
    }
    assert!(exists(&sb, &k));
}

#[test]
fn a_not_found_without_a_usable_timestamp_counts_as_expired() {
    let sb = Sandbox::new();
    let k = key('0');
    put_status(&sb, &k, "not_found", None);
    assert_eq!(
        cache(&sb).lookup(&k, 30.0, 5.0),
        Lookup::Miss(MissReason::Expired)
    );
    assert!(!exists(&sb, &k));
    put_raw(
        &sb,
        &k,
        br#"{"status": "not_found", "cached_at": "yesterday", "candidates": []}"#,
    );
    assert_eq!(
        cache(&sb).lookup(&k, 30.0, 5.0),
        Lookup::Miss(MissReason::Expired)
    );
    // ...but with the TTL off it is a hit.
    put_status(&sb, &k, "not_found", None);
    assert!(matches!(cache(&sb).lookup(&k, 0.0, 5.0), Lookup::Hit(_)));
}

// ---- spec 4: format and tag versions, availability ---------------------------------

#[test]
fn an_old_format_keeps_the_candidates_but_marks_them_unfetched() {
    let sb = Sandbox::new();
    let k = key('1');
    put_raw(&sb, &k, br#"{"format_version": 1, "cached_at": 1, "status": "good",
        "selected_candidate_index": 0, "candidates": [{"url": "https://danbooru.donmai.us/posts/1", "tag_version": 0}]}"#);
    let e = cache(&sb).lookup(&k, 30.0, 2.0).hit().unwrap();
    assert!(e.predates_current_format());
    assert_eq!(e.candidates.len(), 1);
    assert!(!e.candidates[0].booru_tags_fetched && !e.candidates[0].remote_info_fetched);
}

#[test]
fn a_changed_tag_version_rereads_tags_but_not_remote_info() {
    let sb = Sandbox::new();
    let k = key('2');
    let md = "https://mangadex.org/chapter/0123abcd-4567-89ab-cdef-0123456789ab";
    put_raw(
        &sb,
        &k,
        format!(
            r#"{{"format_version": 2, "cached_at": 1, "status": "good", "selected_candidate_index": 0, "candidates": [
              {{"url": "{md}", "tag_version": 0}},
              {{"url": "{md}", "tag_version": 1}},
              {{"url": "{md}"}},
              {{"url": "https://danbooru.donmai.us/posts/1", "tag_version": 1}},
              {{"url": "https://danbooru.donmai.us/posts/1"}}]}}"#
        )
        .as_bytes(),
    );
    let e = cache(&sb).lookup(&k, 30.0, 2.0).hit().unwrap();
    let flags: Vec<(bool, bool)> = e
        .candidates
        .iter()
        .map(|c| (c.booru_tags_fetched, c.remote_info_fetched))
        .collect();
    // MangaDex now declares 1: a stored 0 or absent value is stale. Others declare 0.
    assert_eq!(
        flags,
        [
            (false, true),
            (true, true),
            (false, true),
            (false, true),
            (true, true)
        ]
    );
}

#[test]
fn an_availability_verdict_needs_a_bool_and_a_date_within_30_days() {
    let sb = Sandbox::new();
    let k = key('3');
    let now = 100.0 * DAY;
    let cand = |extra: &str| format!(r#"{{"url": "u"{extra}}}"#);
    let body = [
        cand(&format!(
            r#", "remote_available": true, "availability_checked_at": {}"#,
            now - 29.0 * DAY
        )), // fresh true
        cand(&format!(
            r#", "remote_available": false, "availability_checked_at": {}"#,
            now - 1.0 * DAY
        )), // fresh false
        cand(&format!(
            r#", "remote_available": true, "availability_checked_at": {}"#,
            now - 31.0 * DAY
        )), // stale
        cand(r#", "remote_available": true"#), // undated
        cand(r#", "remote_available": null, "availability_checked_at": 5"#), // null verdict
        cand(&format!(
            r#", "remote_available": 1, "availability_checked_at": {}"#,
            now
        )), // not a bool
        cand(&format!(
            r#", "remote_available": true, "availability_checked_at": "{}""#,
            now
        )), // string date
    ]
    .join(",");
    put_raw(
        &sb,
        &k,
        format!(
            r#"{{"format_version": 2, "cached_at": 1, "status": "good", "candidates": [{body}]}}"#
        )
        .as_bytes(),
    );
    let e = cache(&sb).lookup(&k, 0.0, now).hit().unwrap();
    let got: Vec<Option<bool>> = e.candidates.iter().map(|c| c.remote_available).collect();
    assert_eq!(got, [Some(true), Some(false), None, None, None, None, None]);
    assert_eq!(
        e.candidates[2].availability_checked_at, None,
        "a dropped verdict drops its date too"
    );
}

// ---- drop / clear / size -----------------------------------------------------------

#[test]
fn drop_clear_and_size() {
    let sb = Sandbox::new();
    let c = cache(&sb);
    assert_eq!(
        (c.size_bytes(), c.clear().unwrap()),
        (0, 0),
        "no directory yet"
    );
    assert!(!c.drop_entry(&key('a')).unwrap());
    for ch in ['a', 'b', 'c'] {
        c.store(&key(ch), &good_entry(1.0), false).unwrap();
    }
    let one = fs::metadata(c.dir().join(format!("{}.json", "a".repeat(64))))
        .unwrap()
        .len();
    assert_eq!(c.size_bytes(), 3 * one);
    assert!(c.drop_entry(&key('a')).unwrap());
    assert!(!c.drop_entry(&key('a')).unwrap());
    assert_eq!(
        c.lookup(&key('a'), 0.0, 2.0),
        Lookup::Miss(MissReason::Absent)
    );
    // `clear` removes every *.json and a killed run's temp file, and nothing else.
    fs::write(c.dir().join("notes.txt"), b"keep").unwrap();
    fs::write(c.dir().join(".dead.json.99.0.tmp"), b"half").unwrap();
    assert_eq!(c.clear().unwrap(), 2);
    let mut left: Vec<String> = fs::read_dir(c.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["notes.txt"]);
}

/// Python's own size and clear globs are `*.json`; a temp file must never match.
#[test]
fn temp_files_do_not_look_like_cache_entries() {
    let sb = Sandbox::new();
    let c = cache(&sb);
    c.store(&key('a'), &good_entry(1.0), false).unwrap();
    fs::write(c.dir().join(".x.json.1.1.tmp"), vec![0u8; 5000]).unwrap();
    assert!(c.size_bytes() < 5000);
}
