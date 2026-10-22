//! The search result cache, `search_cache/<sha256>.json`: `docs/file-formats.md` section 4,
//! rules 6.3. Python source of truth: `core/search_cache.py`.
//!
//! One small JSON file per file hash, so one bad file cannot take the cache down.
//! The rules, each with a test:
//!
//! - the key is the file's lowercase SHA-256 hex ([`CacheKey`]);
//! - files carry `format_version: 2`, and every candidate a `tag_version` (`1` for a
//!   MangaDex chapter URL, `0` otherwise);
//! - `error` results and provisional results (found while SauceNAO was skipped) are never
//!   stored ([`StoreOutcome`]);
//! - an unreadable, truncated or malformed file is a miss, never an error ([`Lookup`]);
//! - only `not_found` expires, by `search_cache_ttl_days` from `cached_at`, and the file
//!   is deleted by the read that notices;
//! - writes are atomic (temp file in the same directory, fsync, rename), which the Python
//!   app's are not: a file Python left truncated reads as a miss here.
//!
//! This module never touches the network and does not decide *when* to look up or
//! store; that is the search worker's (Phase 2).

mod model;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsutil::{self, Stage};
use crate::paths::Paths;

pub use model::{
    AVAILABILITY_VERDICT_TTL_DAYS, CACHE_FORMAT_VERSION, CacheEntry, Candidate, Malformed,
    MatchStatus, Tag, entry_from_value, entry_to_json, tag_version_for,
};

/// A file hash that is safe to use as a file name: exactly 64 lowercase hex digits.
///
/// The Python app does not validate the key, and an uppercase one would land in a
/// different file there; the Rust app refuses anything but the canonical form so that a
/// hash can never be a path (`../x`) and both apps always agree on the file name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

/// The text was not 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidKey;

impl std::fmt::Display for InvalidKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a cache key is 64 lowercase hex digits (the file's SHA-256)")
    }
}

impl std::error::Error for InvalidKey {}

impl CacheKey {
    pub fn parse(text: &str) -> Result<CacheKey, InvalidKey> {
        if text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(CacheKey(text.to_owned()))
        } else {
            Err(InvalidKey)
        }
    }

    /// From a SHA-256 digest (`hashlib.sha256(...).hexdigest()` is the lowercase hex).
    pub fn from_digest(digest: &[u8; 32]) -> CacheKey {
        let mut s = String::with_capacity(64);
        for b in digest {
            s.push_str(&format!("{b:02x}"));
        }
        CacheKey(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why a lookup did not return an entry. All of them are "miss" to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissReason {
    /// No file.
    Absent,
    /// The file could not be read, or is not valid UTF-8 JSON (truncated, corrupt).
    Unreadable,
    /// Valid JSON that is not a usable entry (spec 4: malformed entries are ignored).
    Malformed(Malformed),
    /// A `not_found` older than the TTL; the file was deleted.
    Expired,
}

/// The result of [`SearchCache::lookup`].
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Hit(CacheEntry),
    Miss(MissReason),
}

impl Lookup {
    pub fn hit(self) -> Option<CacheEntry> {
        match self {
            Lookup::Hit(e) => Some(e),
            Lookup::Miss(_) => None,
        }
    }
}

/// What [`SearchCache::store`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    Written,
    /// `status == error`: retried normally next time, not remembered.
    SkippedError,
    /// Found while SauceNAO was skipped: weaker than a full search, so not handed back
    /// tomorrow as if it were the real answer.
    SkippedProvisional,
}

/// The cache directory.
#[derive(Debug, Clone)]
pub struct SearchCache {
    dir: PathBuf,
}

impl SearchCache {
    pub fn new(paths: &Paths) -> SearchCache {
        SearchCache {
            dir: paths.search_cache_dir(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, key: &CacheKey) -> PathBuf {
        self.dir.join(format!("{}.json", key.as_str()))
    }

    /// `load_cached_result` + `apply_cached_result`: the entry for this hash, or a miss.
    ///
    /// `ttl_days` is `search_cache_ttl_days` (0 or less: nothing expires); `now` is epoch
    /// seconds (`time.time()`). Never fails: a file that cannot be read is a miss.
    pub fn lookup(&self, key: &CacheKey, ttl_days: f64, now: f64) -> Lookup {
        let path = self.path_for(key);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Lookup::Miss(MissReason::Absent);
            }
            Err(_) => return Lookup::Miss(MissReason::Unreadable),
        };
        let root: serde_json::Value = match std::str::from_utf8(&bytes)
            .ok()
            .and_then(|t| serde_json::from_str(t).ok())
        {
            Some(v) => v,
            None => return Lookup::Miss(MissReason::Unreadable),
        };
        if is_expired(&root, ttl_days, now) {
            // Pruned on the way out, so the cache cleans itself as it is used.
            let _ = fs::remove_file(&path);
            return Lookup::Miss(MissReason::Expired);
        }
        match entry_from_value(&root, now) {
            Ok(entry) => Lookup::Hit(entry),
            Err(m) => Lookup::Miss(MissReason::Malformed(m)),
        }
    }

    /// `save_cached_result`: write the entry, atomically, unless it must not be cached.
    ///
    /// `searched_without_saucenao` is the entry's provisional flag (spec 4): it is not a
    /// field of the file.
    pub fn store(
        &self,
        key: &CacheKey,
        entry: &CacheEntry,
        searched_without_saucenao: bool,
    ) -> io::Result<StoreOutcome> {
        self.store_until(key, entry, searched_without_saucenao, None)
    }

    pub(crate) fn store_until(
        &self,
        key: &CacheKey,
        entry: &CacheEntry,
        searched_without_saucenao: bool,
        until: Option<Stage>,
    ) -> io::Result<StoreOutcome> {
        if entry.status == MatchStatus::Error {
            return Ok(StoreOutcome::SkippedError);
        }
        if searched_without_saucenao {
            return Ok(StoreOutcome::SkippedProvisional);
        }
        fs::create_dir_all(&self.dir)?;
        let json = entry_to_json(entry);
        fsutil::write_atomic_until(&self.path_for(key), json.as_bytes(), until)?;
        Ok(StoreOutcome::Written)
    }

    /// `drop_cached_result`: forget one hash. True when there was a file to forget.
    pub fn drop_entry(&self, key: &CacheKey) -> io::Result<bool> {
        match fs::remove_file(self.path_for(key)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// `clear_cache`: remove every `*.json` (and the temp files a killed run left), return
    /// how many entries went. Other files in the directory are not touched.
    pub fn clear(&self) -> io::Result<usize> {
        let mut removed = 0;
        let rd = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        for item in rd {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && !name.starts_with('.') {
                if fs::remove_file(item.path()).is_ok() {
                    removed += 1;
                }
            } else if fsutil::is_temp_leftover(&name) {
                let _ = fs::remove_file(item.path());
            }
        }
        Ok(removed)
    }

    /// `get_cache_size_bytes`: the total size of the `*.json` files; 0 when there is no
    /// directory.
    pub fn size_bytes(&self) -> u64 {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return 0;
        };
        rd.filter_map(Result::ok)
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.ends_with(".json") && !n.starts_with('.')
            })
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }
}

/// `_is_expired`: only `not_found` ages out; a missing or non-numeric `cached_at` on such
/// an entry counts as expired.
fn is_expired(root: &serde_json::Value, ttl_days: f64, now: f64) -> bool {
    if ttl_days <= 0.0 {
        return false;
    }
    let Some(obj) = root.as_object() else {
        return false;
    };
    if obj.get("status").and_then(|v| v.as_str()) != Some("not_found") {
        return false;
    }
    match obj.get("cached_at").and_then(crate::pycompat::number) {
        None => true,
        Some(at) => now - at > ttl_days * 86_400.0,
    }
}
