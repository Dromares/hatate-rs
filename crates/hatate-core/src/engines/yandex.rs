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

/// Danbooru's mirrors (`^(?:[a-z0-9-]+\.)?donma[i\u{131}\u{130}]\.(?:us|moe)$`, `re.IGNORECASE`): Yandex indexed
/// some posts under them, and only the main host has a parser.
static DANBOORU_MIRROR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[a-z0-9\-\u{131}\u{130}]+\.)?donma[i\u{131}\u{130}]\.(?:us|moe)\z")
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
        let Some(url) = post_url(raw_url)? else {
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
///
/// `Err` where Python's `boorus.find_parser` raises `ValueError` out of `urlparse` (see
/// [`boorus::host::raises_value_error`]), which `post_url` does not catch.
pub fn post_url(url: &str) -> Result<Option<String>, UnexpectedInput> {
    // Yandex tags every outbound link with its own utm_ parameters.
    let mut url = UTM_RE.replace_all(pycompat::strip(url), "").into_owned();
    if !url.contains('?') && url.contains('&') {
        url = url.replacen('&', "?", 1);
    }
    let Ok(mut parts) = pyurl::try_urlparse(&url, "") else {
        return Ok(None);
    };
    if DANBOORU_MIRROR_RE.is_match(&parts.netloc) && parts.netloc != "danbooru.donmai.us" {
        parts.netloc = "danbooru.donmai.us".to_string();
        url = pyurl::urlunparse(&parts);
    }
    if !POST_URL_RE.is_match(&url) {
        return Ok(None);
    }
    let url = canonicalize_url(&url);
    if boorus::host::raises_value_error(&url) {
        return Err(UnexpectedInput);
    }
    Ok(boorus::find_parser(&url).map(|_| url))
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

#[cfg(test)]
mod tests {
    //! `tests/test_yandex.py::TestReadingTheResultsPage` at `76c85c4`, test for test. The
    //! `TestStandingDown` tests are the request half (the captcha rest, the stand-down after three
    //! empty pages, the pacing) and `TestTakingPart` is engine settings: Phase 3.

    use super::*;
    use serde_json::json;

    /// `_site(url, domain, w, h)`: Yandex tags every outbound link with its own utm_ parameters.
    fn site(url: &str, domain: &str) -> Value {
        let tagged = if url.contains('?') {
            format!("{url}&utm_medium=organic&utm_source=yandexsmartcamera")
        } else {
            format!("{url}?utm_medium=organic&utm_source=yandexsmartcamera")
        };
        json!({
            "title": "t",
            "url": tagged,
            "domain": domain,
            "thumb": {"url": "//avatars.mds.yandex.net/i?id=abc&n=13", "width": 148, "height": 90},
            "originalImage": {"url": "https://img.example/1.jpg", "width": 1000, "height": 1400},
        })
    }

    /// `html.escape(text, quote=True)`.
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    /// `_page(sites)`: other data-state blobs first, then the one carrying cbirSites,
    /// HTML-escaped as served.
    fn page(sites: &[Value]) -> String {
        let other = escape(&json!({"form": {"action": "/images/search"}}).to_string());
        let state = escape(&json!({"initialState": {"cbirSites": {"sites": sites}}}).to_string());
        format!(
            "<html><div data-state=\"{other}\"></div><div class=\"CbirSites\" data-state=\"{state}\"></div></html>"
        )
    }

    fn sites() -> Vec<Value> {
        vec![
            site(
                "https://ru.pinterest.com/pin/267753140320940932/",
                "ru.pinterest.com",
            ),
            site(
                "https://rule34.us/index.php?r=posts%2Findex&q=dirtynicky",
                "rule34.us",
            ),
            site(
                "https://rule34.us/index.php?r=posts%2Fview&id=757670",
                "rule34.us",
            ),
            site(
                "https://safebooru.org/index.php?page=post&s=list&tags=baku-p",
                "safebooru.org",
            ),
            site(
                "https://kagamihara.donmai.us/posts/6307169?q=1girl",
                "kagamihara.donmai.us",
            ),
            site("https://telegra.ph/Some-post-01-01", "telegra.ph"),
            site(
                "https://xbooru.com/index.php?page=post&s=view&id=855502",
                "xbooru.com",
            ),
        ]
    }

    #[test]
    fn finds_the_sites_block_among_the_others() {
        let sites = sites();
        assert_eq!(
            parse_sites(&page(&sites)).unwrap().unwrap().len(),
            sites.len()
        );
    }

    #[test]
    fn a_page_with_no_block_is_not_an_empty_answer() {
        assert_eq!(
            parse_sites("<html><div data-state=\"{}\"></div></html>"),
            Ok(None)
        );
        assert_eq!(parse_sites(&page(&[])), Ok(Some(vec![])));
    }

    #[test]
    fn keeps_only_post_pages_the_app_can_read() {
        // Pinterest and Telegraph carry no tags; a tag search or a post list is not the post,
        // even on a site with a parser.
        let found = parse_sites(&page(&sites())).unwrap().unwrap();
        let urls: Vec<String> = keep_post_pages(&found)
            .unwrap()
            .into_iter()
            .map(|m| m.url)
            .collect();
        assert_eq!(urls.len(), 3);
        assert!(urls[0].contains("rule34.us") && urls[0].contains("757670"));
        assert!(urls[1].starts_with("https://danbooru.donmai.us/posts/6307169"));
        assert!(urls[2].contains("855502"));
        assert!(!urls.iter().any(|u| u.contains("utm_")));
    }

    #[test]
    fn scored_by_position_among_the_kept() {
        let found = parse_sites(&page(&sites())).unwrap().unwrap();
        let matches = keep_post_pages(&found).unwrap();
        let scores: Vec<f64> = matches.iter().map(|m| m.similarity).collect();
        assert_eq!(scores, [80.0, 78.0, 76.0]);
        assert!(
            matches[0]
                .thumb_url
                .as_deref()
                .unwrap()
                .starts_with("https://avatars.mds.yandex.net/")
        );
        assert_eq!(
            (matches[0].width, matches[0].height),
            (Some(1000), Some(1400))
        );
    }

    #[test]
    fn the_same_post_twice_is_one_row() {
        let sites = sites();
        let twice = [sites[2].clone(), sites[2].clone()];
        assert_eq!(keep_post_pages(&twice).unwrap().len(), 1);
    }

    // ---- what Python's code does that its tests do not say ----

    #[test]
    fn a_dozen_posts_are_cut_at_eight_and_the_score_floors_at_fifty() {
        let many: Vec<Value> = (1..=12)
            .map(|id| site(&format!("https://danbooru.donmai.us/posts/{id}"), "d"))
            .collect();
        let matches = keep_post_pages(&many).unwrap();
        assert_eq!(matches.len(), MAX_RESULTS);
        assert_eq!(matches[7].similarity, 66.0);
        assert!(matches.iter().all(|m| m.similarity >= SIMILARITY_FLOOR));
    }

    #[test]
    fn a_danbooru_mirror_is_the_main_host_and_a_protocol_relative_thumb_gets_https() {
        let mirror = json!({
            "url": "https://kagamihara.donmai.us/posts/6307169?q=1girl&utm_source=y",
            "thumb": {"url": "//t.example/a.jpg"},
            "originalImage": {"url": "https://o.example/b.jpg", "width": "1000", "height": 1400.9},
        });
        let m = keep_post_pages(&[mirror]).unwrap().remove(0);
        assert_eq!(m.url, "https://danbooru.donmai.us/posts/6307169?q=1girl");
        assert_eq!(m.thumb_url.as_deref(), Some("https://t.example/a.jpg"));
        // `int("1000")` and `int(1400.9)`.
        assert_eq!((m.width, m.height), (Some(1000), Some(1400)));
        assert_eq!(m.source_name.as_deref(), Some("danbooru.donmai.us"));
    }

    #[test]
    fn with_no_thumb_the_original_image_is_the_thumbnail() {
        let no_thumb = json!({
            "url": "https://danbooru.donmai.us/posts/1",
            "originalImage": {"url": "https://o.example/b.jpg"},
        });
        let m = keep_post_pages(&[no_thumb]).unwrap().remove(0);
        assert_eq!(m.thumb_url.as_deref(), Some("https://o.example/b.jpg"));
        assert_eq!((m.width, m.height), (None, None));
    }

    #[test]
    fn a_value_python_would_raise_on_is_an_error_and_not_an_empty_answer() {
        // `(site.get("thumb") or {}).get("url")` on a list is an AttributeError.
        let bad_thumb = json!({"url": "https://danbooru.donmai.us/posts/1", "thumb": [1]});
        assert_eq!(keep_post_pages(&[bad_thumb]), Err(UnexpectedInput));
        // `"x".strip()` is fine, `5 .strip()` is not.
        assert_eq!(keep_post_pages(&[json!({"url": 5})]), Err(UnexpectedInput));
        // A site that is not an object is skipped, as `isinstance(site, dict)` skips it.
        assert_eq!(keep_post_pages(&[json!("x"), json!(null)]), Ok(vec![]));
        // A block that parses to a list: `state.get` is an AttributeError.
        assert_eq!(
            parse_sites("<div data-state=\"[&quot;cbirSites&quot;]\"></div>"),
            Err(UnexpectedInput)
        );
        // One that is not JSON at all is skipped and the search goes on.
        assert_eq!(
            parse_sites(
                "<div data-state=\"cbirSites {\"></div><div data-state=\"{&quot;initialState&quot;: {&quot;cbirSites&quot;: {}}}\"></div>"
            ),
            Ok(Some(vec![]))
        );
    }

    #[test]
    fn a_post_url_python_cannot_parse_is_dropped_or_an_error_as_python_does() {
        // `urlparse` raises on the unbalanced bracket and `post_url` catches it.
        assert_eq!(post_url("https://[danbooru.donmai.us/posts/1"), Ok(None));
        // With no scheme, `find_parser` puts one on and `urlparse` then raises, which
        // `post_url` does not catch.
        assert_eq!(
            post_url("pixiv.net\u{ff0f}/comments/1"),
            Err(UnexpectedInput)
        );
    }

    #[test]
    fn dotted_and_dotless_capital_i_count_as_i_under_ignorecase() {
        // Python's `re.IGNORECASE` lets `i` match `ı` and `İ`; `(?i)` alone does not.
        assert!(
            post_url("https://danbooru.donmai.us/posts/v\u{131}ew/12")
                .unwrap()
                .is_some()
        );
        assert!(
            post_url("https://danbooru.donmai.us/pOSts/vIEw/12")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            post_url("https://DONMA\u{130}.us/posts/12")
                .unwrap()
                .as_deref(),
            Some("https://danbooru.donmai.us/posts/12")
        );
    }

    #[test]
    fn a_data_state_without_cbir_sites_is_not_read_at_all() {
        // Python skips it before it decodes it, so a blob that would raise is never looked at.
        let page = format!(
            "<div data-state=\"[1]\"></div><div data-state=\"5\"></div>{}",
            page(&sites())
        );
        assert_eq!(parse_sites(&page).unwrap().unwrap().len(), sites().len());
    }
}
