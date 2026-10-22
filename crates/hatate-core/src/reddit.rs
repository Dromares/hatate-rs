//! Turning a subreddit link into the one post a match came from (`core/reddit.py`).
//!
//! Google Lens sometimes answers with a SUBREDDIT, `reddit.com/r/Name/`, rather than the post the
//! picture is in. Handed to Hydrus, that is a subreddit gallery URL, and Hydrus downloads the
//! whole subreddit. What Lens does give alongside it is the picture's own reddit file,
//! `i.redd.it/<id>.png`, as the result's thumbnail. Every image post embeds its file, so the post
//! is the one in that subreddit whose entry names that id. Reddit's JSON API answers 403 without
//! an account, but each subreddit's public RSS feed still works and carries the image ids.
//!
//! When the post cannot be found, the match becomes the direct image link instead. That still
//! gets the picture, and never the whole subreddit.
//!
//! This is the parse half. The request half is Phase 3: the HTTP GET of a feed (Python's
//! `net.get` with [`USER_AGENT`]), the per-session `_feed_cache` it fills (keyed by
//! [`feed_cache_key`], only for a 200 answer), and the loop over [`FEEDS`] that
//! [`resolve_listing`] is handed as `find_post`.

use std::sync::LazyLock;

use regex::Regex;
use tracing::info;

use crate::pycompat;

pub const USER_AGENT: &str = "linux:hatate-linux:1.0 (reverse image tagger)";

/// Newest first, then the all-time top: between them they cover what a subreddit's visitors, and
/// so Google, are most likely to have seen. Each is a single request, cached per subreddit for
/// the session.
pub const FEEDS: [&str; 2] = ["new/.rss?limit=100", "top/.rss?t=all&limit=100"];

// Python's `$` also matches before a final newline; `\n?\z` is the same thing. `.` excludes only
// `\n` in both.
static SUBREDDIT_LISTING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^https?://(?:www\.|old\.|new\.)?reddit\.com/r/([A-Za-z0-9_]+)/?(?:(?:hot|new|top|rising|controversial)/?)?(?:[?#].*)?\n?\z",
    )
    .expect("subreddit listing regex")
});
static MEDIA_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://(?:i|preview)\.redd\.it/([A-Za-z0-9]+)\.([A-Za-z0-9]+)")
        .expect("media regex")
});
static ENTRY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<entry>(.*?)</entry>").expect("entry regex"));
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<link href="([^"]+/comments/[^"]+)""#).expect("link regex"));

/// The subreddit a listing URL shows, or `None` if it is not one: a post, a user page and every
/// other site are all `None`.
pub fn subreddit_of(url: &str) -> Option<String> {
    SUBREDDIT_LISTING_RE
        .captures(pycompat::strip(url))
        .map(|m| m[1].to_string())
}

/// `(id, extension)` of a reddit-hosted image URL, or `None`.
pub fn media_of(url: Option<&str>) -> Option<(String, String)> {
    MEDIA_RE
        .captures(pycompat::strip(url?))
        .map(|m| (m[1].to_string(), m[2].to_string()))
}

/// The address of one subreddit feed.
pub fn feed_url(subreddit: &str, feed: &str) -> String {
    format!("https://www.reddit.com/r/{subreddit}/{feed}")
}

/// What the session cache files a feed under: `{subreddit.lower()}/{feed}`.
pub fn feed_cache_key(subreddit: &str, feed: &str) -> String {
    format!("{}/{feed}", subreddit.to_lowercase())
}

/// The entries of a feed answer: the text between each `<entry>` and `</entry>`. `None` for any
/// status but 200 (a 429 clears too, so it is not remembered either).
pub fn feed_entries(status: u16, text: &str) -> Option<Vec<String>> {
    if status != 200 {
        return None;
    }
    Some(
        ENTRY_RE
            .captures_iter(text)
            .map(|m| m[1].to_string())
            .collect(),
    )
}

/// The link of the first entry that names `media_id`, among one feed's entries.
pub fn find_post_in(entries: &[String], media_id: &str) -> Option<String> {
    entries
        .iter()
        .filter(|entry| entry.contains(media_id))
        .find_map(|entry| LINK_RE.captures(entry).map(|m| m[1].to_string()))
}

/// What to use instead of a subreddit listing URL.
///
/// The post that holds the image if it can be found, else the image's own `i.redd.it` link, else
/// `None`, meaning there is nothing safe to offer, since the listing itself would import the
/// whole subreddit. Returns `url` unchanged when it is not a subreddit listing.
///
/// `find_post(subreddit, media_id)` is Python's `find_post`: the lookup across [`FEEDS`], which
/// needs the network and so is the caller's.
pub fn resolve_listing(
    url: &str,
    image_url: Option<&str>,
    find_post: impl FnOnce(&str, &str) -> Option<String>,
) -> Option<String> {
    let Some(subreddit) = subreddit_of(url) else {
        return Some(url.to_string());
    };
    let Some((media_id, ext)) = media_of(image_url) else {
        info!("Dropping {url}: a subreddit, with no image to find the post by");
        return None;
    };
    if let Some(post) = find_post(&subreddit, &media_id) {
        info!("Resolved {url} to the post holding its image: {post}");
        return Some(post);
    }
    let direct = format!("https://i.redd.it/{media_id}.{ext}");
    info!(
        "Could not find the post in r/{subreddit} holding {direct} - using the image link itself \
         rather than the whole subreddit"
    );
    Some(direct)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?><feed xmlns="http://www.w3.org/2005/Atom">"#,
        r#"<entry><author><name>/u/someone</name></author><content type="html">"#,
        "&lt;a href=&quot;https://www.reddit.com/r/YanchaGalAnjouSan/comments/1vz8vo8/",
        "manga_cover/&quot;&gt; &lt;img src=&quot;https://preview.redd.it/ogpxyscp4slh1.png",
        "?width=140&amp;amp;height=140&quot; alt=&quot;Manga cover&quot; /&gt;",
        "</content><id>t3_1vz8vo8</id>",
        r#"<link href="https://www.reddit.com/r/YanchaGalAnjouSan/comments/1vz8vo8/manga_cover/" />"#,
        "<title>Manga cover</title></entry>",
        r#"<entry><content type="html">&lt;img src=&quot;https://preview.redd.it/other1.jpg&quot;"#,
        "</content>",
        r#"<link href="https://www.reddit.com/r/YanchaGalAnjouSan/comments/1aaaaaa/other/" />"#,
        "</entry></feed>"
    );
    const SUB: &str = "https://www.reddit.com/r/YanchaGalAnjouSan/";
    const POST: &str = "https://www.reddit.com/r/YanchaGalAnjouSan/comments/1vz8vo8/manga_cover/";

    /// What `resolve_listing` is handed in place of the network: the lookup over a canned feed.
    fn lookup(subreddit: &str, media_id: &str) -> Option<String> {
        assert_eq!(subreddit, "YanchaGalAnjouSan");
        let entries = feed_entries(200, FEED)?;
        find_post_in(&entries, media_id)
    }

    // test_only_listings_count_as_subreddits
    #[test]
    fn only_listings_count_as_subreddits() {
        assert_eq!(subreddit_of(SUB).as_deref(), Some("YanchaGalAnjouSan"));
        assert_eq!(
            subreddit_of("https://old.reddit.com/r/manga/new/").as_deref(),
            Some("manga")
        );
        for listing in [
            "https://reddit.com/r/manga",
            "https://new.reddit.com/r/manga/top/?t=all",
            "  HTTPS://WWW.REDDIT.COM/r/manga/Hot#x  ",
            "https://www.reddit.com/r/manga/controversial/",
        ] {
            assert_eq!(subreddit_of(listing).as_deref(), Some("manga"), "{listing}");
        }
        for other in [
            POST,
            "https://www.reddit.com/user/someone/",
            "https://example.com/r/manga/",
            "https://www.reddit.com/r/manga/comments/1/x/",
            "https://www.reddit.com/r/manga/best/",
            "https://www.reddit.com/r/manga?a\nb",
            "",
        ] {
            assert_eq!(subreddit_of(other), None, "{other:?}");
        }
    }

    #[test]
    fn media_is_an_id_and_an_extension() {
        for (url, want) in [
            (
                "https://i.redd.it/ogpxyscp4slh1.png",
                ("ogpxyscp4slh1", "png"),
            ),
            (
                " https://preview.redd.it/m42sxjdyus7e1.jpeg?width=640 ",
                ("m42sxjdyus7e1", "jpeg"),
            ),
        ] {
            assert_eq!(
                media_of(Some(url)),
                Some((want.0.to_string(), want.1.to_string()))
            );
        }
        for other in [
            Some("https://cdn.example.com/a.jpg"),
            Some("https://i.redd.it/x"),
            Some(""),
            None,
        ] {
            assert_eq!(media_of(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_feed_is_filed_by_its_lowered_subreddit_and_the_feed_path() {
        assert_eq!(
            feed_url("YanchaGalAnjouSan", FEEDS[0]),
            "https://www.reddit.com/r/YanchaGalAnjouSan/new/.rss?limit=100"
        );
        assert_eq!(
            feed_cache_key("YanchaGalAnjouSan", FEEDS[1]),
            "yanchagalanjousan/top/.rss?t=all&limit=100"
        );
    }

    // test_a_rate_limited_feed_falls_back_and_is_not_remembered
    #[test]
    fn only_a_200_feed_has_entries() {
        for status in [429, 404, 500, 201, 301] {
            assert_eq!(feed_entries(status, FEED), None, "{status}");
        }
        let entries = feed_entries(200, FEED).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].starts_with("<author>"));
        assert_eq!(feed_entries(200, "no entries here"), Some(vec![]));
    }

    // test_the_post_holding_the_image_is_found
    #[test]
    fn the_post_holding_the_image_is_found() {
        let image = Some("https://i.redd.it/ogpxyscp4slh1.png");
        assert_eq!(resolve_listing(SUB, image, lookup).as_deref(), Some(POST));
    }

    // test_an_unfound_post_falls_back_to_the_image_not_the_subreddit
    #[test]
    fn an_unfound_post_falls_back_to_the_image_not_the_subreddit() {
        assert_eq!(
            resolve_listing(SUB, Some("https://i.redd.it/notinfeed.png"), lookup).as_deref(),
            Some("https://i.redd.it/notinfeed.png")
        );
        // The preview host names the same file; the fallback is always the i.redd.it original.
        assert_eq!(
            resolve_listing(
                SUB,
                Some("https://preview.redd.it/notinfeed.jpg?w=1"),
                |_, _| None
            )
            .as_deref(),
            Some("https://i.redd.it/notinfeed.jpg")
        );
    }

    // test_a_subreddit_with_no_image_to_go_on_is_dropped
    #[test]
    fn a_subreddit_with_no_image_to_go_on_is_dropped() {
        let never = |_: &str, _: &str| -> Option<String> { panic!("looked a post up") };
        assert_eq!(resolve_listing(SUB, None, never), None);
        assert_eq!(
            resolve_listing(SUB, Some("https://cdn.example.com/a.jpg"), never),
            None
        );
    }

    // test_other_urls_are_untouched_and_never_fetch
    #[test]
    fn other_urls_are_untouched_and_never_looked_up() {
        let never = |_: &str, _: &str| -> Option<String> { panic!("looked a post up") };
        assert_eq!(
            resolve_listing(POST, Some("https://i.redd.it/x.png"), never).as_deref(),
            Some(POST)
        );
        assert_eq!(
            resolve_listing("https://gelbooru.com/a", None, never).as_deref(),
            Some("https://gelbooru.com/a")
        );
    }

    #[test]
    fn the_first_entry_naming_the_id_with_a_post_link_wins() {
        let entries = vec![
            "mentions abc but has no link".to_string(),
            r#"abc <link href="https://www.reddit.com/r/x/comments/1/a/" />"#.to_string(),
            r#"abc <link href="https://www.reddit.com/r/x/comments/2/b/" />"#.to_string(),
        ];
        assert_eq!(
            find_post_in(&entries, "abc").as_deref(),
            Some("https://www.reddit.com/r/x/comments/1/a/")
        );
        assert_eq!(find_post_in(&entries, "zzz"), None);
        assert_eq!(find_post_in(&[], "abc"), None);
    }
}
