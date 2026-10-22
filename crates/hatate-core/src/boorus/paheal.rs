//! rule34.paheal.net: Shimmie2, so none of the Gelbooru-family parsers fit
//! (`core/boorus/paheal.py`).
//!
//! Reached through Google Lens rather than IQDB or SauceNAO: neither of those indexes Paheal, but
//! Lens's "Exact matches" tab names its posts, so matches arrive here with a rebuilt
//! `/post/view/{id}` URL.
//!
//! Everything worth having is on the post page and needs no API:
//!
//! - Tags are `<a class="tag_name" href="/post/list/Ben_10/1">Ben 10</a>`. The HREF is used
//!   rather than the link text, because the text is the display form with spaces ("Gwen
//!   Tennyson") while the href carries the site's own canonical underscored name
//!   ("Gwen_Tennyson"), which is the form every other parser here produces and the one Hydrus
//!   expects.
//! - The original file, its dimensions and its media type are all attributes of `#main_image`,
//!   so no second request is needed to learn the size of the match.
//!
//! Paheal has no tag categories, so its tags carry the empty namespace (not `None`: the corpus
//! records `""`). That is the site being simple, not a parser gap.
//!
//! Every hook is a pure function of the body and the post-page URL; Paheal keeps no state.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use tracing::debug;

use super::SiteParser;
use super::host::on_host;
use super::parsed::{soup_of, text_of};
use super::sizes::py_int;
use crate::model::{Tag, TagSource};
use crate::{pycompat, pyurl};

const HOSTS: [&str; 2] = ["rule34.paheal.net", "paheal.net"];

/// "/post/list/Gwen_Tennyson/1" -> "Gwen_Tennyson". The trailing page number is part of the
/// route, not the tag. (`[\s\x1c-\x1f]` is Python's `\s`.)
static TAG_HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/post/list/([^/]+)/\d+[\s\x1c-\x1f]*$").expect("tag href regex"));

/// Site navigation that uses the same route as a tag link. These are Shimmie's own listing
/// shortcuts, not tags on this post.
const NON_TAGS: [&str; 4] = ["", "all", "any", "none"];

const MIME_TO_FORMAT: [(&str, &str); 6] = [
    ("image/jpeg", "JPEG"),
    ("image/png", "PNG"),
    ("image/gif", "GIF"),
    ("image/webp", "WEBP"),
    ("video/webm", "WEBM"),
    ("video/mp4", "MP4"),
];

static TAG_LINK: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a.tag_name[href]").expect("selector"));
static MAIN_IMAGE: LazyLock<(Selector, Selector)> = LazyLock::new(|| {
    (
        Selector::parse("#main_image").expect("selector"),
        Selector::parse("img.shm-main-image").expect("selector"),
    )
});

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_file_info: Some(|body, url, _| parse_file_info(body, url)),
    ..SiteParser::new("paheal", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let soup = soup_of(html);
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    for anchor in soup.select(&TAG_LINK) {
        // Fall back to the visible text, which differs only in using spaces where the canonical
        // name uses underscores.
        let name = tag_from_href(anchor.value().attr("href").unwrap_or(""))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| text_of(anchor).replace(' ', "_"));
        let key = name.to_lowercase();
        if name.is_empty() || NON_TAGS.contains(&key.as_str()) || !seen.insert(key) {
            continue;
        }
        tags.push(Tag {
            name,
            namespace: Some(String::new()),
            source: TagSource::Booru,
        });
    }
    if tags.is_empty() {
        debug!("No tags extracted from {url} (markup may have changed)");
    }
    tags
}

/// The original file, which lives on Paheal's CDN.
///
/// Its URL has no extension; the type is stated in `data-mime` instead, so nothing downstream
/// should infer the format from the path.
pub fn parse_file_url(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);
    let src = main_image(&soup)?.value().attr("src").unwrap_or("");
    let src = pycompat::strip(src);
    (!src.is_empty()).then(|| pyurl::urljoin(url, src))
}

/// Paheal serves one image at full size and no sample, so the original is the preview. Returning
/// `None` instead would make the GUI fall back to the search engine's thumbnail.
pub fn parse_preview_url(html: &str, url: &str) -> Option<String> {
    parse_file_url(html, url)
}

/// Both `data-width` and `data-height` as Python's `int()` reads them, or neither.
pub fn parse_dimensions(html: &str, _url: &str) -> (Option<i64>, Option<i64>) {
    let soup = soup_of(html);
    let Some(image) = main_image(&soup) else {
        return (None, None);
    };
    let attr = |name: &str| image.value().attr(name).and_then(py_int);
    match (attr("data-width"), attr("data-height")) {
        (Some(width), Some(height)) => (Some(width), Some(height)),
        _ => (None, None),
    }
}

/// `(format, size_bytes)` as the page states them.
///
/// Only the format is taken from here. Paheal prints the size rounded to whole KB ("262KB" for
/// 268,096 bytes), and a size that is wrong by a few thousand bytes is worse than no size at all:
/// it would be compared against the local file's exact one. The size is left to the HEAD
/// request, which the CDN answers correctly.
pub fn parse_file_info(html: &str, _url: &str) -> (Option<String>, Option<i64>) {
    let soup = soup_of(html);
    let mime = main_image(&soup)
        .and_then(|image| image.value().attr("data-mime"))
        .map(|m| pycompat::strip(m).to_lowercase())
        .unwrap_or_default();
    let format = MIME_TO_FORMAT
        .iter()
        .find(|(k, _)| *k == mime)
        .map(|(_, f)| (*f).to_string());
    (format, None)
}

fn main_image(soup: &Html) -> Option<ElementRef<'_>> {
    let (by_id, by_class) = &*MAIN_IMAGE;
    soup.select(by_id)
        .next()
        .or_else(|| soup.select(by_class).next())
}

fn tag_from_href(href: &str) -> Option<String> {
    let m = TAG_HREF_RE.captures(href)?;
    Some(pycompat::strip(&pyurl::unquote(&m[1])).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://rule34.paheal.net/post/view/5674224";

    fn names(tags: &[Tag]) -> Vec<&str> {
        assert!(tags.iter().all(|t| t.namespace.as_deref() == Some("")));
        tags.iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn claims_paheal_hosts_only() {
        assert!(matches("https://rule34.paheal.net/post/view/1"));
        assert!(matches("https://paheal.net/post/view/1"));
        assert!(matches("https://cdn.paheal.net/x"));
        assert!(!matches("https://rule34.xxx/"));
        assert!(!matches("https://example.com/paheal.net"));
    }

    #[test]
    fn tags_take_the_canonical_name_from_the_href() {
        let html = r#"
          <a class="tag_name" href="/post/list/Ben_10/1">Ben 10</a>
          <a class="tag_name" href="/post/list/Gwen%20Tennyson%21/12">Gwen Tennyson</a>
          <a class="tag_name" href="/post/list/caf%C3%A9/1 ">x</a>
          <a class="tag_name" href="/post/list/ Spaced /3">x</a>
        "#;
        assert_eq!(
            names(&parse(html, URL)),
            ["Ben_10", "Gwen Tennyson!", "caf\u{e9}", "Spaced"]
        );
    }

    #[test]
    fn a_link_that_is_not_a_listing_falls_back_to_its_text() {
        let html = r#"
          <a class="tag_name" href="/wiki/Gwen">Gwen Tennyson</a>
          <a class="tag_name" href="/post/list/%20/1">  Some  Tag </a>
          <a class="tag_name" href="/post/list/Page/x">Pager</a>
          <a class="tag_name">no href</a>
          <a class="other" href="/post/list/Other/1">other</a>
        "#;
        // The text is stripped at its ends only, and every space inside it becomes an underscore.
        assert_eq!(
            names(&parse(html, URL)),
            ["Gwen_Tennyson", "Some__Tag", "Pager"]
        );
    }

    #[test]
    fn shortcuts_and_repeats_are_dropped_and_the_first_spelling_stays() {
        let html = r#"
          <a class="tag_name" href="/post/list/all/1">all</a>
          <a class="tag_name" href="/post/list/ANY/1">any</a>
          <a class="tag_name" href="/post/list/None/1">none</a>
          <a class="tag_name" href="/post/list/%20/1">  </a>
          <a class="tag_name" href="/post/list/Red_Hair/1">x</a>
          <a class="tag_name" href="/post/list/red_hair/2">x</a>
        "#;
        assert_eq!(names(&parse(html, URL)), ["Red_Hair"]);
        assert!(parse("<p>no tags</p>", URL).is_empty());
    }

    #[test]
    fn the_file_is_the_main_image_resolved_against_the_post() {
        let html = r#"<img id="main_image" src=" https://cdn.paheal.net/_images/ab/file.jpg ">"#;
        assert_eq!(
            parse_file_url(html, URL).as_deref(),
            Some("https://cdn.paheal.net/_images/ab/file.jpg")
        );
        assert_eq!(parse_preview_url(html, URL), parse_file_url(html, URL));
        let relative = r#"<img id="main_image" src="/_images/file">"#;
        assert_eq!(
            parse_file_url(relative, URL).as_deref(),
            Some("https://rule34.paheal.net/_images/file")
        );
        for html in [
            r#"<img id="main_image" src="">"#,
            r#"<img id="main_image" src="   ">"#,
            r#"<img id="main_image">"#,
            r#"<img src="x">"#,
            "",
        ] {
            assert_eq!(parse_file_url(html, URL), None, "{html}");
        }
    }

    #[test]
    fn the_class_is_the_fallback_for_the_id() {
        let both = r#"<img class="shm-main-image" src="/c"><img id="main_image" src="/i">"#;
        assert_eq!(
            parse_file_url(both, URL).as_deref(),
            Some("https://rule34.paheal.net/i")
        );
        let class_only = r#"<img class="x shm-main-image" src="/c">"#;
        assert_eq!(
            parse_file_url(class_only, URL).as_deref(),
            Some("https://rule34.paheal.net/c")
        );
        // The first #main_image is the image, even when a later one has the src.
        let first_wins = r#"<img id="main_image"><img id="main_image" src="/later">"#;
        assert_eq!(parse_file_url(first_wins, URL), None);
    }

    #[test]
    fn dimensions_are_both_read_as_int_or_neither() {
        let dims = |w: &str, h: &str| {
            parse_dimensions(
                &format!(r#"<img id="main_image" data-width="{w}" data-height="{h}">"#),
                URL,
            )
        };
        assert_eq!(dims("1350", "1688"), (Some(1350), Some(1688)));
        assert_eq!(dims(" 12 ", "+7"), (Some(12), Some(7)));
        assert_eq!(dims("1_000", "2"), (Some(1000), Some(2)));
        assert_eq!(dims("-5", "2"), (Some(-5), Some(2)));
        assert_eq!(dims("\u{661}\u{662}", "3"), (Some(12), Some(3)));
        for (w, h) in [
            ("12.5", "3"),
            ("12", ""),
            ("", ""),
            ("1__0", "3"),
            ("_1", "3"),
            ("1_", "3"),
        ] {
            assert_eq!(dims(w, h), (None, None), "{w:?} {h:?}");
        }
        assert_eq!(
            parse_dimensions(r#"<img id="main_image" data-width="3">"#, URL),
            (None, None)
        );
        assert_eq!(parse_dimensions("<p></p>", URL), (None, None));
    }

    #[test]
    fn the_format_comes_from_the_mime_and_the_size_is_never_taken() {
        let info = |mime: &str| {
            parse_file_info(
                &format!(r#"<img id="main_image" data-mime="{mime}" data-filesize="268096">"#),
                URL,
            )
        };
        for (mime, format) in [
            ("image/jpeg", "JPEG"),
            ("image/png", "PNG"),
            ("image/gif", "GIF"),
            ("image/webp", "WEBP"),
            ("video/webm", "WEBM"),
            ("video/mp4", "MP4"),
            ("  IMAGE/JPEG ", "JPEG"),
        ] {
            assert_eq!(info(mime), (Some(format.to_string()), None), "{mime}");
        }
        assert_eq!(info("image/avif"), (None, None));
        assert_eq!(info(""), (None, None));
        assert_eq!(parse_file_info("<p></p>", URL), (None, None));
        assert_eq!(
            parse_file_info(r#"<img id="main_image">"#, URL),
            (None, None)
        );
    }

    #[test]
    fn paheal_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_file_info"
            ]
        );
    }
}
