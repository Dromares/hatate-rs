//! xbooru.com (`core/boorus/xbooru.py`).
//!
//! Gelbooru-family software, so the tag-list markup is read by [`gelbooru`]. Its own module, not
//! another host in Gelbooru's list, because Gelbooru's Data API now 401s without credentials and
//! remembers that for the rest of the session: sharing that flag would skip this host's
//! still-public API after the first Gelbooru 401. That latch and the per-module memo live in the
//! fetch layer; the hooks here are given this host's own `api_post` in the [`Context`] and
//! otherwise read what Gelbooru's page-only readers read.

use tracing::debug;

use super::host::on_host;
use super::{Context, SiteParser, gelbooru};
use crate::model::Tag;

const HOST: &str = "xbooru.com";

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(parse_file_url),
    parse_preview_url: Some(parse_preview_url),
    parse_dimensions: Some(parse_dimensions),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("xbooru", matches, |body, url, _| parse(body, url))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "https://xbooru.com/index.php?page=post&s=view&id=987654";

    // The recorded fallback page: the "Original image" link, the displayed sample under #image,
    // the Statistics sidebar's Size label and one artist tag (the wiki "?" link is not the tag).
    const HTML_PAGE: &str = concat!(
        "<html><body>",
        r#"<a href="https://xbooru.com//images/a/b/original.png">Original image</a>"#,
        r#"<img id="image" src="https://xbooru.com//samples/a/b/sample.jpg">"#,
        "<li>Size: 1536x2048</li>",
        r#"<ul id="tag-sidebar"><li class="tag-type-artist tag">"#,
        r#"<a href="index.php?page=wiki&amp;s=list&amp;search=someone">?</a>"#,
        r#"<a href="index.php?page=post&amp;s=list&amp;tags=someone">someone</a>"#,
        "</li></ul></body></html>"
    );

    fn api(post: serde_json::Value) -> Context {
        Context::with_api_post(post.as_object().cloned())
    }

    #[test]
    fn claims_its_own_host_and_not_a_relative() {
        assert!(matches(URL));
        assert!(!matches(
            "https://rule34.xxx/index.php?page=post&s=view&id=1"
        ));
        assert!(!matches(
            "https://gelbooru.com/index.php?page=post&s=view&id=1"
        ));
    }

    #[test]
    fn the_api_answer_wins_and_the_page_is_the_fallback() {
        let post = json!({
            "file_url": "https://xbooru.com//images/a/b/api-original.png",
            "sample_url": "https://xbooru.com//samples/a/b/api-sample.jpg",
            "width": "3000",
            "height": "4000"
        });
        let ctx = api(post);
        assert_eq!(
            parse_file_url(HTML_PAGE, URL, &ctx).as_deref(),
            Some("https://xbooru.com//images/a/b/api-original.png")
        );
        assert_eq!(
            parse_preview_url(HTML_PAGE, URL, &ctx).as_deref(),
            Some("https://xbooru.com//samples/a/b/api-sample.jpg")
        );
        // The API answers sizes as strings; they come back as numbers.
        assert_eq!(
            parse_dimensions(HTML_PAGE, URL, &ctx),
            (Some(3000), Some(4000))
        );

        let none = Context::default();
        assert_eq!(
            parse_file_url(HTML_PAGE, URL, &none).as_deref(),
            Some("https://xbooru.com//images/a/b/original.png")
        );
        assert_eq!(
            parse_preview_url(HTML_PAGE, URL, &none).as_deref(),
            Some("https://xbooru.com//samples/a/b/sample.jpg")
        );
        assert_eq!(
            parse_dimensions(HTML_PAGE, URL, &none),
            (Some(1536), Some(2048))
        );
        // A post with no file_url still falls back to the page for it.
        let sized = api(json!({"width": "10", "height": "10"}));
        assert_eq!(
            parse_file_url(HTML_PAGE, URL, &sized).as_deref(),
            Some("https://xbooru.com//images/a/b/original.png")
        );
    }

    #[test]
    fn nothing_anywhere_is_none_not_an_error() {
        let none = Context::default();
        assert_eq!(parse_file_url("<html></html>", URL, &none), None);
        assert_eq!(parse_preview_url("<html></html>", URL, &none), None);
        assert_eq!(parse_dimensions("<html></html>", URL, &none), (None, None));
    }

    #[test]
    fn non_numeric_or_missing_api_dimensions_fall_back_to_the_page() {
        for post in [
            json!({"width": "wide", "height": "tall"}),
            json!({"width": "3000"}),
        ] {
            assert_eq!(
                parse_dimensions(HTML_PAGE, URL, &api(post)),
                (Some(1536), Some(2048))
            );
        }
    }

    #[test]
    fn tags_come_from_the_shared_markup_reader() {
        let tags = parse(HTML_PAGE, URL);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "someone");
        assert_eq!(tags[0].namespace.as_deref(), Some("artist"));
        assert!(parse("<html><body>nothing here</body></html>", URL).is_empty());
    }

    #[test]
    fn the_rating_comes_off_the_page() {
        let html = HTML_PAGE.replace("<li>Size:", "<li>Rating: Explicit</li><li>Size:");
        assert_eq!(parse_rating(&html, URL).as_deref(), Some("explicit"));
        assert_eq!(parse_rating(HTML_PAGE, URL), None);
    }

    #[test]
    fn xbooru_defines_exactly_the_hooks_python_does() {
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
