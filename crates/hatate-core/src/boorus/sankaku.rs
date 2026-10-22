//! Sankaku Complex, via its modern API, looked up by file hash (`core/boorus/sankaku.py`).
//!
//! Sankaku's old numeric post ids are the only ones IQDB and SauceNAO know, and they no longer
//! lead anywhere. Verified against the live site when the Python was written:
//!
//! - `chan.sankakucomplex.com` bounces EVERY post URL to an OIDC login page, so a redirect there
//!   proves nothing about whether a post exists. The app used to read that bounce as "deleted"
//!   and drop the match, which is why Sankaku matches kept disappearing.
//! - The modern API answers anonymously and carries everything worth having (tags with types,
//!   dimensions, byte size, format) but is keyed by new-style ids like `QyMk8vZ6Kak`. A numeric
//!   id returns `{"code": "invalid id"}`, and no legacy-to-modern redirect service exists.
//!
//! So the only key left is the FILE ITSELF: Sankaku indexes by md5, and the local file's hash
//! finds the post whenever the copy is byte-identical to Sankaku's. When it misses, nothing is
//! claimed. That is also why the match gets repointed ([`parse_canonical_url`]): the modern URL
//! is one the user can open, where the legacy one is a login wall.
//!
//! Every hook is a pure function of the response body and the post-page URL (which none of them
//! reads); the hash comes in through [`FetchContext`] for [`resolve_fetch_url_with_context`].

use serde_json::{Map, Value};
use tracing::debug;

use super::host::on_host;
use super::json_fields::{absolute_url, dimensions, size_bytes, tag_type_id};
use super::parsed::json_of;
use super::{FetchContext, SiteParser, rating};
use crate::model::{Tag, TagSource};
use crate::pycompat;

const HOSTS: [&str; 2] = ["sankakucomplex.com", "sankaku.app"];
const API_ENDPOINT: &str = "https://sankakuapi.com/posts";
const POST_URL_BASE: &str = "https://www.sankakucomplex.com/posts";

/// Verified against live posts: 1 is the artist, 3 the copyright, 4 the character, 8 and 9
/// housekeeping. 0 and 5 are both descriptive (5 is Sankaku's "genre", a content descriptor and
/// not a namespace this app has anywhere else), so both stay in the default namespace.
const TAG_TYPES: [(i64, &str); 5] = [
    (1, "artist"), // artist -> creator happens later via the user's remap
    (3, "copyright"),
    (4, "character"),
    (8, "meta"),
    (9, "meta"),
];
const DEFAULT_NAMESPACE: &str = "general";

/// Sankaku's own rating codes. Danbooru 1.x lineage, where "s" is SAFE and not Danbooru 2's
/// "sensitive". "g" is an input alias of the search, not a fourth stored rating.
const RATING_CODES: [(&str, &str); 3] = [("s", "safe"), ("q", "questionable"), ("e", "explicit")];

const FILE_TYPE_FORMATS: [(&str, &str); 8] = [
    ("image/jpeg", "JPEG"),
    ("image/jpg", "JPEG"),
    ("image/png", "PNG"),
    ("image/gif", "GIF"),
    ("image/webp", "WEBP"),
    ("image/avif", "AVIF"),
    ("video/mp4", "MP4"),
    ("video/webm", "WEBM"),
];

/// What the registry knows about this site: one entry per hook the module defines. A hash miss
/// (the local file is not Sankaku's copy) is the ordinary outcome here, not a sign the site
/// changed, so an empty result is not worth a "markup may have changed" warning.
pub static PARSER: SiteParser = SiteParser {
    empty_result_is_normal: true,
    resolve_fetch_url_with_context: Some(resolve_fetch_url_with_context),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    parse_file_info: Some(|body, url, _| parse_file_info(body, url)),
    parse_canonical_url: Some(|body, url, _| parse_canonical_url(body, url)),
    incomplete_reason: Some(|body, url, _| incomplete_reason(body, url)),
    ..SiteParser::new("sankaku", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// `^[0-9a-f]{32}$`. Python's `$` also matches in front of one trailing newline, which then goes
/// into the query as `%0A`; that is kept, so the URL is the one Python builds.
fn is_md5(candidate: &str) -> bool {
    let hex = candidate.strip_suffix('\n').unwrap_or(candidate);
    hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Where to look this post up, given the file being searched.
///
/// Returns the original URL when there is no hash to search by. That fetches the legacy page,
/// which yields nothing: the same as before, rather than a new kind of failure.
pub fn resolve_fetch_url_with_context(url: &str, context: &FetchContext) -> String {
    let Some(md5) = context.local_md5.as_deref().filter(|m| is_md5(m)) else {
        debug!("No local md5 available - Sankaku's own id cannot be resolved, skipping");
        return url.to_string();
    };
    // `urlencode` of `tags=md5:<hash>&limit=1`: the colon is escaped, the hash is hex.
    let hash = md5.to_lowercase().replace('\n', "%0A");
    format!("{API_ENDPOINT}?tags=md5%3A{hash}&limit=1")
}

pub fn parse(body: &str, _url: &str) -> Vec<Tag> {
    let Some(post) = post_of(body) else {
        return Vec::new();
    };
    let Some(Value::Array(entries)) = post.get("tags").filter(|v| pycompat::truthy(v)) else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    for raw in entries {
        let Some(raw) = raw.as_object() else { continue };
        // `name_en or name`: a truthy `name_en` that is not a string ends the entry, it does
        // not fall back to `name`.
        let name = raw
            .get("name_en")
            .filter(|v| pycompat::truthy(v))
            .or_else(|| raw.get("name"));
        let Some(name) = name.and_then(Value::as_str).map(pycompat::strip) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let type_id = tag_type_id(raw.get("type"));
        let namespace = TAG_TYPES
            .iter()
            .find(|(id, _)| *id == type_id)
            .map_or(DEFAULT_NAMESPACE, |(_, ns)| ns);
        tags.push(Tag {
            name: name.replace(' ', "_"),
            namespace: Some(namespace.to_string()),
            source: TagSource::Booru,
        });
    }
    tags
}

/// The modern post URL, now that the hash has identified the post.
///
/// The legacy numeric URL the search was handed is a login wall for everyone, so leaving the
/// match pointing at it hands the user a dead link and associates a dead link with the file in
/// Hydrus.
pub fn parse_canonical_url(body: &str, _url: &str) -> Option<String> {
    let post = post_of(body)?;
    let id = post.get("id")?.as_str().filter(|id| !id.is_empty())?;
    Some(format!("{POST_URL_BASE}/{id}"))
}

pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    absolute_url(post_of(body)?.get("file_url"))
}

/// The sample is a proper mid-size render; the "preview" is a tiny thumbnail, so it is only the
/// fallback.
pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    let post = post_of(body)?;
    absolute_url(post.get("sample_url")).or_else(|| absolute_url(post.get("preview_url")))
}

/// The post's rating, from the API's own `rating` field, through Sankaku's own table.
///
/// Only the API path can answer: the legacy page this parser is otherwise handed is an OIDC login
/// wall, so there is no markup to read a rating off even in principle. A hash miss returns no
/// post and therefore no rating, which is the ordinary outcome here rather than a fault.
pub fn parse_rating(body: &str, _url: &str) -> Option<String> {
    let post = post_of(body)?;
    rating::from_code(post.get("rating").unwrap_or(&Value::Null), &RATING_CODES)
}

pub fn parse_dimensions(body: &str, _url: &str) -> (Option<i64>, Option<i64>) {
    match post_of(body) {
        Some(post) if !post.is_empty() => dimensions(&post),
        _ => (None, None),
    }
}

/// `(format, size)` as the API states them for the original.
///
/// Worth taking from the API rather than a HEAD: Sankaku's file URLs are signed and time-limited,
/// so a HEAD on a stale one would report nothing and leave the match looking sizeless.
pub fn parse_file_info(body: &str, _url: &str) -> (Option<String>, Option<i64>) {
    let Some(post) = post_of(body).filter(|post| !post.is_empty()) else {
        return (None, None);
    };
    let format = post
        .get("file_type")
        .and_then(Value::as_str)
        .and_then(|file_type| {
            let mime = pycompat::strip(file_type.split(';').next().unwrap_or("")).to_lowercase();
            FILE_TYPE_FORMATS
                .iter()
                .find(|(known, _)| *known == mime)
                .map(|(_, name)| (*name).to_string())
        });
    (format, size_bytes(post.get("file_size")))
}

/// The single post the hash search returned, if it found one.
///
/// An empty list is the normal miss (the local file simply isn't Sankaku's copy) and is not an
/// error. The legacy HTML page, reached because there was no hash to search with, is not JSON and
/// has nothing to read.
fn post_of(body: &str) -> Option<Map<String, Value>> {
    if body.is_empty() {
        return None;
    }
    let data = match json_of(body).ok()? {
        // `data.get("data") or data.get("posts") or []`
        Value::Object(object) => ["data", "posts"]
            .iter()
            .filter_map(|key| object.get(*key))
            .find(|v| pycompat::truthy(v))
            .cloned()
            .unwrap_or(Value::Array(Vec::new())),
        other => other,
    };
    match data {
        Value::Array(items) => match items.into_iter().next()? {
            Value::Object(post) => Some(post),
            _ => None,
        },
        _ => None,
    }
}

/// Why a Sankaku lookup came back empty.
///
/// Both outcomes look identical from the outside (a match with no tags) and they are not the same
/// problem, so neither is left to be guessed at:
///
/// - The hash search ran and Sankaku has no such file. The post is real and its picture is fine;
///   the local copy is simply not byte-identical to Sankaku's, which is what a re-encode or a
///   resave produces. There is nothing further to try, because the numeric id in the URL is a
///   dead end.
/// - There was no md5 to search with, so the legacy HTML page was fetched instead and it carries
///   nothing. That happens when the local file could not be read.
///
/// `None` when the body did contain a post, since then the empty tag list is a genuinely untagged
/// post rather than a miss.
pub fn incomplete_reason(body: &str, _url: &str) -> Option<String> {
    if body.is_empty() {
        return Some("Sankaku returned nothing for this file".to_string());
    }
    if json_of(body).is_err() {
        return Some(
            "No local file hash to look this up with - Sankaku can only be \
             searched by file hash, since its old numeric post ids no longer resolve"
                .to_string(),
        );
    }
    if post_of(body).is_some() {
        return None;
    }
    Some(
        "Sankaku has no byte-identical copy of this file, so its post cannot be \
         identified - the local file is probably a re-encode or a resave. Sankaku's \
         old numeric ids no longer resolve, so there is no other way to reach it."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://www.sankakucomplex.com/post/12345";
    const MD5: &str = "0123456789abcdef0123456789ABCDEF";

    fn names(tags: &[Tag]) -> Vec<(String, String)> {
        tags.iter()
            .map(|t| (t.name.clone(), t.namespace.clone().unwrap_or_default()))
            .collect()
    }

    #[test]
    fn the_hosts_are_the_two_sankaku_domains_and_nothing_that_contains_them() {
        assert!(matches("https://chan.sankakucomplex.com/post/show/1"));
        assert!(matches("https://www.sankaku.app/posts/abc"));
        assert!(!matches("https://notsankakucomplex.com/"));
        assert!(!matches("https://example.com/sankakucomplex.com"));
    }

    #[test]
    fn defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url_with_context",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_rating",
                "parse_file_info",
                "parse_canonical_url",
                "incomplete_reason",
            ]
        );
        assert!(PARSER.empty_result_is_normal);
    }

    #[test]
    fn the_hash_search_url_is_built_from_a_valid_md5_only() {
        let with = |md5: Option<&str>| {
            resolve_fetch_url_with_context(
                URL,
                &FetchContext {
                    local_md5: md5.map(str::to_string),
                },
            )
        };
        assert_eq!(
            with(Some(MD5)),
            "https://sankakuapi.com/posts?tags=md5%3A0123456789abcdef0123456789abcdef&limit=1"
        );
        // `$` matches before one trailing newline, and the newline is carried into the query.
        assert_eq!(
            with(Some("0123456789abcdef0123456789abcdef\n")),
            "https://sankakuapi.com/posts?tags=md5%3A0123456789abcdef0123456789abcdef%0A&limit=1"
        );
        for bad in [
            None,
            Some(""),
            Some("abc"),
            Some("0123456789abcdef0123456789abcdeg"),
            Some("0123456789abcdef0123456789abcdef\n\n"),
            Some("0123456789abcdef0123456789abcdef0"),
        ] {
            assert_eq!(with(bad), URL, "{bad:?}");
        }
    }

    #[test]
    fn tags_are_namespaced_by_the_sites_own_types() {
        let body = r#"[{"tags": [
            {"name_en": " an artist ", "type": 1}, {"name": "fallback", "type": "3"},
            {"name_en": "", "name": "second_choice", "type": 4}, {"name_en": "m", "type": 8},
            {"name_en": "m9", "type": 9}, {"name_en": "genre", "type": 5},
            {"name_en": "d", "type": 0}, {"name_en": "no type"}, {"name_en": "junk", "type": "x"},
            {"name_en": 5, "name": "ignored"}, {"name_en": "   "}, "not a dict", {"type": 1}
        ]}]"#;
        assert_eq!(
            names(&parse(body, URL)),
            [
                ("an_artist", "artist"),
                ("fallback", "copyright"),
                ("second_choice", "character"),
                ("m", "meta"),
                ("m9", "meta"),
                ("genre", "general"),
                ("d", "general"),
                ("no_type", "general"),
                ("junk", "general"),
            ]
            .map(|(n, ns)| (n.to_string(), ns.to_string()))
        );
    }

    #[test]
    fn a_post_may_sit_under_data_or_posts_and_only_the_first_counts() {
        let tags = |body| parse(body, URL).len();
        assert_eq!(tags(r#"{"data": [{"tags": [{"name": "a"}]}]}"#), 1);
        assert_eq!(tags(r#"{"posts": [{"tags": [{"name": "a"}]}]}"#), 1);
        assert_eq!(
            tags(r#"{"data": [], "posts": [{"tags": [{"name": "a"}]}]}"#),
            1
        );
        assert_eq!(
            tags(r#"[{"tags": [{"name": "a"}]}, {"tags": [{"name": "b"}, {"name": "c"}]}]"#),
            1
        );
        for miss in [
            "[]",
            "{}",
            "null",
            "3",
            r#"{"data": 3}"#,
            r#"[5]"#,
            "<html>",
            "",
        ] {
            assert_eq!(tags(miss), 0, "{miss:?}");
        }
    }

    #[test]
    fn the_canonical_url_needs_a_string_id() {
        let canonical = |body| parse_canonical_url(body, URL);
        assert_eq!(
            canonical(r#"[{"id": "QyMk8vZ6Kak"}]"#).as_deref(),
            Some("https://www.sankakucomplex.com/posts/QyMk8vZ6Kak")
        );
        for body in [
            r#"[{"id": 12}]"#,
            r#"[{"id": ""}]"#,
            r#"[{}]"#,
            "[]",
            "<html>",
        ] {
            assert_eq!(canonical(body), None, "{body:?}");
        }
    }

    #[test]
    fn the_sample_is_preferred_to_the_preview_and_only_absolute_urls_count() {
        let preview = |body| parse_preview_url(body, URL);
        assert_eq!(
            preview(r#"[{"sample_url": "https://s/1", "preview_url": "https://p/1"}]"#).as_deref(),
            Some("https://s/1")
        );
        assert_eq!(
            preview(r#"[{"sample_url": "//s/1", "preview_url": "http://p/1"}]"#).as_deref(),
            Some("http://p/1")
        );
        assert_eq!(preview(r#"[{"sample_url": "s/1"}]"#), None);
        assert_eq!(parse_file_url(r#"[{"file_url": "/f.png"}]"#, URL), None);
        assert_eq!(
            parse_file_url(r#"[{"file_url": "https://f/a.png"}]"#, URL).as_deref(),
            Some("https://f/a.png")
        );
    }

    #[test]
    fn the_rating_goes_through_sankakus_own_table() {
        let rating = |code: &str| parse_rating(&format!(r#"[{{"rating": {code}}}]"#), URL);
        assert_eq!(rating(r#""s""#).as_deref(), Some("safe"));
        assert_eq!(rating(r#""Q""#).as_deref(), Some("questionable"));
        assert_eq!(rating(r#""explicit""#).as_deref(), Some("explicit"));
        // "g" is a search alias and "sensitive" is Danbooru's word: neither is Sankaku's.
        for unknown in [r#""g""#, r#""sensitive""#, "null", "1", r#""""#] {
            assert_eq!(rating(unknown), None, "{unknown}");
        }
        assert_eq!(parse_rating("[]", URL), None);
        assert_eq!(parse_rating("[{}]", URL), None);
    }

    #[test]
    fn dimensions_and_file_info_come_from_the_post() {
        let body = r#"[{"width": 1920, "height": "1080", "file_type": "Image/PNG; charset=x",
                        "file_size": 2048}]"#;
        assert_eq!(parse_dimensions(body, URL), (Some(1920), Some(1080)));
        assert_eq!(
            parse_file_info(body, URL),
            (Some("PNG".to_string()), Some(2048))
        );
        let odd = r#"[{"width": 5, "file_type": "image/tiff", "file_size": -1}]"#;
        assert_eq!(parse_dimensions(odd, URL), (None, None));
        assert_eq!(parse_file_info(odd, URL), (None, None));
        for empty in ["[{}]", "[]", "<html>", ""] {
            assert_eq!(parse_dimensions(empty, URL), (None, None), "{empty:?}");
            assert_eq!(parse_file_info(empty, URL), (None, None), "{empty:?}");
        }
    }

    #[test]
    fn each_kind_of_miss_gets_its_own_reason() {
        let reason = |body| incomplete_reason(body, URL);
        assert!(reason("").unwrap().contains("returned nothing"));
        assert!(
            reason("<html>login</html>")
                .unwrap()
                .contains("No local file hash")
        );
        assert!(reason("[]").unwrap().contains("no byte-identical copy"));
        assert!(
            reason("{\"data\": []}")
                .unwrap()
                .contains("no byte-identical copy")
        );
        // A post that is merely untagged gets no excuse.
        assert_eq!(reason(r#"[{"id": "abc", "tags": []}]"#), None);
    }
}
