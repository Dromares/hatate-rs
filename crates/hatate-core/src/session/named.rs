//! Session documents: named sessions (spec 3.5) and the legacy `session.json` (spec 3.6),
//! which is the same document.
//!
//! ```text
//! {"version": 1, "saved_at": <epoch float>, "entries": [ <entry JSON>, ... ]}
//! ```

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use super::codec::{self, Decoded};
use super::report::{LoadReport, Notice, SaveError, SkippedRow, Source};
use super::tolerant;
use super::{LoadOptions, LoadedSession};
use crate::model::Entry;

/// `SESSION_FORMAT_VERSION` (`core/session.py:37`).
pub const SESSION_FORMAT_VERSION: i64 = 1;

#[derive(Serialize)]
struct Document<'a> {
    version: i64,
    saved_at: f64,
    entries: &'a [Entry],
}

/// Open a named session ("Open Session"). A file that cannot be read, is not an object,
/// or has a `version` other than 1 gives an empty list and a notice, never an error
/// (spec 3.5). A malformed element is skipped and counted.
pub fn load(path: &Path, opts: &LoadOptions) -> LoadedSession {
    let (entries, report) = read_document(path, Source::Named, opts.now);
    LoadedSession { entries, report }
}

/// How many entries a session file holds, without building any: `None` when it is not a
/// readable version-1 session (`session_entry_count`, used to preview "Open Session").
pub fn entry_count(path: &Path) -> Option<usize> {
    let value = parse_file(path).ok()?;
    let Value::Object(doc) = value else {
        return None;
    };
    if !version_is_one(doc.get("version")) {
        return None;
    }
    match doc.get("entries") {
        Some(Value::Array(a)) => Some(a.len()),
        Some(v) if !truthy_empty(v) => None,
        _ => Some(0),
    }
}

/// `Value` is falsy and so counts as "no entries".
fn truthy_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// `payload.get("version") != 1` is false: Python's `==` takes `1`, `1.0` and `true`.
fn version_is_one(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        Some(Value::Bool(b)) => *b,
        _ => false,
    }
}

fn parse_file(path: &Path) -> Result<Value, Notice> {
    let bytes = fs::read(path).map_err(|e| Notice::FileUnreadable {
        kind: Some(e.kind()),
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Notice::FileUnreadable { kind: None })?;
    tolerant::parse(text).map_err(|_| Notice::FileUnreadable { kind: None })
}

/// `_load_json_session`. `source` only labels the report.
pub(crate) fn read_document(path: &Path, source: Source, now: f64) -> (Vec<Entry>, LoadReport) {
    let mut report = LoadReport::new(source);
    let value = match parse_file(path) {
        Ok(v) => v,
        Err(notice) => {
            report.notices.push(notice);
            return (Vec::new(), report);
        }
    };
    let Value::Object(doc) = value else {
        report.notices.push(Notice::NotASessionObject);
        return (Vec::new(), report);
    };
    if !version_is_one(doc.get("version")) {
        report.notices.push(Notice::UnsupportedVersion);
        return (Vec::new(), report);
    }
    let items: &[Value] = match doc.get("entries") {
        Some(Value::Array(a)) => a,
        Some(v) if !truthy_empty(v) => {
            report.notices.push(Notice::EntriesNotAList);
            &[]
        }
        _ => &[],
    };
    let mut entries = Vec::new();
    for (position, item) in items.iter().enumerate() {
        let mut coerced = Vec::new();
        match codec::entry_from_value(item, now, &mut coerced) {
            Ok(Decoded::Entry(e)) => {
                for field in coerced {
                    report.notices.push(Notice::FieldCoerced {
                        row: position,
                        field,
                    });
                }
                entries.push(*e);
            }
            Ok(Decoded::Ignored) => report.ignored += 1,
            Err(why) => report.skipped.push(SkippedRow { position, why }),
        }
    }
    (entries, report)
}

/// "Save Session As" (`write_session_payload`, `core/session.py:211-246`): the document goes
/// to a unique temp file `<name>.<random>.tmp` in the same directory, created `0600`,
/// flushed and fsynced, renamed over the target, and the directory is fsynced. A failed write
/// removes its temp file. The target is never left half-written.
pub fn save(path: &Path, entries: &[Entry], now: f64) -> Result<(), SaveError> {
    let text = codec::to_python_json(&Document {
        version: SESSION_FORMAT_VERSION,
        saved_at: now,
        entries,
    })
    .map_err(SaveError::Encode)?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    fs::create_dir_all(&dir)?;
    let (tmp_path, mut tmp) = create_unique_tmp(&dir, name)?;
    let result = (|| -> io::Result<()> {
        tmp.write_all(text.as_bytes())?;
        tmp.flush()?;
        tmp.sync_all()?;
        fs::rename(&tmp_path, path)?;
        // Best effort, like the original: it only costs the extra guarantee.
        if let Ok(d) = File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

fn create_unique_tmp(dir: &Path, name: &std::ffi::OsStr) -> io::Result<(PathBuf, File)> {
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let tag = format!(
            "{:x}{:x}{:x}",
            std::process::id(),
            nanos,
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let mut file_name = name.to_os_string();
        file_name.push(format!(".{tag}.tmp"));
        let candidate = dir.join(file_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(f) => return Ok((candidate, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}
