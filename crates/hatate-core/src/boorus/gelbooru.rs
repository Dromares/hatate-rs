//! Gelbooru, and the shared half of its relatives Safebooru, rule34 and Xbooru
//! (`core/boorus/gelbooru.py`).
//!
//! The four run the same Danbooru-1.x-descended software, so they share one page layout: a tag
//! sidebar of `<li>` rows whose class names the category, and a Statistics block that spells out
//! `Size: 1232x918` and `Rating: Explicit`. This module is where that layout is read; the other
//! three call into it.
//!
//! Gelbooru has an official Data API that names the exact file, the sample and the original's
//! dimensions, so each hook asks it first and falls back to the page. The API post is the
//! [`Context`]'s `api_post`: Python memoises the one most recent `(url, post)` in a module global
//! so that `parse_file_url` and `parse_preview_url`, called back to back for one post, share one
//! request. Fetching it, and Gelbooru's "401 once, never ask again this session" latch
//! (`_api_unauthorized`), belong with the HTTP client; here the post is simply given.
//!
//! The `*_from_html` functions are the page-only halves. Safebooru, rule34 and Xbooru call those
//! where the API-first hook would also be correct but would share Gelbooru's 401 latch.

use std::sync::LazyLock;

use scraper::{ElementRef, Node, Selector};
use serde_json::{Map, Value};
use tracing::{debug, info};

use super::host::on_host;
use super::parsed::{classes_of, soup_of, text_joined, text_of};
use super::sizes::{describe_size_context, parse_size_label};
use super::{Context, SiteParser, rating};
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOSTS: [&str; 1] = ["gelbooru.com"];

const CATEGORY_MAP: [(&str, &str); 5] = [
    ("tag-type-general", "general"),
    ("tag-type-artist", "artist"),
    ("tag-type-copyright", "copyright"),
    ("tag-type-character", "character"),
    ("tag-type-metadata", "meta"),
];

/// A listing page is told from a post page by two things together, measured across Gelbooru,
/// rule34.xxx, Safebooru and Xbooru: a real post carries its own image and no thumbnail grid, the
/// listing carries a grid of dozens and no post image.
const LISTING_THUMB_MARKER: &str = "thumbnail-preview";
const POST_MEDIA_MARKERS: [&str; 2] = ["id=\"image\"", "id=\"gelcomVideoPlayer\""];
const MIN_LISTING_THUMBS: usize = 2;

/// Gelbooru calls the tag list `#tag-list`; Safebooru, rule34 and Xbooru, which delegate their
/// tag parsing here, call it `#tag-sidebar`. In the order Python tries them.
static TAG_CONTAINERS: LazyLock<Vec<Selector>> = LazyLock::new(|| {
    [
        "ul#tag-list",
        "div#tag-list",
        "ul#tag-sidebar",
        "div#tag-sidebar",
    ]
    .into_iter()
    .map(|css| Selector::parse(css).expect("selector"))
    .collect()
});
static LI: LazyLock<Selector> = LazyLock::new(|| Selector::parse("li").expect("selector"));
static POST_LINK: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[href*='page=post']").expect("selector"));
static ANY_LINK: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").expect("selector"));
static IMAGE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("#image").expect("selector"));

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(parse_file_url),
    parse_preview_url: Some(parse_preview_url),
    parse_dimensions: Some(parse_dimensions),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("gelbooru", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// Whether this body is a post LIST rather than a post.
///
/// A deleted post does not 404 here: it answers 200 after redirecting to the post list, and that
/// list carries a tag sidebar of its own in the same markup, so its tags would be written into
/// the user's library as if they described the picture. Deliberately needs both halves of the
/// test: "no post image" alone would call every page whose markup shifted a listing and silently
/// drop its tags; the grid alone would trip on a post page that showed related thumbnails.
pub fn looks_like_a_listing(html: &str) -> bool {
    if POST_MEDIA_MARKERS.iter().any(|m| html.contains(m)) {
        return false;
    }
    html.matches(LISTING_THUMB_MARKER).count() >= MIN_LISTING_THUMBS
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    if looks_like_a_listing(html) {
        info!(
            "{url} answered with a post LIST rather than the post - it has been deleted, so its \
             'tags' would belong to other posts"
        );
        return Vec::new();
    }
    let soup = soup_of(html);
    let container = TAG_CONTAINERS
        .iter()
        .find_map(|selector| soup.select(selector).next());
    let Some(container) = container else {
        debug!("No tag-list/tag-sidebar container found on {url} (markup may have changed)");
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
            .select(&POST_LINK)
            .next()
            .or_else(|| li.select(&ANY_LINK).next())
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

/// The post's rating, off the Statistics sidebar's "Rating:" line.
///
/// Read from the page rather than the Data API on purpose: the page is already in hand, the API
/// needs credentials, and the sidebar spells the rating out in words, so there is no per-site
/// letter table to get wrong. A listing carries a sidebar of its own, so it is refused for the
/// same reason [`parse`] refuses it.
pub fn parse_rating(html: &str, url: &str) -> Option<String> {
    if looks_like_a_listing(html) {
        return None;
    }
    parse_rating_from_html(html, url)
}

/// The page-only half, shared with the family parsers that must not route through this module's
/// Data-API latch.
pub fn parse_rating_from_html(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);
    let rating = rating::from_label(&text_joined(soup.root_element(), " "));
    if rating.is_none() {
        debug!("No 'Rating:' line found on {url} (markup may have changed)");
    }
    rating
}

/// A truthy string field of the Data-API post, as Python's `post and post.get(key)`. Python hands
/// back whatever truthy value the record holds; the API only ever sends a string URL there, and
/// a different type has nothing downstream can use.
fn api_url_field(ctx: &Context, key: &str) -> Option<String> {
    ctx.api_post
        .as_ref()?
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The Data-API post's `width` and `height` when both are truthy, as `int()` reads them; `None`
/// when either is absent or falsy.
///
/// Gelbooru returns the two values as the record holds them, rule34 and Xbooru return
/// `int(...)` of them and fall back to the page when that raises. Only the second is typed
/// here: a record whose size is not a number, which Python's Gelbooru would pass on untouched,
/// falls back to the page. The real API sends JSON integers (Xbooru sends them as strings),
/// and no corpus item holds anything else.
fn api_dimensions(ctx: &Context) -> Option<(i64, i64)> {
    let post = ctx.api_post.as_ref()?;
    let (width, height) = (post.get("width")?, post.get("height")?);
    if !(pycompat::truthy(width) && pycompat::truthy(height)) {
        return None;
    }
    Some((pycompat::int(width)?, pycompat::int(height)?))
}

/// The exact direct file URL: the Data API's, else the page's. Prefer the API over guessing at
/// page markup, since a wrong guess once made the format and size checks HEAD the HTML post page
/// itself and report "HTML".
pub fn parse_file_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    api_url_field(ctx, "file_url").or_else(|| parse_file_url_from_html(html, url))
}

/// The `.string` of a bs4 tag: its only child's text, or, when that child is a tag, that tag's
/// `.string`, recursively. `None` when there is not exactly one child. A comment counts as a
/// string, as it does in bs4.
fn tag_string(el: ElementRef<'_>) -> Option<String> {
    let mut children = el.children();
    let only = children.next()?;
    if children.next().is_some() {
        return None;
    }
    match only.value() {
        Node::Text(text) => Some(text.to_string()),
        Node::Comment(comment) => Some(comment.to_string()),
        Node::Element(_) => tag_string(ElementRef::wrap(only)?),
        _ => None,
    }
}

/// An attribute's value when it is non-empty (`el.get(attr)` used as a condition).
fn attr(el: ElementRef<'_>, name: &str) -> Option<String> {
    el.value()
        .attr(name)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The page-only half of [`parse_file_url`]: the "Original image" link, else the on-page
/// `#image`.
pub fn parse_file_url_from_html(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);

    // `soup.find("a", string=...)` takes the FIRST link whose string matches; if that link has
    // no href the search does not go on to the next one.
    let original_link = soup
        .select(&ANY_LINK)
        .find(|a| tag_string(*a).is_some_and(|s| s.to_lowercase().contains("original image")));
    if let Some(href) = original_link.and_then(|a| attr(a, "href")) {
        return Some(href);
    }
    if let Some(src) = soup.select(&IMAGE).next().and_then(|img| attr(img, "src")) {
        return Some(src);
    }
    debug!("Could not find any direct file URL on {url} (API and HTML scrape both failed)");
    None
}

/// The Data API's `sample_url` is a mid-resolution downscaled version Gelbooru itself generates
/// for display: sharper than the search engine's tiny thumbnail, much smaller than the original.
/// Falls back to the on-page `#image` src (usually the same sample).
pub fn parse_preview_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    api_url_field(ctx, "sample_url").or_else(|| parse_preview_url_from_html(html, url))
}

pub fn parse_preview_url_from_html(html: &str, _url: &str) -> Option<String> {
    let soup = soup_of(html);
    soup.select(&IMAGE).next().and_then(|img| attr(img, "src"))
}

/// The ORIGINAL image's dimensions, not the sample's. The page displays a downscaled sample, so
/// reading the dimensions off the visible `<img>` would make a genuinely larger match look like a
/// downgrade.
pub fn parse_dimensions(html: &str, url: &str, ctx: &Context) -> (Option<i64>, Option<i64>) {
    match api_dimensions(ctx) {
        Some((width, height)) => (Some(width), Some(height)),
        None => parse_dimensions_from_html(html, url),
    }
}

/// The Statistics sidebar lists the original file's dimensions as `Size: 1152x1440`. Anchored to
/// the "Size:" label rather than hunting for any NxN, since a booru page is full of numbers (tag
/// counts, ids, scores). Size parsing is shared with the other Danbooru 1.x descendants
/// ([`super::sizes`]).
pub fn parse_dimensions_from_html(html: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let soup = soup_of(html);
    let text = text_joined(soup.root_element(), " ");
    let (width, height) = parse_size_label(&text);
    if width.is_some_and(|w| w != 0) && height.is_some_and(|h| h != 0) {
        return (width, height);
    }
    debug!(
        "No 'Size: WxH' found on {url} (markup may have changed). Context: {:?}",
        describe_size_context(&text)
    );
    (None, None)
}

/// Pulls the first post out of a Gelbooru-family Data API body.
///
/// Gelbooru wraps it as `{"post": [ {...} ]}` (or `{"post": {...}}`); rule34.xxx and Xbooru
/// return a bare list of posts. A caller that only handled the wrapped form silently got nothing
/// from those two even when the API answered 200 with a perfectly good record.
pub fn post_from_dapi(data: &Value) -> Option<&Map<String, Value>> {
    match data {
        Value::Array(posts) => posts.first()?.as_object(),
        Value::Object(map) => match map.get("post")? {
            Value::Array(posts) => posts.first()?.as_object(),
            post => post.as_object(),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "https://gelbooru.com/index.php?page=post&s=view&id=1";

    fn api(post: Value) -> Context {
        Context::with_api_post(post.as_object().cloned())
    }

    const LISTING: &str = concat!(
        "<html><body>",
        r#"<div class="thumbnail-preview"><a href="?id=1"><img></a></div>"#,
        r#"<div class="thumbnail-preview"><a href="?id=2"><img></a></div>"#,
        r#"<div class="thumbnail-preview"><a href="?id=3"><img></a></div>"#,
        r#"<ul id="tag-list">"#,
        r#"<li class="tag-type-artist"><a href="?page=post">koyama_shigeru</a></li>"#,
        "</ul></body></html>"
    );

    const POST: &str = concat!(
        r#"<html><body><img id="image" src="https://img.gelbooru.com/x.jpg">"#,
        r#"<ul id="tag-list">"#,
        r#"<li class="tag-type-character"><a href="?page=post">hakurei_reimu</a></li>"#,
        "</ul></body></html>"
    );

    #[test]
    fn claims_gelbooru_only() {
        assert!(matches(
            "https://gelbooru.com/index.php?page=post&s=view&id=1"
        ));
        assert!(matches("https://GELBOORU.com/x"));
        assert!(!matches(
            "https://safebooru.org/index.php?page=post&s=view&id=1"
        ));
        assert!(!matches("https://example.com/gelbooru.com/x"));
    }

    #[test]
    fn a_listing_is_recognised_and_a_post_is_not() {
        assert!(looks_like_a_listing(LISTING));
        assert!(!looks_like_a_listing(POST));
    }

    #[test]
    fn a_listing_yields_no_tags_and_no_rating() {
        assert!(parse(LISTING, URL).is_empty());
        let rated = LISTING.replace("</ul>", "<li>Rating: Explicit</li></ul>");
        assert_eq!(parse_rating(&rated, URL), None);
        // The page-only half does not look: it is the caller's to guard.
        assert_eq!(
            parse_rating_from_html(&rated, URL).as_deref(),
            Some("explicit")
        );
    }

    #[test]
    fn a_real_post_is_untouched() {
        let tags = parse(POST, URL);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "hakurei_reimu");
        assert_eq!(tags[0].namespace.as_deref(), Some("character"));
    }

    #[test]
    fn both_halves_of_the_listing_test_are_required() {
        // A grid AND a post image: a post, whatever else is on the page.
        let both = r#"<img id="image"><div class="thumbnail-preview"></div>"#.repeat(3);
        assert!(!looks_like_a_listing(&both));
        let video = r#"<div id="gelcomVideoPlayer"></div>"#.to_string()
            + &r#"<div class="thumbnail-preview"></div>"#.repeat(3);
        assert!(!looks_like_a_listing(&video));
        // Neither: an unrecognised page, not a listing.
        assert!(!looks_like_a_listing(
            "<html><body>nothing here</body></html>"
        ));
        // One stray thumbnail is not a grid.
        assert!(!looks_like_a_listing(
            r#"<div class="thumbnail-preview"></div>"#
        ));
        assert!(!looks_like_a_listing(""));
    }

    #[test]
    fn the_tag_container_is_found_by_either_name_and_the_wiki_link_is_not_the_tag() {
        let sidebar = concat!(
            r#"<div id="tag-sidebar"><li class="tag-type-artist tag">"#,
            r#"<a href="index.php?page=wiki&amp;s=list&amp;search=x">?</a>"#,
            r#"<a href="index.php?page=post&amp;s=list&amp;tags=x">some one</a></li></div>"#
        );
        let tags = parse(sidebar, URL);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "some_one");
        assert_eq!(tags[0].namespace.as_deref(), Some("artist"));
        // `ul#tag-list` is looked for before `ul#tag-sidebar`.
        let both = r#"<ul id="tag-sidebar"><li><a>b</a></li></ul><ul id="tag-list"><li><a>a</a></li></ul>"#;
        assert_eq!(parse(both, URL)[0].name, "a");
        // A row with no link, or an empty one, is skipped; an unknown class is "general".
        let rows =
            r#"<ul id="tag-list"><li>plain</li><li><a> </a></li><li class="x"><a>k</a></li></ul>"#;
        let tags = parse(rows, URL);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].namespace.as_deref(), Some("general"));
    }

    #[test]
    fn a_page_with_neither_container_yields_nothing() {
        assert!(
            parse(
                "<html><body><ul id=\"other\"><li><a>x</a></li></ul></body></html>",
                URL
            )
            .is_empty()
        );
    }

    #[test]
    fn the_original_image_link_beats_the_displayed_image() {
        let page = concat!(
            r#"<a href="https://i/orig.png">Original image</a>"#,
            r#"<img id="image" src="https://i/sample.jpg">"#
        );
        assert_eq!(
            parse_file_url_from_html(page, URL).as_deref(),
            Some("https://i/orig.png")
        );
        assert_eq!(
            parse_preview_url_from_html(page, URL).as_deref(),
            Some("https://i/sample.jpg")
        );
        // The label is read through nested single children, as bs4's `.string` does.
        let nested = r#"<a href="https://i/o.png"><b>ORIGINAL IMAGE</b></a>"#;
        assert_eq!(
            parse_file_url_from_html(nested, URL).as_deref(),
            Some("https://i/o.png")
        );
        // Two children: no `.string`, so not the link.
        let split =
            r#"<a href="https://i/o.png">Original <b>image</b></a><img id="image" src="s">"#;
        assert_eq!(parse_file_url_from_html(split, URL).as_deref(), Some("s"));
    }

    #[test]
    fn the_first_matching_link_is_the_only_one_tried() {
        let page = concat!(
            r#"<a>Original image</a><a href="https://i/second.png">Original image</a>"#,
            r#"<img id="image" src="https://i/sample.jpg">"#
        );
        assert_eq!(
            parse_file_url_from_html(page, URL).as_deref(),
            Some("https://i/sample.jpg")
        );
        assert_eq!(parse_file_url_from_html("<html></html>", URL), None);
        assert_eq!(parse_preview_url_from_html("<html></html>", URL), None);
    }

    #[test]
    fn the_api_post_wins_where_it_has_the_field_and_the_page_is_the_fallback() {
        let page = r#"<a href="p">Original image</a><img id="image" src="s"><li>Size: 10x20</li>"#;
        let ctx = api(json!({"file_url": "F", "sample_url": "S", "width": 3, "height": 4}));
        assert_eq!(parse_file_url(page, URL, &ctx).as_deref(), Some("F"));
        assert_eq!(parse_preview_url(page, URL, &ctx).as_deref(), Some("S"));
        assert_eq!(parse_dimensions(page, URL, &ctx), (Some(3), Some(4)));
        // A post without the field, or with an empty one, or no post at all.
        for ctx in [
            api(json!({"width": 1})),
            api(json!({"file_url": "", "sample_url": null})),
            api(json!({})),
            Context::default(),
        ] {
            assert_eq!(parse_file_url(page, URL, &ctx).as_deref(), Some("p"));
            assert_eq!(parse_preview_url(page, URL, &ctx).as_deref(), Some("s"));
            assert_eq!(parse_dimensions(page, URL, &ctx), (Some(10), Some(20)));
        }
    }

    #[test]
    fn api_dimensions_are_ints_and_junk_falls_back_to_the_page() {
        let page = "<li>Size: 1536x2048</li>";
        let strings = api(json!({"width": "3000", "height": "4000"}));
        assert_eq!(
            parse_dimensions(page, URL, &strings),
            (Some(3000), Some(4000))
        );
        for post in [
            json!({"width": "wide", "height": "tall"}),
            json!({"width": "3000"}),
            json!({"width": 0, "height": 5}),
            json!({"width": [1], "height": 5}),
        ] {
            assert_eq!(
                parse_dimensions(page, URL, &api(post.clone())),
                (Some(1536), Some(2048)),
                "{post}"
            );
        }
        assert_eq!(
            parse_dimensions("<html></html>", URL, &Context::default()),
            (None, None)
        );
    }

    #[test]
    fn the_size_label_is_the_originals_not_the_samples() {
        let page = r#"<img id="image" width="850" height="1000"><li>Size: 1232x918</li>"#;
        assert_eq!(
            parse_dimensions_from_html(page, URL),
            (Some(1232), Some(918))
        );
        // A zero is no size.
        assert_eq!(
            parse_dimensions_from_html("<li>Size: 0x5</li>", URL),
            (None, None)
        );
    }

    #[test]
    fn a_dapi_body_gives_its_first_post_wrapped_or_bare() {
        let bare = json!([{"id": 1, "file_url": "https://x/a.png"}]);
        assert_eq!(
            post_from_dapi(&bare).unwrap()["file_url"],
            "https://x/a.png"
        );
        let wrapped = json!({"post": [{"id": 2}]});
        assert_eq!(post_from_dapi(&wrapped).unwrap()["id"], 2);
        let single = json!({"post": {"id": 3}});
        assert_eq!(post_from_dapi(&single).unwrap()["id"], 3);
        for none in [
            json!([]),
            json!({}),
            json!("nope"),
            json!(null),
            json!([1]),
            json!({"post": []}),
            json!({"post": 5}),
            json!({"post": [null]}),
        ] {
            assert!(post_from_dapi(&none).is_none(), "{none}");
        }
    }

    #[test]
    fn gelbooru_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_rating"
            ]
        );
    }
}
