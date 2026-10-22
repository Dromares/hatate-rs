//! Google reverse image search (`core/google_images.py`): the three readers of what Google
//! answers, a classic results page, a Lens page and a Cloud Vision web-detection payload.
//!
//! Google is a general web index, not a booru index: what it hands back is a page URL, not a
//! tagged booru post. It reports no similarity, and its order is page relevance, so candidates
//! get an ordinal score that starts below ascii2d's (a Google hit can surface a source nothing
//! else found without overturning a real IQDB or SauceNAO match).
//!
//! `search` has two routes, picked by [`vision_key`], and each is cut where it reads the answer:
//! [`vision_from_response`] for the Cloud Vision call (the status mapping, the per-image error
//! inside a 200, the hand-off to [`parse_vision_results`]) and [`public_from_response`] for the
//! public upload (the 413, the consent-wall and bot-check test, the other statuses, Lens page or
//! classic page, the JavaScript shell). The latch that stands the engine down for the session
//! (`_blocked`) is not kept here: an error that [`GoogleImagesError::latches`] is the signal for
//! the client to set it. The request half (reading and downscaling the picture, the Vision body
//! and the multipart upload, the consent cookie and headers, the deadline, the network errors,
//! `reset_blocked_flag` at the start of a search) is Phase 3.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Selector};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{Response, UnexpectedInput, objects_in};
use crate::boorus::parsed::{soup_of, text_stripped};
use crate::pycompat;
use crate::pyrepr::py_str;
use crate::pyurl;

pub const GOOGLE_URL: &str = "https://www.google.com/";
/// The public upload (a multipart `encoded_image`, `hl=en`, redirects followed).
pub const SEARCH_BY_IMAGE_URL: &str = "https://www.google.com/searchbyimage/upload";
/// The Cloud Vision call (a JSON body, `key=<api key>`).
pub const VISION_URL: &str = "https://vision.googleapis.com/v1/images:annotate";

/// What a consent wall or a bot check says in its first 6000 characters.
const CHALLENGE_MARKERS: [&str; 5] = [
    "our systems have detected unusual traffic",
    "/sorry/index",
    "recaptcha",
    "id=\"captcha",
    "unusual traffic from your computer network",
];

/// The shell Google serves in place of results: its no-JavaScript notice links to
/// `/httpservice/retry/enablejs` and hides the page body until its scripts have run.
const JS_SHELL_MARKERS: [&str; 2] = ["/httpservice/retry/enablejs", "if you are not redirected"];

const CHALLENGE_MESSAGE: &str = "Google is answering with a consent page or an \"unusual \
traffic\" check instead of results, so it won't accept uploads from this app right now. No \
setting fixes it - it clears on Google's schedule. Turn Google Images off under Settings > \
Engine, or try again later.";
const JAVASCRIPT_MESSAGE: &str = "Google accepted the image but answers with a page that only \
fills in its results once its own JavaScript has run, which this app does not do. Add a Google \
Cloud Vision API key under Settings > Engine to use Google's web detection API instead, or turn \
Google Images off.";

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
static JS_STRING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:[^"\\]|\\.){2,400})""#).expect("js string regex"));

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

// ----------------------------------------------------------------------
// `search`: what is done with the answers
// ----------------------------------------------------------------------

/// `GoogleImagesError`, its subclass `GoogleImagesBlockedError` and that one's subclass
/// `GoogleImagesJavaScriptError`, plus the case where Python raises something else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoogleImagesError {
    /// `GoogleImagesError`: a failure about this image, with its user-facing message. Not
    /// latched: the next image is still asked.
    Failed(String),
    /// `GoogleImagesBlockedError`: Google will not answer this app for the rest of the session (a
    /// consent wall, an "unusual traffic" check, a Vision key that is rejected or out of quota),
    /// so the client latches the engine (`_blocked`).
    Blocked(String),
    /// `GoogleImagesJavaScriptError`: the upload worked and the page holds no results until
    /// Google's JavaScript has run. A property of Google's front end, so latched like
    /// [`Blocked`](Self::Blocked); kept distinct because its message names the fix (a Vision key).
    JavaScript,
    /// Python raised out of `search` on an answer the service never gives; see
    /// [`UnexpectedInput`].
    Unexpected(UnexpectedInput),
}

impl GoogleImagesError {
    /// Whether Python's `_latch` made this error, which is when the client sets `_blocked`.
    pub fn latches(&self) -> bool {
        matches!(self, Self::Blocked(_) | Self::JavaScript)
    }
}

impl std::fmt::Display for GoogleImagesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(message) | Self::Blocked(message) => f.write_str(message),
            Self::JavaScript => f.write_str(JAVASCRIPT_MESSAGE),
            Self::Unexpected(raised) => raised.fmt(f),
        }
    }
}

impl std::error::Error for GoogleImagesError {}

impl From<UnexpectedInput> for GoogleImagesError {
    fn from(raised: UnexpectedInput) -> Self {
        GoogleImagesError::Unexpected(raised)
    }
}

/// The Vision API key `search` uses, stripped, or `None` when it is blank and the public upload
/// is used instead (`(api_key or "").strip()`).
pub fn vision_key(api_key: &str) -> Option<&str> {
    let key = pycompat::strip(api_key);
    (!key.is_empty()).then_some(key)
}

/// `_looks_like_a_challenge`: a consent wall or a bot check, served as an ordinary 200 as often
/// as not, so the status cannot tell it from results.
fn looks_like_a_challenge(response: &Response<'_>) -> bool {
    let url = response.url.to_lowercase();
    if url.contains("consent.google.") || url.contains("/sorry/") {
        return true;
    }
    let body: String = response
        .text
        .chars()
        .take(6000)
        .collect::<String>()
        .to_lowercase();
    CHALLENGE_MARKERS.iter().any(|marker| body.contains(marker))
}

/// `looks_like_a_javascript_shell`: whether the page holds no results until Google's scripts run.
/// Told apart from a genuinely empty page so that "Google found nothing" is never recorded for an
/// image Google was never asked about.
pub fn looks_like_a_javascript_shell(html: &str) -> bool {
    let head: String = html.chars().take(20000).collect::<String>().to_lowercase();
    JS_SHELL_MARKERS.iter().any(|marker| head.contains(marker))
}

/// What `_search_public` does with the upload's final answer (redirects followed): the 413, the
/// consent wall or bot check (checked before the status), any other status, then the Lens reader
/// for a `lens.google.` landing and the classic reader for the rest. An empty parse of a
/// JavaScript shell is an error, not "no matches".
pub fn public_from_response(
    response: &Response<'_>,
) -> Result<Vec<GoogleImagesMatch>, GoogleImagesError> {
    if response.status == 413 {
        return Err(GoogleImagesError::Failed(
            "Google rejected the image as too large, even after downscaling".to_owned(),
        ));
    }
    if looks_like_a_challenge(response) {
        return Err(GoogleImagesError::Blocked(CHALLENGE_MESSAGE.to_owned()));
    }
    if response.status != 200 {
        return Err(GoogleImagesError::Failed(format!(
            "Google returned HTTP {}",
            response.status
        )));
    }

    let final_url = if response.url.is_empty() {
        GOOGLE_URL
    } else {
        response.url
    };
    let matches = if final_url.to_lowercase().contains("lens.google.") {
        parse_lens_results(response.text)
    } else {
        parse_results(response.text, final_url)?
    };
    if matches.is_empty() && looks_like_a_javascript_shell(response.text) {
        return Err(GoogleImagesError::JavaScript);
    }
    Ok(matches)
}

/// What `_search_vision` does with the call's answer. `payload` is `resp.json()`, `None` when it
/// would not decode (Python reads that as `{}`). Any status but 200 is an error: a rejected key,
/// a disabled API and an exhausted quota fail every image in the batch identically, so they come
/// back [`Blocked`](GoogleImagesError::Blocked); a malformed request is about this image and
/// does not. A per-image error is reported inside a 200 and read out of the body.
///
/// `Err(Unexpected)` where Python raises on a payload the service never sends: one that is not an
/// object, an `error` or `message` or `status` that is truthy and of the wrong type, or a
/// `responses` that is not a list of objects.
pub fn vision_from_response(
    status: u16,
    payload: Option<&Value>,
) -> Result<Vec<GoogleImagesMatch>, GoogleImagesError> {
    let empty = Value::Object(Map::new());
    let payload = payload.unwrap_or(&empty);
    let Value::Object(body) = payload else {
        return Err(UnexpectedInput.into());
    };
    if status != 200 {
        return Err(vision_error(status, body)?);
    }

    // `inner = (payload.get("responses") or [{}])[0].get("error")`
    let inner = match body.get("responses") {
        Some(Value::Array(responses)) => match responses.first() {
            Some(Value::Object(first)) => first.get("error"),
            Some(_) => return Err(UnexpectedInput.into()),
            None => None,
        },
        Some(other) if pycompat::truthy(other) => return Err(UnexpectedInput.into()),
        _ => None,
    };
    if let Some(inner) = inner.filter(|inner| pycompat::truthy(inner)) {
        let Value::Object(fields) = inner else {
            return Err(UnexpectedInput.into());
        };
        // `inner.get('message') or inner`
        let message = match fields.get("message") {
            Some(message) if pycompat::truthy(message) => py_str(message),
            _ => py_str(inner),
        };
        return Err(GoogleImagesError::Failed(format!(
            "Google Vision: {message}"
        )));
    }

    Ok(parse_vision_results(payload)?)
}

/// `_vision_error`: a Vision failure as the right kind of error. The text fields are read as
/// Python reads them, `(x or "").strip()` and `(x or "").upper()`, so a truthy value that is not
/// a string is `Err`.
fn vision_error(
    status: u16,
    payload: &Map<String, Value>,
) -> Result<GoogleImagesError, UnexpectedInput> {
    let error = match payload.get("error") {
        Some(Value::Object(error)) => Some(error),
        Some(other) if pycompat::truthy(other) => return Err(UnexpectedInput),
        _ => None,
    };
    let text = |key: &str| -> Result<&str, UnexpectedInput> {
        match error.and_then(|error| error.get(key)) {
            Some(Value::String(text)) => Ok(text),
            Some(other) if pycompat::truthy(other) => Err(UnexpectedInput),
            _ => Ok(""),
        }
    };
    let message = pycompat::strip(text("message")?);
    let message = if message.is_empty() {
        format!("HTTP {status}")
    } else {
        message.to_owned()
    };
    let reason = text("status")?.to_uppercase();

    Ok(if status == 429 || reason == "RESOURCE_EXHAUSTED" {
        GoogleImagesError::Blocked(format!(
            "Google Vision quota is exhausted, so it can't answer any more images this \
                 month: {message}. Turn Google Images off under Settings > Engine, or raise \
                 the quota in the Google Cloud console."
        ))
    } else if matches!(status, 401 | 403)
        || reason == "PERMISSION_DENIED"
        || reason == "UNAUTHENTICATED"
    {
        GoogleImagesError::Blocked(format!(
            "Google Vision refused the API key: {message}. Check the key under Settings > \
                 Engine, and that the Cloud Vision API is enabled for its project."
        ))
    } else if status == 400 && message.to_lowercase().contains("api key") {
        GoogleImagesError::Blocked(format!("Google Vision rejected the API key: {message}"))
    } else {
        GoogleImagesError::Failed(format!("Google Vision: {message}"))
    })
}

#[cfg(test)]
mod tests {
    //! `tests/test_search_quality.py::TestGoogleImagesParser` and
    //! `TestGoogleVisionWebDetection` at `76c85c4`, test for test. The tests that go through
    //! `google_images.search` (a Vision key used instead of the public page, a rejected key, an
    //! exhausted quota, a per-image error, a consent wall, the JavaScript shell, a Lens landing)
    //! are the request half, and `test_ascii2d_and_google_images_are_ordinal_too` is the engine
    //! runner's: Phase 3.

    use super::*;
    use serde_json::json;

    const SERP: &str = r#"
    <html><body><div id="search">
      <div class="g">
        <a href="https://danbooru.donmai.us/posts/123"><h3>Some post - Danbooru</h3></a>
        <img src="https://encrypted-tbn0.gstatic.com/images?q=tbn:AAA">
      </div>
      <div class="g">
        <a href="/url?q=https://www.pixiv.net/artworks/456&amp;sa=U"><h3>An artwork - pixiv</h3></a>
      </div>
      <div class="g">
        <a href="https://www.google.com/preferences"><h3>Settings</h3></a>
      </div>
    </div></body></html>
    "#;
    const SERP_URL: &str = "https://www.google.com/search?tbs=sbi:x";

    const LENS: &str = concat!(
        "<html><body><script>AF_initDataCallback({key:'ds:1', data:[",
        r#"["https:\/\/encrypted-tbn0.gstatic.com\/images?q=tbn:XYZ","#,
        r#""https:\/\/www.deviantart.com\/someone\/art\/A-Picture-123","#,
        r#""A Picture by someone","#,
        r#""deviantart.com"]"#,
        "]});</script></body></html>",
    );

    fn urls(matches: &[GoogleImagesMatch]) -> Vec<&str> {
        matches.iter().map(|m| m.url.as_str()).collect()
    }

    #[test]
    fn a_results_page_yields_its_result_links() {
        let matches = parse_results(SERP, SERP_URL).unwrap();
        assert_eq!(
            urls(&matches),
            [
                "https://danbooru.donmai.us/posts/123",
                "https://www.pixiv.net/artworks/456"
            ]
        );
    }

    #[test]
    fn googles_own_pages_are_not_offered_as_sources() {
        // /preferences is navigation, not a page the picture came from.
        let matches = parse_results(SERP, SERP_URL).unwrap();
        assert!(!matches.iter().any(|m| m.url.contains("google.com")));
    }

    #[test]
    fn a_redirector_link_is_unwrapped() {
        // Stored as the URL a user could open, not as Google's /url?q= wrapper.
        let matches = parse_results(SERP, SERP_URL).unwrap();
        assert!(urls(&matches).contains(&"https://www.pixiv.net/artworks/456"));
        assert!(!matches.iter().any(|m| m.url.contains("/url?q=")));
    }

    #[test]
    fn results_are_scored_in_order_below_ascii2d() {
        // ascii2d's BOVW (feature search) start is 80 (`core.ascii2d.BOVW_SIMILARITY_START`).
        let matches = parse_results(SERP, SERP_URL).unwrap();
        assert!(matches[0].similarity < 80.0);
        assert!(matches[0].similarity > matches[1].similarity);
    }

    #[test]
    fn a_thumbnail_beside_a_result_is_kept() {
        let matches = parse_results(SERP, SERP_URL).unwrap();
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://encrypted-tbn0.gstatic.com/images?q=tbn:AAA")
        );
    }

    #[test]
    fn a_lens_page_yields_its_matches() {
        let matches = parse_lens_results(LENS);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].url,
            "https://www.deviantart.com/someone/art/A-Picture-123"
        );
        assert_eq!(matches[0].title.as_deref(), Some("A Picture by someone"));
        assert_eq!(matches[0].source_name.as_deref(), Some("deviantart.com"));
    }

    #[test]
    fn a_lens_thumbnail_is_kept_not_mistaken_for_a_source() {
        // The gstatic URL just before a result is its preview. Dropping every Google host would
        // lose it; treating it as a source would list Google as where the image came from.
        let matches = parse_lens_results(LENS);
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://encrypted-tbn0.gstatic.com/images?q=tbn:XYZ")
        );
    }

    #[test]
    fn junk_parses_to_nothing_rather_than_raising() {
        assert_eq!(
            parse_results("<html></html>", "https://www.google.com/"),
            Ok(vec![])
        );
        assert_eq!(parse_lens_results("<html></html>"), vec![]);
    }

    fn payload() -> Value {
        json!({"responses": [{"webDetection": {
            "pagesWithMatchingImages": [
                {
                    "url": "https://example.test/partial-only",
                    "pageTitle": "A crop of it",
                    "partialMatchingImages": [{"url": "https://example.test/crop.jpg"}],
                },
                {
                    "url": "https://www.deviantart.com/someone/art/A-Picture-123",
                    "pageTitle": "A Picture &amp; friends by <b>someone</b>",
                    "fullMatchingImages": [{"url": "https://images.test/full.jpg"}],
                },
                {
                    "url": "https://www.google.com/imgres?q=x",
                    "pageTitle": "Google",
                    "fullMatchingImages": [{"url": "https://images.test/x.jpg"}],
                },
                // Listed as a page with matching images, but carrying none.
                {"url": "https://www.youtube.com/shorts/qT_OeqrMtU8", "pageTitle": "Monkey Mischief"},
            ],
            "fullMatchingImages": [
                {"url": "https://rule34storage.b-cdn.net/posts/1383/1383457/1383457.picsmall.jpg"},
            ],
            "partialMatchingImages": [{"url": "https://images.test/partial-cdn.jpg"}],
            "visuallySimilarImages": [
                {"url": "https://images.test/similar-1.jpg"},
                {"url": "https://images.test/similar-2.jpg"},
            ],
        }}]})
    }

    #[test]
    fn the_exact_matches_are_read_not_just_the_pages() {
        // REGRESSION in Python: reading only pagesWithMatchingImages returned the noise and
        // dropped the answer.
        let matches = parse_vision_results(&payload()).unwrap();
        let urls = urls(&matches);
        assert!(urls.contains(&"https://rule34.us/index.php?r=posts/view&id=1383457"));
        assert!(urls.contains(&"https://images.test/partial-cdn.jpg"));
    }

    #[test]
    fn results_are_banded_by_how_well_evidenced_they_are() {
        let matches = parse_vision_results(&payload()).unwrap();
        assert_eq!(
            urls(&matches),
            [
                // exact: a page carrying a full match, then the full match itself
                "https://www.deviantart.com/someone/art/A-Picture-123",
                "https://rule34.us/index.php?r=posts/view&id=1383457",
                // partial: a page carrying only a partial, then the partial itself
                "https://example.test/partial-only",
                "https://images.test/partial-cdn.jpg",
                // a page Vision attached no matching image to, last
                "https://www.youtube.com/shorts/qT_OeqrMtU8",
            ]
        );
        let scores: Vec<f64> = matches.iter().map(|m| m.similarity).collect();
        assert_eq!(scores, [78.0, 76.0, 70.0, 68.0, 60.0]);
    }

    #[test]
    fn a_bare_page_never_outranks_an_evidenced_match() {
        let matches = parse_vision_results(&payload()).unwrap();
        let youtube = matches.iter().find(|m| m.url.contains("youtube")).unwrap();
        assert_eq!(youtube, matches.last().unwrap());
        assert!(
            matches[..matches.len() - 1]
                .iter()
                .all(|m| m.similarity > youtube.similarity)
        );
    }

    #[test]
    fn visually_similar_images_are_never_offered() {
        let matches = parse_vision_results(&payload()).unwrap();
        assert!(!matches.iter().any(|m| m.url.contains("similar-")));
    }

    #[test]
    fn a_cdn_image_becomes_the_post_it_belongs_to() {
        assert_eq!(
            post_url_for_image(
                "https://rule34storage.b-cdn.net/posts/1383/1383457/1383457.picsmall.jpg"
            )
            .as_deref(),
            Some("https://rule34.us/index.php?r=posts/view&id=1383457")
        );
    }

    #[test]
    fn an_unrecognised_cdn_is_left_alone_not_guessed_at() {
        for url in [
            "https://img2.rule34.us/thumbnails/1f/f7/thumbnail_abc.jpg",
            "https://img.kemono.cr/thumbnail/data/69/7f/697f.jpg",
            "https://pbs.twimg.com/media/FDbkoMGWQAQaUyN.jpg:small",
            "",
        ] {
            assert_eq!(post_url_for_image(url), None, "{url}");
        }
    }

    #[test]
    fn a_page_title_is_cleaned_up() {
        // Vision hands back the page's own <title>, with the matched words wrapped in <b> and its
        // entities unresolved.
        let matches = parse_vision_results(&payload()).unwrap();
        assert_eq!(
            matches[0].title.as_deref(),
            Some("A Picture & friends by someone")
        );
    }

    #[test]
    fn googles_own_pages_are_not_offered_as_vision_sources() {
        let matches = parse_vision_results(&payload()).unwrap();
        assert!(!matches.iter().any(|m| m.url.contains("google.com")));
    }

    #[test]
    fn the_matching_image_becomes_the_thumbnail() {
        let matches = parse_vision_results(&payload()).unwrap();
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://images.test/full.jpg")
        );
        let recovered = matches
            .iter()
            .find(|m| m.url.starts_with("https://rule34.us/"))
            .unwrap();
        assert_eq!(
            recovered.thumb_url.as_deref(),
            Some("https://rule34storage.b-cdn.net/posts/1383/1383457/1383457.picsmall.jpg")
        );
    }

    #[test]
    fn an_empty_or_malformed_body_is_no_matches_not_a_crash() {
        for payload in [
            json!({}),
            json!({"responses": []}),
            json!({"responses": [{}]}),
            json!({"responses": [{"webDetection": {}}]}),
        ] {
            assert_eq!(parse_vision_results(&payload), Ok(vec![]), "{payload}");
        }
    }

    // ---- what Python's code does that its tests do not say ----

    #[test]
    fn the_host_rules_are_python_s() {
        for host in [
            "google.com",
            "www.google.co.uk",
            "GOOGLE.COM.AU",
            "gstatic.com",
            "encrypted-tbn0.gstatic.com",
            "goo.gl",
        ] {
            assert!(is_google_host(host), "{host}");
        }
        for host in [
            "",
            "example.com",
            "xgoogle.com",
            "google.c",
            "notgstatic.com",
            "google.abcd",
        ] {
            assert!(!is_google_host(host), "{host}");
        }
        // Python's `[a-z]` under IGNORECASE also matches a dotless i.
        assert!(is_google_host("google.\u{131}\u{131}"));
        assert_eq!(
            host_label("https://WWW.Example.COM/x").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_label("https://[bad/x"), None);
        assert_eq!(host_label("/relative"), None);
    }

    #[test]
    fn a_link_urlparse_refuses_leaves_parse_results_as_an_error() {
        // `urlparse` raises `ValueError` on the unbalanced bracket; nothing in Python catches it.
        let page = r#"<a href="https://x]/y"><h3>t</h3></a>"#;
        assert_eq!(parse_results(page, ""), Err(UnexpectedInput));
    }

    #[test]
    fn the_first_image_of_a_scope_is_the_only_one_looked_at() {
        // `scope.find("img")` is the first image under the scope. A `data:` one is passed over
        // for the wider scope, but that scope's first image is the same one, so a real image
        // after it is never reached.
        let hidden = concat!(
            r#"<div><div><a href="https://a.example/p"><h3>t</h3><img src="data:image/gif;base64,AAAA"></a>"#,
            r#"<img src="/thumb.png"></div></div>"#
        );
        let matches = parse_results(hidden, "https://www.google.com/search?q=1").unwrap();
        assert_eq!(matches[0].thumb_url, None);
        // An image with no source at all is passed over the same way (and is also first).
        let empty = hidden.replace("data:image/gif;base64,AAAA", "");
        let matches = parse_results(&empty, "https://www.google.com/search?q=1").unwrap();
        assert_eq!(matches[0].thumb_url, None);
        // With the real image first, it is joined onto the page's address.
        let real = r#"<div><div><a href="https://a.example/p"><h3>t</h3></a><img data-src="/thumb.png"></div></div>"#;
        let matches = parse_results(real, "https://www.google.com/search?q=1").unwrap();
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://www.google.com/thumb.png")
        );
    }

    #[test]
    fn a_lens_title_is_the_nearest_literal_that_reads_like_one() {
        let blob = r#"["https://a.example/p","/x","1234","ab","  A Title  ","later title"]"#;
        let matches = parse_lens_results(blob);
        // `/x` is a path, `1234` has no letter, `ab` is too short; the third position is out.
        assert_eq!(matches[0].title, None);
        let blob = r#"["https://a.example/p","/x","ab","  A Title  "]"#;
        assert_eq!(
            parse_lens_results(blob)[0].title.as_deref(),
            Some("A Title")
        );
    }

    #[test]
    fn lens_escapes_are_resolved_by_hand() {
        let blob = r#"["https:\/\/a.example\/café","a \"quoted\" A\ud83d"]"#;
        let matches = parse_lens_results(blob);
        assert_eq!(matches[0].url, "https://a.example/caf\u{e9}");
        // A lone surrogate cannot be a Rust `char`; Python keeps it. See `unescape`.
        assert_eq!(matches[0].title.as_deref(), Some("a \"quoted\" A\u{fffd}"));
    }

    #[test]
    fn at_most_eight_results_and_the_score_floors_at_fifty() {
        let blob: String = (0..12)
            .map(|i| format!(r#""https://h{i}.example/p","Title {i}""#))
            .collect::<Vec<_>>()
            .join(",");
        let matches = parse_lens_results(&blob);
        assert_eq!(matches.len(), MAX_RESULTS);
        assert_eq!(matches[7].similarity, 64.0);
        assert!(matches.iter().all(|m| m.similarity >= SIMILARITY_FLOOR));
    }

    #[test]
    fn a_vision_value_python_would_raise_on_is_an_error() {
        for payload in [
            json!([]),
            json!({"responses": "abc"}),
            json!({"responses": {"0": {}}}),
            json!({"responses": [null]}),
            json!({"responses": [{"webDetection": [1]}]}),
            json!({"responses": [{"webDetection": {"fullMatchingImages": 5}}]}),
            json!({"responses": [{"webDetection": {"fullMatchingImages": [{"url": 5}]}}]}),
        ] {
            assert_eq!(
                parse_vision_results(&payload),
                Err(UnexpectedInput),
                "{payload}"
            );
        }
    }

    #[test]
    fn the_thumbnail_is_the_image_url_as_given_and_the_page_url_is_stripped_of_its_fragment() {
        let payload = json!({"responses": [{"webDetection": {"fullMatchingImages": [
            {"url": "  https://a.example/i.jpg#f  "},
        ]}}]});
        let m = parse_vision_results(&payload).unwrap().remove(0);
        assert_eq!(m.url, "https://a.example/i.jpg");
        assert_eq!(
            m.thumb_url.as_deref(),
            Some("  https://a.example/i.jpg#f  ")
        );
    }

    #[test]
    fn every_google_host_in_the_list_is_one() {
        let listed = [
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
        assert_eq!(GOOGLE_HOST_SUFFIXES, listed);
        for suffix in GOOGLE_HOST_SUFFIXES {
            assert!(is_google_host(suffix), "{suffix}");
            assert!(is_google_host(&format!("sub.{suffix}")), "sub.{suffix}");
            assert!(!is_google_host(&format!("not{suffix}")), "not{suffix}");
        }
    }

    #[test]
    fn a_redirector_names_its_target_by_q_then_url_then_imgrefurl() {
        let page = |href: &str| format!(r#"<a href="{href}"><h3>t</h3></a>"#);
        let one = |href: &str| {
            parse_results(&page(href), "https://www.google.com/")
                .unwrap()
                .into_iter()
                .map(|m| m.url)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            one("/url?imgrefurl=https://r.example/"),
            ["https://r.example/"]
        );
        assert_eq!(
            one("/url?url=https://u.example/&imgrefurl=https://r.example/"),
            ["https://u.example/"]
        );
        assert_eq!(
            one("/url?q=https://q.example/&url=https://u.example/"),
            ["https://q.example/"]
        );
        // /imgres is a redirector too; a target that is not http(s), or none, is dropped.
        assert_eq!(
            one("/imgres?imgrefurl=http://x.example/y"),
            ["http://x.example/y"]
        );
        assert_eq!(one("/url?q=ftp://x/"), Vec::<String>::new());
        assert_eq!(one("/imgres?q=x"), Vec::<String>::new());
    }

    #[test]
    fn the_same_link_twice_is_one_result() {
        let page = concat!(
            r#"<a href="https://a.example/p"><h3>first</h3></a>"#,
            r#"<a href="/url?q=https://a.example/p"><h3>second</h3></a>"#
        );
        let matches = parse_results(page, "").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title.as_deref(), Some("first"));
    }

    #[test]
    fn a_lens_title_is_between_three_and_two_hundred_characters() {
        let blob = |title: &str| format!(r#"["https://a.example/p","{title}"]"#);
        let title = |n: usize| parse_lens_results(&blob(&"t".repeat(n)))[0].title.clone();
        assert_eq!(title(2), None);
        assert_eq!(title(3).map(|t| t.len()), Some(3));
        assert_eq!(title(200).map(|t| t.len()), Some(200));
        assert_eq!(title(201), None);
    }

    #[test]
    fn a_lens_title_is_looked_for_in_the_next_three_literals_only() {
        let blob = r#"["https://a.example/p","/x","1234","A Title"]"#;
        assert_eq!(
            parse_lens_results(blob)[0].title.as_deref(),
            Some("A Title")
        );
        let blob = r#"["https://a.example/p","/x","1234","!!!","A Title"]"#;
        assert_eq!(parse_lens_results(blob)[0].title, None);
    }

    #[test]
    fn vision_stops_at_eight_results_across_the_bands() {
        let images: Vec<Value> = (0..12)
            .map(|i| json!({"url": format!("https://i{i}.example/a.jpg")}))
            .collect();
        let payload = json!({"responses": [{"webDetection": {"fullMatchingImages": images}}]});
        let matches = parse_vision_results(&payload).unwrap();
        assert_eq!(matches.len(), MAX_RESULTS);
        assert_eq!(matches[7].similarity, 64.0);
    }

    #[test]
    fn a_pages_image_is_its_thumbnail_unless_a_cdn_image_stands_for_the_post() {
        // The page names a CDN image that is a post: the post becomes the url, and the image,
        // as the page gave it, the thumbnail.
        let payload = json!({"responses": [{"webDetection": {"fullMatchingImages": [
            {"url": "  https://rule34storage.b-cdn.net/posts/1/2/3.jpg#f "},
            {"url": "https://rule34storage.b-cdn.net/posts/1/2/4.jpg"},
        ]}}]});
        let matches = parse_vision_results(&payload).unwrap();
        assert_eq!(
            matches[0].url,
            "https://rule34.us/index.php?r=posts/view&id=2"
        );
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("  https://rule34storage.b-cdn.net/posts/1/2/3.jpg#f ")
        );
        // The second image names the same post: one result.
        assert_eq!(matches.len(), 1);
        // A page with no image of its own gets the image it was recovered from.
        let payload = json!({"responses": [{"webDetection": {"pagesWithMatchingImages": [
            {"url": "https://rule34storage.b-cdn.net/posts/1/2/3.jpg"},
        ]}}]});
        let matches = parse_vision_results(&payload).unwrap();
        assert_eq!(
            matches[0].url,
            "https://rule34.us/index.php?r=posts/view&id=2"
        );
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://rule34storage.b-cdn.net/posts/1/2/3.jpg")
        );
    }

    // ---- `search`, through the two halves that read an answer ---------------------------------
    // `TestGoogleVisionWebDetection` (a key, a rejected key, an exhausted quota, a per-image
    // error) and `TestGoogleImagesBlocked` (a consent wall, a traffic check, a server error, the
    // JavaScript shell, a Lens landing).

    const SEARCH_URL: &str = "https://www.google.com/search?tbs=sbi:x";
    const CONSENT_URL: &str = "https://consent.google.com/m?continue=https://www.google.com/";
    const SORRY: &str = "<html><body>Our systems have detected unusual traffic from your computer \
                         network.</body></html>";
    const SHELL: &str = r#"<html><body><noscript><meta http-equiv="refresh" content="0;url=/httpservice/retry/enablejs?sei=x"><div>Please click <a href="/httpservice/retry/enablejs?sei=x">here</a> if you are not redirected within a few seconds.</div></noscript></body></html>"#;

    fn public(
        status: u16,
        url: &str,
        text: &str,
    ) -> Result<Vec<GoogleImagesMatch>, GoogleImagesError> {
        public_from_response(&Response { status, url, text })
    }

    #[test]
    fn a_key_picks_the_vision_route_and_a_blank_one_the_public_page() {
        assert_eq!(vision_key("k"), Some("k"));
        assert_eq!(vision_key("  k\u{a0}\n"), Some("k"));
        assert_eq!(vision_key(""), None);
        assert_eq!(vision_key(" \t\u{3000}"), None);
        let matches = vision_from_response(200, Some(&payload())).unwrap();
        assert_eq!(
            matches[0].url,
            "https://www.deviantart.com/someone/art/A-Picture-123"
        );
    }

    #[test]
    fn a_rejected_key_stands_down_for_the_batch() {
        let body = json!({"error": {"code": 403, "status": "PERMISSION_DENIED",
            "message": "Cloud Vision API has not been used in project 1"}});
        let error = vision_from_response(403, Some(&body)).unwrap_err();
        assert!(error.latches());
        assert!(matches!(error, GoogleImagesError::Blocked(_)));
        assert!(error.to_string().contains("Settings > Engine"));
    }

    #[test]
    fn an_exhausted_quota_stands_down_too() {
        let body = json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED", "message": "Quota exceeded"}});
        assert!(
            vision_from_response(429, Some(&body))
                .unwrap_err()
                .latches()
        );
        // The reason alone is enough, whatever the status.
        let error = vision_from_response(500, Some(&body)).unwrap_err();
        assert!(error.latches());
        assert!(
            error
                .to_string()
                .starts_with("Google Vision quota is exhausted")
        );
    }

    #[test]
    fn the_other_vision_failures_are_read_by_status_reason_and_message() {
        for (status, body, latches, message) in [
            (
                401,
                json!({}),
                true,
                "Google Vision refused the API key: HTTP 401. ",
            ),
            (
                500,
                json!({"error": {"status": "unauthenticated", "message": " nope "}}),
                true,
                "Google Vision refused the API key: nope. ",
            ),
            (
                400,
                json!({"error": {"message": "API key not valid. Please pass a valid API key."}}),
                true,
                "Google Vision rejected the API key: API key not valid.",
            ),
            (
                400,
                json!({"error": {"message": "Bad image data"}}),
                false,
                "Google Vision: Bad image data",
            ),
            (503, json!({"error": ""}), false, "Google Vision: HTTP 503"),
            (
                500,
                json!({"error": {"message": "   "}}),
                false,
                "Google Vision: HTTP 500",
            ),
        ] {
            let error = vision_from_response(status, Some(&body)).unwrap_err();
            assert_eq!(error.latches(), latches, "{status} {body}");
            assert!(
                error.to_string().starts_with(message),
                "{status} {body}: {error}"
            );
        }
        // A body that is not JSON reads as `{}`.
        assert_eq!(
            vision_from_response(502, None).unwrap_err(),
            GoogleImagesError::Failed("Google Vision: HTTP 502".to_owned())
        );
        assert_eq!(vision_from_response(200, None).unwrap(), vec![]);
    }

    #[test]
    fn a_per_image_error_is_not_latched() {
        let body = json!({"responses": [{"error": {"code": 3, "message": "Bad image data"}}]});
        let error = vision_from_response(200, Some(&body)).unwrap_err();
        assert_eq!(
            error,
            GoogleImagesError::Failed("Google Vision: Bad image data".to_owned())
        );
        assert!(!error.latches());
        // With no message the error itself is shown, as Python formats it.
        let body = json!({"responses": [{"error": {"code": 3}}]});
        assert_eq!(
            vision_from_response(200, Some(&body))
                .unwrap_err()
                .to_string(),
            "Google Vision: {'code': 3}"
        );
    }

    #[test]
    fn an_empty_or_malformed_vision_body_is_no_matches_not_an_error() {
        for body in [
            json!({}),
            json!({"responses": []}),
            json!({"responses": [{}]}),
            json!({"responses": [{"error": null}]}),
            json!({"responses": [{"error": {}}]}),
            json!({"responses": [{"webDetection": {}}]}),
        ] {
            assert_eq!(
                vision_from_response(200, Some(&body)).unwrap(),
                vec![],
                "{body}"
            );
        }
    }

    #[test]
    fn a_vision_body_python_raises_on_is_an_error() {
        for (status, body) in [
            (200, json!([])),
            (200, json!(null)),
            (403, json!([])),
            (200, json!({"responses": ["x"]})),
            (200, json!({"responses": "x"})),
            (200, json!({"responses": {"a": 1}})),
            (200, json!({"responses": 5})),
            (200, json!({"responses": [{"error": "boom"}]})),
            (200, json!({"responses": [{"error": [1]}]})),
            (400, json!({"error": "boom"})),
            (400, json!({"error": {"message": 5}})),
            (400, json!({"error": {"status": ["x"]}})),
        ] {
            assert_eq!(
                vision_from_response(status, Some(&body)),
                Err(GoogleImagesError::Unexpected(UnexpectedInput)),
                "{status} {body}"
            );
        }
    }

    #[test]
    fn a_consent_wall_is_recognised_and_latched() {
        // Served as an ordinary 200, which is why the status alone cannot tell results from a wall.
        let error = public(200, CONSENT_URL, "<html>consent</html>").unwrap_err();
        assert!(matches!(error, GoogleImagesError::Blocked(_)));
        assert!(error.latches());
        assert!(
            public(200, "https://www.google.de/sorry/index?continue=x", "")
                .unwrap_err()
                .latches()
        );
    }

    #[test]
    fn an_unusual_traffic_check_is_recognised() {
        let error = public(429, SEARCH_URL, SORRY).unwrap_err();
        assert!(error.latches());
        assert!(error.to_string().contains("Settings > Engine"));
        for marker in [
            "/sorry/index",
            "reCAPTCHA",
            "<div id=\"captcha\">",
            "UNUSUAL TRAFFIC FROM YOUR COMPUTER NETWORK",
        ] {
            assert!(
                public(200, SEARCH_URL, marker).unwrap_err().latches(),
                "{marker}"
            );
        }
        // Only the first 6000 characters are read.
        let late = format!("{}recaptcha", "x".repeat(6000));
        assert_eq!(public(200, SEARCH_URL, &late), Ok(vec![]));
    }

    #[test]
    fn a_plain_server_error_is_not_called_a_block() {
        let error = public(500, SEARCH_URL, "nope").unwrap_err();
        assert_eq!(
            error,
            GoogleImagesError::Failed("Google returned HTTP 500".to_owned())
        );
        assert!(!error.latches());
    }

    #[test]
    fn too_large_is_checked_before_the_challenge() {
        let error = public(413, CONSENT_URL, SORRY).unwrap_err();
        assert_eq!(
            error,
            GoogleImagesError::Failed(
                "Google rejected the image as too large, even after downscaling".to_owned()
            )
        );
    }

    #[test]
    fn a_javascript_only_page_is_reported_not_recorded_as_empty() {
        let error = public(200, SEARCH_URL, SHELL).unwrap_err();
        assert_eq!(error, GoogleImagesError::JavaScript);
        assert!(error.latches());
        assert!(error.to_string().contains("Vision")); // names the fix
        // Only an empty parse is an error; a shell that holds results is read.
        let with_results = format!("{SERP}{SHELL}");
        assert_eq!(public(200, SERP_URL, &with_results).unwrap().len(), 2);
        // The marker is looked for in the first 20000 characters only.
        let late = format!("{}{SHELL}", " ".repeat(20000));
        assert_eq!(public(200, SEARCH_URL, &late), Ok(vec![]));
    }

    #[test]
    fn a_lens_landing_is_parsed_through_the_lens_reader() {
        let matches = public(200, "https://lens.google.com/search?p=abc", LENS).unwrap();
        let urls: Vec<_> = matches.iter().map(|m| m.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://www.deviantart.com/someone/art/A-Picture-123"]
        );
        // The classic reader finds nothing on it.
        assert_eq!(public(200, SEARCH_URL, LENS), Ok(vec![]));
    }

    #[test]
    fn a_page_with_no_url_of_its_own_is_read_against_googles() {
        let page = r#"<a href="/url?q=https://www.pixiv.net/artworks/456&amp;sa=U"><h3>t</h3></a>"#;
        let matches = public(200, "", page).unwrap();
        assert_eq!(matches[0].url, "https://www.pixiv.net/artworks/456");
    }
}
