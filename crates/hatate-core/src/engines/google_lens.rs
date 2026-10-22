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

use super::google_images::{host_label, is_google_host};
use crate::pyalpha::is_alpha;
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
        if digits.is_empty() {
            return None;
        }
        crate::boorus::sizes::to_int(&digits)
    };
    match (side(&found[1]), side(&found[2])) {
        (Some(w), Some(h)) => (Some(w), Some(h)),
        _ => (None, None),
    }
}

/// `base64.b64decode(data)` as `binascii.a2b_base64` reads a `str` outside strict mode (CPython
/// 3.14): characters outside the alphabet are skipped and padding never ends the data. A `=`
/// counts only once a quad has two characters, and any data character after it resets the count;
/// at the end a quad of one character is an error, and so is a quad of two or three that
/// `=` has not filled to four. `None` where Python raises (`ValueError` for non-ASCII text,
/// `binascii.Error` for a bad length).
fn b64decode(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() {
        return None;
    }
    let (mut out, mut quad, mut left, mut pads) = (Vec::new(), 0u8, 0u8, 0u8);
    for &c in text.as_bytes() {
        let v = match c {
            b'=' => {
                if quad >= 2 {
                    pads = pads.saturating_add(1);
                }
                continue;
            }
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
    (quad == 0 || (quad >= 2 && quad + pads >= 4)).then_some(out)
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Each test below is `tests/test_search_quality.py` at 76c85c4, `TestGoogleLensPayloadParser`
    // or `TestGoogleLensExactMatches`, by the same name. The `search()` tests of those classes
    // (`test_search_brings_a_deep_booru_post_to_the_top`, `test_exact_matches_outrank_visual_ones`,
    // `test_scores_stay_ordered_after_the_two_tabs_are_merged`) drive the browser and are Phase 5.

    const PAYLOAD: &str = concat!(
        r#"["https:\/\/preview.redd.it\/doom-thumb.jpeg","XORNM3qJQeMrvM","#,
        r#""https:\/\/www.reddit.com\/r\/Doom\/comments\/1gdo146\/doom_slayer\/","#,
        r#""Doom Slayer only cares about Killing Demons","#,
        r#""https:\/\/www.google.com\/search?q=x","ignored","#,
        r#""https:\/\/sceneario.com\/onmars.jpg","PMmAJBJTUg_9sM","#,
        r#""https:\/\/sceneario.com\/galerie\/on-mars-t1\/","#,
        r#""ON MARS - T1 - S. Runberg\u002fGrun"]"#,
    );

    fn urls(matches: &[GoogleLensMatch]) -> Vec<&str> {
        matches.iter().map(|m| m.url.as_str()).collect()
    }

    #[test]
    fn results_past_the_first_eight_are_not_dropped() {
        let mut parts = Vec::new();
        for i in 0..12 {
            parts.push(format!(r#""https:\/\/www.pinterest.com\/pin\/{i}\/""#));
            parts.push(format!(r#""Pin number {i} of many""#));
        }
        parts.push(r#""https:\/\/danbooru.donmai.us\/posts\/42""#.to_string());
        parts.push(r#""Danbooru post forty two""#.to_string());
        let matches = parse_lens_payload(&format!("[{}]", parts.join(",")));
        assert_eq!(matches.len(), 13);
        assert_eq!(matches[12].url, "https://danbooru.donmai.us/posts/42");
    }

    #[test]
    fn source_pages_are_extracted_with_their_titles() {
        let matches = parse_lens_payload(PAYLOAD);
        assert_eq!(
            urls(&matches),
            [
                "https://www.reddit.com/r/Doom/comments/1gdo146/doom_slayer/",
                "https://sceneario.com/galerie/on-mars-t1/"
            ]
        );
        assert_eq!(
            matches[0].title.as_deref(),
            Some("Doom Slayer only cares about Killing Demons")
        );
    }

    #[test]
    fn the_picture_before_a_result_becomes_its_thumbnail() {
        let matches = parse_lens_payload(PAYLOAD);
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://preview.redd.it/doom-thumb.jpeg")
        );
        assert_eq!(
            matches[1].thumb_url.as_deref(),
            Some("https://sceneario.com/onmars.jpg")
        );
    }

    #[test]
    fn an_image_id_is_never_mistaken_for_a_title() {
        assert!(!looks_like_a_title("XORNM3qJQeMrvM"));
        assert!(looks_like_a_title("Doom Slayer only cares"));
        // The rest of `_looks_like_a_title`: length 3..=200 in code points, no URL or data prefix,
        // a letter (`str.isalpha`, so a roman numeral or a combining mark is not one), a space.
        assert!(looks_like_a_title("a b"));
        assert!(!looks_like_a_title("ab"));
        assert!(looks_like_a_title(&format!("a{}", " ".repeat(199))));
        assert!(!looks_like_a_title(&format!("a{}", " ".repeat(200))));
        assert!(looks_like_a_title("\u{3042} \u{3044}"));
        assert!(!looks_like_a_title("1 2 3"));
        assert!(!looks_like_a_title("\u{2167} \u{2167}"));
        assert!(!looks_like_a_title("/path x"));
        assert!(!looks_like_a_title("data:abc x"));
        assert!(!looks_like_a_title("https://x y"));
    }

    #[test]
    fn googles_own_links_are_not_sources() {
        assert!(
            !parse_lens_payload(PAYLOAD)
                .iter()
                .any(|m| m.url.contains("google.com"))
        );
        for host in [
            "google.com",
            "www.google.co.uk",
            "google.com.au",
            "lh3.googleusercontent.com",
            "gstatic.com",
            "goo.gl",
            "W3.ORG",
        ] {
            assert!(is_google_host(host), "{host}");
        }
        for host in [
            "notgoo.gl",
            "google.abcd",
            "google.c",
            "mygoogle.com",
            "example.com",
        ] {
            assert!(!is_google_host(host), "{host}");
        }
        // `[a-z]` under `re.IGNORECASE` also takes U+017F and U+0131.
        assert!(is_google_host("google.co\u{17f}"));
        assert!(is_google_host("google.c\u{131}o"));
    }

    #[test]
    fn escapes_are_resolved() {
        let matches = parse_lens_payload(PAYLOAD);
        assert_eq!(
            matches[1].title.as_deref(),
            Some("ON MARS - T1 - S. Runberg/Grun")
        );
        // The order of the replacements: `\\u0041` is a backslash and `A`'s tail, not an `A`.
        assert_eq!(unescape(r"a\/b\u0041\\u0041"), r"a/bA\A");
        assert_eq!(unescape(r#"say \"hi\""#), r#"say "hi""#);
        // A surrogate escape has no `char`.
        assert_eq!(unescape(r"\ud83d\ude00"), "\u{fffd}\u{fffd}");
    }

    #[test]
    fn results_are_scored_in_order() {
        let matches = parse_lens_payload(PAYLOAD);
        assert!(matches[0].similarity > matches[1].similarity);
        const {
            assert!(SIMILARITY_START > super::super::google_images::SIMILARITY_START);
            assert!(SIMILARITY_START < super::super::ascii2d::COLOR_SIMILARITY_START);
        }
        // The floor holds past 16 results.
        let parts: Vec<String> = (0..20)
            .flat_map(|i| {
                [
                    format!(r#""https://a.example/{i}""#),
                    format!(r#""Title {i} here""#),
                ]
            })
            .collect();
        let many = parse_lens_payload(&parts.join(","));
        assert_eq!(many.len(), 20);
        assert_eq!(many[15].similarity, 50.0);
        assert_eq!(many[19].similarity, SIMILARITY_FLOOR);
    }

    #[test]
    fn junk_is_no_matches_not_a_crash() {
        for payload in ["", "<html></html>", r#""not a url""#, "null"] {
            assert!(parse_lens_payload(payload).is_empty(), "{payload:?}");
            assert!(parse_exact_matches(payload).is_empty(), "{payload:?}");
        }
        for payload in ["<div>nothing here</div>", "Post : - Rule 34 Paheal"] {
            assert!(parse_exact_matches(payload).is_empty(), "{payload:?}");
        }
    }

    #[test]
    fn a_picture_of_one_zerochan_post_on_another_posts_page_is_paired_with_its_own() {
        let payload = r#"["https://s1.zerochan.net/Sakamata.Chloe.600.3699483.jpg","https://www.zerochan.net/3700079","Sakamata Chloe - Zerochan"]"#;
        let matches = parse_lens_payload(payload);
        assert_eq!(urls(&matches), ["https://www.zerochan.net/3699483"]);
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("https://s1.zerochan.net/Sakamata.Chloe.600.3699483.jpg")
        );
        // The same post, or a page of another site, is left alone.
        assert_eq!(
            owner_of_image(
                "https://www.zerochan.net/3699483",
                Some("https://s1.zerochan.net/a.3699483.jpg")
            ),
            "https://www.zerochan.net/3699483"
        );
        assert_eq!(
            owner_of_image(
                "https://example.com/1",
                Some("https://s1.zerochan.net/a.2.jpg")
            ),
            "https://example.com/1"
        );
        // `re.IGNORECASE` folds `i` with U+0131 and `s` with U+017F in the image URL; the page
        // pattern has no flag.
        assert_eq!(
            owner_of_image(
                "https://www.zerochan.net/1",
                Some("https://\u{17f}tat\u{131}c.zerochan.net/a.2.g\u{131}f")
            ),
            "https://www.zerochan.net/2"
        );
        assert_eq!(
            owner_of_image(
                "HTTPS://www.zerochan.net/1",
                Some("https://s1.zerochan.net/a.2.jpg")
            ),
            "HTTPS://www.zerochan.net/1"
        );
        // `$` also matches before a final newline.
        assert_eq!(
            owner_of_image(
                "https://www.zerochan.net/1\n",
                Some("https://s1.zerochan.net/a.2.jpg\n")
            ),
            "https://www.zerochan.net/2"
        );
    }

    const EXACT: &str = concat!(
        r#"<div class="ZhosBf T7iOye" style="-webkit-line-clamp:2">"#,
        "Post 5674224: Ben_10 Flipherrrr Gwen_Tennyson - Rule 34 Paheal</div>",
        r#"<div class="xuPcX yUTMj">Rule 34 Paheal</div>"#,
        r#"<div class="ZhosBf T7iOye">Post 5674250: Ben_10 Flipherrrr Gwen_Tennyson "#,
        "&amp; friends - Rule 34 Paheal</div>",
        r#"<div class="ZhosBf">Gwen Tennyson - Page 8 - HentaiRox</div>"#,
    );
    const RULE34XXX: &str = r#"<div class="ZhosBf">Rule 34 | 4585855 / - Rule 34</div>"#;
    const RULE34US: &str = concat!(
        r#"<div class="ZhosBf">If it exists, there is porn of it / tekuho / "#,
        "368224 - Rule34.us</div>"
    );

    #[test]
    fn a_post_id_in_the_title_becomes_the_post_url() {
        let matches = parse_exact_matches(EXACT);
        assert_eq!(
            urls(&matches),
            [
                "https://rule34.paheal.net/post/view/5674224",
                "https://rule34.paheal.net/post/view/5674250"
            ]
        );
        // The title is unescaped and stripped; the second carries `&amp;` -> `&`.
        assert_eq!(
            matches[1].title.as_deref(),
            Some("Post 5674250: Ben_10 Flipherrrr Gwen_Tennyson & friends - Rule 34 Paheal")
        );
        assert_eq!(matches[0].source_name.as_deref(), Some("rule34.paheal.net"));
        assert_eq!(matches[0].similarity, 0.0);
        assert_eq!(matches[0].thumb_url, None);
    }

    #[test]
    fn the_site_name_is_found_when_it_is_not_in_the_title() {
        let payload = format!(
            "{}{}{}",
            r#"<div class="ZhosBf T7iOye" style="-webkit-line-clamp:2">Post 5674080: Chel Tanluca The_Road_to_El_Dorado</div>"#,
            format_args!(
                r#"<div class="oYQBg">{}</div>"#,
                "<span>filler</span>".repeat(60)
            ),
            r#"<div class="xuPcX yUTMj">Rule 34 Paheal</div>"#,
        );
        assert_eq!(
            urls(&parse_exact_matches(&payload)),
            ["https://rule34.paheal.net/post/view/5674080"]
        );
    }

    #[test]
    fn a_tile_cannot_borrow_the_next_tile_s_site_name() {
        let payload = concat!(
            "<div>Post 111111: something on a site we cannot rebuild</div>",
            "<div>Post 222222: a real one</div>",
            r#"<div class="xuPcX">Rule 34 Paheal</div>"#
        );
        assert_eq!(
            urls(&parse_exact_matches(payload)),
            ["https://rule34.paheal.net/post/view/222222"]
        );
    }

    #[test]
    fn the_site_name_is_not_searched_for_indefinitely() {
        let payload = format!(
            "<div>Post 333333: a post</div>{}<div>Rule 34 Paheal</div>",
            "<span>x</span>".repeat(400)
        );
        assert!(payload.chars().count() > SITE_LABEL_WINDOW);
        assert!(parse_exact_matches(&payload).is_empty());
        // The window is counted in characters, not bytes: 2,500 two-byte characters still fit.
        let near = format!(
            "<div>Post 333333: a post</div>{}Rule 34 Paheal",
            "\u{e9}".repeat(2480)
        );
        assert_eq!(parse_exact_matches(&near).len(), 1);
    }

    #[test]
    fn a_rule34_xxx_post_is_rebuilt_from_its_title() {
        assert_eq!(
            urls(&parse_exact_matches(RULE34XXX)),
            ["https://rule34.xxx/index.php?page=post&s=view&id=4585855"]
        );
    }

    #[test]
    fn a_rule34_us_post_is_rebuilt_from_its_title() {
        assert_eq!(
            urls(&parse_exact_matches(RULE34US)),
            ["https://rule34.us/index.php?r=posts/view&id=368224"]
        );
    }

    #[test]
    fn the_two_rule34_sites_are_told_apart() {
        let both = format!("{RULE34XXX}{RULE34US}");
        let mut found: Vec<String> = parse_exact_matches(&both)
            .into_iter()
            .map(|m| m.url)
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                "https://rule34.us/index.php?r=posts/view&id=368224",
                "https://rule34.xxx/index.php?page=post&s=view&id=4585855"
            ]
        );
    }

    #[test]
    fn the_rule34_mirrors_are_not_mistaken_for_it() {
        let mirrors = concat!(
            "<div>favela funk - Rule 34 XYZ</div>",
            "<div>disney, pixar - Rule 34 World</div>",
            "<div>the incredibles - Rule 34 Archive</div>",
            "<div>Rule34 - If it exists, there is porn of it / phat smash</div>"
        );
        assert!(parse_exact_matches(mirrors).is_empty());
    }

    #[test]
    fn both_sites_are_recovered_in_document_order() {
        let found = parse_exact_matches(&format!("{RULE34XXX}{EXACT}"));
        assert_eq!(
            found[0].url,
            "https://rule34.xxx/index.php?page=post&s=view&id=4585855"
        );
        assert!(found.iter().any(|m| m.url.contains("paheal")));
    }

    #[test]
    fn titles_naming_no_rebuildable_site_are_left_alone() {
        assert!(
            !parse_exact_matches(EXACT)
                .iter()
                .any(|m| m.url.to_lowercase().contains("hentairox"))
        );
    }

    #[test]
    fn a_rendered_tile_yields_its_thumbnail_and_the_source_size() {
        let tiles = json!([{
            "text": "If it exists, there is porn of it / tekuho / 368224 - Rule34.us 1,072x1,108 Rule34.us",
            "thumb": "data:image/jpeg;base64,AAAA"
        }]);
        let matches = parse_exact_tiles(&tiles);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].url,
            "https://rule34.us/index.php?r=posts/view&id=368224"
        );
        assert_eq!(
            (matches[0].width, matches[0].height),
            (Some(1072), Some(1108))
        );
        assert_eq!(
            matches[0].thumb_url.as_deref(),
            Some("data:image/jpeg;base64,AAAA")
        );
        assert_eq!(matches[0].title.as_deref(), Some("/ 368224 - Rule34.us"));
    }

    #[test]
    fn a_lazy_load_placeholder_is_not_taken_for_a_thumbnail() {
        let tiles = json!([{
            "text": "If it exists, there is porn of it / gattles / 6608448 - Rule34.us",
            "thumb": "data:image/gif;base64,R0lGODlhAQABAIAAAP///////yH5BAEKAAEALAAAAAABAAEAAAICTAEAOw=="
        }]);
        let matches = parse_exact_tiles(&tiles);
        assert_eq!(
            urls(&matches),
            ["https://rule34.us/index.php?r=posts/view&id=6608448"]
        );
        assert_eq!(matches[0].thumb_url, None);
    }

    #[test]
    fn a_tile_that_names_no_rebuildable_site_is_ignored() {
        assert!(parse_exact_tiles(&json!([{"text": "Sign in", "thumb": "data:x"}])).is_empty());
        assert!(parse_exact_tiles(&Value::Null).is_empty());
        // Entries that are not objects, and a tile that is missing both keys, are skipped.
        assert!(parse_exact_tiles(&json!([null, 5, "x", [], {}])).is_empty());
    }

    #[test]
    fn a_tile_is_read_the_way_python_reads_str_and_dimensions() {
        let tile = |text: Value| parse_exact_tiles(&json!([{ "text": text }]));
        // `str(x or "")`: a falsy value is empty; another type is its `repr`.
        assert!(tile(json!(0)).is_empty() && tile(json!(null)).is_empty());
        // Either side that is not a number drops both; a comma-only side is the failing case.
        let sized = |suffix: &str| {
            let m = &tile(json!(format!("/ 123 - Rule34.us {suffix}")))[0];
            (m.width, m.height)
        };
        assert_eq!(sized("1,072x1,108"), (Some(1072), Some(1108)));
        assert_eq!(sized("250 \u{d7} 177"), (Some(250), Some(177)));
        assert_eq!(sized(",,x,,"), (None, None));
        assert_eq!(sized("1,2x,,"), (None, None));
        assert_eq!(sized("99x"), (None, None));
        assert_eq!(sized("\u{661}\u{662}x\u{663}\u{664}"), (Some(12), Some(34)));
        // The first pair is the one read, even when it is unusable.
        assert_eq!(sized(",,x,, 5x6"), (None, None));
    }

    #[test]
    fn the_same_post_in_two_tiles_is_listed_once_with_the_first() {
        let tiles = json!([
            {"text": "/ 123 - Rule34.us 55x66", "thumb": ""},
            {"text": "/ 123 - Rule34.us 77x88", "thumb": ""},
        ]);
        let matches = parse_exact_tiles(&tiles);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].width, Some(55));
    }

    #[test]
    fn base64_is_read_the_way_python_3_14_reads_a_str() {
        assert_eq!(b64decode("QUJD"), Some(b"ABC".to_vec()));
        assert_eq!(b64decode("QUJDRA=="), Some(b"ABCD".to_vec()));
        // Characters outside the alphabet are skipped; padding does not end the data.
        assert_eq!(b64decode("QU JD\n"), Some(b"ABC".to_vec()));
        assert_eq!(b64decode("QUJDRA==AA"), Some(b"ABCD\0\0".to_vec()));
        assert_eq!(b64decode("QQ==QQ=="), Some(vec![b'A', 4, 16]));
        assert_eq!(b64decode("=QQ=="), Some(b"A".to_vec()));
        // Short of padding, and one character over a quad.
        for bad in [
            "QQ", "QQ=", "QUJDRA=", "Q", "QUJDR", "QUI=AA", "QUJD=AA", "Q=U=J", "QQ=x",
        ] {
            assert_eq!(b64decode(bad), None, "{bad}");
        }
        // `str.encode("ascii")` fails first.
        assert_eq!(b64decode("QUJD\u{e9}"), None);
    }

    #[test]
    fn only_a_decodable_speck_is_a_placeholder() {
        let gif =
            "data:image/gif;base64,R0lGODlhAQABAIAAAP///////yH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==";
        assert!(is_placeholder_thumb(gif));
        // Unreadable, not an image, not a data URI, or no payload: left to the caller.
        for src in [
            "data:image/gif;base64,R0lGODlhAQABAIAAAP///////yH5BAEKAAEALAAAAAABAAEAAAICTAEAOw=",
            "data:image/jpeg;base64,AAAA",
            "data:image/gif;base64",
            "data:",
            "",
            "http://x/y.gif",
            "xxxxxxxx",
        ] {
            assert!(!is_placeholder_thumb(src), "{src}");
        }
    }
}
