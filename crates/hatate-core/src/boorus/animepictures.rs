//! Anime-Pictures, via its official JSON API (`core/boorus/animepictures.py`).
//!
//! The HTML site is JS-rendered, so the page itself carries no tags. The JSON at
//! `api.anime-pictures.net/api/v3/posts/{id}` carries everything useful, but its shape is not the
//! obvious one:
//!
//! ```text
//! {
//!   "post":  {"id":…, "md5":…, "width":…, "height":…,
//!             "big_preview":…, "medium_preview":…, "small_preview":…},
//!   "tags":  [ {"tag": {"tag": "name", "type": 4}, …}, … ],
//!   "file_url": "596771-5728x4000-original-….jpeg"
//! }
//! ```
//!
//! Three traps, all of which the Python parser has been bitten by:
//!
//! - `tags` sits at the TOP level, beside `post` rather than inside it, and each entry WRAPS the
//!   tag in another `tag` key.
//! - the top-level `file_url` is a download *filename*, not a URL.
//! - the full-resolution image is not reachable anonymously at all. A constructed CDN path
//!   answers 302 to the site's front page, which would hand back an HTML page for the app to
//!   store as if it were an image, so [`parse_file_url`] returns `None` unless the response
//!   itself holds an absolute URL, and callers fall back to the preview.
//!
//! There is no `parse_rating`, on purpose: the response has no rating key, and the one
//! rating-shaped field, `erotics`, is an intensity level with no stated labels. Turning it into
//! a rating would mean inventing both how many levels exist and what each is called.
//!
//! Every hook is a pure function of the response body and the post-page URL; none keeps state.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};
use tracing::debug;

use super::SiteParser;
use super::host::on_host;
use super::json_fields::{absolute_url, dimensions, size_bytes, tag_type_id};
use super::parsed::json_of;
use crate::model::{Tag, TagSource};
use crate::pycompat;
use crate::pyrepr::py_str;

const HOST: &str = "anime-pictures.net";
const API_HOST: &str = "https://api.anime-pictures.net";

/// Both URL forms have to match. Search engines hand us the legacy `/pictures/view_post/{id}`,
/// which the site 301s to `/posts/{id}`; matching only the modern form meant the API was never
/// used and the JS-rendered HTML page got parsed instead, yielding no tags and no picture.
static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(?:pictures/view_post|posts)/(\d+)").expect("post id regex"));

/// Verified against live posts rather than guessed: 1 names characters, 4 the artist, 5 a game
/// and 6 an anime/"original", both of which are copyright in Hydrus terms. 2 and 7 are
/// descriptive ("long hair", "girl", "highres"), so they stay in the default namespace.
/// Anything unrecognised falls through to general rather than being dropped.
const TAG_TYPES: [(i64, &str); 4] = [
    (1, "character"),
    (4, "artist"), // artist -> creator happens later via the user's remap
    (5, "copyright"),
    (6, "copyright"),
];
const DEFAULT_NAMESPACE: &str = "general";

/// The API states the original's extension; these are the same names `fetch_remote_info`
/// derives from a Content-Type, so the UI shows one vocabulary whichever source a match's
/// format came from.
const EXTENSION_FORMATS: [(&str, &str); 9] = [
    ("jpg", "JPEG"),
    ("jpeg", "JPEG"),
    ("png", "PNG"),
    ("gif", "GIF"),
    ("webp", "WEBP"),
    ("bmp", "BMP"),
    ("avif", "AVIF"),
    ("webm", "WEBM"),
    ("mp4", "MP4"),
];

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_file_info: Some(|body, url, _| parse_file_info(body, url)),
    restriction_for_status: Some(|status, body, url, _| restriction_for_status(status, body, url)),
    ..SiteParser::new("animepictures", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &[HOST])
}

pub fn resolve_fetch_url(url: &str) -> String {
    match POST_ID_RE.captures(url) {
        Some(m) => format!("{API_HOST}/api/v3/posts/{}", &m[1]),
        None => url.to_string(),
    }
}

pub fn parse(body: &str, _url: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    for raw in tag_entries(body) {
        let Some(raw) = raw.as_object() else { continue };
        // Normally {"tag": {"tag": "name", "type": 4}}; tolerate a flat
        // {"tag": "name", "type": 4} in case the API is ever simplified.
        let info = raw.get("tag").and_then(Value::as_object).unwrap_or(raw);
        let Some(name) = info.get("tag").and_then(Value::as_str) else {
            continue;
        };
        let name = pycompat::strip(name).replace(' ', "_");
        if name.is_empty() {
            continue;
        }
        let type_id = tag_type_id(info.get("type"));
        let namespace = TAG_TYPES
            .iter()
            .find(|(id, _)| *id == type_id)
            .map_or(DEFAULT_NAMESPACE, |(_, ns)| ns);
        tags.push(Tag {
            name,
            namespace: Some(namespace.to_string()),
            source: TagSource::Booru,
        });
    }
    tags
}

/// The full image, or `None` when there isn't one we can actually get.
///
/// Only an absolute http(s) URL counts. The API's own `file_url` is a filename, and every
/// constructed CDN path tried so far answers with the site's front page: returning either would
/// have the app download HTML and treat it as the matched image.
pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    holders(body).iter().find_map(|holder| {
        ["file_url", "url"]
            .iter()
            .find_map(|key| absolute_url(holder.get(*key)))
    })
}

pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    let post = post_of(body)?;
    ["big_preview", "medium_preview", "small_preview"]
        .iter()
        .find_map(|key| absolute_url(post.get(*key)))
}

/// Why a non-200 means "sign in", rather than "the fetch failed".
///
/// Account-only posts answer the API with `403 {"errormsg": "Forbidden", "success": false}`,
/// which carries none of the post: no tags, no dimensions, not even a preview. Saying so turns a
/// blank match into one the user can act on, since adding cookies for a logged-in account does
/// fill it in.
///
/// Deliberately narrow. A 403 with a Cloudflare challenge body is a bot check that a login would
/// not fix, and this site does throw those occasionally, so only the API's own JSON error counts.
pub fn restriction_for_status(status_code: u16, body: &str, _url: &str) -> Option<String> {
    if status_code != 403 {
        return None;
    }
    let data = json_of(body).ok()?; // an HTML 403 is a bot check, not an account gate
    let data = data.as_object()?;
    if data.get("success") != Some(&Value::Bool(false)) {
        return None;
    }
    let reason = match data.get("errormsg").filter(|v| pycompat::truthy(v)) {
        Some(message) => pycompat::strip(&py_str(message)).to_string(),
        None => "Forbidden".to_string(),
    };
    Some(format!(
        "Anime-Pictures returned {reason} for this post - it is account-only. \
         Add cookies from a logged-in account under Settings > Site Logins to see it."
    ))
}

/// `(format, size_in_bytes)` for the original file, straight from the API.
///
/// Normally these come from a HEAD on the direct file URL, but this site has none we can reach,
/// so that HEAD would fall back to measuring the search engine's THUMBNAIL: a few KB and the
/// thumbnail's format reported as though they described the match. The API states both outright
/// for the original, which is better than a HEAD anyway: it is authoritative and costs no extra
/// request.
pub fn parse_file_info(body: &str, _url: &str) -> (Option<String>, Option<i64>) {
    let Some(post) = post_of(body) else {
        return (None, None);
    };
    let format = post
        .get("ext")
        .and_then(Value::as_str)
        .map(pycompat::strip)
        .filter(|ext| !ext.is_empty())
        .and_then(|ext| {
            let ext = ext.to_lowercase();
            let ext = ext.trim_start_matches('.');
            EXTENSION_FORMATS
                .iter()
                .find(|(known, _)| *known == ext)
                .map(|(_, name)| (*name).to_string())
        });
    (format, size_bytes(post.get("size")))
}

pub fn parse_dimensions(body: &str, _url: &str) -> (Option<i64>, Option<i64>) {
    post_of(body).map_or((None, None), |post| dimensions(&post))
}

/// The response as an object. Anything else (the JS-rendered HTML page, an array, an empty body)
/// is "not the v3 API", most likely because `resolve_fetch_url` did not match a post id.
fn document_of(body: &str) -> Option<Map<String, Value>> {
    if body.is_empty() {
        return None;
    }
    match json_of(body) {
        Ok(Value::Object(document)) => Some(document),
        Ok(_) => None,
        Err(_) => {
            debug!(
                "Anime-Pictures response was not JSON - expected the v3 API, so this is most \
                 likely the JS-rendered HTML page (check resolve_fetch_url matched the post id \
                 in the URL)"
            );
            None
        }
    }
}

/// The document and its `post`, in that order. Fields have moved between the two across API
/// revisions, so look in both rather than committing to one and silently returning nothing.
fn holders(body: &str) -> Vec<Map<String, Value>> {
    let Some(mut document) = document_of(body) else {
        return Vec::new();
    };
    match document.get("post").and_then(Value::as_object).cloned() {
        Some(post) => {
            document.remove("post");
            vec![document, post]
        }
        None => vec![document],
    }
}

/// The `post` sub-object when it looks like a post, else the document when it does.
fn post_of(body: &str) -> Option<Map<String, Value>> {
    holders(body)
        .into_iter()
        .rev() // the "post" sub-object first
        .find(|holder| {
            ["id", "md5", "width"]
                .iter()
                .any(|key| holder.contains_key(*key))
        })
}

fn tag_entries(body: &str) -> Vec<Value> {
    holders(body)
        .iter()
        .find_map(|holder| holder.get("tags").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://anime-pictures.net/posts/123";

    #[test]
    fn both_url_forms_resolve_to_the_api_and_anything_else_to_itself() {
        let api = "https://api.anime-pictures.net/api/v3/posts/123";
        assert_eq!(
            resolve_fetch_url("https://anime-pictures.net/posts/123"),
            api
        );
        assert_eq!(
            resolve_fetch_url("https://anime-pictures.net/pictures/view_post/123?lang=en"),
            api
        );
        let other = "https://anime-pictures.net/pictures/view_posts/";
        assert_eq!(resolve_fetch_url(other), other);
        assert_eq!(resolve_fetch_url(""), "");
    }

    #[test]
    fn defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_file_info",
                "restriction_for_status",
            ]
        );
        assert!(PARSER.parse_rating.is_none());
        assert!(matches("https://anime-pictures.net/"));
        assert!(!matches("https://api.anime-pictures.net.evil.example/"));
    }

    #[test]
    fn tags_are_read_from_the_top_level_wrapped_or_flat() {
        let body = r#"{"post": {"id": 1, "tags": [{"tag": {"tag": "ignored"}}]},
            "tags": [
              {"tag": {"tag": "long hair", "type": 2}},
              {"tag": {"tag": " artist x ", "type": 4}},
              {"tag": {"tag": "game", "type": "5"}},
              {"tag": {"tag": "anime", "type": 6}},
              {"tag": {"tag": "girl_char", "type": 1}},
              {"tag": "flat", "type": 4},
              {"tag": {"tag": "   "}}, {"tag": {"tag": 7}}, {"tag": {"type": 1}}, "junk"
            ]}"#;
        let got: Vec<_> = parse(body, URL)
            .into_iter()
            .map(|t| (t.name, t.namespace.unwrap()))
            .collect();
        let want = [
            ("long_hair", "general"),
            ("artist_x", "artist"),
            ("game", "copyright"),
            ("anime", "copyright"),
            ("girl_char", "character"),
            ("flat", "artist"),
        ]
        .map(|(n, ns)| (n.to_string(), ns.to_string()));
        assert_eq!(got, want);
    }

    #[test]
    fn the_first_tags_list_wins_even_when_it_is_empty() {
        let body = r#"{"tags": [], "post": {"tags": [{"tag": {"tag": "x"}}]}}"#;
        assert!(parse(body, URL).is_empty());
        let in_post = r#"{"post": {"id": 1, "tags": [{"tag": {"tag": "x"}}]}}"#;
        assert_eq!(parse(in_post, URL).len(), 1);
        for miss in ["", "<html>", "[]", "null", r#"{"tags": {"a": 1}}"#] {
            assert!(parse(miss, URL).is_empty(), "{miss:?}");
        }
    }

    #[test]
    fn a_filename_is_not_a_file_url_but_an_absolute_one_is() {
        let filename = r#"{"post": {"id": 1}, "file_url": "596771-5728x4000-original.jpeg"}"#;
        assert_eq!(parse_file_url(filename, URL), None);
        let absolute =
            r#"{"post": {"id": 1, "file_url": "https://cdn/f.png"}, "file_url": "f.png"}"#;
        assert_eq!(
            parse_file_url(absolute, URL).as_deref(),
            Some("https://cdn/f.png")
        );
        // The document is searched before the post, `file_url` before `url`.
        let both = r#"{"post": {"url": "http://p/1"}, "url": "https://d/1", "file_url": "x"}"#;
        assert_eq!(parse_file_url(both, URL).as_deref(), Some("https://d/1"));
        assert_eq!(parse_file_url("<html>", URL), None);
    }

    #[test]
    fn the_preview_is_the_biggest_absolute_one() {
        let preview = |body| parse_preview_url(body, URL);
        let body = r#"{"post": {"id": 1, "big_preview": "b", "medium_preview": "https://m/1",
                                "small_preview": "https://s/1"}}"#;
        assert_eq!(preview(body).as_deref(), Some("https://m/1"));
        assert_eq!(preview(r#"{"post": {"id": 1}}"#), None);
        // A document with no `post` object is itself the post, if it looks like one.
        let flat = r#"{"id": 1, "small_preview": "http://s/1"}"#;
        assert_eq!(preview(flat).as_deref(), Some("http://s/1"));
        assert_eq!(preview(r#"{"small_preview": "http://s/1"}"#), None);
    }

    #[test]
    fn dimensions_and_file_info_come_from_the_post_object() {
        let body = r#"{"post": {"id": 1, "width": 5728, "height": 4000, "ext": " .JPG ",
                                "size": "12345"}}"#;
        assert_eq!(parse_dimensions(body, URL), (Some(5728), Some(4000)));
        assert_eq!(
            parse_file_info(body, URL),
            (Some("JPEG".into()), Some(12345))
        );
        let odd = r#"{"post": {"id": 1, "width": 1, "ext": "tiff", "size": 0}}"#;
        assert_eq!(parse_dimensions(odd, URL), (None, None));
        assert_eq!(parse_file_info(odd, URL), (None, None));
        for none in ["", "<html>", "{}", r#"{"post": {"name": "x"}}"#] {
            assert_eq!(parse_dimensions(none, URL), (None, None), "{none:?}");
            assert_eq!(parse_file_info(none, URL), (None, None), "{none:?}");
        }
    }

    #[test]
    fn only_the_apis_own_403_means_account_only() {
        let gate = |status, body| restriction_for_status(status, body, URL);
        let forbidden = r#"{"errormsg": "Forbidden", "success": false}"#;
        let reason = gate(403, forbidden).unwrap();
        assert!(
            reason.starts_with("Anime-Pictures returned Forbidden for this post"),
            "{reason}"
        );
        assert!(
            reason.ends_with("Settings > Site Logins to see it."),
            "{reason}"
        );
        assert!(
            gate(403, r#"{"errormsg": " Nope ", "success": false}"#)
                .unwrap()
                .contains("returned Nope for")
        );
        // No message, or a falsy one, is the generic word; a non-string one prints as Python would.
        assert!(
            gate(403, r#"{"success": false}"#)
                .unwrap()
                .contains("returned Forbidden for")
        );
        assert!(
            gate(403, r#"{"errormsg": 0, "success": false}"#)
                .unwrap()
                .contains("returned Forbidden for")
        );
        assert!(
            gate(403, r#"{"errormsg": ["it's"], "success": false}"#)
                .unwrap()
                .contains(r#"returned ["it's"] for"#)
        );
        for (status, body) in [
            (200, forbidden),
            (404, forbidden),
            (403, "<html>Just a moment...</html>"),
            (403, r#"{"errormsg": "Forbidden", "success": true}"#),
            (403, r#"{"errormsg": "Forbidden"}"#),
            (403, r#"["success", false]"#),
        ] {
            assert_eq!(gate(status, body), None, "{status} {body}");
        }
    }
}
