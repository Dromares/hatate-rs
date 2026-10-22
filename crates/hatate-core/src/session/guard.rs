//! Single-writer detection (rule 6.2.10, spec 3.7).
//!
//! The Python app takes no lock and writes no PID: `crash.log.running` holds only a
//! timestamp (5.3). What it does hold, for as long as it runs, is `crash.log` open: the
//! `faulthandler` stream "must not be closed or garbage collected while it might still be
//! needed" (`core/crashlog.py`). So "the Python app is running" is "another process of this
//! user has `CONFIG_DIR/crash.log` open", which `/proc/<pid>/fd` answers for a source
//! checkout and for the AppImage alike, without guessing at command lines.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::Paths;

/// Where to look and which process is us.
#[derive(Debug, Clone)]
pub struct GuardConfig {
    /// The `proc` filesystem. Tests point it at a fake tree; everything else uses `/proc`.
    pub proc_root: PathBuf,
    /// A process that holds `crash.log` open but is this one never counts.
    pub own_pid: u32,
}

impl Default for GuardConfig {
    fn default() -> GuardConfig {
        GuardConfig {
            proc_root: PathBuf::from("/proc"),
            own_pid: std::process::id(),
        }
    }
}

/// The Python app is running against this profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonAppRunning {
    pub pid: u32,
}

impl fmt::Display for PythonAppRunning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the Hatate Python app is running (process {}, it holds crash.log open), so the \
             session will not be written: each app keeps the list in memory and every save \
             overwrites the other's rows. Quit the Python app first",
            self.pid
        )
    }
}

impl std::error::Error for PythonAppRunning {}

/// The pid of another process that has this profile's `crash.log` open, if there is one.
/// A process that cannot be inspected (it exited, or belongs to another user) is skipped:
/// another user's process cannot be holding this user's profile.
pub fn find_python_app(paths: &Paths, config: &GuardConfig) -> Option<u32> {
    let target = fs::canonicalize(paths.crash_log()).ok()?;
    let mut found: Option<u32> = None;
    for entry in fs::read_dir(&config.proc_root).ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == config.own_pid {
            continue;
        }
        if holds_open(&entry.path().join("fd"), &target) {
            found = Some(found.map_or(pid, |p| p.min(pid)));
        }
    }
    found
}

fn holds_open(fd_dir: &Path, target: &Path) -> bool {
    let Ok(fds) = fs::read_dir(fd_dir) else {
        return false;
    };
    fds.flatten()
        .any(|fd| fs::read_link(fd.path()).is_ok_and(|link| link == target))
}

/// `Ok` when no other process holds the profile open for the Python app.
pub fn ensure_single_writer(paths: &Paths, config: &GuardConfig) -> Result<(), PythonAppRunning> {
    match find_python_app(paths, config) {
        Some(pid) => Err(PythonAppRunning { pid }),
        None => Ok(()),
    }
}
