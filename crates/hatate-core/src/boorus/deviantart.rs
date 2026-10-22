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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE: &str = "https://images-wixmp.example/f/uuid/name.jpg";
    const URL: &str = "https://www.deviantart.com/radprofile/art/A-Deviation-470555824";
    const RENDER: &str = "/v1/fill/w_800,h_1083,q_75,strp/<prettyName>-fullview.jpg";

    fn state(original: Value, render: &str, username: &str) -> Value {
        json!({"@@entities": {
            "deviationExtended": {"470555824": {"originalFile": original}},
            "deviation": {"470555824": {"author": 12605389, "media": {
                "baseUri": BASE,
                "prettyName": "a_deviation_by_radprofile",
                "token": ["signing-token"],
                "types": [
                    {"t": "150", "c": "/v1/fit/w_150,h_150/<prettyName>-150.jpg"},
                    {"t": "fullview", "c": render},
                ],
            }}},
            "user": {"12605389": {"username": username}},
        }})
    }

    /// A JSON document inside a JS string inside `JSON.parse(...)`, ending the line, with the
    /// `\'` escape the real page uses and JSON rejects, as `TestDeviantArtParser._page` builds it.
    fn page_of(state: &Value) -> String {
        let inner = serde_json::to_string(state).unwrap();
        let js = serde_json::to_string(&inner).unwrap().replace('\'', "\\'");
        format!(
            "<html><script>\nwindow.__REER__ = {{ init: function(n, d) {{ __REER__.push({{n,d}}); }} }};\n\
             window.__INITIAL_STATE__ = JSON.parse({js});\n</script></html>"
        )
    }

    fn png() -> Value {
        json!({"type": "png", "width": 2400, "height": 4000, "filesize": 4845515})
    }

    fn page() -> String {
        page_of(&state(png(), RENDER, "Rad'Profile"))
    }

    fn page_with(original: Value) -> String {
        page_of(&state(original, RENDER, "Rad'Profile"))
    }

    fn names(tags: &[Tag]) -> Vec<(Option<&str>, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref(), t.name.as_str()))
            .collect()
    }

    // tests/test_search_quality.py::TestDeviantArtParser::test_the_artist_becomes_a_tag
    #[test]
    fn the_artist_becomes_a_tag() {
        assert_eq!(
            names(&parse(&page(), URL)),
            [(Some("artist"), "Rad'Profile")]
        );
        // A space in the name is an underscore in the tag.
        let spaced = page_of(&state(png(), RENDER, "Rad Profile"));
        assert_eq!(
            names(&parse(&spaced, URL)),
            [(Some("artist"), "Rad_Profile")]
        );
    }

    // ...::test_the_original_file_size_and_format_are_reported
    #[test]
    fn the_original_file_size_and_format_are_reported() {
        assert_eq!(
            parse_file_info(&page(), URL),
            (Some("PNG".to_string()), Some(4_845_515))
        );
    }

    // ...::test_dimensions_are_the_originals_not_the_renders
    #[test]
    fn dimensions_are_the_originals_not_the_renders() {
        assert_eq!(parse_dimensions(&page(), URL), (Some(2400), Some(4000)));
    }

    // ...::test_the_render_is_a_preview_and_never_the_file
    #[test]
    fn the_render_is_a_preview_and_never_the_file() {
        let page = page();
        let preview = parse_preview_url(&page, URL).expect("a preview");
        assert!(preview.contains("/v1/fill/w_800"));
        assert!(preview.contains("a_deviation_by_radprofile")); // <prettyName> substituted
        assert!(preview.contains("token=signing-token"));
        assert_eq!(parse_file_url(&page, URL), None);
    }

    // ...::test_a_blurred_adult_render_is_flagged
    #[test]
    fn a_blurred_adult_render_is_flagged() {
        let blurred = "/v1/fill/w_1024,h_1707,q_80,strp,blur_73/<prettyName>-fullview.jpg";
        let page = page_of(&state(png(), blurred, "x"));
        let reason = parse_restriction(&page, URL).expect("flagged");
        assert!(reason.to_lowercase().contains("blurred"));
        assert!(reason.contains("Site Logins"));
    }

    // ...::test_a_normal_render_is_not_flagged
    #[test]
    fn a_normal_render_is_not_flagged_and_blur_must_be_a_path_parameter() {
        assert_eq!(parse_restriction(&page(), URL), None);
        for (path, flagged) in [
            ("/v1/fill/blur_1/x", true),
            ("/v1/fill,blur_12/x", true),
            ("/v1/fill/blurb_1/x", false),
            ("/v1/fill/blur_/x", false),
            ("/v1/fill/ablur_1/x", false),
        ] {
            let page = page_of(&state(png(), path, "x"));
            assert_eq!(parse_restriction(&page, URL).is_some(), flagged, "{path}");
        }
    }

    // ...::test_state_extraction_survives_inline_js_containing_a_paren (REGRESSION: matching to
    // the first ");" truncated a 427KB document at 13KB).
    #[test]
    fn state_extraction_survives_inline_js_containing_a_paren() {
        let page = page();
        assert!(page.contains("__REER__.push({n,d});")); // the decoy really is present
        assert_eq!(parse_dimensions(&page, URL), (Some(2400), Some(4000)));
        // The end of the LINE is the anchor, so a later line's ");" does not extend the match.
        let trailing = format!("{page}\nother(1);\n");
        assert_eq!(parse_dimensions(&trailing, URL), (Some(2400), Some(4000)));
    }

    // ...::test_missing_or_junk_original_file_is_reported_as_unknown
    #[test]
    fn missing_or_junk_original_file_is_reported_as_unknown() {
        for original in [
            json!({}),
            json!({"type": "png"}),
            json!({"type": "png", "filesize": 0}),
            json!({"type": "xyz", "filesize": "lots"}),
        ] {
            let page = page_with(original.clone());
            let (format, size) = parse_file_info(&page, URL);
            assert_eq!(size, None, "{original}");
            assert!(
                matches!(format.as_deref(), None | Some("PNG")),
                "{original}"
            );
            assert_eq!(parse_dimensions(&page, URL), (None, None), "{original}");
        }
    }

    // ...::test_a_page_without_state_yields_nothing_without_raising
    #[test]
    fn a_page_without_state_yields_nothing() {
        let html = "<!DOCTYPE html><html><body>no state here</body></html>";
        assert!(parse(html, URL).is_empty());
        assert_eq!(parse_file_url(html, URL), None);
        assert_eq!(parse_preview_url(html, URL), None);
        assert_eq!(parse_restriction(html, URL), None);
        assert_eq!(parse_dimensions(html, URL), (None, None));
        assert_eq!(parse_file_info(html, URL), (None, None));
    }

    // ...::test_parser_is_registered_for_both_domains
    #[test]
    fn the_parser_is_registered_for_both_domains() {
        for url in [
            "https://deviantart.com/view/1",
            "https://www.deviantart.com/a/art/b-1",
            "https://fav.me/d7s5n74",
        ] {
            assert!(matches(url), "{url}");
            assert_eq!(
                super::super::find_parser(url).map(|p| p.name),
                Some("deviantart")
            );
        }
        for url in [
            "https://deviantart.com.evil.test/x",
            "https://example.com/?u=fav.me",
            "",
        ] {
            assert!(!matches(url), "{url}");
        }
    }

    // Python keeps `\'` legal inside the JS string: JSON rejects it, so it is unwrapped first.
    #[test]
    fn the_js_only_apostrophe_escape_is_unwrapped_before_parsing() {
        let page = page();
        assert!(
            page.contains("\\'"),
            "fixture must contain a JS-only escape"
        );
        assert_eq!(parse(&page, URL).len(), 1);
    }

    #[test]
    fn a_state_that_does_not_parse_or_is_empty_is_no_state() {
        for raw in [
            "JSON.parse(\"{\");\n",
            "JSON.parse(\"\");\n",
            "JSON.parse(\"{}\");\n",
            "JSON.parse(\"[]\");\n",
            "JSON.parse(\"\\\"\\\"\");\n",
            // A string that is not a document, and a document that is not a string.
            "JSON.parse(\"x\");\n",
            "JSON.parse(5);\n",
            "JSON.parse(null);\n",
            "JSON.parse(\"[1]\");\n",
        ] {
            let page = format!(
                "window.__INITIAL_STATE__ = JSON.parse({}",
                &raw["JSON.parse(".len()..]
            );
            assert!(parse(&page, URL).is_empty(), "{raw}");
            assert_eq!(parse_dimensions(&page, URL), (None, None), "{raw}");
        }
    }

    #[test]
    fn the_state_line_may_use_any_python_whitespace() {
        // `\s` in Python counts U+001C..U+001F; the regex crate's `\s` does not.
        let inner =
            serde_json::to_string(&serde_json::to_string(&state(png(), RENDER, "a")).unwrap())
                .unwrap();
        for sep in [" ", "\u{1c}", "\u{a0}", "\t", ""] {
            let page = format!("window.__INITIAL_STATE__{sep}={sep}JSON.parse({inner});{sep}\n");
            assert_eq!(parse(&page, URL).len(), 1, "{sep:?}");
        }
        let crlf = format!("window.__INITIAL_STATE__ = JSON.parse({inner});\r\n");
        assert_eq!(parse(&crlf, URL).len(), 1);
        let unterminated = format!("window.__INITIAL_STATE__ = JSON.parse({inner});");
        assert_eq!(parse(&unterminated, URL).len(), 1);
        let junk_after = format!("window.__INITIAL_STATE__ = JSON.parse({inner}); x\n");
        assert!(parse(&junk_after, URL).is_empty());
    }

    #[test]
    fn the_deviation_is_picked_by_the_id_the_url_ends_in() {
        let mut two = state(png(), RENDER, "a");
        let entities = two["@@entities"].as_object_mut().unwrap();
        entities["deviationExtended"]["2"] =
            json!({"originalFile": {"type": "gif", "width": 1, "height": 2, "filesize": 3}});
        entities["deviation"]["2"] = entities["deviation"]["470555824"].clone();
        let page = page_of(&two);
        let pick = |url| parse_dimensions(&page, url);
        assert_eq!(pick("https://x/art/y-470555824"), (Some(2400), Some(4000)));
        assert_eq!(
            pick("https://x/art/y-470555824?ref=1"),
            (Some(2400), Some(4000))
        );
        assert_eq!(pick("https://x/view/2#c"), (Some(1), Some(2)));
        assert_eq!(pick("https://x/view/2\n"), (Some(1), Some(2)));
        // An id the state does not hold, or no id, with two deviations in it, is no guess.
        assert_eq!(pick("https://x/view/3"), (None, None));
        assert_eq!(pick("https://x/view/abc"), (None, None));
        assert_eq!(pick(""), (None, None));
        // With one, the URL need not say.
        let one = page_of(&state(png(), RENDER, "a"));
        assert_eq!(
            parse_dimensions(&one, "https://fav.me/d7s5n74"),
            (Some(2400), Some(4000))
        );
        assert_eq!(
            parse_dimensions(&one, "https://x/view/9"),
            (Some(2400), Some(4000))
        );
    }

    #[test]
    fn the_best_render_wins_and_a_later_entry_of_the_same_name_replaces_an_earlier() {
        let with_types = |types: Value| {
            let mut s = state(png(), RENDER, "a");
            s["@@entities"]["deviation"]["470555824"]["media"]["types"] = types;
            page_of(&s)
        };
        let preview = |types| parse_preview_url(&with_types(types), URL);
        assert_eq!(
            preview(json!([{"t": "300W", "c": "/w"}, {"t": "400T", "c": "/t"}, {"t": "preview", "c": "/p"}])).as_deref(),
            Some(&*format!("{BASE}/p?token=signing-token"))
        );
        assert_eq!(
            preview(json!([{"t": "fullview", "c": "/a"}, {"t": "fullview", "c": "/b"}])).as_deref(),
            Some(&*format!("{BASE}/b?token=signing-token"))
        );
        // Entries that are not objects are skipped; a chosen entry without a string path is none.
        assert_eq!(
            preview(json!([null, 1, "x", {"t": "350T", "c": "/q"}])).as_deref(),
            Some(&*format!("{BASE}/q?token=signing-token"))
        );
        assert_eq!(
            preview(json!([{"t": "fullview"}, {"t": "preview", "c": "/p"}])),
            None
        );
        assert_eq!(preview(json!([{"t": "150", "c": "/x"}])), None);
        assert_eq!(preview(json!([])), None);
        // A string or an object holds no entries; a number or `true` cannot be iterated; a `t`
        // that is a list or an object cannot be a key. Python raises on those three.
        assert_eq!(preview(json!("abc")), None);
        assert_eq!(preview(json!({"fullview": 1})), None);
        assert_eq!(preview(json!(5)), None);
        assert_eq!(preview(json!(true)), None);
        assert_eq!(preview(json!([{"t": ["fullview"], "c": "/a"}])), None);
        // Falsy values are the empty list.
        for falsy in [json!(0), json!(false), json!(null), json!("")] {
            assert_eq!(preview(falsy), None);
        }
    }

    #[test]
    fn the_token_is_the_first_one_and_only_a_non_empty_string() {
        let with_token = |token: Value| {
            let mut s = state(png(), "/r.jpg", "a");
            s["@@entities"]["deviation"]["470555824"]["media"]["token"] = token;
            page_of(&s)
        };
        let preview = |token| parse_preview_url(&with_token(token), URL).unwrap();
        assert_eq!(preview(json!(["a", "b"])), format!("{BASE}/r.jpg?token=a"));
        for none in [json!([]), json!([""]), json!([5]), json!("x"), json!(null)] {
            assert_eq!(preview(none), format!("{BASE}/r.jpg"));
        }
        // No `/v1/` in it, so the render is offered as the file.
        let page = with_token(json!(["a"]));
        assert_eq!(
            parse_file_url(&page, URL),
            Some(format!("{BASE}/r.jpg?token=a"))
        );
    }

    #[test]
    fn a_base_uri_must_be_an_absolute_url_and_a_pretty_name_a_string() {
        let with_media = |key: &str, value: Value| {
            let mut s = state(png(), RENDER, "a");
            s["@@entities"]["deviation"]["470555824"]["media"][key] = value;
            page_of(&s)
        };
        for base in [json!("//x/y"), json!("name.jpg"), json!(true), json!(null)] {
            assert_eq!(parse_preview_url(&with_media("baseUri", base), URL), None);
        }
        let http = parse_preview_url(&with_media("baseUri", json!("http://h/p")), URL);
        assert!(http.unwrap().starts_with("http://h/p/v1/fill/"));
        let unnamed = parse_preview_url(&with_media("prettyName", json!(5)), URL).unwrap();
        assert!(unnamed.contains("<prettyName>-fullview.jpg"), "{unnamed}");
    }

    #[test]
    fn format_names_follow_the_type_after_strip_lower_and_dots() {
        let format_of = |file_type: Value| {
            let page = page_with(json!({"type": file_type, "filesize": 5}));
            parse_file_info(&page, URL).0
        };
        for (raw, want) in [
            ("png", "PNG"),
            (" PNG ", "PNG"),
            (".jpg", "JPEG"),
            ("..WebP", "WEBP"),
            ("jpeg", "JPEG"),
            ("gif", "GIF"),
            ("bmp", "BMP"),
            ("avif", "AVIF"),
            ("mp4", "MP4"),
            ("webm", "WEBM"),
        ] {
            assert_eq!(format_of(json!(raw)).as_deref(), Some(want), "{raw}");
        }
        for odd in [
            json!("jpe"),
            json!("tiff"),
            json!(""),
            json!(5),
            json!(null),
            json!("p.ng"),
        ] {
            assert_eq!(format_of(odd.clone()), None, "{odd}");
        }
    }

    #[test]
    fn sizes_and_dimensions_are_read_as_python_int_reads_them() {
        let info =
            |filesize: Value| parse_file_info(&page_with(json!({"filesize": filesize})), URL).1;
        for (raw, want) in [
            (json!(4_845_515), Some(4_845_515)),
            (json!("9"), Some(9)),
            (json!(2.5), Some(2)),
            (json!(true), Some(1)),
            (json!(" 7 "), Some(7)),
            (json!("1_000"), Some(1000)),
            (json!("\u{663}"), Some(3)),
            (json!(0), None),
            (json!(-1), None),
            (json!("x"), None),
            (json!([]), None),
            (json!(null), None),
        ] {
            assert_eq!(info(raw.clone()), want, "{raw}");
        }
        let dims = |w: Value, h: Value| {
            parse_dimensions(&page_with(json!({"width": w, "height": h})), URL)
        };
        assert_eq!(dims(json!("7"), json!(8)), (Some(7), Some(8)));
        assert_eq!(dims(json!(2.9), json!("3")), (Some(2), Some(3)));
        // Both sides or neither; a bad side spoils both; a negative one is kept.
        assert_eq!(dims(json!(0), json!(5)), (None, None));
        assert_eq!(dims(json!(5), json!(null)), (None, None));
        assert_eq!(dims(json!("x"), json!(5)), (None, None));
        assert_eq!(dims(json!([1]), json!(5)), (None, None));
        assert_eq!(dims(json!(-3), json!(4)), (Some(-3), Some(4)));
    }

    #[test]
    fn the_artist_is_looked_up_by_the_author_id_as_python_prints_it() {
        let users = json!({
            "12605389": {"username": "int"},
            "12605389.0": {"username": "float"},
            "True": {"username": "bool"},
            "1e+16": {"username": "exp"},
        });
        let artist = |author: Value| {
            let mut s = state(png(), RENDER, "unused");
            s["@@entities"]["deviation"]["470555824"]["author"] = author;
            s["@@entities"]["user"] = users.clone();
            names(&parse(&page_of(&s), URL))
                .into_iter()
                .map(|(_, name)| name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(artist(json!(12_605_389)), ["int"]);
        assert_eq!(artist(json!("12605389")), ["int"]);
        assert_eq!(artist(json!(12_605_389.0)), ["float"]);
        assert_eq!(artist(json!(true)), ["bool"]);
        assert_eq!(artist(json!(1e16)), ["exp"]);
        assert!(artist(json!(null)).is_empty());
        assert!(artist(json!(7)).is_empty());
        assert!(artist(json!([1])).is_empty());
    }

    #[test]
    fn a_username_that_is_not_a_string_makes_no_tag() {
        for username in [
            json!(null),
            json!(""),
            json!(5),
            json!(["a"]),
            json!(true),
            json!({}),
        ] {
            let page = page_of(&{
                let mut s = state(png(), RENDER, "x");
                s["@@entities"]["user"]["12605389"]["username"] = username.clone();
                s
            });
            assert!(parse(&page, URL).is_empty(), "{username}");
        }
    }

    // Where Python raises, the hook has nothing to say.
    #[test]
    fn a_state_of_an_unexpected_shape_is_nothing_where_python_raises() {
        let broken = |edit: fn(&mut Value)| {
            let mut s = state(png(), RENDER, "a");
            edit(&mut s);
            page_of(&s)
        };
        let pages = [
            // `deviation` is a list, `deviation[key]` is a string, the user entry is a list.
            broken(|s| s["@@entities"]["deviation"] = json!([1])),
            broken(|s| s["@@entities"]["deviation"]["470555824"] = json!("x")),
            broken(|s| s["@@entities"]["user"]["12605389"] = json!([1])),
            // `deviationExtended[key]` is a string: only the hooks that read it raise.
            broken(|s| s["@@entities"]["deviationExtended"]["470555824"] = json!("x")),
        ];
        for (i, page) in pages.iter().enumerate() {
            assert_eq!(parse_dimensions(page, URL), (None, None), "{i}");
            assert_eq!(parse_file_info(page, URL), (None, None), "{i}");
        }
        for page in &pages[..3] {
            assert!(parse(page, URL).is_empty());
            assert_eq!(parse_preview_url(page, URL), None);
            assert_eq!(parse_restriction(page, URL), None);
        }
        // `parse` and the render never read `deviationExtended`, so a bad entry does not hurt them.
        assert_eq!(parse(&pages[3], URL).len(), 1);
        assert!(parse_preview_url(&pages[3], URL).is_some());
    }

    #[test]
    fn a_missing_or_falsy_entry_is_an_empty_one_and_not_an_error() {
        let with = |edit: fn(&mut Value)| {
            let mut s = state(png(), RENDER, "a");
            edit(&mut s);
            page_of(&s)
        };
        // `deviation` absent or falsy: no artist and no render, but the original is still there.
        for edit in [
            (|s: &mut Value| {
                s["@@entities"].as_object_mut().unwrap().remove("deviation");
            }) as fn(&mut Value),
            |s| s["@@entities"]["deviation"] = json!(null),
            |s| s["@@entities"]["deviation"] = json!({}),
            |s| s["@@entities"]["deviation"]["470555824"] = json!(0),
        ] {
            let page = with(edit);
            assert!(parse(&page, URL).is_empty());
            assert_eq!(parse_preview_url(&page, URL), None);
            assert_eq!(parse_dimensions(&page, URL), (Some(2400), Some(4000)));
        }
        // `deviationExtended` empty or not a table: the page has no deviation at all.
        for edit in [
            (|s: &mut Value| s["@@entities"]["deviationExtended"] = json!({})) as fn(&mut Value),
            |s| s["@@entities"]["deviationExtended"] = json!([1]),
            |s| s["@@entities"] = json!([1]),
            |s| s["@@entities"] = json!(null),
        ] {
            let page = with(edit);
            assert!(parse(&page, URL).is_empty());
            assert_eq!(parse_dimensions(&page, URL), (None, None));
        }
        // `originalFile` is not an object: unknown.
        assert_eq!(
            parse_file_info(
                &with(
                    |s| s["@@entities"]["deviationExtended"]["470555824"]["originalFile"] =
                        json!([1])
                ),
                URL
            ),
            (None, None)
        );
    }
}
