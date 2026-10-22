//! Booru page parsers (`core/boorus/__init__.py` and its shared helpers).
//!
//! Each parser turns a booru's page, or its API's response, into a list of namespaced
//! [`Tag`]s plus whatever else the page states: the direct URL of the full-resolution file (as
//! opposed to the booru's viewer page, which serves HTML and would give wrong answers for
//! anything that expects an image), a mid-size preview, the original's dimensions, rating, format
//! and byte size, and whether the post is gated or gone.
//!
//! **Pattern for a site** (the long form is in `docs/parsers.md`): one module per site, named
//! after the Python module; plain `pub fn`s that take the body and the post-page URL and return
//! owned values; no I/O and no globals. State that Python keeps in module globals (the Data-API
//! post that the Gelbooru family memoises, Pawchive's creator-name cache and remote image sizes)
//! is an explicit parameter, carried in [`Context`]. The module exports a
//! `static PARSER: SiteParser` naming the hooks it defines, and is added to [`PARSERS`] in
//! Python's order.
//!
//! What stays out: `fetch_page_info`'s HTTP, retry and cookie handling, the
//! file-hash LRU (`local_md5`) and `_remote_size.py`, which opens a socket. They belong with the
//! engine clients. This module has the part of `fetch_page_info` that runs after the body is in
//! hand: [`page_info_from_body`].

pub mod animepictures;
pub mod danbooru;
pub mod deviantart;
pub mod e621;
pub mod eshuushuu;
pub mod gelbooru;
pub mod host;
pub mod joyreactor;
mod json_fields;
pub mod mangadex;
pub mod moebooru;
pub mod paheal;
pub mod parsed;
pub mod pawchive;
pub mod pixiv;
pub mod rating;
pub mod redditpost;
pub mod rule34;
pub mod rule34us;
pub mod safebooru;
pub mod sankaku;
pub mod sizes;
pub mod twitter;
pub mod xbooru;
pub mod zerochan;

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Map, Value};
use tracing::{debug, info, warn};

use crate::model::Tag;

/// Mirrors the bound used for availability checks: far enough to reach a deletion notice, never
/// the whole page.
pub const SOFT_404_SCAN_BYTES: usize = 60_000;

/// The state a parser reads that is not in its arguments. Python keeps this in module globals;
/// the corpus records it as `context` in `call.json`. A field is added by the ticket of the first
/// site that needs it, together with its key in the runner's mapping.
///
/// The fetch that fills a field is not here (it opens a socket, so it belongs with the engine
/// clients): whoever builds the context has already done it, and a parser that finds its entry
/// missing answers as Python does when the same request fails.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Context {
    /// The Gelbooru family's Data-API post for the URL being parsed (Python's `_last_api_post`,
    /// the single memoised `(url, post)` the fetch fills). `None` means "the API gave nothing",
    /// which sends every hook down its HTML path. Whoever builds the context has already
    /// matched the post to the URL; the parsers do not check.
    pub api_post: Option<Map<String, Value>>,
    /// Pawchive's creator names already looked up (Python's `pawchive._artist_cache`), keyed
    /// `"{service}/{user}"`. `""` is a remembered miss: the profile was asked for and gave no
    /// name, so there is no artist tag and no second request.
    pub artist_cache: HashMap<String, String>,
    /// Image sizes already read from the head of a remote file (Python's
    /// `_remote_size._cache`), keyed by the file's URL. `(None, None)` is a remembered "this
    /// file's size cannot be read from its start".
    pub remote_image_sizes: HashMap<String, (Option<i64>, Option<i64>)>,
}

impl Context {
    /// A context carrying the Gelbooru family's Data-API post, or `None` for "the API gave
    /// nothing".
    pub fn with_api_post(api_post: Option<Map<String, Value>>) -> Context {
        Context {
            api_post,
            ..Context::default()
        }
    }

    /// A context carrying Pawchive's creator-name cache.
    pub fn with_artist_cache(artist_cache: HashMap<String, String>) -> Context {
        Context {
            artist_cache,
            ..Context::default()
        }
    }

    /// Remembers a creator lookup as Python's `_artist_name` does: the stripped name, or `""`
    /// for "no name", which stops the same creator being asked about again.
    pub fn remember_artist(&mut self, key: &str, name: Option<&str>) {
        let name = name.map(crate::pycompat::strip).unwrap_or_default();
        self.artist_cache.insert(key.to_string(), name.to_string());
    }

    /// Remembers an image size read from a remote file. A network failure is not remembered
    /// (a timeout says nothing about the file); a file whose size cannot be read from its start
    /// is, as `(None, None)`.
    pub fn remember_image_size(&mut self, url: &str, size: (Option<i64>, Option<i64>)) {
        self.remote_image_sizes.insert(url.to_string(), size);
    }
}

/// What `resolve_fetch_url_with_context` is given: the one thing a site with dead URLs needs to
/// look a post up by. `local_md5` is computed only when a parser asks for it.
#[derive(Debug, Clone, Default)]
pub struct FetchContext {
    pub local_md5: Option<String>,
}

/// One entry of `parse_pages`: the Python hook returns a list of dicts, one per image.
pub type Page = Map<String, Value>;

pub type MatchesFn = fn(&str) -> bool;
pub type ParseFn = fn(&str, &str, &Context) -> Vec<Tag>;
pub type StringHook = fn(&str, &str, &Context) -> Option<String>;
pub type DimensionsHook = fn(&str, &str, &Context) -> (Option<i64>, Option<i64>);
pub type FileInfoHook = fn(&str, &str, &Context) -> (Option<String>, Option<i64>);
pub type PageCountHook = fn(&str, &str, &Context) -> i64;
pub type PagesHook = fn(&str, &str, &Context) -> Vec<Page>;
pub type StatusHook = fn(u16, &str, &str, &Context) -> Option<String>;

/// What a site module provides: `matches` and `parse` are required, every other hook is optional
/// and is `None` when the Python module does not define it. This is the `SiteParser` protocol
/// and `OPTIONAL_PARSER_HOOKS` of Python, as a struct, so that a hook with a misspelled name is
/// a compile error and not a hook that silently never runs.
#[derive(Clone, Copy)]
pub struct SiteParser {
    /// The Python module's short name, as the corpus and the log call it.
    pub name: &'static str,
    /// `EMPTY_RESULT_IS_NORMAL`: for a site that finds posts by file hash, a miss just means the
    /// local file is not the site's copy, which is not worth a warning.
    pub empty_result_is_normal: bool,
    pub matches: MatchesFn,
    pub parse: ParseFn,
    pub resolve_fetch_url: Option<fn(&str) -> String>,
    pub resolve_fetch_url_with_context: Option<fn(&str, &FetchContext) -> String>,
    pub parse_file_url: Option<StringHook>,
    pub parse_preview_url: Option<StringHook>,
    pub parse_dimensions: Option<DimensionsHook>,
    pub parse_rating: Option<StringHook>,
    pub parse_file_info: Option<FileInfoHook>,
    pub parse_page_count: Option<PageCountHook>,
    pub parse_pages: Option<PagesHook>,
    pub pages_api_url: Option<fn(&str) -> Option<String>>,
    pub parse_restriction: Option<StringHook>,
    pub restriction_for_status: Option<StatusHook>,
    pub parse_canonical_url: Option<StringHook>,
    pub incomplete_reason: Option<StringHook>,
}

impl SiteParser {
    /// A parser with only the two required hooks; add the rest with struct-update syntax.
    pub const fn new(name: &'static str, matches: MatchesFn, parse: ParseFn) -> SiteParser {
        SiteParser {
            name,
            empty_result_is_normal: false,
            matches,
            parse,
            resolve_fetch_url: None,
            resolve_fetch_url_with_context: None,
            parse_file_url: None,
            parse_preview_url: None,
            parse_dimensions: None,
            parse_rating: None,
            parse_file_info: None,
            parse_page_count: None,
            parse_pages: None,
            pages_api_url: None,
            parse_restriction: None,
            restriction_for_status: None,
            parse_canonical_url: None,
            incomplete_reason: None,
        }
    }
}

/// The optional hooks, as `(name, what it is for)`: `OPTIONAL_PARSER_HOOKS`.
pub const OPTIONAL_PARSER_HOOKS: [(&str, &str); 14] = [
    (
        "resolve_fetch_url",
        "fetch something other than the URL itself (an API, usually)",
    ),
    (
        "resolve_fetch_url_with_context",
        "same, when the choice needs the local file's hash",
    ),
    ("parse_file_url", "the full-resolution image"),
    ("parse_preview_url", "a mid-size sample image"),
    ("parse_dimensions", "width and height of the original"),
    (
        "parse_rating",
        "the post's age rating, where the site states one",
    ),
    ("parse_file_info", "format and byte size of the original"),
    ("parse_page_count", "how many images the post holds"),
    ("parse_pages", "the per-page URLs, for multi-page posts"),
    ("pages_api_url", "where that page list is fetched from"),
    (
        "parse_restriction",
        "the post exists but the site won't serve it",
    ),
    (
        "restriction_for_status",
        "the same, recognised from an HTTP status",
    ),
    (
        "parse_canonical_url",
        "where a legacy URL actually lives now",
    ),
    (
        "incomplete_reason",
        "why a match came back without its tags or its file",
    ),
];

impl SiteParser {
    /// Which optional hooks this parser defines, by `OPTIONAL_PARSER_HOOKS` name.
    pub fn defined_hooks(&self) -> Vec<&'static str> {
        let defined = [
            self.resolve_fetch_url.is_some(),
            self.resolve_fetch_url_with_context.is_some(),
            self.parse_file_url.is_some(),
            self.parse_preview_url.is_some(),
            self.parse_dimensions.is_some(),
            self.parse_rating.is_some(),
            self.parse_file_info.is_some(),
            self.parse_page_count.is_some(),
            self.parse_pages.is_some(),
            self.pages_api_url.is_some(),
            self.parse_restriction.is_some(),
            self.restriction_for_status.is_some(),
            self.parse_canonical_url.is_some(),
            self.incomplete_reason.is_some(),
        ];
        OPTIONAL_PARSER_HOOKS
            .iter()
            .zip(defined)
            .filter_map(|((name, _), is_defined)| is_defined.then_some(*name))
            .collect()
    }
}

/// Every site, in the order Python's `PARSERS` lists them: the first whose `matches` claims a
/// URL wins. A site that has no Rust port yet is its line, commented out, at its place.
pub static PARSERS: &[&SiteParser] = &[
    &danbooru::PARSER,
    &gelbooru::PARSER,
    &safebooru::PARSER,
    &moebooru::PARSER,
    &pixiv::PARSER,
    &e621::PARSER,
    &zerochan::PARSER,
    &eshuushuu::PARSER,
    &animepictures::PARSER,
    &rule34::PARSER,
    &xbooru::PARSER,
    &paheal::PARSER,
    &rule34us::PARSER,
    &redditpost::PARSER,
    &deviantart::PARSER,
    &pawchive::PARSER,
    &sankaku::PARSER,
    &twitter::PARSER,
    &mangadex::PARSER,
    &joyreactor::PARSER,
];

/// The parser that claims `url`, if any.
pub fn find_parser(url: &str) -> Option<&'static SiteParser> {
    PARSERS.iter().copied().find(|p| (p.matches)(url))
}

/// `BooruError` and `BooruContentGoneError`. Raised by the fetch, which is not here yet; the
/// type is, so the corpus runner's error-variant map and the engine clients agree on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooruError {
    /// The fetch did not work (a timeout, a rate limit, a server error, an unexpected status).
    Fetch(String),
    /// The post itself is definitively gone (HTTP 404/410), as opposed to a transient failure.
    /// Python subclasses `BooruError` so handlers that only want "the fetch didn't work" keep
    /// working; match both variants for that.
    ContentGone(String),
}

impl std::fmt::Display for BooruError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BooruError::Fetch(msg) | BooruError::ContentGone(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for BooruError {}

/// What a fetch of a booru post page yields (`BooruPageInfo`). See the Python class for why each
/// field is kept apart from the others; the ones that are easy to conflate:
///
/// - `rating` stays out of `tags` on purpose: whether it becomes a tag at all is the user's
///   setting, and parser health judges a parser by the tags it scraped.
/// - `restricted` (the post is gated behind an account tier) is not `gone_reason` (it is
///   deleted) and not `incomplete_reason` (it is fine and reachable, this app just cannot get its
///   tags).
/// - `page_count > 1` means `file_url`, `preview_url` and the dimensions describe only the FIRST
///   image, so the caller has to establish which one matched.
/// - `fetched` is true only if a parser actually fetched the page. False means no parser matched
///   the site, which is not the same as "fetched and found nothing".
#[derive(Debug, Clone, PartialEq)]
pub struct BooruPageInfo {
    pub tags: Vec<Tag>,
    pub file_url: Option<String>,
    pub preview_url: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub file_format: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub rating: Option<String>,
    pub gone_reason: Option<String>,
    pub page_count: i64,
    pub incomplete_reason: Option<String>,
    pub restricted: Option<String>,
    pub canonical_url: Option<String>,
    pub fetched_url: Option<String>,
    pub body: Option<String>,
    pub fetched: bool,
}

impl Default for BooruPageInfo {
    fn default() -> Self {
        BooruPageInfo {
            tags: Vec::new(),
            file_url: None,
            preview_url: None,
            width: None,
            height: None,
            file_format: None,
            file_size_bytes: None,
            rating: None,
            gone_reason: None,
            page_count: 1,
            incomplete_reason: None,
            restricted: None,
            canonical_url: None,
            fetched_url: None,
            body: None,
            fetched: false,
        }
    }
}

/// The first of `markers` that appears in the first [`SOFT_404_SCAN_BYTES`] of the body,
/// compared case-insensitively. Some sites keep a deleted post's URL alive and answer HTTP 200
/// with a notice where the image should be; the body is already in hand, so checking is free,
/// and it is the ONLY check a single-candidate match gets. `markers` are lower-case.
pub fn gone_marker(body: &str, markers: &[&str]) -> Option<String> {
    let mut end = body.len().min(SOFT_404_SCAN_BYTES);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let lowered = body[..end].to_lowercase();
    markers.iter().find(|m| lowered.contains(*m)).map(|m| {
        info!("post says it has been deleted (matched {m:?})");
        (*m).to_string()
    })
}

/// Runs one hook, returning `default` (and logging) if it panics.
fn guarded<T>(url: &str, hook: &str, default: T, run: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or_else(|_| {
        warn!("{hook} panicked for {url} (continuing without it)");
        default
    })
}

/// The half of `fetch_page_info` that runs once the body is in hand and its status was 200:
/// every hook asked of the one body, and `incomplete_reason` asked when the match came back
/// missing its tags or its file. Sets `fetched`, `body` and `fetched_url`; the caller adds
/// `gone_reason` (from [`gone_marker`] or a redirect).
///
/// Python wraps each hook in `try/except` so one parser slip cannot take the others down: a
/// parser that raises in `parse` loses its tags and keeps its file URL, which is the right blast
/// radius. A hook that panics is caught the same way here and counts as "has nothing to say". A
/// panic is a bug in the port, not a way for a site to answer, so the message still goes to the
/// log. Parser-health counters (`record_empty`, `record_success`) are the caller's: the result
/// says whether `tags` is empty and whether the parser calls that normal
/// (`empty_result_is_normal`).
pub fn page_info_from_body(
    parser: &SiteParser,
    body: &str,
    url: &str,
    fetched_url: &str,
    ctx: &Context,
) -> BooruPageInfo {
    let tags = guarded(url, "parse", Vec::new(), || (parser.parse)(body, url, ctx));
    let file_url = guarded(url, "parse_file_url", None, || {
        parser.parse_file_url.and_then(|hook| hook(body, url, ctx))
    });
    let preview_url = guarded(url, "parse_preview_url", None, || {
        parser
            .parse_preview_url
            .and_then(|hook| hook(body, url, ctx))
    });
    let restricted = guarded(url, "parse_restriction", None, || {
        parser
            .parse_restriction
            .and_then(|hook| hook(body, url, ctx))
    });
    let page_count = guarded(url, "parse_page_count", 1, || {
        parser
            .parse_page_count
            .map_or(1, |hook| hook(body, url, ctx).max(1))
    });
    let (width, height) = guarded(url, "parse_dimensions", (None, None), || {
        parser
            .parse_dimensions
            .map_or((None, None), |hook| hook(body, url, ctx))
    });
    let rating = guarded(url, "parse_rating", None, || {
        parser.parse_rating.and_then(|hook| hook(body, url, ctx))
    });
    let (file_format, file_size_bytes) = guarded(url, "parse_file_info", (None, None), || {
        parser
            .parse_file_info
            .map_or((None, None), |hook| hook(body, url, ctx))
    });
    let canonical_url = guarded(url, "parse_canonical_url", None, || {
        parser
            .parse_canonical_url
            .and_then(|hook| hook(body, url, ctx))
    });

    // Asked when the match came back missing something a complete one would have, its tags, its
    // file, or both, and only of a parser with something to say about why. The file matters as
    // much as the tags: e621 hands back a withheld post's tags in full and nulls only its file
    // url, so a tags-only condition would never fire for the case that needs explaining.
    let incomplete_reason = if tags.is_empty() || file_url.is_none() {
        guarded(url, "incomplete_reason", None, || {
            parser
                .incomplete_reason
                .and_then(|hook| hook(body, url, ctx))
        })
    } else {
        None
    };
    if tags.is_empty() {
        // For most sites an empty result means the markup moved. For one that finds posts by
        // file hash, a miss just means the local file isn't the site's copy.
        if parser.empty_result_is_normal {
            debug!("No match for {url} (parser: {})", parser.name);
        } else {
            warn!(
                "No tags extracted from {url} - the site's markup may have changed (parser: {})",
                parser.name
            );
        }
    }
    if let Some(reason) = &restricted {
        info!("{url} is restricted: {reason}");
    }

    BooruPageInfo {
        tags,
        file_url,
        preview_url,
        width,
        height,
        file_format,
        file_size_bytes,
        rating,
        gone_reason: None,
        page_count,
        incomplete_reason,
        restricted,
        canonical_url,
        fetched_url: Some(fetched_url.to_string()),
        body: Some(body.to_string()),
        fetched: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TagSource;

    fn tag(name: &str) -> Tag {
        Tag {
            name: name.to_string(),
            namespace: None,
            source: TagSource::Booru,
        }
    }

    #[test]
    fn the_registry_routes_a_url_to_its_site_or_to_nothing() {
        assert_eq!(
            find_parser("https://danbooru.donmai.us/posts/1").map(|p| p.name),
            Some("danbooru")
        );
        for (url, site) in [
            ("https://e621.net/posts/219935", "e621"),
            ("https://e926.net/posts/1", "e621"),
            ("https://yande.re/post/show/123", "moebooru"),
            ("https://konachan.com/post/show/1", "moebooru"),
            ("https://rule34.paheal.net/post/view/5674224", "paheal"),
            (
                "https://rule34.us/index.php?r=posts/view&id=6608448",
                "rule34us",
            ),
        ] {
            assert_eq!(find_parser(url).map(|p| p.name), Some(site), "{url}");
        }
        assert!(find_parser("https://example.com/").is_none());
        assert!(find_parser("").is_none());
        // The probe URL, and a URL with no scheme.
        assert!(find_parser("https://danbooru.donmai.us/posts/1000000").is_some());
        assert!(find_parser("danbooru.donmai.us/posts/1").is_some());
        // A host inside another host's name, or in a path, is nobody's.
        assert!(find_parser("https://hentaivox.com/view/186931/6").is_none());
        assert!(find_parser("https://r34.app/posts/rule34.xxx/anna_(tekuho)").is_none());
        assert!(find_parser("https://mangadex.org/chapter/abc/").is_none());
        assert!(find_parser("https://e-hentai.org/g/1234567/abcdef0123/").is_none());
    }

    #[test]
    fn the_api_and_markup_sites_are_found_by_their_legacy_and_modern_url_shapes() {
        for (url, site) in [
            ("https://anime-pictures.net/posts/596771", "animepictures"),
            (
                "https://anime-pictures.net/pictures/view_post/596771",
                "animepictures",
            ),
            ("https://chan.sankakucomplex.com/post/show/12345", "sankaku"),
            (
                "https://chan.sankakucomplex.com/en/posts/4064096",
                "sankaku",
            ),
            (
                "https://www.sankakucomplex.com/posts/QyMk8vZ6Kak",
                "sankaku",
            ),
            ("https://reactor.cc/post/5351071", "joyreactor"),
            ("https://joyreactor.com/post/5351071", "joyreactor"),
            ("https://joyreactor.cc/post/5351071", "joyreactor"),
        ] {
            assert_eq!(find_parser(url).map(|p| p.name), Some(site), "{url}");
        }
        // A reactor page that is not a post is nobody's.
        assert!(find_parser("https://reactor.cc/tag/art").is_none());
    }

    #[test]
    fn pixiv_twitter_and_reddit_posts_are_found_and_their_neighbours_are_nobodys() {
        for (url, site) in [
            ("https://www.pixiv.net/artworks/75494369", "pixiv"),
            ("https://www.pixiv.net/en/artworks/75494369", "pixiv"),
            (
                "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=75494369",
                "pixiv",
            ),
            ("https://twitter.com/user/status/1", "twitter"),
            ("https://x.com/user/status/1", "twitter"),
            ("https://fxtwitter.com/user/status/1", "twitter"),
            ("https://redd.it/1hhrx0v", "redditpost"),
            ("https://www.reddit.com/comments/1hhrx0v", "redditpost"),
            ("https://old.reddit.com/r/x/comments/1hhrx0v/", "redditpost"),
        ] {
            assert_eq!(find_parser(url).map(|p| p.name), Some(site), "{url}");
        }
        // Media and shortener URLs carry no tweet id, a listing or user page no post, and a
        // picture's own file belongs to no post: classification only.
        for url in [
            "https://pbs.twimg.com/media/abc.jpg",
            "https://t.co/abc",
            "https://anilist.co/anime/21",
            "https://www.reddit.com/r/5ToubunNoHanayome/",
            "https://www.reddit.com/user/someone",
            "https://i.redd.it/m42sxjdyus7e1.jpeg",
        ] {
            assert!(find_parser(url).is_none(), "{url}");
        }
    }

    #[test]
    fn a_sankaku_hash_miss_reaches_the_page_info_as_an_explanation_and_only_then() {
        let ctx = Context::default();
        let url = "https://chan.sankakucomplex.com/en/posts/4604859";
        let miss = page_info_from_body(&sankaku::PARSER, "[]", url, url, &ctx);
        assert!(miss.tags.is_empty());
        assert!(
            miss.incomplete_reason
                .as_deref()
                .is_some_and(|r| r.contains("byte-identical"))
        );
        // With tags the post is real, so there is nothing to explain.
        let body = r#"[{"id": "abc", "tags": [{"name": "cat", "type": 0}]}]"#;
        let hit = page_info_from_body(&sankaku::PARSER, body, url, url, &ctx);
        assert_eq!(hit.tags.len(), 1);
        assert_eq!(hit.incomplete_reason, None);
        assert_eq!(
            hit.canonical_url.as_deref(),
            Some("https://www.sankakucomplex.com/posts/abc")
        );
    }

    #[test]
    fn every_parser_is_registered_once_under_its_own_name() {
        let mut names: Vec<_> = PARSERS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PARSERS.len());
        for parser in PARSERS {
            let url = format!("https://{}.invalid/", parser.name);
            assert!(
                !(parser.matches)(&url),
                "{} claims an unrelated host",
                parser.name
            );
        }
    }

    #[test]
    fn the_hook_list_names_every_field_in_order() {
        let all = SiteParser {
            resolve_fetch_url: Some(|u| u.to_string()),
            resolve_fetch_url_with_context: Some(|u, _| u.to_string()),
            parse_file_url: Some(why),
            parse_preview_url: Some(why),
            parse_dimensions: Some(|_, _, _| (None, None)),
            parse_rating: Some(why),
            parse_file_info: Some(|_, _, _| (None, None)),
            parse_page_count: Some(|_, _, _| 1),
            parse_pages: Some(|_, _, _| Vec::new()),
            pages_api_url: Some(|_| None),
            parse_restriction: Some(why),
            restriction_for_status: Some(|_, _, _, _| None),
            parse_canonical_url: Some(why),
            incomplete_reason: Some(why),
            ..SiteParser::new("all", never_matches, no_tags)
        };
        let names: Vec<_> = OPTIONAL_PARSER_HOOKS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(all.defined_hooks(), names);
        assert!(
            SiteParser::new("none", never_matches, no_tags)
                .defined_hooks()
                .is_empty()
        );
    }

    #[test]
    fn danbooru_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            danbooru::PARSER.defined_hooks(),
            [
                "resolve_fetch_url",
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_rating",
                "parse_restriction",
            ]
        );
        assert!(danbooru::PARSER.incomplete_reason.is_none());
    }

    fn two_tags(_: &str, _: &str, _: &Context) -> Vec<Tag> {
        vec![tag("a"), tag("b")]
    }

    fn no_tags(_: &str, _: &str, _: &Context) -> Vec<Tag> {
        Vec::new()
    }

    fn never_matches(_: &str) -> bool {
        false
    }

    fn why(_: &str, _: &str, _: &Context) -> Option<String> {
        Some("withheld".to_string())
    }

    fn file(_: &str, _: &str, _: &Context) -> Option<String> {
        Some("https://f/x.png".to_string())
    }

    #[test]
    fn incomplete_reason_is_asked_only_of_a_match_missing_its_tags_or_its_file() {
        let ctx = Context::default();
        let with = |parse, file_url| SiteParser {
            incomplete_reason: Some(why),
            parse_file_url: file_url,
            ..SiteParser::new("t", never_matches, parse)
        };
        // Tags but no file: asked (the e621 case).
        let info = page_info_from_body(&with(two_tags, None), "b", "u", "f", &ctx);
        assert_eq!(info.incomplete_reason.as_deref(), Some("withheld"));
        // File but no tags: asked.
        let info = page_info_from_body(&with(no_tags, Some(file)), "b", "u", "f", &ctx);
        assert_eq!(info.incomplete_reason.as_deref(), Some("withheld"));
        // Complete: not asked.
        let info = page_info_from_body(&with(two_tags, Some(file)), "b", "u", "f", &ctx);
        assert_eq!(info.incomplete_reason, None);
        // A parser with no such hook has no reason.
        let bare = SiteParser::new("t", never_matches, no_tags);
        assert_eq!(
            page_info_from_body(&bare, "b", "u", "f", &ctx).incomplete_reason,
            None
        );
    }

    #[test]
    fn a_page_is_assembled_from_every_hook_the_parser_defines() {
        let ctx = Context::default();
        let body = r#"{"id": 5, "image_width": 3, "image_height": 4, "rating": "q",
                       "file_url": "F", "large_file_url": "L", "tag_string_general": "x y"}"#;
        let url = "https://danbooru.donmai.us/posts/5";
        let fetched = "https://danbooru.donmai.us/posts/5.json";
        let info = page_info_from_body(&danbooru::PARSER, body, url, fetched, &ctx);
        assert_eq!(info.tags.len(), 2);
        assert_eq!(info.file_url.as_deref(), Some("F"));
        assert_eq!(info.preview_url.as_deref(), Some("L"));
        assert_eq!((info.width, info.height), (Some(3), Some(4)));
        assert_eq!(info.rating.as_deref(), Some("questionable"));
        assert_eq!(info.restricted, None);
        assert_eq!(info.page_count, 1);
        assert_eq!(info.fetched_url.as_deref(), Some(fetched));
        assert_eq!(info.body.as_deref(), Some(body));
        assert!(info.fetched);
        // No parser matched: nothing was requested, which is not "fetched and found nothing".
        assert!(!BooruPageInfo::default().fetched);
    }

    #[test]
    fn a_hook_that_panics_costs_its_own_answer_and_nothing_else() {
        let ctx = Context::default();
        fn boom(_: &str, _: &str, _: &Context) -> Vec<Tag> {
            panic!("markup moved")
        }
        // `parse` blows up: no tags, and the file URL is still there.
        let parser = SiteParser {
            parse_file_url: Some(file),
            incomplete_reason: Some(why),
            ..SiteParser::new("t", never_matches, boom)
        };
        let info = page_info_from_body(&parser, "b", "u", "f", &ctx);
        assert!(info.tags.is_empty());
        assert_eq!(info.file_url.as_deref(), Some("https://f/x.png"));
        assert_eq!(info.incomplete_reason.as_deref(), Some("withheld"));
        // `parse_rating` blows up: no rating, and the tags are still there.
        let parser = SiteParser {
            parse_rating: Some(|_, _, _| panic!("markup moved")),
            parse_dimensions: Some(|_, _, _| panic!("markup moved")),
            parse_page_count: Some(|_, _, _| panic!("markup moved")),
            parse_file_info: Some(|_, _, _| panic!("markup moved")),
            ..SiteParser::new("t", never_matches, two_tags)
        };
        let info = page_info_from_body(&parser, "b", "u", "f", &ctx);
        assert_eq!(info.rating, None);
        assert_eq!((info.width, info.height), (None, None));
        assert_eq!((info.file_format, info.file_size_bytes), (None, None));
        assert_eq!(info.page_count, 1);
        assert_eq!(info.tags.len(), 2);
        // A hook that panics while asked "why is this incomplete?" does not cost the match.
        let parser = SiteParser {
            incomplete_reason: Some(|_, _, _| panic!("boom")),
            ..SiteParser::new("t", never_matches, no_tags)
        };
        assert_eq!(
            page_info_from_body(&parser, "b", "u", "f", &ctx).incomplete_reason,
            None
        );
    }

    #[test]
    fn rating_is_kept_out_of_the_tag_list() {
        let parser = SiteParser {
            parse_rating: Some(|_, _, _| Some("explicit".to_string())),
            ..SiteParser::new("t", never_matches, two_tags)
        };
        let info = page_info_from_body(&parser, "b", "u", "f", &Context::default());
        assert_eq!(info.rating.as_deref(), Some("explicit"));
        assert_eq!(info.tags, [tag("a"), tag("b")]);
    }

    #[test]
    fn a_page_count_below_one_is_one() {
        let parser = SiteParser {
            parse_page_count: Some(|_, _, _| 0),
            ..SiteParser::new("t", never_matches, no_tags)
        };
        let info = page_info_from_body(&parser, "", "u", "f", &Context::default());
        assert_eq!(info.page_count, 1);
    }

    #[test]
    fn a_deletion_notice_is_found_in_the_scanned_prefix_only() {
        assert_eq!(
            gone_marker("<p>This Post Was Deleted.</p>", &["was deleted"]).as_deref(),
            Some("was deleted")
        );
        assert_eq!(gone_marker("fine", &["was deleted"]), None);
        let late = format!("{}was deleted", "a".repeat(SOFT_404_SCAN_BYTES));
        assert_eq!(gone_marker(&late, &["was deleted"]), None);
        // The cut falls inside a multi-byte character: it is moved back, not split.
        let wide = "\u{e9}".repeat(SOFT_404_SCAN_BYTES);
        assert_eq!(gone_marker(&wide, &["was deleted"]), None);
    }

    #[test]
    fn the_two_error_kinds_stay_apart_and_print_their_message() {
        assert_ne!(
            BooruError::Fetch("x".into()),
            BooruError::ContentGone("x".into())
        );
        assert_eq!(BooruError::ContentGone("gone".into()).to_string(), "gone");
    }
}
