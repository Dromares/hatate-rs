//! e621 and its mirrors, via the official public JSON API rather than by scraping the page
//! (`core/boorus/e621.py`).
//!
//! The API returns exact, already-categorised tag data plus the real file URL in one response,
//! which is both more reliable and simpler than guessing at DOM structure. There is no HTML
//! fallback: a body that is not JSON yields nothing.
//!
//! Every hook is a pure function of the body and the post-page URL. e621 keeps no state, so
//! nothing here takes a [`Context`](super::Context).
//!
//! Where Python would raise on a shape the API does not send (a `tags` that is a list, a `file`
//! that is a string), the hook has nothing to say here, which is what `fetch_page_info` makes of
//! a hook that raises. A tag name that is not a string, and a width that is not a JSON integer,
//! are likewise left out where Python would pass them through unchecked.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};
use tracing::{debug, warn};

use super::host::on_host;
use super::parsed::json_of;
use super::{SiteParser, rating};
use crate::model::{Tag, TagSource};
use crate::pycompat;

/// e926 is the SFW-only mirror, e6ai the AI-art one.
const HOSTS: [&str; 3] = ["e621.net", "e926.net", "e6ai.net"];

/// e621's own field names for each tag category, straight from its public JSON API response (a
/// post's "tags" object). "invalid" is intentionally excluded: those are tags pending cleanup or
/// removal, not descriptive ones.
const CATEGORY_FIELDS: [&str; 7] = [
    "general",
    "species",
    "character",
    "copyright",
    "artist",
    "lore",
    "meta",
];

static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/posts?/(?:show/)?(\d+)").expect("post id regex"));

/// e621's three ratings, as its API states them in the post's "rating" field. Unlike Danbooru,
/// "s" here means SAFE: e621 never split it into general/sensitive, so the two sites' letters
/// must not be shared.
const RATING_CODES: [(&str, &str); 3] = [("s", "safe"), ("q", "questionable"), ("e", "explicit")];

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    restriction_for_status: Some(|status, body, url, _| restriction_for_status(status, body, url)),
    incomplete_reason: Some(|body, url, _| incomplete_reason(body, url)),
    ..SiteParser::new("e621", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// Point the fetch at the JSON representation of the post, on the host the link came from.
pub fn resolve_fetch_url(url: &str) -> String {
    let Some(m) = POST_ID_RE.captures(url) else {
        debug!("Could not find a post ID in {url}, falling back to fetching it as-is");
        return url.to_string();
    };
    // Python's `h in url` is a substring test, as is this one.
    let host = HOSTS
        .iter()
        .find(|h| url.contains(**h))
        .unwrap_or(&HOSTS[0]);
    format!("https://{host}/posts/{}.json", &m[1])
}

/// The post record, or `None` when the body is not JSON or holds no post object.
///
/// The API nests the post under a "post" key; a bare post object is read too, in case that ever
/// changes.
fn load_post(body: &str, url: &str) -> Option<Map<String, Value>> {
    let data = match json_of(body) {
        Ok(data) => data,
        Err(_) => {
            warn!(
                "e621 response for {url} was not valid JSON (resolve_fetch_url may need updating)"
            );
            return None;
        }
    };
    let Value::Object(mut data) = data else {
        return None;
    };
    match data.remove("post") {
        Some(Value::Object(post)) => Some(post),
        Some(_) => None,
        None => Some(data),
    }
}

/// `post.get(key) or {}`: the nested object, an empty one when the key is absent or falsy, and
/// `None` where Python's next `.get` would raise (a truthy value that is not an object).
fn section<'a>(post: &'a Map<String, Value>, key: &str) -> Option<Option<&'a Map<String, Value>>> {
    match post.get(key) {
        None => Some(None),
        Some(Value::Object(map)) => Some(Some(map)),
        Some(other) if !pycompat::truthy(other) => Some(None),
        Some(_) => None,
    }
}

/// `(post.get(section) or {}).get(key)`.
fn section_value<'a>(
    post: &'a Map<String, Value>,
    section_key: &str,
    key: &str,
) -> Option<&'a Value> {
    section(post, section_key)??.get(key)
}

pub fn parse(body: &str, url: &str) -> Vec<Tag> {
    let Some(post) = load_post(body, url) else {
        return Vec::new();
    };
    let Some(Some(groups)) = section(&post, "tags") else {
        debug!("No tags object in e621 response for {url} (API shape may have changed)");
        return Vec::new();
    };
    if groups.is_empty() {
        debug!("No tags object in e621 response for {url} (API shape may have changed)");
        return Vec::new();
    }
    let mut tags = Vec::new();
    for category in CATEGORY_FIELDS {
        let Some(Value::Array(names)) = groups.get(category) else {
            continue;
        };
        for name in names.iter().filter_map(Value::as_str) {
            tags.push(Tag {
                name: name.to_string(),
                namespace: Some(category.to_string()),
                source: TagSource::Booru,
            });
        }
    }
    tags
}

/// The original file, or `None` where e621 withholds it (`"url": null`).
pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    section_value(&post, "file", "url")?
        .as_str()
        .map(str::to_string)
}

/// The best picture to show for this post without downloading more than needed. Reuses the JSON
/// body already fetched for tags and the file URL.
///
/// e621 offers three: a `sample` (mid-resolution), the `file` (the original), and a `preview`
/// (a ~150px thumbnail).
///
/// `sample` is the one to want, but it only EXISTS when the original was big enough to be worth
/// downscaling. For a smaller post e621 sends `{"has": false, "url": null}`, and returning that
/// null meant the app fell back to the search engine's tiny thumbnail and showed a visibly
/// blurry match for exactly the posts whose original was small enough to have shown in full. So
/// the file itself is the fallback, which by definition is only reached when it is already
/// modest in size.
///
/// The 150px preview is last, for a post whose file URL is withheld. A relative or otherwise
/// scheme-less URL is skipped: nothing downstream can fetch it.
pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    for key in ["sample", "file", "preview"] {
        // `?`: a section that is not an object ends the search, as Python's `.get` raising does.
        if let Some(Value::String(candidate)) = section(&post, key)?.and_then(|s| s.get("url"))
            && (candidate.starts_with("http://") || candidate.starts_with("https://"))
        {
            return Some(candidate.clone());
        }
    }
    None
}

/// The post's rating, from the API's own "rating" field.
pub fn parse_rating(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    rating::from_code(post.get("rating").unwrap_or(&Value::Null), &RATING_CODES)
}

/// The API reports the post's real width and height directly in its "file" object.
pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let Some(post) = load_post(body, url) else {
        return (None, None);
    };
    let int = |key: &str| section_value(&post, "file", key).and_then(Value::as_i64);
    (int("width"), int("height"))
}

/// Why a 401 means "your credentials are wrong", not "the fetch failed".
///
/// e621 answers 401 to EVERY request carrying a bad username or API key, including ones that
/// would have worked anonymously. So a typo in the settings does not degrade e621, it breaks it
/// completely, and without this the user would see a generic failed fetch on every single post
/// with nothing pointing at the credentials they just entered.
///
/// Reported rather than raised so the match survives: the post is fine, and clearing the
/// credentials would bring it straight back.
pub fn restriction_for_status(status_code: u16, _body: &str, _url: &str) -> Option<String> {
    (status_code == 401).then(|| {
        "e621 rejected the API credentials - check the username and API key in \
         Settings > Sites (Account > Manage API Access on e621), or clear them to \
         browse anonymously"
            .to_string()
    })
}

/// Why an e621 post came back with no file to show.
///
/// e621 returns the post either way and simply nulls `file.url`, so the two reasons for that
/// look identical from here and need telling apart, because only one of them is worth acting on.
///
/// A DELETED post is the common one, and measured: sampling 60 of this app's own e621 matches
/// found exactly one withheld file, and it was deleted. No account unlocks those, so pointing at
/// the sign-in setting would send someone after a fix that cannot work.
///
/// Anything else withheld is the case an account may actually cover, and the message says "may"
/// because e621 also gates on per-account content settings rather than on the credentials alone.
pub fn incomplete_reason(body: &str, url: &str) -> Option<String> {
    let post = load_post(body, url)?;
    if section_value(&post, "file", "url").is_some_and(pycompat::truthy) {
        return None; // the post is fully available; it simply has no tags
    }
    // Python raises on a `file` or `flags` that is not an object, and then has no reason.
    section(&post, "file")?;
    section(&post, "flags")?;
    if section_value(&post, "flags", "deleted").is_some_and(pycompat::truthy) {
        return Some(
            "This e621 post has been deleted - e621 still describes it but withholds \
             the file, so there is no picture to preview or download. Signing in does \
             not bring a deleted post back."
                .to_string(),
        );
    }
    Some(
        "e621 is withholding this post's file. An account may show it - add an e621 \
         username and API key in Settings > Sites, and check the content settings on \
         your e621 account - though some posts are withheld from everyone."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://e621.net/posts/1";

    fn names(tags: &[Tag]) -> Vec<(&str, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref().unwrap(), t.name.as_str()))
            .collect()
    }

    #[test]
    fn claims_e621_and_its_mirrors_only() {
        assert!(matches("https://e621.net/posts/219935"));
        assert!(matches("https://e926.net/posts/1"));
        assert!(matches("https://e6ai.net/posts/1"));
        assert!(!matches("https://danbooru.donmai.us/posts/1"));
        assert!(!matches("https://example.com/e621.net/posts/1"));
    }

    #[test]
    fn the_fetch_goes_to_the_json_api_on_the_mirror_the_link_came_from() {
        assert_eq!(
            resolve_fetch_url("https://e621.net/posts/219935?q=x"),
            "https://e621.net/posts/219935.json"
        );
        assert_eq!(
            resolve_fetch_url("https://e926.net/posts/7"),
            "https://e926.net/posts/7.json"
        );
        assert_eq!(
            resolve_fetch_url("https://e6ai.net/post/show/7/tag"),
            "https://e6ai.net/posts/7.json"
        );
        // Python's `h in url`: the host only has to appear somewhere, and the first of the
        // list that does wins.
        assert_eq!(
            resolve_fetch_url("https://mirror.example/e926.net/posts/9"),
            "https://e926.net/posts/9.json"
        );
        assert_eq!(
            resolve_fetch_url("https://mirror.example/posts/9"),
            "https://e621.net/posts/9.json"
        );
        // No post ID to ask the API about: the page itself is fetched.
        assert_eq!(
            resolve_fetch_url("https://e621.net/artists"),
            "https://e621.net/artists"
        );
    }

    #[test]
    fn tags_come_out_in_category_order_and_invalid_is_dropped() {
        let body = r#"{"post": {"tags": {"meta": ["m"], "general": ["a", "b"], "invalid": ["x"],
                       "artist": ["z"], "species": ["s"], "lore": [], "unknown": ["q"]}}}"#;
        assert_eq!(
            names(&parse(body, URL)),
            [
                ("general", "a"),
                ("general", "b"),
                ("species", "s"),
                ("artist", "z"),
                ("meta", "m")
            ]
        );
    }

    #[test]
    fn a_bare_post_object_is_read_like_a_nested_one() {
        let bare = r#"{"id": 1, "file": {"url": "https://x/y.jpg", "width": 3, "height": 4},
                       "tags": {"general": ["a"]}, "rating": "s"}"#;
        assert_eq!(names(&parse(bare, URL)), [("general", "a")]);
        assert_eq!(
            parse_file_url(bare, URL).as_deref(),
            Some("https://x/y.jpg")
        );
        assert_eq!(parse_dimensions(bare, URL), (Some(3), Some(4)));
        assert_eq!(parse_rating(bare, URL).as_deref(), Some("safe"));
    }

    #[test]
    fn a_body_that_is_not_a_post_says_nothing() {
        for body in [
            "not json",
            "",
            "[1]",
            "null",
            r#"{"post": null}"#,
            r#"{"post": "x"}"#,
            r#"{"success": false, "reason": "x"}"#,
        ] {
            assert!(parse(body, URL).is_empty(), "{body}");
            assert_eq!(parse_file_url(body, URL), None, "{body}");
            assert_eq!(parse_preview_url(body, URL), None, "{body}");
            assert_eq!(parse_rating(body, URL), None, "{body}");
            assert_eq!(parse_dimensions(body, URL), (None, None), "{body}");
        }
        for body in [
            "not json",
            "[1]",
            r#"{"post": null}"#,
            r#"{"post": {"file": "x"}}"#,
            r#"{"post": {"file": {}, "flags": [1]}}"#,
        ] {
            assert_eq!(incomplete_reason(body, URL), None, "{body}");
        }
    }

    #[test]
    fn a_tags_value_that_is_not_an_object_of_lists_gives_no_tags() {
        for body in [
            r#"{"post": {"tags": {}}}"#,
            r#"{"post": {"tags": null}}"#,
            r#"{"post": {"tags": []}}"#,
            r#"{"post": {"tags": "general"}}"#,
            r#"{"post": {"tags": {"general": "a b"}}}"#,
            r#"{"post": {"tags": {"general": null}}}"#,
            r#"{"post": {"tags": {"general": [1, null]}}}"#,
        ] {
            assert!(parse(body, URL).is_empty(), "{body}");
        }
    }

    #[test]
    fn the_file_url_is_the_one_in_the_file_object() {
        let url_of =
            |file: &str| parse_file_url(&format!(r#"{{"post": {{"file": {file}}}}}"#), URL);
        assert_eq!(
            url_of(r#"{"url": "https://x/f.jpg"}"#).as_deref(),
            Some("https://x/f.jpg")
        );
        // A relative one is returned as it is; only the preview insists on a scheme.
        assert_eq!(
            url_of(r#"{"url": "/data/f.jpg"}"#).as_deref(),
            Some("/data/f.jpg")
        );
        assert_eq!(url_of(r#"{"url": ""}"#).as_deref(), Some(""));
        assert_eq!(url_of(r#"{"url": null}"#), None);
        assert_eq!(url_of("{}"), None);
        assert_eq!(url_of("null"), None);
        assert_eq!(url_of(r#""f""#), None);
    }

    #[test]
    fn the_preview_prefers_sample_then_file_then_the_thumbnail() {
        let body = |sample: &str, file: &str, preview: &str| {
            format!(
                r#"{{"post": {{"sample": {{"url": {sample}}}, "file": {{"url": {file}}},
                    "preview": {{"url": {preview}}}}}}}"#
            )
        };
        let all = body(r#""https://s""#, r#""https://f""#, r#""https://p""#);
        assert_eq!(parse_preview_url(&all, URL).as_deref(), Some("https://s"));
        let no_sample = body("null", r#""http://f""#, r#""https://p""#);
        assert_eq!(
            parse_preview_url(&no_sample, URL).as_deref(),
            Some("http://f")
        );
        let withheld = body("null", "null", r#""https://p""#);
        assert_eq!(
            parse_preview_url(&withheld, URL).as_deref(),
            Some("https://p")
        );
        let nothing = body("null", "null", "null");
        assert_eq!(parse_preview_url(&nothing, URL), None);
        // Relative, scheme-less and non-string URLs are not offered, and the search goes on.
        let junk = body(r#""/data/s.jpg""#, "7", r#""ftp://p""#);
        assert_eq!(parse_preview_url(&junk, URL), None);
        let skip_to_file = body(r#""//s""#, r#""https://f""#, "null");
        assert_eq!(
            parse_preview_url(&skip_to_file, URL).as_deref(),
            Some("https://f")
        );
        // A section that is not an object ends the search, as Python's `.get` raising does.
        let raising = r#"{"post": {"sample": "x", "file": {"url": "https://f"}}}"#;
        assert_eq!(parse_preview_url(raising, URL), None);
        let absent = r#"{"post": {"sample": null, "file": {"url": "https://f"}}}"#;
        assert_eq!(parse_preview_url(absent, URL).as_deref(), Some("https://f"));
    }

    #[test]
    fn the_rating_goes_through_e621s_own_table() {
        let rating = |r: &str| parse_rating(&format!(r#"{{"post": {{"rating": {r}}}}}"#), URL);
        assert_eq!(rating(r#""s""#).as_deref(), Some("safe"));
        assert_eq!(rating(r#""q""#).as_deref(), Some("questionable"));
        assert_eq!(rating(r#"" E ""#).as_deref(), Some("explicit"));
        assert_eq!(rating(r#""Safe""#).as_deref(), Some("safe"));
        // Danbooru's words are not e621's.
        assert_eq!(rating(r#""g""#), None);
        assert_eq!(rating(r#""sensitive""#), None);
        assert_eq!(rating("null"), None);
        assert_eq!(parse_rating(r#"{"post": {}}"#, URL), None);
    }

    #[test]
    fn dimensions_are_the_files_not_the_samples() {
        let body = r#"{"post": {"file": {"width": 664, "height": 909},
                       "sample": {"width": 1, "height": 2}}}"#;
        assert_eq!(parse_dimensions(body, URL), (Some(664), Some(909)));
        assert_eq!(parse_dimensions(r#"{"post": {}}"#, URL), (None, None));
        assert_eq!(
            parse_dimensions(r#"{"post": {"file": {"width": "9", "height": 4}}}"#, URL),
            (None, Some(4))
        );
    }

    #[test]
    fn only_a_401_is_blamed_on_the_credentials() {
        let reason = restriction_for_status(401, "", URL).unwrap();
        assert!(reason.starts_with("e621 rejected the API credentials"));
        for status in [200, 403, 404, 429, 500] {
            assert_eq!(restriction_for_status(status, "", URL), None, "{status}");
        }
    }

    #[test]
    fn a_missing_file_is_explained_as_deleted_or_withheld() {
        let reason = |post: &str| incomplete_reason(&format!(r#"{{"post": {post}}}"#), URL);
        // Fully available: no excuse needed.
        assert_eq!(reason(r#"{"file": {"url": "https://x/y.jpg"}}"#), None);
        // A deleted post is not blamed on being logged out.
        let deleted = reason(r#"{"file": {"url": null}, "flags": {"deleted": true}}"#).unwrap();
        assert!(deleted.starts_with("This e621 post has been deleted"));
        assert!(deleted.ends_with("Signing in does not bring a deleted post back."));
        // A live one is pointed at the account.
        for post in [
            r#"{"file": {"url": null}, "flags": {"deleted": false}}"#,
            r#"{"file": {"url": null}}"#,
            r#"{"file": {"url": ""}, "flags": null}"#,
            r#"{}"#,
        ] {
            let withheld = reason(post).unwrap();
            assert!(
                withheld.starts_with("e621 is withholding this post's file"),
                "{post}"
            );
        }
    }

    #[test]
    fn e621_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_rating",
                "restriction_for_status",
                "incomplete_reason",
            ]
        );
        assert!(!PARSER.empty_result_is_normal);
    }
}
