//! JoyReactor / reactor.cc, via the post page's own markup (`core/boorus/joyreactor.py`).
//!
//! Read with regular expressions over the raw text, not through an HTML tree, exactly as Python
//! does: the tag run and the post's image are each anchored on one piece of markup the site
//! emits, and the page has other markup of the same shape that a tree walk would have to be
//! taught to avoid.
//!
//! A post's tags sit in one unambiguous element, `<strong class="taglist">`, as a run of
//! `<a title="Name" ... data-tag-id="N">Name</a>` entries, immediately after the poster's byline
//! and before the post body. Lower in the page, a "More on this topic" block links to OTHER posts'
//! tags, which is why tags are read from inside the taglist specifically and not by scanning the
//! page for any `data-tag-id`.
//!
//! The post's own image sits in the one `<a ... class="prettyPhotoLink">` anchor: the
//! full-quality, unwatermarked copy one path segment over from the watermarked one actually
//! embedded. A commenter's attached image is wrapped in the exact same markup inside the comment
//! thread that follows the post body, and a post whose own media is a `<video class="video_gif">`
//! has no anchor of its own, so [`parse_file_url`] stops at `id="comment_list"` (the thread's own
//! container) and a post with no anchor before that point yields `None` rather than a stranger's
//! picture.
//!
//! `reactor.cc`, `joyreactor.com` and `joyreactor.cc` are the same site behind different
//! hostnames; all three serve the same `/post/{id}` markup.

use std::sync::LazyLock;

use regex::Regex;

use super::SiteParser;
use super::host::on_host;
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOSTS: [&str; 3] = ["reactor.cc", "joyreactor.com", "joyreactor.cc"];
const COMMENT_LIST_MARKER: &str = r#"id="comment_list""#;

static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/post/(\d+)(?:[/?#]|$)").expect("post id regex"));
static TAGLIST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<strong class="taglist">(.*?)</strong>"#).expect("taglist regex")
});
// Python's `\s` on text also takes U+001C..U+001F, which Rust's does not: the class below adds them.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<a[\s\x1c-\x1f]+title="([^"]*)"[^>]*data-tag-id="\d+""#).expect("tag regex")
});
static FILE_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<a href="([^"]+)"[\s\x1c-\x1f]+class="prettyPhotoLink""#)
        .expect("file url regex")
});

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    ..SiteParser::new("joyreactor", matches, |body, url, _| parse(body, url))
};

/// On one of the three hosts AND a `/post/{id}` URL: the site's other pages (a tag's feed, a
/// user's page) carry no single post to read.
pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS) && post_id(url).is_some()
}

pub fn post_id(url: &str) -> Option<String> {
    POST_ID_RE
        .captures(pycompat::strip(url))
        .map(|m| m[1].to_string())
}

pub fn parse(body: &str, _url: &str) -> Vec<Tag> {
    let Some(taglist) = TAGLIST_RE.captures(body) else {
        return Vec::new();
    };
    TAG_RE
        .captures_iter(&taglist[1])
        .map(|m| Tag {
            name: pycompat::html_unescape(&m[1]),
            namespace: None,
            source: TagSource::Booru,
        })
        .collect()
}

pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    let post_html = body.find(COMMENT_LIST_MARKER).map_or(body, |i| &body[..i]);
    let found = FILE_URL_RE.captures(post_html)?;
    let file_url = pycompat::html_unescape(&found[1]);
    Some(match file_url.starts_with("//") {
        true => format!("https:{file_url}"),
        false => file_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://reactor.cc/post/5351071";

    #[test]
    fn only_a_post_page_on_one_of_the_three_hosts_matches() {
        for host in [
            "reactor.cc",
            "joyreactor.com",
            "joyreactor.cc",
            "m.reactor.cc",
        ] {
            assert!(matches(&format!("https://{host}/post/123")), "{host}");
        }
        assert!(matches("https://reactor.cc/post/123/"));
        assert!(matches("https://reactor.cc/post/123?x=1#c"));
        assert!(matches("  https://reactor.cc/POST/123  "));
        // A feed, a user page and a post-shaped path on another host are nobody's.
        assert!(!matches("https://reactor.cc/tag/art"));
        assert!(!matches("https://reactor.cc/post/abc"));
        assert!(!matches("https://reactor.cc/post/123abc"));
        assert!(!matches("https://example.com/post/123"));
        assert!(!matches("https://notreactor.cc/post/123"));
    }

    #[test]
    fn the_post_id_is_the_number_after_post() {
        assert_eq!(
            post_id("https://reactor.cc/post/5351071").as_deref(),
            Some("5351071")
        );
        assert_eq!(
            post_id("https://reactor.cc/post/5351071#c1").as_deref(),
            Some("5351071")
        );
        assert_eq!(post_id("https://reactor.cc/tag/post"), None);
        assert_eq!(post_id(""), None);
    }

    #[test]
    fn defines_exactly_the_hooks_python_does() {
        assert_eq!(PARSER.defined_hooks(), ["parse_file_url"]);
    }

    #[test]
    fn tags_come_from_the_taglist_only_and_are_unescaped() {
        let body = concat!(
            r#"<div><strong class="taglist"><a title="Cat &amp; Dog" href="/tag/x" data-tag-id="1">Cat</a>"#,
            "\n",
            r#"<a   title="&#1050;&#1086;&#1090;" class="c" data-tag-id="22">k</a>"#,
            r#"<a title="no id" href="/x">n</a></strong>"#,
            r#"<div>More: <a title="Other" data-tag-id="9">o</a></div>"#
        );
        let names: Vec<_> = parse(body, URL).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Cat & Dog", "\u{41a}\u{43e}\u{442}"]);
        assert!(parse(body, URL).iter().all(|t| t.namespace.is_none()));
        for none in ["", "<html>", r#"<strong class="taglist">unterminated"#] {
            assert!(parse(none, URL).is_empty(), "{none:?}");
        }
    }

    #[test]
    fn the_file_url_is_the_posts_own_anchor_not_a_comments() {
        let post = r#"<div class="image"><a href="//img.example/pics/post/full/x-1.jpeg" class="prettyPhotoLink"><img></a></div>"#;
        let comment =
            r#"<div id="comment_list"><a href="//img.example/c.png" class="prettyPhotoLink">"#;
        assert_eq!(
            parse_file_url(&format!("{post}{comment}"), URL).as_deref(),
            Some("https://img.example/pics/post/full/x-1.jpeg")
        );
        // No anchor before the thread: None, not the comment's picture.
        assert_eq!(parse_file_url(&format!("<video>{comment}"), URL), None);
        // Without a thread marker the whole page is searched.
        assert_eq!(
            parse_file_url(post, URL).as_deref().map(|u| &u[..8]),
            Some("https://")
        );
        assert_eq!(parse_file_url("", URL), None);
    }

    #[test]
    fn an_absolute_href_is_unescaped_and_passed_through() {
        let body = r#"<a href="http://a.example/p?x=1&amp;y=2"  class="prettyPhotoLink">"#;
        assert_eq!(
            parse_file_url(body, URL).as_deref(),
            Some("http://a.example/p?x=1&y=2")
        );
    }
}
