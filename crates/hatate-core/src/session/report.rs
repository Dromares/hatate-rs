//! What a load or save noticed. Like `config::report`, nothing here holds a value from the
//! user's data: only counts, row positions, field names, file names and error kinds, so a
//! notice is safe to log.

use std::fmt;
use std::io;

/// Where a list of entries came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Nothing was found.
    None,
    /// `session.db`.
    Database,
    /// The legacy `session.json` (spec 3.6).
    LegacyJson,
    /// A named session file (spec 3.5).
    Named,
}

/// Why a row or element could not be turned into an entry. Each one is what makes the Python
/// app raise while reading it (spec 3.4, 3.4.1, 6.5): the row is skipped and counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corrupt {
    NotText,
    NotJson,
    NotAnObject,
    PathNotAString,
    CandidatesNotAList,
    CandidateNotAnObject,
    CandidateWithoutUrl,
    TagListNotAList,
    TagNotAnObject,
    TagWithoutName,
    BadTagVersion,
    BadSelectedIndex,
}

impl fmt::Display for Corrupt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Corrupt::NotText => "the data column is not text",
            Corrupt::NotJson => "the data is not valid JSON",
            Corrupt::NotAnObject => "the data is not a JSON object",
            Corrupt::PathNotAString => "the path is not a string",
            Corrupt::CandidatesNotAList => "candidates is not a list",
            Corrupt::CandidateNotAnObject => "a candidate is not an object",
            Corrupt::CandidateWithoutUrl => "a candidate has no url",
            Corrupt::TagListNotAList => "a tag list is not a list",
            Corrupt::TagNotAnObject => "a tag is not an object",
            Corrupt::TagWithoutName => "a tag has no name",
            Corrupt::BadTagVersion => "a candidate's tag_version is not an integer",
            Corrupt::BadSelectedIndex => "selected_candidate_index is not usable as an index",
        })
    }
}

/// A row that was skipped and counted (spec 3.4: "corrupt").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkippedRow {
    /// 0-based position among the rows read, in `ORDER BY ordinal` order, or among the
    /// elements of a session file's `entries` array.
    pub position: usize,
    pub why: Corrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// No `session.db` and no `session.json`.
    NothingSaved,
    /// `session.db` could not be used (not an SQLite file, locked, ...): the list is empty and
    /// the file was left alone. The message is SQLite's, which names no row data.
    DatabaseUnusable {
        message: String,
    },
    /// A value had the wrong type for its field and the default was used instead (Python
    /// would carry the wrong value forward).
    FieldCoerced {
        row: usize,
        field: &'static str,
    },
    /// A session file could not be read or is not UTF-8 JSON.
    FileUnreadable {
        kind: Option<io::ErrorKind>,
    },
    /// A session file whose top level is not an object.
    NotASessionObject,
    /// A session file whose `version` is not 1: ignored, empty list (spec 3.5).
    UnsupportedVersion,
    /// A session file whose `entries` is not an array: treated as empty.
    EntriesNotAList,
    /// `session.json` was written into a new `session.db` (spec 3.6.2).
    LegacyMigrated {
        entries: usize,
    },
    /// The migration could not write `session.db`; this run continues from the JSON.
    LegacyMigrationFailed {
        message: String,
    },
    /// A migrated `session.json` older than 30 days and than the database was removed (3.6.3).
    LegacyRetired {
        age_days: u64,
    },
    LegacyRetireFailed {
        kind: io::ErrorKind,
    },
}

/// The outcome of reading entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    pub source: Source,
    /// Corrupt rows, dropped. The caller should tell the user: their search state is gone.
    pub skipped: Vec<SkippedRow>,
    /// Rows without a usable path, dropped without being counted as corrupt (spec 3.4).
    pub ignored: usize,
    /// Rows reset from `searching` to `not_searched` after an unclean shutdown (rule 6.2.6).
    pub reset_mid_search: usize,
    pub notices: Vec<Notice>,
}

impl LoadReport {
    pub(crate) fn new(source: Source) -> LoadReport {
        LoadReport {
            source,
            skipped: Vec::new(),
            ignored: 0,
            reset_mid_search: 0,
            notices: Vec::new(),
        }
    }
}

/// `session.db` could not be written. SQLite's message, which names columns and constraints
/// but no row data, or the I/O error.
#[derive(Debug)]
pub enum SaveError {
    /// The Python app is running; nothing was written (rule 6.2.10).
    Refused(super::guard::PythonAppRunning),
    Database(String),
    Io(io::Error),
    Encode(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Refused(why) => write!(f, "{why}"),
            SaveError::Database(m) => write!(f, "could not write the session database: {m}"),
            SaveError::Io(e) => write!(f, "could not write the session: {}", e.kind()),
            SaveError::Encode(m) => write!(f, "could not encode the session: {m}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<rusqlite::Error> for SaveError {
    fn from(e: rusqlite::Error) -> SaveError {
        SaveError::Database(e.to_string())
    }
}

impl From<io::Error> for SaveError {
    fn from(e: io::Error) -> SaveError {
        SaveError::Io(e)
    }
}
