//! A real `SIGKILL` in the middle of cache writes (DAN-1982 acceptance: "killing the
//! process mid-write leaves either the old file or the new one, never a truncated one").
//!
//! The parent test starts this same test binary as a child that rewrites one cache entry
//! in a loop, alternating between two different large entries, and kills it at varying
//! moments. After every kill the entry must still be a complete hit for one of the two
//! contents. A writer that truncated in place (as the Python app does) fails this within a
//! few kills because the entries are large enough that most kills land mid-write.

mod common;

use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Sandbox;
use hatate_core::cache::{
    CacheEntry, CacheKey, Candidate, Lookup, MatchStatus, SearchCache, entry_to_json,
};

const ENV_ROOT: &str = "HATATE_CACHE_KILL_ROOT";

fn key() -> CacheKey {
    CacheKey::parse(&"e".repeat(64)).unwrap()
}

/// `n` candidates, so the file is about 600 bytes per candidate. A and B differ in size.
fn big_entry(tag: &str, n: usize, at: f64) -> CacheEntry {
    let candidates = (0..n)
        .map(|i| {
            let mut c = Candidate::new(format!("https://example.invalid/{tag}/{i}"));
            c.engine_tags = (0..8)
                .map(|t| hatate_core::cache::Tag {
                    name: format!("tag_{tag}_{i}_{t}"),
                    namespace: None,
                })
                .collect();
            c
        })
        .collect();
    CacheEntry::new(MatchStatus::Good, candidates, at)
}

const A: (&str, usize) = ("a", 1500);
const B: (&str, usize) = ("b", 2500);

fn cache_in(root: &std::path::Path) -> SearchCache {
    let vars = [
        ("XDG_CONFIG_HOME", root.join("xdg/config")),
        ("XDG_CACHE_HOME", root.join("xdg/cache")),
    ];
    let paths = hatate_core::paths::Paths::from_lookup(|k| {
        vars.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.clone().into())
    })
    .unwrap();
    SearchCache::new(&paths)
}

/// Not a test of its own: the child half. Ignored so a normal run skips it; the parent
/// runs it with `--ignored --exact`.
#[test]
#[ignore = "child process of writer_killed_mid_write_never_leaves_a_partial_entry"]
fn child_writer() {
    let Some(root) = std::env::var_os(ENV_ROOT) else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    // Same guard as the sandbox: only ever a temp dir made by these tests.
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("hatate-core-test-")
    );
    let cache = cache_in(&root);
    let k = key();
    let mut i = 0u64;
    loop {
        let (tag, n) = if i % 2 == 0 { B } else { A };
        cache
            .store(&k, &big_entry(tag, n, i as f64), false)
            .unwrap();
        if i == 0 {
            fs::write(root.join("child-ready"), b"1").unwrap();
        }
        i += 1;
    }
}

#[test]
fn writer_killed_mid_write_never_leaves_a_partial_entry() {
    let sb = Sandbox::new();
    let root = sb.dir.path().to_path_buf();
    let cache = cache_in(&root);
    let k = key();
    // The "old" entry every run starts from.
    cache.store(&k, &big_entry(A.0, A.1, 1.0), false).unwrap();
    // The only two lengths the target file may ever have.
    let full_lengths = [
        entry_to_json(&big_entry(A.0, A.1, 1.0)).len() as u64,
        entry_to_json(&big_entry(B.0, B.1, 2.0)).len() as u64,
    ];
    assert_ne!(full_lengths[0], full_lengths[1]);
    let target = cache.dir().join(format!("{}.json", k.as_str()));

    let exe = std::env::current_exe().unwrap();
    let mut completed_between = 0usize;
    for round in 0..40u64 {
        let _ = fs::remove_file(root.join("child-ready"));
        let mut child = Command::new(&exe)
            .args(["--ignored", "--exact", "child_writer", "--test-threads=1"])
            .env(ENV_ROOT, &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child");
        // Wait until the child has written at least once, so the kill lands in its loop.
        let started = Instant::now();
        while !root.join("child-ready").exists() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "child never started"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        if round % 2 == 0 {
            // Kill at the first moment a write is visibly in flight: a temp file exists, or
            // the target is not one of the two complete lengths (a writer that truncates in
            // place). This is what makes the test fail against a non-atomic writer.
            let waited = Instant::now();
            loop {
                let in_flight = fs::read_dir(cache.dir())
                    .unwrap()
                    .any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".tmp")))
                    || fs::metadata(&target).is_ok_and(|m| !full_lengths.contains(&m.len()));
                if in_flight {
                    break;
                }
                assert!(
                    waited.elapsed() < Duration::from_secs(30),
                    "no write ever seen in flight"
                );
            }
        } else {
            // 0..~25 ms of looping before the kill, varied per round.
            std::thread::sleep(Duration::from_micros(200 + (round * 7919) % 25_000));
        }
        child.kill().expect("SIGKILL"); // `Child::kill` is SIGKILL on unix
        child.wait().unwrap();

        match cache.lookup(&k, 0.0, 1e12) {
            Lookup::Hit(e) => {
                let first = &e.candidates[0].url;
                let n = e.candidates.len();
                assert!(
                    (first.contains("/a/") && n == A.1) || (first.contains("/b/") && n == B.1),
                    "round {round}: a mixed or short entry: {first}, {n} candidates"
                );
                completed_between += 1;
            }
            other => panic!("round {round}: expected a complete hit after SIGKILL, got {other:?}"),
        }
    }
    assert_eq!(completed_between, 40);

    // Kills leave temp files behind (never `*.json`); `clear` sweeps them.
    let leftovers = fs::read_dir(cache.dir())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count();
    eprintln!("temp files left by 40 kills: {leftovers}");
    cache.clear().unwrap();
    assert_eq!(fs::read_dir(cache.dir()).unwrap().count(), 0);
}
