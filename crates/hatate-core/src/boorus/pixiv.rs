//! Pixiv, via its AJAX API (`core/boorus/pixiv.py`).
//!
//! The artwork page used to embed a `meta-preload-data` JSON blob and no longer does, so the data
//! comes from `/ajax/illust/{id}` instead: clean JSON with the tags, the full-resolution image
//! URL, the dimensions and the artist. [`resolve_fetch_url`] names that URL and fetches nothing.
//! The page-scraping path is kept as a fallback, as in Python, in case Pixiv serves the blob to
//! some clients again.
//!
//! The legacy `member_illust.php?mode=medium&illust_id=X` form (still common in older search
//! engine indexes) serves no blob either, but its `illust_id` names the same artwork, so both
//! URL forms are fetched through the modern API.
//!
//! One artwork can carry dozens of images under one URL, and `/ajax/illust/{id}` describes only
//! the FIRST: the file URL, preview and dimensions belong to page 1 whatever page matched.
//! [`parse_page_count`] says whether that matters; [`pages_api_url`] and [`parse_pages`] read the
//! per-page listing.
//!
//! Where Python would raise on a shape Pixiv never sends (a `tags` that is a list, a `userName`
//! that is a number), the hook returns what `fetch_page_info` would make of it: nothing.

use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{Map, Value};
use tracing::{debug, warn};

use super::host::on_host;
use super::json_fields::int_of;
use super::parsed::{json_of, soup_of};
use super::{Context, Page, SiteParser};
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOST: &str = "pixiv.net";

/// Pixiv's generic error text, which it sends specifically when the session cookie is missing or
/// expired.
const COOKIE_ERROR: &str = "An unknown error occurred";

// `\d` is Unicode in both Python's `re` and here, so the two read the same digits.
static ILLUST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/artworks/(\d+)").expect("illust id regex"));
static LEGACY_ILLUST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[?&]illust_id=(\d+)").expect("legacy illust id regex"));

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _: &Context| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_page_count: Some(|body, url, _| parse_page_count(body, url)),
    parse_pages: Some(|body, url, _| parse_pages(body, url)),
    pages_api_url: Some(pages_api_url),
    ..SiteParser::new("pixiv", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &[HOST])
}

/// The artwork's numeric ID, from either URL form.
pub fn illust_id(url: &str) -> Option<String> {
    ILLUST_ID_RE
        .captures(url)
        .or_else(|| LEGACY_ILLUST_ID_RE.captures(url))
        .map(|m| m[1].to_string())
}

/// Pixiv's AJAX API rather than the artwork page. A URL with no ID in it is fetched as it is.
pub fn resolve_fetch_url(url: &str) -> String {
    match illust_id(url) {
        Some(id) => format!("https://www.pixiv.net/ajax/illust/{id}"),
        None => {
            debug!("No illust ID found in {url}, fetching as-is");
            url.to_string()
        }
    }
}

/// The illust object, from either the AJAX API response or the older embedded page blob,
/// whichever this response actually is.
fn illust_data(body: &str, url: &str) -> Option<Map<String, Value>> {
    // AJAX API: {"error": false, "body": {...}}
    if let Ok(Value::Object(data)) = json_of(body) {
        if data.get("error").is_some_and(pycompat::truthy) {
            let message = data.get("message").and_then(Value::as_str).unwrap_or("");
            if message == COOKIE_ERROR {
                warn!(
                    "Pixiv rejected the request for {url} - the PHPSESSID cookie is missing or \
                     expired (Settings > Site Logins)"
                );
            } else {
                warn!(
                    "Pixiv API error for {url}: {}",
                    if message.is_empty() {
                        "unspecified"
                    } else {
                        message
                    }
                );
            }
            return None;
        }
        if let Some(Value::Object(body_obj)) = data.get("body")
            && ["urls", "tags"]
                .iter()
                .any(|key| body_obj.get(*key).is_some_and(pycompat::truthy))
        {
            return Some(body_obj.clone());
        }
    }

    // Fallback: the legacy embedded page blob. Its `illust` is a dict keyed by artwork ID and
    // the first value is the artwork.
    let legacy = extract_preload_data(body, url)?;
    match legacy.get("illust") {
        Some(Value::Object(illusts)) => match illusts.values().next() {
            Some(Value::Object(illust)) => Some(illust.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The `<meta id=...>` element with this id, the first in document order.
fn find_meta<'a>(soup: &'a Html, id: &str) -> Option<scraper::ElementRef<'a>> {
    let selector = Selector::parse(&format!(r#"meta[id="{id}"]"#)).expect("meta selector");
    soup.select(&selector).next()
}

/// Pixiv's embedded JSON blob, from the page's `meta-preload-data` or `meta-global-data`
/// element: whichever parses to an object with an `illust` in it.
fn extract_preload_data(html: &str, url: &str) -> Option<Map<String, Value>> {
    let soup = soup_of(html);

    for tag_id in ["meta-preload-data", "meta-global-data"] {
        let Some(tag) = find_meta(&soup, tag_id) else {
            continue;
        };
        let content = tag.value().attr("content").unwrap_or("");
        if content.is_empty() {
            debug!("<meta id={tag_id}> on {url} had no content attribute");
            continue;
        }
        match serde_json::from_str::<Value>(content) {
            Err(_) => warn!("<meta id={tag_id}> on {url} did not contain valid JSON"),
            Ok(Value::Object(data)) if data.get("illust").is_some_and(pycompat::truthy) => {
                return Some(data);
            }
            Ok(_) => debug!("<meta id={tag_id}> on {url} parsed but had no illust data"),
        }
    }

    // Nothing usable: say why, so the log helps next time and does not just say "not found".
    if find_meta(&soup, "meta-preload-data").is_none() {
        let lowered = html.to_lowercase();
        if lowered.contains("login") && (lowered.contains("signup") || lowered.contains("sign up"))
        {
            warn!(
                "No preload data on {url} and the page looks like a login wall - this artwork \
                 may be restricted to logged-in users (Pixiv added that per-work setting in 2025)"
            );
        } else {
            warn!(
                "No <meta id=meta-preload-data> element on {url} at all (page was {} bytes) - \
                 Pixiv may have changed its page structure",
                html.chars().count()
            );
        }
    }
    None
}

/// Tags plus the artist. The artist lives outside the tag list, so it is pulled separately or it
/// is lost entirely.
pub fn parse(body: &str, url: &str) -> Vec<Tag> {
    let Some(illust) = illust_data(body, url) else {
        return Vec::new();
    };
    let Some(tags) = tags_of(&illust) else {
        return Vec::new();
    };
    if tags.is_empty() {
        debug!("Pixiv data found for {url} but it contained no tags");
    }
    tags
}

/// The tags of an illust, or `None` where Python raises.
fn tags_of(illust: &Map<String, Value>) -> Option<Vec<Tag>> {
    let listed = match illust.get("tags").filter(|v| pycompat::truthy(v)) {
        None => None,
        Some(Value::Object(outer)) => outer.get("tags").filter(|v| pycompat::truthy(v)),
        Some(_) => return None,
    };
    let entries: &[Value] = match listed {
        None => &[],
        Some(Value::Array(entries)) => entries,
        Some(_) => return None,
    };

    let mut tags = Vec::new();
    for entry in entries {
        let Value::Object(entry) = entry else {
            return None;
        };
        if let Some(name) = entry.get("tag").filter(|v| pycompat::truthy(v)) {
            tags.push(Tag {
                name: name.as_str()?.replace(' ', "_"),
                source: TagSource::Booru,
                namespace: None,
            });
        }
    }
    if let Some(artist) = illust.get("userName").filter(|v| pycompat::truthy(v)) {
        tags.push(Tag {
            name: artist.as_str()?.replace(' ', "_"),
            source: TagSource::Booru,
            namespace: Some("creator".to_string()),
        });
    }
    Some(tags)
}

/// `(illust.get("urls") or {}).get(key)`. A value that is not a string, and a `urls` that is not
/// an object (Python raises), read as absent.
fn url_of(body: &str, url: &str, key: &str) -> Option<String> {
    let illust = illust_data(body, url)?;
    match illust.get("urls").filter(|v| pycompat::truthy(v))? {
        Value::Object(urls) => urls.get(key)?.as_str().map(str::to_string),
        _ => None,
    }
}

/// Full-resolution original: what download-and-send grabs.
pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    url_of(body, url, "original")
}

/// Pixiv's own display-size copy, for the preview thumbnail. `regular`, or `small` when there is
/// none (an empty string counts as none).
pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    match url_of(body, url, "regular") {
        Some(regular) if !regular.is_empty() => Some(regular),
        _ => url_of(body, url, "small"),
    }
}

/// How many images this artwork holds: 1 for an ordinary post, and 1 where the count is missing,
/// not a number, or not above one.
pub fn parse_page_count(body: &str, url: &str) -> i64 {
    let Some(illust) = illust_data(body, url) else {
        return 1;
    };
    illust
        .get("pageCount")
        .filter(|v| pycompat::truthy(v))
        .and_then(int_of)
        .map_or(1, |count| count.max(1))
}

/// Pixiv's per-page listing for an artwork, which `/ajax/illust/{id}` does not include.
pub fn pages_api_url(url: &str) -> Option<String> {
    illust_id(url).map(|id| format!("https://www.pixiv.net/ajax/illust/{id}/pages"))
}

/// The page list from `/ajax/illust/{id}/pages`: one entry per image, each with its own `urls`
/// (original/regular/small). An entry that is not an object or has no `urls` is left out.
pub fn parse_pages(body: &str, _url: &str) -> Vec<Page> {
    let Ok(Value::Object(data)) = json_of(body) else {
        return Vec::new();
    };
    if data.get("error").is_some_and(pycompat::truthy) {
        return Vec::new();
    }
    let Some(Value::Array(pages)) = data.get("body") else {
        return Vec::new();
    };
    pages
        .iter()
        .filter_map(|page| match page {
            Value::Object(page) if page.get("urls").is_some_and(pycompat::truthy) => {
                Some(page.clone())
            }
            _ => None,
        })
        .collect()
}

/// An integer `width`/`height`. Python hands back whatever JSON held; a value that is not a whole
/// number reads as unknown here.
fn whole_number(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_number()?;
    number.as_i64().or_else(|| {
        let float = number.as_f64()?;
        (float.fract() == 0.0 && float.abs() < 9.0e18).then_some(float as i64)
    })
}

pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    match illust_data(body, url) {
        Some(illust) => (
            whole_number(illust.get("width")),
            whole_number(illust.get("height")),
        ),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ajax() -> String {
        json!({"error": false, "message": "", "body": {
            "illustId": "75494369", "userName": "Some Artist",
            "width": 2894, "height": 4093,
            "tags": {"tags": [{"tag": "original"}]},
            "urls": {"original": "https://i.pximg.net/img-original/o.png",
                     "regular": "https://i.pximg.net/img-master/r.jpg"},
        }})
        .to_string()
    }

    #[test]
    fn claims_pixiv_only() {
        assert!(matches("https://www.pixiv.net/artworks/1"));
        assert!(!matches("https://i.pximg.net/x"));
        assert!(!matches("https://example.com/pixiv.net/artworks/1"));
    }

    // test_all_url_forms_resolve_to_the_ajax_api
    #[test]
    fn all_url_forms_resolve_to_the_ajax_api() {
        for src in [
            "https://www.pixiv.net/artworks/75494369",
            "https://www.pixiv.net/en/artworks/75494369",
            "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=75494369",
        ] {
            assert_eq!(
                resolve_fetch_url(src),
                "https://www.pixiv.net/ajax/illust/75494369",
                "{src}"
            );
        }
        assert_eq!(
            resolve_fetch_url("https://www.pixiv.net/users/1"),
            "https://www.pixiv.net/users/1"
        );
    }

    // test_extracts_full_resolution_url, test_preview_differs_from_original
    #[test]
    fn the_original_and_the_regular_render_differ() {
        assert_eq!(
            parse_file_url(&ajax(), "u").as_deref(),
            Some("https://i.pximg.net/img-original/o.png")
        );
        assert_eq!(
            parse_preview_url(&ajax(), "u").as_deref(),
            Some("https://i.pximg.net/img-master/r.jpg")
        );
    }

    #[test]
    fn the_preview_falls_back_to_small_when_regular_is_missing_or_empty() {
        for regular in [json!(null), json!("")] {
            let body = json!({"body": {"urls": {"regular": regular, "small": "S"}}}).to_string();
            assert_eq!(parse_preview_url(&body, "u").as_deref(), Some("S"));
        }
        // An empty original is returned as it is; only the preview skips an empty string.
        let body = json!({"body": {"urls": {"original": ""}}}).to_string();
        assert_eq!(parse_file_url(&body, "u").as_deref(), Some(""));
    }

    // test_extracts_tags_artist_and_dimensions
    #[test]
    fn tags_artist_and_dimensions() {
        let tags: Vec<_> = parse(&ajax(), "u")
            .into_iter()
            .map(|t| (t.namespace, t.name))
            .collect();
        assert_eq!(
            tags,
            [
                (None, "original".to_string()),
                (Some("creator".to_string()), "Some_Artist".to_string())
            ]
        );
        assert_eq!(parse_dimensions(&ajax(), "u"), (Some(2894), Some(4093)));
    }

    // test_api_error_yields_nothing_without_raising
    #[test]
    fn an_api_error_yields_nothing() {
        for message in [COOKIE_ERROR, "boom", ""] {
            let err = json!({"error": true, "message": message, "body": []}).to_string();
            assert!(parse(&err, "u").is_empty());
            assert_eq!(parse_file_url(&err, "u"), None);
            assert_eq!(parse_page_count(&err, "u"), 1);
            assert_eq!(parse_dimensions(&err, "u"), (None, None));
        }
    }

    // test_falls_back_to_legacy_embedded_blob
    #[test]
    fn falls_back_to_the_legacy_embedded_blob() {
        let legacy = json!({"illust": {"1": {
            "tags": {"tags": [{"tag": "original"}]}, "userName": "An Artist",
            "width": 100, "height": 200,
            "urls": {"original": "https://i.pximg.net/legacy.png"}}}});
        let page = format!(
            r#"<html><head><meta id="meta-preload-data" content='{legacy}'></head></html>"#
        );
        assert_eq!(
            parse_file_url(&page, "u").as_deref(),
            Some("https://i.pximg.net/legacy.png")
        );
        assert_eq!(parse(&page, "u").len(), 2);
        assert_eq!(parse_dimensions(&page, "u"), (Some(100), Some(200)));
    }

    #[test]
    fn the_global_data_element_is_read_when_the_preload_one_has_no_illust() {
        let page = concat!(
            r#"<meta id="meta-preload-data" content='{"x": 1}'>"#,
            r#"<meta id="meta-global-data" content='{"illust": {"9": {"urls": {"original": "G"}}}}'>"#
        );
        assert_eq!(parse_file_url(page, "u").as_deref(), Some("G"));
        // Empty, broken and non-object blobs are skipped, not fatal.
        for content in ["", "{not json", "[1]", r#"{"illust": {}}"#] {
            let page = format!(r#"<meta id="meta-preload-data" content='{content}'>"#);
            assert_eq!(parse_file_url(&page, "u"), None, "{content:?}");
        }
    }

    // test_garbage_input_is_safe
    #[test]
    fn garbage_input_is_safe() {
        assert!(parse("<html>nope</html>", "u").is_empty());
        assert!(parse("", "u").is_empty());
        assert!(parse("null", "u").is_empty());
    }

    #[test]
    fn a_shape_pixiv_never_sends_reads_as_nothing() {
        for body in [
            json!({"body": {"urls": {"original": "x"}, "tags": ["a"]}}),
            json!({"body": {"urls": {"original": "x"}, "tags": {"tags": {"a": 1}}}}),
            json!({"body": {"urls": {"original": "x"}, "tags": {"tags": [1]}}}),
            json!({"body": {"urls": {"original": "x"}, "tags": {"tags": [{"tag": 5}]}}}),
            json!({"body": {"urls": {"original": "x"}, "userName": 5}}),
        ] {
            assert!(parse(&body.to_string(), "u").is_empty(), "{body}");
        }
        let body = json!({"body": {"urls": "x"}}).to_string();
        assert_eq!(parse_file_url(&body, "u"), None);
        let body = json!({"body": {"urls": {"original": 5}}}).to_string();
        assert_eq!(parse_file_url(&body, "u"), None);
        // A tag with no name is skipped, not an error.
        let body = json!({"body": {"tags": {"tags": [{"tag": ""}, {}, {"tag": "a b"}]}}});
        let names: Vec<_> = parse(&body.to_string(), "u")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["a_b"]);
    }

    // test_page_count, test_missing_page_count_means_one
    #[test]
    fn page_count_is_at_least_one() {
        let count = |v: Value| {
            let body = json!({"error": false, "body": {"pageCount": v, "urls": {"original": "x"}}});
            parse_page_count(&body.to_string(), "u")
        };
        assert_eq!(count(json!(1)), 1);
        assert_eq!(count(json!(7)), 7);
        assert_eq!(count(json!("3")), 3);
        for odd in [json!(null), json!(0), json!(-4), json!("x"), json!([2])] {
            assert_eq!(count(odd.clone()), 1, "{odd}");
        }
        let body = json!({"error": false, "body": {"urls": {"original": "x"}}});
        assert_eq!(parse_page_count(&body.to_string(), "u"), 1);
    }

    // test_pages_api_url_for_both_url_forms
    #[test]
    fn pages_api_url_for_both_url_forms() {
        assert_eq!(
            pages_api_url("https://www.pixiv.net/artworks/12345678").as_deref(),
            Some("https://www.pixiv.net/ajax/illust/12345678/pages")
        );
        assert_eq!(
            pages_api_url("https://www.pixiv.net/member_illust.php?mode=medium&illust_id=999")
                .as_deref(),
            Some("https://www.pixiv.net/ajax/illust/999/pages")
        );
        assert_eq!(pages_api_url("https://www.pixiv.net/users/1"), None);
    }

    // test_parses_the_page_list, test_error_response_yields_no_pages
    #[test]
    fn the_page_list_keeps_entries_with_urls() {
        let body = json!({"error": false, "body": [
            {"urls": {"small": "s0", "original": "o0"}},
            {"urls": {}}, {"width": 1}, 5,
            {"urls": {"small": "s1", "original": "o1"}}]});
        let pages = parse_pages(&body.to_string(), "u");
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1]["urls"]["original"], "o1");
        for body in [
            json!({"error": true}),
            json!({"body": {"urls": {"a": 1}}}),
            json!([1]),
        ] {
            assert!(parse_pages(&body.to_string(), "u").is_empty());
        }
        assert!(parse_pages("not json", "u").is_empty());
    }

    #[test]
    fn dimensions_are_whole_numbers_or_unknown() {
        let dims = |w: Value, h: Value| {
            parse_dimensions(
                &json!({"body": {"urls": {"original": "x"}, "width": w, "height": h}}).to_string(),
                "u",
            )
        };
        assert_eq!(dims(json!(3), json!(4.0)), (Some(3), Some(4)));
        assert_eq!(dims(json!(null), json!("4")), (None, None));
        assert_eq!(dims(json!(3.5), json!(4)), (None, Some(4)));
    }

    #[test]
    fn pixiv_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_page_count",
                "parse_pages",
                "pages_api_url"
            ]
        );
    }
}
