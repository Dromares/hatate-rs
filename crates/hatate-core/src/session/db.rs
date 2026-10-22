//! `session.db`: the SQLite store (spec 3.1 to 3.3, 3.7).

use std::collections::HashSet;
use std::fs::{DirBuilder, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use super::codec::{self, Coerced, Decoded};
use super::report::{Corrupt, LoadReport, Notice, SaveError, SkippedRow, Source};
use super::tolerant;
use crate::config::pyjson;
use crate::model::Entry;

/// `SCHEMA_VERSION` (`core/session_db.py:37`). Nothing reads it back; it is written anyway.
pub(crate) const SCHEMA_VERSION: &str = "1";

/// The DDL of spec 3.2, character for character as `core/session_db.py:54-61` passes it to
/// SQLite, including the indentation SQLite keeps in `sqlite_master`.
pub(crate) const ENTRIES_DDL: &str = concat!(
    "\n        CREATE TABLE IF NOT EXISTS entries (\n",
    "            path    TEXT PRIMARY KEY,\n",
    "            ordinal INTEGER NOT NULL,\n",
    "            data    TEXT NOT NULL\n",
    "        )\n    "
);
pub(crate) const META_DDL: &str =
    "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)";

/// `sqlite3.connect(path, timeout=30.0)`.
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// Open an existing database to read it. Never creates the file and never runs a write
/// statement; read-write access is only requested so that closing the last connection can
/// remove the `-wal` and `-shm` files like any other close, and a database the user cannot
/// write is opened read-only instead.
pub(crate) fn open_for_read(path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?,
    };
    conn.busy_timeout(BUSY_TIMEOUT)?;
    Ok(conn)
}

/// `_connect`: create the directory and the file, then WAL, `synchronous=FULL` and the two
/// `CREATE TABLE IF NOT EXISTS` statements, in that order (spec 3.1, rule 6.2.2).
///
/// A file created here is `0600` (the Python app leaves it to the umask): it lists the
/// user's image paths.
pub(crate) fn open_for_write(path: &Path) -> Result<Connection, SaveError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    // Created here so the mode is right from the first byte; SQLite then opens it.
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    // `journal_mode` answers with a row, so it cannot go through `execute`.
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA synchronous=FULL")?;
    conn.execute(ENTRIES_DDL, [])?;
    conn.execute(META_DDL, [])?;
    Ok(conn)
}

fn has_entries_table(conn: &Connection) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'entries'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

/// `SELECT data FROM entries ORDER BY ordinal`, each row turned into an entry. A bad row is
/// skipped and counted; one bad row never discards the others (spec 3.4). A database that
/// cannot be read at all gives an empty list and a [`Notice::DatabaseUnusable`], like the
/// Python app's `return [], 0`.
pub(crate) fn read_entries(conn: &Connection, now: f64) -> (Vec<Entry>, LoadReport) {
    let mut report = LoadReport::new(Source::Database);
    match read_rows(conn, now, &mut report) {
        Ok(entries) => (entries, report),
        Err(e) => {
            let mut report = LoadReport::new(Source::Database);
            report.notices.push(Notice::DatabaseUnusable {
                message: e.to_string(),
            });
            (Vec::new(), report)
        }
    }
}

fn read_rows(
    conn: &Connection,
    now: f64,
    report: &mut LoadReport,
) -> Result<Vec<Entry>, rusqlite::Error> {
    if !has_entries_table(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT data FROM entries ORDER BY ordinal")?;
    let mut rows = stmt.query([])?;
    let mut entries = Vec::new();
    let mut position = 0usize;
    while let Some(row) = rows.next()? {
        let decoded = match row.get_ref(0)? {
            ValueRef::Text(b) | ValueRef::Blob(b) => decode(b, now),
            _ => Err(Corrupt::NotText),
        };
        match decoded {
            Ok((Decoded::Entry(e), coerced)) => {
                for field in coerced {
                    report.notices.push(Notice::FieldCoerced {
                        row: position,
                        field,
                    });
                }
                entries.push(*e);
            }
            Ok((Decoded::Ignored, _)) => report.ignored += 1,
            Err(why) => report.skipped.push(SkippedRow { position, why }),
        }
        position += 1;
    }
    Ok(entries)
}

/// One `data` value: bytes, to JSON, to an entry.
pub(crate) fn decode(bytes: &[u8], now: f64) -> Result<(Decoded, Coerced), Corrupt> {
    let text = std::str::from_utf8(bytes).map_err(|_| Corrupt::NotText)?;
    let value = tolerant::parse(text).map_err(|_| Corrupt::NotJson)?;
    let mut coerced = Vec::new();
    let decoded = codec::entry_from_value(&value, now, &mut coerced)?;
    Ok((decoded, coerced))
}

pub(crate) fn count_entries(conn: &Connection) -> Result<usize, rusqlite::Error> {
    if !has_entries_table(conn)? {
        return Ok(0);
    }
    conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get::<_, i64>(0))
        .map(|n| n.max(0) as usize)
}

/// A full save (`save_entries(entries, None)`): one transaction, so the whole save lands or
/// none of it. Rows whose path is no longer in the list are deleted, **decided from what the
/// file holds** (`core/session_db.py:101-114`); every entry is upserted with its list
/// position as `ordinal`, which makes the ordinals `0..n-1`; both `meta` rows are rewritten.
pub(crate) fn save_all(
    conn: &mut Connection,
    entries: &[Entry],
    now: f64,
) -> Result<(), SaveError> {
    // Encode first: an entry that cannot be encoded must not leave a half-written save, and
    // the write lock should not be held while this runs.
    let mut payloads = Vec::with_capacity(entries.len());
    for entry in entries {
        payloads.push(codec::to_python_json(entry).map_err(SaveError::Encode)?);
    }
    let mut saved_at = String::new();
    pyjson::float(&mut saved_at, now);

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing: Vec<String> = {
        let mut stmt = tx.prepare("SELECT path FROM entries")?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            if let ValueRef::Text(b) = row.get_ref(0)? {
                if let Ok(s) = std::str::from_utf8(b) {
                    out.push(s.to_owned());
                }
            }
        }
        out
    };
    let current: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    const UPSERT_META: &str = "INSERT INTO meta(key, value) VALUES(?, ?) \
                               ON CONFLICT(key) DO UPDATE SET value=excluded.value";
    tx.execute(UPSERT_META, ["schema_version", SCHEMA_VERSION])?;
    tx.execute(UPSERT_META, ["saved_at", saved_at.as_str()])?;
    {
        let mut delete = tx.prepare("DELETE FROM entries WHERE path = ?")?;
        for path in existing.iter().filter(|p| !current.contains(p.as_str())) {
            delete.execute([path])?;
        }
        let mut upsert = tx.prepare(
            "INSERT INTO entries(path, ordinal, data) VALUES(?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET ordinal=excluded.ordinal, data=excluded.data",
        )?;
        for (ordinal, (entry, payload)) in entries.iter().zip(&payloads).enumerate() {
            upsert.execute(rusqlite::params![entry.path, ordinal as i64, payload])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// `clear()`: delete every row, keep the file. Returns whether there was anything.
pub(crate) fn clear_entries(conn: &mut Connection) -> Result<bool, SaveError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let had = tx.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get::<_, i64>(0))? > 0;
    tx.execute("DELETE FROM entries", [])?;
    tx.commit()?;
    Ok(had)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rule 6.2.7: `synchronous=FULL` is a property of the connection, so it is checked on
    // the connection a save uses (the file cannot show it).
    #[test]
    fn a_writer_connection_is_wal_and_synchronous_full() {
        let dir = tempfile::Builder::new()
            .prefix("hatate-core-test-")
            .tempdir()
            .unwrap();
        let conn = open_for_write(&dir.path().join("session.db")).unwrap();
        let sync: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 2, "FULL");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(timeout, 30_000);
    }
}
