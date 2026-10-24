//! The persisted stop record (DAN-2224, Cloud's ruling on `phase6-calls` call 2): when a run
//! ends `Ending::Stopped`, the Rust app notes it in `hatate-rs.json`, so the next launch can say
//! "Run stopped, everything is saved" to a user who comes back to unfinished work.
//!
//! **Rust-only.** Python never writes or reads it, and nothing here touches `session.db`,
//! `session.json` or `config.json`. The record is a *trigger*: it says a stop happened and when.
//! The counts a banner shows come from the restored session, never from the record.
//!
//! Rules (DAN-2224 decision 2):
//! - written on `Ending::Stopped` only, atomically; a failed write is non-fatal and the caller
//!   logs it and falls back to Python's `Restored N image(s) from your last session`;
//! - cleared when any run starts, on Resume Queue and on Dismiss;
//! - the crash marker wins: with `previous_run_unclean` the state is Crashed (or Failed-empty)
//!   whatever the record says, and the record is cleared;
//! - zero `NotSearched` rows in the restored session, no session file, or an unreadable one: no
//!   banner, the record is cleared;
//! - a malformed or unknown-version record is ignored and cleared; it never gives a banner or an
//!   error;
//! - an in-session Stop shows no banner, and `Failed` is never persisted.
//!
//! The file is a JSON object. This module owns the `stop_record` key and keeps every other key it
//! finds. A file that is not a JSON object is set aside as `hatate-rs.json.unreadable` (the way
//! `config.json` is) and the record starts afresh.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::fsutil;
use crate::model::{Entry, MatchStatus};
use crate::paths::Paths;

/// The object key inside `hatate-rs.json`.
pub const KEY: &str = "stop_record";

/// The only version this build writes and reads.
pub const VERSION: i64 = 1;

/// What a stop leaves behind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopRecord {
    /// When the run stopped: epoch seconds (`time.time()`). Used for the age and nothing else.
    pub at: f64,
    /// How many rows were still `NotSearched` then. Kept for a person reading the file; the
    /// banner counts the restored session instead.
    pub unsearched: i64,
}

/// What reading the file found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Read {
    /// No file, or no record in it.
    Absent,
    Valid(StopRecord),
    /// Something is under the key but it is not a version-1 record.
    Malformed,
}

/// `hatate-rs.json`, for the one key this module owns.
#[derive(Debug, Clone)]
pub struct StopRecordFile {
    path: PathBuf,
}

impl StopRecordFile {
    pub fn new(paths: &Paths) -> StopRecordFile {
        StopRecordFile {
            path: paths.rust_settings_file(),
        }
    }

    /// The file's path, for a test that checks nothing else changed.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Reads the record. A file that cannot be read or parsed is `Absent`: it gives no banner and
    /// the next write replaces it.
    pub fn read(&self) -> Read {
        let Some(doc) = self.document() else {
            return Read::Absent;
        };
        match doc.get(KEY) {
            None => Read::Absent,
            Some(v) => parse(v).map_or(Read::Malformed, Read::Valid),
        }
    }

    /// Writes the record for a stop. Atomic (temp file and rename). Other keys are kept.
    pub fn write(&self, record: &StopRecord) -> io::Result<()> {
        let mut doc = self.document_or_set_aside();
        doc.insert(
            KEY.to_owned(),
            json!({
                "version": VERSION,
                "ended": "stopped",
                "at": record.at,
                "unsearched": record.unsearched,
            }),
        );
        self.store(&doc)
    }

    /// Removes the record. A missing record or file is fine. If it was the only key the file goes
    /// too.
    pub fn clear(&self) -> io::Result<()> {
        let Some(mut doc) = self.document() else {
            return Ok(());
        };
        if doc.remove(KEY).is_none() {
            return Ok(());
        }
        if doc.is_empty() {
            return match fs::remove_file(&self.path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        self.store(&doc)
    }

    fn document(&self) -> Option<Map<String, Value>> {
        let bytes = fs::read(&self.path).ok()?;
        match serde_json::from_slice::<Value>(&bytes).ok()? {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    fn document_or_set_aside(&self) -> Map<String, Value> {
        if let Some(doc) = self.document() {
            return doc;
        }
        if self.path.exists() {
            let mut aside = self.path.clone().into_os_string();
            aside.push(".unreadable");
            let _ = fs::rename(&self.path, PathBuf::from(aside));
        }
        Map::new()
    }

    fn store(&self, doc: &Map<String, Value>) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string_pretty(doc).map_err(io::Error::other)?;
        text.push('\n');
        fsutil::write_atomic(&self.path, text.as_bytes())
    }

    /// Reads, decides and clears, for a launch. `previous_run_unclean` is the crash marker found
    /// at start-up, `had_session_file` whether a session existed on disk before the load, and
    /// `entries` what the load gave. A failed clear is returned in `clear_error` and is not
    /// fatal.
    pub fn resolve(
        &self,
        previous_run_unclean: bool,
        had_session_file: bool,
        entries: &[Entry],
    ) -> Resolved {
        let record = self.read();
        let (banner, clear) =
            launch_banner(previous_run_unclean, had_session_file, entries, record);
        let clear_error = if clear { self.clear().err() } else { None };
        Resolved {
            banner,
            clear_error,
        }
    }
}

/// What [`StopRecordFile::resolve`] decided.
#[derive(Debug)]
pub struct Resolved {
    pub banner: LaunchBanner,
    pub clear_error: Option<io::Error>,
}

fn parse(value: &Value) -> Option<StopRecord> {
    let o = value.as_object()?;
    if o.get("version")?.as_i64()? != VERSION || o.get("ended")?.as_str()? != "stopped" {
        return None;
    }
    let at = o.get("at")?.as_f64()?;
    let unsearched = o.get("unsearched")?.as_i64()?;
    (at.is_finite() && unsearched >= 0).then_some(StopRecord { at, unsearched })
}

/// The counts a launch banner states, taken from the restored rows (`_interrupted_prose`,
/// `gui/main_window.py:198-224`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreCounts {
    pub total: usize,
    /// Sent and Hydrus confirmed it holds the file.
    pub sent: usize,
    /// Sent, nothing has confirmed it landed.
    pub queued: usize,
    /// Decided and waiting for the user: `needs_review` and Good or Poor.
    pub waiting: usize,
    /// Still `NotSearched`.
    pub unreached: usize,
}

impl RestoreCounts {
    pub fn of(entries: &[Entry]) -> RestoreCounts {
        let mut c = RestoreCounts {
            total: entries.len(),
            sent: 0,
            queued: 0,
            waiting: 0,
            unreached: 0,
        };
        for e in entries {
            if e.sent_to_hydrus && e.hydrus_import_confirmed {
                c.sent += 1;
            }
            if e.sent_to_hydrus && !e.hydrus_import_confirmed {
                c.queued += 1;
            }
            if crate::queue::needs_review(e)
                && matches!(e.status, MatchStatus::Good | MatchStatus::Poor)
            {
                c.waiting += 1;
            }
            if e.status == MatchStatus::NotSearched {
                c.unreached += 1;
            }
        }
        c
    }
}

fn is_are(n: usize) -> &'static str {
    if n == 1 { "is" } else { "are" }
}

/// `_plural(count, "image")`.
fn images(count: usize) -> String {
    format!("{count} image{}", if count == 1 { "" } else { "s" })
}

/// The counts clause of a launch banner, shared by Crashed and Interrupted so there is one
/// function: `3 of 5 images are already sent to Hydrus, 1 is still queued with Hydrus, 2 are
/// decided and waiting for your review, and 0 were never reached`. A queued count of zero is left
/// out. No full stop.
pub fn counts_clause(c: &RestoreCounts) -> String {
    let mut parts = vec![format!(
        "{} of {} {} already sent to Hydrus",
        c.sent,
        images(c.total),
        if c.total == 1 { "is" } else { "are" }
    )];
    if c.queued > 0 {
        parts.push(format!(
            "{} {} still queued with Hydrus",
            c.queued,
            is_are(c.queued)
        ));
    }
    parts.push(format!(
        "{} {} decided and waiting for your review",
        c.waiting,
        is_are(c.waiting)
    ));
    let last = format!(
        "{} {} never reached",
        c.unreached,
        if c.unreached == 1 { "was" } else { "were" }
    );
    format!("{}, and {last}", parts.join(", "))
}

/// The Crashed banner's body: Python's `_interrupted_prose`, byte for byte.
pub fn crashed_body(c: &RestoreCounts) -> String {
    format!(
        "Hatate did not exit cleanly during this run. Everything up to the last autosave is safe. {}.",
        counts_clause(c)
    )
}

/// The Interrupted banner's body (`phase6-calls` call 2b): new copy on the Crashed counts clause.
pub fn interrupted_body(c: &RestoreCounts) -> String {
    format!(
        "Hatate was stopped during this run. Everything it had done is saved. {}.",
        counts_clause(c)
    )
}

/// What a launch shows above the table.
#[derive(Debug, Clone, PartialEq)]
pub enum LaunchBanner {
    /// Nothing: Python's `Restored N image(s) from your last session` status line, or an empty
    /// start.
    None,
    /// `previous_run_unclean` and at least one row restored.
    Crashed(RestoreCounts),
    /// `previous_run_unclean`, a session file, and zero rows.
    FailedEmpty,
    /// A valid stop record, no crash marker, and at least one row still unsearched. `at` is for
    /// the age ("Stopped 3h ago"); the counts are the restored session's.
    Interrupted { counts: RestoreCounts, at: f64 },
}

impl LaunchBanner {
    /// The run strip's state for this banner.
    pub fn run_state(&self) -> crate::run_state::RunState {
        use crate::run_state::{RunEnd, RunState};
        match self {
            LaunchBanner::None => RunState::Idle,
            LaunchBanner::Crashed(_) => RunState::Ended(RunEnd::Crashed),
            LaunchBanner::FailedEmpty => RunState::Ended(RunEnd::FailedEmpty),
            LaunchBanner::Interrupted { .. } => RunState::Ended(RunEnd::Interrupted),
        }
    }
}

/// The launch decision, as a pure function. Returns the banner and whether the stop record must
/// be cleared now.
///
/// - `previous_run_unclean` wins over the record (decision 2, point 4); the record is cleared.
/// - Unclean and a session file with no rows: `FailedEmpty` (`main_window.py:1414-1420`).
/// - Unclean and no session file: nothing (Python shows nothing, and so does this).
/// - Clean: a valid record plus at least one `NotSearched` row is `Interrupted`. Any other clean
///   launch has no banner, and a record that is there (valid or not) is cleared.
pub fn launch_banner(
    previous_run_unclean: bool,
    had_session_file: bool,
    entries: &[Entry],
    record: Read,
) -> (LaunchBanner, bool) {
    let has_record = !matches!(record, Read::Absent);
    if previous_run_unclean {
        let banner = if !entries.is_empty() {
            LaunchBanner::Crashed(RestoreCounts::of(entries))
        } else if had_session_file {
            LaunchBanner::FailedEmpty
        } else {
            LaunchBanner::None
        };
        return (banner, has_record);
    }
    if let Read::Valid(rec) = record {
        let counts = RestoreCounts::of(entries);
        if counts.unreached > 0 {
            return (LaunchBanner::Interrupted { counts, at: rec.at }, false);
        }
    }
    (LaunchBanner::None, has_record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(statuses: &[MatchStatus]) -> Vec<Entry> {
        statuses
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut e = Entry::new(format!("/pictures/{i}.png"));
                e.status = *s;
                e
            })
            .collect()
    }

    #[test]
    fn the_counts_clause_is_pythons_for_one_and_for_two() {
        let mut one = rows(&[MatchStatus::NotSearched]);
        let c = RestoreCounts::of(&one);
        assert_eq!(
            counts_clause(&c),
            "0 of 1 image is already sent to Hydrus, 0 are decided and waiting for your review, and 1 was never reached"
        );
        one[0].status = MatchStatus::Good;
        one[0].sent_to_hydrus = true;
        one[0].hydrus_import_confirmed = false;
        let c = RestoreCounts::of(&one);
        assert_eq!(
            counts_clause(&c),
            "0 of 1 image is already sent to Hydrus, 1 is still queued with Hydrus, 0 are decided and waiting for your review, and 0 were never reached"
        );
        let two = rows(&[MatchStatus::Good, MatchStatus::NotSearched]);
        let c = RestoreCounts::of(&two);
        assert_eq!(
            crashed_body(&c),
            "Hatate did not exit cleanly during this run. Everything up to the last autosave is safe. 0 of 2 images are already sent to Hydrus, 1 is decided and waiting for your review, and 1 was never reached."
        );
        assert_eq!(
            interrupted_body(&c),
            "Hatate was stopped during this run. Everything it had done is saved. 0 of 2 images are already sent to Hydrus, 1 is decided and waiting for your review, and 1 was never reached."
        );
    }

    #[test]
    fn counts_are_taken_from_the_rows_as_python_counts_them() {
        let mut e = rows(&[
            MatchStatus::Good,
            MatchStatus::Poor,
            MatchStatus::NotFound,
            MatchStatus::NotSearched,
            MatchStatus::Good,
        ]);
        e[0].sent_to_hydrus = true;
        e[0].hydrus_import_confirmed = true;
        e[1].reviewed = true; // decided: not waiting
        e[4].sent_to_hydrus = true; // queued; sent is a decision, so not waiting either
        let c = RestoreCounts::of(&e);
        assert_eq!(
            c,
            RestoreCounts {
                total: 5,
                sent: 1,
                queued: 1,
                waiting: 0,
                unreached: 1
            }
        );
    }

    #[test]
    fn launch_rules_follow_the_ruling() {
        let unsearched = rows(&[MatchStatus::Good, MatchStatus::NotSearched]);
        let done = rows(&[MatchStatus::Good, MatchStatus::NotFound]);
        let rec = Read::Valid(StopRecord {
            at: 100.0,
            unsearched: 1,
        });
        let counts = RestoreCounts::of(&unsearched);

        // Clean launch, valid record, an unsearched row: Interrupted, record kept.
        assert_eq!(
            launch_banner(false, true, &unsearched, rec),
            (LaunchBanner::Interrupted { counts, at: 100.0 }, false)
        );
        // The marker wins over the record, and the record is cleared (point 4).
        assert_eq!(
            launch_banner(true, true, &unsearched, rec),
            (LaunchBanner::Crashed(counts), true)
        );
        // Point 5: no unsearched row, no file, or an unreadable session: no banner, cleared.
        assert_eq!(
            launch_banner(false, true, &done, rec),
            (LaunchBanner::None, true)
        );
        assert_eq!(
            launch_banner(false, false, &[], rec),
            (LaunchBanner::None, true)
        );
        assert_eq!(
            launch_banner(false, true, &[], rec),
            (LaunchBanner::None, true)
        );
        // Point 6: a malformed record never gives a banner, and is cleared.
        assert_eq!(
            launch_banner(false, true, &unsearched, Read::Malformed),
            (LaunchBanner::None, true)
        );
        // No record: nothing, nothing to clear.
        assert_eq!(
            launch_banner(false, true, &unsearched, Read::Absent),
            (LaunchBanner::None, false)
        );
        // B1: the marker with a session file and no rows is Failed-empty; with no file, nothing.
        assert_eq!(
            launch_banner(true, true, &[], Read::Absent),
            (LaunchBanner::FailedEmpty, false)
        );
        assert_eq!(
            launch_banner(true, false, &[], Read::Absent),
            (LaunchBanner::None, false)
        );
    }

    #[test]
    fn a_record_round_trips_and_other_keys_survive() {
        let dir = tempfile::tempdir().unwrap();
        let file = StopRecordFile {
            path: dir.path().join("hatate-rs.json"),
        };
        assert_eq!(file.read(), Read::Absent);
        std::fs::write(file.path(), "{\"other\": {\"a\": 1}}\n").unwrap();
        file.write(&StopRecord {
            at: 1234.5,
            unsearched: 7,
        })
        .unwrap();
        assert_eq!(
            file.read(),
            Read::Valid(StopRecord {
                at: 1234.5,
                unsearched: 7
            })
        );
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(text.contains("\"other\"") && text.contains("\"ended\": \"stopped\""));
        file.clear().unwrap();
        assert_eq!(file.read(), Read::Absent);
        assert!(
            std::fs::read_to_string(file.path())
                .unwrap()
                .contains("\"other\"")
        );
        // Clearing the only key removes the file.
        std::fs::remove_file(file.path()).unwrap();
        file.write(&StopRecord {
            at: 1.0,
            unsearched: 0,
        })
        .unwrap();
        file.clear().unwrap();
        file.clear().unwrap();
        assert!(!file.path().exists());
    }

    #[test]
    fn malformed_and_unknown_version_records_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = StopRecordFile {
            path: dir.path().join("hatate-rs.json"),
        };
        for bad in [
            r#"{"stop_record": 5}"#,
            r#"{"stop_record": {"version": 2, "ended": "stopped", "at": 1.0, "unsearched": 1}}"#,
            r#"{"stop_record": {"version": 1, "ended": "crashed", "at": 1.0, "unsearched": 1}}"#,
            r#"{"stop_record": {"version": 1, "ended": "stopped", "at": "x", "unsearched": 1}}"#,
            r#"{"stop_record": {"version": 1, "ended": "stopped", "at": 1.0, "unsearched": -1}}"#,
            r#"{"stop_record": {"version": 1, "ended": "stopped", "at": 1.0}}"#,
        ] {
            std::fs::write(file.path(), bad).unwrap();
            assert_eq!(file.read(), Read::Malformed, "{bad}");
            let r = file.resolve(false, true, &rows(&[MatchStatus::NotSearched]));
            assert_eq!(r.banner, LaunchBanner::None, "{bad}");
            assert!(r.clear_error.is_none());
            assert_eq!(file.read(), Read::Absent, "cleared: {bad}");
        }
    }

    #[test]
    fn a_file_that_is_not_json_is_set_aside_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let file = StopRecordFile {
            path: dir.path().join("hatate-rs.json"),
        };
        std::fs::write(file.path(), "not json at all").unwrap();
        assert_eq!(file.read(), Read::Absent);
        file.write(&StopRecord {
            at: 2.0,
            unsearched: 3,
        })
        .unwrap();
        assert_eq!(
            file.read(),
            Read::Valid(StopRecord {
                at: 2.0,
                unsearched: 3
            })
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hatate-rs.json.unreadable")).unwrap(),
            "not json at all"
        );
    }
}
