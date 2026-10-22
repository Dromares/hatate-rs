//! The crash-detection protocol: `crash.log.running` and `last_clean_exit`.
//! `docs/file-formats.md` 5.3, rule 6.4; Python: `core/crashlog.py:52-127`, `main.py:47-48`.
//!
//! The Rust app takes part (DAN-1961 decision): it drops `crash.log.running` at startup,
//! after looking for the previous run's, and removes it at the very end of a clean exit,
//! writing `last_clean_exit`. A clean Rust exit therefore never leaves the "interrupted
//! run" banner for the next Python launch, and a crashed Rust run shows it, as a crashed
//! Python run does.
//!
//! There is deliberately no `Drop` that marks a clean exit: a panic unwinds through
//! `Drop`, and calling a crash "clean" is exactly the bug the protocol exists to catch.
//! The app calls [`RunSession::finish_clean`] explicitly, after the last thing that can
//! still fail (saving the session, writing settings). `crash.log` (faulthandler output)
//! is Python's and is not written.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::config::pyjson;
use crate::fsutil;
use crate::paths::Paths;
use crate::pycompat;

/// What startup found, and the handle for the end of the run.
#[derive(Debug)]
pub struct Started {
    /// The previous run (either app) left its marker: it crashed, was killed, or lost power.
    /// Turns on the "interrupted run" banner (`check_unclean_shutdown`).
    pub previous_run_unclean: bool,
    /// This run's marker could not be written; the run goes on without crash detection
    /// (Python logs a warning and carries on).
    pub marker_error: Option<io::Error>,
    pub session: RunSession,
}

/// A running app. Consume it with [`finish_clean`](RunSession::finish_clean) on a clean exit;
/// dropping it does nothing.
#[derive(Debug)]
pub struct RunSession {
    markers: CrashMarkers,
}

impl RunSession {
    /// `mark_clean_shutdown`: remove the running marker, then record the exit time. Both
    /// steps are tried; the first error is returned. If removing the marker fails the next
    /// launch will show the banner, so the caller should say so in the log.
    pub fn finish_clean(self, now: f64) -> io::Result<()> {
        self.markers.mark_clean_shutdown(now)
    }
}

/// The two marker files of a profile.
#[derive(Debug, Clone)]
pub struct CrashMarkers {
    running: PathBuf,
    clean_exit: PathBuf,
}

impl CrashMarkers {
    pub fn new(paths: &Paths) -> CrashMarkers {
        CrashMarkers {
            running: paths.crash_running_marker(),
            clean_exit: paths.last_clean_exit(),
        }
    }

    /// Startup, in Python's order: check for the previous marker, *then* write this run's.
    /// Writing first would make every run look interrupted.
    pub fn begin(paths: &Paths, now: f64) -> Started {
        let markers = CrashMarkers::new(paths);
        let previous_run_unclean = markers.check_unclean_shutdown();
        let marker_error = markers.mark_running(now).err();
        Started {
            previous_run_unclean,
            marker_error,
            session: RunSession { markers },
        }
    }

    /// `check_unclean_shutdown`: the marker exists.
    pub fn check_unclean_shutdown(&self) -> bool {
        self.running.exists()
    }

    /// `mark_running`: the marker holds `str(time.time())`. Creates `CONFIG_DIR` if needed.
    pub fn mark_running(&self, now: f64) -> io::Result<()> {
        if let Some(dir) = self.running.parent() {
            fs::create_dir_all(dir)?;
        }
        fsutil::write_atomic(&self.running, stamp(now).as_bytes())
    }

    /// `mark_clean_shutdown`. A missing marker is fine.
    pub fn mark_clean_shutdown(&self, now: f64) -> io::Result<()> {
        let removed = match fs::remove_file(&self.running) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
        let recorded = fs::create_dir_all(self.clean_exit.parent().unwrap_or(".".as_ref()))
            .and_then(|()| fsutil::write_atomic(&self.clean_exit, stamp(now).as_bytes()));
        removed.and(recorded)
    }

    /// `last_clean_exit_age_seconds`: how long ago the last clean exit finished, or `None`
    /// if none was recorded or the file is not a number.
    pub fn last_clean_exit_age(&self, now: f64) -> Option<f64> {
        let text = fs::read_to_string(&self.clean_exit).ok()?;
        let stamp = pycompat::float(&serde_json::Value::String(text.trim().to_owned()))?;
        Some((now - stamp).max(0.0))
    }
}

/// `str(time.time())`: Python's float repr.
fn stamp(now: f64) -> String {
    let mut s = String::new();
    pyjson::float(&mut s, now);
    s
}
