//! What a launch shows of the last run (DAN-2216, Cloud's ruling DAN-2224): the real crash marker,
//! the real session files and the real `hatate-rs.json`, in a temp profile. The pure rules are in
//! `stop_record`'s unit tests; these are the ones that need the files.

mod common;

use common::Sandbox;
use hatate_core::crash_marker::CrashMarkers;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::run_state::{RunEnd, RunState};
use hatate_core::session::{LoadOptions, SessionStore};
use hatate_core::stop_record::{LaunchBanner, Read, RestoreCounts, StopRecord, StopRecordFile};

const NOW: f64 = 1_792_725_790.0;

fn rows(statuses: &[MatchStatus]) -> Vec<Entry> {
    statuses
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut e = Entry::new(format!("/pictures/{i}.png"));
            e.status = *s;
            e.hydrus_hash = Some(format!("{i:064x}"));
            e
        })
        .collect()
}

fn save(sb: &Sandbox, entries: &[Entry]) {
    SessionStore::new(&sb.paths)
        .open_writer()
        .unwrap()
        .save(entries, NOW)
        .unwrap();
}

/// What `_restore_saved_session` reads before it decides: whether a session file exists, and the
/// rows the load gives (a marker left behind resets rows that were mid-search).
fn restore(sb: &Sandbox, unclean: bool) -> (bool, Vec<Entry>) {
    let store = SessionStore::new(&sb.paths);
    let had = store.has_saved_session();
    let mut opts = LoadOptions::at(NOW);
    opts.unclean_shutdown = unclean;
    (had, store.load(&opts).entries)
}

/// A real marker: the first launch drops it and never exits cleanly; the second finds it.
fn crashed_last_time(sb: &Sandbox) -> bool {
    let first = CrashMarkers::begin(&sb.paths, NOW - 3600.0);
    assert!(!first.previous_run_unclean);
    drop(first);
    let second = CrashMarkers::begin(&sb.paths, NOW);
    second.previous_run_unclean
}

#[test]
fn a_clean_launch_after_a_stop_with_work_left_is_interrupted_and_the_counts_are_the_sessions() {
    let sb = Sandbox::new();
    save(
        &sb,
        &rows(&[
            MatchStatus::Good,
            MatchStatus::NotSearched,
            MatchStatus::NotSearched,
        ]),
    );
    let file = StopRecordFile::new(&sb.paths);
    // The record says 99; the banner counts the session, which has two.
    file.write(&StopRecord {
        at: NOW - 7200.0,
        unsearched: 99,
    })
    .unwrap();
    let (had, entries) = restore(&sb, false);
    let resolved = file.resolve(false, had, &entries);
    let LaunchBanner::Interrupted { counts, at } = &resolved.banner else {
        panic!("{:?}", resolved.banner);
    };
    assert_eq!(counts.unreached, 2, "from the session, not the record");
    assert_eq!(*at, NOW - 7200.0, "the record is only the age");
    assert_eq!(
        resolved.banner.run_state(),
        RunState::Ended(RunEnd::Interrupted)
    );
    assert!(
        matches!(file.read(), Read::Valid(_)),
        "kept until Resume Queue or Dismiss"
    );
    assert!(resolved.clear_error.is_none());
}

#[test]
fn the_marker_wins_over_the_record_and_the_record_is_cleared() {
    let sb = Sandbox::new();
    save(&sb, &rows(&[MatchStatus::Good, MatchStatus::NotSearched]));
    let file = StopRecordFile::new(&sb.paths);
    file.write(&StopRecord {
        at: NOW - 60.0,
        unsearched: 1,
    })
    .unwrap();
    let unclean = crashed_last_time(&sb);
    assert!(unclean);
    let (had, entries) = restore(&sb, unclean);
    let resolved = file.resolve(unclean, had, &entries);
    assert_eq!(
        resolved.banner,
        LaunchBanner::Crashed(RestoreCounts::of(&entries))
    );
    assert_eq!(
        resolved.banner.run_state(),
        RunState::Ended(RunEnd::Crashed)
    );
    assert_eq!(file.read(), Read::Absent);
}

#[test]
fn a_marker_and_a_session_that_will_not_load_is_failed_empty_and_with_no_session_it_is_nothing() {
    let sb = Sandbox::new();
    let unclean = crashed_last_time(&sb);
    // No session file at all: Python shows nothing, and so does this.
    let (had, entries) = restore(&sb, unclean);
    assert!(!had && entries.is_empty());
    let file = StopRecordFile::new(&sb.paths);
    assert_eq!(
        file.resolve(unclean, had, &entries).banner,
        LaunchBanner::None
    );

    // A session file that cannot be read: "empty or could not be read".
    std::fs::create_dir_all(sb.paths.config_dir()).unwrap();
    std::fs::write(sb.paths.session_db(), b"this is not a database").unwrap();
    let (had, entries) = restore(&sb, unclean);
    assert!(had && entries.is_empty());
    let resolved = file.resolve(unclean, had, &entries);
    assert_eq!(resolved.banner, LaunchBanner::FailedEmpty);
    assert_eq!(
        resolved.banner.run_state(),
        RunState::Ended(RunEnd::FailedEmpty)
    );
}

#[test]
fn a_record_with_nothing_left_to_search_or_no_session_shows_no_banner_and_is_cleared() {
    let sb = Sandbox::new();
    let file = StopRecordFile::new(&sb.paths);

    // Every row searched since: nothing to resume.
    save(&sb, &rows(&[MatchStatus::Good, MatchStatus::NotFound]));
    file.write(&StopRecord {
        at: NOW - 10.0,
        unsearched: 1,
    })
    .unwrap();
    let (had, entries) = restore(&sb, false);
    assert_eq!(
        file.resolve(false, had, &entries).banner,
        LaunchBanner::None
    );
    assert_eq!(file.read(), Read::Absent);

    // The session is gone.
    std::fs::remove_file(sb.paths.session_db()).unwrap();
    file.write(&StopRecord {
        at: NOW - 10.0,
        unsearched: 1,
    })
    .unwrap();
    let (had, entries) = restore(&sb, false);
    assert!(!had);
    assert_eq!(
        file.resolve(false, had, &entries).banner,
        LaunchBanner::None
    );
    assert_eq!(file.read(), Read::Absent);

    // The session is unreadable.
    std::fs::write(sb.paths.session_db(), b"garbage").unwrap();
    file.write(&StopRecord {
        at: NOW - 10.0,
        unsearched: 1,
    })
    .unwrap();
    let (had, entries) = restore(&sb, false);
    assert!(had && entries.is_empty());
    assert_eq!(
        file.resolve(false, had, &entries).banner,
        LaunchBanner::None
    );
    assert_eq!(file.read(), Read::Absent);
}

#[test]
fn a_malformed_record_is_ignored_and_cleared_without_an_error() {
    let sb = Sandbox::new();
    save(&sb, &rows(&[MatchStatus::NotSearched]));
    let file = StopRecordFile::new(&sb.paths);
    std::fs::write(
        file.path(),
        r#"{"stop_record": {"version": 7}, "keep": true}"#,
    )
    .unwrap();
    let (had, entries) = restore(&sb, false);
    let resolved = file.resolve(false, had, &entries);
    assert_eq!(resolved.banner, LaunchBanner::None);
    assert!(resolved.clear_error.is_none());
    let left = std::fs::read_to_string(file.path()).unwrap();
    assert!(
        left.contains("keep") && !left.contains("stop_record"),
        "{left}"
    );
}

#[test]
fn no_launch_rule_touches_the_python_files() {
    let sb = Sandbox::new();
    save(&sb, &rows(&[MatchStatus::Good, MatchStatus::NotSearched]));
    std::fs::write(sb.paths.config_file(), b"{}\n").unwrap();
    let python = |sb: &Sandbox| {
        [sb.paths.session_db(), sb.paths.config_file()].map(|p| std::fs::read(p).unwrap())
    };
    let before = python(&sb);
    let file = StopRecordFile::new(&sb.paths);
    file.write(&StopRecord {
        at: NOW,
        unsearched: 1,
    })
    .unwrap();
    let (had, entries) = restore(&sb, false);
    let _ = file.resolve(false, had, &entries);
    file.clear().unwrap();
    assert_eq!(python(&sb), before);
}
