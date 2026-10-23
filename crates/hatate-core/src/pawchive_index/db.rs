//! The index database (`PawchiveIndex`): SQLite at Python's path with Python's schema, one
//! connection per call that commits on success and is always closed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, params};

use super::{
    Creator, IndexMatch, MAX_MATCH_DISTANCE, MAX_RESULTS, PawchiveError, post_url, similarity_for,
    thumbnail_url,
};
use crate::clock::{Clock, SystemClock};
use crate::image_compare::Hash;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS creators (
    service     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    name        TEXT,
    added_at    REAL,
    indexed_at  REAL,
    post_count  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (service, user_id)
);
CREATE TABLE IF NOT EXISTS images (
    service     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    post_id     TEXT NOT NULL,
    page_index  INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    path        TEXT NOT NULL,
    title       TEXT,
    dhash8      TEXT,               -- hex; NULL when pawchive had no thumbnail
    dhash16     TEXT,
    PRIMARY KEY (service, user_id, post_id, page_index)
);
CREATE INDEX IF NOT EXISTS images_by_sha256 ON images (sha256);
";

/// `_generation`: bumped on every write by any index, to invalidate every index's row cache.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn bump() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// One row of `images` as `add_images` takes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRow {
    pub service: String,
    pub user_id: String,
    pub post_id: String,
    pub page_index: i64,
    pub sha256: String,
    pub path: String,
    pub title: Option<String>,
    /// Lower-case hex, 16 digits; `None` when Pawchive had no thumbnail.
    pub dhash8: Option<String>,
    /// Lower-case hex, 64 digits.
    pub dhash16: Option<String>,
}

/// A row of the in-memory search cache: `(int(dhash16, 16), service, user_id, post_id,
/// page_index, path, title, artist)`.
#[derive(Debug, Clone)]
struct CachedRow {
    hash: Hash,
    service: String,
    user_id: String,
    post_id: String,
    page_index: i64,
    path: String,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Default)]
struct Cache {
    key: Option<u64>,
    rows: Arc<Vec<CachedRow>>,
}

pub struct PawchiveIndex {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    cache: Mutex<Cache>,
}

/// The SELECT both searches read an image row with.
const SEARCH_COLUMNS: &str = "SELECT i.dhash16, i.service, i.user_id, i.post_id, i.page_index,
                                   i.path, i.title, c.name
                            FROM images i LEFT JOIN creators c
                              ON c.service = i.service AND c.user_id = i.user_id";

impl PawchiveIndex {
    /// An index at `path` (`paths.pawchive_index_db()` in the app), on the real clock.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_clock(path, Arc::new(SystemClock::default()))
    }

    /// As [`PawchiveIndex::new`] with the clock `added_at` and `indexed_at` are read from.
    pub fn with_clock(path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        PawchiveIndex {
            path: path.into(),
            clock,
            cache: Mutex::new(Cache::default()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `_connect`: the folder made, the schema in place. Commit with [`Connection::execute`]
    /// inside a transaction the caller opens; a failure rolls back when the connection closes.
    fn connect(&self) -> Result<Connection, PawchiveError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PawchiveError::io(&e, parent))?;
        }
        let conn = Connection::open(&self.path).map_err(|e| PawchiveError::database(&e))?;
        conn.busy_timeout(Duration::from_secs(30))
            .map_err(|e| PawchiveError::database(&e))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| PawchiveError::database(&e))?;
        Ok(conn)
    }

    /// Runs `work` in one transaction that commits when it succeeds.
    fn write<T>(
        &self,
        work: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<T, PawchiveError> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction()
            .map_err(|e| PawchiveError::database(&e))?;
        let out = work(&tx).map_err(|e| PawchiveError::database(&e))?;
        tx.commit().map_err(|e| PawchiveError::database(&e))?;
        Ok(out)
    }

    /// `creators()`: every indexed artist, by name.
    pub fn creators(&self) -> Result<Vec<Creator>, PawchiveError> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "
                SELECT c.service, c.user_id, c.name, c.indexed_at, c.post_count,
                       COUNT(i.sha256), SUM(CASE WHEN i.sha256 IS NOT NULL AND i.dhash16 IS NULL
                                            THEN 1 ELSE 0 END)
                FROM creators c LEFT JOIN images i
                  ON i.service = c.service AND i.user_id = c.user_id
                GROUP BY c.service, c.user_id
                ORDER BY LOWER(COALESCE(c.name, c.user_id))
            ",
            )
            .map_err(|e| PawchiveError::database(&e))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Creator {
                    service: r.get(0)?,
                    user_id: r.get(1)?,
                    name: r.get(2)?,
                    indexed_at: r.get(3)?,
                    post_count: r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    image_count: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    unfingerprinted: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|e| PawchiveError::database(&e))?;
        Ok(rows)
    }

    /// `add_creator`: adds an artist to index. `false` if it was already there.
    pub fn add_creator(
        &self,
        service: &str,
        user_id: &str,
        name: Option<&str>,
    ) -> Result<bool, PawchiveError> {
        let now = self.clock.wall();
        let added = self.write(|tx| {
            let changed = tx.execute(
                "INSERT OR IGNORE INTO creators (service, user_id, name, added_at) VALUES (?,?,?,?)",
                params![service, user_id, name, now],
            )?;
            if let Some(name) = name.filter(|n| !n.is_empty()) {
                tx.execute(
                    "UPDATE creators SET name = ? WHERE service = ? AND user_id = ?",
                    params![name, service, user_id],
                )?;
            }
            Ok(changed > 0)
        })?;
        bump();
        Ok(added)
    }

    /// `remove_creator`: the artist and every image of theirs.
    pub fn remove_creator(&self, service: &str, user_id: &str) -> Result<(), PawchiveError> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM images WHERE service = ? AND user_id = ?",
                params![service, user_id],
            )?;
            tx.execute(
                "DELETE FROM creators WHERE service = ? AND user_id = ?",
                params![service, user_id],
            )?;
            Ok(())
        })?;
        bump();
        Ok(())
    }

    /// `known_pages`: the `(post_id, page_index)` pairs already indexed for an artist.
    pub fn known_pages(
        &self,
        service: &str,
        user_id: &str,
    ) -> Result<HashSet<(String, i64)>, PawchiveError> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT post_id, page_index FROM images WHERE service = ? AND user_id = ?")
            .map_err(|e| PawchiveError::database(&e))?;
        stmt.query_map(params![service, user_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(|rows| rows.collect::<rusqlite::Result<HashSet<_>>>())
            .map_err(|e| PawchiveError::database(&e))
    }

    /// `add_images`: `INSERT OR REPLACE` of every row, in one transaction. Nothing for no rows.
    pub fn add_images(&self, rows: &[ImageRow]) -> Result<(), PawchiveError> {
        if rows.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            let mut stmt = tx.prepare(
                "
                INSERT OR REPLACE INTO images
                    (service, user_id, post_id, page_index, sha256, path, title, dhash8, dhash16)
                VALUES (?,?,?,?,?,?,?,?,?)",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.service,
                    row.user_id,
                    row.post_id,
                    row.page_index,
                    row.sha256,
                    row.path,
                    row.title,
                    row.dhash8,
                    row.dhash16
                ])?;
            }
            Ok(())
        })?;
        bump();
        Ok(())
    }

    /// `finish_creator`: stamps an artist as indexed. A `None` name keeps the one there.
    pub fn finish_creator(
        &self,
        service: &str,
        user_id: &str,
        name: Option<&str>,
        posts: usize,
    ) -> Result<(), PawchiveError> {
        let now = self.clock.wall();
        self.write(|tx| {
            tx.execute(
                "
                UPDATE creators SET name = COALESCE(?, name), indexed_at = ?, post_count = ?
                WHERE service = ? AND user_id = ?",
                params![name, now, posts as i64, service, user_id],
            )?;
            Ok(())
        })?;
        bump();
        Ok(())
    }

    /// `image_count`: how many images have a fingerprint. 0 with no file.
    pub fn image_count(&self) -> Result<i64, PawchiveError> {
        if !self.path.exists() {
            return Ok(0);
        }
        let conn = self.connect()?;
        conn.query_row(
            "SELECT COUNT(*) FROM images WHERE dhash16 IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|e| PawchiveError::database(&e))
    }

    /// `_fingerprinted_rows`: every fingerprinted image, held in memory between writes (by any
    /// index in the process). Empty while the file does not exist.
    fn fingerprinted_rows(&self) -> Result<Arc<Vec<CachedRow>>, PawchiveError> {
        if !self.path.exists() {
            return Ok(Arc::new(Vec::new()));
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let generation = GENERATION.load(Ordering::SeqCst);
        if cache.key != Some(generation) {
            let conn = self.connect()?;
            let sql = format!("{SEARCH_COLUMNS}\n WHERE i.dhash16 IS NOT NULL");
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| PawchiveError::database(&e))?;
            let rows = stmt
                .query_map([], |r| cached_row(r, false))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|e| PawchiveError::database(&e))?;
            cache.rows = Arc::new(rows.into_iter().collect::<Result<Vec<_>, _>>()?);
            cache.key = Some(generation);
        }
        Ok(Arc::clone(&cache.rows))
    }

    /// `is_empty`: whether nothing is fingerprinted.
    pub fn is_empty(&self) -> Result<bool, PawchiveError> {
        Ok(self.fingerprinted_rows()?.is_empty())
    }

    /// `search` with Python's defaults ([`MAX_RESULTS`], [`MAX_MATCH_DISTANCE`]).
    pub fn search_default(
        &self,
        local_hash16: Option<&Hash>,
        sha256: Option<&str>,
    ) -> Result<Vec<IndexMatch>, PawchiveError> {
        self.search(local_hash16, sha256, MAX_RESULTS, MAX_MATCH_DISTANCE)
    }

    /// `search`: the indexed images most like the local one, best first, one per post. A row with
    /// the same SHA-256 is the very same file.
    pub fn search(
        &self,
        local_hash16: Option<&Hash>,
        sha256: Option<&str>,
        limit: usize,
        max_distance: u32,
    ) -> Result<Vec<IndexMatch>, PawchiveError> {
        // post key -> (distance, row), in the order a dict keeps: first insertion.
        let mut best: Vec<((String, String, String), u32, CachedRow)> = Vec::new();
        let mut position: HashMap<(String, String, String), usize> = HashMap::new();
        if let Some(local) = local_hash16 {
            let all = self.fingerprinted_rows()?;
            for row in all.iter() {
                let distance = local.xor_popcount(&row.hash);
                if distance > max_distance {
                    continue;
                }
                let key = (
                    row.service.clone(),
                    row.user_id.clone(),
                    row.post_id.clone(),
                );
                match position.get(&key) {
                    Some(&at) => {
                        if distance < best[at].1 {
                            best[at].1 = distance;
                            best[at].2 = row.clone();
                        }
                    }
                    None => {
                        position.insert(key.clone(), best.len());
                        best.push((key, distance, row.clone()));
                    }
                }
            }
        }

        let mut exact_keys: HashSet<(String, String, String)> = HashSet::new();
        if let Some(sha256) = sha256.filter(|s| !s.is_empty())
            && self.path.exists()
        {
            let conn = self.connect()?;
            let sql = format!("{SEARCH_COLUMNS}\n WHERE i.sha256 = ?");
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| PawchiveError::database(&e))?;
            let rows = stmt
                .query_map(params![sha256.to_lowercase()], |r| cached_row(r, true))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|e| PawchiveError::database(&e))?;
            for row in rows {
                let row = row?;
                let key = (
                    row.service.clone(),
                    row.user_id.clone(),
                    row.post_id.clone(),
                );
                match position.get(&key) {
                    Some(&at) => {
                        best[at].1 = 0;
                        best[at].2 = row;
                    }
                    None => {
                        position.insert(key.clone(), best.len());
                        best.push((key.clone(), 0, row));
                    }
                }
                exact_keys.insert(key);
            }
        }

        // `sorted(..., key=(distance, key not in exact_keys))` is stable.
        best.sort_by_key(|(key, distance, _)| (*distance, !exact_keys.contains(key)));
        Ok(best
            .into_iter()
            .take(limit)
            .map(|(key, distance, row)| IndexMatch {
                url: post_url(&row.service, &row.user_id, &row.post_id),
                thumb_url: thumbnail_url(&row.path),
                page_index: row.page_index,
                distance,
                similarity: similarity_for(i64::from(distance)),
                exact: exact_keys.contains(&key),
                artist: row.artist,
                title: row.title,
            })
            .collect())
    }
}

/// One row of [`SEARCH_COLUMNS`]. The hash is `int(r[0], 16)`; the by-SHA-256 read takes a missing
/// or empty one as 0 (`int(r[0], 16) if r[0] else 0`), the fingerprinted read never has one.
fn cached_row(
    r: &rusqlite::Row<'_>,
    lenient: bool,
) -> rusqlite::Result<Result<CachedRow, PawchiveError>> {
    let hex: Option<String> = r.get(0)?;
    let hash = match hex.as_deref() {
        None | Some("") if lenient => Some(Hash([0; 4])),
        Some(text) => Hash::from_hex(text),
        None => None,
    };
    let row = (|| {
        Ok::<_, rusqlite::Error>(CachedRow {
            hash: Hash([0; 4]),
            service: r.get(1)?,
            user_id: r.get(2)?,
            post_id: r.get(3)?,
            page_index: r.get(4)?,
            path: r.get(5)?,
            title: r.get(6)?,
            artist: r.get(7)?,
        })
    })()?;
    Ok(match hash {
        Some(hash) => Ok(CachedRow { hash, ..row }),
        None => Err(PawchiveError::unexpected(format!(
            "invalid literal for int() with base 16: {}",
            crate::pyrepr::py_repr(&serde_json::Value::String(hex.unwrap_or_default()))
        ))),
    })
}
