//! rule34.us: its own software, unrelated to rule34.xxx despite the name
//! (`core/boorus/rule34us.py`).
//!
//! Reached through Google Lens: neither IQDB nor SauceNAO indexes it, but Lens's "Exact matches"
//! tab names its posts, and the URL is rebuilt as `index.php?r=posts/view&id={id}`.
//!
//! Without this parser such a match had no image at all but the one Lens paired with it, and
//! Lens's tile can be a 1x1 lazy-load placeholder, which made the side-by-side comparison show a
//! blank. Everything worth having is on the post page, no API needed:
//!
//! - Tags are links to the tag's listing, `r=posts/index&q=<tag>`, inside an
//!   `<li class="{artist,copyright,character,metadata,general}-tag">`. The `q` parameter is the
//!   canonical underscored name; the link text is the display form with spaces. The same `li`
//!   classes are also used for the post's info box ("Id: ...", "Original"), which is why only a
//!   `li` holding a tag-listing link counts as a tag.
//! - The original is the "Original" link in that info box; the page's main `<img>` (or
//!   `<video>`) is the same file, used as a fallback.
//! - "Size: 2039w x 2894h" states the dimensions, for videos too.
//!
//! No rating comes from here, and that is a measured result rather than an omission. The other
//! Danbooru 1.x descendants spell the rating out in the same Statistics block this parser already
//! walks for "Size:", so rule34.us looks like it should too, and it does not: the block holds
//! Id, Added by, Created, Score and Size, full stop. The only occurrence of the string "rating"
//! on the page is a static `<meta name="rating" content="adult" />` in `<head>`, served
//! byte-identically on every page of the site: boilerplate about the site, not a statement about
//! the post. Reading it would stamp the same word on every rule34.us match while looking like
//! per-post fact, which is exactly what [`rating`](super::rating) refuses to do.
//!
//! Every hook is a pure function of the body and the post-page URL; rule34.us keeps no state.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::Selector;
use tracing::debug;

use super::SiteParser;
use super::host::on_host;
use super::parsed::{classes_of, page_text, soup_of, text_of};
use super::sizes::to_int;
use crate::model::{Tag, TagSource};
use crate::pyurl;

const HOSTS: [&str; 1] = ["rule34.us"];

const CATEGORY_MAP: [(&str, &str); 5] = [
    ("artist-tag", "artist"),
    ("copyright-tag", "copyright"),
    ("character-tag", "character"),
    ("metadata-tag", "meta"),
    ("general-tag", "general"),
];

/// `[\s\x1c-\x1f]` is Python's `\s` for text: Rust's leaves out the four ASCII separators.
static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Size:[\s\x1c-\x1f]*(\d+)[\s\x1c-\x1f]*w[\s\x1c-\x1f]*x[\s\x1c-\x1f]*(\d+)[\s\x1c-\x1f]*h")
        .expect("size regex")
});
static MEDIA_HOST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^https?://[^/]*rule34\.us/(?:images|videos)/").expect("media host regex")
});

static LI: LazyLock<Selector> = LazyLock::new(|| Selector::parse("li").expect("selector"));
static ANCHOR_WITH_HREF: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("a[href]").expect("selector"));
static MEDIA_WITH_SRC: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse("video source[src], video[src], img[src]").expect("selector"));

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    ..SiteParser::new("rule34us", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let soup = soup_of(html);
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    for item in soup.select(&LI) {
        let Some(namespace) = classes_of(item).into_iter().find_map(|c| {
            CATEGORY_MAP
                .iter()
                .find(|(k, _)| *k == c)
                .map(|(_, ns)| *ns)
        }) else {
            continue;
        };
        let Some(name) = item
            .select(&ANCHOR_WITH_HREF)
            .next()
            .and_then(|a| tag_from_href(a.value().attr("href").unwrap_or("")))
        else {
            continue;
        };
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        tags.push(Tag {
            name,
            namespace: Some(namespace.to_string()),
            source: TagSource::Booru,
        });
    }
    if tags.is_empty() {
        debug!("No tags extracted from {url} (markup may have changed)");
    }
    tags
}

/// The original: the "Original" link in the info box when it points at the site's media host,
/// else the first video or image on the page that does.
pub fn parse_file_url(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);
    for anchor in soup.select(&ANCHOR_WITH_HREF) {
        let href = anchor.value().attr("href").unwrap_or("");
        if text_of(anchor).to_lowercase() == "original" && MEDIA_HOST_RE.is_match(href) {
            return Some(href.to_string());
        }
    }
    for media in soup.select(&MEDIA_WITH_SRC) {
        let src = pyurl::urljoin(url, media.value().attr("src").unwrap_or(""));
        if MEDIA_HOST_RE.is_match(&src) {
            return Some(src);
        }
    }
    None
}

/// The page shows the original at full size and offers no sample, so the original is the preview.
/// Returning `None` would leave the GUI with the search engine's thumbnail, which for a Lens
/// match can be a blank placeholder.
pub fn parse_preview_url(html: &str, url: &str) -> Option<String> {
    parse_file_url(html, url)
}

pub fn parse_dimensions(html: &str, _url: &str) -> (Option<i64>, Option<i64>) {
    let text = page_text(&soup_of(html), " ");
    match SIZE_RE.captures(&text) {
        // A number too large for an `i64` is nothing here; Python would return the big int.
        Some(m) => (to_int(&m[1]), to_int(&m[2])),
        None => (None, None),
    }
}

/// The tag a listing link names: its `q` parameter, when the link is `r=posts/index`.
fn tag_from_href(href: &str) -> Option<String> {
    let query = pyurl::urlparse(href, "").query;
    if pyurl::first_query_value(&query, "r")
        .as_deref()
        .unwrap_or("")
        != "posts/index"
    {
        return None;
    }
    let name = pyurl::first_query_value(&query, "q")?;
    let name = crate::pycompat::strip(&name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://rule34.us/index.php?r=posts/view&id=6608448";
    const MEDIA: &str = "https://img2.rule34.us/images/cb/6f/x.png";

    fn names(tags: &[Tag]) -> Vec<(&str, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref().unwrap(), t.name.as_str()))
            .collect()
    }

    #[test]
    fn claims_rule34_us_hosts_only() {
        assert!(matches("https://rule34.us/index.php?r=posts/view&id=1"));
        assert!(matches("https://img2.rule34.us/images/a.png"));
        assert!(!matches("https://rule34.xxx/index.php?page=post"));
        assert!(!matches("https://example.com/rule34.us"));
    }

    #[test]
    fn tags_are_the_listing_links_inside_a_category_li() {
        let html = r#"<ul>
          <li class="artist-tag"><a href="index.php?r=posts/index&q=some_artist">some artist</a></li>
          <li class="general-tag"><a href="/index.php?r=posts/index&amp;q=blue%20sky">blue sky</a></li>
          <li class="metadata-tag"><a href="?r=posts/index&q=highres">highres</a></li>
          <li class="character-tag copyright-tag"><a href="?r=posts/index&q=first">first</a></li>
        </ul>"#;
        assert_eq!(
            names(&parse(html, URL)),
            [
                ("artist", "some_artist"),
                ("general", "blue sky"),
                ("meta", "highres"),
                ("character", "first"),
            ]
        );
    }

    #[test]
    fn a_li_that_names_no_tag_is_skipped() {
        let html = r#"<ul>
          <li class="general-tag"><a href="index.php?r=posts/view&id=1">view</a></li>
          <li class="general-tag"><a href="index.php?r=posts/index&q=">empty</a></li>
          <li class="general-tag"><a href="index.php?r=posts/index&q=%20%20">blank</a></li>
          <li class="general-tag"><a href="index.php?r=posts/index">no q</a></li>
          <li class="general-tag"><span>no link</span></li>
          <li class="general-tag"><a>no href</a><a href="?r=posts/index&q=second">second</a></li>
          <li class="info"><a href="?r=posts/index&q=not_a_tag">info box</a></li>
          <li><a href="?r=posts/index&q=no_class">no class</a></li>
          <li class="general-tag"><a href="?r=posts/index&q=a">a</a><a href="?r=posts/index&q=b">b</a></li>
        </ul>"#;
        // Only the FIRST link with an href is read, and only a tag-listing one counts.
        assert_eq!(
            names(&parse(html, URL)),
            [("general", "second"), ("general", "a")]
        );
        assert!(parse("", URL).is_empty());
    }

    #[test]
    fn a_tag_seen_twice_keeps_its_first_category_and_case() {
        let html = r#"<ul>
          <li class="general-tag"><a href="?r=posts/index&q=Blue_Sky">x</a></li>
          <li class="artist-tag"><a href="?r=posts/index&q=blue_sky">x</a></li>
          <li class="artist-tag"><a href="?r=posts/index&q=%C3%89">x</a></li>
          <li class="artist-tag"><a href="?r=posts/index&q=%C3%A9">x</a></li>
        </ul>"#;
        assert_eq!(
            names(&parse(html, URL)),
            [("general", "Blue_Sky"), ("artist", "\u{c9}")]
        );
    }

    #[test]
    fn the_original_link_beats_the_page_image() {
        let html =
            format!(r#"<img src="{MEDIA}b.png"><ul><li><a href="{MEDIA}">Original</a></li></ul>"#);
        assert_eq!(parse_file_url(&html, URL).as_deref(), Some(MEDIA));
        assert_eq!(parse_preview_url(&html, URL).as_deref(), Some(MEDIA));
        // The label is matched stripped and lower-cased.
        let spaced = format!(r#"<a href="{MEDIA}">  <b> ORIGINAL </b> </a>"#);
        assert_eq!(parse_file_url(&spaced, URL).as_deref(), Some(MEDIA));
        // An "Original" link to somewhere else is not the file.
        let elsewhere = r#"<a href="https://example.com/images/a.png">Original</a>"#;
        assert_eq!(parse_file_url(elsewhere, URL), None);
        let lookalike = r#"<a href="https://evilrule34.us/images/a.png">Original</a>"#;
        assert_eq!(
            parse_file_url(lookalike, URL).as_deref(),
            Some("https://evilrule34.us/images/a.png")
        );
    }

    #[test]
    fn without_the_link_the_first_media_element_on_the_media_host_is_used() {
        let video = r#"<img src="https://other.example/images/a.png">
            <video><source src="https://img.rule34.us/videos/v.mp4"></video>
            <img src="https://img.rule34.us/images/late.png">"#;
        assert_eq!(
            parse_file_url(video, URL).as_deref(),
            Some("https://img.rule34.us/videos/v.mp4")
        );
        let relative = r#"<img src="/images/a.png">"#;
        assert_eq!(
            parse_file_url(relative, "https://rule34.us/index.php").as_deref(),
            Some("https://rule34.us/images/a.png")
        );
        // A relative one resolves against the post page, which is not on the media path.
        assert_eq!(parse_file_url(relative, "https://example.com/x"), None);
        let upper = r#"<video src="HTTP://CDN.RULE34.US/VIDEOS/v.mp4"></video>"#;
        assert_eq!(
            parse_file_url(upper, URL).as_deref(),
            Some("HTTP://CDN.RULE34.US/VIDEOS/v.mp4")
        );
        assert_eq!(parse_file_url("<img>", URL), None);
        assert_eq!(parse_file_url("", URL), None);
    }

    #[test]
    fn dimensions_come_from_the_size_line_wherever_it_is() {
        let html = "<li>Id: 1</li><li>Size: 2039w x 2894h</li>";
        assert_eq!(parse_dimensions(html, URL), (Some(2039), Some(2894)));
        // Split over elements: the text is joined with a space.
        let split = "<b>Size:</b><i>10w</i><i>x</i><u>20h</u>";
        assert_eq!(parse_dimensions(split, URL), (Some(10), Some(20)));
        assert_eq!(parse_dimensions("size:1W X2H", URL), (Some(1), Some(2)));
        assert_eq!(parse_dimensions("Size: 10 x 20", URL), (None, None));
        assert_eq!(parse_dimensions("", URL), (None, None));
        // Script text is not page text.
        let hidden = "<script>Size: 1w x 2h</script>Size: 3w x 4h";
        assert_eq!(parse_dimensions(hidden, URL), (Some(3), Some(4)));
    }

    #[test]
    fn rule34_us_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            ["parse_file_url", "parse_preview_url", "parse_dimensions"]
        );
    }
}
