//! The search-cache entry and its JSON, `docs/file-formats.md` sections 3.4.1 and 4.
//!
//! Reading copies `core/search_cache.py` (`_candidate_from_dict`, `apply_cached_result`):
//! the same required keys, the same defaults, the same coercions. Writing copies
//! `save_cached_result` and `json.dumps(data)`: six top-level keys, twenty candidate
//! keys, in that order, one line, `", "` and `": "` separators, non-ASCII escaped.

use serde_json::{Map, Value};

use crate::config::pyjson;
use crate::pycompat;

/// `CACHE_FORMAT_VERSION` (`core/search_cache.py:42`).
pub const CACHE_FORMAT_VERSION: i64 = 2;

/// How long a saved availability verdict stands (`AVAILABILITY_VERDICT_TTL_DAYS`, `:149`).
pub const AVAILABILITY_VERDICT_TTL_DAYS: f64 = 30.0;

/// `MatchStatus` (`core/models.py:27-33`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchStatus {
    NotSearched,
    Good,
    Poor,
    NotFound,
    Error,
    Searching,
}

impl MatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchStatus::NotSearched => "not_searched",
            MatchStatus::Good => "good",
            MatchStatus::Poor => "poor",
            MatchStatus::NotFound => "not_found",
            MatchStatus::Error => "error",
            MatchStatus::Searching => "searching",
        }
    }

    pub fn parse(s: &str) -> Option<MatchStatus> {
        Some(match s {
            "not_searched" => MatchStatus::NotSearched,
            "good" => MatchStatus::Good,
            "poor" => MatchStatus::Poor,
            "not_found" => MatchStatus::NotFound,
            "error" => MatchStatus::Error,
            "searching" => MatchStatus::Searching,
            _ => return None,
        })
    }
}

/// A tag as the cache stores it inside a candidate: no `source` (it is implied by the
/// array it sits in, spec 3.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub namespace: Option<String>,
}

/// A candidate match: the 20 keys of spec 3.4.1, plus two flags the reader derives.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub url: String,
    pub source_name: Option<String>,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub similarity_measured: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub engine: String,
    pub engine_tags: Vec<Tag>,
    pub booru_tags: Vec<Tag>,
    pub direct_file_url: Option<String>,
    pub preview_url: Option<String>,
    pub remote_format: Option<String>,
    pub remote_size_bytes: Option<i64>,
    pub incomplete_reason: Option<String>,
    pub tags_borrowed_from: Option<String>,
    /// Tri-state; `None` is "never checked", not "alive" (spec 3.4.1).
    pub remote_available: Option<bool>,
    pub availability_checked_at: Option<f64>,
    pub restricted: Option<String>,
    /// Derived on read, never stored: the entry was written by the current format *and* its
    /// stored `tag_version` equals the parser's current one. False means "re-read the
    /// booru page for tags" (spec 4).
    pub booru_tags_fetched: bool,
    /// Derived on read, never stored: the entry was written by the current format.
    pub remote_info_fetched: bool,
}

impl Candidate {
    /// A candidate with Python's defaults (`MatchCandidate`), for callers that build one.
    pub fn new(url: impl Into<String>) -> Candidate {
        Candidate {
            url: url.into(),
            source_name: None,
            thumb_url: None,
            similarity: 0.0,
            similarity_measured: false,
            width: None,
            height: None,
            engine: "IQDB".to_owned(),
            engine_tags: Vec::new(),
            booru_tags: Vec::new(),
            direct_file_url: None,
            preview_url: None,
            remote_format: None,
            remote_size_bytes: None,
            incomplete_reason: None,
            tags_borrowed_from: None,
            remote_available: None,
            availability_checked_at: None,
            restricted: None,
            booru_tags_fetched: false,
            remote_info_fetched: false,
        }
    }
}

/// One cached search result: the six top-level keys of spec 4.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    /// The `format_version` found in the file; always [`CACHE_FORMAT_VERSION`] when written.
    pub format_version: i64,
    /// Epoch seconds. Reading keeps `None` when the key is missing or not a number.
    pub cached_at: Option<f64>,
    pub status: MatchStatus,
    pub error_message: Option<String>,
    /// Always a valid index when there are candidates, else 0 (an out-of-range stored
    /// value reads as 0, `apply_cached_result`).
    pub selected_candidate_index: usize,
    pub candidates: Vec<Candidate>,
}

impl CacheEntry {
    /// An entry to store now.
    pub fn new(status: MatchStatus, candidates: Vec<Candidate>, now: f64) -> CacheEntry {
        CacheEntry {
            format_version: CACHE_FORMAT_VERSION,
            cached_at: Some(now),
            status,
            error_message: None,
            selected_candidate_index: 0,
            candidates,
        }
    }

    /// True when the file was written before the current format. The candidates are kept
    /// (the rate-limited part) and their page details are re-read.
    pub fn predates_current_format(&self) -> bool {
        self.format_version < CACHE_FORMAT_VERSION
    }
}

/// Why a file did not become an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malformed {
    /// Not a JSON object at the top.
    NotAnObject,
    /// `status` is a value `MatchStatus` does not have.
    Status,
    /// `format_version` or a `tag_version` that `int()` would reject.
    Number,
    /// `candidates` is not an array, or an element is not an object.
    Candidates,
    /// A candidate without a string `url`, or a tag without a string `name`.
    MissingRequired,
}

/// The parser `TAG_VERSION` for a URL (`_tag_version_for`): `1` for a MangaDex chapter
/// URL, `0` for everything else (spec 4: only MangaDex declares one,
/// `core/boorus/mangadex.py:62`).
pub fn tag_version_for(url: &str) -> i64 {
    i64::from(is_mangadex_chapter(url))
}

/// `mangadex.matches` (`core/boorus/mangadex.py:73-79`): the host (the whole `netloc`,
/// lower-cased) is `mangadex.org` or a sub-domain, and the URL contains
/// `/chapter/<uuid>`.
fn is_mangadex_chapter(url: &str) -> bool {
    let Some(netloc) = netloc(url) else {
        return false;
    };
    let host = netloc.to_ascii_lowercase();
    (host == "mangadex.org" || host.ends_with(".mangadex.org")) && has_chapter_uuid(url)
}

/// `urlparse(url).netloc`: after `scheme:` and `//`, up to the next `/`, `?` or `#`.
fn netloc(url: &str) -> Option<&str> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    if !first_ok || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let rest = url[colon + 1..].strip_prefix("//")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// `re.search("/chapter/<8>-<4>-<4>-<4>-<12 hex>")`.
fn has_chapter_uuid(url: &str) -> bool {
    const SHAPE: [usize; 5] = [8, 4, 4, 4, 12];
    let mut from = 0;
    while let Some(i) = url[from..].find("/chapter/") {
        let mut rest = &url.as_bytes()[from + i + "/chapter/".len()..];
        let mut ok = true;
        for (n, len) in SHAPE.iter().enumerate() {
            let need = len + usize::from(n > 0);
            if rest.len() < need
                || (n > 0 && rest[0] != b'-')
                || !rest[usize::from(n > 0)..need]
                    .iter()
                    .all(u8::is_ascii_hexdigit)
            {
                ok = false;
                break;
            }
            rest = &rest[need..];
        }
        if ok {
            return true;
        }
        from += i + 1;
    }
    false
}

// ---------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------

/// `int(d.get(key, 0) or 0)`.
fn int_or_zero(d: &Map<String, Value>, key: &str) -> Result<i64, Malformed> {
    match d.get(key) {
        None => Ok(0),
        Some(v) if !pycompat::truthy(v) => Ok(0),
        Some(v) => pycompat::int(v).ok_or(Malformed::Number),
    }
}

fn opt_string(d: &Map<String, Value>, key: &str) -> Option<String> {
    d.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn opt_int(d: &Map<String, Value>, key: &str) -> Option<i64> {
    d.get(key).and_then(|v| match v {
        Value::Number(n) => n.as_i64(),
        _ => None,
    })
}

fn tags(d: &Map<String, Value>, key: &str) -> Result<Vec<Tag>, Malformed> {
    let Some(v) = d.get(key) else {
        return Ok(Vec::new());
    };
    let items = v.as_array().ok_or(Malformed::Candidates)?;
    items
        .iter()
        .map(|t| {
            let t = t.as_object().ok_or(Malformed::Candidates)?;
            let name = t
                .get("name")
                .and_then(Value::as_str)
                .ok_or(Malformed::MissingRequired)?;
            Ok(Tag {
                name: name.to_owned(),
                namespace: opt_string(t, "namespace"),
            })
        })
        .collect()
}

/// `_restore_availability`: accepted only as a real bool *and* a numeric timestamp not
/// older than 30 days; anything else is unknown.
fn restore_availability(d: &Map<String, Value>, now: f64) -> (Option<bool>, Option<f64>) {
    let Some(verdict) = d.get("remote_available").and_then(Value::as_bool) else {
        return (None, None);
    };
    let Some(checked) = d.get("availability_checked_at").and_then(|v| match v {
        // `isinstance(x, (int, float)) and not isinstance(x, bool)`
        Value::Number(n) => n.as_f64(),
        _ => None,
    }) else {
        return (None, None);
    };
    if now - checked > AVAILABILITY_VERDICT_TTL_DAYS * 86_400.0 {
        return (None, None);
    }
    (Some(verdict), Some(checked))
}

fn candidate_from(v: &Value, format_version: i64, now: f64) -> Result<Candidate, Malformed> {
    let d = v.as_object().ok_or(Malformed::Candidates)?;
    let url = d
        .get("url")
        .and_then(Value::as_str)
        .ok_or(Malformed::MissingRequired)?
        .to_owned();
    let stored_tag_version = int_or_zero(d, "tag_version")?;
    let tags_current = stored_tag_version == tag_version_for(&url);
    let fetched = format_version >= CACHE_FORMAT_VERSION;
    let (remote_available, availability_checked_at) = restore_availability(d, now);
    Ok(Candidate {
        source_name: opt_string(d, "source_name"),
        thumb_url: opt_string(d, "thumb_url"),
        similarity: d
            .get("similarity")
            .and_then(pycompat::number)
            .unwrap_or(0.0),
        similarity_measured: d.get("similarity_measured").is_some_and(pycompat::truthy),
        width: opt_int(d, "width"),
        height: opt_int(d, "height"),
        engine: d
            .get("engine")
            .and_then(Value::as_str)
            .unwrap_or("IQDB")
            .to_owned(),
        engine_tags: tags(d, "engine_tags")?,
        booru_tags: tags(d, "booru_tags")?,
        direct_file_url: opt_string(d, "direct_file_url"),
        preview_url: opt_string(d, "preview_url"),
        remote_format: opt_string(d, "remote_format"),
        remote_size_bytes: opt_int(d, "remote_size_bytes"),
        incomplete_reason: opt_string(d, "incomplete_reason"),
        tags_borrowed_from: opt_string(d, "tags_borrowed_from"),
        remote_available,
        availability_checked_at,
        restricted: opt_string(d, "restricted"),
        booru_tags_fetched: fetched && tags_current,
        remote_info_fetched: fetched,
        url,
    })
}

/// Turn parsed JSON into an entry. `now` (epoch seconds) is needed only for the 30-day
/// availability-verdict rule.
pub fn entry_from_value(root: &Value, now: f64) -> Result<CacheEntry, Malformed> {
    let d = root.as_object().ok_or(Malformed::NotAnObject)?;
    // `MatchStatus(v) if v else NOT_FOUND`
    let status = match d.get("status") {
        None => MatchStatus::NotFound,
        Some(v) if !pycompat::truthy(v) => MatchStatus::NotFound,
        Some(v) => v
            .as_str()
            .and_then(MatchStatus::parse)
            .ok_or(Malformed::Status)?,
    };
    let format_version = int_or_zero(d, "format_version")?;
    let candidates = match d.get("candidates") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or(Malformed::Candidates)?
            .iter()
            .map(|c| candidate_from(c, format_version, now))
            .collect::<Result<Vec<_>, _>>()?,
    };
    // `0 <= i < len(candidates)` else 0. A stored non-number is not caught by Python
    // (a crash); here it reads as 0.
    let stored = d
        .get("selected_candidate_index")
        .and_then(pycompat::int)
        .unwrap_or(0);
    let selected_candidate_index = usize::try_from(stored)
        .ok()
        .filter(|i| *i < candidates.len())
        .unwrap_or(0);
    Ok(CacheEntry {
        format_version,
        cached_at: d.get("cached_at").and_then(pycompat::number),
        status,
        error_message: opt_string(d, "error_message"),
        selected_candidate_index,
        candidates,
    })
}

// ---------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------

struct Obj<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> Obj<'a> {
    fn open(out: &'a mut String) -> Self {
        out.push('{');
        Obj { out, first: true }
    }
    fn key(&mut self, k: &str) -> &mut String {
        if !self.first {
            self.out.push_str(", ");
        }
        self.first = false;
        pyjson::string(self.out, k);
        self.out.push_str(": ");
        self.out
    }
    fn close(self) {
        self.out.push('}');
    }
}

fn put_opt_str(o: &mut Obj, k: &str, v: &Option<String>) {
    let out = o.key(k);
    match v {
        Some(s) => pyjson::string(out, s),
        None => out.push_str("null"),
    }
}

fn put_opt_int(o: &mut Obj, k: &str, v: Option<i64>) {
    let out = o.key(k);
    match v {
        Some(n) => out.push_str(&n.to_string()),
        None => out.push_str("null"),
    }
}

fn put_tags(o: &mut Obj, k: &str, tags: &[Tag]) {
    let out = o.key(k);
    out.push('[');
    for (i, t) in tags.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let mut to = Obj::open(out);
        let s = to.key("name");
        pyjson::string(s, &t.name);
        put_opt_str(&mut to, "namespace", &t.namespace);
        to.close();
    }
    out.push(']');
}

fn write_candidate(out: &mut String, c: &Candidate) {
    let mut o = Obj::open(out);
    pyjson::string(o.key("url"), &c.url);
    put_opt_str(&mut o, "source_name", &c.source_name);
    put_opt_str(&mut o, "thumb_url", &c.thumb_url);
    pyjson::float(o.key("similarity"), c.similarity);
    o.key("similarity_measured")
        .push_str(if c.similarity_measured {
            "true"
        } else {
            "false"
        });
    put_opt_int(&mut o, "width", c.width);
    put_opt_int(&mut o, "height", c.height);
    pyjson::string(o.key("engine"), &c.engine);
    put_tags(&mut o, "engine_tags", &c.engine_tags);
    put_tags(&mut o, "booru_tags", &c.booru_tags);
    put_opt_str(&mut o, "direct_file_url", &c.direct_file_url);
    put_opt_str(&mut o, "preview_url", &c.preview_url);
    put_opt_str(&mut o, "remote_format", &c.remote_format);
    put_opt_int(&mut o, "remote_size_bytes", c.remote_size_bytes);
    put_opt_str(&mut o, "incomplete_reason", &c.incomplete_reason);
    put_opt_str(&mut o, "tags_borrowed_from", &c.tags_borrowed_from);
    let out = o.key("remote_available");
    out.push_str(match c.remote_available {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    });
    let out = o.key("availability_checked_at");
    match c.availability_checked_at {
        Some(t) => pyjson::float(out, t),
        None => out.push_str("null"),
    }
    put_opt_str(&mut o, "restricted", &c.restricted);
    // The parser's *current* TAG_VERSION for this URL, not a stored value.
    o.key("tag_version")
        .push_str(&tag_version_for(&c.url).to_string());
    o.close();
}

/// The bytes the Python app would write with `json.dumps(data)` for this entry. The
/// `format_version` is always the current one; `cached_at` falls back to 0.0.
pub fn entry_to_json(e: &CacheEntry) -> String {
    let mut out = String::new();
    let mut o = Obj::open(&mut out);
    o.key("format_version")
        .push_str(&CACHE_FORMAT_VERSION.to_string());
    pyjson::float(o.key("cached_at"), e.cached_at.unwrap_or(0.0));
    pyjson::string(o.key("status"), e.status.as_str());
    put_opt_str(&mut o, "error_message", &e.error_message);
    o.key("selected_candidate_index")
        .push_str(&e.selected_candidate_index.to_string());
    let list = o.key("candidates");
    list.push('[');
    for (i, c) in e.candidates.iter().enumerate() {
        if i > 0 {
            list.push_str(", ");
        }
        write_candidate(list, c);
    }
    list.push(']');
    o.close();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangadex_tag_version_matches_the_python_parser() {
        let id = "0123abcd-4567-89ab-cdef-0123456789AB";
        for (url, want) in [
            (format!("https://mangadex.org/chapter/{id}"), 1),
            (format!("https://mangadex.org/chapter/{id}/3"), 1),
            (format!("https://www.mangadex.org/chapter/{id}"), 1),
            (format!("HTTPS://MangaDex.ORG/chapter/{id}"), 1),
            (format!("https://mangadex.org/x?u=/chapter/{id}"), 1),
            // netloc includes the port, so the host test fails (Python quirk).
            (format!("https://mangadex.org:443/chapter/{id}"), 0),
            (
                "https://mangadex.org/title/0123abcd-4567-89ab-cdef-0123456789ab".into(),
                0,
            ),
            ("https://mangadex.org/chapter/not-a-uuid".into(), 0),
            (format!("https://evilmangadex.org/chapter/{id}"), 0),
            (format!("https://mangadex.org.evil.com/chapter/{id}"), 0),
            (format!("mangadex.org/chapter/{id}"), 0),
            ("https://danbooru.donmai.us/posts/1".into(), 0),
            (String::new(), 0),
        ] {
            assert_eq!(tag_version_for(&url), want, "{url}");
        }
    }
}
