//! Pawchive, via its Kemono-style JSON API (`core/boorus/pawchive.py`).
//!
//! Pawchive is a Kemono fork: posts live at `/{service}/user/{uid}/post/{pid}` and the API
//! mirroring them at `/api/v1/...` answers ANONYMOUSLY, even though the HTML pages behind those
//! same URLs are gated behind an account ("...is only available to registered users"). That gap
//! is the whole reason a parser is worth having here.
//!
//! Only pawchive.pw is handled. pawchive.org shares the name and the label in `core/sites.py` but
//! is a different site (it 404s on both the post routes and the API), so it stays
//! classification-only. The Pawchive creator index (`core/pawchive_index.py`) is not this module.
//!
//! What the API does and does not give:
//!
//! - tags, when the post has them, as a POSTGRES ARRAY LITERAL in a string, not a JSON list:
//!   `{Bondage,Damsel,"Total Versext",gefesselt}`;
//! - no artist name, only a numeric user id. The name comes from a second call to the profile
//!   endpoint, cached per creator; without it most posts yield no tags at all, since plenty
//!   carry none of their own;
//! - a preview from `img.pawchive.pw`, and the ORIGINAL from `file.pawchive.pw`, a separate host,
//!   which is why the original looks account-gated if you only probe the image host. No login is
//!   involved: where the archive holds the full file it serves it to anyone;
//! - `has_full`, which says whether it holds that file at all. A false one goes with
//!   `preview_state` "pending" and its original 404s for everyone, so no file URL is offered;
//! - no dimensions anywhere. The file host honours Range requests, though, so
//!   [`parse_dimensions`] reads them from the head of the original itself.
//!
//! Posts are routinely multi-image; "file" is always `attachments[0]`, so the page count is
//! reported honestly even though only Pixiv posts currently get resolved to the matching page.
//!
//! **The two network steps are the caller's.** Python makes two requests from inside the parser:
//! the creator's profile (`_artist_name`) and a ranged read of the original's first bytes
//! (`_remote_size.image_size`). Neither is made here. [`artist_profile_request`] and
//! [`dimensions_request`] say what the engine client has to fetch for a body, and what it
//! learned goes into the [`Context`] (`artist_cache`, `remote_image_sizes`) before the hooks run,
//! under the keys Python uses. A hook that finds its entry missing answers as Python does when
//! the request fails: no artist tag, `(None, None)`.
//!
//! Where Python raises on a record of an unexpected shape (an `attachments` that is the number
//! `5`), the hook has nothing to say: `[]` or `None`, which is what the page assembly makes of a
//! hook that raises.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use tracing::debug;

use super::parsed::json_of;
use super::{Context, Page, SiteParser};
use crate::model::{Tag, TagSource};
use crate::pyrepr::py_str;
use crate::{pycompat, pycsv, pyurl};

use super::host::on_host;

pub const HOSTS: [&str; 1] = ["pawchive.pw"];
pub const THUMBNAIL_BASE: &str = "https://img.pawchive.pw/thumbnail/data";
/// Originals live on their own host, NOT under `img.pawchive.pw`, where every `/data` path 404s
/// and makes them look account-gated when they are not. This is the host the site's own download
/// links point at.
pub const FILE_BASE: &str = "https://file.pawchive.pw/data";

static POST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/(?P<service>[A-Za-z0-9_.-]+)/user/(?P<user>[^/?#]+)/post/(?P<post>[^/?#]+)")
        .expect("post regex")
});

/// A path that is a file's SHA-256 (`$` also matches before one final `\n`, as Python's does).
static SHA256_IN_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/([0-9a-f]{64})\.[A-Za-z0-9]+\n?\z").expect("sha256 regex"));

/// What the registry knows about this site. Python defines no `parse_file_info` (the normal HEAD
/// on the file URL reports format and size), no `parse_rating` and no `parse_restriction`.
pub static PARSER: SiteParser = SiteParser {
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(parse_dimensions),
    parse_page_count: Some(|body, url, _| parse_page_count(body, url)),
    parse_pages: Some(|body, url, _| parse_pages(body, url)),
    pages_api_url: Some(pages_api_url),
    ..SiteParser::new("pawchive", matches, parse)
};

/// Python raised where this is returned. The hooks turn it into their empty answer.
#[derive(Debug, Clone, Copy)]
struct Raised;

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// The host a request for this post goes to: the URL's own `netloc`, else `pawchive.pw`.
fn host_of(url: &str) -> String {
    let netloc = pyurl::urlparse(url, "").netloc;
    if netloc.is_empty() {
        HOSTS[0].to_string()
    } else {
        netloc
    }
}

/// The API mirror of a post page, or the input unchanged.
///
/// The page itself is account-gated, so falling back to it yields nothing, which is the old
/// behaviour, not a new failure.
pub fn resolve_fetch_url(url: &str) -> String {
    match POST_RE.captures(url) {
        Some(found) => format!(
            "https://{}/api/v1/{}/user/{}/post/{}",
            host_of(url),
            &found["service"],
            &found["user"],
            &found["post"]
        ),
        None => url.to_string(),
    }
}

pub fn parse(body: &str, _url: &str, ctx: &Context) -> Vec<Tag> {
    let Some(post) = post(body) else {
        return Vec::new();
    };
    let mut tags: Vec<Tag> = tag_names(post.get("tags"))
        .into_iter()
        .map(|name| Tag {
            name: name.replace(' ', "_"),
            source: TagSource::Booru,
            namespace: Some("general".to_string()),
        })
        .collect();
    if let Some(artist) = artist_name(&post, ctx) {
        // The site's own namespace, so the user's artist -> creator remap fires exactly as it
        // does for every other parser here.
        tags.push(Tag {
            name: artist.replace(' ', "_"),
            source: TagSource::Booru,
            namespace: Some("artist".to_string()),
        });
    }
    tags
}

pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    let path = primary_path(body)?;
    Some(format!("{THUMBNAIL_BASE}{path}"))
}

/// The full-resolution original, when the archive actually holds it.
///
/// `has_full` is the site's own word for that, and it is a statement about what has been
/// imported, not about permissions: a false one goes with `preview_state` "pending", and its file
/// 404s for everyone. Only a post the archive has finished scraping has an original to point at,
/// so guessing a URL for the others would produce a dead link that looks like a broken parser.
pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    let post = post(body).filter(|post| post.get("has_full").is_some_and(pycompat::truthy))?;
    let entry = primary_entry(&post).ok().flatten()?;
    let path = entry.get("path").and_then(Value::as_str)?;
    // The site appends the original filename; harmless, and it gives anything downloading this
    // something better than a bare hash.
    let suffix = match entry.get("name").and_then(Value::as_str) {
        Some(name) if !name.is_empty() => format!("?f={}", pyurl::quote(name)),
        _ => String::new(),
    };
    Some(format!("{FILE_BASE}{path}{suffix}"))
}

/// The URL whose first bytes [`parse_dimensions`] needs read, or `None` when it needs nothing
/// (the archive does not hold the original, so no request is made at all).
///
/// The API has no dimensions at all, but `file.pawchive.pw` serves ranged requests, and an
/// image's header states its size. Only for a post whose original the archive holds, the same
/// condition [`parse_file_url`] uses, since the others 404.
pub fn dimensions_request(body: &str) -> Option<String> {
    let post = post(body).filter(|post| post.get("has_full").is_some_and(pycompat::truthy))?;
    let path = primary_entry(&post).ok().flatten().and_then(|entry| {
        entry
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string)
    })?;
    Some(format!("{FILE_BASE}{path}"))
}

/// Width and height of the original, from what the caller read of its first bytes.
///
/// `ctx.remote_image_sizes` holds that read, keyed by the URL [`dimensions_request`] names; a
/// post with no original, or one nobody has read yet, is `(None, None)`.
pub fn parse_dimensions(body: &str, _url: &str, ctx: &Context) -> (Option<i64>, Option<i64>) {
    dimensions_request(body)
        .and_then(|file_url| ctx.remote_image_sizes.get(&file_url).copied())
        .unwrap_or((None, None))
}

pub fn parse_page_count(body: &str, _url: &str) -> i64 {
    let Some(post) = post(body) else {
        return 1;
    };
    match post.get("attachments") {
        Some(Value::Array(attachments)) if !attachments.is_empty() => attachments.len() as i64,
        _ => 1,
    }
}

// -- which image of a post --------------------------------------------------------------------
// Opting in to `search_engine._resolve_multipage_candidate`, which otherwise leaves every
// multi-image post on its first image, so a match on image 5 was previewed and downloaded as
// image 1.

/// The post's own API record, which lists every file in it.
pub fn pages_api_url(url: &str) -> Option<String> {
    let fetch_url = resolve_fetch_url(url);
    (fetch_url != url).then_some(fetch_url)
}

/// One entry per file, in the site's order, shaped like Pixiv's page list. Each carries the
/// file's SHA-256 (its path IS the hash), so the page holding the local file can be named
/// exactly, without comparing a single picture.
pub fn parse_pages(body: &str, _url: &str) -> Vec<Page> {
    let Some(post) = post(body) else {
        return Vec::new();
    };
    let Ok(holders) = holders(&post) else {
        return Vec::new();
    };
    let has_full = post.get("has_full").is_some_and(pycompat::truthy);
    let mut pages = Vec::new();
    let mut seen = HashSet::new();
    for holder in holders {
        let Some(path) = path_of(holder) else {
            continue;
        };
        if !seen.insert(path) {
            continue;
        }
        let thumbnail = format!("{THUMBNAIL_BASE}{path}");
        let sha = SHA256_IN_PATH_RE
            .captures(path)
            .map(|found| found[1].to_string());
        let original = has_full.then(|| format!("{FILE_BASE}{path}"));
        let mut page = Map::new();
        page.insert(
            "urls".into(),
            json!({"original": original, "regular": thumbnail, "small": thumbnail}),
        );
        page.insert("width".into(), Value::Null);
        page.insert("height".into(), Value::Null);
        page.insert("sha256".into(), json!(sha));
        pages.push(page);
    }
    pages
}

// -- the creator's name -----------------------------------------------------------------------

/// What the engine client has to ask Pawchive before the hooks run on this body: the creator's
/// profile, when the post names one and the context does not already know the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistProfileRequest {
    /// The `artist_cache` key, `"{service}/{user}"`; `Context::remember_artist` takes it back.
    pub key: String,
    pub profile_url: String,
}

/// The profile request [`parse`] needs and cannot make (`None` when it needs none).
///
/// Worth a second request: plenty of posts have no tags of their own, so without this they would
/// contribute nothing at all. Python bounds it with a 10 second timeout and sends the app's
/// user agent; any failure just means no artist tag, and is not remembered as a miss.
pub fn artist_profile_request(
    body: &str,
    url: &str,
    ctx: &Context,
) -> Option<ArtistProfileRequest> {
    let post = post(body)?;
    let (service, user) = service_and_user(&post)?;
    let key = format!("{service}/{user}");
    if ctx.artist_cache.contains_key(&key) {
        return None;
    }
    let profile_url = format!(
        "https://{}/api/v1/{service}/user/{user}/profile",
        host_of(url)
    );
    Some(ArtistProfileRequest { key, profile_url })
}

/// `service` and `user` as `str()` prints them, when both are present and truthy.
fn service_and_user(post: &Map<String, Value>) -> Option<(String, String)> {
    let service = post.get("service").filter(|v| pycompat::truthy(v))?;
    let user = post.get("user").filter(|v| pycompat::truthy(v))?;
    Some((py_str(service), py_str(user)))
}

/// `_artist_name`, for a creator the context already knows: the remembered name, where `""` is a
/// remembered miss and not a cache gap.
fn artist_name(post: &Map<String, Value>, ctx: &Context) -> Option<String> {
    let (service, user) = service_and_user(post)?;
    ctx.artist_cache
        .get(&format!("{service}/{user}"))
        .filter(|name| !name.is_empty())
        .cloned()
}

// -- internals --------------------------------------------------------------------------------

/// `_post`: the post record, whether the API wrapped it (`{"post": {...}}`, as some Kemono
/// revisions do) or not. `None` for an empty body, a body that is not JSON (the account-gated
/// HTML page, because the URL had no post id to turn into an API call; not an error, just
/// nothing), and anything that does not look like a post.
fn post(body: &str) -> Option<Map<String, Value>> {
    if body.is_empty() {
        return None;
    }
    let data = match json_of(body) {
        Ok(Value::Object(data)) => data,
        Ok(_) => return None,
        Err(_) => {
            debug!("Pawchive response was not JSON (the account-gated page?)");
            return None;
        }
    };
    let data = match data.get("post") {
        Some(Value::Object(inner)) => inner.clone(),
        _ => data,
    };
    ["id", "file", "attachments"]
        .iter()
        .any(|key| data.contains_key(*key))
        .then_some(data)
}

/// `(post.get("file"), *(post.get("attachments") or []))`: the places a file entry can be, in
/// order. Splatting a truthy value that is neither a list, a string nor an object is a TypeError;
/// a string or an object yields only strings, which are not entries.
fn holders(post: &Map<String, Value>) -> Result<Vec<&Value>, Raised> {
    let mut holders: Vec<&Value> = post.get("file").into_iter().collect();
    match post.get("attachments") {
        Some(Value::Array(items)) => holders.extend(items),
        Some(Value::Bool(true)) => return Err(Raised),
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f != 0.0) => return Err(Raised),
        _ => {}
    }
    Ok(holders)
}

/// A file entry's `path`, when it is an object holding a string that starts with `/`.
fn path_of(holder: &Value) -> Option<&str> {
    holder
        .as_object()?
        .get("path")?
        .as_str()
        .filter(|path| path.starts_with('/'))
}

/// `_primary_entry`: the post's main image entry. "file" and `attachments[0]` are usually the
/// same entry, and "file" is preferred as the field the site itself calls primary, but "file"
/// can be an empty `{}` with every image under attachments (`patreon/6714576/post/154701291`),
/// so it is only a first choice, never the only one.
fn primary_entry(post: &Map<String, Value>) -> Result<Option<&Map<String, Value>>, Raised> {
    Ok(holders(post)?
        .into_iter()
        .find(|holder| path_of(holder).is_some())
        .and_then(Value::as_object))
}

fn primary_path(body: &str) -> Option<String> {
    let post = post(body)?;
    let entry = primary_entry(&post).ok().flatten()?;
    entry
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `_tag_names`: tag names out of whatever the API put in "tags".
///
/// Normally a Postgres array literal in a string, `{a,b,"c, d"}`, where a quoted element can
/// itself contain the separator, so this cannot be a plain split. Python's `csv` module handles
/// the quoting rules, and so does its port here ([`pycsv`]). A real JSON list is accepted too, in
/// case the API is ever tidied up.
fn tag_names(raw: Option<&Value>) -> Vec<String> {
    let strip = |text: &str| pycompat::strip(text).to_string();
    match raw {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| strip(&py_str(item)))
            .filter(|name| !name.is_empty())
            .collect(),
        Some(Value::String(raw)) => {
            let mut text = pycompat::strip(raw);
            if text.starts_with('{') && text.ends_with('}') && text.len() >= 2 {
                text = &text[1..text.len() - 1];
            }
            if text.is_empty() {
                return Vec::new();
            }
            match pycsv::first_record(text) {
                Ok(fields) => fields
                    .iter()
                    .map(|field| strip(field))
                    .filter(|name| !name.is_empty())
                    .collect(),
                Err(_) => {
                    debug!(
                        "Could not read Pawchive tag list: {:?}",
                        raw.chars().take(120).collect::<String>()
                    );
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const URL: &str = "https://pawchive.pw/patreon/user/50093102/post/83421639";

    /// `TestPawchiveParser._body`: the record Python's tests build, with overrides.
    fn body(overrides: Value) -> String {
        let mut record = json!({
            "id": "83421639", "user": "50093102", "service": "patreon",
            "title": "A Post",
            // A POSTGRES ARRAY LITERAL in a string, not a JSON list, and a quoted element
            // containing the separator.
            "tags": "{Bondage,Damsel,\"Total Versext\",gefesselt}",
            "file": {"name": "a.jpg", "path": "/93/0e/930e7282.jpg"},
            "attachments": [{"name": "a.jpg", "path": "/93/0e/930e7282.jpg"}],
        });
        for (key, value) in overrides.as_object().expect("overrides are an object") {
            record[key] = value.clone();
        }
        record.to_string()
    }

    /// The creator cache `setUp` pre-seeds so no test reaches the network.
    fn ctx() -> Context {
        Context::with_artist_cache(HashMap::from([(
            "patreon/50093102".to_string(),
            "Sandra Spick".to_string(),
        )]))
    }

    fn named(tags: &[Tag], namespace: &str) -> Vec<String> {
        tags.iter()
            .filter(|t| t.namespace.as_deref() == Some(namespace))
            .map(|t| t.name.clone())
            .collect()
    }

    // tests/test_search_quality.py::TestPawchiveParser::test_post_urls_resolve_to_the_api
    #[test]
    fn post_urls_resolve_to_the_api() {
        assert_eq!(
            resolve_fetch_url(URL),
            "https://pawchive.pw/api/v1/patreon/user/50093102/post/83421639"
        );
        // The host is the URL's own netloc, as written.
        assert_eq!(
            resolve_fetch_url("https://PAWCHIVE.pw:8080/p/user/1/post/2?x#y"),
            "https://PAWCHIVE.pw:8080/api/v1/p/user/1/post/2"
        );
        assert_eq!(
            resolve_fetch_url("/p/user/1/post/2"),
            "https://pawchive.pw/api/v1/p/user/1/post/2"
        );
    }

    // ...::test_a_url_with_no_post_is_left_alone
    #[test]
    fn a_url_with_no_post_is_left_alone() {
        for url in [
            "https://pawchive.pw/",
            "https://pawchive.pw/posts/popular",
            "",
            "https://pawchive.pw/a/user//post/p",
            "https://pawchive.pw/a/user/b/post/",
        ] {
            assert_eq!(resolve_fetch_url(url), url);
            assert_eq!(pages_api_url(url), None);
        }
        assert_eq!(
            pages_api_url(URL).as_deref(),
            Some("https://pawchive.pw/api/v1/patreon/user/50093102/post/83421639")
        );
    }

    // ...::test_postgres_array_tags_are_split_on_the_right_commas: a quoted element contains the
    // separator, so a plain split would turn one tag into two.
    #[test]
    fn postgres_array_tags_are_split_on_the_right_commas() {
        let tags = parse(&body(json!({})), URL, &ctx());
        assert_eq!(
            named(&tags, "general"),
            ["Bondage", "Damsel", "Total_Versext", "gefesselt"]
        );
    }

    // ...::test_the_creator_becomes_an_artist_tag: most posts carry no tags of their own.
    #[test]
    fn the_creator_becomes_an_artist_tag() {
        let tags = parse(&body(json!({"tags": null})), URL, &ctx());
        assert_eq!(
            tags,
            [Tag {
                name: "Sandra_Spick".to_string(),
                namespace: Some("artist".to_string()),
                source: TagSource::Booru,
            }]
        );
        // The artist comes after the post's own tags.
        let all = parse(&body(json!({})), URL, &ctx());
        assert_eq!(all.last().map(|t| t.name.as_str()), Some("Sandra_Spick"));
    }

    // ...::test_a_json_list_of_tags_is_also_accepted
    #[test]
    fn a_json_list_of_tags_is_also_accepted() {
        let tags = parse(&body(json!({"tags": ["a", "b b"]})), URL, &ctx());
        assert_eq!(named(&tags, "general"), ["a", "b_b"]);
    }

    // ...::test_the_preview_comes_from_the_image_host
    #[test]
    fn the_preview_comes_from_the_image_host() {
        assert_eq!(
            parse_preview_url(&body(json!({})), URL).as_deref(),
            Some("https://img.pawchive.pw/thumbnail/data/93/0e/930e7282.jpg")
        );
    }

    // ...::test_the_original_comes_from_the_file_host: every /data path on img.pawchive.pw 404s.
    #[test]
    fn the_original_comes_from_the_file_host() {
        assert_eq!(
            parse_file_url(&body(json!({"has_full": true})), URL).as_deref(),
            Some("https://file.pawchive.pw/data/93/0e/930e7282.jpg?f=a.jpg")
        );
    }

    // ...::test_no_file_url_when_the_archive_lacks_the_full_file: has_full is about what has been
    // imported, not permissions.
    #[test]
    fn no_file_url_when_the_archive_lacks_the_full_file() {
        for body in [body(json!({"has_full": false})), body(json!({}))] {
            assert_eq!(parse_file_url(&body, URL), None);
        }
        // Any truthy value counts; any falsy one does not.
        for (value, has) in [
            (json!(1), true),
            (json!("yes"), true),
            (json!([0]), true),
            (json!(0), false),
            (json!(""), false),
            (json!(null), false),
            (json!([]), false),
        ] {
            let body = body(json!({"has_full": value.clone()}));
            assert_eq!(parse_file_url(&body, URL).is_some(), has, "{value}");
        }
    }

    // ...::test_a_filename_with_awkward_characters_is_url_encoded
    #[test]
    fn a_filename_with_awkward_characters_is_url_encoded() {
        let file = json!({"name": "a b&c.jpg", "path": "/93/0e/930e7282.jpg"});
        let url = parse_file_url(&body(json!({"has_full": true, "file": file})), URL);
        assert!(url.unwrap().contains("?f=a%20b%26c.jpg"));
        // No name, an empty one or a non-string one: no suffix.
        for name in [json!(""), json!(5), json!(null)] {
            let file = json!({"name": name, "path": "/x"});
            let url = parse_file_url(&body(json!({"has_full": true, "file": file})), URL);
            assert_eq!(url.as_deref(), Some("https://file.pawchive.pw/data/x"));
        }
    }

    // ...::test_an_empty_file_field_falls_back_to_the_attachments (seen live: "file" is {} and
    // every image is under attachments)
    #[test]
    fn an_empty_file_field_falls_back_to_the_attachments() {
        let post = body(json!({
            "has_full": true, "file": {},
            "attachments": [
                {"name": "b.jpg", "path": "/a6/bc/a6bc.jpg"},
                {"name": "c.jpg", "path": "/ec/8b/ec8b.jpg"},
            ],
        }));
        assert_eq!(
            parse_file_url(&post, URL),
            Some(format!("{FILE_BASE}/a6/bc/a6bc.jpg?f=b.jpg"))
        );
        // A path that does not start with "/" is not an entry, whichever field it is in.
        let relative = body(
            json!({"has_full": true, "file": {"path": "a/b.jpg"}, "attachments": [{"path": "/q.jpg"}]}),
        );
        assert_eq!(
            parse_file_url(&relative, URL),
            Some(format!("{FILE_BASE}/q.jpg"))
        );
    }

    // ...::test_multi_image_posts_report_their_page_count
    #[test]
    fn multi_image_posts_report_their_page_count() {
        let attachments: Vec<Value> = (0..12)
            .map(|i| json!({"name": format!("{i}.jpg"), "path": format!("/a/b/{i}.jpg")}))
            .collect();
        assert_eq!(
            parse_page_count(&body(json!({"attachments": attachments})), URL),
            12
        );
        assert_eq!(parse_page_count(&body(json!({})), URL), 1);
        for odd in [
            json!([]),
            json!(null),
            json!("abc"),
            json!({"a": 1}),
            json!(5),
        ] {
            assert_eq!(parse_page_count(&body(json!({"attachments": odd})), URL), 1);
        }
        assert_eq!(parse_page_count("<html></html>", URL), 1);
    }

    // ...::test_file_info_is_not_claimed: the API does not report it, and the normal HEAD on the
    // file URL does, so this hook is deliberately absent.
    #[test]
    fn file_info_is_not_claimed_and_the_hooks_are_the_ones_python_defines() {
        assert!(PARSER.parse_file_info.is_none());
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_page_count",
                "parse_pages",
                "pages_api_url",
            ]
        );
        assert!(!PARSER.empty_result_is_normal);
    }

    // ...::test_dimensions_are_read_from_the_head_of_the_original: the read itself (PIL over a
    // Range request) is the engine client's; what the parser decides is which URL to read and
    // what to say with the answer.
    #[test]
    fn dimensions_come_from_a_read_of_the_original_never_the_preview() {
        let path = "/aa/bb/0123.png";
        let post = body(json!({"has_full": true, "file": {"name": "a.png", "path": path}}));
        let file_url = format!("{FILE_BASE}{path}");
        assert_eq!(
            dimensions_request(&post).as_deref(),
            Some(file_url.as_str())
        );
        // Nothing read yet: Python's read failed, and answers (None, None).
        assert_eq!(
            parse_dimensions(&post, URL, &Context::default()),
            (None, None)
        );
        let mut ctx = Context::default();
        ctx.remember_image_size(&file_url, (Some(321), Some(123)));
        assert_eq!(parse_dimensions(&post, URL, &ctx), (Some(321), Some(123)));
        // A read of the preview's URL is not the original's.
        let mut preview = Context::default();
        preview.remember_image_size(&format!("{THUMBNAIL_BASE}{path}"), (Some(1), Some(1)));
        assert_eq!(parse_dimensions(&post, URL, &preview), (None, None));
        // A file whose size cannot be read from its start is remembered as `(None, None)`.
        let mut unreadable = Context::default();
        unreadable.remember_image_size(&file_url, (None, None));
        assert_eq!(parse_dimensions(&post, URL, &unreadable), (None, None));
    }

    // ...::test_no_dimensions_without_the_original: has_full false means the original 404s for
    // everyone, so no request is made at all.
    #[test]
    fn no_dimensions_and_no_request_without_the_original() {
        for post in [
            body(json!({"has_full": false})),
            body(json!({})),
            body(json!({"has_full": true, "file": {}, "attachments": []})),
            "<html></html>".to_string(),
            String::new(),
        ] {
            assert_eq!(dimensions_request(&post), None, "{post}");
            assert_eq!(
                parse_dimensions(&post, URL, &Context::default()),
                (None, None)
            );
        }
    }

    // ...::test_a_network_failure_is_not_remembered
    #[test]
    fn a_failed_read_or_lookup_is_not_something_the_context_remembers_unless_told_to() {
        let post = body(json!({"has_full": true, "file": {"path": "/aa/bb/0123.png"}}));
        // The caller leaves the context alone on a network failure: the request is still due.
        let ctx = Context::default();
        assert!(dimensions_request(&post).is_some());
        assert_eq!(parse_dimensions(&post, URL, &ctx), (None, None));
        assert!(ctx.remote_image_sizes.is_empty());
        assert_eq!(
            artist_profile_request(&post, URL, &ctx)
                .map(|r| r.key)
                .as_deref(),
            Some("patreon/50093102")
        );
    }

    // ...::test_the_account_gated_html_page_yields_nothing
    #[test]
    fn the_account_gated_html_page_yields_nothing() {
        let html = "<!DOCTYPE html><html><body>only available to registered users</body></html>";
        assert!(parse(html, URL, &ctx()).is_empty());
        assert_eq!(parse_preview_url(html, URL), None);
        assert_eq!(parse_page_count(html, URL), 1);
        assert!(parse_pages(html, URL).is_empty());
        assert_eq!(parse_file_url(html, URL), None);
    }

    // ...::test_a_failed_creator_lookup_just_means_no_artist_tag
    #[test]
    fn a_failed_creator_lookup_just_means_no_artist_tag() {
        let post = body(json!({"user": "99999999", "tags": null}));
        // A creator the context does not know: Python's request failed.
        assert!(parse(&post, URL, &ctx()).is_empty());
        // A remembered miss is "" and is no tag either, and is not asked about again.
        let mut ctx = ctx();
        ctx.remember_artist("patreon/99999999", None);
        assert!(parse(&post, URL, &ctx).is_empty());
        assert_eq!(artist_profile_request(&post, URL, &ctx), None);
    }

    // ...::test_only_the_pw_domain_is_parsed
    #[test]
    fn only_the_pw_domain_is_parsed() {
        assert!(matches(URL));
        assert_eq!(
            super::super::find_parser(URL).map(|p| p.name),
            Some("pawchive")
        );
        assert!(super::super::find_parser("https://pawchive.org/some/post").is_none());
        // `core/parser_probe.py` PROBES.
        let probe = "https://pawchive.pw/patreon/user/50093102/post/83421639";
        assert_eq!(
            super::super::find_parser(probe).map(|p| p.name),
            Some("pawchive")
        );
        assert!(!matches("https://notpawchive.pw/a/user/b/post/c"));
        assert!(matches("https://img.pawchive.pw/x"));
    }

    // tests/test_search_quality.py::TestPawchivePagesByHash::test_pages_carry_their_hash_and_file_links
    #[test]
    fn pages_carry_their_hash_and_file_links() {
        let shas = ["a".repeat(64), "b".repeat(64)];
        let files: Vec<Value> = shas
            .iter()
            .enumerate()
            .map(|(i, h)| json!({"name": format!("{i}.png"), "path": format!("/{}/{}/{h}.png", &h[..2], &h[2..4])}))
            .collect();
        let record = |has_full| {
            json!({"post": {"id": "9", "user": "1", "service": "fanbox", "has_full": has_full,
                            "file": files[0], "attachments": files}})
            .to_string()
        };
        let pages = parse_pages(&record(true), "https://pawchive.pw/fanbox/user/1/post/9");
        let hashes: Vec<_> = pages
            .iter()
            .map(|p| p["sha256"].as_str().unwrap())
            .collect();
        assert_eq!(hashes, shas);
        assert_eq!(
            pages[1]["urls"]["original"],
            json!(format!(
                "https://file.pawchive.pw/data/bb/bb/{}.png",
                "b".repeat(64)
            ))
        );
        assert_eq!(pages[1]["urls"]["regular"], pages[1]["urls"]["small"]);
        assert_eq!(pages[1]["width"], Value::Null);
        assert_eq!(pages[1]["height"], Value::Null);
        // "file" is attachments[0]: listed once.
        assert_eq!(pages.len(), 2);
        let no_full = parse_pages(&record(false), "https://x/");
        assert_eq!(no_full[1]["urls"]["original"], Value::Null);
    }

    #[test]
    fn a_page_without_a_hash_in_its_path_has_no_sha() {
        let hash = "c".repeat(64);
        let record = json!({"id": 1, "attachments": [
            {"path": "/x/y/plain.jpg"},
            {"path": format!("/x/{hash}.png\n")},
            {"path": format!("/x/{hash}.tar.gz")},
            {"path": format!("/x/{}.png", "A".repeat(64))},
            {"path": format!("/x/{}.png", "c".repeat(63))},
            {"path": format!("/x/{hash}.")},
            {"path": "no-slash.jpg"},
            "text", null, 5,
            {"path": "/x/y/plain.jpg"},
        ]})
        .to_string();
        let pages = parse_pages(&record, URL);
        let shas: Vec<_> = pages.iter().map(|p| p["sha256"].clone()).collect();
        assert_eq!(
            shas,
            [
                json!(null),
                json!(hash),
                json!(null),
                json!(null),
                json!(null),
                json!(null)
            ]
        );
    }

    #[test]
    fn a_post_is_the_record_or_the_record_inside_post_and_must_look_like_one() {
        let record = json!({"id": 5, "file": {"path": "/a.png"}});
        let wrapped = json!({"post": record.clone()});
        for text in [record.to_string(), wrapped.to_string()] {
            assert_eq!(
                parse_preview_url(&text, URL).as_deref(),
                Some("https://img.pawchive.pw/thumbnail/data/a.png")
            );
        }
        for text in [
            "",
            "[]",
            "5",
            "null",
            "{}",
            "{\"post\": 5}",
            "{\"post\": {}}",
            "<html>x</html>",
            "{\"title\": \"no id, file or attachments\"}",
        ] {
            assert_eq!(parse_preview_url(text, URL), None, "{text}");
        }
        // Any one of the three keys is enough, even if null.
        assert_eq!(parse_page_count("{\"attachments\": null}", URL), 1);
        let outer = json!({"id": 1, "file": {"path": "/outer.png"}, "post": {"id": 2, "file": {"path": "/inner.png"}}});
        assert_eq!(
            parse_preview_url(&outer.to_string(), URL).as_deref(),
            Some("https://img.pawchive.pw/thumbnail/data/inner.png")
        );
    }

    #[test]
    fn tag_lists_are_read_as_python_s_csv_reads_them() {
        let names = |tags: &str| {
            let post = body(json!({"tags": tags, "user": ""}));
            named(&parse(&post, URL, &ctx()), "general")
        };
        assert_eq!(names("{a, b ,\"c, d\",e}"), ["a", "b", "c,_d", "e"]);
        assert_eq!(names("a,b"), ["a", "b"]);
        assert_eq!(names("{}"), Vec::<String>::new());
        assert_eq!(names(""), Vec::<String>::new());
        assert_eq!(names("{a,,b}"), ["a", "b"]);
        assert_eq!(names("{\"a\"\"b\",c}"), ["a\"b", "c"]);
        assert_eq!(names("{a\\,b,c}"), ["a,b", "c"]);
        // Only the first line is a record; a quoted field may span lines; a bare CR inside an
        // unquoted field is `csv.Error` and so no tags at all.
        assert_eq!(names("{a,b\nc,d}"), ["a", "b"]);
        assert_eq!(names("{\"a\nb\",c}"), ["a\nb", "c"]);
        assert_eq!(names("{a\rb}"), Vec::<String>::new());
        // An unclosed quote still gives its field.
        assert_eq!(names("\"a"), ["a"]);
        assert_eq!(names("{\"a"), ["{\"a"]);
        // Only a matching pair of braces is removed.
        assert_eq!(names("{a"), ["{a"]);
        assert_eq!(names("a}"), ["a}"]);
        assert_eq!(names("{{a}}"), ["{a}"]);
        // A field over csv's size limit is `csv.Error` too.
        assert_eq!(
            names(&format!("{{{}}}", "x".repeat(131_073))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_json_list_of_tags_is_str_of_each_item_stripped() {
        let names = |tags: Value| {
            let post = body(json!({"tags": tags, "user": ""}));
            named(&parse(&post, URL, &ctx()), "general")
        };
        assert_eq!(
            names(json!(["  ", "", " a ", 1, 2.5, true, null])),
            ["a", "1", "2.5", "True", "None"]
        );
        assert_eq!(names(json!([["a"], {"k": "v"}])), ["['a']", "{'k':_'v'}"]);
        for other in [json!(5), json!({"a": 1}), json!(true), json!(null)] {
            assert_eq!(names(other), Vec::<String>::new());
        }
    }

    #[test]
    fn the_creator_key_is_service_slash_user_as_python_prints_them() {
        let artist = |service: Value, user: Value| {
            let post = body(json!({"service": service, "user": user, "tags": null}));
            let ctx = Context::with_artist_cache(HashMap::from([
                ("5/7".to_string(), "Num".to_string()),
                ("True/x".to_string(), "Bool".to_string()),
                ("1.5/2".to_string(), "Float".to_string()),
                ("patreon/50093102".to_string(), "Sandra Spick".to_string()),
                ("fanbox/1".to_string(), String::new()),
                ("a/b".to_string(), " padded ".to_string()),
            ]));
            named(&parse(&post, URL, &ctx), "artist")
        };
        assert_eq!(
            artist(json!("patreon"), json!("50093102")),
            ["Sandra_Spick"]
        );
        assert_eq!(artist(json!(5), json!(7)), ["Num"]);
        assert_eq!(artist(json!(true), json!("x")), ["Bool"]);
        assert_eq!(artist(json!(1.5), json!(2)), ["Float"]);
        // A remembered miss, a falsy service or user, and an unknown creator: no artist.
        assert!(artist(json!("fanbox"), json!(1)).is_empty());
        assert!(artist(json!(""), json!("x")).is_empty());
        assert!(artist(json!("patreon"), json!(0)).is_empty());
        assert!(artist(json!(null), json!("x")).is_empty());
        assert!(artist(json!("patreon"), json!("1")).is_empty());
        // The cache is trusted as it stands (`remember_artist` is what strips): a name inserted
        // padded keeps its padding, which the tag turns into underscores.
        assert_eq!(artist(json!("a"), json!("b")), ["_padded_"]);
    }

    #[test]
    fn the_profile_request_names_what_the_engine_client_has_to_fetch() {
        let post = body(json!({}));
        let empty = Context::default();
        let request = artist_profile_request(&post, URL, &empty).expect("a request");
        assert_eq!(request.key, "patreon/50093102");
        assert_eq!(
            request.profile_url,
            "https://pawchive.pw/api/v1/patreon/user/50093102/profile"
        );
        // The host the post link came from, and `pawchive.pw` for a link with none.
        let other =
            artist_profile_request(&post, "https://PAWCHIVE.pw:8080/x/user/1/post/2", &empty);
        assert!(
            other
                .unwrap()
                .profile_url
                .starts_with("https://PAWCHIVE.pw:8080/api/v1/")
        );
        let bare = artist_profile_request(&post, "x", &empty).unwrap();
        assert!(bare.profile_url.starts_with("https://pawchive.pw/api/v1/"));
        // Known creators, remembered misses, and posts that name nobody need no request.
        assert_eq!(artist_profile_request(&post, URL, &ctx()), None);
        let mut missed = Context::default();
        missed.remember_artist("patreon/50093102", None);
        assert_eq!(artist_profile_request(&post, URL, &missed), None);
        assert_eq!(
            artist_profile_request(&body(json!({"user": ""})), URL, &empty),
            None
        );
        assert_eq!(artist_profile_request("<html></html>", URL, &empty), None);
    }

    #[test]
    fn remembering_a_creator_stores_the_stripped_name_or_the_empty_miss() {
        let mut ctx = Context::default();
        ctx.remember_artist("a/1", Some("  Name Here \n"));
        ctx.remember_artist("a/2", Some("   "));
        ctx.remember_artist("a/3", None);
        assert_eq!(ctx.artist_cache["a/1"], "Name Here");
        assert_eq!(ctx.artist_cache["a/2"], "");
        assert_eq!(ctx.artist_cache["a/3"], "");
    }

    #[test]
    fn an_attachments_value_python_cannot_iterate_gives_nothing_where_python_raises() {
        for attachments in [json!(5), json!(true), json!(1.5)] {
            let post = body(json!({"attachments": attachments.clone(), "has_full": true}));
            assert_eq!(parse_preview_url(&post, URL), None, "{attachments}");
            assert_eq!(parse_file_url(&post, URL), None);
            assert!(parse_pages(&post, URL).is_empty());
            assert_eq!(dimensions_request(&post), None);
        }
        // A string or an object iterates to strings, which are not entries; falsy ones to nothing.
        for attachments in [
            json!("abc"),
            json!({"a": 1}),
            json!(0),
            json!(false),
            json!(null),
            json!([]),
        ] {
            let post = body(json!({"attachments": attachments.clone()}));
            assert!(parse_preview_url(&post, URL).is_some(), "{attachments}");
        }
    }
}
