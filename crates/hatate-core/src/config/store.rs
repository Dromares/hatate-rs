//! Reading and writing `config.json` (spec 2.2, 2.4; rules 6.1.2, 6.1.3, 6.1.7).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde_json::{Map, Value};

use super::field::Section;
use super::migrate;
use super::model::{CURRENT_SCHEMA_VERSION, Settings};
use super::report::{LoadReport, Notice, SaveError, Source, Unreadable};
use crate::paths::Paths;

const FILE_MODE: u32 = 0o600;
const DIR_MODE: u32 = 0o700;

/// The result of [`Settings::load`]: usable settings, and what happened on the way.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub settings: Settings,
    pub report: LoadReport,
}

impl Settings {
    /// Load `config.json` the way the Python app does, but without ever failing.
    ///
    /// - No config file: defaults, saved at once (spec 2.2.1).
    /// - `config.json`, then `config.json.bak`; if only the backup works, the broken main
    ///   file is set aside as `config.json.unreadable`. If neither works, defaults, and
    ///   the main file is set aside the same way so no save can overwrite it (rule 6.1.3).
    /// - A wrong-typed value keeps its default; an unknown nested key is ignored; a
    ///   top-level array, a BOM or a truncated file counts as unreadable. Nothing panics.
    /// - Migrations from any older `schema_version`, then a save at 13 (rule 6.1.2).
    ///
    /// Everything that deserves a log line is in [`Loaded::report`], without values.
    pub fn load(paths: &Paths) -> Loaded {
        let mut report = LoadReport::default();
        let main = paths.config_file();

        if fs::metadata(&main).is_err() {
            report.push(Notice::FirstRun);
            let settings = Settings::with_defaults(paths);
            save_and_note(&settings, paths, &mut report);
            return Loaded { settings, report };
        }

        let mut raw: Option<Map<String, Value>> = None;
        for (source, path) in [
            (Source::Main, main.clone()),
            (Source::Backup, paths.config_backup_file()),
        ] {
            if fs::metadata(&path).is_err() {
                continue;
            }
            match read_object(&path) {
                Ok(obj) => {
                    raw = Some(obj);
                    if source == Source::Backup {
                        report.push(Notice::RecoveredFromBackup);
                        set_aside_unreadable(paths, &mut report);
                    }
                    break;
                }
                Err(why) => report.push(Notice::CouldNotRead { source, why }),
            }
        }

        let Some(raw) = raw else {
            report.push(Notice::NothingReadable);
            set_aside_unreadable(paths, &mut report);
            return Loaded {
                settings: Settings::with_defaults(paths),
                report,
            };
        };

        // Tighten an existing config now rather than at the next save (spec 2.2.4).
        if fs::metadata(&main).is_ok() {
            restrict(&main, FILE_MODE, &mut report);
        }
        restrict(paths.config_dir(), DIR_MODE, &mut report);

        let stored_version = stored_version(&raw, &mut report);
        let mut settings = Settings::with_defaults(paths);
        settings.read_object(&raw, "", &mut report);
        migrate::run(&mut settings, &raw, stored_version);

        if stored_version < CURRENT_SCHEMA_VERSION {
            report.push(Notice::Migrated {
                from: stored_version,
            });
            settings.schema_version = CURRENT_SCHEMA_VERSION;
            save_and_note(&settings, paths, &mut report);
        }
        report.finish();
        Loaded { settings, report }
    }

    /// Write `config.json` atomically (spec 2.4, rule 6.1.7).
    ///
    /// `CONFIG_DIR` is created and set to `0700`; an existing `config.json` is copied to
    /// `config.json.bak` first (a failure there is reported, not fatal); the new content
    /// goes to `config.json.tmp`, created `0600`, fsynced, then renamed over the target.
    /// Returns the notices for the non-fatal steps.
    pub fn save(&self, paths: &Paths) -> Result<Vec<Notice>, SaveError> {
        let mut notices = Vec::new();
        let dir = paths.config_dir();
        let main = paths.config_file();
        let tmp = paths.config_tmp_file();

        fs::create_dir_all(dir).map_err(|e| SaveError::new("create", dir.to_path_buf(), e))?;
        if let Err(e) = fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE)) {
            notices.push(Notice::CouldNotRestrict {
                path: dir.to_path_buf(),
                kind: e.kind(),
            });
        }

        if fs::metadata(&main).is_ok() {
            let bak = paths.config_backup_file();
            if let Err(e) = copy_private(&main, &bak) {
                notices.push(Notice::CouldNotBackUp { kind: e.kind() });
            }
        }

        let payload = self.to_json_string();
        write_private(&tmp, payload.as_bytes())
            .map_err(|e| SaveError::new("write", tmp.clone(), e))?;
        fs::rename(&tmp, &main).map_err(|e| SaveError::new("replace", main.clone(), e))?;
        // Best effort: make the rename itself durable. Python does not, and a filesystem
        // that cannot fsync a directory has already given us the atomic rename.
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(notices)
    }
}

fn save_and_note(settings: &Settings, paths: &Paths, report: &mut LoadReport) {
    match settings.save(paths) {
        Ok(notices) => notices.into_iter().for_each(|n| report.push(n)),
        Err(e) => report.push(Notice::SaveFailed {
            kind: e.source.kind(),
        }),
    }
}

/// `raw.get("schema_version", 1)`: a missing or non-integer value counts as 1.
fn stored_version(raw: &Map<String, Value>, report: &mut LoadReport) -> i64 {
    match raw.get("schema_version") {
        None => 1,
        Some(v) => match v.as_i64() {
            Some(n) => n,
            None => {
                // The field itself is reported as WrongType by the reader.
                report.push(Notice::SchemaVersionAssumedOne);
                1
            }
        },
    }
}

/// Read and parse one config file into its top-level object.
fn read_object(path: &Path) -> Result<Map<String, Value>, Unreadable> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| Unreadable::Io(e.kind()))?;
    parse_object(&bytes)
}

pub(crate) fn parse_object(bytes: &[u8]) -> Result<Map<String, Value>, Unreadable> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(Unreadable::Bom);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Unreadable::NotUtf8)?;
    // The error is reduced to its position: serde_json messages can quote input.
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(obj)) => Ok(obj),
        Ok(_) => Err(Unreadable::NotAnObject),
        Err(e) => Err(Unreadable::Syntax {
            line: e.line(),
            column: e.column(),
        }),
    }
}

/// Move `config.json` out of the way of any future save (`_preserve_unreadable_config`).
/// An earlier `.unreadable` is never replaced: the next free `.unreadable.N` is used.
fn set_aside_unreadable(paths: &Paths, report: &mut LoadReport) {
    let main = paths.config_file();
    if fs::metadata(&main).is_err() {
        return;
    }
    let mut target = paths.config_unreadable_file();
    let mut n = 0u32;
    while fs::symlink_metadata(&target).is_ok() {
        n += 1;
        target = crate::paths::rotated(&paths.config_unreadable_file(), n);
    }
    match fs::rename(&main, &target) {
        Ok(()) => report.push(Notice::SetAsideUnreadable { to: target }),
        Err(e) => report.push(Notice::CouldNotSetAside { kind: e.kind() }),
    }
}

fn restrict(path: &Path, mode: u32, report: &mut LoadReport) {
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        report.push(Notice::CouldNotRestrict {
            path: path.to_path_buf(),
            kind: e.kind(),
        });
    }
}

/// Create (or truncate) `path` with mode `0600` set at creation, write, fsync, chmod.
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(FILE_MODE)
        .open(path)?;
    f.write_all(data)?;
    f.flush()?;
    f.sync_all()?;
    // A leftover file from an older run keeps its old mode through O_CREAT.
    fs::set_permissions(path, fs::Permissions::from_mode(FILE_MODE))
}

fn copy_private(from: &Path, to: &Path) -> io::Result<()> {
    let data = fs::read(from)?;
    write_private(to, &data)
}
