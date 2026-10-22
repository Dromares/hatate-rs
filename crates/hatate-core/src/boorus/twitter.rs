//! Twitter/X, via FxTwitter's public JSON API (`core/boorus/twitter.py`).
//!
//! Twitter's own pages are JS-driven and need a logged-in session, so there is nothing there for
//! a parser to read. `api.fxtwitter.com` answers anonymously with the author and, for each photo,
//! its ORIGINAL `pbs.twimg.com` URL plus real width and height. That fixes two things the page
//! could not: a deleted tweet is told apart from a live one (the API 404s only when the tweet has
//! really gone), and the file and preview are the real picture and not the search engine's
//! thumbnail.
//!
//! No tags beyond the author: a tweet has no booru-style vocabulary, and its text is prose.
//!
//! Where Python would raise on a shape the API never sends (an `author` that is a list), the hook
//! returns what `fetch_page_info` would make of it: nothing.

use std::sync::LazyLock;

use regex::{NoExpand, Regex};
use serde_json::{Map, Value, json};

use super::host::on_host;
use super::json_fields::int_of;
use super::parsed::json_of;
use super::sizes::py_int;
use super::{Context, Page, SiteParser};
use crate::model::{Tag, TagSource};
use crate::pycompat;

/// Deliberately not `t.co` (a shortener, with no id to read) or `pbs.twimg.com` (media itself,
/// belonging to no identifiable tweet): those stay classification-only.
const HOSTS: [&str; 6] = [
    "twitter.com",
    "x.com",
    "fxtwitter.com",
    "vxtwitter.com",
    "fixvx.com",
    "fixupx.com",
];

const API_ENDPOINT: &str = "https://api.fxtwitter.com/status";

// `\d` is Unicode in both Python's `re` and here, so the two read the same digits.
static TWEET_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/status(?:es)?/(\d+)").expect("tweet id regex"));
/// Search engines sometimes name which image of a multi-photo tweet matched, right in the URL.
static PHOTO_INDEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/photo/(\d+)").expect("photo index regex"));
static NAME_PARAM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"name=[^&]*").expect("name param regex"));

/// Twitter's own render sizes. `orig` is uncapped; `large` is the 2048px render, plenty for a
/// preview; `small` is the 680px one, which is cheap enough to hash a whole tweet's photos with.
const ORIGINAL_SIZE: &str = "orig";
const PREVIEW_SIZE: &str = "large";
const THUMBNAIL_SIZE: &str = "small";

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _: &Context| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_page_count: Some(|body, url, _| parse_page_count(body, url)),
    parse_pages: Some(|body, url, _| parse_pages(body, url)),
    pages_api_url: Some(pages_api_url),
    ..SiteParser::new("twitter", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// The API record for the tweet the URL names; a URL with no tweet id is left alone.
pub fn resolve_fetch_url(url: &str) -> String {
    match TWEET_ID_RE.captures(url) {
        Some(m) => format!("{API_ENDPOINT}/{}", &m[1]),
        None => url.to_string(),
    }
}

/// The author, as an `artist` tag: the only tag a tweet really has, stored under "artist" so the
/// user's artist -> creator remap fires, as for every other parser.
pub fn parse(body: &str, _url: &str) -> Vec<Tag> {
    let Some(tweet) = tweet(body) else {
        return Vec::new();
    };
    let author = match tweet.get("author").filter(|v| pycompat::truthy(v)) {
        None => return Vec::new(),
        Some(Value::Object(author)) => author.get("screen_name"),
        Some(_) => return Vec::new(),
    };
    let Some(author) = author.and_then(Value::as_str) else {
        return Vec::new();
    };
    let author = pycompat::strip(author);
    if author.is_empty() {
        return Vec::new();
    }
    vec![Tag {
        name: author.replace(' ', "_"),
        source: TagSource::Booru,
        namespace: Some("artist".to_string()),
    }]
}

pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    photo_url(body, url, ORIGINAL_SIZE)
}

pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    photo_url(body, url, PREVIEW_SIZE)
}

/// The matched photo's real width and height, or `(None, None)` unless both are known.
pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let Some(photo) = photo(body, url) else {
        return (None, None);
    };
    match (as_int(photo.get("width")), as_int(photo.get("height"))) {
        (Some(width), Some(height)) => (Some(width), Some(height)),
        _ => (None, None),
    }
}

pub fn parse_page_count(body: &str, _url: &str) -> i64 {
    match photos(body).len() {
        0 => 1,
        count => count as i64,
    }
}

// -- which image of the tweet ------------------------------------------
// Opting in to the multi-page resolver, which leaves a multi-image post on its FIRST image
// unless the parser can list the others. The /photo/N suffix `photo` reads is no substitute:
// SauceNAO's result URLs carry none, and the API's own media facets name /photo/1 for every
// photo in the tweet.

/// Where the photo list comes from: the same API record the tags were read from, so the resolver
/// reuses that body instead of refetching. `None` for a URL with no tweet id.
pub fn pages_api_url(url: &str) -> Option<String> {
    let fetch_url = resolve_fetch_url(url);
    (fetch_url != url).then_some(fetch_url)
}

/// One entry per photo, in the tweet's order, shaped like Pixiv's page list. Every photo carries
/// its real width and height, which the resolver can often settle the question with outright;
/// `small` is the render that gets hashed when it cannot. A photo with no usable URL is left out
/// and does not shift the others' indexes.
pub fn parse_pages(body: &str, _url: &str) -> Vec<Page> {
    photos(body)
        .iter()
        .filter_map(|photo| {
            let raw = photo.get("url");
            let original = sized(raw, ORIGINAL_SIZE)?;
            let Value::Object(page) = json!({
                "urls": {
                    "original": original,
                    "regular": sized(raw, PREVIEW_SIZE),
                    "small": sized(raw, THUMBNAIL_SIZE),
                },
                "width": as_int(photo.get("width")),
                "height": as_int(photo.get("height")),
            }) else {
                unreachable!("an object literal")
            };
            Some(page)
        })
        .collect()
}

// -- internals --------------------------------------------------------

/// The API record's `tweet` object. An empty body, a body that is not JSON (Twitter's own HTML,
/// reached because the URL carried no tweet id) and a record with no tweet all give `None`.
fn tweet(body: &str) -> Option<Map<String, Value>> {
    if body.is_empty() {
        return None;
    }
    let Value::Object(mut data) = json_of(body).ok()? else {
        return None;
    };
    match data.remove("tweet")? {
        Value::Object(tweet) => Some(tweet),
        _ => None,
    }
}

/// The photos that are objects, in order.
fn photos(body: &str) -> Vec<Map<String, Value>> {
    let Some(mut tweet) = tweet(body) else {
        return Vec::new();
    };
    let Some(Value::Object(mut media)) = tweet.remove("media") else {
        return Vec::new();
    };
    let Some(Value::Array(photos)) = media.remove("photos") else {
        return Vec::new();
    };
    photos
        .into_iter()
        .filter_map(|photo| match photo {
            Value::Object(photo) => Some(photo),
            _ => None,
        })
        .collect()
}

/// The photo this match is about. A tweet can hold four images. When the URL names which one
/// (the /photo/N suffix some engines produce) that is used; otherwise the first, the same default
/// the app applies to every other multi-image site it cannot resolve.
fn photo(body: &str, url: &str) -> Option<Map<String, Value>> {
    let mut photos = photos(body);
    if photos.is_empty() {
        return None;
    }
    let mut index = 0;
    if let Some(named) = PHOTO_INDEX_RE.captures(url) {
        // /photo/1 is the first. A number too big for i64 is as out of range as any other.
        let candidate = py_int(&named[1]).map(|n| n - 1);
        match candidate
            .and_then(|c| usize::try_from(c).ok())
            .filter(|c| *c < photos.len())
        {
            Some(candidate) => index = candidate,
            None => tracing::debug!(
                "{url} names photo {} but the tweet has {} - using the first",
                &named[1],
                photos.len()
            ),
        }
    }
    Some(photos.swap_remove(index))
}

fn photo_url(body: &str, url: &str, size: &str) -> Option<String> {
    sized(photo(body, url)?.get("url"), size)
}

/// One photo's URL at the render size wanted. Only an absolute `http(s)` string is a URL. The API
/// hands out `...?name=orig`; the size wanted is swapped in rather than assuming which one it
/// used.
fn sized(raw: Option<&Value>, size: &str) -> Option<String> {
    let raw = raw?.as_str()?;
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return None;
    }
    if raw.contains("name=") {
        let wanted = format!("name={size}");
        return Some(
            NAME_PARAM_RE
                .replace_all(raw, NoExpand(&wanted))
                .into_owned(),
        );
    }
    let joiner = if raw.contains('?') { '&' } else { '?' };
    Some(format!("{raw}{joiner}name={size}"))
}

/// `int(value or 0) or None`: a number that is not zero, from anything `int()` reads, and `None`
/// for a value it rejects.
fn as_int(value: Option<&Value>) -> Option<i64> {
    let value = value.filter(|v| pycompat::truthy(v))?;
    int_of(value).filter(|n| *n != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://twitter.com/i/web/status/1060134528432230400";

    fn body(photos: Option<Value>) -> String {
        let photos = photos.unwrap_or_else(|| {
            json!([
                {"type": "photo", "url": "https://pbs.twimg.com/media/AAA.png?name=orig",
                 "width": 808, "height": 650},
                {"type": "photo", "url": "https://pbs.twimg.com/media/BBB.png?name=orig",
                 "width": 530, "height": 750},
            ])
        });
        json!({"code": 200, "message": "OK", "tweet": {
            "id": "1060134528432230400",
            "author": {"screen_name": "poppuqn", "name": "ポップ"},
            "media": {"photos": photos},
        }})
        .to_string()
    }

    #[test]
    fn claims_the_tweet_hosts_and_not_media_or_shorteners() {
        for url in [
            "https://twitter.com/user/status/1",
            "https://x.com/user/status/1",
            "https://fxtwitter.com/user/status/1",
            "https://vxtwitter.com/user/status/1",
            "https://fixvx.com/user/status/1",
            "https://fixupx.com/user/status/1",
        ] {
            assert!(matches(url), "{url}");
        }
        // test_media_and_shortener_urls_stay_unparsed, test_anilist_is_not_twitter_because_of_t_co
        for url in [
            "https://pbs.twimg.com/media/abc.jpg",
            "https://t.co/abc",
            "https://anilist.co/anime/21",
            "https://hentaivox.com/x.com",
        ] {
            assert!(!matches(url), "{url}");
        }
    }

    // test_tweet_urls_resolve_to_the_api, test_a_url_with_no_tweet_id_is_left_alone
    #[test]
    fn tweet_urls_resolve_to_the_api() {
        for url in [
            "https://twitter.com/i/web/status/123",
            "https://x.com/user/status/123",
            "https://fxtwitter.com/user/status/123",
            "https://twitter.com/user/statuses/123",
        ] {
            assert_eq!(
                resolve_fetch_url(url),
                "https://api.fxtwitter.com/status/123"
            );
        }
        for url in ["https://twitter.com/someone", "https://x.com/"] {
            assert_eq!(resolve_fetch_url(url), url);
        }
    }

    // test_the_author_becomes_an_artist_tag
    #[test]
    fn the_author_becomes_an_artist_tag() {
        let tags = parse(&body(None), URL);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].namespace.as_deref(), Some("artist"));
        assert_eq!(tags[0].name, "poppuqn");
        let spaced = json!({"tweet": {"author": {"screen_name": "  a b "}}}).to_string();
        assert_eq!(parse(&spaced, URL)[0].name, "a_b");
        for odd in [
            json!({"tweet": {"author": {"screen_name": "  "}}}),
            json!({"tweet": {"author": {"screen_name": 5}}}),
            json!({"tweet": {"author": ["x"]}}),
            json!({"tweet": {}}),
            json!({"tweet": "x"}),
            json!([1]),
        ] {
            assert!(parse(&odd.to_string(), URL).is_empty(), "{odd}");
        }
    }

    // test_real_dimensions_are_reported, test_a_url_naming_a_photo_selects_that_one,
    // test_an_out_of_range_photo_falls_back_to_the_first
    #[test]
    fn the_url_can_name_the_photo() {
        let b = body(None);
        assert_eq!(parse_dimensions(&b, URL), (Some(808), Some(650)));
        let at = |suffix: &str| parse_dimensions(&b, &format!("{URL}{suffix}"));
        assert_eq!(at("/photo/2"), (Some(530), Some(750)));
        assert_eq!(at("/photo/1"), (Some(808), Some(650)));
        for out_of_range in ["/photo/9", "/photo/0", "/photo/99999999999999999999999"] {
            assert_eq!(at(out_of_range), (Some(808), Some(650)), "{out_of_range}");
        }
        assert!(
            parse_file_url(&b, &format!("{URL}/photo/2"))
                .unwrap()
                .contains("BBB")
        );
    }

    // test_the_file_is_the_uncapped_original_and_the_preview_is_the_large_render
    #[test]
    fn the_file_is_the_original_and_the_preview_is_the_large_render() {
        let b = body(None);
        assert_eq!(
            parse_file_url(&b, URL).as_deref(),
            Some("https://pbs.twimg.com/media/AAA.png?name=orig")
        );
        assert_eq!(
            parse_preview_url(&b, URL).as_deref(),
            Some("https://pbs.twimg.com/media/AAA.png?name=large")
        );
    }

    #[test]
    fn the_size_is_swapped_in_or_added() {
        let size = |raw: Value| sized(Some(&raw), "large");
        assert_eq!(
            size(json!("https://p/a.png?name=orig&x=1")).as_deref(),
            Some("https://p/a.png?name=large&x=1")
        );
        assert_eq!(
            size(json!("https://p/a.png?name=a&y=2&name=b")).as_deref(),
            Some("https://p/a.png?name=large&y=2&name=large")
        );
        assert_eq!(
            size(json!("https://p/a.png")).as_deref(),
            Some("https://p/a.png?name=large")
        );
        assert_eq!(
            size(json!("http://p/a.png?x=1")).as_deref(),
            Some("http://p/a.png?x=1&name=large")
        );
        for odd in [json!("//p/a.png"), json!("a.png"), json!(5), json!(null)] {
            assert_eq!(size(odd.clone()), None, "{odd}");
        }
        assert_eq!(sized(None, "large"), None);
    }

    // test_page_count_reflects_the_tweets_images
    #[test]
    fn page_count_reflects_the_tweets_images() {
        assert_eq!(parse_page_count(&body(None), URL), 2);
        assert_eq!(parse_page_count(&body(Some(json!([]))), URL), 1);
    }

    // test_the_photo_list_is_offered_for_the_multipage_resolver,
    // test_the_photo_list_is_fetched_from_the_record_already_read,
    // test_a_url_with_no_tweet_id_offers_no_photo_list
    #[test]
    fn the_photo_list_is_offered_for_the_multipage_resolver() {
        assert_eq!(
            pages_api_url(URL).as_deref(),
            Some("https://api.fxtwitter.com/status/1060134528432230400")
        );
        assert_eq!(pages_api_url(URL), Some(resolve_fetch_url(URL)));
        assert_eq!(pages_api_url("https://twitter.com/someone"), None);

        let pages = parse_pages(&body(None), URL);
        let dims: Vec<_> = pages
            .iter()
            .map(|p| (p["width"].clone(), p["height"].clone()))
            .collect();
        assert_eq!(dims, [(json!(808), json!(650)), (json!(530), json!(750))]);
        assert_eq!(
            pages[1]["urls"],
            json!({
                "original": "https://pbs.twimg.com/media/BBB.png?name=orig",
                "regular": "https://pbs.twimg.com/media/BBB.png?name=large",
                "small": "https://pbs.twimg.com/media/BBB.png?name=small",
            })
        );
    }

    // test_a_photo_with_no_usable_url_is_left_out_of_the_list
    #[test]
    fn a_photo_with_no_usable_url_is_left_out_of_the_list() {
        let b = body(Some(json!([
            {"type": "photo", "url": null, "width": 1, "height": 2},
            {"type": "photo", "url": "https://pbs.twimg.com/media/CCC.png?name=orig",
             "width": 3, "height": 4},
            "not a photo",
        ])));
        let pages = parse_pages(&b, URL);
        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0]["urls"]["original"],
            "https://pbs.twimg.com/media/CCC.png?name=orig"
        );
        // The page count and the first photo still count the unusable one (the string is not a
        // photo and counts for neither).
        assert_eq!(parse_page_count(&b, URL), 2);
        assert_eq!(parse_file_url(&b, URL), None);
        assert_eq!(parse_dimensions(&b, URL), (Some(1), Some(2)));
    }

    // test_a_tweet_with_no_photos_claims_nothing
    #[test]
    fn a_tweet_with_no_photos_claims_nothing() {
        let b = body(Some(json!([])));
        assert_eq!(parse_file_url(&b, URL), None);
        assert_eq!(parse_preview_url(&b, URL), None);
        assert_eq!(parse_dimensions(&b, URL), (None, None));
        assert!(parse_pages(&b, URL).is_empty());
        // the author is still worth having
        assert_eq!(parse(&b, URL).len(), 1);
    }

    // test_twitters_own_html_yields_nothing_without_raising
    #[test]
    fn twitters_own_html_yields_nothing() {
        for html in ["<!DOCTYPE html><html><body>JS-driven</body></html>", ""] {
            assert!(parse(html, URL).is_empty());
            assert_eq!(parse_file_url(html, URL), None);
            assert_eq!(parse_dimensions(html, URL), (None, None));
            assert_eq!(parse_page_count(html, URL), 1);
            assert!(parse_pages(html, URL).is_empty());
        }
    }

    #[test]
    fn dimensions_need_both_sides_and_read_what_int_reads() {
        let dims = |w: Value, h: Value| {
            parse_dimensions(
                &body(Some(
                    json!([{"url": "https://p/a", "width": w, "height": h}]),
                )),
                URL,
            )
        };
        assert_eq!(dims(json!("12"), json!(4.9)), (Some(12), Some(4)));
        assert_eq!(dims(json!(-3), json!(true)), (Some(-3), Some(1)));
        for odd in [json!(0), json!(null), json!("x"), json!([1]), json!("0")] {
            assert_eq!(dims(odd.clone(), json!(5)), (None, None), "{odd}");
        }
    }

    #[test]
    fn twitter_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_page_count",
                "parse_pages",
                "pages_api_url"
            ]
        );
    }
}
