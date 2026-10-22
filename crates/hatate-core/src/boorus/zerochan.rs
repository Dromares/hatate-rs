//! Zerochan (`core/boorus/zerochan.py`).
//!
//! Uses Zerochan's documented read-only JSON API rather than scraping HTML: appending `?json` to
//! a post URL returns structured data including the direct full-resolution image URL, so the
//! original file comes back and not a re-encoded preview, which is the whole point of
//! downloading from the source instead of the search engine's copy.
//!
//! Every hook is a pure function of the body and the post-page URL; Zerochan keeps no state.
//!
//! Where Python hands a field through unchecked, this reads it as the type the API sends: a
//! `full` or a preview that is not a string, and a `width` or `height` that is not a JSON
//! integer, are left out. (Python would return the number or list itself.) A `tags` that is a
//! string or an object is walked the way Python walks it, by character and by key; one that is a
//! number makes Python raise, which `fetch_page_info` turns into no tags, and here too.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};
use tracing::{debug, warn};

use super::SiteParser;
use super::host::on_host;
use super::parsed::json_of;
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOSTS: [&str; 1] = ["zerochan.net"];

/// Post URLs look like `https://www.zerochan.net/1234567`. (`\d` is Unicode in both.)
static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"zerochan\.net/(\d+)").expect("post id regex"));

/// Zerochan's own tag categories -> the namespaces the rest of the app (and Hydrus's PTR
/// convention) uses.
const NAMESPACE_MAP: [(&str, &str); 7] = [
    ("mangaka", "creator"),
    ("artist", "creator"),
    ("character", "character"),
    ("series", "series"),
    ("game", "series"),
    ("studio", "studio"),
    ("source", "series"),
];

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    ..SiteParser::new("zerochan", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// Redirect the fetch to Zerochan's JSON API for this post: it returns the full-resolution image
/// URL and structured tags directly, which is both more reliable and higher quality than parsing
/// the HTML page.
pub fn resolve_fetch_url(url: &str) -> String {
    match POST_ID_RE.captures(url) {
        Some(found) => format!("https://www.zerochan.net/{}?json", &found[1]),
        None => {
            debug!("No post ID found in {url}, fetching as-is");
            url.to_string()
        }
    }
}

/// The response as an object, or `None` for anything else. An empty object is returned as it is:
/// the hooks that read `if not data` check for it themselves.
fn load(body: &str, url: &str) -> Option<Map<String, Value>> {
    match json_of(body) {
        Ok(Value::Object(data)) => Some(data),
        Ok(_) => None,
        Err(_) => {
            warn!("Zerochan response for {url} was not valid JSON (API may have changed)");
            None
        }
    }
}

/// `for raw in data.get("tags") or []`, as the strings that loop would see. Items of a list that
/// are not strings are skipped by the loop's own `isinstance`, so they are dropped here.
fn raw_tags(data: &Map<String, Value>) -> Vec<String> {
    match data.get("tags") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) => s.chars().map(String::from).collect(),
        Some(Value::Object(o)) => o.keys().cloned().collect(),
        // A truthy number or `true` is not iterable: Python raises, and the hook has no tags.
        _ => Vec::new(),
    }
}

/// `NAMESPACE_MAP.get(raw.lower(), raw.lower() or None)`.
fn normalize_namespace(raw: &str) -> Option<String> {
    let lowered = raw.to_lowercase();
    match NAMESPACE_MAP.iter().find(|(from, _)| *from == lowered) {
        Some((_, namespace)) => Some((*namespace).to_string()),
        None => (!lowered.is_empty()).then_some(lowered),
    }
}

pub fn parse(body: &str, url: &str) -> Vec<Tag> {
    let Some(data) = load(body, url).filter(|data| !data.is_empty()) else {
        return Vec::new();
    };

    let mut tags = Vec::new();
    for raw in raw_tags(&data) {
        // Zerochan tags arrive as "Character: Name" / "Series: Title" / "Mangaka: Artist": split
        // that into the namespace:name form, so they line up with how every other site's tags
        // are stored.
        let (namespace, name) = match raw.split_once(':') {
            Some((namespace_raw, name)) => (
                normalize_namespace(pycompat::strip(namespace_raw)),
                pycompat::strip(name),
            ),
            None => (None, pycompat::strip(&raw)),
        };
        if !name.is_empty() {
            tags.push(Tag {
                name: name.replace(' ', "_"),
                namespace,
                source: TagSource::Booru,
            });
        }
    }

    if tags.is_empty() {
        debug!("No tags in Zerochan response for {url}");
    }
    tags
}

/// The API's "full" field is the direct full-resolution original.
pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    let data = load(body, url)?;
    data.get("full")?.as_str().map(String::from)
}

/// The best preview short of the full original.
///
/// Zerochan offers four sizes, and they are not what their names suggest (measured against a
/// live post, 2976x4055): small 102x139 (5 KB), medium 240x327 (11 KB), large 600x818 (76 KB),
/// full 2976x4055 (5573 KB). "medium" is a 240px thumbnail, barely better than the search
/// engine's own and visibly blurry; "large" is the one that reads as a preview, at 76 KB.
/// "full" is deliberately not used here; that is what [`parse_file_url`] is for.
pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    let data = load(body, url)?;
    ["large", "medium", "small"].iter().find_map(|key| {
        data.get(*key)
            .and_then(Value::as_str)
            .filter(|c| c.starts_with("http://") || c.starts_with("https://"))
            .map(String::from)
    })
}

pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let Some(data) = load(body, url) else {
        return (None, None);
    };
    let int = |key: &str| data.get(key).and_then(Value::as_i64);
    (int("width"), int("height"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "u";

    fn body(value: Value) -> String {
        value.to_string()
    }

    fn named(tags: &[Tag]) -> Vec<(Option<&str>, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref(), t.name.as_str()))
            .collect()
    }

    #[test]
    fn resolve_fetch_url_asks_for_the_json_api() {
        assert_eq!(
            resolve_fetch_url("https://www.zerochan.net/3793685"),
            "https://www.zerochan.net/3793685?json"
        );
        assert_eq!(
            resolve_fetch_url("https://www.zerochan.net/Some+Tag"),
            "https://www.zerochan.net/Some+Tag"
        );
    }

    #[test]
    fn namespaces_are_mapped_and_unknown_ones_lowercased() {
        let b = body(
            json!({"tags": ["Mangaka: An Artist", "Character: A Char", "Long Hair",
                                     "Studio: Ghibli", "Pose: Sitting", ": bare", "Game: A Game"]}),
        );
        assert_eq!(
            named(&parse(&b, URL)),
            [
                (Some("creator"), "An_Artist"),
                (Some("character"), "A_Char"),
                (None, "Long_Hair"),
                (Some("studio"), "Ghibli"),
                (Some("pose"), "Sitting"),
                (None, "bare"),
                (Some("series"), "A_Game"),
            ]
        );
    }

    #[test]
    fn a_tag_with_no_name_or_no_string_is_skipped() {
        let b = body(json!({"tags": ["Character:", "  ", 5, null, ["x"], "ok"]}));
        assert_eq!(named(&parse(&b, URL)), [(None, "ok")]);
    }

    #[test]
    fn a_tags_field_of_another_shape_is_walked_like_python_walks_it() {
        assert_eq!(
            named(&parse(&body(json!({"tags": "ab"})), URL)),
            [(None, "a"), (None, "b")]
        );
        assert_eq!(
            named(&parse(&body(json!({"tags": {"x:y": 1}})), URL)),
            [(Some("x"), "y")]
        );
        assert!(parse(&body(json!({"tags": 7})), URL).is_empty());
        assert!(parse(&body(json!({"tags": true})), URL).is_empty());
        assert!(parse(&body(json!({"tags": 0})), URL).is_empty());
    }

    #[test]
    fn anything_but_a_non_empty_object_is_no_tags() {
        for b in ["", "not json", "[]", "null", "{}", "\"x\"", "3"] {
            assert!(parse(b, URL).is_empty(), "{b}");
            assert_eq!(parse_file_url(b, URL), None, "{b}");
            assert_eq!(parse_preview_url(b, URL), None, "{b}");
            assert_eq!(parse_dimensions(b, URL), (None, None), "{b}");
        }
    }

    #[test]
    fn preview_prefers_large_then_medium_then_small_and_never_the_full_original() {
        let all = |extra: Value| {
            let mut v = json!({"small": "https://z/s.jpg", "medium": "https://z/m.avif",
                               "large": "https://z/l.jpg", "full": "https://z/f.jpg",
                               "width": 2976, "height": 4055, "tags": []});
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            body(v)
        };
        assert_eq!(
            parse_preview_url(&all(json!({})), URL).as_deref(),
            Some("https://z/l.jpg")
        );
        assert_eq!(
            parse_preview_url(&all(json!({"large": null})), URL).as_deref(),
            Some("https://z/m.avif")
        );
        assert_eq!(
            parse_preview_url(&all(json!({"large": null, "medium": null})), URL).as_deref(),
            Some("https://z/s.jpg")
        );
        let none = all(json!({"large": null, "medium": null, "small": null}));
        assert_eq!(parse_preview_url(&none, URL), None);
        assert_eq!(
            parse_file_url(&none, URL).as_deref(),
            Some("https://z/f.jpg")
        );
        // A candidate that is not an http(s) URL is passed over for the next size.
        assert_eq!(
            parse_preview_url(&all(json!({"large": "ftp://z/l", "medium": "//z/m"})), URL)
                .as_deref(),
            Some("https://z/s.jpg")
        );
    }

    #[test]
    fn dimensions_are_the_json_integers_or_nothing() {
        assert_eq!(
            parse_dimensions(&body(json!({"width": 1200, "height": 1600})), URL),
            (Some(1200), Some(1600))
        );
        assert_eq!(
            parse_dimensions(&body(json!({"width": 1200})), URL),
            (Some(1200), None)
        );
        assert_eq!(
            parse_dimensions(&body(json!({"width": "1200", "height": 1.5})), URL),
            (None, None)
        );
    }

    #[test]
    fn only_zerochan_is_claimed() {
        assert!(matches("https://www.zerochan.net/123456"));
        assert!(matches("https://zerochan.net/1"));
        assert!(!matches("https://notzerochan.net/1"));
        assert!(!matches("https://e621.net/posts/219935"));
    }
}
