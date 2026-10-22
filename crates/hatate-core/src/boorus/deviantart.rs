//! DeviantArt, via the JSON state embedded in its deviation pages (`core/boorus/deviantart.py`).
//!
//! DeviantArt publishes no booru-style tag vocabulary, so this is a limited parser by nature: what
//! comes out is the artist, the real file's size and dimensions, and a picture, not a tag list.
//!
//! It reads `window.__INITIAL_STATE__` out of the deviation page rather than using the public
//! oEmbed endpoint, which describes only the downscaled render it hands over (its width and height
//! are the render's, and it names no file size at all). The page state carries the ORIGINAL's
//! type, dimensions and byte size, even for an adult deviation viewed logged-out, where only the
//! picture is withheld and the facts about it are not.
//!
//! Two things this deliberately does NOT report:
//!
//! - A file URL. The original answers 403 even with the page's own signing token, so every URL
//!   here is a render. Passing a render off as the match's file would have it downloaded and sent
//!   to Hydrus as though it were the artwork.
//! - Anything for a deviation whose state is missing. A page that didn't load its state yields
//!   nothing rather than a guess.
//!
//! Every hook is a pure function of the body and the post-page URL; the site keeps no state.
//!
//! Where Python raises on a state of an unexpected shape (a `deviation` entry that is a list, a
//! `types` entry whose `t` is a list, a state that is the number `5`), the hook has nothing to
//! say, which is what the page assembly does with a hook that raises: `[]` for `parse`, `None`
//! for the URL and restriction hooks, `(None, None)` for the pairs. [`Raised`] marks those paths.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};
use tracing::debug;

use super::SiteParser;
use super::host::on_host;
use super::json_fields::{dimensions, size_bytes};
use crate::model::{Tag, TagSource};
use crate::{pycompat, pyrepr};

pub const HOSTS: [&str; 2] = ["deviantart.com", "fav.me"];

/// The page embeds its state as `JSON.parse("...")`: a JSON document inside a JS string literal,
/// so it needs unwrapping twice.
///
/// The end anchor has to be the end of the LINE, not just the first `);`. That string contains
/// inline JS of its own (`__REER__.push({n,d});`), so a non-greedy match to `);` alone stops 13KB
/// in and silently truncates a 427KB document, which parses as nothing and looks exactly like a
/// page that carried no state at all. (`[\s\x1c-\x1f]` is Python's `\s`, which counts the four
/// ASCII separators that Unicode `White_Space`, and so the regex crate's `\s`, does not.)
static STATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?sm)window\.__INITIAL_STATE__[\s\x1c-\x1f]*=[\s\x1c-\x1f]*JSON\.parse\((.*?)\);[\s\x1c-\x1f]*$",
    )
    .expect("state regex")
});

/// The id the URL ends in: digits followed by `?`, `#` or the end (Python's `$`, which also
/// matches before one final `\n`).
static WANTED_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)(?:\?|#|\n?\z)").expect("id regex"));

/// A blur parameter in the render path DeviantArt signs.
static BLUR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[,/]blur_\d+").expect("blur regex"));

/// Renders are served from paths carrying transformation parameters; the unadorned `baseUri` is
/// the original (which DeviantArt will not serve us).
const RENDER_MARKER: &str = "/v1/";

/// Best picture first. "fullview" is the largest render DeviantArt offers without a download, and
/// is what the site itself shows.
const PREVIEW_TYPES: [&str; 5] = ["fullview", "preview", "400T", "350T", "300W"];

/// `originalFile.type` to the names `fetch_remote_info` derives from a Content-Type, so the UI
/// shows one vocabulary whatever the source.
const FILE_TYPE_FORMATS: [(&str, &str); 9] = [
    ("jpeg", "JPEG"),
    ("jpg", "JPEG"),
    ("png", "PNG"),
    ("gif", "GIF"),
    ("webp", "WEBP"),
    ("bmp", "BMP"),
    ("avif", "AVIF"),
    ("mp4", "MP4"),
    ("webm", "WEBM"),
];

const BLURRED: &str = "DeviantArt is only serving a blurred copy of this adult deviation, which is \
                       what it shows when logged out. Add cookies from a logged-in account under \
                       Settings > Site Logins to see it properly.";

/// What the registry knows about this site. `restriction_for_status` and the rest are not defined
/// by the Python module.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_file_info: Some(|body, url, _| parse_file_info(body, url)),
    parse_restriction: Some(|body, url, _| parse_restriction(body, url)),
    ..SiteParser::new("deviantart", matches, |body, url, _| parse(body, url))
};

/// Python raised where this is returned. The hooks turn it into their empty answer.
#[derive(Debug, Clone, Copy)]
struct Raised;

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// The artist, as a creator tag. The page state exposes no others.
///
/// Stored under the site's own "artist" namespace so the user's artist -> creator remap fires,
/// matching every other parser here.
pub fn parse(body: &str, url: &str) -> Vec<Tag> {
    let username = match deviation(body, url) {
        Ok(Some(found)) => found.username,
        _ => None,
    };
    // `if not username: return []`, then `username.replace(...)`, which raises on a truthy value
    // that is not a string.
    match username {
        Some(Value::String(name)) if !name.is_empty() => vec![Tag {
            name: name.replace(' ', "_"),
            source: TagSource::Booru,
            namespace: Some("artist".to_string()),
        }],
        _ => Vec::new(),
    }
}

/// `(format, size_in_bytes)` of the ORIGINAL file.
///
/// This is the reason the page is parsed rather than oEmbed. There is no reachable file URL to
/// HEAD, so without the site stating these they could not be reported at all, and the HEAD would
/// instead measure the search engine's thumbnail and present that as the match's size.
pub fn parse_file_info(body: &str, url: &str) -> (Option<String>, Option<i64>) {
    let Ok(Some(original)) = original_file(body, url) else {
        return (None, None);
    };
    if original.is_empty() {
        return (None, None);
    }
    let format = original
        .get("type")
        .and_then(Value::as_str)
        .and_then(|file_type| {
            let key = pycompat::strip(file_type)
                .to_lowercase()
                .trim_start_matches('.')
                .to_string();
            FILE_TYPE_FORMATS
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, format)| (*format).to_string())
        });
    (format, size_bytes(original.get("filesize")))
}

/// The ORIGINAL's dimensions, not the render's.
///
/// The app compares these against the local file to decide whether a match is an upgrade, so the
/// render's size (which is what oEmbed reports) would present real upgrades as downgrades.
pub fn parse_dimensions(body: &str, url: &str) -> (Option<i64>, Option<i64>) {
    match original_file(body, url) {
        Ok(Some(original)) if !original.is_empty() => dimensions(&original),
        _ => (None, None),
    }
}

pub fn parse_preview_url(body: &str, url: &str) -> Option<String> {
    render_url(body, url).ok().flatten()
}

/// The original, which DeviantArt does not serve.
///
/// Its `baseUri` answers 403 even with the page's own token, so anything obtainable here is a
/// render. Only a URL that carries no render parameters is offered, on the chance that ever
/// changes.
pub fn parse_file_url(body: &str, url: &str) -> Option<String> {
    render_url(body, url)
        .ok()
        .flatten()
        .filter(|rendered| !rendered.contains(RENDER_MARKER))
}

/// Why this deviation isn't fully visible, or `None` if it is.
///
/// An adult deviation viewed logged-out has the blur baked into the render path DeviantArt signs
/// (`blur_73`), so nothing recovers the real picture but a login.
pub fn parse_restriction(body: &str, url: &str) -> Option<String> {
    let rendered = render_url(body, url).ok().flatten()?;
    BLUR_RE.is_match(&rendered).then(|| BLURRED.to_string())
}

// -- state extraction -------------------------------------------------------------------------

/// `_state`: the page's state object, `Ok(None)` for a page with none (or one that does not
/// parse, or an empty one). `Err` is a state that is neither: Python's `state.get` raises on it.
fn state(body: &str) -> Result<Option<Map<String, Value>>, Raised> {
    let Some(found) = STATE_RE.captures(body) else {
        debug!("No __INITIAL_STATE__ in the DeviantArt response");
        return Ok(None);
    };
    // \' is legal in a JS string and rejected by JSON, and the page does contain it. Nothing else
    // in there needs fixing.
    let raw = found[1].replace("\\'", "'");
    let unwrapped = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::String(inner)) => inner,
        // `json.loads` of something that is not a string is a TypeError, which `except
        // ValueError` does not catch.
        Ok(_) => return Err(Raised),
        Err(error) => {
            debug!("Could not parse DeviantArt page state: {error}");
            return Ok(None);
        }
    };
    match serde_json::from_str::<Value>(&unwrapped) {
        Ok(Value::Object(state)) => Ok((!state.is_empty()).then_some(state)),
        Ok(other) if !pycompat::truthy(&other) => Ok(None),
        Ok(_) => Err(Raised),
        Err(error) => {
            debug!("Could not parse DeviantArt page state: {error}");
            Ok(None)
        }
    }
}

/// What `_deviation` returns for a page that has its deviation: the entries are the JSON values
/// themselves (`{}` where the entry is missing or falsy), and `username` is whatever the user
/// table holds, if anything.
struct Found {
    deviation: Value,
    extended: Value,
    username: Option<Value>,
}

/// Python's `x or {}`.
fn or_empty(v: Option<&Value>) -> Value {
    match v {
        Some(v) if pycompat::truthy(v) => v.clone(),
        _ => Value::Object(Map::new()),
    }
}

/// `_deviation`: the page's OWN deviation.
///
/// A page also carries entries for related deviations, so the right one has to be picked
/// deliberately. "deviationExtended" holds only the one the page is about, which makes it the
/// anchor; the id in the URL is used to disambiguate if that ever stops being true.
fn deviation(body: &str, url: &str) -> Result<Option<Found>, Raised> {
    let Some(state) = state(body)? else {
        return Ok(None);
    };
    let Some(Value::Object(entities)) = state.get("@@entities") else {
        return Ok(None);
    };
    let Some(Value::Object(extended_table)) = entities.get("deviationExtended") else {
        return Ok(None);
    };
    if extended_table.is_empty() {
        return Ok(None);
    }
    let deviation_table = entities.get("deviation");

    let wanted = WANTED_ID_RE
        .captures(url)
        .map(|found| found[1].to_string())
        .filter(|id| extended_table.contains_key(id));
    let key = match wanted {
        Some(id) => id,
        None if extended_table.len() == 1 => {
            extended_table.keys().next().expect("one key").to_string()
        }
        None => {
            debug!(
                "Could not tell which of {} deviations this page is about",
                extended_table.len()
            );
            return Ok(None);
        }
    };

    let extended = or_empty(extended_table.get(&key));
    // `(deviation_table or {}).get(key) or {}`: `.get` on a table that is truthy and not a dict
    // raises.
    let deviation = match deviation_table {
        Some(Value::Object(table)) => or_empty(table.get(&key)),
        Some(other) if pycompat::truthy(other) => return Err(Raised),
        _ => or_empty(None),
    };

    // `deviation.get("author")` raises on an entry that is truthy and not a dict.
    let Value::Object(entry) = &deviation else {
        return Err(Raised);
    };
    let mut username = None;
    if let (Some(Value::Object(users)), Some(author_id)) =
        (entities.get("user"), entry.get("author"))
        && !author_id.is_null()
    {
        let user = or_empty(users.get(&pyrepr::py_str(author_id)));
        let Value::Object(user) = user else {
            return Err(Raised);
        };
        username = user.get("username").cloned();
    }
    Ok(Some(Found {
        deviation,
        extended,
        username,
    }))
}

/// `_original_file`: the page's `originalFile` entry when it is an object (an empty one
/// included; the callers treat it as "none").
fn original_file(body: &str, url: &str) -> Result<Option<Map<String, Value>>, Raised> {
    let Some(found) = deviation(body, url)? else {
        return Ok(None);
    };
    // `(extended or {}).get("originalFile")` raises on an entry that is truthy and not a dict.
    let Value::Object(extended) = &found.extended else {
        return Err(Raised);
    };
    Ok(match extended.get("originalFile") {
        Some(Value::Object(original)) => Some(original.clone()),
        _ => None,
    })
}

/// `_render_url`: the best picture DeviantArt will actually serve for this deviation.
///
/// Built rather than read: the state gives a base URI, a signing token and a set of path
/// templates with the file's name substituted in.
fn render_url(body: &str, url: &str) -> Result<Option<String>, Raised> {
    let Some(found) = deviation(body, url)? else {
        return Ok(None);
    };
    let Some(Value::Object(media)) = found.deviation.get("media") else {
        return Ok(None);
    };
    let Some(base) = media
        .get("baseUri")
        .and_then(Value::as_str)
        .filter(|base| base.starts_with("http://") || base.starts_with("https://"))
    else {
        return Ok(None);
    };

    // `{t.get("t"): t for t in media.get("types") or [] if isinstance(t, dict)}`: a later entry
    // with the same name wins, iterating a truthy value that is neither a list, a string nor an
    // object is a TypeError, and a `t` that is a list or an object is unhashable.
    let mut types: Vec<(&str, &Map<String, Value>)> = Vec::new();
    match media.get("types") {
        Some(Value::Array(items)) => {
            for item in items {
                let Value::Object(entry) = item else {
                    continue;
                };
                match entry.get("t") {
                    Some(Value::Array(_) | Value::Object(_)) => return Err(Raised),
                    Some(Value::String(name)) => types.push((name, entry)),
                    _ => {}
                }
            }
        }
        Some(Value::Bool(true)) => return Err(Raised),
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f != 0.0) => return Err(Raised),
        _ => {}
    }
    let Some(chosen) = PREVIEW_TYPES.iter().find_map(|wanted| {
        types
            .iter()
            .rev()
            .find(|(name, _)| name == wanted)
            .map(|(_, entry)| *entry)
    }) else {
        return Ok(None);
    };
    let Some(path) = chosen.get("c").and_then(Value::as_str) else {
        return Ok(None);
    };
    let path = match media.get("prettyName").and_then(Value::as_str) {
        Some(pretty) => path.replace("<prettyName>", pretty),
        None => path.to_string(),
    };

    let token = match media.get("token") {
        Some(Value::Array(tokens)) => tokens
            .first()
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty()),
        _ => None,
    };
    Ok(Some(match token {
        Some(token) => format!("{base}{path}?token={token}"),
        None => format!("{base}{path}"),
    }))
}
