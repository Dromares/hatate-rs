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

use futures::StreamExt;

use super::host::on_host;
use super::{Context, SiteParser, gelbooru};
use crate::engines::client::urlencode;
use crate::image_compare::{Hash, Source, dhash};
use crate::model::Tag;
use crate::net::NetClient;
use crate::pycompat;
use crate::pyimage::DecompressionBomb;
use crate::pyresponse::PyResponse;
use crate::remote::{get_request, ua_headers};

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
// "Missing authentication".

/// `LISTING_URL`.
pub const LISTING_URL: &str = "https://rule34.xxx/index.php";

/// `POST_URL.format(id)`.
pub fn post_url(post_id: &str) -> String {
    format!("https://rule34.xxx/index.php?page=post&s=view&id={post_id}")
}

/// `LISTING_HASH_SIZE`: a 16x16 dhash, because the 8x8 one cannot tell variants apart. Measured:
/// the parent and two sibling variants (the same scene edited) all hashed to distance 0 at 8x8. At
/// 16x16 the right post was 2 out of 256 and the nearest variant 9; a different picture was 20+.
pub const LISTING_HASH_SIZE: usize = 16;

/// `LISTING_MAX_DISTANCE`.
pub const LISTING_MAX_DISTANCE: i64 = 6;

/// `LISTING_THUMB_WORKERS`.
pub const LISTING_THUMB_WORKERS: usize = 5;

/// `(post id, thumbnail URL)` for each post on a listing page.
pub fn listing_posts(html: &str) -> Vec<(String, String)> {
    LISTING_POST_RE
        .captures_iter(html)
        .map(|m| (m[1].to_string(), m[2].to_string()))
        .collect()
}

/// A thumbnail's `(distance, post id, thumbnail URL)`.
type Scored = (i64, String, String);

/// `find_post_by_search(tags, local_hash, timeout)`: `(post URL, thumbnail URL, distance)` of the
/// post on the first page of this search that is the same picture as `local_hash`, or `None`.
///
/// `local_hash` is a dhash at [`LISTING_HASH_SIZE`]. Only the first page is read, one request and
/// a page of small thumbnails, which covers the precise searches (`parent:`, `id:`) this exists
/// for. The thumbnails are fetched five at a time; every one is asked for even when an earlier one
/// turns out to be a decompression bomb. Python's pool cancels the thumbnails it has not started
/// when the bomb is raised, so how many go out there depends on timing; all of them is one of the
/// things it does.
pub async fn find_post_by_search(
    net: &NetClient,
    tags: &str,
    local_hash: Option<&Hash>,
    timeout: f64,
) -> Result<Option<(String, String, i64)>, DecompressionBomb> {
    let tags = pycompat::strip(tags);
    let Some(local_hash) = local_hash.filter(|_| !tags.is_empty()) else {
        return Ok(None);
    };
    let headers = ua_headers();
    let query = urlencode(&[("page", "post"), ("s", "list"), ("tags", tags)]);
    let Ok(req) = get_request(
        &format!("{LISTING_URL}?{query}"),
        timeout,
        None,
        &headers,
        None,
    ) else {
        return Ok(None);
    };
    let resp = match net.send(req).await {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) => {
            debug!("rule34 listing for {tags:?} failed: {e}");
            return Ok(None);
        }
    };
    if resp.status != 200 {
        debug!("rule34 listing for {tags:?} returned HTTP {}", resp.status);
        return Ok(None);
    }
    let posts = listing_posts(&resp.text());
    if posts.is_empty() {
        return Ok(None);
    }

    let distance = |(post_id, thumb): (String, String)| {
        let headers = headers.clone();
        async move {
            let Ok(req) = get_request(&thumb, timeout, None, &headers, None) else {
                return Ok::<_, DecompressionBomb>(None);
            };
            let Ok(resp) = net.send(req).await else {
                return Ok(None);
            };
            let data = PyResponse::from_net(resp).content;
            let Some(other) = dhash(Source::Bytes(&data), LISTING_HASH_SIZE)? else {
                return Ok(None);
            };
            Ok(Some((
                i64::from(local_hash.xor_popcount(&other)),
                post_id,
                thumb,
            )))
        }
    };
    let results: Vec<Result<Option<Scored>, DecompressionBomb>> =
        futures::stream::iter(posts.into_iter().map(distance))
            .buffered(LISTING_THUMB_WORKERS)
            .collect()
            .await;
    let mut scored = Vec::new();
    for result in results {
        if let Some(entry) = result? {
            scored.push(entry);
        }
    }
    // `min(scored)`: the tuple order, distance then id then thumbnail.
    let Some((best_distance, post_id, thumb)) = scored.into_iter().min() else {
        return Ok(None);
    };
    debug!("rule34 listing {tags:?}: closest is {post_id} at distance {best_distance}");
    if best_distance > LISTING_MAX_DISTANCE {
        return Ok(None);
    }
    Ok(Some((post_url(&post_id), thumb, best_distance)))
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
