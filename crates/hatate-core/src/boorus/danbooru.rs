//! Danbooru, via its official JSON API rather than by scraping the page
//! (`core/boorus/danbooru.py`).
//!
//! Adding ".json" to a post URL returns the post record, which names the files explicitly:
//!
//! ```text
//! file_url          the actual full-resolution image
//! large_file_url    what the site DISPLAYS, often downscaled
//! preview_file_url  the small thumbnail
//! ```
//!
//! That distinction is the whole reason this module stopped scraping. The HTML approach read a
//! `data-file-url` attribute and, when it was not there, fell back to the on-page `<img>` src,
//! which is `large_file_url`, the downscaled sample. So "full resolution" silently became the
//! same downscaled image the preview already used. The API also gives tags already split by
//! category and the real dimensions.
//!
//! Falls back to parsing HTML if the JSON is not available, so an unrecognised URL shape or an
//! API hiccup degrades rather than breaks.
//!
//! Every hook is a pure function of the body and the post-page URL. Danbooru keeps no state, so
//! nothing here takes a [`Context`](super::Context).

use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value;
use tracing::{debug, info};

use super::host::on_host;
use super::parsed::{classes_of, json_of, soup_of, text_of};
use super::{SiteParser, rating};
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOST: &str = "danbooru.donmai.us";

/// Both URL forms. Danbooru's current one is `/posts/{id}`, but SauceNAO's index still returns
/// the legacy `/post/show/{id}` for older entries, and matching only the modern form meant those
/// never reached the JSON API: no file URL, no preview, no dimensions, and no Gold-account
/// detection, because the restriction check reads the JSON record.
static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/posts?/(?:show/)?(\d+)").expect("post id regex"));

/// The API returns tags pre-split by category, which is both more reliable and better structured
/// than reading class names off `<li>` elements. In the order Python walks them.
const TAG_FIELDS: [(&str, &str); 5] = [
    ("tag_string_artist", "artist"),
    ("tag_string_character", "character"),
    ("tag_string_copyright", "copyright"),
    ("tag_string_meta", "meta"),
    ("tag_string_general", "general"),
];

/// Danbooru's own rating vocabulary, from its API docs' "rating" field. "s" is SENSITIVE here,
/// not "safe": Danbooru split the old safe rating into general/sensitive, while e621 kept "s"
/// meaning safe. That collision is why each site keeps its own table.
const RATING_CODES: [(&str, &str); 4] = [
    ("g", "general"),
    ("s", "sensitive"),
    ("q", "questionable"),
    ("e", "explicit"),
];

/// Retained for the HTML fallback path only.
const CATEGORY_MAP: [(&str, &str); 5] = [
    ("category-0", "general"),
    ("category-1", "artist"),
    ("category-3", "copyright"),
    ("category-4", "character"),
    ("category-5", "meta"),
];

static TAG_LIST: LazyLock<(Selector, Selector)> = LazyLock::new(|| {
    (
        Selector::parse("section#tag-list").expect("selector"),
        Selector::parse("ul#tag-list").expect("selector"),
    )
});
static LI: LazyLock<Selector> = LazyLock::new(|| Selector::parse("li").expect("selector"));
static SEARCH_TAG: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a.search-tag").expect("selector"));
static TAGS_LINK: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[href*='tags']").expect("selector"));
static IMAGE_CONTAINER: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("#image-container").expect("selector"));
static IMAGE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("#image").expect("selector"));

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_restriction: Some(|body, url, _| parse_restriction(body, url)),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("danbooru", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &[HOST])
}

/// Point the fetch at the JSON representation of the post.
///
/// Danbooru asks clients to identify themselves and to avoid hammering per-ID endpoints; one
/// lookup per match the user is already waiting on is well within that.
pub fn resolve_fetch_url(url: &str) -> String {
    match POST_ID_RE.captures(url) {
        Some(m) => format!("https://{HOST}/posts/{}.json", &m[1]),
        None => {
            debug!("No post ID in {url} - falling back to fetching the page itself");
            url.to_string()
        }
    }
}

/// The post record, or `None` when the body is not one: HTML (the caller falls back to
/// scraping), a JSON value that is not an object, an error payload, or an object with no `id`.
fn load_post(body: &str, url: &str) -> Option<serde_json::Map<String, Value>> {
    let Value::Object(data) = json_of(body).ok()? else {
        return None;
    };
    if data.get("success") == Some(&Value::Bool(false)) || !data.contains_key("id") {
        let message = data.get("message").map_or("None".to_string(), py_str);
        debug!(
            "Danbooru API returned no usable post for {url}: {}",
            message.chars().take(120).collect::<String>()
        );
        return None;
    }
    Some(data)
}

/// `str(x)` for the one place Python formats a JSON value into a log line.
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A named field's value when it is a non-empty string. Python hands back whatever truthy value
/// the record holds; the API only ever sends a string URL there, and a different type has
/// nothing downstream can use.
fn url_field(post: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    post.get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn truthy_field(post: &serde_json::Map<String, Value>, key: &str) -> bool {
    post.get(key).is_some_and(pycompat::truthy)
}

pub fn parse(body: &str, url: &str) -> Vec<Tag> {
    let Some(post) = load_post(body, url) else {
        return parse_html_tags(body, url);
    };
    let mut tags = Vec::new();
    for (field, namespace) in TAG_FIELDS {
        // A tag field that is not a string is empty here; Python would raise on `.split()`.
        let names = post.get(field).and_then(Value::as_str).unwrap_or("");
        for name in pycompat::split(names) {
            tags.push(Tag {
                name: name.to_string(),
                namespace: Some(namespace.to_string()),
                source: TagSource::Booru,
            });
        }
    }
    if tags.is_empty() {
        debug!("Danbooru post {url} had no tags in any category field");
    }
    tags
}

/// The genuine full-resolution file.
///
/// Deliberately does NOT quietly substitute `large_file_url` when `file_url` is absent; that is
/// what made the old scraper misleading. Danbooru withholds `file_url` for some posts (deleted
/// or restricted ones), and then the sample genuinely is the best available, so it IS used, but
/// the substitution is logged, because "the comparison looks low-res" is otherwise impossible
/// to explain.
pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    let Some(post) = load_post(body, url) else {
        return parse_html_file_url(body, url);
    };
    if let Some(file_url) = url_field(&post, "file_url") {
        return Some(file_url);
    }
    if let Some(large) = url_field(&post, "large_file_url") {
        info!(
            "Danbooru gives no full-resolution file_url for {url} (restricted or deleted post) - \
             using the downscaled large_file_url instead, so comparisons will show the sample"
        );
        return Some(large);
    }
    debug!("Danbooru post {url} exposes no usable file URL at all");
    None
}

/// `large_file_url`: what the site itself shows. Sharper than the search engine's thumbnail, far
/// smaller than the original.
pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    let Some(post) = load_post(body, url) else {
        return parse_html_preview_url(body, url);
    };
    url_field(&post, "large_file_url").or_else(|| url_field(&post, "preview_file_url"))
}

/// The original's real dimensions: what the size comparison should judge against, not the
/// sample's. A value that is not a JSON integer is `None` (Python passes it through unchecked).
pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let Some(post) = load_post(body, url) else {
        return (None, None);
    };
    let int = |key: &str| post.get(key).and_then(Value::as_i64);
    (int("image_width"), int("image_height"))
}

/// Why this post can't be fully viewed, or `None` if it can.
///
/// Danbooru gates some posts behind a Gold account in two separate ways:
///
/// - BANNED posts, where an artist asked for their work to be taken down. Danbooru keeps the
///   post but shows it only to Gold+ accounts. Flagged explicitly as `is_banned`.
/// - CENSORED TAGS (loli/shota), which Danbooru's own page describes as requiring "a Gold
///   account to view".
///
/// Both look the same to an anonymous client: the post record comes back, tags and all, but with
/// no `file_url`. That is the general test used here; it catches whatever other restriction
/// Danbooru adds later, without needing to know its name.
///
/// Returns a short human reason so the log can say WHICH kind, since "banned" and "censored"
/// have different implications for whether getting an account would help.
pub fn parse_restriction(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    if truthy_field(&post, "is_banned") {
        return Some(
            "banned on Danbooru (artist takedown) - only Gold+ accounts can view it".to_string(),
        );
    }
    // The record exists but the file is withheld. For an anonymous request that means a
    // Gold-gated post; `large_file_url` is normally withheld along with it.
    if !truthy_field(&post, "file_url") && !truthy_field(&post, "large_file_url") {
        return Some("restricted on Danbooru - viewing it needs a Gold account".to_string());
    }
    None
}

/// The post's rating, from the API's own "rating" field.
///
/// JSON only, deliberately. The HTML fallback path is reached when the API did not answer, and
/// the rating is not something this parser can read off the page with the same confidence as the
/// named field, so that path contributes no rating rather than a guessed one.
pub fn parse_rating(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    rating::from_code(post.get("rating").unwrap_or(&Value::Null), &RATING_CODES)
}

/// The original's size in bytes, as the record states it. Not one of the registry's hooks:
/// nothing in the fetch path asks for it.
pub fn parse_file_size(body: &str, url: &str) -> Option<i64> {
    load_post(body, url)?.get("file_size")?.as_i64()
}

// ----------------------------------------------------------------------
// HTML fallback, used only when the JSON API isn't available.
// ----------------------------------------------------------------------
fn tag_list_of(soup: &Html) -> Option<scraper::ElementRef<'_>> {
    let (section, list) = &*TAG_LIST;
    soup.select(section)
        .next()
        .or_else(|| soup.select(list).next())
}

fn parse_html_tags(html: &str, url: &str) -> Vec<Tag> {
    let soup = soup_of(html);
    let Some(container) = tag_list_of(&soup) else {
        debug!("No #tag-list container found on {url} (markup may have changed)");
        return Vec::new();
    };
    let mut tags = Vec::new();
    for li in container.select(&LI) {
        let classes = classes_of(li);
        let namespace = classes
            .iter()
            .find_map(|c| CATEGORY_MAP.iter().find(|(k, _)| k == c))
            .map_or("general", |(_, ns)| ns);
        let Some(name_el) = li
            .select(&SEARCH_TAG)
            .next()
            .or_else(|| li.select(&TAGS_LINK).next())
        else {
            continue;
        };
        let name = text_of(name_el).replace(' ', "_");
        if !name.is_empty() {
            tags.push(Tag {
                name,
                namespace: Some(namespace.to_string()),
                source: TagSource::Booru,
            });
        }
    }
    tags
}

fn attr_of(soup: &Html, selector: &Selector, attr: &str) -> Option<String> {
    soup.select(selector)
        .next()?
        .value()
        .attr(attr)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn parse_html_file_url(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);
    if let Some(file_url) = attr_of(&soup, &IMAGE_CONTAINER, "data-file-url") {
        return Some(file_url);
    }
    // NOTE: no #image fallback here on purpose. That element is the downscaled sample, and
    // returning it as the "full resolution file" is exactly the bug this module was rewritten to
    // fix: it made a low-resolution comparison look like a successful one.
    debug!("No data-file-url on {url} and no JSON available - no full-resolution URL");
    None
}

fn parse_html_preview_url(html: &str, _url: &str) -> Option<String> {
    attr_of(&soup_of(html), &IMAGE, "src")
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://danbooru.donmai.us/posts/1";

    fn names(tags: &[Tag]) -> Vec<(&str, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref().unwrap(), t.name.as_str()))
            .collect()
    }

    #[test]
    fn claims_danbooru_hosts_only() {
        assert!(matches("https://danbooru.donmai.us/posts/5105386"));
        assert!(matches("http://DANBOORU.donmai.us/post/show/1"));
        assert!(!matches("https://sonohara.donmai.us/x"));
        assert!(!matches("https://e621.net/posts/219935"));
        assert!(!matches("https://example.com/danbooru.donmai.us/posts/1"));
    }

    #[test]
    fn both_url_forms_reach_the_json_api() {
        for url in [
            "https://danbooru.donmai.us/posts/123",
            "https://danbooru.donmai.us/posts/123?q=x",
            "https://danbooru.donmai.us/post/show/123",
            "https://danbooru.donmai.us/post/show/123/some_tags",
        ] {
            assert_eq!(
                resolve_fetch_url(url),
                "https://danbooru.donmai.us/posts/123.json",
                "{url}"
            );
        }
        // No ID to ask the API about: the page itself is fetched.
        assert_eq!(
            resolve_fetch_url("https://danbooru.donmai.us/artists"),
            "https://danbooru.donmai.us/artists"
        );
    }

    #[test]
    fn tags_come_out_in_category_order() {
        let body = r#"{"id": 1, "tag_string_general": "a  b", "tag_string_artist": "z",
                       "tag_string_meta": "m\u001fn"}"#;
        assert_eq!(
            names(&parse(body, URL)),
            [
                ("artist", "z"),
                ("meta", "m"),
                ("meta", "n"),
                ("general", "a"),
                ("general", "b")
            ]
        );
    }

    #[test]
    fn a_record_without_an_id_is_not_a_post() {
        // `"id" in data` is key presence, so a null id still makes a post.
        assert_eq!(
            parse_file_size(r#"{"id": null, "file_size": 7}"#, URL),
            Some(7)
        );
        assert_eq!(parse_file_size(r#"{"file_size": 7}"#, URL), None);
        // `success` has to be the boolean false.
        assert_eq!(
            parse_file_size(r#"{"id": 1, "success": 0, "file_size": 7}"#, URL),
            Some(7)
        );
        assert_eq!(
            parse_file_size(r#"{"id": 1, "success": false, "file_size": 7}"#, URL),
            None
        );
        assert_eq!(parse_file_size("[1]", URL), None);
    }

    #[test]
    fn the_sample_stands_in_for_a_withheld_original_and_says_so() {
        let body = r#"{"id": 1, "file_url": "", "large_file_url": "https://c/s.jpg"}"#;
        assert_eq!(
            parse_file_url(body, URL).as_deref(),
            Some("https://c/s.jpg")
        );
        assert_eq!(parse_restriction(body, URL), None);
        assert_eq!(parse_file_url(r#"{"id": 1}"#, URL), None);
    }

    #[test]
    fn preview_prefers_the_displayed_sample_over_the_thumbnail() {
        let both = r#"{"id": 1, "large_file_url": "L", "preview_file_url": "P"}"#;
        assert_eq!(parse_preview_url(both, URL).as_deref(), Some("L"));
        assert_eq!(
            parse_preview_url(r#"{"id": 1, "preview_file_url": "P"}"#, URL).as_deref(),
            Some("P")
        );
    }

    #[test]
    fn html_tags_take_their_namespace_from_the_first_known_class() {
        let html = r#"<ul id="tag-list">
            <li class="x category-4 category-1"><a class="search-tag">some  character</a></li>
            <li><a href="/posts?tags=plain">plain</a></li>
            <li class="category-9"><span>no link</span></li>
            <li class="category-0"><a class="search-tag">   </a></li>
        </ul>"#;
        assert_eq!(
            names(&parse(html, URL)),
            [("character", "some__character"), ("general", "plain")]
        );
    }

    #[test]
    fn a_section_tag_list_wins_over_a_ul_one() {
        let html = r#"<ul id="tag-list"><li><a class="search-tag">ul</a></li></ul>
                      <section id="tag-list"><li><a class="search-tag">section</a></li></section>"#;
        assert_eq!(names(&parse(html, URL)), [("general", "section")]);
    }

    #[test]
    fn the_html_path_never_passes_the_sample_off_as_the_original() {
        let html = r#"<section id="image-container"><img id="image" src="S"></section>"#;
        assert_eq!(parse_file_url(html, URL), None);
        assert_eq!(parse_preview_url(html, URL).as_deref(), Some("S"));
        let with_file = r#"<section id="image-container" data-file-url="O"></section>"#;
        assert_eq!(parse_file_url(with_file, URL).as_deref(), Some("O"));
        let empty = r#"<section id="image-container" data-file-url=""></section>"#;
        assert_eq!(parse_file_url(empty, URL), None);
    }

    #[test]
    fn the_html_path_states_no_rating_dimensions_or_restriction() {
        let html = "<html><body><section id='tag-list'></section>Rating: Explicit</body></html>";
        assert_eq!(parse_rating(html, URL), None);
        assert_eq!(parse_dimensions(html, URL), (None, None));
        assert_eq!(parse_restriction(html, URL), None);
        assert_eq!(parse_file_size(html, URL), None);
    }

    #[test]
    fn dimensions_that_are_not_integers_are_absent() {
        let body = r#"{"id": 1, "image_width": "10", "image_height": 20}"#;
        assert_eq!(parse_dimensions(body, URL), (None, Some(20)));
    }
}
