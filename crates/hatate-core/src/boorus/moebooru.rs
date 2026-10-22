//! Yande.re and Konachan, the Moebooru sites (`core/boorus/moebooru.py`).
//!
//! Moebooru is a Danbooru 1.x descendant, so the page is scraped: a tag sidebar, a Statistics
//! block, a full-resolution link and a resized sample. Dimensions and rating come from the same
//! page rather than an API.
//!
//! Every hook is a pure function of the body and the post-page URL; Moebooru keeps no state.

use std::sync::LazyLock;

use regex::Regex;
use scraper::Selector;
use tracing::debug;

use super::host::on_host;
use super::parsed::{page_text, soup_of, text_joined, text_of};
use super::sizes::{describe_size_context, parse_bare_dimensions, parse_size_label, to_int};
use super::{SiteParser, rating};
use crate::model::{Tag, TagSource};

const HOSTS: [&str; 3] = ["yande.re", "konachan.com", "konachan.net"];

/// The Statistics sidebar, which is where both "Size: WxH" and "Rating: Safe" live. Named as a
/// selector rather than scraped out of the whole page for the reason spelled out in
/// [`parse_rating`].
const STATS_SELECTOR: &str = "#stats";

/// Moebooru embeds the post's own record in a `Post.register(...)` call. Matching on the QUOTED
/// key is what keeps this off the sample: the page also carries "sample_width" and
/// "actual_preview_width", and neither has a quote immediately before "width", so neither can
/// match. (`[\s\x1c-\x1f]` is Python's `\s`.)
static JSON_WIDTH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""width"[\s\x1c-\x1f]*:[\s\x1c-\x1f]*(\d+)"#).expect("width regex")
});
static JSON_HEIGHT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""height"[\s\x1c-\x1f]*:[\s\x1c-\x1f]*(\d+)"#).expect("height regex")
});

const CATEGORY_MAP: [(&str, &str); 5] = [
    ("tag-type-general", "general"),
    ("tag-type-artist", "artist"),
    ("tag-type-copyright", "copyright"),
    ("tag-type-character", "character"),
    ("tag-type-circle", "circle"),
];

struct Selectors {
    tag_sidebar: Selector,
    li: Selector,
    tag_link: Selector,
    any_link: Selector,
    highres: Selector,
    image: Selector,
    stats: Selector,
}

static SELECTORS: LazyLock<Selectors> = LazyLock::new(|| {
    let parse = |css: &str| Selector::parse(css).expect("selector");
    Selectors {
        tag_sidebar: parse("ul#tag-sidebar"),
        li: parse("li"),
        tag_link: parse("a[href*='/post?tags=']"),
        any_link: parse("a"),
        highres: parse("#highres"),
        image: parse("#image"),
        stats: parse(STATS_SELECTOR),
    }
});

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    ..SiteParser::new("moebooru", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let sel = &*SELECTORS;
    let soup = soup_of(html);
    let mut tags = Vec::new();
    let Some(container) = soup.select(&sel.tag_sidebar).next() else {
        debug!("No #tag-sidebar container found on {url} (markup may have changed)");
        return tags;
    };
    for li in container.select(&sel.li) {
        let namespace = super::parsed::classes_of(li)
            .into_iter()
            .find_map(|c| {
                CATEGORY_MAP
                    .iter()
                    .find(|(k, _)| *k == c)
                    .map(|(_, ns)| *ns)
            })
            .unwrap_or("general");
        let Some(name_el) = li
            .select(&sel.tag_link)
            .next()
            .or_else(|| li.select(&sel.any_link).next())
        else {
            continue;
        };
        let name = text_of(name_el).replace(' ', "_");
        if !name.is_empty() {
            tags.push(Tag {
                name,
                namespace: Some(namespace.to_string()),
                source: TagSource::Booru,
            });
        }
    }
    tags
}

/// Moebooru sites link the full-resolution file from an anchor commonly id'd `highres` in the
/// sidebar; fall back to the on-page `#image` element (may be a resized sample) if that is not
/// present.
pub fn parse_file_url(html: &str, url: &str) -> Option<String> {
    let sel = &*SELECTORS;
    let soup = soup_of(html);
    if let Some(href) = soup
        .select(&sel.highres)
        .next()
        .and_then(|e| e.value().attr("href"))
        .filter(|v| !v.is_empty())
    {
        return Some(href.to_string());
    }
    if let Some(src) = soup
        .select(&sel.image)
        .next()
        .and_then(|e| e.value().attr("src"))
        .filter(|v| !v.is_empty())
    {
        return Some(src.to_string());
    }
    debug!("Could not find any direct file URL on {url}");
    None
}

/// The on-page `#image` element is the resized sample Moebooru shows by default: a good
/// mid-resolution preview, reusing the same HTML already fetched for tags and the file URL.
pub fn parse_preview_url(html: &str, _url: &str) -> Option<String> {
    let soup = soup_of(html);
    soup.select(&SELECTORS.image)
        .next()
        .and_then(|e| e.value().attr("src"))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The post's rating, off the Statistics sidebar's "Rating:" line: the same block the "Size: WxH"
/// scrape below already walks.
///
/// Read from the sidebar rather than from the embedded `Post.register` record, because the page
/// registers a record for every RELATED post as well as this one and each carries its own
/// "rating"; there is no way to tell from the JSON which one is the picture being looked at. The
/// Statistics block holds exactly one Rating line.
///
/// Scoped to that one ELEMENT rather than run over the page text. The page's text includes the
/// post's comments, and a comment is user-supplied prose: "mis-tagged, Rating: explicit surely?"
/// is an ordinary thing for someone to write, and the label pattern would match it. Today
/// yande.re happens to render comments after the sidebar, so the first match is still the right
/// one, but that is page layout, not a guarantee, and the failure mode is a wrong rating in the
/// user's library presented as fact. Reading the element that actually states it does not depend
/// on the ordering.
///
/// (Scripts are NOT the hazard here, checked rather than assumed: `parsed::text_joined` omits
/// `<script>` text, so neither the `Post.register` records nor the default blacklist yande.re
/// ships, which does contain the string "rating:e", can reach a page-text scrape at all.)
///
/// Words, not letters, so no per-site code table is needed here, which matters because
/// Moebooru's "s" is *safe* while Danbooru 2's is *sensitive*.
pub fn parse_rating(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);
    let Some(stats) = soup.select(&SELECTORS.stats).next() else {
        debug!("No {STATS_SELECTOR} Statistics block found on {url} (markup may have changed)");
        return None;
    };
    let found = rating::from_label(&text_joined(stats, " "));
    if found.is_none() {
        debug!("No 'Rating:' line in the Statistics block on {url}");
    }
    found
}

/// The ORIGINAL image's dimensions, never the sample's.
///
/// That distinction is the whole point: Moebooru shows a downscaled sample by default, so reading
/// width/height off the visible `<img>` would report the sample and make a genuinely larger match
/// look like a downgrade, the opposite of what the size comparison is for.
///
/// Three sources, all from the page already fetched for tags, so this costs no extra request:
///
/// 1. The embedded `Post.register` record, which states the original's width/height outright.
///    Both numbers are taken even when zero; a number too large for an `i64` is not read (Python
///    would return the big int).
/// 2. The "Size: WxH" Statistics sidebar, shared with the other Danbooru 1.x descendants. Zero
///    in either place is no answer, and the next source is tried.
/// 3. The full-resolution download link's own text, which usually reads like
///    "PNG (1500x2000, 2.5 MB)". Last because it is unlabelled digits: safe only because it is
///    read from that single link rather than the whole page.
pub fn parse_dimensions(html: &str, url: &str) -> (Option<i64>, Option<i64>) {
    let first_number = |re: &Regex| re.captures(html).and_then(|m| to_int(&m[1]));
    if let (Some(width), Some(height)) =
        (first_number(&JSON_WIDTH_RE), first_number(&JSON_HEIGHT_RE))
    {
        return (Some(width), Some(height));
    }

    let soup = soup_of(html);
    let text = page_text(&soup, " ");

    if let (Some(width), Some(height)) = parse_size_label(&text)
        && width != 0
        && height != 0
    {
        return (Some(width), Some(height));
    }

    if let Some(highres) = soup.select(&SELECTORS.highres).next()
        && let (Some(width), Some(height)) = parse_bare_dimensions(&text_joined(highres, " "))
        && width != 0
        && height != 0
    {
        return (Some(width), Some(height));
    }

    debug!(
        "No dimensions found on {url} - no Post.register record, no 'Size:' label, and no \
         dimensions in the highres link. Context: {:?}",
        describe_size_context(&text)
    );
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://yande.re/post/show/1";

    fn names(tags: &[Tag]) -> Vec<(&str, &str)> {
        tags.iter()
            .map(|t| (t.namespace.as_deref().unwrap(), t.name.as_str()))
            .collect()
    }

    #[test]
    fn claims_yande_re_and_konachan_only() {
        assert!(matches("https://yande.re/post/show/1"));
        assert!(matches("https://konachan.com/post/show/1"));
        assert!(matches("https://konachan.net/post/show/1"));
        assert!(matches("https://files.yande.re/image/x.png"));
        assert!(!matches("https://danbooru.donmai.us/posts/1"));
        assert!(!matches("https://example.com/yande.re/post/show/1"));
        assert!(!matches("https://e621.net/posts/219935"));
    }

    #[test]
    fn sidebar_tags_take_their_namespace_from_the_first_known_class() {
        let html = r#"<ul id="tag-sidebar">
          <li class="tag-type-artist"><a href="/wiki/show?title=x">?</a> <a href="/post?tags=an_artist">an artist</a></li>
          <li class="x tag-type-character tag-type-artist"><a href="/post?tags=c">c</a></li>
          <li class="tag-type-circle"><a href="/post?tags=ring">ring</a></li>
          <li><a href="/post?tags=plain">plain</a></li>
          <li class="tag-type-unknown"><a href="/post?tags=odd">odd</a></li>
          <li class="tag-type-copyright"><a href="/else">first any link</a></li>
        </ul>"#;
        assert_eq!(
            names(&parse(html, URL)),
            [
                ("artist", "an_artist"),
                ("character", "c"),
                ("circle", "ring"),
                ("general", "plain"),
                ("general", "odd"),
                ("copyright", "first_any_link"),
            ]
        );
    }

    #[test]
    fn a_sidebar_li_without_a_usable_name_is_skipped() {
        let html = r#"<ul id="tag-sidebar">
          <li class="tag-type-artist">no link</li>
          <li class="tag-type-artist"><a href="/post?tags=blank">   </a></li>
          <li class="tag-type-artist"><a href="/post?tags=ok">ok</a></li>
        </ul>"#;
        assert_eq!(names(&parse(html, URL)), [("artist", "ok")]);
        // No sidebar, or a tag-sidebar that is not a ul: nothing.
        assert!(parse("<div id='tag-sidebar'><li><a>x</a></li></div>", URL).is_empty());
        assert!(parse("", URL).is_empty());
    }

    #[test]
    fn a_nested_li_is_read_as_well_as_its_parent() {
        let html = r#"<ul id="tag-sidebar"><li class="tag-type-artist"><a href="/post?tags=a">a</a>
            <ul><li class="tag-type-circle"><a href="/post?tags=b">b</a></li></ul></li></ul>"#;
        assert_eq!(names(&parse(html, URL)), [("artist", "a"), ("circle", "b")]);
    }

    #[test]
    fn the_file_is_the_highres_link_else_the_page_image() {
        let both = r##"<a id="highres" href="https://files/full.png">x</a><img id="image" src="https://files/sample.jpg">"##;
        assert_eq!(
            parse_file_url(both, URL).as_deref(),
            Some("https://files/full.png")
        );
        assert_eq!(
            parse_preview_url(both, URL).as_deref(),
            Some("https://files/sample.jpg")
        );
        let image_only = r#"<img id="image" src="https://files/sample.jpg">"#;
        assert_eq!(
            parse_file_url(image_only, URL).as_deref(),
            Some("https://files/sample.jpg")
        );
        // A highres anchor with an empty or missing href does not stop the fallback, and the
        // first #highres is the one asked.
        let empty = r#"<a id="highres" href="">x</a><a id="highres" href="/later">x</a><img id="image" src="S">"#;
        assert_eq!(parse_file_url(empty, URL).as_deref(), Some("S"));
        assert_eq!(parse_file_url("<p></p>", URL), None);
        assert_eq!(parse_preview_url(r#"<img id="image" src="">"#, URL), None);
        assert_eq!(parse_preview_url("", URL), None);
    }

    #[test]
    fn the_rating_is_read_in_the_statistics_block_only() {
        let html = r#"<div id="comments">mis-tagged, Rating: Explicit surely?</div>
            <div id="stats"><ul><li>Id: 1</li><li>Size: 2x3</li><li>Rating: Safe</li></ul></div>"#;
        assert_eq!(parse_rating(html, URL).as_deref(), Some("safe"));
        // The label may straddle elements: the block's text is joined with a space.
        let split = r#"<div id="stats"><li>Rating:</li><li>Questionable</li></div>"#;
        assert_eq!(parse_rating(split, URL).as_deref(), Some("questionable"));
        // No block, or a block with no label, says nothing; the page text is never searched.
        assert_eq!(parse_rating("<p>Rating: Explicit</p>", URL), None);
        assert_eq!(
            parse_rating(r#"<div id="stats">Size: 2x3</div>"#, URL),
            None
        );
        // Script text does not count.
        let script = r#"<div id="stats"><script>Rating: Explicit</script></div>"#;
        assert_eq!(parse_rating(script, URL), None);
    }

    #[test]
    fn the_registered_record_states_the_original_and_never_the_sample() {
        let html = r#"<script>Post.register({"sample_width": 850, "sample_height": 600,
            "width": 1500, "height" : 2000, "actual_preview_width": 150})</script>
            <li>Size: 1x1</li>"#;
        assert_eq!(parse_dimensions(html, URL), (Some(1500), Some(2000)));
        // Both numbers are needed; one alone does not stop the search, and zero is accepted.
        let one = r#"{"width": 5} Size: 7x8"#;
        assert_eq!(parse_dimensions(one, URL), (Some(7), Some(8)));
        assert_eq!(
            parse_dimensions(r#"{"width": 0, "height":0}"#, URL),
            (Some(0), Some(0))
        );
        assert_eq!(
            parse_dimensions(r#"{"sample_width": 5, "sample_height": 6}"#, URL),
            (None, None)
        );
    }

    #[test]
    fn the_size_label_then_the_highres_text_are_the_fallbacks() {
        assert_eq!(
            parse_dimensions("<li>Size: 1232 \u{d7} 918</li>", URL),
            (Some(1232), Some(918))
        );
        let link = r##"<a id="highres" href="#">PNG (1500x2000, 2.5 MB)</a>"##;
        assert_eq!(parse_dimensions(link, URL), (Some(1500), Some(2000)));
        // A zero in the label is no answer, and the link is tried.
        let zero = r##"<li>Size: 0x5</li><a id="highres" href="#">JPG (30x40)</a>"##;
        assert_eq!(parse_dimensions(zero, URL), (Some(30), Some(40)));
        // Digits elsewhere on the page are not read.
        assert_eq!(parse_dimensions("<p>1500x2000</p>", URL), (None, None));
        assert_eq!(parse_dimensions("", URL), (None, None));
    }

    #[test]
    fn moebooru_defines_exactly_the_hooks_python_does() {
        assert_eq!(
            PARSER.defined_hooks(),
            [
                "parse_file_url",
                "parse_preview_url",
                "parse_dimensions",
                "parse_rating"
            ]
        );
    }
}
