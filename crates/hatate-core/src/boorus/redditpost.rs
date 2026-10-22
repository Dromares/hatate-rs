//! Reddit posts, via each post's public RSS feed (`core/boorus/redditpost.py`).
//!
//! Reddit's JSON API answers 403 to an anonymous client, and old.reddit's HTML does not carry the
//! image. A post's own feed, `reddit.com/comments/{id}/.rss`, answers anonymously with everything
//! a match needs: the post's first entry names the ORIGINAL (`i.redd.it`) as its `[link]`, a
//! `preview.redd.it` thumbnail, the subreddit, the title and the poster. A post that no longer
//! exists answers 404, which the fetch already reads as gone.
//!
//! What a post gives, and what it does not:
//!
//! - The picture: the original from `i.redd.it` (an imgur direct link is taken too). A GALLERY
//!   links to `reddit.com/gallery/{id}`, not a file; its feed shows only the first image, whose
//!   `i.redd.it` original has the same id as the thumbnail, so a gallery match is its first image.
//! - Dimensions: none in the feed. They are read from the first bytes of the original, which
//!   `i.redd.it` serves by Range (`_remote_size.py`, Phase 3). [`parse_dimensions`] is told how to
//!   do that read by its caller and opens no socket itself.
//! - Tags: `subreddit:` and `title:`. The poster is NOT tagged as the creator: on Reddit it is as
//!   often a reposter as the artist.
//!
//! Read with regular expressions over the raw feed text, as Python does.

use std::sync::LazyLock;

use regex::Regex;

use super::host::on_host;
use super::{Context, SiteParser};
use crate::model::{Tag, TagSource};
use crate::pycompat;
use crate::reddit::USER_AGENT;

const HOSTS: [&str; 2] = ["reddit.com", "redd.it"];

// Python's `$` also matches before a final newline; `\n?\z` is the same thing. Python's
// IGNORECASE also lets `[a-z]` match U+0130 and U+0131, which `(?i)` here does not.
/// A post: `/r/{sub}/comments/{id}/...`, `/comments/{id}`, or the `redd.it/{id}` short link. Not a
/// subreddit listing, a user page or a gallery page.
static POST_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:/comments/|^https?://(?:www\.)?redd\.it/)([a-z0-9]{3,12})(?:[/?#]|\n?\z)")
        .expect("post id regex")
});
static ENTRY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<entry>(.*?)</entry>").expect("entry regex"));
static CATEGORY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<category term="([^"]+)""#).expect("category regex"));
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<title>(.*?)</title>").expect("title regex"));
static CONTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<content type="html">(.*?)</content>"#).expect("content regex")
});
static THUMB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<media:thumbnail url="([^"]+)""#).expect("thumbnail regex"));
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<a href="([^"]+)">\[link\]</a>"#).expect("link regex"));
static DIRECT_IMAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^https?://(?:i\.redd\.it|i\.imgur\.com)/[A-Za-z0-9_-]+\.(?:jpe?g|png|gif|webp)\n?\z",
    )
    .expect("direct image regex")
});
static PREVIEW_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://preview\.redd\.it/([A-Za-z0-9]+)\.([A-Za-z0-9]+)")
        .expect("preview id regex")
});

/// What the registry knows about this site: one entry per hook the module defines.
///
/// `parse_dimensions` is not in it: Python defines it, but it reads the remote file, so its
/// caller has to supply that read (Phase 3).
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _: &Context| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    incomplete_reason: Some(|body, url, _| incomplete_reason(body, url)),
    ..SiteParser::new("redditpost", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS) && post_id(url).is_some()
}

/// The post's id, lower-cased.
pub fn post_id(url: &str) -> Option<String> {
    POST_ID_RE
        .captures(pycompat::strip(url))
        .map(|m| m[1].to_lowercase())
}

/// The post's own feed.
pub fn resolve_fetch_url(url: &str) -> String {
    match post_id(url) {
        Some(id) => format!("https://www.reddit.com/comments/{id}/.rss"),
        None => url.to_string(),
    }
}

/// `subreddit:` and `title:`; the poster is not a tag.
pub fn parse(body: &str, _url: &str) -> Vec<Tag> {
    let Some(entry) = post_entry(body) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    if let Some(subreddit) = first(&CATEGORY_RE, entry) {
        tags.push(Tag {
            name: subreddit.to_string(),
            source: TagSource::Booru,
            namespace: Some("subreddit".to_string()),
        });
    }
    if let Some(title) = first(&TITLE_RE, entry).filter(|title| !title.is_empty()) {
        tags.push(Tag {
            name: pycompat::strip(&pycompat::html_unescape(title)).to_string(),
            source: TagSource::Booru,
            namespace: Some("title".to_string()),
        });
    }
    tags
}

pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    let entry = post_entry(body)?;
    let content = pycompat::html_unescape(first(&CONTENT_RE, entry).unwrap_or(""));
    if let Some(link) = first(&LINK_RE, &content)
        && DIRECT_IMAGE_RE.is_match(link)
    {
        return Some(link.to_string());
    }
    // A gallery, or a link the feed only shows as a thumbnail: the preview is of the (first)
    // image, and its original has the same id.
    let preview = preview(entry)?;
    let found = PREVIEW_ID_RE.captures(&preview)?;
    Some(format!("https://i.redd.it/{}.{}", &found[1], &found[2]))
}

pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    preview(post_entry(body)?)
}

/// The original's size, read from its first bytes: `(None, None)` unless the file is on
/// `i.redd.it`. `remote_size(file_url, user_agent)` is Python's `_remote_size.image_size`, the
/// ranged read that needs a socket; it is called only for an `i.redd.it` file.
pub fn parse_dimensions(
    body: &str,
    url: &str,
    remote_size: impl FnOnce(&str, &str) -> (Option<i64>, Option<i64>),
) -> (Option<i64>, Option<i64>) {
    match parse_file_url(body, url) {
        Some(file_url) if on_host(&file_url, &["i.redd.it"]) => remote_size(&file_url, USER_AGENT),
        _ => (None, None),
    }
}

pub fn incomplete_reason(body: &str, url: &str) -> Option<String> {
    if post_entry(body).is_none() {
        return Some("Reddit's feed for this post was empty".to_string());
    }
    if parse_file_url(body, url).is_none() {
        return Some("the post links to something other than a picture Reddit hosts".to_string());
    }
    None
}

// -- internals --------------------------------------------------------

/// The post's own entry: the first; the rest are comments. The first entry is always the post
/// (id `t3_...`), but be sure.
fn post_entry(body: &str) -> Option<&str> {
    let entry = ENTRY_RE.captures(body)?.get(1)?.as_str();
    entry.contains("<id>t3_").then_some(entry)
}

fn preview(entry: &str) -> Option<String> {
    first(&THUMB_RE, entry).map(pycompat::html_unescape)
}

fn first<'a>(pattern: &Regex, text: &'a str) -> Option<&'a str> {
    pattern
        .captures(text)
        .map(|m| m.get(1).expect("group 1").as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str =
        "https://www.reddit.com/r/5ToubunNoHanayome/comments/1hhrx0v/fuu_with_miku_and_itsuki/";
    /// A real feed (post 1hhrx0v) with its comments replaced by one made-up entry.
    const FEED: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?><feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">"#,
        r#"<category term=" reddit.com" label="r/ reddit.com"/><id>/comments/1hhrx0v/.rss</id>"#,
        r#"<title>Fuu with Miku and Itsuki ~ : reddit.com</title><entry>"#,
        r#"<author><name>/u/CruelAngel94</name></author>"#,
        r#"<category term="5ToubunNoHanayome" label="r/5ToubunNoHanayome"/>"#,
        r#"<content type="html">&lt;table&gt; &lt;tr&gt;&lt;td&gt; &lt;a href=&quot;https://www.reddit.com/r/5ToubunNoHanayome/comments/1hhrx0v/fuu_with_miku_and_itsuki/&quot;&gt;"#,
        r#" &lt;img src=&quot;https://preview.redd.it/m42sxjdyus7e1.jpeg?width=640&amp;amp;crop=smart&quot; /&gt; &lt;/a&gt; &lt;/td&gt;&lt;td&gt;"#,
        r#" &lt;span&gt;&lt;a href=&quot;https://i.redd.it/m42sxjdyus7e1.jpeg&quot;&gt;[link]&lt;/a&gt;&lt;/span&gt; &lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;</content>"#,
        r#"<id>t3_1hhrx0v</id>"#,
        r#"<media:thumbnail url="https://preview.redd.it/m42sxjdyus7e1.jpeg?width=640&amp;crop=smart&amp;auto=webp" />"#,
        r#"<title>Fuu with Miku and Itsuki ~</title></entry>"#,
        r#"<entry><author><name>/u/someone</name></author><content type="html">&lt;p&gt;nice&lt;/p&gt;</content><id>t1_abc</id><title>/u/someone on Fuu</title></entry></feed>"#
    );

    // test_it_claims_posts_but_not_listings_or_users
    #[test]
    fn it_claims_posts_but_not_listings_or_users() {
        for url in [
            URL,
            "https://old.reddit.com/r/x/comments/1hhrx0v/",
            "https://www.reddit.com/comments/1hhrx0v",
            "https://redd.it/1hhrx0v",
            "https://www.redd.it/1HHRX0V?x=1",
        ] {
            assert!(matches(url), "{url}");
        }
        for url in [
            "https://www.reddit.com/r/5ToubunNoHanayome/",
            "https://www.reddit.com/user/someone",
            "https://i.redd.it/m42sxjdyus7e1.jpeg",
            "https://example.com/comments/1hhrx0v",
            "https://www.reddit.com/comments/ab",
            "https://www.reddit.com/comments/1234567890123",
            "https://www.reddit.com/gallery/1hhrx0v",
        ] {
            assert!(!matches(url), "{url}");
        }
        assert_eq!(
            post_id("https://redd.it/1HHRX0V").as_deref(),
            Some("1hhrx0v")
        );
    }

    // test_the_feed_it_fetches_is_the_posts_own
    #[test]
    fn the_feed_it_fetches_is_the_posts_own() {
        assert_eq!(
            resolve_fetch_url(URL),
            "https://www.reddit.com/comments/1hhrx0v/.rss"
        );
        assert_eq!(
            resolve_fetch_url("https://www.reddit.com/"),
            "https://www.reddit.com/"
        );
    }

    // test_subreddit_and_title_become_tags_and_the_poster_does_not
    #[test]
    fn subreddit_and_title_become_tags_and_the_poster_does_not() {
        let tags: Vec<_> = parse(FEED, URL)
            .into_iter()
            .map(|t| (t.namespace.unwrap(), t.name))
            .collect();
        assert_eq!(
            tags,
            [
                ("subreddit".to_string(), "5ToubunNoHanayome".to_string()),
                (
                    "title".to_string(),
                    "Fuu with Miku and Itsuki ~".to_string()
                ),
            ]
        );
        // The title is unescaped and stripped, and kept even when nothing is left of it.
        let feed = FEED.replace(
            "<title>Fuu with Miku and Itsuki ~</title></entry>",
            "<title> &amp; &#32;</title></entry>",
        );
        assert_eq!(parse(&feed, URL)[1].name, "&");
        let feed = FEED.replace(
            "Fuu with Miku and Itsuki ~</title></entry>",
            " </title></entry>",
        );
        assert_eq!(parse(&feed, URL)[1].name, "");
    }

    // test_the_original_and_the_preview
    #[test]
    fn the_original_and_the_preview() {
        assert_eq!(
            parse_file_url(FEED, URL).as_deref(),
            Some("https://i.redd.it/m42sxjdyus7e1.jpeg")
        );
        assert_eq!(
            parse_preview_url(FEED, URL).as_deref(),
            Some("https://preview.redd.it/m42sxjdyus7e1.jpeg?width=640&crop=smart&auto=webp")
        );
    }

    #[test]
    fn an_imgur_direct_link_is_taken_and_other_links_are_not() {
        for (link, direct) in [
            ("https://i.imgur.com/Ab_c-1.JPG", true),
            ("https://i.redd.it/abc.webp", true),
            ("https://imgur.com/abc", false),
            ("https://i.imgur.com/abc.mp4", false),
            ("https://i.redd.it/abc.png?x=1", false),
            ("https://youtu.be/x", false),
        ] {
            let feed = FEED.replace("https://i.redd.it/m42sxjdyus7e1.jpeg", link);
            let want = if direct {
                link.to_string()
            } else {
                // Not a picture of its own: the thumbnail's id names the original.
                "https://i.redd.it/m42sxjdyus7e1.jpeg".to_string()
            };
            assert_eq!(parse_file_url(&feed, URL), Some(want), "{link}");
        }
    }

    // test_dimensions_are_read_from_the_original
    #[test]
    fn dimensions_are_read_from_the_original_on_reddits_own_host_only() {
        let size = parse_dimensions(FEED, URL, |file_url, agent| {
            assert_eq!(file_url, "https://i.redd.it/m42sxjdyus7e1.jpeg");
            assert_eq!(agent, USER_AGENT);
            (Some(2339), Some(3276))
        });
        assert_eq!(size, (Some(2339), Some(3276)));

        // An imgur link is a file, but not one this reads the size of; nothing is asked.
        let imgur = FEED.replace(
            "https://i.redd.it/m42sxjdyus7e1.jpeg",
            "https://i.imgur.com/a.png",
        );
        let never = |_: &str, _: &str| -> (Option<i64>, Option<i64>) { panic!("asked for a size") };
        assert_eq!(parse_dimensions(&imgur, URL, never), (None, None));
        assert_eq!(parse_dimensions("<feed></feed>", URL, never), (None, None));
    }

    // test_a_gallery_is_its_first_image
    #[test]
    fn a_gallery_is_its_first_image() {
        let feed = FEED.replace(
            "https://i.redd.it/m42sxjdyus7e1.jpeg",
            "https://www.reddit.com/gallery/1hhrx0v",
        );
        assert_eq!(
            parse_file_url(&feed, URL).as_deref(),
            Some("https://i.redd.it/m42sxjdyus7e1.jpeg")
        );
    }

    // test_a_link_to_elsewhere_with_no_preview_says_why
    #[test]
    fn a_link_to_elsewhere_with_no_preview_says_why() {
        let feed = FEED.replace("https://i.redd.it/m42sxjdyus7e1.jpeg", "https://youtu.be/x");
        let no_thumb = Regex::new(r#"<media:thumbnail url="[^"]+" />"#).unwrap();
        let feed = no_thumb.replace(&feed, "");
        assert_eq!(parse_file_url(&feed, URL), None);
        assert!(
            incomplete_reason(&feed, URL)
                .unwrap()
                .contains("something other than a picture")
        );
        assert_eq!(incomplete_reason(FEED, URL), None);
    }

    // test_an_empty_feed_claims_nothing
    #[test]
    fn an_empty_feed_claims_nothing() {
        let empty = r#"<?xml version="1.0"?><feed></feed>"#;
        assert!(parse(empty, URL).is_empty());
        assert_eq!(parse_file_url(empty, URL), None);
        assert_eq!(parse_preview_url(empty, URL), None);
        assert_eq!(
            incomplete_reason(empty, URL).as_deref(),
            Some("Reddit's feed for this post was empty")
        );
        // A first entry that is not the post (a comment) is no post either.
        let comment = FEED.replacen("<id>t3_", "<id>t1_", 1);
        assert!(parse(&comment, URL).is_empty());
    }

    #[test]
    fn redditpost_defines_the_hooks_python_does_but_the_remote_size_read() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "incomplete_reason"
            ]
        );
    }
}
