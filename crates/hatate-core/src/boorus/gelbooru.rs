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
