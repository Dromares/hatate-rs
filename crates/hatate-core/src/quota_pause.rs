//! `saucenao_quota_pause.json`: a batch paused by SauceNAO's daily quota.
//! `docs/file-formats.md` 5.5, rule 6.4; Python: `core/saucenao.py:244-335`,
//! `gui/main_window.py:544-560`.
//!
//! The file is read by whichever app starts next, so the Rust app writes the four exact
//! keys (`paused_at`, `searched`, `remaining`, `reset_at`, in that order) and honours an
//! existing file: a pause the Python app left is still a pause here, and the reverse.
//! Deciding *when* a batch pauses is the SauceNAO engine's (Phase 2); this module is the
//! file.

use std::fs;
use std::io;

use serde_json::Value;

use crate::config::pyjson;
use crate::fsutil;
use crate::paths::Paths;
use crate::pycompat;
use crate::timeutil;

/// `QuotaPauseState`: the four keys of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaPause {
    /// `time.time()` when the batch stopped.
    pub paused_at: f64,
    /// Images searched before pausing.
    pub searched: i64,
    /// Images left unsearched.
    pub remaining: i64,
    /// The next 00:00 UTC, as `datetime.isoformat()` (`2026-10-23T00:00:00+00:00`):
    /// absolute, so it stays right however long it sits unread.
    pub reset_at: String,
}

impl QuotaPause {
    /// A pause starting now, resetting at the next 00:00 UTC (`record_quota_pause`).
    pub fn starting_at(now: f64, searched: i64, remaining: i64) -> QuotaPause {
        QuotaPause {
            paused_at: now,
            searched,
            remaining,
            reset_at: timeutil::next_utc_midnight_iso(now),
        }
    }

    /// `json.dumps(state.__dict__)`: compact, `", "` and `": "`, Python float repr.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\"paused_at\": ");
        pyjson::float(&mut out, self.paused_at);
        out.push_str(&format!(
            ", \"searched\": {}, \"remaining\": {}, \"reset_at\": ",
            self.searched, self.remaining
        ));
        pyjson::string(&mut out, &self.reset_at);
        out.push('}');
        out
    }

    /// `reset_at` as epoch seconds, or `None` when it does not parse as an ISO-8601 time.
    pub fn reset_epoch(&self) -> Option<f64> {
        timeutil::parse_iso(&self.reset_at)
    }
}

/// `get_quota_pause_state`: the stored pause, or `None` when there is no file or it cannot
/// be read ("nothing to show, the same as never having paused"). Python's coercions apply:
/// `float(paused_at)`, `int(searched)`, `int(remaining)`, `str(reset_at)`.
pub fn parse(bytes: &[u8]) -> Option<QuotaPause> {
    let root: Value = serde_json::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    let d = root.as_object()?;
    Some(QuotaPause {
        paused_at: pycompat::float(d.get("paused_at")?)?,
        searched: pycompat::int(d.get("searched")?)?,
        remaining: pycompat::int(d.get("remaining")?)?,
        // `str(x)`: a non-string never parses as a time, so the startup check clears it,
        // whatever Python would have printed for it.
        reset_at: match d.get("reset_at")? {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        },
    })
}

/// What the startup check found.
#[derive(Debug, Clone, PartialEq)]
pub enum StartupPause {
    /// No file, or an unreadable one: nothing to show.
    None,
    /// A pause that is still in force: show it, and keep SauceNAO paused until `reset_at`.
    Active(QuotaPause),
    /// The allowance has come back (or `reset_at` is unparsable): the file was deleted.
    Cleared(QuotaPause),
}

/// The pause file in a profile.
#[derive(Debug, Clone)]
pub struct QuotaPauseFile {
    path: std::path::PathBuf,
}

impl QuotaPauseFile {
    pub fn new(paths: &Paths) -> QuotaPauseFile {
        QuotaPauseFile {
            path: paths.saucenao_quota_pause(),
        }
    }

    /// `get_quota_pause_state`.
    pub fn read(&self) -> Option<QuotaPause> {
        parse(&fs::read(&self.path).ok()?)
    }

    /// `record_quota_pause`, but atomic: the parent directory is created if needed.
    pub fn record(&self, pause: &QuotaPause) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fsutil::write_atomic(&self.path, pause.to_json().as_bytes())
    }

    /// `clear_quota_pause`: called when the user starts a new search. A missing file is fine.
    pub fn clear(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// `_show_persisted_quota_pause`: at startup, honour a pause that is still in force and
    /// delete one that is stale (`reset_at` already past, or unparsable).
    pub fn check_at_startup(&self, now: f64) -> io::Result<StartupPause> {
        let Some(pause) = self.read() else {
            return Ok(StartupPause::None);
        };
        match pause.reset_epoch() {
            Some(reset) if reset > now => Ok(StartupPause::Active(pause)),
            _ => {
                self.clear()?;
                Ok(StartupPause::Cleared(pause))
            }
        }
    }
}
