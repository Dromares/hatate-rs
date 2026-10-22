//! `hatate-rs.log`: the Rust app's own log, rotated like `app.log`.
//! `docs/file-formats.md` 5.2 and rule 6.4; Python: `core/applog.py`.
//!
//! The Rust app **never writes `app.log`** (DAN-1961 decision): the Python app reads the
//! tail of that file to explain an interrupted run, and two writers rotating the same
//! files would corrupt each other. The Rust log lives next to it as `hatate-rs.log`
//! (`Paths::rust_log_file`), a name nothing in spec 1.2 or 1.3 uses.
//!
//! Same rotation as `RotatingFileHandler(maxBytes=2_000_000, backupCount=3)`: before a
//! record is written, if the file size plus the record (with its newline) would reach
//! `maxBytes`, `.2` becomes `.3`, `.1` becomes `.2`, the file becomes `.1` and a new file
//! starts. Same line format, `%Y-%m-%d %H:%M:%S [LEVEL] name: message`, with one
//! deliberate difference: the stamp is **UTC** (there is no time-zone database here), and
//! the first line of every run says so.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::paths::{Paths, rotated};
use crate::timeutil;

/// `maxBytes` of `app.log` (`core/applog.py:46`).
pub const MAX_BYTES: u64 = 2_000_000;
/// `backupCount` of `app.log`.
pub const BACKUPS: u32 = 3;

/// Python's logging levels, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
            Level::Critical => "CRITICAL",
        }
    }
}

/// A size-rotated, append-only log file.
#[derive(Debug)]
pub struct RotatingLog {
    path: PathBuf,
    max_bytes: u64,
    backups: u32,
    file: Option<File>,
    size: u64,
}

impl RotatingLog {
    /// Open `hatate-rs.log` in the profile, create `CONFIG_DIR` if needed, and write the
    /// first line of the run (`Logging started, writing to <path>`, as `setup_logging`).
    pub fn start(paths: &Paths, now: f64) -> io::Result<RotatingLog> {
        let mut log = RotatingLog::open(paths.rust_log_file(), MAX_BYTES, BACKUPS)?;
        let line = format!(
            "Logging started, writing to {} (timestamps are UTC)",
            log.path.display()
        );
        log.record(now, Level::Info, "hatate", &line)?;
        Ok(log)
    }

    /// Open (append) `path`, creating its directory.
    pub fn open(path: PathBuf, max_bytes: u64, backups: u32) -> io::Result<RotatingLog> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let mut log = RotatingLog {
            path,
            max_bytes,
            backups,
            file: None,
            size: 0,
        };
        log.reopen()?;
        Ok(log)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn reopen(&mut self) -> io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.size = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    /// The line `%(asctime)s [%(levelname)s] %(name)s: %(message)s` for a record, without
    /// the newline.
    pub fn format_line(now: f64, level: Level, name: &str, message: &str) -> String {
        format!(
            "{} [{}] {}: {}",
            timeutil::format_stamp(now),
            level.as_str(),
            name,
            message
        )
    }

    /// Write one record, rotating first when it would not fit.
    pub fn record(&mut self, now: f64, level: Level, name: &str, message: &str) -> io::Result<()> {
        let mut line = Self::format_line(now, level, name, message);
        line.push('\n');
        // `shouldRollover`: `tell() + len(msg) >= maxBytes`.
        if self.max_bytes > 0 && self.size + line.len() as u64 >= self.max_bytes {
            self.roll_over()?;
        }
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => {
                self.reopen()?;
                self.file.as_mut().expect("reopened")
            }
        };
        file.write_all(line.as_bytes())?;
        self.size += line.len() as u64;
        Ok(())
    }

    /// `doRollover`.
    fn roll_over(&mut self) -> io::Result<()> {
        self.file = None;
        if self.backups > 0 {
            for i in (1..self.backups).rev() {
                let (src, dst) = (rotated(&self.path, i), rotated(&self.path, i + 1));
                if src.exists() {
                    if dst.exists() {
                        fs::remove_file(&dst)?;
                    }
                    fs::rename(&src, &dst)?;
                }
            }
            let first = rotated(&self.path, 1);
            if first.exists() {
                fs::remove_file(&first)?;
            }
            fs::rename(&self.path, &first)?;
        } else {
            fs::remove_file(&self.path)?;
        }
        self.reopen()
    }
}
