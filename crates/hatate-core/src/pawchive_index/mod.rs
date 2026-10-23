//! `core/pawchive_index.py`: a local index of chosen Pawchive artists, for finding EDITED copies
//! of a picture (Phase 4.f, DAN-2128), and `workers/pawchive_index_worker.py` without Qt.
//!
//! Pawchive's exact lookup (`engines::pawchive_lookup`) finds byte-identical files by SHA-256. A
//! resized or re-saved copy has another hash, and the site offers no search by picture, so this
//! keeps a picture index of its own for the artists the user picks: each image's post, artist and
//! SHA-256 (read off its path, which IS the hash) and two fingerprints of its thumbnail. Searching
//! is local and costs no request.
//!
//! - [`PawchiveIndex`] (`db`): the SQLite file, at Python's path with Python's schema. Python's
//!   `PawchiveIndex` opens a file written here and the reverse (`docs/pawchive-index.md`).
//! - [`PawchiveCrawler`] (`crawl`): the polite crawl ([`PawchiveCrawler::index_creator`]), the
//!   artist search ([`PawchiveCrawler::find_creators`]) and the two worker bodies.
//! - [`fingerprints`], [`similarity_for`], [`creator_from_url`] and the result types are here.
//!
//! **Transport.** Python crawls with a plain `requests.Session` of its own, not `core/net.py`:
//! no host wait and no net-layer 429 retry. The crawl has its own pause (one request a second, a
//! 429 waited out) and its own Stop check every 0.2 s, so the crawler does the same through a
//! [`Clock`] and a [`NetClient`] with `RetryMode::Never` and no host gate. The hosts are injected
//! through the crawler's constructor and nothing in a binary redirects them.
//!
//! **Errors (DAN-2116 item 1).** [`PawchiveError`] has `kind()`, `message()` (Python's `str(exc)`),
//! `user_message()` (what failed and what to do; no URL, query, path or OS text) and `detail()`.
//! `CrawlResult.error` stays the one string Python writes, with the kind alongside it.

mod crawl;
mod db;
mod json_error;

pub use db::{ImageRow, SCHEMA};

use std::fmt;

use serde_json::Value;

use crate::error::ErrorKind;
use crate::image_compare::{Hash, Source, dhash_pair};
use crate::net::{self, NetError};
use crate::pyimage::DecompressionBomb;

pub use crawl::{
    CreatorFinder, CreatorIndexer, IndexJobsOutcome, PawchiveCrawler, find_creators_for_ui,
    run_index_jobs,
};
pub use db::PawchiveIndex;

pub const API: &str = "https://pawchive.pw/api/v1";
/// `pawchive_lookup.POST_URL`, with `{service}`, `{user}` and `{post}` filled in by [`post_url`].
pub const POST_URL: &str = "https://pawchive.pw/{service}/user/{user}/post/{post}";
pub use crate::engines::pawchive_lookup::THUMBNAIL_URL;
pub use crate::net::USER_AGENT;

/// Posts per listing page: the API's own size.
pub const PAGE_SIZE: usize = 50;
/// Seconds between requests: an archive, not a CDN.
pub const REQUEST_DELAY: f64 = 1.0;
pub const MAX_RETRIES: usize = 3;
/// After a 429 that names no usable `Retry-After`.
pub const RETRY_WAIT: f64 = 30.0;
pub const CREATORS_CACHE_MAX_AGE: f64 = 7.0 * 24.0 * 3600.0;
pub const IMAGE_EXTENSIONS: [&str; 8] = [
    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".avif", ".jfif",
];

/// Distance on the 256-bit hash -> a percentage that reads like the rest of the app, where 90+
/// means "the same picture" (anchors measured on real Pawchive posts, `core/pawchive_index.py`).
pub const SIMILARITY_ANCHORS: [(i64, f64); 4] = [(0, 100.0), (6, 90.0), (24, 70.0), (128, 0.0)];
/// Past this it is a different picture.
pub const MAX_MATCH_DISTANCE: u32 = 32;
pub const MAX_RESULTS: usize = 5;

/// `Creator`: one indexed artist.
#[derive(Debug, Clone, PartialEq)]
pub struct Creator {
    pub service: String,
    pub user_id: String,
    pub name: Option<String>,
    pub indexed_at: Option<f64>,
    pub post_count: i64,
    pub image_count: i64,
    /// Images Pawchive had no thumbnail for.
    pub unfingerprinted: i64,
}

impl Creator {
    /// `Creator.label`: `"name (service)"`, the user id standing in for an empty name.
    pub fn label(&self) -> String {
        let shown = match self.name.as_deref() {
            Some(name) if !name.is_empty() => name,
            _ => &self.user_id,
        };
        format!("{shown} ({})", self.service)
    }
}

/// `IndexMatch`: one indexed post that looks like the local image.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexMatch {
    pub url: String,
    pub thumb_url: String,
    pub page_index: i64,
    /// On the 256-bit hash; 0 for an exact copy.
    pub distance: u32,
    pub similarity: f64,
    /// Same SHA-256: the very same file.
    pub exact: bool,
    pub artist: Option<String>,
    pub title: Option<String>,
}

/// `CrawlResult`: how indexing one artist went.
#[derive(Debug, Clone, PartialEq)]
pub struct CrawlResult {
    pub service: String,
    pub user_id: String,
    pub name: Option<String>,
    pub posts: usize,
    pub images: usize,
    pub new: usize,
    pub unfingerprinted: usize,
    pub stopped: bool,
    /// Python's `error` string, which the Index window shows.
    pub error: Option<String>,
    /// What kind of failure `error` is (DAN-2116). Never written anywhere.
    pub error_kind: Option<ErrorKind>,
}

impl CrawlResult {
    /// `CrawlResult(service=..., user_id=...)`: nothing found, nothing wrong.
    pub fn default_for(service: &str, user_id: &str) -> Self {
        Self::new(service, user_id)
    }

    fn new(service: &str, user_id: &str) -> Self {
        CrawlResult {
            service: service.to_owned(),
            user_id: user_id.to_owned(),
            name: None,
            posts: 0,
            images: 0,
            new: 0,
            unfingerprinted: 0,
            stopped: false,
            error: None,
            error_kind: None,
        }
    }

    /// What to show a person about `error`: no URL, query, path or OS text. `None` when it went well.
    pub fn error_user_message(&self) -> Option<String> {
        self.error_kind.map(user_message_for)
    }
}

/// Anything that stopped a Pawchive call from doing what it was asked: a failed request, an
/// answer that is not shaped the way the code reads it, or a database that cannot be used.
///
/// Where Python's own handler catches the failure (`requests.RequestException`, `ValueError`) the
/// crawl reports it in [`CrawlResult::error`]; the rest is returned as an `Err`, as it escapes
/// Python.
#[derive(Debug, Clone)]
pub struct PawchiveError {
    kind: ErrorKind,
    status_code: Option<u16>,
    message: String,
    detail: String,
}

impl PawchiveError {
    /// An error of a kind with Python's text for it (also what a stand-in [`CreatorIndexer`] fails with).
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        let message = message.into();
        PawchiveError {
            kind,
            status_code: None,
            detail: message.clone(),
            message,
        }
    }

    pub(crate) fn status(status: u16, message: impl Into<String>) -> Self {
        PawchiveError {
            status_code: Some(status),
            ..Self::new(ErrorKind::HttpStatus, message)
        }
    }

    pub(crate) fn net(err: &NetError) -> Self {
        PawchiveError {
            kind: ErrorKind::Net(err.kind()),
            status_code: None,
            message: err.to_string(),
            detail: format!("{} [{:?}] {}", err, err.kind(), err.detail()),
        }
    }

    /// A body whose shape is not the one the code reads: Python's `AttributeError` or `TypeError`,
    /// with its text.
    pub(crate) fn unexpected(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnexpectedResponse, message)
    }

    pub(crate) fn database(err: &rusqlite::Error) -> Self {
        // `rusqlite` ends the text of a failed open with `: <path of the file>`; `sqlite3` says
        // only "unable to open database file", which is what Python's message carries. The path
        // stays in `detail`.
        let message = match err {
            rusqlite::Error::SqliteFailure(failure, _)
                if failure.code == rusqlite::ErrorCode::CannotOpen =>
            {
                "unable to open database file".to_owned()
            }
            _ => err.to_string(),
        };
        PawchiveError {
            kind: ErrorKind::Database,
            status_code: None,
            message,
            detail: format!("{err:?}"),
        }
    }

    pub(crate) fn io(err: &std::io::Error, path: &std::path::Path) -> Self {
        PawchiveError {
            kind: ErrorKind::Io,
            status_code: None,
            message: crate::image_prep::os_error_text(err, path),
            detail: format!("{err:?} {}", path.display()),
        }
    }

    pub(crate) fn bomb(err: &DecompressionBomb) -> Self {
        Self::new(ErrorKind::UnexpectedResponse, err.to_string())
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The HTTP status Pawchive answered with, where there was one.
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    /// Python's `str(exc)`: the text `CrawlResult.error` carries.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What to show a person: what failed and what to do about it.
    pub fn user_message(&self) -> String {
        user_message_for(self.kind)
    }

    /// The raw text for a log.
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl fmt::Display for PawchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PawchiveError {}

fn user_message_for(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::Net(net::ErrorKind::Timeout) | ErrorKind::Net(net::ErrorKind::DeadlineExceeded) => {
            "Pawchive did not answer in time. Try again in a few minutes."
        }
        ErrorKind::Net(net::ErrorKind::Tls) => {
            "Could not make a secure connection to Pawchive. Check the date and the certificates on this computer."
        }
        ErrorKind::Net(_) => "Could not reach Pawchive. Check the internet connection and try again.",
        ErrorKind::NotFound => "Pawchive has no such artist. Check the link and try again.",
        ErrorKind::HttpStatus => "Pawchive answered with an error. Try again in a few minutes.",
        ErrorKind::UnexpectedResponse => {
            "Pawchive sent an answer Hatate cannot read. Try again later."
        }
        ErrorKind::Database => {
            "The Pawchive index could not be read or written. Check that its file is not locked or damaged."
        }
        ErrorKind::Io => {
            "A Pawchive file could not be read or written. Check that the folder exists and can be written to."
        }
        _ => "The Pawchive request failed. Try again.",
    }
    .to_owned()
}

/// `similarity_for`: the distance on the 256-bit hash as a percentage, interpolated between
/// [`SIMILARITY_ANCHORS`] and rounded to one place as Python's `round(x, 1)` does.
pub fn similarity_for(distance: i64) -> f64 {
    if distance <= 0 {
        return 100.0;
    }
    for pair in SIMILARITY_ANCHORS.windows(2) {
        let (low_d, low_pct) = pair[0];
        let (high_d, high_pct) = pair[1];
        if distance <= high_d {
            let raw = low_pct
                + (high_pct - low_pct) * (distance - low_d) as f64 / (high_d - low_d) as f64;
            return round1(raw);
        }
    }
    0.0
}

/// `round(x, 1)`: Python rounds the exact binary value to the nearest tenth, ties to even, which
/// is what Rust's fixed-precision formatting does.
fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

/// `fingerprints`: the 64-bit and the 256-bit difference hash of a picture, decoding it once.
/// `Ok((None, None))` is a picture that could not be read; the `Err` is the
/// `DecompressionBombError` Python lets through.
pub fn fingerprints(source: Source<'_>) -> Result<(Option<Hash>, Option<Hash>), DecompressionBomb> {
    Ok(match dhash_pair(source)? {
        Some((h8, h16)) => (Some(h8), Some(h16)),
        None => (None, None),
    })
}

/// `creator_from_url`: `(service, user id)` from any Pawchive artist or post link.
pub fn creator_from_url(text: &str) -> Option<(String, String)> {
    use regex::Regex;
    use std::sync::LazyLock;
    static CREATOR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"pawchive\.pw/([A-Za-z0-9_.-]+)/user/([^/?#\s]+)").expect("creator url regex")
    });
    let caps = CREATOR_URL_RE.captures(text)?;
    Some((caps[1].to_owned(), caps[2].to_owned()))
}

/// `POST_URL.format(service=..., user=..., post=...)`.
pub fn post_url(service: &str, user: &str, post: &str) -> String {
    format!("https://pawchive.pw/{service}/user/{user}/post/{post}")
}

/// `THUMBNAIL_URL.format(path=...)`.
pub fn thumbnail_url(path: &str) -> String {
    format!("{THUMBNAIL_URL}{path}")
}

/// A JSON value as Python's `str()` prints it, for ids that are not strings.
pub(crate) fn py_text(v: &Value) -> String {
    crate::pyrepr::py_str(v)
}
