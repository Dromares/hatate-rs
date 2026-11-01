//! DAN-2501: a panic is written to `crash.log`, and the next start still shows the banner.
//!
//! The panicking process is this test binary run again with `HATATE_CRASH_CHILD` set: the test
//! `child_panics` does nothing unless that variable names a profile directory. The parent reads
//! what the child left.

use hatate_core::crash_marker::{self, CrashMarkers};
use hatate_core::paths::Paths;
use hatate_core::stop_record::{LaunchBanner, Read, launch_banner};
use hatate_core::version::VERSION;
use std::process::Command;

const CHILD_ENV: &str = "HATATE_CRASH_CHILD";

/// Runs only as the child of `a_panic_is_written_to_crash_log_...`.
#[test]
fn child_panics() {
    let Some(home) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let paths = Paths::from_lookup(|n| (n == "HOME").then(|| home.clone())).unwrap();
    // What `hatate-gtk` does at startup, in its order.
    crash_marker::install_panic_hook(&paths, "hatate-gtk");
    crash_marker::arm_crash_log(&paths);
    let _started = CrashMarkers::begin(&paths, 1.0);
    std::thread::Builder::new()
        .name("worker-7".into())
        .spawn(|| panic!("the child's panic message"))
        .unwrap()
        .join()
        .unwrap_err();
    // A panic on the main thread ends the process as it would in the app.
    panic!("main thread gives up");
}

fn run_child(home: &std::path::Path) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["child_panics", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env("RUST_BACKTRACE", "0")
        .output()
        .unwrap()
}

#[test]
fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::from_lookup(|n| (n == "HOME").then(|| home.path().into())).unwrap();

    let out = run_child(home.path());
    assert!(!out.status.success(), "the child must die of its panic");
    // The default hook ran after ours: stderr says what it always said.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("main thread gives up"), "{stderr}");

    let log = std::fs::read_to_string(paths.crash_log()).unwrap();
    let marks = log.matches(crash_marker_mark()).count();
    assert_eq!(marks, 1, "one section for the one run:\n{log}");
    let entries: Vec<&str> = log
        .lines()
        .filter(|l| l.starts_with("Fatal Rust panic:"))
        .collect();
    // The worker's panic and the main thread's: each is an entry. The header is the line the
    // banner counts, so the count is the number of panics, not of backtrace frames.
    assert_eq!(
        entries,
        [
            "Fatal Rust panic: the child's panic message",
            "Fatal Rust panic: main thread gives up"
        ],
        "{log}"
    );
    assert!(log.contains(&format!("  version: {VERSION}\n")), "{log}");
    assert!(log.contains("  binary: hatate-gtk\n"), "{log}");
    assert!(log.contains("  thread: worker-7\n"), "{log}");
    // The test harness names the thread that runs a test after the test.
    assert!(log.contains("  thread: child_panics\n"), "{log}");
    assert!(log.contains("  location: "), "{log}");
    assert!(log.contains("\nBacktrace:\n"), "{log}");
    // `RUST_BACKTRACE=0` in the child: the entry carries the frames anyway.
    assert!(
        log.lines().any(|l| l.trim_start().starts_with("0: ")),
        "{log}"
    );

    // The next start: the running marker the dead child left is found, which is what turns the
    // Crashed banner on (`launch_banner`), and a second run's mark makes this run's entries the
    // "previous run" the banner counts.
    let next = CrashMarkers::begin(&paths, 2.0);
    assert!(next.previous_run_unclean, "the panic left the marker");
    let entries = vec![hatate_core::model::Entry::new("/pictures/0.png")];
    let (banner, _) = launch_banner(next.previous_run_unclean, true, &entries, Read::Absent);
    assert!(matches!(banner, LaunchBanner::Crashed(_)), "{banner:?}");
}

#[test]
fn a_run_that_never_panics_creates_no_crash_log() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::from_lookup(|n| (n == "HOME").then(|| home.path().into())).unwrap();
    // `hatate --version` installs the hook and never arms: no file.
    crash_marker::install_panic_hook(&paths, "hatate");
    assert!(!paths.crash_log().exists());
}

fn crash_marker_mark() -> &'static str {
    hatate_core::crash_report::SESSION_START_MARK
}

/// Every binary in the workspace installs the hook in `main` (a source check, like
/// `the_launcher_and_the_lookup_name_the_same_bundled_path`: a binary's `main` cannot be called).
#[test]
fn every_binary_installs_the_hook() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (krate, call) in [
        ("hatate-cli", "install_panic_hook(&paths, \"hatate\")"),
        ("hatate-gtk", "install_panic_hook(&paths, \"hatate-gtk\")"),
        (
            "hatate-mcp",
            "crash_report::install(crash_log, \"hatate-mcp\"",
        ),
        (
            "hatate-apprun",
            "crash_report::install(crash_log, \"AppRun\"",
        ),
    ] {
        let main = std::fs::read_to_string(crates.join(krate).join("src/main.rs")).unwrap();
        assert!(main.contains(call), "{krate} does not call {call}");
    }
    // The GTK window and `batch` also open their section (the other commands only print).
    let gtk = std::fs::read_to_string(crates.join("hatate-gtk/src/main.rs")).unwrap();
    assert!(gtk.contains("arm_crash_log(&paths)"));
    let batch = std::fs::read_to_string(crates.join("hatate-cli/src/batch_cmd.rs")).unwrap();
    assert!(batch.contains("arm_crash_log(&paths)"));
}
