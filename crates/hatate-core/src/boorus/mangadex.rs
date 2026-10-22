//! MangaDex chapters (`core/boorus/mangadex.py`).
//!
//! MangaDex was classification-only for a long time, because a chapter URL points at many images
//! rather than one. That stopped being true when multi-page support arrived: "which of these
//! images is the one I have?" is what the multi-page resolver answers, by hashing each page
//! against the local file. A chapter is a multi-page post like any other.
//!
//! Everything comes from the public API at `api.mangadex.org` rather than the site's HTML, which
//! is a JS front end with nothing useful in the served markup. `/at-home/server/{chapterId}`
//! returns the node serving that chapter plus its page list, in two resolutions:
//!
//! - `data`: the original scans;
//! - `dataSaver`: the same pages recompressed, a fraction of the size.
//!
//! The saver images are what get hashed to find the matching page; the original is what the
//! match then points at. The URLs are short-lived (the `baseUrl` is an assignment to one node of
//! the MangaDex@Home network, good for a few minutes), and chapters disappear often.
//!
//! No tags come from here: series, volume, chapter and page are applied by the Hydrus importer.
//!
//! Every hook is a pure function of the body and the post-page URL; MangaDex keeps no state.
//!
//! Python formats `baseUrl` and the chapter `hash` into the URLs with an f-string, so a number
//! there would be printed. The API sends strings, and here anything else means no pages. A
//! `dataSaver` that is neither a list nor absent is read the way Python indexes it (a string by
//! character; an object or a number makes Python raise, and a hook that raises has nothing to
//! say).

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use tracing::{debug, warn};

use super::parsed::json_of;
use super::{Page, SiteParser};
use crate::model::Tag;
use crate::{pycompat, pyurl};

pub const API_BASE: &str = "https://api.mangadex.org";

/// 1: stopped emitting series/volume/chapter/page. Entries cached while this parser still
/// produced them restore with `booru_tags_fetched=False`, so the tags are dropped on next use
/// rather than served from disk forever.
pub const TAG_VERSION: u32 = 1;

/// MangaDex identifies everything by UUID. Anchored to the `/chapter/` route specifically: a
/// `/title/` URL is a whole manga, which has no page list of its own and is left
/// classification-only.
static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"/chapter/([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})",
    )
    .expect("chapter regex")
});

/// What the registry knows about this site. This parser never returns tags, so an empty result is
/// normal (`EMPTY_RESULT_IS_NORMAL`); without it every match would log a "markup may have
/// changed" warning and the app would call the parser broken after three.
pub static PARSER: SiteParser = SiteParser {
    empty_result_is_normal: true,
    resolve_fetch_url: Some(resolve_fetch_url),
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_page_count: Some(|body, url, _| parse_page_count(body, url)),
    parse_pages: Some(|body, url, _| parse_pages(body, url)),
    pages_api_url: Some(pages_api_url),
    ..SiteParser::new("mangadex", matches, |body, url, _| parse(body, url))
};

/// A chapter URL on mangadex.org or a subdomain. The host is the URL's whole `netloc`, as
/// Python's `urlparse(url).netloc` reads it, so a port or a `user@` prefix is not claimed.
pub fn matches(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    let host = pyurl::urlparse(url, "").netloc.to_lowercase();
    (host == "mangadex.org" || host.ends_with(".mangadex.org")) && CHAPTER_RE.is_match(url)
}

pub fn chapter_id(url: &str) -> Option<String> {
    CHAPTER_RE.captures(url).map(|found| found[1].to_string())
}

/// Fetch the API's page list instead of the chapter page, which is a JS front end with nothing in
/// its markup.
pub fn resolve_fetch_url(url: &str) -> String {
    match chapter_id(url) {
        Some(id) => format!("{API_BASE}/at-home/server/{id}"),
        None => {
            debug!("No chapter id in {url}, fetching it as-is");
            url.to_string()
        }
    }
}

/// Where the page list lives, for the multi-page resolver: the same response the fetch above
/// already asked for, since it carries every page.
pub fn pages_api_url(url: &str) -> Option<String> {
    chapter_id(url).map(|id| format!("{API_BASE}/at-home/server/{id}"))
}

/// One page of the chapter: the original and, where the API listed one, its saver image.
struct PageUrls {
    original: String,
    saver: Option<String>,
}

/// `dataSaver[index]` as Python evaluates `index < len(savers) and savers[index]`, for a value
/// that is truthy; `Err` is where Python raises.
fn saver_at(savers: &Value, chars: &[char], index: usize) -> Result<Option<String>, ()> {
    match savers {
        Value::Null => Ok(None),
        Value::Array(items) => Ok(items
            .get(index)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)),
        Value::String(_) => Ok(chars.get(index).map(char::to_string)),
        // `len()` works, and `savers[index]` is a KeyError wherever it is reached.
        Value::Object(items) if index >= items.len() => Ok(None),
        // `len()` of a number is a TypeError.
        _ => Err(()),
    }
}

/// A string field that is present and not empty (`not base` is false for it).
fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `_page_urls`: the pages in order, or none if the response is not one we understand.
fn page_urls(body: &str) -> Vec<PageUrls> {
    let data = match json_of(body) {
        Ok(Value::Object(data)) => data,
        Ok(_) => return Vec::new(),
        Err(_) => {
            warn!("MangaDex response was not valid JSON (the API may have changed)");
            return Vec::new();
        }
    };
    if data.get("result").and_then(Value::as_str) != Some("ok") {
        return Vec::new();
    }
    let (Some(base), Some(Value::Object(chapter))) =
        (non_empty(data.get("baseUrl")), data.get("chapter"))
    else {
        return Vec::new();
    };
    let Some(chapter_hash) = non_empty(chapter.get("hash")) else {
        return Vec::new();
    };

    let truthy = |key: &str| chapter.get(key).filter(|v| pycompat::truthy(v));
    let originals = match truthy("data") {
        None => &[][..],
        Some(Value::Array(items)) => items,
        Some(_) => return Vec::new(),
    };
    let savers = truthy("dataSaver").unwrap_or(&Value::Null);
    let saver_chars: Vec<char> = savers.as_str().unwrap_or("").chars().collect();

    let mut pages = Vec::new();
    for (index, name) in originals.iter().enumerate() {
        let Some(name) = name.as_str().filter(|n| !n.is_empty()) else {
            continue;
        };
        // The two lists are parallel, but only pair them where both really have an entry: a
        // mismatched length would otherwise attach the wrong page's saver image to a page,
        // which is precisely the mistake this whole mechanism exists to avoid.
        let Ok(saver) = saver_at(savers, &saver_chars, index) else {
            return Vec::new();
        };
        pages.push(PageUrls {
            original: format!("{base}/data/{chapter_hash}/{name}"),
            saver: saver.map(|s| format!("{base}/data-saver/{chapter_hash}/{s}")),
        });
    }
    pages
}

/// The page list in the shape the multi-page resolver expects.
///
/// No width or height: the API does not report them. Absent is correct rather than guessed, as
/// the resolver treats unknown dimensions as "no evidence" and falls back to hashing, whereas
/// invented ones would be evidence pointing at the wrong page.
pub fn parse_pages(body: &str, _url: &str) -> Vec<Page> {
    page_urls(body)
        .into_iter()
        .map(|page| {
            let saver = page.saver.unwrap_or_else(|| page.original.clone());
            let mut entry = Map::new();
            entry.insert(
                "urls".into(),
                // "regular" and "small" are both the saver image: it is the only smaller size
                // MangaDex offers, and it is what both the preview and the page hashing want.
                json!({"original": page.original, "regular": saver, "small": saver}),
            );
            entry.insert("width".into(), Value::Null);
            entry.insert("height".into(), Value::Null);
            entry
        })
        .collect()
}

pub fn parse_page_count(body: &str, _url: &str) -> i64 {
    i64::try_from(page_urls(body).len())
        .unwrap_or(i64::MAX)
        .max(1)
}

/// The first page's original. Which page actually matched is settled later by the multi-page
/// resolver; this is the honest default until then, and the one used when a chapter really is a
/// single page.
pub fn parse_file_url(body: &str, _url: &str) -> Option<String> {
    page_urls(body).into_iter().next().map(|page| page.original)
}

/// The first page's saver image: a fraction of the original's size and plenty for the preview
/// panel.
pub fn parse_preview_url(body: &str, _url: &str) -> Option<String> {
    page_urls(body)
        .into_iter()
        .next()
        .map(|page| page.saver.unwrap_or(page.original))
}

/// No tags; see the note at the top of this module.
pub fn parse(_body: &str, _url: &str) -> Vec<Tag> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boorus::find_parser;
    use crate::cache::tag_version_for;

    const URL: &str = "https://mangadex.org/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f/";

    /// `tests/test_parsers_and_tags.py::MANGADEX_AT_HOME`.
    const AT_HOME: &str = r#"{
        "result": "ok",
        "baseUrl": "https://node1.mangadex.network",
        "chapter": {
            "hash": "abc123hash",
            "data": ["1-aaa.png", "2-bbb.png", "3-ccc.png"],
            "dataSaver": ["1-xxx.jpg", "2-yyy.jpg", "3-zzz.jpg"]
        }
    }"#;

    fn body(base: &str, hash: &str, data: &str, saver: &str) -> String {
        format!(
            r#"{{"result":"ok","baseUrl":{base},"chapter":{{"hash":{hash},"data":{data},"dataSaver":{saver}}}}}"#
        )
    }

    // ---- tests/test_parsers_and_tags.py::TestMangadexParser

    #[test]
    fn it_claims_chapter_urls() {
        assert!(matches(URL));
        assert!(matches(URL.trim_end_matches('/')));
    }

    #[test]
    fn a_title_url_is_left_alone() {
        assert!(!matches(
            "https://mangadex.org/title/2f1d73c5-b3c9-450a-bc5f-fa702940c36f/some-manga"
        ));
    }

    #[test]
    fn other_sites_are_not_claimed() {
        assert!(!matches("https://danbooru.donmai.us/posts/1"));
        assert!(!matches(
            "https://notmangadex.org/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        ));
        assert!(!matches(""));
    }

    #[test]
    fn the_registry_routes_chapters_here() {
        assert_eq!(find_parser(URL).map(|p| p.name), Some("mangadex"));
        // tests/test_parsers_and_tags.py::test_mangadex_has_no_content_parser: "abc" is no UUID.
        assert!(find_parser("https://mangadex.org/chapter/abc/").is_none());
    }

    #[test]
    fn it_fetches_the_api_not_the_page() {
        assert_eq!(
            resolve_fetch_url(URL),
            "https://api.mangadex.org/at-home/server/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        );
        assert_eq!(
            resolve_fetch_url("https://mangadex.org/title/x"),
            "https://mangadex.org/title/x"
        );
    }

    #[test]
    fn the_page_list_url_is_the_same_response() {
        assert_eq!(pages_api_url(URL).unwrap(), resolve_fetch_url(URL));
        assert_eq!(pages_api_url("https://mangadex.org/title/x"), None);
    }

    #[test]
    fn page_count() {
        assert_eq!(parse_page_count(AT_HOME, URL), 3);
    }

    #[test]
    fn the_file_url_is_the_full_resolution_page() {
        assert_eq!(
            parse_file_url(AT_HOME, URL).as_deref(),
            Some("https://node1.mangadex.network/data/abc123hash/1-aaa.png")
        );
    }

    #[test]
    fn the_preview_is_the_saver_image() {
        assert_eq!(
            parse_preview_url(AT_HOME, URL).as_deref(),
            Some("https://node1.mangadex.network/data-saver/abc123hash/1-xxx.jpg")
        );
    }

    #[test]
    fn pages_carry_both_sizes_in_order() {
        let pages = parse_pages(AT_HOME, URL);
        assert_eq!(pages.len(), 3);
        assert_eq!(
            pages[2]["urls"]["original"],
            "https://node1.mangadex.network/data/abc123hash/3-ccc.png"
        );
        // "small" is what the resolver hashes, so it must be the cheap one.
        assert_eq!(
            pages[2]["urls"]["small"],
            "https://node1.mangadex.network/data-saver/abc123hash/3-zzz.jpg"
        );
        assert_eq!(pages[2]["urls"]["regular"], pages[2]["urls"]["small"]);
    }

    #[test]
    fn dimensions_are_absent_rather_than_guessed() {
        let page = &parse_pages(AT_HOME, URL)[0];
        assert!(page["width"].is_null());
        assert!(page["height"].is_null());
    }

    #[test]
    fn a_missing_saver_falls_back_to_the_original() {
        let b = body(
            r#""https://n.example""#,
            r#""h""#,
            r#"["1-a.png","2-b.png"]"#,
            r#"["1-x.jpg"]"#,
        );
        let pages = parse_pages(&b, URL);
        assert_eq!(
            pages[1]["urls"]["small"],
            "https://n.example/data/h/2-b.png"
        );
        assert_eq!(
            pages[0]["urls"]["small"],
            "https://n.example/data-saver/h/1-x.jpg"
        );
        // ...and the preview of a single such page is its original.
        let one = body(r#""https://n.example""#, r#""h""#, r#"["1-a.png"]"#, "[]");
        assert_eq!(
            parse_preview_url(&one, URL).as_deref(),
            Some("https://n.example/data/h/1-a.png")
        );
    }

    #[test]
    fn an_error_response_yields_nothing() {
        let b = r#"{"result": "error", "errors": [{"status": 404}]}"#;
        assert!(parse_pages(b, URL).is_empty());
        assert_eq!(parse_file_url(b, URL), None);
        assert_eq!(parse_page_count(b, URL), 1);
    }

    #[test]
    fn no_tags_are_generated() {
        assert!(parse(AT_HOME, URL).is_empty());
    }

    #[test]
    fn returning_no_tags_is_not_treated_as_a_broken_parser() {
        assert!(PARSER.empty_result_is_normal);
    }

    #[test]
    fn junk_is_survived() {
        for b in [
            "",
            "not json",
            "[]",
            r#"{"result":"ok"}"#,
            r#"{"result":"ok","baseUrl":"x","chapter":{}}"#,
        ] {
            assert!(parse_pages(b, URL).is_empty(), "{b}");
            assert_eq!(parse_file_url(b, URL), None, "{b}");
            assert_eq!(parse_preview_url(b, URL), None, "{b}");
            assert_eq!(parse_page_count(b, URL), 1, "{b}");
        }
    }

    // ---- tests/test_parsers_and_tags.py::TestPerParserTagInvalidation (the parser's half)

    #[test]
    fn the_cache_reads_the_same_tag_version_and_the_same_urls() {
        assert_eq!(i64::from(TAG_VERSION), tag_version_for(URL));
        for url in [
            URL,
            "https://mangadex.org/title/2f1d73c5-b3c9-450a-bc5f-fa702940c36f/x",
            "https://MANGADEX.ORG/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f",
            "https://mangadex.org:443/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f",
            "https://notmangadex.org/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f",
        ] {
            assert_eq!(matches(url), tag_version_for(url) == 1, "{url}");
        }
    }

    // ---- tests/test_boorus_harness.py::test_every_row (construction half)

    #[test]
    fn the_host_is_the_whole_netloc() {
        assert!(matches(
            "https://www.mangadex.org/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        ));
        assert!(!matches(
            "https://mangadex.org:443/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        ));
        assert!(!matches(
            "https://user@mangadex.org/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        ));
        assert!(!matches(
            "https://mangadex.org/title/x?r=/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36"
        ));
        // Nothing but the netloc is checked for the host; the id can be anywhere in the URL.
        assert!(matches(
            "https://mangadex.org/x?u=/chapter/2f1d73c5-b3c9-450a-bc5f-fa702940c36f"
        ));
    }

    // ---- what the differential against Python found worth pinning

    #[test]
    fn a_page_without_a_name_keeps_its_place_in_the_saver_list() {
        let b = body(
            r#""b""#,
            r#""h""#,
            r#"["", "b", 7, "d"]"#,
            r#"["w","x","y","z"]"#,
        );
        let urls: Vec<_> = parse_pages(&b, URL)
            .iter()
            .map(|p| p["urls"]["small"].clone())
            .collect();
        assert_eq!(urls, ["b/data-saver/h/x", "b/data-saver/h/z"]);
    }

    #[test]
    fn a_saver_that_is_a_string_is_indexed_by_character() {
        let b = body(r#""b""#, r#""h""#, r#"["a","c"]"#, r#""xy""#);
        let urls: Vec<_> = parse_pages(&b, URL)
            .iter()
            .map(|p| p["urls"]["small"].clone())
            .collect();
        assert_eq!(urls, ["b/data-saver/h/x", "b/data-saver/h/y"]);
    }

    #[test]
    fn a_saver_python_cannot_index_leaves_no_pages() {
        for saver in ["5", "true", r#"{"a":1}"#] {
            let b = body(r#""b""#, r#""h""#, r#"["a","c"]"#, saver);
            assert!(parse_pages(&b, URL).is_empty(), "{saver}");
            assert_eq!(parse_page_count(&b, URL), 1, "{saver}");
        }
        // An object shorter than the page list is only indexed where it is long enough.
        let b = body(r#""b""#, r#""h""#, "[]", r#"{"a":1}"#);
        assert!(parse_pages(&b, URL).is_empty());
    }

    #[test]
    fn a_base_or_hash_that_is_not_a_string_leaves_no_pages() {
        for (base, hash) in [
            ("5", r#""h""#),
            ("true", r#""h""#),
            (r#""b""#, "12"),
            (r#""""#, r#""h""#),
        ] {
            let b = body(base, hash, r#"["a"]"#, r#"["s"]"#);
            assert!(parse_pages(&b, URL).is_empty(), "{base} {hash}");
        }
    }
}
