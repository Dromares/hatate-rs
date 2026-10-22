//! MangaDex chapters (`core/boorus/mangadex.py`).
//!
//! MangaDex was classification-only for a long time, because a chapter URL points at many images
//! rather than one. That stopped being true when multi-page support arrived: "which of these
//! images is the one I have?" is what the multi-page resolver answers, by hashing each page
//! against the local file. A chapter is a multi-page post like any other.
//!
//! Everything comes from the public API at `api.mangadex.org` rather than the site's HTML, which
//! is a JS front end with nothing useful in the served markup. `/at-home/server/{chapterId}`
//! returns the node serving that chapter plus its page list, in two resolutions:
//!
//! - `data`: the original scans;
//! - `dataSaver`: the same pages recompressed, a fraction of the size.
//!
//! The saver images are what get hashed to find the matching page; the original is what the
//! match then points at. The URLs are short-lived (the `baseUrl` is an assignment to one node of
//! the MangaDex@Home network, good for a few minutes), and chapters disappear often.
//!
//! No tags come from here: series, volume, chapter and page are applied by the Hydrus importer.
//!
//! Every hook is a pure function of the body and the post-page URL; MangaDex keeps no state.
//!
//! Python formats `baseUrl` and the chapter `hash` into the URLs with an f-string, so a number
//! there would be printed. The API sends strings, and here anything else means no pages. A
//! `dataSaver` that is neither a list nor absent is read the way Python indexes it (a string by
//! character; an object or a number makes Python raise, and a hook that raises has nothing to
//! say).

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use tracing::{debug, warn};

use super::parsed::json_of;
use super::{Page, SiteParser};
use crate::model::Tag;
use crate::{pycompat, pyurl};

pub const API_BASE: &str = "https://api.mangadex.org";

/// 1: stopped emitting series/volume/chapter/page. Entries cached while this parser still
/// produced them restore with `booru_tags_fetched=False`, so the tags are dropped on next use
/// rather than served from disk forever.
pub const TAG_VERSION: u32 = 1;

/// MangaDex identifies everything by UUID. Anchored to the `/chapter/` route specifically: a
/// `/title/` URL is a whole manga, which has no page list of its own and is left
/// classification-only.
static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"/chapter/([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})",
    )
    .expect("chapter regex")
});

/// What the registry knows about this site. This parser never returns tags, so an empty result is
/// normal (`EMPTY_RESULT_IS_NORMAL`); without it every match would log a "markup may have
/// changed" warning and the app would call the parser broken after three.
pub static PARSER: SiteParser = SiteParser {
    empty_result_is_normal: true,
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_page_count: Some(|body, url, _| parse_page_count(body, url)),
    parse_pages: Some(|body, url, _| parse_pages(body, url)),
    pages_api_url: Some(pages_api_url),
    ..SiteParser::new("mangadex", matches, |body, url, _| parse(body, url))
};

/// A chapter URL on mangadex.org or a subdomain. The host is the URL's whole `netloc`, as
/// Python's `urlparse(url).netloc` reads it, so a port or a `user@` prefix is not claimed.
pub fn matches(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    let host = pyurl::urlparse(url, "").netloc.to_lowercase();
    (host == "mangadex.org" || host.ends_with(".mangadex.org")) && CHAPTER_RE.is_match(url)
}

pub fn chapter_id(url: &str) -> Option<String> {
    CHAPTER_RE.captures(url).map(|found| found[1].to_string())
}

/// Fetch the API's page list instead of the chapter page, which is a JS front end with nothing in
/// its markup.
pub fn resolve_fetch_url(url: &str) -> String {
    match chapter_id(url) {
        Some(id) => format!("{API_BASE}/at-home/server/{id}"),
        None => {
            debug!("No chapter id in {url}, fetching it as-is");
            url.to_string()
        }
    }
}

/// Where the page list lives, for the multi-page resolver: the same response the fetch above
/// already asked for, since it carries every page.
pub fn pages_api_url(url: &str) -> Option<String> {
    chapter_id(url).map(|id| format!("{API_BASE}/at-home/server/{id}"))
}

/// One page of the chapter: the original and, where the API listed one, its saver image.
struct PageUrls {
    original: String,
    saver: Option<String>,
}

/// `dataSaver[index]` as Python evaluates `index < len(savers) and savers[index]`, for a value
/// that is truthy; `Err` is where Python raises.
fn saver_at(savers: &Value, chars: &[char], index: usize) -> Result<Option<String>, ()> {
    match savers {
        Value::Null => Ok(None),
        Value::Array(items) => Ok(items
            .get(index)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)),
        Value::String(_) => Ok(chars.get(index).map(char::to_string)),
        // `len()` works, and `savers[index]` is a KeyError wherever it is reached.
        Value::Object(items) if index >= items.len() => Ok(None),
        // `len()` of a number is a TypeError.
        _ => Err(()),
    }
}

/// A string field that is present and not empty (`not base` is false for it).
fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `_page_urls`: the pages in order, or none if the response is not one we understand.
fn page_urls(body: &str) -> Vec<PageUrls> {
    let data = match json_of(body) {
        Ok(Value::Object(data)) => data,
        Ok(_) => return Vec::new(),
        Err(_) => {
            warn!("MangaDex response was not valid JSON (the API may have changed)");
            return Vec::new();
        }
    };
    if data.get("result").and_then(Value::as_str) != Some("ok") {
        return Vec::new();
    }
    let (Some(base), Some(Value::Object(chapter))) =
        (non_empty(data.get("baseUrl")), data.get("chapter"))
    else {
        return Vec::new();
    };
    let Some(chapter_hash) = non_empty(chapter.get("hash")) else {
        return Vec::new();
    };

    let truthy = |key: &str| chapter.get(key).filter(|v| pycompat::truthy(v));
    let originals = match truthy("data") {
        None => &[][..],
        Some(Value::Array(items)) => items,
        Some(_) => return Vec::new(),
    };
    let savers = truthy("dataSaver").unwrap_or(&Value::Null);
    let saver_chars: Vec<char> = savers.as_str().unwrap_or("").chars().collect();

    let mut pages = Vec::new();
    for (index, name) in originals.iter().enumerate() {
        let Some(name) = name.as_str().filter(|n| !n.is_empty()) else {
            continue;
        };
        // The two lists are parallel, but only pair them where both really have an entry: a
        // mismatched length would otherwise attach the wrong page's saver image to a page,
        // which is precisely the mistake this whole mechanism exists to avoid.
        let Ok(saver) = saver_at(savers, &saver_chars, index) else {
            return Vec::new();
        };
        pages.push(PageUrls {
            original: format!("{base}/data/{chapter_hash}/{name}"),
            saver: saver.map(|s| format!("{base}/data-saver/{chapter_hash}/{s}")),
        });
    }
    pages
}

/// The page list in the shape the multi-page resolver expects.
///
/// No width or height: the API does not report them. Absent is correct rather than guessed, as
/// the resolver treats unknown dimensions as "no evidence" and falls back to hashing, whereas
/// invented ones would be evidence pointing at the wrong page.
pub fn parse_pages(body: &str, _url: &str) -> Vec<Page> {
    page_urls(body)
        .into_iter()
        .map(|page| {
            let saver = page.saver.unwrap_or_else(|| page.original.clone());
            let mut entry = Map::new();
            entry.insert(
                "urls".into(),
                // "regular" and "small" are both the saver image: it is the only smaller size
                // MangaDex offers, and it is what both the preview and the page hashing want.
                json!({"original": page.original, "regular": saver, "small": saver}),
            );
            entry.insert("width".into(), Value::Null);
            entry.insert("height".into(), Value::Null);
            entry
        })
        .collect()
}

pub fn parse_page_count(body: &str, _url: &str) -> i64 {
    i64::try_from(page_urls(body).len())
        .unwrap_or(i64::MAX)
        .max(1)
}

/// The first page's original. Which page actually matched is settled later by the multi-page
/// resolver; this is the honest default until then, and the one used when a chapter really is a
/// single page.
pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    page_urls(body).into_iter().next().map(|page| page.original)
}

/// The first page's saver image: a fraction of the original's size and plenty for the preview
/// panel.
pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    page_urls(body)
        .into_iter()
        .next()
        .map(|page| page.saver.unwrap_or(page.original))
}

/// No tags; see the note at the top of this module.
pub fn parse(_body: &str, _url: &str) -> Vec<Tag> {
    Vec::new()
}
