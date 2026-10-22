//! Google reverse image search (`core/google_images.py`): the three readers of what Google
//! answers, a classic results page, a Lens page and a Cloud Vision web-detection payload.
//!
//! Google is a general web index, not a booru index: what it hands back is a page URL, not a
//! tagged booru post. It reports no similarity, and its order is page relevance, so candidates
//! get an ordinal score that starts below ascii2d's (a Google hit can surface a source nothing
//! else found without overturning a real IQDB or SauceNAO match).
//!
//! The request half (the Vision call, the public upload, the consent cookie, the error mapping
//! for a rejected key or an exhausted quota, the latch that stands the engine down for the
//! session, the JavaScript-shell and challenge checks) is Phase 3.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Selector};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{UnexpectedInput, objects_in};
use crate::boorus::parsed::{soup_of, text_stripped};
use crate::pycompat;
use crate::pyurl;

pub const GOOGLE_URL: &str = "https://www.google.com/";

/// Below ascii2d's feature-search start (80): Google ranks by page relevance, not by visual
/// similarity.
pub const SIMILARITY_START: f64 = 78.0;
/// Pages Vision lists as carrying only a PARTIAL match (a crop, a resize, a similar picture):
/// a lower band, so an exact match always sorts above them.
pub const PARTIAL_SIMILARITY_START: f64 = 70.0;
/// A page Vision returned under `pagesWithMatchingImages` but attached no matching image to: a
/// loose association, kept but sorted below every match Vision actually evidenced.
pub const BARE_PAGE_SIMILARITY_START: f64 = 60.0;
pub const SIMILARITY_STEP: f64 = 2.0;
pub const SIMILARITY_FLOOR: f64 = 50.0;
pub const MAX_RESULTS: usize = 8;

/// Google's own infrastructure: none of it is ever the page an image came from.
const GOOGLE_HOST_SUFFIXES: [&str; 11] = [
    "gstatic.com",
    "googleusercontent.com",
    "googleapis.com",
    "googleadservices.com",
    "googlesyndication.com",
    "googletagmanager.com",
    "google-analytics.com",
    "ggpht.com",
    "goo.gl",
    "schema.org",
    "w3.org",
];

/// Hosts that serve Google's result thumbnails: not sources, but the preview the GUI shows.
const THUMBNAIL_HOST_HINTS: [&str; 4] = [
    "encrypted-tbn",
    "gstatic.com",
    "googleusercontent.com",
    "ggpht.com",
];

/// A Google redirector wrapping the real destination, used on the classic results page when
/// JavaScript is off.
const REDIRECT_PATHS: [&str; 2] = ["/url", "/imgres"];

/// `(?:^|\.)google(?:\.[a-z]{2,3}){1,2}$` with `re.IGNORECASE`. The host is lower-cased first,
/// so what is left of the flag is that `[a-z]` also matches `ı` (and `İ`), which `(?i)` does not
/// give it. `$` is `\n?\z`, as in Python.
static GOOGLE_DOMAIN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|\.)google(?:\.[a-z\u{131}\u{130}]{2,3}){1,2}\n?\z")
        .expect("google domain regex")
});

/// String literals inside a Lens data blob, in document order. Slashes are escaped in that JSON,
/// which is why the scheme is matched loosely.
static JS_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    regex::RegexBuilder::new(r#""((?:[^"\\]|\\.){2,400})""#)
        .size_limit(1 << 28)
        .build()
        .expect("js string regex")
});

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("tag regex"));
static UNICODE_ESCAPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\u([0-9a-fA-F]{4})").expect("unicode escape regex"));
static LETTER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}").expect("letter regex"));

/// Image CDNs whose URL carries the id of the post it belongs to, so a bare image match can be
/// turned into a page a user can read tags off. A short, verified list and not a pattern: CONFIRMED
/// live that `rule34storage.b-cdn.net/posts/1383/1383457/` is rule34.us post 1383457. Hosts whose
/// URL is only a content hash carry no id to recover and stay the direct image link.
static CDN_POST_PATTERNS: LazyLock<[(Regex, &'static str); 1]> = LazyLock::new(|| {
    [(
        Regex::new(r"(?i)^https?://rule34storage\.b-cdn\.net/posts/\d+/(\d+)/")
            .expect("cdn post regex"),
        "https://rule34.us/index.php?r=posts/view&id=",
    )]
});

/// One page Google knows the picture from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleImagesMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub title: Option<String>,
    pub source_name: Option<String>,
}

fn score(rank: usize, start: f64) -> f64 {
    (start - rank as f64 * SIMILARITY_STEP).max(SIMILARITY_FLOOR)
}

// ----------------------------------------------------------------------
// Cloud Vision web detection
// ----------------------------------------------------------------------

type Candidate = (String, Option<String>, Option<String>);

/// Turns a Vision `WEB_DETECTION` body into matches.
///
/// Vision answers in several arrays and the useful one is not the obvious one:
/// `fullMatchingImages` holds the exact matches (the booru CDNs the pictures came from);
/// `pagesWithMatchingImages` is mostly noise; `visuallySimilarImages` is related only by subject
/// and is never read. Results are banded by how strongly Vision evidenced them, and keep
/// Vision's own order within a band.
///
/// `Err` where Python raises on an input the service never sends: a payload that is not an
/// object, `responses` that is a non-empty string, object or number, a first response that is
/// not an object, a `webDetection` that is truthy and not an object, a list field that is a
/// number, or a `url` that is truthy and not a string.
pub fn parse_vision_results(payload: &Value) -> Result<Vec<GoogleImagesMatch>, UnexpectedInput> {
    let payload = payload.as_object().ok_or(UnexpectedInput)?;
    // `responses = payload.get("responses") or []`, then `responses[0] if responses else {}`
    let first: Option<&Map<String, Value>> = match payload.get("responses") {
        Some(responses) if pycompat::truthy(responses) => match responses {
            Value::Array(items) => Some(items[0].as_object().ok_or(UnexpectedInput)?),
            _ => return Err(UnexpectedInput),
        },
        _ => None,
    };
    let detection: Option<&Map<String, Value>> = match first.and_then(|r| r.get("webDetection")) {
        Some(v) if pycompat::truthy(v) => Some(v.as_object().ok_or(UnexpectedInput)?),
        _ => None,
    };
    let field = |name: &str| detection.and_then(|d| d.get(name));

    let mut exact: Vec<Candidate> = Vec::new();
    let mut partial: Vec<Candidate> = Vec::new();
    let mut bare: Vec<Candidate> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for page in objects_in(field("pagesWithMatchingImages"))? {
        let full_images = page.get("fullMatchingImages");
        let part_images = page.get("partialMatchingImages");
        let truthy = |v: Option<&Value>| v.is_some_and(pycompat::truthy);
        let bucket = if truthy(full_images) {
            &mut exact
        } else if truthy(part_images) {
            &mut partial
        } else {
            &mut bare
        };
        let evidence = if truthy(full_images) {
            full_images
        } else {
            part_images
        };
        collect(
            bucket,
            &mut seen,
            page.get("url"),
            clean_title(page.get("pageTitle")),
            first_image_url(evidence)?,
        )?;
    }

    // The exact matches themselves, which carry no page of their own.
    for image in objects_in(field("fullMatchingImages"))? {
        let url = image.get("url");
        collect(&mut exact, &mut seen, url, None, raw_url(url))?;
    }
    for image in objects_in(field("partialMatchingImages"))? {
        let url = image.get("url");
        collect(&mut partial, &mut seen, url, None, raw_url(url))?;
    }

    let mut matches: Vec<GoogleImagesMatch> = Vec::new();
    for (group, start) in [
        (exact, SIMILARITY_START),
        (partial, PARTIAL_SIMILARITY_START),
        (bare, BARE_PAGE_SIMILARITY_START),
    ] {
        for (rank, (url, title, thumb)) in group.into_iter().enumerate() {
            if matches.len() >= MAX_RESULTS {
                return Ok(matches);
            }
            matches.push(GoogleImagesMatch {
                source_name: host_label(&url),
                url,
                thumb_url: thumb,
                similarity: score(rank, start),
                title,
            });
        }
    }
    Ok(matches)
}

/// `image.get("url")` as a thumbnail, untouched: the raw string, whitespace and fragment and all.
fn raw_url(url: Option<&Value>) -> Option<String> {
    url.and_then(Value::as_str).map(str::to_string)
}

/// Python's `collect`.
fn collect(
    bucket: &mut Vec<Candidate>,
    seen: &mut HashSet<String>,
    raw: Option<&Value>,
    title: Option<String>,
    thumb: Option<String>,
) -> Result<(), UnexpectedInput> {
    // `(raw_url or "").strip().split("#", 1)[0]`
    let raw = match raw {
        Some(v) if pycompat::truthy(v) => v.as_str().ok_or(UnexpectedInput)?,
        _ => "",
    };
    let stripped = pycompat::strip(raw);
    let mut url = stripped.split('#').next().unwrap_or("").to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(());
    }
    // A bare CDN image is not somewhere a user can read tags. Where the post it belongs to can
    // be recovered, the candidate becomes that post and the image becomes its thumbnail.
    let mut thumb = thumb;
    if let Some(post_url) = post_url_for_image(&url) {
        thumb = thumb.filter(|t| !t.is_empty()).or(Some(url));
        url = post_url;
    }
    if seen.contains(&url) || is_google_host(&host_label(&url).unwrap_or_default()) {
        return Ok(());
    }
    seen.insert(url.clone());
    bucket.push((url, title, thumb));
    Ok(())
}

/// The post page a CDN image belongs to, when its URL names it. `None` for anything not on the
/// verified list, which leaves the match as the image link it already was and never a guessed
/// URL.
pub fn post_url_for_image(image_url: &str) -> Option<String> {
    CDN_POST_PATTERNS.iter().find_map(|(pattern, template)| {
        pattern
            .captures(image_url)
            .map(|found| format!("{template}{}", &found[1]))
    })
}

/// The first image URL in a `fullMatchingImages` / `partialMatchingImages` list that is a web
/// URL. `Err` where Python's `.strip()` is called on a truthy `url` that is not a string, or the
/// list is a number.
fn first_image_url(images: Option<&Value>) -> Result<Option<String>, UnexpectedInput> {
    for image in objects_in(images)? {
        let url = match image.get("url") {
            Some(v) if pycompat::truthy(v) => pycompat::strip(v.as_str().ok_or(UnexpectedInput)?),
            _ => "",
        };
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(Some(url.to_string()));
        }
    }
    Ok(None)
}

/// A Vision page title: the page's own `<title>`, sometimes with the matched words wrapped in
/// `<b>` and always with its entities unresolved.
fn clean_title(title: Option<&Value>) -> Option<String> {
    let title = title?.as_str()?;
    let text = pycompat::html_unescape(&TAG_RE.replace_all(title, ""));
    let text = pycompat::strip(&text);
    (!text.is_empty()).then(|| text.to_string())
}

// ----------------------------------------------------------------------
// The classic results page
// ----------------------------------------------------------------------

/// Turns a classic Google results page into matches.
///
/// Google's result markup is generated and its class names change, so the anchor is found by
/// structure: a link wrapping a heading is a result, and a link through `/url?q=` is one on the
/// no-JavaScript page.
///
/// `Err` where `urlparse` raises `ValueError` on a link (Python lets it leave the function).
pub fn parse_results(
    html: &str,
    page_url: &str,
) -> Result<Vec<GoogleImagesMatch>, UnexpectedInput> {
    let soup = soup_of(html);
    let anchor_selector = Selector::parse("a[href]").expect("anchor selector");
    let links: Vec<(ElementRef, String)> = soup
        .select(&anchor_selector)
        .map(|a| {
            (
                a,
                pycompat::strip(a.value().attr("href").unwrap_or("")).to_string(),
            )
        })
        .collect();
    let mut anchors: Vec<&(ElementRef, String)> = links
        .iter()
        .filter(|(a, href)| !href.is_empty() && heading_in(*a).is_some())
        .collect();
    if anchors.is_empty() {
        for link in &links {
            let (_, href) = link;
            if href.is_empty() {
                continue;
            }
            let path = pyurl::try_urlparse(href, "")
                .map_err(|_| UnexpectedInput)?
                .path;
            if REDIRECT_PATHS.contains(&path.as_str()) {
                anchors.push(link);
            }
        }
    }

    let mut found: Vec<GoogleImagesMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (a, href) in anchors {
        let Some(url) = resolve_href(href, page_url)? else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let title = text_stripped(heading_in(*a).unwrap_or(*a), " ");
        found.push(GoogleImagesMatch {
            thumb_url: thumb_near(*a, page_url)?,
            similarity: score(found.len(), SIMILARITY_START),
            title: (!title.is_empty()).then_some(title),
            source_name: host_label(&url),
            url,
        });
        if found.len() >= MAX_RESULTS {
            break;
        }
    }
    Ok(found)
}

/// `a.find(["h3", "h2"])`: the first heading under the anchor, in document order.
fn heading_in(a: ElementRef<'_>) -> Option<ElementRef<'_>> {
    a.descendants()
        .skip(1)
        .filter_map(ElementRef::wrap)
        .find(|e| matches!(e.value().name(), "h3" | "h2"))
}

/// The real destination behind a result link. Google's no-JavaScript page wraps every result in
/// `/url?q=...`, and a relative href is one of its own pages. Both are unwrapped here, so a
/// candidate is stored as the URL a user could open.
fn resolve_href(href: &str, page_url: &str) -> Result<Option<String>, UnexpectedInput> {
    let href = pycompat::strip(href);
    if href.is_empty() {
        return Ok(None);
    }
    let base = if page_url.is_empty() {
        GOOGLE_URL
    } else {
        page_url
    };
    let absolute = pyurl::try_urljoin(base, href).map_err(|_| UnexpectedInput)?;
    let parsed = pyurl::try_urlparse(&absolute, "").map_err(|_| UnexpectedInput)?;
    if REDIRECT_PATHS.contains(&parsed.path.as_str()) {
        for key in ["q", "url", "imgrefurl"] {
            if let Some(target) = pyurl::first_query_value(&parsed.query, key)
                && (target.starts_with("http://") || target.starts_with("https://"))
            {
                return Ok(Some(target));
            }
        }
        return Ok(None);
    }
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Ok(None);
    }
    if is_google_host(&parsed.netloc.to_lowercase()) {
        return Ok(None);
    }
    Ok(Some(absolute))
}

/// The preview image belonging to a result, if the page carries one: inside the anchor first,
/// then its containing block, then that block's. Google puts the thumbnail in either place
/// depending on the layout.
fn thumb_near(anchor: ElementRef<'_>, page_url: &str) -> Result<Option<String>, UnexpectedInput> {
    let parent = anchor.parent();
    let scopes = [Some(*anchor), parent, parent.and_then(|p| p.parent())];
    for scope in scopes.into_iter().flatten() {
        let img = scope
            .descendants()
            .skip(1)
            .filter_map(ElementRef::wrap)
            .find(|e| e.value().name() == "img");
        let src = img
            .map(|img| {
                let pick = |name: &str| img.value().attr(name).filter(|v| !v.is_empty());
                pycompat::strip(pick("src").or_else(|| pick("data-src")).unwrap_or("")).to_string()
            })
            .unwrap_or_default();
        if src.starts_with("data:") {
            continue;
        }
        if !src.is_empty() {
            let base = if page_url.is_empty() {
                GOOGLE_URL
            } else {
                page_url
            };
            let joined = pyurl::try_urljoin(base, &src).map_err(|_| UnexpectedInput)?;
            return Ok(Some(joined));
        }
    }
    Ok(None)
}

// ----------------------------------------------------------------------
// A Lens page
// ----------------------------------------------------------------------

/// Pulls matches out of a Google Lens page's embedded data.
///
/// Lens renders from a JSON blob whose arrays are positional and undocumented, so nothing here
/// counts fields. It reads the blob's string literals in order and uses two facts that have held
/// across every layout change so far: a result's page URL is followed closely by its title, and
/// its thumbnail (on a Google asset host) appears just before it.
pub fn parse_lens_results(html: &str) -> Vec<GoogleImagesMatch> {
    let mut found: Vec<GoogleImagesMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let literals: Vec<String> = JS_STRING_RE
        .captures_iter(html)
        .map(|m| unescape(&m[1]))
        .collect();

    let mut pending_thumb: Option<String> = None;
    for (index, literal) in literals.iter().enumerate() {
        if !(literal.starts_with("http://") || literal.starts_with("https://")) {
            continue;
        }
        let host = host_label(literal).unwrap_or_default();
        if is_google_host(&host) {
            if THUMBNAIL_HOST_HINTS.iter().any(|hint| host.contains(hint)) {
                pending_thumb = Some(literal.clone());
            }
            continue;
        }
        let url = literal.split('#').next().unwrap_or("").to_string();
        if !seen.insert(url.clone()) {
            continue;
        }
        found.push(GoogleImagesMatch {
            url,
            thumb_url: pending_thumb.take(),
            similarity: score(found.len(), SIMILARITY_START),
            title: title_after(&literals, index),
            source_name: (!host.is_empty()).then_some(host),
        });
        if found.len() >= MAX_RESULTS {
            break;
        }
    }
    found
}

/// The nearest following literal that reads like a page title: bounded to three positions, since
/// in every Lens layout seen the title sits immediately after the URL and looking further would
/// start picking up an unrelated result's text.
fn title_after(literals: &[String], index: usize) -> Option<String> {
    for candidate in literals.iter().skip(index + 1).take(3) {
        let text = pycompat::strip(candidate);
        let length = text.chars().count();
        if !(3..=200).contains(&length) {
            continue;
        }
        if ["http://", "https://", "/", "data:"]
            .iter()
            .any(|prefix| text.starts_with(prefix))
        {
            continue;
        }
        // `any(ch.isalpha())`: a letter is a character of general category L.
        if !LETTER_RE.is_match(text) {
            continue;
        }
        return Some(text.to_string());
    }
    None
}

/// JSON-escaped slashes and quotes, and `\uXXXX` escapes, back to plain text. Done by hand
/// rather than with a JSON parser: these literals come out of a much larger blob one at a time,
/// and a single one that carries an escape a parser cannot read must not throw away every other
/// result on the page.
///
/// An escape that names a surrogate (`\ud83d`) is a lone surrogate in Python, which a Rust
/// `String` cannot hold; it becomes U+FFFD. Python also leaves a surrogate pair as two
/// surrogates and does not join it into one character.
fn unescape(literal: &str) -> String {
    let text = literal.replace("\\/", "/").replace("\\\"", "\"");
    UNICODE_ESCAPE_RE
        .replace_all(&text, |caps: &regex::Captures| {
            let code = u32::from_str_radix(&caps[1], 16).unwrap_or(0xFFFD);
            char::from_u32(code).unwrap_or('\u{fffd}').to_string()
        })
        .into_owned()
}

// ----------------------------------------------------------------------
// Shared helpers
// ----------------------------------------------------------------------

/// Whether `host` is Google's own: a Google domain in any country, or one of the asset and
/// tracking hosts in [`GOOGLE_HOST_SUFFIXES`].
pub fn is_google_host(host: &str) -> bool {
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if GOOGLE_DOMAIN_RE.is_match(host) {
        return true;
    }
    GOOGLE_HOST_SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|before| before.ends_with('.'))
    })
}

/// The host of `url`, lower-cased and without a leading `www.`, or `None` when it has none or
/// `urlparse` raises `ValueError` (which Python catches here).
pub fn host_label(url: &str) -> Option<String> {
    let netloc = pyurl::try_urlparse(url, "").ok()?.netloc.to_lowercase();
    let host = netloc.strip_prefix("www.").unwrap_or(&netloc);
    (!host.is_empty()).then(|| host.to_string())
}
