//! The session: the working list that survives a restart.
//! Contract: `docs/file-formats.md` section 3 and rules 6.2.
//!
//! - [`SessionStore`] reads. It never runs a write statement, so it is safe next to the
//!   Python app.
//! - [`SessionWriter`] writes `session.db`, migrates the legacy `session.json` and retires
//!   its backup. It can only be had while the Python app is not detectably running
//!   (rule 6.2.10, [`guard`]).
//! - [`named`] reads and writes the standalone JSON files of "Save Session As".
//!
//! Every function that depends on the time takes it as a parameter (epoch seconds), so the
//! 30-day rules of 3.4.1 and 3.6 are testable without a clock.

mod codec;
mod db;
pub mod guard;
pub mod named;
mod report;
mod tolerant;

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub use guard::{GuardConfig, PythonAppRunning};
pub use report::{Corrupt, LoadReport, Notice, SaveError, SkippedRow, Source};

use crate::model::{Entry, mark_interrupted_mid_search};
use crate::paths::Paths;

/// `LEGACY_BACKUP_DAYS` (`core/session.py:42`).
pub const LEGACY_BACKUP_DAYS: f64 = 30.0;

/// Seconds since the Unix epoch, as `time.time()`.
pub fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// What a load needs besides the files.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    /// Epoch seconds: availability verdicts older than 30 days are dropped, and a legacy
    /// backup is retired by its age.
    pub now: f64,
    /// The previous run died (the `crash.log.running` marker was found at start-up, spec
    /// 5.3). Every `searching` row is then set back to `not_searched` and flagged
    /// (rule 6.2.6); after a clean shutdown it stays as it is. Only the automatic session
    /// is affected, not a named one.
    pub unclean_shutdown: bool,
}

impl LoadOptions {
    pub fn at(now: f64) -> LoadOptions {
        LoadOptions {
            now,
            unclean_shutdown: false,
        }
    }
}

impl Default for LoadOptions {
    fn default() -> LoadOptions {
        LoadOptions::at(now_epoch())
    }
}

/// The restored list, in saved order, and what happened on the way.
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub entries: Vec<Entry>,
    pub report: LoadReport,
}

/// Read access to `CONFIG_DIR`'s automatic session.
#[derive(Debug, Clone)]
pub struct SessionStore {
    paths: Paths,
}

impl SessionStore {
    pub fn new(paths: &Paths) -> SessionStore {
        SessionStore {
            paths: paths.clone(),
        }
    }

    /// Either store exists on disk (`has_saved_session`): tells "nothing was ever saved"
    /// from "a session exists but came back empty".
    pub fn has_saved_session(&self) -> bool {
        self.paths.session_db().exists() || self.paths.session_json().exists()
    }

    /// Seconds since the newest of the two stores was written, by file modification time.
    pub fn saved_session_age_seconds(&self, now: f64) -> Option<f64> {
        [self.paths.session_db(), self.paths.session_json()]
            .iter()
            .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
            .filter_map(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .reduce(f64::max)
            .map(|newest| (now - newest).max(0.0))
    }

    /// Rows in `session.db`, without building any entry; `0` when it is absent or unusable.
    pub fn entry_count(&self) -> usize {
        let path = self.paths.session_db();
        if !path.exists() {
            return 0;
        }
        db::open_for_read(&path)
            .and_then(|c| db::count_entries(&c))
            .unwrap_or(0)
    }

    /// `load_session()` without its side effects: `session.db` if it exists (the legacy
    /// `session.json` is then not consulted, even when the database is empty), else
    /// `session.json`, else nothing (spec 3.6). Nothing is written and nothing is deleted;
    /// [`SessionWriter::restore`] adds the migration and the retirement.
    pub fn load(&self, opts: &LoadOptions) -> LoadedSession {
        let db_path = self.paths.session_db();
        let mut loaded = if db_path.exists() {
            match db::open_for_read(&db_path) {
                Ok(conn) => {
                    let (entries, report) = db::read_entries(&conn, opts.now);
                    LoadedSession { entries, report }
                }
                Err(e) => {
                    let mut report = LoadReport::new(Source::Database);
                    report.notices.push(Notice::DatabaseUnusable {
                        message: e.to_string(),
                    });
                    LoadedSession {
                        entries: Vec::new(),
                        report,
                    }
                }
            }
        } else if self.paths.session_json().exists() {
            let (entries, report) =
                named::read_document(&self.paths.session_json(), Source::LegacyJson, opts.now);
            LoadedSession { entries, report }
        } else {
            let mut report = LoadReport::new(Source::None);
            report.notices.push(Notice::NothingSaved);
            LoadedSession {
                entries: Vec::new(),
                report,
            }
        };
        if opts.unclean_shutdown {
            loaded.report.reset_mid_search = mark_interrupted_mid_search(&mut loaded.entries);
        }
        loaded
    }

    /// Take the right to write. Refused while the Python app is detectably running.
    pub fn open_writer(&self) -> Result<SessionWriter, PythonAppRunning> {
        self.open_writer_with(GuardConfig::default())
    }

    /// [`SessionStore::open_writer`] with an explicit `/proc` location and own pid.
    pub fn open_writer_with(&self, guard: GuardConfig) -> Result<SessionWriter, PythonAppRunning> {
        guard::ensure_single_writer(&self.paths, &guard)?;
        Ok(SessionWriter {
            store: self.clone(),
            guard,
        })
    }
}

/// Write access. Each operation re-checks that the Python app has not started since, and a
/// connection to `session.db` lives only as long as one operation, like the Python app's, so
/// `-wal` and `-shm` do not outlast a save (spec 3.1).
#[derive(Debug, Clone)]
pub struct SessionWriter {
    store: SessionStore,
    guard: GuardConfig,
}

impl SessionWriter {
    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    fn check(&self) -> Result<(), SaveError> {
        guard::ensure_single_writer(&self.store.paths, &self.guard).map_err(SaveError::Refused)
    }

    /// A full save of the automatic session (spec 3.7): one transaction, contiguous
    /// ordinals, rows no longer in the list deleted, both `meta` rows rewritten. On failure
    /// nothing changed.
    pub fn save(&self, entries: &[Entry], now: f64) -> Result<(), SaveError> {
        self.check()?;
        let mut conn = db::open_for_write(&self.store.paths.session_db())?;
        db::save_all(&mut conn, entries, now)
    }

    /// `load_session()` as the Python app runs it at start-up: load; when the list came from
    /// `session.json`, write it into a new `session.db` and keep the JSON as a backup
    /// (3.6.2); when it came from `session.db` with at least one entry, remove a leftover
    /// `session.json` that is older than the database and at least 30 days old (3.6.3).
    pub fn restore(&self, opts: &LoadOptions) -> LoadedSession {
        // Migration and retirement work on the list as the file had it; the reset of a
        // `searching` row is applied last, to the list the caller gets (it reaches disk with
        // the next save, like the Python app's revision bump).
        let mut loaded = self.store.load(&LoadOptions {
            unclean_shutdown: false,
            ..*opts
        });
        match loaded.report.source {
            Source::LegacyJson if !loaded.entries.is_empty() => {
                match self.save(&loaded.entries, opts.now) {
                    Ok(()) => loaded.report.notices.push(Notice::LegacyMigrated {
                        entries: loaded.entries.len(),
                    }),
                    Err(e) => loaded.report.notices.push(Notice::LegacyMigrationFailed {
                        message: e.to_string(),
                    }),
                }
            }
            Source::Database if !loaded.entries.is_empty() => {
                self.retire_legacy(opts.now, &mut loaded.report);
            }
            _ => {}
        }
        if opts.unclean_shutdown {
            loaded.report.reset_mid_search = mark_interrupted_mid_search(&mut loaded.entries);
        }
        loaded
    }

    fn retire_legacy(&self, now: f64, report: &mut LoadReport) {
        if self.check().is_err() {
            return;
        }
        let paths = &self.store.paths;
        let (Ok(legacy), Ok(current)) = (
            fs::metadata(paths.session_json()),
            fs::metadata(paths.session_db()),
        ) else {
            return;
        };
        let secs = |m: &fs::Metadata| {
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
        };
        let (Some(legacy_at), Some(current_at)) = (secs(&legacy), secs(&current)) else {
            return;
        };
        let age_days = (now - legacy_at) / 86400.0;
        if legacy_at >= current_at || age_days < LEGACY_BACKUP_DAYS {
            return;
        }
        match fs::remove_file(paths.session_json()) {
            Ok(()) => report.notices.push(Notice::LegacyRetired {
                age_days: age_days as u64,
            }),
            Err(e) => report
                .notices
                .push(Notice::LegacyRetireFailed { kind: e.kind() }),
        }
    }

    /// `clear_session()`: empty `session.db` (the file stays) and delete `session.json`, so
    /// the next start does not migrate it back. Returns whether the database held rows.
    pub fn clear(&self) -> Result<bool, SaveError> {
        self.check()?;
        let paths = &self.store.paths;
        let mut had = false;
        if paths.session_db().exists() {
            let mut conn = db::open_for_write(&paths.session_db())?;
            had = db::clear_entries(&mut conn)?;
        }
        match fs::remove_file(paths.session_json()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(had)
    }
}
