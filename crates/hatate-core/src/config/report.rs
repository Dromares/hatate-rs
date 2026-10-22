//! What a load or save noticed. Nothing here ever holds a setting's value: only key
//! names, counts, file paths and I/O error kinds, so a notice is safe to log (rule 6.1.10).

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Which file a config was (or was not) read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Main,
    Backup,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Main => "config.json",
            Source::Backup => "config.json.bak",
        })
    }
}

/// Why a config file could not be used. No file content, ever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    Io(io::ErrorKind),
    NotUtf8,
    Bom,
    /// Not valid JSON; position only.
    Syntax {
        line: usize,
        column: usize,
    },
    /// Valid JSON, but the top level is not an object (an array, a string, `null`, ...).
    NotAnObject,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unreadable::Io(kind) => write!(f, "I/O error ({kind})"),
            Unreadable::NotUtf8 => f.write_str("not valid UTF-8"),
            Unreadable::Bom => f.write_str("starts with a UTF-8 byte order mark"),
            Unreadable::Syntax { line, column } => {
                write!(f, "not valid JSON (line {line}, column {column})")
            }
            Unreadable::NotAnObject => f.write_str("the top level is not a JSON object"),
        }
    }
}

/// One thing worth telling the caller about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// No config existed: defaults were used and saved.
    FirstRun,
    /// A source could not be used.
    CouldNotRead {
        source: Source,
        why: Unreadable,
    },
    /// `config.json` was unusable and the settings came from `config.json.bak`.
    RecoveredFromBackup,
    /// Neither file was usable: defaults are in effect.
    NothingReadable,
    /// The unreadable `config.json` was renamed so no save can overwrite it.
    SetAsideUnreadable {
        to: PathBuf,
    },
    CouldNotSetAside {
        kind: io::ErrorKind,
    },
    /// A value had the wrong JSON type and the default was kept. `key` is a dotted path.
    WrongType {
        key: String,
    },
    /// An unknown key inside a nested object (Python would fail to start on it); dropped.
    UnknownNestedKey {
        section: String,
    },
    /// Unknown top-level keys, dropped (Python drops them at its next save too).
    UnknownKeysIgnored {
        count: usize,
    },
    /// `schema_version` was missing or not an integer and was taken to be 1.
    SchemaVersionAssumedOne,
    /// Migrations ran, from this stored version up to 13.
    Migrated {
        from: i64,
    },
    /// The permissions of a path could not be tightened (e.g. FAT).
    CouldNotRestrict {
        path: PathBuf,
        kind: io::ErrorKind,
    },
    /// The previous config could not be copied to `config.json.bak`; the save went on.
    CouldNotBackUp {
        kind: io::ErrorKind,
    },
    /// A save after loading failed. The settings in memory are still good.
    SaveFailed {
        kind: io::ErrorKind,
    },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::FirstRun => f.write_str("no config file: created one with defaults"),
            Notice::CouldNotRead { source, why } => write!(f, "could not use {source}: {why}"),
            Notice::RecoveredFromBackup => {
                f.write_str("recovered the settings from config.json.bak")
            }
            Notice::NothingReadable => f.write_str("no readable config or backup: using defaults"),
            Notice::SetAsideUnreadable { to } => write!(
                f,
                "kept the unreadable config as {}: your keys and cookies are still in it",
                to.display()
            ),
            Notice::CouldNotSetAside { kind } => {
                write!(f, "could not set aside the unreadable config ({kind})")
            }
            Notice::WrongType { key } => write!(f, "{key} has the wrong type: kept the default"),
            Notice::UnknownNestedKey { section } => {
                write!(f, "ignored an unknown key inside {section}")
            }
            Notice::UnknownKeysIgnored { count } => {
                write!(f, "ignored {count} unrecognised top-level key(s)")
            }
            Notice::SchemaVersionAssumedOne => {
                f.write_str("schema_version missing or not an integer: treated as 1")
            }
            Notice::Migrated { from } => {
                write!(f, "migrated the config from schema_version {from} to 13")
            }
            Notice::CouldNotRestrict { path, kind } => write!(
                f,
                "could not restrict permissions on {} ({kind}): it may be readable by other users",
                path.display()
            ),
            Notice::CouldNotBackUp { kind } => write!(
                f,
                "could not back up config.json to config.json.bak ({kind})"
            ),
            Notice::SaveFailed { kind } => write!(f, "could not save the config ({kind})"),
        }
    }
}

/// Everything a load noticed, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub notices: Vec<Notice>,
    unknown_top_level: usize,
}

impl LoadReport {
    pub(crate) fn push(&mut self, n: Notice) {
        self.notices.push(n);
    }

    pub(crate) fn wrong_type(&mut self, key: &str) {
        self.push(Notice::WrongType {
            key: key.to_owned(),
        });
    }

    /// An unknown key under `prefix` (empty for the top level).
    pub(crate) fn unknown_key(&mut self, prefix: &str) {
        if prefix.is_empty() {
            self.unknown_top_level += 1;
        } else {
            self.push(Notice::UnknownNestedKey {
                section: prefix.to_owned(),
            });
        }
    }

    pub(crate) fn finish(&mut self) {
        if self.unknown_top_level > 0 {
            let count = std::mem::take(&mut self.unknown_top_level);
            self.push(Notice::UnknownKeysIgnored { count });
        }
    }
}

impl fmt::Display for LoadReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, n) in self.notices.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{n}")?;
        }
        Ok(())
    }
}

/// A save that could not be completed. The message names the failed step and path and
/// carries the OS error text, which never contains a setting's value.
#[derive(Debug)]
pub struct SaveError {
    pub step: &'static str,
    pub path: PathBuf,
    pub source: io::Error,
}

impl SaveError {
    pub(crate) fn new(step: &'static str, path: PathBuf, source: io::Error) -> Self {
        SaveError { step, path, source }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot {} {}: {}",
            self.step,
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
