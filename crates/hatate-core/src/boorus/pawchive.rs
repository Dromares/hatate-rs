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
