//! rule34.xxx (`core/boorus/rule34.py`).
//!
//! Gelbooru-family software, so the tag-list markup is read by [`gelbooru`]. Its own module, not
//! another host in Gelbooru's list, because Gelbooru's Data API now 401s without credentials and
//! remembers that for the rest of the session: sharing that flag would skip this host's
//! still-public API after the first Gelbooru 401. That latch and the per-module memo live in the
//! fetch layer; the hooks here are given this host's own `api_post` in the [`Context`] and
//! otherwise read what Gelbooru's page-only readers read.

use std::sync::LazyLock;

use regex::Regex;
use tracing::debug;

use super::host::on_host;
use super::{Context, SiteParser, gelbooru};
use crate::model::Tag;

const HOST: &str = "rule34.xxx";

/// A post on a listing page: `<a id="p10755759" ...><img src="thumb">`.
static LISTING_POST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<a id="p(\d+)"[^>]*>\s*<img src="([^"]+)""#).expect("listing post regex")
});

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(parse_file_url),
    parse_preview_url: Some(parse_preview_url),
    parse_dimensions: Some(parse_dimensions),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("rule34", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &[HOST])
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let tags = gelbooru::parse(html, url);
    if tags.is_empty() {
        debug!("No tags extracted from {url} (markup may differ from Gelbooru's)");
    }
    tags
}

/// The Statistics sidebar's "Rating:" line: identical markup to Gelbooru's, and read from the
/// page so it costs no extra request.
pub fn parse_rating(html: &str, url: &str) -> Option<String> {
    gelbooru::parse_rating(html, url)
}

pub fn parse_file_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    gelbooru::parse_file_url(html, url, ctx)
}

pub fn parse_preview_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    gelbooru::parse_preview_url(html, url, ctx)
}

pub fn parse_dimensions(html: &str, url: &str, ctx: &Context) -> (Option<i64>, Option<i64>) {
    gelbooru::parse_dimensions(html, url, ctx)
}

// -- finding a post from a search Google indexed ------------------------------------------
//
// Google Lens sometimes lists a rule34.xxx post by the SEARCH it was reached through
// ("Rule 34 / parent:10755759") with the post id nowhere. The search is still there to run and the
// post is one of its results, read off the public listing page, since the Data API now answers
// "Missing authentication". Running it (`find_post_by_search`: the listing request, the thumbnail
// downloads and the 16x16 dhash comparison) is the engine client's, not here.

/// `(post id, thumbnail URL)` for each post on a listing page.
pub fn listing_posts(html: &str) -> Vec<(String, String)> {
    LISTING_POST_RE
        .captures_iter(html)
        .map(|m| (m[1].to_string(), m[2].to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LISTING: &str = concat!(
        r#"<span id="s1" class="thumb"><a id="p10755759" href="/index.php?page=post&s=view"#,
        "&id=10755759&tags=parent%3A10755759\">\n    <img src=\"https://wimg.example/t1.jpg?1\"",
        r#" alt="x"/></a></span>"#,
        r#"<span id="s2" class="thumb"><a id="p10755716" href="/index.php?page=post&s=view"#,
        "&id=10755716&tags=parent%3A10755759\">\n    <img src=\"https://wimg.example/t2.jpg?2\"",
        r#" alt="x"/></a></span>"#
    );

    #[test]
    fn claims_rule34_xxx_only() {
        assert!(matches(
            "https://rule34.xxx/index.php?page=post&s=view&id=1"
        ));
        assert!(!matches("https://rule34.us/index.php?r=posts/view&id=1"));
        assert!(!matches("https://rule34.paheal.net/post/view/1"));
        assert!(!matches("https://r34.app/posts/rule34.xxx/anna_(tekuho)"));
    }

    #[test]
    fn listing_posts_are_read_in_page_order() {
        assert_eq!(
            listing_posts(LISTING),
            [
                (
                    "10755759".to_string(),
                    "https://wimg.example/t1.jpg?1".to_string()
                ),
                (
                    "10755716".to_string(),
                    "https://wimg.example/t2.jpg?2".to_string()
                ),
            ]
        );
        assert!(listing_posts("").is_empty());
        assert!(listing_posts(r#"<a id="pabc"><img src="x">"#).is_empty());
    }

    #[test]
    fn a_bare_list_post_is_read_through_the_shared_hooks() {
        let post = json!({"file_url": "F", "sample_url": "S", "width": 1500, "height": 2000});
        let ctx = Context::with_api_post(post.as_object().cloned());
        assert_eq!(
            parse_file_url("<html></html>", "u", &ctx).as_deref(),
            Some("F")
        );
        assert_eq!(
            parse_preview_url("<html></html>", "u", &ctx).as_deref(),
            Some("S")
        );
        assert_eq!(
            parse_dimensions("<html></html>", "u", &ctx),
            (Some(1500), Some(2000))
        );
    }

    #[test]
    fn rule34_defines_exactly_the_hooks_python_does() {
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
