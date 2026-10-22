//! Google Lens result parsers (`core/google_lens.py`): the page body and the rendered tiles the
//! browser hands back, turned into matches.
//!
//! Three parsers, all pure: [`parse_lens_payload`] reads the "Visual matches" literals out of the
//! response body, [`parse_exact_matches`] recovers post URLs from the "Exact matches" titles in it,
//! and [`parse_exact_tiles`] does the same from the rendered tiles, which also carry a thumbnail
//! and the stated size. Driving the browser (`lens_browser.py`), the upload script, the rest
//! latch and `search()` are Phase 5; so are `rule34_searches`, `taggable_first` and
//! `keep_taggable_first`, which need the booru registry and the Rule 34 search.
//!
//! **Text is code points.** Python slices and counts `str`; the 2,500-character window of
//! `_label_is_near` and the 3..200 length of a title are counted in `char`s here.
//!
//! **Surrogate escapes.** `_unescape` turns `\uD800`..`\uDFFF` into a lone surrogate, which a Rust
//! `String` cannot hold; each reads as U+FFFD (not a letter, not a space, one code point, as the
//! surrogate is), so a title's length and the "has a letter" test agree and only the character
//! itself differs. No corpus item reaches this.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use super::ascii2d::host_label;
use crate::pyalpha::is_alpha;
use crate::pycase::ignorecase_matches;
use crate::pycompat::{html_unescape, strip, truthy};
use crate::pyimage::image_size;
use crate::pyrepr::py_str;

/// `SIMILARITY_START`, `SIMILARITY_STEP`, `SIMILARITY_FLOOR`: the made-up score a visual match gets
/// from its rank.
pub const SIMILARITY_START: f64 = 80.0;
pub const SIMILARITY_STEP: f64 = 2.0;
pub const SIMILARITY_FLOOR: f64 = 50.0;

/// `SITE_LABEL_WINDOW`: how many characters after a title `_label_is_near` looks for the site's name.
const SITE_LABEL_WINDOW: usize = 2500;

/// `PLACEHOLDER_MAX_SIDE`: a data-URI thumbnail no bigger than this is a lazy-load speck.
const PLACEHOLDER_MAX_SIDE: u32 = 4;

/// `GoogleLensMatch`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleLensMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub title: Option<String>,
    pub source_name: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// `_ExactSite`: a site whose Exact-matches title carries its post id.
struct ExactSite {
    /// Captures the id in group 1.
    pattern: Regex,
    /// The post URL, with one `{}` for the id.
    template: &'static str,
    host: &'static str,
    /// What Lens calls the site; only needed when the pattern could belong to more than one.
    label: &'static str,
}

/// `EXACT_MATCH_SITES`. `\d` is Unicode in both languages.
static EXACT_MATCH_SITES: LazyLock<[ExactSite; 3]> = LazyLock::new(|| {
    [
        ExactSite {
            pattern: Regex::new(r"Post (\d{3,9}):([^<\n]{0,200})").expect("paheal pattern"),
            template: "https://rule34.paheal.net/post/view/{}",
            host: "rule34.paheal.net",
            label: "Rule 34 Paheal",
        },
        ExactSite {
            pattern: Regex::new(r"Rule 34 \| (\d{3,9}) ?/").expect("rule34.xxx pattern"),
            template: "https://rule34.xxx/index.php?page=post&s=view&id={}",
            host: "rule34.xxx",
            label: "",
        },
        ExactSite {
            pattern: Regex::new(r"/ ?(\d{3,9}) ?- ?Rule34\.us").expect("rule34.us pattern"),
            template: "https://rule34.us/index.php?r=posts/view&id={}",
            host: "rule34.us",
            label: "",
        },
    ]
});

/// `_TAG_RE`.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("tag regex"));

/// `TILE_DIMENSIONS_RE`: `1,072x1,108` next to the title. `\s` is Python's, `[\s\x1c-\x1f]`.
static TILE_DIMENSIONS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([\d,]{2,10})[\s\x1c-\x1f]*[x\u{d7}][\s\x1c-\x1f]*([\d,]{2,10})")
        .expect("tile dimensions regex")
});

/// `_LITERAL_RE`: a quoted run of 2 to 400 characters, backslash escapes allowed. `.` skips only
/// `\n` in both languages.
static LITERAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:[^"\\]|\\.){2,400})""#).expect("literal regex"));

/// `\\u([0-9a-fA-F]{4})` in `_unescape`.
static UNICODE_ESCAPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\u([0-9a-fA-F]{4})").expect("unicode escape regex"));

/// The page half of `_IMAGE_OWNERS[0]`, compiled without flags. Python's `$` also matches before a
/// final `\n`, which is `\n?\z`.
static ZEROCHAN_PAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://(?:www\.)?zerochan\.net/(\d+)(?:[/?#]|\n?\z)").expect("zerochan page")
});

/// The image half, compiled with `re.IGNORECASE`. Python's rule is not the `regex` crate's `(?i)`:
/// it folds `s` with U+017F and `i` with U+0130 and U+0131 (`crate::pycase`), so those two letters
/// are spelled out as classes and the rest are plain cased pairs.
static ZEROCHAN_IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    let s = "[sS\u{17f}]";
    let i = "[iI\u{130}\u{131}]";
    let pattern = format!(
        r"^[hH][tT][tT][pP]{s}?://(?:{s}\d+|{s}[tT][aA][tT]{i}[cC])\.[zZ][eE][rR][oO][cC][hH][aA][nN]\.[nN][eE][tT]/[^?#]*\.(\d+)\.(?:[jJ][pP][eE]?[gG]|[pP][nN][gG]|[gG]{i}[fF]|[wW][eE][bB][pP])(?:[?#]|\n?\z)"
    );
    Regex::new(&pattern).expect("zerochan image")
});

/// `[a-z]` in a pattern compiled with `re.IGNORECASE`: some letter of the range matches `c`.
fn is_a_to_z_ignorecase(c: char) -> bool {
    ('a'..='z').any(|p| ignorecase_matches(p, c))
}

/// `GOOGLE_HOST_SUFFIXES`.
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

/// `GOOGLE_DOMAIN_RE.search(host)`: `(?:^|\.)google(?:\.[a-z]{2,3}){1,2}$`, case-insensitive. The
/// groups are tried by hand because `[a-z]` under `re.IGNORECASE` is wider than a plain range.
fn is_google_domain(host: &str) -> bool {
    let chars: Vec<char> = host.chars().collect();
    // `rest` after `google`: one or two `.xx` / `.xxx` groups reaching the end.
    fn groups(rest: &[char], left: u8) -> bool {
        if rest.first() != Some(&'.') {
            return false;
        }
        (2..=3).any(|n| {
            rest.len() > n
                && rest[1..=n].iter().all(|&c| is_a_to_z_ignorecase(c))
                && (rest.len() == n + 1 || (left > 1 && groups(&rest[n + 1..], left - 1)))
        })
    }
    (0..chars.len()).any(|start| {
        (start == 0 || chars[start - 1] == '.')
            && chars.len() >= start + 6
            && chars[start..start + 6]
                .iter()
                .zip("google".chars())
                .all(|(&c, p)| ignorecase_matches(p, c))
            && groups(&chars[start + 6..], 2)
    })
}

/// `is_google_host` (`core/google_images.py`).
fn is_google_host(host: &str) -> bool {
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    is_google_domain(host)
        || GOOGLE_HOST_SUFFIXES.iter().any(|suffix| {
            host == *suffix || host.strip_suffix(suffix).is_some_and(|p| p.ends_with('.'))
        })
}

/// `_label_is_near`: whether the site's name appears in this title or just after it, stopping at
/// the next result so one tile cannot borrow its neighbour's site.
fn label_is_near(body: &str, hit: regex::Match<'_>, site: &ExactSite) -> bool {
    let after = &body[hit.end()..];
    let end = after
        .char_indices()
        .nth(SITE_LABEL_WINDOW)
        .map_or(after.len(), |(i, _)| i);
    let mut window = &after[..end];
    if let Some(neighbour) = site.pattern.find(window) {
        window = &window[..neighbour.start()];
    }
    let label = site.label.to_lowercase();
    hit.as_str().to_lowercase().contains(&label) || window.to_lowercase().contains(&label)
}

/// `parse_exact_matches`: matches recovered from the Exact matches tab's titles. Nothing for a page
/// whose titles name no site this can rebuild a URL for.
pub fn parse_exact_matches(payload: &str) -> Vec<GoogleLensMatch> {
    let mut found: Vec<(usize, String, String, &'static str)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for site in EXACT_MATCH_SITES.iter() {
        for caps in site.pattern.captures_iter(payload) {
            let hit = caps.get(0).expect("group 0");
            if !site.label.is_empty() && !label_is_near(payload, hit, site) {
                continue;
            }
            let url = site.template.replace("{}", &caps[1]);
            if !seen.insert(url.clone()) {
                continue;
            }
            let title = strip(&html_unescape(&TAG_RE.replace_all(hit.as_str(), ""))).to_string();
            found.push((hit.start(), url, title, site.host));
        }
    }

    // Document order, so the page's own ranking survives being collected one site at a time.
    found.sort_by_key(|row| row.0);
    found
        .into_iter()
        .map(|(_, url, title, host)| GoogleLensMatch {
            url,
            thumb_url: None,
            similarity: 0.0,
            title: Some(title),
            source_name: Some(host.to_string()),
            width: None,
            height: None,
        })
        .collect()
}

/// `_tile_dimensions`: the source's size as the tile states it after the title.
fn tile_dimensions(text: &str, after: usize) -> (Option<i64>, Option<i64>) {
    let Some(found) = TILE_DIMENSIONS_RE.captures(&text[after..]) else {
        return (None, None);
    };
    // `int(group.replace(",", ""))`: an empty or comma-only side is a `ValueError`, and either
    // side failing drops both. At most 10 characters, so `i64` always holds it.
    let side = |group: &str| -> Option<i64> {
        let digits = group.replace(',', "");
        crate::boorus::sizes::to_int(&digits)
    };
    match (side(&found[1]), side(&found[2])) {
        (Some(w), Some(h)) => (Some(w), Some(h)),
        _ => (None, None),
    }
}

/// `base64.b64decode(data)` as `binascii.a2b_base64` reads a `str` outside strict mode: characters
/// outside the alphabet are skipped, padding ends the data once a quad has two characters, and
/// leftover characters in a quad are an error. `None` where Python raises (`ValueError` for
/// non-ASCII text, `binascii.Error` for a bad length).
fn b64decode(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() {
        return None;
    }
    let (mut out, mut quad, mut left, mut pads) = (Vec::new(), 0u8, 0u8, 0u8);
    for &c in text.as_bytes() {
        if c == b'=' {
            pads += 1;
            if quad >= 2 && quad + pads >= 4 {
                return Some(out);
            }
            continue;
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        pads = 0;
        match quad {
            0 => (quad, left) = (1, v),
            1 => {
                out.push((left << 2) | (v >> 4));
                (quad, left) = (2, v & 0x0f);
            }
            2 => {
                out.push((left << 4) | (v >> 2));
                (quad, left) = (3, v & 0x03);
            }
            _ => {
                out.push((left << 6) | v);
                (quad, left) = (0, 0);
            }
        }
    }
    (quad == 0).then_some(out)
}

/// `_is_placeholder_thumb`: a `data:` URI whose picture is a speck (a lazy-loaded tile that had not
/// loaded yet carries a 1x1 GIF). Anything that is not a readable image is left alone.
fn is_placeholder_thumb(src: &str) -> bool {
    if !src.starts_with("data:") {
        return false;
    }
    let Some((_, data)) = src.split_once(',') else {
        return false;
    };
    b64decode(data)
        .and_then(|bytes| image_size(&bytes))
        .is_some_and(|(w, h)| w.max(h) <= PLACEHOLDER_MAX_SIDE)
}

/// `str(tile.get(key) or "")`.
fn tile_text(tile: &serde_json::Map<String, Value>, key: &str) -> String {
    match tile.get(key) {
        Some(v) if truthy(v) => py_str(v),
        _ => String::new(),
    }
}

/// `parse_exact_tiles`: matches from the RENDERED exact-match tiles, `tiles` being the list the
/// page script returned (`{"text": ..., "thumb": ...}` objects). Anything that is not a list of
/// objects yields nothing for the entries that are not objects.
pub fn parse_exact_tiles(tiles: &Value) -> Vec<GoogleLensMatch> {
    let mut found = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let Some(tiles) = tiles.as_array() else {
        return found;
    };
    for tile in tiles {
        let Some(tile) = tile.as_object() else {
            continue;
        };
        let text = tile_text(tile, "text");
        let mut thumb = tile_text(tile, "thumb");
        if is_placeholder_thumb(&thumb) {
            // Still a match: only the picture is withheld.
            thumb.clear();
        }
        for site in EXACT_MATCH_SITES.iter() {
            let Some(caps) = site.pattern.captures(&text) else {
                continue;
            };
            if !site.label.is_empty() && !text.to_lowercase().contains(&site.label.to_lowercase()) {
                continue;
            }
            let hit = caps.get(0).expect("group 0");
            let url = site.template.replace("{}", &caps[1]);
            if !seen.insert(url.clone()) {
                continue;
            }
            let (width, height) = tile_dimensions(&text, hit.end());
            found.push(GoogleLensMatch {
                url,
                thumb_url: (!thumb.is_empty()).then(|| thumb.clone()),
                similarity: 0.0,
                title: Some(strip(hit.as_str()).to_string()),
                source_name: Some(site.host.to_string()),
                width,
                height,
            });
            break;
        }
    }
    found
}

/// `owner_of_image`: the post `image_url` belongs to, when it is on `page_url`'s site but names a
/// different post; otherwise `page_url` unchanged.
fn owner_of_image(page_url: &str, image_url: Option<&str>) -> String {
    let Some(image_url) = image_url.filter(|u| !u.is_empty()) else {
        return page_url.to_string();
    };
    if let (Some(page), Some(image)) = (
        ZEROCHAN_PAGE_RE.captures(page_url),
        ZEROCHAN_IMAGE_RE.captures(image_url),
    ) && page[1] != image[1]
    {
        return format!("https://www.zerochan.net/{}", &image[1]);
    }
    page_url.to_string()
}

/// `_unescape`: JSON-escaped slashes and unicode escapes back to plain text, by hand.
fn unescape(literal: &str) -> String {
    let text = literal.replace("\\/", "/").replace("\\\"", "\"");
    UNICODE_ESCAPE_RE
        .replace_all(&text, |caps: &regex::Captures| {
            let code = u32::from_str_radix(&caps[1], 16).expect("four hex digits");
            char::from_u32(code).unwrap_or('\u{fffd}').to_string()
        })
        .into_owned()
}

/// `_looks_like_a_title`: a page title, as opposed to a URL or one of Google's own tokens
/// (`XORNM3qJQeMrvM`, which has no spaces).
fn looks_like_a_title(text: &str) -> bool {
    if !(3..=200).contains(&text.chars().count()) {
        return false;
    }
    if ["http://", "https://", "/", "data:"]
        .iter()
        .any(|p| text.starts_with(p))
    {
        return false;
    }
    text.chars().any(is_alpha) && text.contains(' ')
}

/// `parse_lens_payload`: a Lens results response body as matches, every one of them (the caller
/// cuts the list).
pub fn parse_lens_payload(payload: &str) -> Vec<GoogleLensMatch> {
    let literals: Vec<String> = LITERAL_RE
        .captures_iter(payload)
        .map(|m| unescape(&m[1]))
        .collect();
    let mut matches: Vec<GoogleLensMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut previous_url: Option<String> = None;

    for (index, literal) in literals.iter().enumerate() {
        if !(literal.starts_with("http://") || literal.starts_with("https://")) {
            continue;
        }
        let host = host_label(literal).unwrap_or_default();
        if host.is_empty() || is_google_host(&host) {
            continue;
        }
        let url = literal.split('#').next().unwrap_or(literal);
        let following = literals.get(index + 1).map_or("", |next| strip(next));

        if !looks_like_a_title(following) {
            // No title of its own: this is the picture belonging to the result that comes next.
            previous_url = Some(url.to_string());
            continue;
        }
        let url = owner_of_image(url, previous_url.as_deref());
        if seen.contains(&url) {
            previous_url = None;
            continue;
        }
        seen.insert(url.clone());
        let similarity =
            (SIMILARITY_START - matches.len() as f64 * SIMILARITY_STEP).max(SIMILARITY_FLOOR);
        matches.push(GoogleLensMatch {
            url,
            thumb_url: previous_url.take(),
            similarity,
            title: Some(following.to_string()),
            source_name: Some(host),
            width: None,
            height: None,
        });
    }
    matches
}
