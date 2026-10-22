//! Quota pause file, crash markers and the Rust log: `docs/file-formats.md` 5.2, 5.3, 5.5
//! and rules 6.4 (DAN-1982). Temp profiles only (`common::Sandbox`).

mod common;

use std::fs;
use std::panic::{self, AssertUnwindSafe};

use common::{Sandbox, corpus, mode};
use hatate_core::crash_marker::CrashMarkers;
use hatate_core::paths::rotated;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile, StartupPause, parse};
use hatate_core::rotating_log::{BACKUPS, Level, MAX_BYTES, RotatingLog};

const PROFILE: &str = "fresh-profile/xdg-config/hatate-linux";
const T0: f64 = 1_792_639_907.558_033_5; // `paused_at` of the sample
const MIDNIGHT: f64 = 1_792_713_600.0; // the sample's `reset_at`, 2026-10-23T00:00:00+00:00

fn sample(name: &str) -> Vec<u8> {
    fs::read(corpus(&format!("{PROFILE}/{name}"))).unwrap()
}

fn put(sb: &Sandbox, name: &str, bytes: &[u8]) {
    fs::create_dir_all(sb.config_dir()).unwrap();
    fs::write(sb.config_dir().join(name), bytes).unwrap();
}

// =========================== saucenao_quota_pause.json ============================

#[test]
fn the_sample_parses_and_writes_back_byte_identical() {
    let bytes = sample("saucenao_quota_pause.json");
    let p = parse(&bytes).expect("sample parses");
    assert_eq!(
        p,
        QuotaPause {
            paused_at: T0,
            searched: 12,
            remaining: 34,
            reset_at: "2026-10-23T00:00:00+00:00".into()
        }
    );
    // What the Python app wrote is exactly what the Rust writer writes for it.
    assert_eq!(p.to_json().as_bytes(), &bytes[..]);
    // And a pause started at that moment computes the same reset (`daily_quota_reset_at`).
    assert_eq!(QuotaPause::starting_at(T0, 12, 34), p);
}

#[test]
fn record_writes_exactly_the_four_keys_in_order() {
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    f.record(&QuotaPause::starting_at(1_000.25, 3, 9)).unwrap();
    let text = fs::read_to_string(sb.paths.saucenao_quota_pause()).unwrap();
    assert_eq!(
        text,
        r#"{"paused_at": 1000.25, "searched": 3, "remaining": 9, "reset_at": "1970-01-02T00:00:00+00:00"}"#
    );
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["paused_at", "searched", "remaining", "reset_at"]);
    // The directory is created when it does not exist yet, and no temp file is left.
    assert_eq!(sb.listing(), ["saucenao_quota_pause.json"]);
}

#[test]
fn reset_is_the_next_utc_midnight() {
    // 2026-10-22 23:59:59.9 UTC -> 2026-10-23; 2026-10-23 00:00:00 -> 2026-10-24.
    assert_eq!(
        QuotaPause::starting_at(MIDNIGHT - 0.1, 0, 0).reset_at,
        "2026-10-23T00:00:00+00:00"
    );
    assert_eq!(
        QuotaPause::starting_at(MIDNIGHT, 0, 0).reset_at,
        "2026-10-24T00:00:00+00:00"
    );
    assert_eq!(
        QuotaPause::starting_at(MIDNIGHT, 0, 0).reset_epoch(),
        Some(MIDNIGHT + 86_400.0)
    );
}

#[test]
fn an_existing_python_file_is_honoured_while_in_force() {
    let sb = Sandbox::new();
    put(
        &sb,
        "saucenao_quota_pause.json",
        &sample("saucenao_quota_pause.json"),
    );
    let f = QuotaPauseFile::new(&sb.paths);
    // One second before the reset: still paused, and the file stays.
    match f.check_at_startup(MIDNIGHT - 1.0).unwrap() {
        StartupPause::Active(p) => assert_eq!((p.searched, p.remaining), (12, 34)),
        other => panic!("{other:?}"),
    }
    assert!(sb.paths.saucenao_quota_pause().exists());
}

#[test]
fn a_pause_whose_reset_has_passed_is_deleted_at_startup() {
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    for now in [MIDNIGHT, MIDNIGHT + 1.0, MIDNIGHT + 10.0 * 86_400.0] {
        put(
            &sb,
            "saucenao_quota_pause.json",
            &sample("saucenao_quota_pause.json"),
        );
        assert!(
            matches!(f.check_at_startup(now).unwrap(), StartupPause::Cleared(_)),
            "{now}"
        );
        assert!(!sb.paths.saucenao_quota_pause().exists(), "{now}");
    }
}

#[test]
fn an_unparsable_reset_is_deleted_at_startup() {
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    for reset in [
        r#""soon""#,
        r#""""#,
        "5",
        "null",
        r#""2026-99-99T00:00:00+00:00""#,
    ] {
        put(
            &sb,
            "saucenao_quota_pause.json",
            format!(r#"{{"paused_at": 1.0, "searched": 1, "remaining": 1, "reset_at": {reset}}}"#)
                .as_bytes(),
        );
        assert!(
            matches!(f.check_at_startup(5.0).unwrap(), StartupPause::Cleared(_)),
            "{reset}"
        );
        assert!(!sb.paths.saucenao_quota_pause().exists(), "{reset}");
    }
}

#[test]
fn a_missing_or_corrupt_file_means_no_pause_and_is_left_alone() {
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    assert_eq!(f.check_at_startup(5.0).unwrap(), StartupPause::None);
    for bytes in [
        &b""[..],
        b"{",
        b"[]",
        b"null",
        br#"{"paused_at": 1.0}"#,
        br#"{"paused_at": "x", "searched": 1, "remaining": 1, "reset_at": "2099-01-01"}"#,
        br#"{"paused_at": 1.0, "searched": null, "remaining": 1, "reset_at": "2099-01-01"}"#,
        b"\xff\xfe",
    ] {
        put(&sb, "saucenao_quota_pause.json", bytes);
        assert_eq!(f.read(), None, "{bytes:?}");
        assert_eq!(f.check_at_startup(5.0).unwrap(), StartupPause::None);
        // Python's `get_quota_pause_state` returns None and nothing deletes the file.
        assert!(sb.paths.saucenao_quota_pause().exists(), "{bytes:?}");
    }
}

#[test]
fn numbers_are_coerced_like_python_float_and_int() {
    let p = parse(br#"{"paused_at": "12.5", "searched": 7.9, "remaining": "4", "reset_at": "x"}"#)
        .unwrap();
    assert_eq!((p.paused_at, p.searched, p.remaining), (12.5, 7, 4));
    // Extra keys are ignored.
    assert!(
        parse(br#"{"paused_at": 1, "searched": 1, "remaining": 1, "reset_at": "a", "more": 1}"#)
            .is_some()
    );
}

#[test]
fn clear_removes_the_file_and_is_fine_when_there_is_none() {
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    f.clear().unwrap();
    f.record(&QuotaPause::starting_at(1.0, 1, 1)).unwrap();
    f.clear().unwrap();
    assert!(!sb.paths.saucenao_quota_pause().exists());
}

#[test]
fn a_python_reader_would_accept_what_record_writes() {
    // `get_quota_pause_state` is `float`/`int`/`int`/`str` of the four keys; this is the
    // same reading, applied to the Rust bytes (the Python black-box run is in the PR).
    let sb = Sandbox::new();
    let f = QuotaPauseFile::new(&sb.paths);
    let want = QuotaPause::starting_at(1_792_639_907.5, 5, 6);
    f.record(&want).unwrap();
    assert_eq!(f.read(), Some(want));
}

// ================================ crash markers ===================================

#[test]
fn a_clean_run_leaves_no_marker_and_no_banner_for_the_next_launch() {
    let sb = Sandbox::new();
    let started = CrashMarkers::begin(&sb.paths, 1_000.5);
    assert!(!started.previous_run_unclean, "fresh profile");
    assert!(started.marker_error.is_none());
    // While running, the marker holds `str(time.time())`.
    assert_eq!(
        fs::read_to_string(sb.paths.crash_running_marker()).unwrap(),
        "1000.5"
    );
    started.session.finish_clean(2_000.25).unwrap();
    assert!(
        !sb.paths.crash_running_marker().exists(),
        "marker removed on clean exit"
    );
    assert_eq!(
        fs::read_to_string(sb.paths.last_clean_exit()).unwrap(),
        "2000.25"
    );
    // The next launch (either app) finds nothing: no "interrupted run" banner.
    assert!(!CrashMarkers::new(&sb.paths).check_unclean_shutdown());
    let next = CrashMarkers::begin(&sb.paths, 3_000.0);
    assert!(!next.previous_run_unclean);
}

#[test]
fn a_run_that_never_finishes_cleanly_is_seen_by_the_next_launch() {
    let sb = Sandbox::new();
    let first = CrashMarkers::begin(&sb.paths, 1.0);
    drop(first); // no Drop impl: dropping is not a clean exit
    assert!(sb.paths.crash_running_marker().exists());
    let second = CrashMarkers::begin(&sb.paths, 2.0);
    assert!(second.previous_run_unclean);
    // And the new run's own marker replaced the old one.
    assert_eq!(
        fs::read_to_string(sb.paths.crash_running_marker()).unwrap(),
        "2.0"
    );
}

#[test]
fn a_panic_is_not_a_clean_exit() {
    let sb = Sandbox::new();
    let res = panic::catch_unwind(AssertUnwindSafe(|| {
        let _started = CrashMarkers::begin(&sb.paths, 1.0);
        panic!("boom");
    }));
    assert!(res.is_err());
    assert!(
        sb.paths.crash_running_marker().exists(),
        "unwinding must not mark a clean exit"
    );
    assert!(!sb.paths.last_clean_exit().exists());
}

#[test]
fn a_marker_left_by_the_python_app_is_seen_and_replaced() {
    let sb = Sandbox::new();
    put(&sb, "crash.log.running", &sample("crash.log.running"));
    let started = CrashMarkers::begin(&sb.paths, 5.0);
    assert!(started.previous_run_unclean);
    started.session.finish_clean(6.0).unwrap();
    assert!(!sb.paths.crash_running_marker().exists());
}

#[test]
fn marker_text_has_the_shape_python_writes() {
    // Python: `str(time.time())`, e.g. "1792639907.5580258" (samples in the corpus).
    let sb = Sandbox::new();
    let t = 1_792_639_907.558_025_8;
    let s = CrashMarkers::begin(&sb.paths, t);
    assert_eq!(
        fs::read(sb.paths.crash_running_marker()).unwrap(),
        sample("crash.log.running")
    );
    s.session.finish_clean(1_792_639_907.558_015_6).unwrap();
    assert_eq!(
        fs::read(sb.paths.last_clean_exit()).unwrap(),
        sample("last_clean_exit")
    );
}

#[test]
fn clean_exit_with_no_marker_is_fine_and_age_is_read_like_python() {
    let sb = Sandbox::new();
    let m = CrashMarkers::new(&sb.paths);
    assert_eq!(m.last_clean_exit_age(10.0), None, "none recorded");
    m.mark_clean_shutdown(100.0).unwrap();
    assert_eq!(m.last_clean_exit_age(130.0), Some(30.0));
    assert_eq!(m.last_clean_exit_age(50.0), Some(0.0), "never negative");
    put(&sb, "last_clean_exit", b"not a number");
    assert_eq!(m.last_clean_exit_age(130.0), None);
}

#[test]
fn begin_reports_a_marker_it_could_not_write() {
    let sb = Sandbox::new();
    // A directory where the marker should be: the write cannot succeed.
    fs::create_dir_all(sb.paths.crash_running_marker().join("inner")).unwrap();
    let started = CrashMarkers::begin(&sb.paths, 1.0);
    assert!(started.marker_error.is_some());
}

// ================================= hatate-rs.log ==================================

#[test]
fn the_rust_log_is_hatate_rs_log_and_never_app_log() {
    let sb = Sandbox::new();
    put(
        &sb,
        "app.log",
        b"2026-10-22 03:00:00 [INFO] hatate: Logging started, writing to /x/app.log\n",
    );
    let before = fs::read(sb.paths.app_log()).unwrap();
    let mut log = RotatingLog::start(&sb.paths, 1_792_639_907.5).unwrap();
    log.record(1_792_639_908.0, Level::Warning, "hatate.cache", "a message")
        .unwrap();
    assert_eq!(log.path(), sb.paths.rust_log_file());
    assert_eq!(
        sb.paths.rust_log_file().file_name().unwrap(),
        "hatate-rs.log"
    );
    assert_eq!(
        fs::read(sb.paths.app_log()).unwrap(),
        before,
        "app.log is untouched"
    );
    assert!(!rotated(&sb.paths.app_log(), 1).exists());
    let text = fs::read_to_string(sb.paths.rust_log_file()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "2026-10-22 03:31:47 [INFO] hatate: Logging started, writing to {} (timestamps are UTC)",
            sb.paths.rust_log_file().display()
        )
    );
    assert_eq!(
        lines[1],
        "2026-10-22 03:31:48 [WARNING] hatate.cache: a message"
    );
    // Like app.log: world-readable per umask, not 0600 (spec 1.1 "other files").
    assert_eq!(mode(&sb.paths.rust_log_file()) & 0o600, 0o600);
}

#[test]
fn the_log_directory_is_created_and_a_second_run_appends() {
    let sb = Sandbox::new();
    drop(RotatingLog::start(&sb.paths, 1.0).unwrap());
    drop(RotatingLog::start(&sb.paths, 2.0).unwrap());
    let text = fs::read_to_string(sb.paths.rust_log_file()).unwrap();
    assert_eq!(text.matches("Logging started").count(), 2);
    assert_eq!(sb.listing(), ["hatate-rs.log"]);
}

#[test]
fn defaults_are_the_app_log_limits() {
    assert_eq!((MAX_BYTES, BACKUPS), (2_000_000, 3));
}

/// `RotatingFileHandler.shouldRollover`: roll when `size + len(record + "\n") >= maxBytes`.
#[test]
fn rotation_happens_exactly_at_max_bytes() {
    let sb = Sandbox::new();
    let path = sb.paths.rust_log_file();
    let line = |n: usize| RotatingLog::format_line(1.0, Level::Info, "t", &"x".repeat(n));
    let rec = line(10).len() as u64 + 1; // one record plus its newline
    // Room for exactly two records, the third would reach the limit.
    let mut log = RotatingLog::open(path.clone(), rec * 3, 3).unwrap();
    log.record(1.0, Level::Info, "t", &"x".repeat(10)).unwrap();
    log.record(1.0, Level::Info, "t", &"x".repeat(10)).unwrap();
    assert!(!rotated(&path, 1).exists());
    assert_eq!(fs::metadata(&path).unwrap().len(), rec * 2);
    // size 2*rec + rec == max -> rolls (>=), so the third record starts a new file.
    log.record(1.0, Level::Info, "t", &"x".repeat(10)).unwrap();
    assert_eq!(fs::metadata(rotated(&path, 1)).unwrap().len(), rec * 2);
    assert_eq!(fs::metadata(&path).unwrap().len(), rec);
    // One byte short of the limit does not roll.
    let mut log = RotatingLog::open(sb.config_dir().join("b.log"), rec * 3 + 1, 3).unwrap();
    for _ in 0..3 {
        log.record(1.0, Level::Info, "t", &"x".repeat(10)).unwrap();
    }
    assert!(!rotated(&sb.config_dir().join("b.log"), 1).exists());
}

#[test]
fn rotation_keeps_three_backups_and_drops_the_oldest() {
    let sb = Sandbox::new();
    let path = sb.paths.rust_log_file();
    let mut log = RotatingLog::open(path.clone(), 40, 3).unwrap();
    // Each record is longer than half the limit, so every record rolls the previous one.
    for i in 0..6 {
        log.record(1.0, Level::Info, "t", &format!("record number {i}"))
            .unwrap();
    }
    let read = |p: &std::path::Path| fs::read_to_string(p).unwrap();
    assert!(read(&path).contains("record number 5"));
    assert!(read(&rotated(&path, 1)).contains("record number 4"));
    assert!(read(&rotated(&path, 2)).contains("record number 3"));
    assert!(read(&rotated(&path, 3)).contains("record number 2"));
    assert!(
        !rotated(&path, 4).exists(),
        "only three backups (backupCount=3)"
    );
    // Records 0 and 1 are gone.
    let all: String = [
        path.clone(),
        rotated(&path, 1),
        rotated(&path, 2),
        rotated(&path, 3),
    ]
    .iter()
    .map(|p| read(p))
    .collect();
    assert!(!all.contains("record number 0") && !all.contains("record number 1"));
}

#[test]
fn the_rust_log_never_touches_app_log_files_even_when_rotating() {
    let sb = Sandbox::new();
    for n in ["app.log", "app.log.1", "app.log.2", "app.log.3"] {
        put(&sb, n, n.as_bytes());
    }
    let mut log = RotatingLog::open(sb.paths.rust_log_file(), 30, 3).unwrap();
    for i in 0..10 {
        log.record(1.0, Level::Error, "t", &format!("n{i}"))
            .unwrap();
    }
    for n in ["app.log", "app.log.1", "app.log.2", "app.log.3"] {
        assert_eq!(
            fs::read(sb.config_dir().join(n)).unwrap(),
            n.as_bytes(),
            "{n}"
        );
    }
}

#[test]
fn an_existing_log_is_appended_to_and_its_size_counts() {
    let sb = Sandbox::new();
    let path = sb.paths.rust_log_file();
    put(&sb, "hatate-rs.log", &vec![b'a'; 95]);
    let mut log = RotatingLog::open(path.clone(), 100, 3).unwrap();
    log.record(1.0, Level::Info, "t", "pushes it over").unwrap();
    assert_eq!(fs::metadata(rotated(&path, 1)).unwrap().len(), 95);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("pushes it over")
    );
}
