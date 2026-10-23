//! The shipped `hatate` binary, run as a process in a throw-away profile. Nothing here may reach
//! the network: the folders are empty or already in the saved session, so no search is made.
//! (The binary has no way to point an engine at a local server, which is the rule; the searches
//! themselves are tested through the library in `hatate-core/tests/batch_run.rs`.)

use std::path::Path;
use std::process::{Command, Output};

use hatate_core::model::{Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::session::{LoadOptions, SessionStore};

fn hatate(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hatate"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .output()
        .expect("run hatate")
}

fn paths_in(home: &Path) -> Paths {
    let home = home.as_os_str().to_owned();
    Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn no_arguments_prints_the_work_in_progress_banner() {
    let home = tempfile::tempdir().unwrap();
    let out = hatate(home.path(), &[]);
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("work-in-progress"));
}

#[test]
fn help_says_batch_is_experimental() {
    let home = tempfile::tempdir().unwrap();
    let out = hatate(home.path(), &["--help"]);
    assert!(out.status.success());
    let help = text(&out.stdout);
    assert!(help.contains("(experimental)"), "{help}");
    assert!(
        help.contains("Hydrus auto-import is not in this build"),
        "{help}"
    );
}

#[test]
fn usage_errors_exit_2_and_touch_no_profile() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        &["batch"][..],
        &["batch", "a", "b"],
        &["batch", "--session", "x"],
        &["nope"],
    ] {
        let out = hatate(home.path(), args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(text(&out.stderr).contains("usage:"));
    }
    assert!(
        std::fs::read_dir(home.path()).unwrap().next().is_none(),
        "nothing was created"
    );
}

#[test]
fn a_folder_that_is_not_one_is_exit_1() {
    let home = tempfile::tempdir().unwrap();
    let out = hatate(home.path(), &["batch", "/definitely/not/here"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("is not a folder"));
}

#[test]
fn an_empty_folder_finishes_cleanly_and_marks_a_clean_exit() {
    let home = tempfile::tempdir().unwrap();
    let pics = tempfile::tempdir().unwrap();
    let out = hatate(home.path(), &["batch", pics.path().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("0 found, 0 added"));
    let paths = paths_in(home.path());
    assert!(paths.last_clean_exit().exists());
    assert!(
        !paths.crash_running_marker().exists(),
        "the running marker is gone at a clean exit"
    );
}

/// A saved session that already holds every file of the folder: the run adds nothing, searches
/// nothing (so makes no request), and leaves the session as it was.
#[test]
fn a_folder_already_in_the_session_is_all_duplicates_and_makes_no_request() {
    let home = tempfile::tempdir().unwrap();
    let pics = tempfile::tempdir().unwrap();
    let paths = paths_in(home.path());
    let mut entries = Vec::new();
    for (i, name) in ["a.png", "b.png"].iter().enumerate() {
        let p = pics.path().join(name);
        image::RgbImage::from_fn(4, 3, |x, _| image::Rgb([x as u8, i as u8, 9]))
            .save(&p)
            .unwrap();
        let mut e = Entry::new(p.to_string_lossy());
        e.status = MatchStatus::NotFound;
        e.hydrus_hash = Some(hatate_core::hydrus_tag_lookup::hash_file(&p));
        entries.push(e);
    }
    let writer = SessionStore::new(&paths).open_writer().unwrap();
    writer.save(&entries, 1_792_725_790.0).unwrap();

    let out = hatate(home.path(), &["batch", pics.path().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let err = text(&out.stderr);
    assert!(
        err.contains("continuing the saved session (2 entries)"),
        "{err}"
    );
    assert!(
        err.contains("2 found, 0 added, 2 duplicate(s), 0 search(es)"),
        "{err}"
    );

    let after = SessionStore::new(&paths)
        .load(&LoadOptions::at(1_792_725_790.0))
        .entries;
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|e| e.status == MatchStatus::NotFound));
}

#[test]
fn two_runs_in_a_row_both_finish_cleanly() {
    let home = tempfile::tempdir().unwrap();
    let pics = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let out = hatate(home.path(), &["batch", pics.path().to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(0));
        assert!(
            !text(&out.stderr).contains("interrupted"),
            "the first run left a clean exit"
        );
    }
}

/// SIGTERM while the hash pass is running: the run stops between files, says so, exits 143 and
/// leaves the saved session as it was. (Stopping in the middle of a search, with the session
/// saved, is `batch_run.rs`: a real search needs the network.)
#[test]
fn sigterm_during_hashing_stops_the_run_with_143() {
    let home = tempfile::tempdir().unwrap();
    let pics = tempfile::tempdir().unwrap();
    // Sparse files: 100 MiB of zeros each, so a hash takes long enough to be interrupted.
    for i in 0..60 {
        let f = std::fs::File::create(pics.path().join(format!("f{i:02}.png"))).unwrap();
        f.set_len(100 * 1024 * 1024).unwrap();
    }
    let child = Command::new(env!("CARGO_BIN_EXE_hatate"))
        .args(["batch", pics.path().to_str().unwrap()])
        .env_clear()
        .env("HOME", home.path())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(700));
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let out = child.wait_with_output().unwrap();
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(143), "{err}");
    assert!(err.contains("stopping"), "{err}");
    assert!(
        !SessionStore::new(&paths_in(home.path())).has_saved_session(),
        "a stop before anything was added writes no session"
    );
}
