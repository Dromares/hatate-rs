//! IQDB reverse image search (`core/iqdb.py`).
//!
//! IQDB (iqdb.org, and 3d.iqdb.org for the 3D/CG index, which shares the markup) takes a
//! multipart POST of the image and answers with an HTML page; there is no JSON API, so the
//! "best match" tables are scraped. This is the half that reads the page: [`search_from_response`]
//! is what `search` does once the answer is in hand, [`parse_results`] is `_parse_results`.
//! The upload, the 45 s floor on the timeout and the network errors are the engine client's
//! (Phase 3; see the module docs of [`super`]).

use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use tracing::{debug, warn};

use super::{extract_dimensions, looks_like_url};
use crate::boorus::parsed::{classes_of, soup_of, text_of, text_stripped};
use crate::boorus::sizes;
use crate::model::Tag;
use crate::{pycompat, pyurl, sites};

pub const IQDB_URL: &str = "https://iqdb.org/";
pub const IQDB_3D_URL: &str = "https://3d.iqdb.org/";

/// The least time an IQDB search gets, whatever the general search timeout says (measured: 8 to
/// 42 s per search whatever the upload's size).
pub const MIN_BUDGET_SECONDS: f64 = 45.0;

/// One result row. Field order is not the serialised order (JSON objects are compared by key).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IqdbMatch {
    /// Link to the booru or source page.
    pub url: String,
    pub thumb_url: Option<String>,
    /// Percent, 0 to 100.
    pub similarity: f64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    /// e.g. "Danbooru", "Gelbooru": [`sites::guess_site_name`] of the URL.
    pub source_name: Option<String>,
    pub is_best_match: bool,
    pub unnamespaced_tags: Vec<Tag>,
}

/// `IqdbError`: the message is user-facing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IqdbError(pub String);

impl std::fmt::Display for IqdbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IqdbError {}

/// "IQDB 3D" for the 3D instance, else "IQDB": how `search` names the instance in its messages.
pub fn instance_name(base_url: &str) -> &'static str {
    if base_url.contains("3d.iqdb") {
        "IQDB 3D"
    } else {
        "IQDB"
    }
}

/// What `search` makes of the answer to its POST: the status mapping, then [`parse_results`].
/// `base_url` is the instance that was asked ([`IQDB_URL`] or [`IQDB_3D_URL`]); `search_3d` is
/// `search` with the second.
pub fn search_from_response(
    base_url: &str,
    status: u16,
    html: &str,
) -> Result<Vec<IqdbMatch>, IqdbError> {
    let instance = instance_name(base_url);
    if status == 413 {
        return Err(IqdbError(format!(
            "{instance} rejected the image as too large, even after downscaling"
        )));
    }
    if status != 200 {
        return Err(IqdbError(format!("{instance} returned HTTP {status}")));
    }
    let matches = parse_results(html, base_url)?;
    debug!("{instance} HTML parsed into {} match(es)", matches.len());
    Ok(matches)
}

// `(\d+(?:\.\d+)?)\s*%\s*similarity`, IGNORECASE. Python folds `s` with U+017F and `i` with the
// two Turkish i's; `(?i)` here would not, so the word is spelled out.
static PERCENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(\d+(?:\.\d+)?)[\s\x1c-\x1f]*%[\s\x1c-\x1f]*[sS\u{17f}][iI\u{130}\u{131}][mM][iI\u{130}\u{131}][lL][aA][rR][iI\u{130}\u{131}][tT][yY]",
    )
    .expect("percent regex")
});

/// `_extract_percent`: the number before "% similarity".
fn extract_percent(text: &str) -> Option<f64> {
    PERCENT_RE
        .captures(text)
        .and_then(|m| sizes::py_float(&m[1]))
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("selector")
}

static ANY_TABLE: LazyLock<Selector> = LazyLock::new(|| selector("table"));
static ANY_TH: LazyLock<Selector> = LazyLock::new(|| selector("th"));
static ANY_A: LazyLock<Selector> = LazyLock::new(|| selector("a"));
static ANY_IMG: LazyLock<Selector> = LazyLock::new(|| selector("img"));

/// Whether `el` has an ancestor `<tag>` with a class or id as given: the other half of a
/// `select("div#pages table")`, whose ancestor may sit outside the element being searched.
fn under(el: ElementRef<'_>, tag: &str, test: impl Fn(ElementRef<'_>) -> bool) -> bool {
    el.ancestors()
        .filter_map(ElementRef::wrap)
        .any(|a| a.value().name() == tag && test(a))
}

/// `_parse_results`: one results page into matches, best match first.
///
/// `Err` when the page has no result tables and does not mention "iqdb" anywhere, which no real
/// IQDB page does: it is a CAPTCHA, a rate-limit notice or another block page that still said
/// 200, and reporting "no matches" for it would be cached as a confirmed NOT_FOUND.
pub fn parse_results(html: &str, base_url: &str) -> Result<Vec<IqdbMatch>, IqdbError> {
    let soup: Html = soup_of(html);
    let tables: Vec<ElementRef<'_>> = soup
        .select(&ANY_TABLE)
        .filter(|t| under(*t, "div", |d| d.value().attr("id") == Some("pages")))
        .collect();
    debug!("Found {} result table(s) in IQDB response", tables.len());

    if tables.is_empty() && !html.to_lowercase().contains("iqdb") {
        warn!(
            "IQDB response has no result tables and doesn't look like a real IQDB page - treating \
             as a failure rather than a confirmed empty result"
        );
        return Err(IqdbError(
            "IQDB's response didn't look like a real results page (possible block/CAPTCHA/rate limit)"
                .to_owned(),
        ));
    }

    let mut matches = Vec::new();
    for (i, table) in tables.into_iter().enumerate() {
        let is_best = table
            .select(&ANY_TH)
            .next()
            .is_some_and(|th| text_of(th).to_lowercase().contains("best match"));

        // `td.image a`: the first link under a cell of class "image".
        let image_link = table
            .select(&ANY_A)
            .find(|a| under(*a, "td", |td| classes_of(td).contains(&"image")));
        let Some(image_link) = image_link else {
            debug!("Table {i}: no td.image a element, skipping (markup may have changed)");
            continue;
        };

        // A row can hold several links (a raw-filename download beside the source page), so the
        // first is not enough: only an absolute URL counts. The href is used as written, not
        // stripped, once it has passed the test.
        let url = table
            .select(&ANY_A)
            .filter_map(|a| a.value().attr("href"))
            .find(|href| !href.is_empty() && looks_like_url(href))
            .map(|href| match href.strip_prefix("//") {
                Some(_) => format!("https:{href}"),
                None => href.to_owned(),
            });
        let Some(url) = url else {
            debug!("Table {i}: no absolute-URL <a> found, skipping");
            continue;
        };

        // Thumbnails are ROOT-relative paths ("/danbooru/3/5/7/hash.jpg"); urljoin handles that,
        // the protocol-relative form and the absolute one alike.
        let thumb_url = image_link
            .select(&ANY_IMG)
            .next()
            .and_then(|img| img.value().attr("src"))
            .filter(|src| !src.is_empty())
            .map(|src| pyurl::urljoin(base_url, pycompat::strip(src)));

        let text = text_stripped(table, " ");
        let similarity = extract_percent(&text);
        let (width, height) = extract_dimensions(&text);
        let source_name = sites::guess_site_name(&url).map(str::to_owned);
        debug!(
            "Table {i}: parsed match url={url} site={source_name:?} similarity={similarity:?} best={is_best}"
        );

        matches.push(IqdbMatch {
            url,
            thumb_url,
            // `similarity or 0.0`
            similarity: similarity.unwrap_or(0.0),
            width,
            height,
            source_name,
            is_best_match: is_best,
            unnamespaced_tags: Vec::new(),
        });
    }

    // `(not is_best_match, -similarity)`, stable.
    matches.sort_by(|a, b| {
        (!a.is_best_match, -a.similarity)
            .partial_cmp(&(!b.is_best_match, -b.similarity))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(rows: &str) -> String {
        format!(
            "<html><head><title>iqdb</title></head><body><div id=\"pages\">{rows}</div></body></html>"
        )
    }

    fn row(head: &str, href: &str, src: &str, text: &str) -> String {
        format!(
            "<table><tr><th>{head}</th></tr><tr><td class=\"image\"><a href=\"{href}\"><img src=\"{src}\"></a></td><td>{text}</td></tr></table>"
        )
    }

    #[test]
    fn a_best_match_row_is_read_whole() {
        let html = page(&row(
            "Best match",
            "https://danbooru.donmai.us/posts/1",
            "/danbooru/3/5/7/abc.jpg",
            "1200x1600 [Danbooru] 97% similarity",
        ));
        let got = parse_results(&html, IQDB_URL).unwrap();
        assert_eq!(got.len(), 1);
        let m = &got[0];
        assert_eq!(m.url, "https://danbooru.donmai.us/posts/1");
        assert_eq!(
            m.thumb_url.as_deref(),
            Some("https://iqdb.org/danbooru/3/5/7/abc.jpg")
        );
        assert_eq!(
            (m.similarity, m.width, m.height),
            (97.0, Some(1200), Some(1600))
        );
        assert_eq!(m.source_name.as_deref(), Some("Danbooru"));
        assert!(m.is_best_match);
        assert!(m.unnamespaced_tags.is_empty());
    }

    #[test]
    fn best_matches_sort_first_then_by_similarity_and_ties_keep_page_order() {
        let html = page(
            &[
                row(
                    "Additional match",
                    "https://a.example/1",
                    "/t",
                    "50% similarity",
                ),
                row(
                    "Additional match",
                    "https://a.example/2",
                    "/t",
                    "90% similarity",
                ),
                row("Best match", "https://a.example/3", "/t", "10% similarity"),
                row(
                    "Additional match",
                    "https://a.example/4",
                    "/t",
                    "90% similarity",
                ),
            ]
            .concat(),
        );
        let urls: Vec<_> = parse_results(&html, IQDB_URL)
            .unwrap()
            .into_iter()
            .map(|m| m.url)
            .collect();
        assert_eq!(
            urls,
            [
                "https://a.example/3",
                "https://a.example/2",
                "https://a.example/4",
                "https://a.example/1"
            ]
        );
    }

    #[test]
    fn the_first_absolute_link_is_the_source_and_it_is_used_as_written() {
        let table = "<table><tr><th>Best match</th></tr><tr><td class=image><a href=\"/relative\"><img src=\" //cdn.example/t.jpg \"></a>\
                     <a href=\"\">e</a><a href=\" //b.example/p \">x</a></td>\
                     <td>5% similarity</td></tr></table>";
        let got = parse_results(&page(table), IQDB_URL).unwrap();
        // The thumbnail is the first link's image, whichever link is the source. Stripped for the
        // test, not for the value: the leading space defeats the `//` rewrite.
        assert_eq!(got[0].url, " //b.example/p ");
        assert_eq!(
            got[0].thumb_url.as_deref(),
            Some("https://cdn.example/t.jpg")
        );
        let table = table.replace(" //b.example/p ", "//b.example/p");
        let got = parse_results(&page(&table), IQDB_URL).unwrap();
        assert_eq!(got[0].url, "https://b.example/p");
    }

    #[test]
    fn rows_without_an_image_cell_or_a_source_link_are_skipped() {
        let no_cell = "<table><tr><th>Best match</th></tr><tr><td><a href=\"https://a.example/1\">x</a></td></tr></table>";
        let no_link = "<table><tr><td class=image><a href=\"/rel\">x</a></td></tr></table>";
        assert!(
            parse_results(&page(&[no_cell, no_link].concat()), IQDB_URL)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_missing_similarity_is_zero_and_missing_dimensions_are_none() {
        let html = page(&row(
            "Additional match",
            "https://a.example/1",
            "",
            "no numbers",
        ));
        let m = &parse_results(&html, IQDB_URL).unwrap()[0];
        assert_eq!((m.similarity, m.width, m.height), (0.0, None, None));
        assert_eq!(m.thumb_url, None);
        assert_eq!(m.source_name, None);
        assert!(!m.is_best_match);
    }

    #[test]
    fn a_table_outside_div_pages_is_not_a_result() {
        let html = format!(
            "<html><body>iqdb {}</body></html>",
            row("Best match", "https://a.example/1", "/t", "9% similarity")
        );
        assert!(parse_results(&html, IQDB_URL).unwrap().is_empty());
    }

    #[test]
    fn a_page_that_is_not_iqdbs_is_an_error_and_an_empty_iqdb_page_is_not() {
        let err =
            parse_results("<html><body>Just a moment...</body></html>", IQDB_URL).unwrap_err();
        assert!(
            err.0
                .starts_with("IQDB's response didn't look like a real results page")
        );
        assert!(parse_results("", IQDB_URL).is_err());
        assert!(
            parse_results("<html><title>IQDB</title></html>", IQDB_URL)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_percent_is_found_in_python_whitespace_and_case() {
        assert_eq!(extract_percent("x 97.5 %\u{1f}SIMILARITY y"), Some(97.5));
        assert_eq!(extract_percent("97%similarity"), Some(97.0));
        assert_eq!(extract_percent("\u{661}\u{662}% similarity"), Some(12.0));
        assert_eq!(extract_percent("97% sim"), None);
        assert_eq!(extract_percent("97 similarity"), None);
        // The spellings Python's IGNORECASE accepts and `(?i)` would not.
        assert_eq!(
            extract_percent("3% \u{17f}\u{130}m\u{131}larity"),
            Some(3.0)
        );
    }

    #[test]
    fn the_status_decides_before_the_page_is_read() {
        let html = page(&row(
            "Best match",
            "https://a.example/1",
            "/t",
            "9% similarity",
        ));
        assert_eq!(search_from_response(IQDB_URL, 200, &html).unwrap().len(), 1);
        assert_eq!(
            search_from_response(IQDB_URL, 413, &html).unwrap_err().0,
            "IQDB rejected the image as too large, even after downscaling"
        );
        assert_eq!(
            search_from_response(IQDB_3D_URL, 503, &html).unwrap_err().0,
            "IQDB 3D returned HTTP 503"
        );
        assert_eq!(instance_name("https://3d.iqdb.org/"), "IQDB 3D");
        assert_eq!(instance_name("https://iqdb.org/"), "IQDB");
    }

    #[test]
    fn a_relative_thumbnail_is_joined_to_the_instance_that_answered() {
        let html = page(&row(
            "Best match",
            "https://a.example/1",
            "danbooru/t.jpg",
            "9% similarity",
        ));
        let m = &parse_results(&html, IQDB_3D_URL).unwrap()[0];
        assert_eq!(
            m.thumb_url.as_deref(),
            Some("https://3d.iqdb.org/danbooru/t.jpg")
        );
    }
}
