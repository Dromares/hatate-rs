//! Yandex reverse image search (`core/yandex.py`): the results page and the choice of what to
//! keep.
//!
//! Yandex has no API. Python posts the picture, gets a `cbirId`, and fetches the "sites" tab
//! for it; the page is server-rendered and its results are embedded as JSON in a `data-state`
//! attribute (`initialState.cbirSites.sites`): page URL, domain, a thumbnail and the size of the
//! copy that page holds. [`parse_sites`] reads that block; [`keep_post_pages`] keeps only the
//! results on a post page of a site that has a parser (Pinterest pins, wallpaper hosts and
//! listing or tag-search pages carry nothing to read tags from) and scores them by position.
//!
//! The request half (the upload, the `cbirId`, the captcha rest, the stand-down after three
//! failed pages in a row, the pacing) is Phase 3.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use super::UnexpectedInput;
use crate::boorus;
use crate::pycompat;
use crate::pyurl;
use crate::sites::canonicalize_url;

/// Level with Google Lens's start: same kind of engine, same kind of guess. Below ascii2d's
/// colour search, so neither overturns a measured IQDB or SauceNAO match on a position alone.
pub const SIMILARITY_START: f64 = 80.0;
pub const SIMILARITY_STEP: f64 = 2.0;
pub const SIMILARITY_FLOOR: f64 = 50.0;
pub const MAX_RESULTS: usize = 8;

/// One result worth a row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct YandexMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub source_name: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// A post page, as opposed to a listing, a tag search or a profile. Python compiles it with
/// `re.IGNORECASE`, under which `i` also matches `ı` and `İ` and `[a-z]` matches them too; `(?i)`
/// has neither, so they are spelled out (`ſ` and the Kelvin sign, which Python also adds, `(?i)`
/// already has). `$` is `\n?\z`, since Python's also matches before one final newline.
static POST_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)",
        r"[?&][i\u{131}\u{130}]d=\d+",
        r"|/posts?/(?:show/|v[i\u{131}\u{130}]ew/)?\d+",
        r"|/posts?/[0-9a-z\u{131}\u{130}]{8,}(?:[/?#]|\n?\z)",
        r"|/artworks/\d+|[i\u{131}\u{130}]llust_[i\u{131}\u{130}]d=\d+",
        r"|/status(?:es)?/\d+",
        r"|/art/[^/?#]+-\d+",
        r"|/comments/[0-9a-z\u{131}\u{130}]+",
        r"|zerochan\.net/\d+",
    ))
    .expect("post url regex")
});

/// Danbooru's mirrors (`^(?:[a-z0-9-]+\.)?donmai\.(?:us|moe)$`, `re.IGNORECASE`): Yandex indexed
/// some posts under them, and only the main host has a parser.
static DANBOORU_MIRROR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[a-z0-9\-\u{131}\u{130}]+\.)?donmai\.(?:us|moe)\z")
        .expect("danbooru mirror regex")
});

static DATA_STATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-state="([^"]*)""#).expect("data-state regex"));
static UTM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[?&]utm_[a-z]+=[^&#]*").expect("utm regex"));

/// The "sites" results embedded in a results page, as Yandex gives them. `None` when the page
/// has no results block at all, which is not the same as a block with nothing in it (`Some` of an
/// empty list). A `data-state` that does not parse as JSON is skipped, as Python skips it.
///
/// `Err` where Python raises `AttributeError`: a block that parses to something that is not an
/// object (`5`, `[]`, `null`), or an `initialState` that is neither an object nor empty.
pub fn parse_sites(page: &str) -> Result<Option<Vec<Value>>, UnexpectedInput> {
    for m in DATA_STATE_RE.captures_iter(page) {
        let raw = &m[1];
        if !raw.contains("cbirSites") {
            continue;
        }
        let Ok(state) = serde_json::from_str::<Value>(&pycompat::html_unescape(raw)) else {
            continue;
        };
        let Value::Object(state) = state else {
            return Err(UnexpectedInput);
        };
        // `(state.get("initialState") or {}).get("cbirSites")`
        let block = match state.get("initialState") {
            None => None,
            Some(initial) if !pycompat::truthy(initial) => None,
            Some(Value::Object(initial)) => initial.get("cbirSites"),
            Some(_) => return Err(UnexpectedInput),
        };
        if let Some(Value::Object(block)) = block {
            return Ok(Some(match block.get("sites") {
                Some(Value::Array(sites)) => sites.clone(),
                _ => Vec::new(),
            }));
        }
    }
    Ok(None)
}

/// The results worth a row: a post page on a site with a parser, at most [`MAX_RESULTS`] of them,
/// the same post once, in Yandex's order.
///
/// Scored by position among the ones kept, not among everything Yandex returned: the Pinterest
/// pins ahead of a booru post say nothing about how good the post is.
///
/// `Err` where Python raises on a value it reads without checking: a truthy `url` that is not a
/// string, a `thumb` or `originalImage` that is truthy and not an object, a `thumb.url` that is
/// truthy and not a string, or an `originalImage.url` that is neither a string nor `null` (Python
/// would carry it through; a [`YandexMatch`] cannot).
pub fn keep_post_pages(sites: &[Value]) -> Result<Vec<YandexMatch>, UnexpectedInput> {
    let mut matches: Vec<YandexMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for site in sites {
        let Some(site) = site.as_object() else {
            continue;
        };
        // `post_url(site.get("url") or "")`
        let raw_url = match site.get("url") {
            Some(v) if pycompat::truthy(v) => v.as_str().ok_or(UnexpectedInput)?,
            _ => "",
        };
        let Some(url) = post_url(raw_url) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let thumb = match site.get("thumb") {
            Some(v) if pycompat::truthy(v) => v.as_object().ok_or(UnexpectedInput)?.get("url"),
            _ => None,
        };
        let thumb: Option<String> = match thumb {
            Some(v) if pycompat::truthy(v) => {
                let s = v.as_str().ok_or(UnexpectedInput)?;
                Some(match s.strip_prefix("//") {
                    Some(_) => format!("https:{s}"),
                    None => s.to_string(),
                })
            }
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        let original = match site.get("originalImage") {
            Some(v) if pycompat::truthy(v) => Some(v.as_object().ok_or(UnexpectedInput)?),
            _ => None,
        };
        let field = |name: &str| original.and_then(|o| o.get(name));
        // `thumb or original.get("url")`
        let thumb_url = match thumb {
            Some(t) if !t.is_empty() => Some(t),
            _ => match field("url") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(UnexpectedInput),
            },
        };
        let similarity =
            (SIMILARITY_START - matches.len() as f64 * SIMILARITY_STEP).max(SIMILARITY_FLOOR);
        let source_name = host_label(&url)?;
        matches.push(YandexMatch {
            url,
            thumb_url,
            similarity,
            source_name,
            width: field("width").and_then(pycompat::int),
            height: field("height").and_then(pycompat::int),
        });
        if matches.len() >= MAX_RESULTS {
            break;
        }
    }
    Ok(matches)
}

/// The canonical post URL, or `None` if this is not a post page on a site the booru parsers can
/// read.
pub fn post_url(url: &str) -> Option<String> {
    // Yandex tags every outbound link with its own utm_ parameters.
    let mut url = UTM_RE.replace_all(pycompat::strip(url), "").into_owned();
    if !url.contains('?') && url.contains('&') {
        url = url.replacen('&', "?", 1);
    }
    let mut parts = pyurl::try_urlparse(&url, "").ok()?;
    if DANBOORU_MIRROR_RE.is_match(&parts.netloc) && parts.netloc != "danbooru.donmai.us" {
        parts.netloc = "danbooru.donmai.us".to_string();
        url = pyurl::urlunparse(&parts);
    }
    if !POST_URL_RE.is_match(&url) {
        return None;
    }
    let url = canonicalize_url(&url);
    boorus::find_parser(&url).map(|_| url)
}

/// The host of `url`, lower-cased and without a leading `www.`, or `None` when it has none. `Err`
/// where `urlparse` raises `ValueError`, which Python does not catch here.
fn host_label(url: &str) -> Result<Option<String>, UnexpectedInput> {
    let netloc = pyurl::try_urlparse(url, "")
        .map_err(|_| UnexpectedInput)?
        .netloc
        .to_lowercase();
    let host = netloc.strip_prefix("www.").unwrap_or(&netloc);
    Ok((!host.is_empty()).then(|| host.to_string()))
}
