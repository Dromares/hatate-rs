//! Writes a small profile with the Rust code, for the black-box check against the Python
//! app (DAN-1982 acceptance: "a Rust-written entry is read by the Python app's own cache
//! reader at 76c85c4 as a hit"). Ignored by default; the verifier runs it with
//! `HATATE_BLACKBOX_OUT=<new empty dir under $TMPDIR> cargo test -p hatate-core --test
//! blackbox_emit -- --ignored`, then points a Python 76c85c4 checkout at
//! `XDG_CONFIG_HOME=$HATATE_BLACKBOX_OUT/config` and calls `core.search_cache`,
//! `core.saucenao` and `core.crashlog`. The Python side is a throw-away script and does
//! not belong in this Rust-only tree.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

use hatate_core::cache::{
    CacheEntry, CacheKey, Candidate, MatchStatus, SearchCache, StoreOutcome, Tag,
};
use hatate_core::crash_marker::CrashMarkers;
use hatate_core::paths::Paths;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::rotating_log::{Level, RotatingLog};

fn key(c: char) -> CacheKey {
    CacheKey::parse(&c.to_string().repeat(64)).unwrap()
}

#[test]
#[ignore = "run by hand for the Python black-box check; needs HATATE_BLACKBOX_OUT"]
fn emit_profile_for_the_python_black_box() {
    let Some(out) = std::env::var_os("HATATE_BLACKBOX_OUT") else {
        panic!("set HATATE_BLACKBOX_OUT to a new empty directory under the temp dir");
    };
    let out = PathBuf::from(out);
    assert!(out.starts_with(std::env::temp_dir()), "temp dirs only");
    let vars: HashMap<&str, OsString> = HashMap::from([
        ("HOME", out.join("home").into()),
        ("XDG_CONFIG_HOME", out.join("config").into()),
        ("XDG_CACHE_HOME", out.join("cache").into()),
    ]);
    let paths = Paths::from_lookup(|k| vars.get(k).cloned()).unwrap();
    let now = hatate_core::timeutil::now_epoch();
    let cache = SearchCache::new(&paths);

    // 1. A found result with two candidates, tags (one non-ASCII), the second selected.
    let mut a = Candidate::new("https://danbooru.donmai.us/posts/1000001");
    a.source_name = Some("Danbooru".into());
    a.similarity = 96.5;
    a.width = Some(1600);
    a.height = Some(1200);
    a.engine_tags = vec![Tag {
        name: "caf\u{e9}".into(),
        namespace: Some("character".into()),
    }];
    a.booru_tags = vec![Tag {
        name: "solo".into(),
        namespace: None,
    }];
    a.remote_available = Some(true);
    a.availability_checked_at = Some(now);
    let mut b = Candidate::new("https://www.pixiv.net/artworks/1000002");
    b.engine = "SauceNAO".into();
    b.incomplete_reason = Some("login required".into());
    let mut good = CacheEntry::new(MatchStatus::Good, vec![a, b], now);
    good.selected_candidate_index = 1;
    assert_eq!(
        cache.store(&key('1'), &good, false).unwrap(),
        StoreOutcome::Written
    );

    // 2. A MangaDex candidate: tag_version 1 must make Python treat the tags as current.
    let mut md =
        Candidate::new("https://mangadex.org/chapter/0123abcd-4567-89ab-cdef-0123456789ab");
    md.engine = "SauceNAO".into();
    let entry = CacheEntry::new(MatchStatus::Poor, vec![md], now);
    cache.store(&key('2'), &entry, false).unwrap();

    // 3. A not_found with no candidates.
    cache
        .store(
            &key('3'),
            &CacheEntry::new(MatchStatus::NotFound, vec![], now),
            false,
        )
        .unwrap();

    // Quota pause: started now, so it is still in force for any reasonable check.
    QuotaPauseFile::new(&paths)
        .record(&QuotaPause::starting_at(now, 12, 34))
        .unwrap();

    // Crash markers: a *clean* Rust run (so Python must see no interrupted run), and for
    // the other direction a second profile entry point is not needed.
    let started = CrashMarkers::begin(&paths, now);
    assert!(!started.previous_run_unclean);
    started.session.finish_clean(now + 1.0).unwrap();

    let mut log = RotatingLog::start(&paths, now).unwrap();
    log.record(now, Level::Info, "hatate.test", "black-box profile written")
        .unwrap();
}
