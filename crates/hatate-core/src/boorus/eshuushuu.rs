//! e-shuushuu (`core/boorus/eshuushuu.py`).
//!
//! The site was rebuilt at some point after the Python app's booru parsers were first written, so
//! nothing here matches the old e-shuushuu markup that tutorials and older scrapers describe.
//! What the current site serves:
//!
//! - Post pages live at `/images/{id}` (plural). The older `/image/{id}` form still appears in
//!   search-engine results; the match is on the host, so both reach this parser.
//! - Every tag on a post page is a link to `/search?tags={tag_id}`, followed by a separate "jump
//!   to tag page" link to `/tags/{tag_id}`.
//! - The full-resolution file is a direct link to `cdn.e-shuushuu.net/fullsize/{date}-{id}.{ext}`,
//!   and the thumbnail is the same path with `/thumbs/` and a `.webp` extension.
//!
//! Selectors key off those URL PATTERNS rather than CSS classes or element ids, because a restyle
//! changes class names far more often than it changes route shapes.
//!
//! NAMESPACES. e-shuushuu classifies its tags (Artist / Source / Character / Theme) but the post
//! page's tag links carry no per-tag type marker. The classification is recoverable from the
//! page's own `og:description`:
//!
//! ```text
//! Cute anime artwork by <ARTIST> from <SOURCE>. 800×600. Tagged: <THEMES>
//! Cute anime artwork by <ARTIST> of <CHARACTER>. 984×1392. Tagged: <THEMES>
//! ```
//!
//! The tag NAMES are taken from the links (authoritative and complete); the description is only
//! asked where each already-known name sits: inside the "Tagged:" run, or right after "by" / "of"
//! / "from". A name found in none of those positions is left unnamespaced rather than guessed at,
//! because a wrong namespace is worse than none.
//!
//! Every hook is a pure function of the body and the post-page URL; the site keeps no state.
//!
//! **Python's `re` rules.** The description is matched with `re.IGNORECASE` patterns, a
//! lookahead and `\b`, none of which the `regex` crate has in Python's meaning. They are written
//! out by hand over characters, with the case rule and the word-character rule of
//! [`crate::pycase`], so that a tag called `Ⅷ`, `ı` or `İ` is judged as Python judges it.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use scraper::{Html, Selector};
use tracing::debug;

use super::SiteParser;
use super::host::on_host;
use super::parsed::{page_text, soup_of, text_joined, text_of};
use super::sizes::{parse_bare_dimensions, py_int};
use crate::model::{Tag, TagSource};
use crate::pycase::{casefold, ignorecase_matches, is_word_char};
use crate::{pycompat, pyurl};

const HOSTS: [&str; 1] = ["e-shuushuu.net"];

/// "Dimensions: 800 × 600" on the post page, matched against the page's flattened text rather
/// than a specific element, so the label and value being in separate tags does not matter.
///
/// Python's pattern is `Dimensions:\s*([\d,]+)\s*[×x]\s*([\d,]+)` with `re.IGNORECASE`. `(?i)`
/// is not that: `re.IGNORECASE` also lets `i` match `ı` and `İ`, so the letters that have such
/// extra cases are spelled out. (`[\s\x1c-\x1f]` is Python's `\s`.)
static DIMENSIONS_RE: LazyLock<Regex> = LazyLock::new(|| {
    let ws = r"[\s\x1c-\x1f]";
    Regex::new(&format!(
        "[Dd][iI\u{130}\u{131}][mM][eE][nN][sS\u{17f}][iI\u{130}\u{131}][oO][nN][sS\u{17f}]:\
         {ws}*([\\d,]+){ws}*[\u{d7}xX]{ws}*([\\d,]+)"
    ))
    .expect("dimensions regex")
});

/// Where a tag sits in og:description -> the namespace the rest of the app uses. In the order
/// Python's dict lists them, which is the order they are tried in.
const ROLE_NAMESPACES: [(&str, &str); 3] =
    [("by", "creator"), ("of", "character"), ("from", "series")];

struct Selectors {
    tag_link: Selector,
    og_description: Selector,
    meta_description: Selector,
    fullsize_link: Selector,
    fullsize_img: Selector,
    og_image: Selector,
    thumbs_img: Selector,
    dimensions_class: Selector,
}

static SELECTORS: LazyLock<Selectors> = LazyLock::new(|| {
    let parse = |css: &str| Selector::parse(css).expect("selector");
    Selectors {
        tag_link: parse(r#"a[href*="search?tags="]"#),
        og_description: parse(r#"meta[property="og:description"]"#),
        meta_description: parse(r#"meta[name="description"]"#),
        fullsize_link: parse(r#"a[href*="/fullsize/"]"#),
        fullsize_img: parse(r#"img[src*="/fullsize/"]"#),
        og_image: parse(r#"meta[property="og:image"]"#),
        thumbs_img: parse(r#"img[src*="/thumbs/"]"#),
        dimensions_class: parse(r#"[class*="dimensions"]"#),
    }
});

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(|body, url, _| parse_file_url(body, url)),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    ..SiteParser::new("eshuushuu", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &HOSTS)
}

/// The page's own description of the image, which is where the namespaces are read from. The
/// first of the two `<meta>` tags that exists is the one used, even when it has no `content`.
fn description(soup: &Html) -> String {
    let selectors = &*SELECTORS;
    soup.select(&selectors.og_description)
        .next()
        .or_else(|| soup.select(&selectors.meta_description).next())
        .and_then(|meta| meta.value().attr("content"))
        .unwrap_or("")
        .to_string()
}

/// Python's `\s` over characters.
fn is_space(c: char) -> bool {
    pycompat::is_space(c)
}

/// `\b` at `at`: exactly one side is a word character; the ends of the text count as non-word.
fn at_boundary(chars: &[char], at: usize) -> bool {
    let before = at > 0 && is_word_char(chars[at - 1]);
    let after = chars.get(at).copied().is_some_and(is_word_char);
    before != after
}

/// A literal under `re.IGNORECASE`: the position after it, if `chars` has it at `at`.
fn literal_at(chars: &[char], at: usize, literal: &str) -> Option<usize> {
    let mut pos = at;
    for expected in literal.chars() {
        if !ignorecase_matches(expected, *chars.get(pos)?) {
            return None;
        }
        pos += 1;
    }
    Some(pos)
}

/// `\s+` followed by whatever `then` accepts, with the backtracking a regex does: the longest
/// run of whitespace first, then shorter ones.
fn spaces_then(chars: &[char], at: usize, then: impl Fn(usize) -> bool) -> bool {
    let run = chars[at..].iter().take_while(|c| is_space(**c)).count();
    (1..=run).rev().any(|n| then(at + n))
}

/// `(?=$|[.,]|\s+(?:by|of|from)\b)` at `at`. Python's `$` also matches just before a final
/// newline.
fn name_ends_here(chars: &[char], at: usize) -> bool {
    let end = chars.len();
    if at == end || (at + 1 == end && chars[at] == '\n') {
        return true;
    }
    if matches!(chars[at], '.' | ',') {
        return true;
    }
    spaces_then(chars, at, |next| {
        ROLE_NAMESPACES
            .iter()
            .any(|(word, _)| literal_at(chars, next, word).is_some_and(|e| at_boundary(chars, e)))
    })
}

/// `re.search(r"\b" + preposition + r"\s+" + re.escape(name) + r"(?=...)", head, re.I)`: does
/// `name` follow `preposition` somewhere in `head`, ending at a real boundary?
fn role_matches(head: &[char], preposition: &str, name: &str) -> bool {
    (0..=head.len()).any(|start| {
        if !at_boundary(head, start) {
            return false;
        }
        let Some(after_word) = literal_at(head, start, preposition) else {
            return false;
        };
        spaces_then(head, after_word, |at| {
            literal_at(head, at, name).is_some_and(|end| name_ends_here(head, end))
        })
    })
}

/// Which namespace a known tag name belongs to, judged only by where that exact name appears in
/// og:description. `None` when the name is not found in any recognised position, so an
/// unfamiliar description format degrades to flat tags rather than mislabelled ones.
fn namespace_for(name: &str, head: &[char], themes: &HashSet<String>) -> Option<&'static str> {
    // Checked first: a theme that happens to also read like a role should stay a theme, since
    // the Tagged: run is the unambiguous signal.
    if themes.contains(&casefold(name)) {
        return None;
    }
    ROLE_NAMESPACES
        .iter()
        .find(|(preposition, _)| role_matches(head, preposition, name))
        .map(|(_, namespace)| *namespace)
}

/// `re.split(r"\bTagged:\s*", description, maxsplit=1, flags=re.I)`: the text before the first
/// "Tagged:" and, when there is one, the text after it.
fn split_at_tagged(description: &str) -> (String, Option<String>) {
    let chars: Vec<char> = description.chars().collect();
    for start in 0..chars.len() {
        if !at_boundary(&chars, start) {
            continue;
        }
        if let Some(mut end) = literal_at(&chars, start, "Tagged:") {
            while chars.get(end).copied().is_some_and(is_space) {
                end += 1;
            }
            return (
                chars[..start].iter().collect(),
                Some(chars[end..].iter().collect()),
            );
        }
    }
    (description.to_string(), None)
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let soup = soup_of(html);

    // Each tag links to its search page. The adjacent /tags/{id} link is a "go to tag page"
    // affordance, not a second tag, so it is excluded: including it would duplicate every tag
    // under a numeric name.
    let anchors: Vec<_> = soup.select(&SELECTORS.tag_link).collect();
    if anchors.is_empty() {
        debug!("No tag links found on {url} (markup may have changed)");
        return Vec::new();
    }

    let description = description(&soup);
    let (head, tagged) = split_at_tagged(&description);
    let head: Vec<char> = head.chars().collect();
    let themes: HashSet<String> = tagged
        .iter()
        .flat_map(|rest| rest.split(','))
        .map(pycompat::strip)
        .filter(|t| !t.is_empty())
        .map(|t| casefold(t.trim_end_matches('.')))
        .collect();

    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let mut namespaced = 0;
    for anchor in anchors {
        let name = text_of(anchor);
        if name.is_empty() {
            continue;
        }
        // The page also carries navigational links to the same route (a tag sidebar, "related
        // tags", etc). Deduplicate by name so a tag listed twice becomes one Tag.
        if !seen.insert(casefold(&name)) {
            continue;
        }
        let namespace = namespace_for(&name, &head, &themes);
        if namespace.is_some() {
            namespaced += 1;
        }
        // Underscored to match every other parser, so a tag from here dedupes against the same
        // tag from another site.
        tags.push(Tag {
            name: name.replace(' ', "_"),
            namespace: namespace.map(String::from),
            source: TagSource::Booru,
        });
    }

    if !tags.is_empty() && namespaced == 0 && !description.is_empty() {
        debug!(
            "No tag on {url} could be classified from its description - e-shuushuu may have \
             changed its og:description format; tags are still returned, just unnamespaced"
        );
    }
    tags
}

/// The full-resolution file, linked directly from the post page as `cdn.e-shuushuu.net/fullsize/`
/// - not to be confused with the `/thumbs/` URL, which is the small webp preview.
pub fn parse_file_url(html: &str, url: &str) -> Option<String> {
    let soup = soup_of(html);

    let link = soup
        .select(&SELECTORS.fullsize_link)
        .next()
        .and_then(|a| a.value().attr("href"))
        .filter(|href| !href.is_empty());
    if let Some(href) = link {
        return Some(absolute(href));
    }

    // Some layouts show the full image inline rather than as a link.
    let img = soup
        .select(&SELECTORS.fullsize_img)
        .next()
        .and_then(|img| img.value().attr("src"))
        .filter(|src| !src.is_empty());
    if let Some(src) = img {
        return Some(absolute(src));
    }

    debug!("Could not find a /fullsize/ file URL on {url}");
    None
}

/// The CDN's own webp thumbnail. Worth using as the preview because e-shuushuu's full-resolution
/// files can be very large (tens of megabytes).
pub fn parse_preview_url(html: &str, _url: &str) -> Option<String> {
    let soup = soup_of(html);

    let meta = soup
        .select(&SELECTORS.og_image)
        .next()
        .and_then(|m| m.value().attr("content"))
        .filter(|content| !content.is_empty());
    if let Some(content) = meta {
        return Some(absolute(content));
    }

    soup.select(&SELECTORS.thumbs_img)
        .next()
        .and_then(|img| img.value().attr("src"))
        .filter(|src| !src.is_empty())
        .map(absolute)
}

pub fn parse_dimensions(html: &str, _url: &str) -> (Option<i64>, Option<i64>) {
    let soup = soup_of(html);
    if let Some(found) = DIMENSIONS_RE.captures(&page_text(&soup, " ")) {
        // `int()` of a run that is only commas raises, and Python gives up there: it does not
        // go on to the element below.
        let number = |m: usize| py_int(&found[m].replace(',', ""));
        return match (number(1), number(2)) {
            (Some(width), Some(height)) => (Some(width), Some(height)),
            _ => (None, None),
        };
    }

    // The site's redesign moved the word "Dimensions" out of the text and into a class name,
    // leaving just "640 × 480" on its own, so the label-anchored pattern above stopped matching
    // and every match arrived with no size at all. Read the element itself instead.
    match soup.select(&SELECTORS.dimensions_class).next() {
        Some(element) => parse_bare_dimensions(&text_joined(element, " ")),
        None => (None, None),
    }
}

/// The CDN links are absolute already, but protocol-relative ("//cdn...") URLs would otherwise be
/// handed downstream as-is and fail to fetch, so they are normalised to https.
fn absolute(href: &str) -> String {
    let stripped = pycompat::strip(href);
    let href = if href.contains('%') {
        pyurl::unquote(stripped)
    } else {
        stripped.to_string()
    };
    if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://e-shuushuu.net{href}")
    } else {
        href
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boorus::find_parser;

    const URL: &str = "https://e-shuushuu.net/images/331875";

    /// The recorded post page of `tests/test_parsers_and_tags.py::ESHUUSHUU_POST_HTML` (image
    /// #331875, whose tag set was cross-checked against e-shuushuu's own Atom feed).
    const POST_HTML: &str = r#"
<html><head>
<meta property="og:image" content="https://cdn.e-shuushuu.net/thumbs/2010-10-07-331875.webp">
<meta property="og:description" content="Cute anime artwork by Sumomo KPA from Cabal Online. 800&times;600. Tagged: black hair, gloves, long hair, smile, tree, yellow eyes.">
</head><body>
<main class="some-class-that-may-change">
  <a href="https://cdn.e-shuushuu.net/fullsize/2010-10-07-331875.jpeg"><img
     src="https://cdn.e-shuushuu.net/thumbs/2010-10-07-331875.webp"></a>
  <section><h2>Tags</h2>
    <a href="https://e-shuushuu.net/search?tags=2457">Sumomo KPA</a><a href="https://e-shuushuu.net/tags/2457">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=12878">Cabal Online</a><a href="https://e-shuushuu.net/tags/12878">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=181">black hair</a><a href="https://e-shuushuu.net/tags/181">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=1343">gloves</a><a href="https://e-shuushuu.net/tags/1343">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=46">long hair</a><a href="https://e-shuushuu.net/tags/46">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=143">smile</a><a href="https://e-shuushuu.net/tags/143">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=126">tree</a><a href="https://e-shuushuu.net/tags/126">&rsaquo;</a>
    <a href="https://e-shuushuu.net/search?tags=12524">yellow eyes</a><a href="https://e-shuushuu.net/tags/12524">&rsaquo;</a>
  </section>
  <dl><dt>Dimensions:</dt><dd>800 &times; 600</dd></dl>
</main></body></html>
"#;

    /// A page with these tag links and this description, and nothing else.
    fn page(description: &str, tags: &[&str]) -> String {
        let links: String = tags
            .iter()
            .enumerate()
            .map(|(i, t)| format!(r#"<a href="/search?tags={i}">{t}</a>"#))
            .collect();
        format!(
            r#"<html><head><meta property="og:description" content="{description}"></head><body>{links}</body></html>"#
        )
    }

    fn namespaces(tags: &[Tag]) -> Vec<(&str, Option<&str>)> {
        tags.iter()
            .map(|t| (t.name.as_str(), t.namespace.as_deref()))
            .collect()
    }

    fn namespace_of(tags: &[Tag], name: &str) -> Option<String> {
        tags.iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no tag {name}"))
            .namespace
            .clone()
    }

    // ---- tests/test_parsers_and_tags.py::TestEshuushuuParser

    #[test]
    fn extracts_the_real_tag_set() {
        let names: Vec<_> = parse(POST_HTML, URL).into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Sumomo_KPA",
                "Cabal_Online",
                "black_hair",
                "gloves",
                "long_hair",
                "smile",
                "tree",
                "yellow_eyes"
            ]
        );
    }

    #[test]
    fn tag_page_jump_links_are_not_treated_as_tags() {
        let names: Vec<_> = parse(POST_HTML, URL).into_iter().map(|t| t.name).collect();
        assert_eq!(names.len(), 8);
        assert!(names.iter().all(|n| {
            !n.trim_matches(['\u{203a}', '>', ' '])
                .chars()
                .all(|c| c.is_ascii_digit())
        }));
    }

    #[test]
    fn namespaces_match_the_atom_feeds_classification() {
        let tags = parse(POST_HTML, URL);
        assert_eq!(
            namespace_of(&tags, "Sumomo_KPA").as_deref(),
            Some("creator")
        );
        assert_eq!(
            namespace_of(&tags, "Cabal_Online").as_deref(),
            Some("series")
        );
        for theme in [
            "black_hair",
            "gloves",
            "long_hair",
            "smile",
            "tree",
            "yellow_eyes",
        ] {
            assert_eq!(namespace_of(&tags, theme), None, "{theme}");
        }
    }

    #[test]
    fn character_role_is_distinguished_from_source() {
        let html = POST_HTML.replace(
            "by Sumomo KPA from Cabal Online.",
            "by Sumomo KPA of Cabal Online.",
        );
        assert_eq!(
            namespace_of(&parse(&html, URL), "Cabal_Online").as_deref(),
            Some("character")
        );
    }

    #[test]
    fn unrecognised_description_yields_flat_tags_not_wrong_ones() {
        let html = POST_HTML.replace(
            "Cute anime artwork by Sumomo KPA from Cabal Online. 800&times;600. \
             Tagged: black hair, gloves, long hair, smile, tree, yellow eyes.",
            "An entirely different sentence shape.",
        );
        let tags = parse(&html, URL);
        assert_eq!(tags.len(), 8);
        assert!(tags.iter().all(|t| t.namespace.is_none()));
    }

    #[test]
    fn artist_name_containing_a_preposition() {
        let html = page(
            "Cute anime artwork by Fagi of Note from Touhou. 1&times;1. Tagged: smile.",
            &["Fagi of Note", "Touhou", "smile"],
        );
        let tags = parse(&html, URL);
        assert_eq!(
            namespace_of(&tags, "Fagi_of_Note").as_deref(),
            Some("creator")
        );
        assert_eq!(namespace_of(&tags, "Touhou").as_deref(), Some("series"));
    }

    #[test]
    fn short_name_does_not_claim_a_longer_names_role() {
        let html = page(
            "Cute anime artwork by Fagimoto. 1&times;1. Tagged: hat.",
            &["Fagi", "Fagimoto", "hat"],
        );
        let tags = parse(&html, URL);
        assert_eq!(namespace_of(&tags, "Fagimoto").as_deref(), Some("creator"));
        assert_eq!(namespace_of(&tags, "Fagi"), None);
    }

    #[test]
    fn regex_metacharacters_in_tag_names() {
        let html = page(
            "Cute anime artwork by Someone. 1&times;1. Tagged: :3, ^_^.",
            &[":3", "^_^"],
        );
        let tags = parse(&html, URL);
        assert_eq!(namespaces(&tags), [(":3", None), ("^_^", None)]);
    }

    #[test]
    fn file_url_is_the_fullsize_file_not_the_thumbnail() {
        let got = parse_file_url(POST_HTML, URL).unwrap();
        assert_eq!(
            got,
            "https://cdn.e-shuushuu.net/fullsize/2010-10-07-331875.jpeg"
        );
        assert!(!got.contains("/thumbs/"));
    }

    #[test]
    fn preview_url_is_the_thumbnail() {
        let got = parse_preview_url(POST_HTML, URL).unwrap();
        assert!(got.contains("/thumbs/"));
    }

    #[test]
    fn dimensions() {
        assert_eq!(parse_dimensions(POST_HTML, URL), (Some(800), Some(600)));
    }

    #[test]
    fn protocol_relative_urls_are_normalized() {
        let html = r#"<a href="//cdn.e-shuushuu.net/fullsize/x.jpg">f</a>"#;
        assert_eq!(
            parse_file_url(html, URL).as_deref(),
            Some("https://cdn.e-shuushuu.net/fullsize/x.jpg")
        );
    }

    #[test]
    fn parser_is_registered_for_both_url_forms() {
        for url in [
            "https://e-shuushuu.net/images/331875",
            "https://e-shuushuu.net/image/331875",
        ] {
            assert_eq!(find_parser(url).map(|p| p.name), Some("eshuushuu"), "{url}");
        }
    }

    #[test]
    fn empty_page_returns_no_tags_without_failing() {
        assert!(parse("<html><body></body></html>", URL).is_empty());
        assert_eq!(parse_file_url("<html></html>", URL), None);
    }

    // ---- tests/test_boorus_harness.py::test_every_row (construction half)

    #[test]
    fn claims_its_own_host_and_not_another() {
        assert!(matches("https://e-shuushuu.net/image/331875"));
        assert!(matches("https://www.e-shuushuu.net/"));
        assert!(!matches("https://danbooru.donmai.us/posts/1"));
        assert!(!matches("https://e-shuushuu.net.evil.example/"));
        assert!(!matches("https://evil.example/e-shuushuu.net"));
    }

    // ---- what the differential against Python found worth pinning

    #[test]
    fn a_name_is_matched_as_python_ignores_case() {
        // `re.IGNORECASE`: `İ` and `ı` read as `i`, and `ſ` as `s`.
        let html = page("artwork by \u{130}\u{131}", &["ii"]);
        assert_eq!(
            namespace_of(&parse(&html, URL), "ii").as_deref(),
            Some("creator")
        );
        let html = page("artwork by ſ from K", &["s", "\u{212a}"]);
        let tags = parse(&html, URL);
        assert_eq!(namespace_of(&tags, "s").as_deref(), Some("creator"));
        assert_eq!(namespace_of(&tags, "\u{212a}").as_deref(), Some("series"));
    }

    #[test]
    fn a_theme_is_judged_by_casefold_not_lowercase() {
        // `ß` casefolds to `ss`, so the theme "STRASSE" is the tag "Straße".
        let html = page("by Straße. Tagged: STRASSE", &["Straße"]);
        assert_eq!(namespace_of(&parse(&html, URL), "Straße"), None);
        // Tags that casefold alike are one tag.
        let html = page("", &["Straße", "STRASSE", "straSSe"]);
        assert_eq!(parse(&html, URL).len(), 1);
    }

    #[test]
    fn the_name_must_end_at_a_boundary_python_would_see() {
        // `$` also matches before a final newline; a following word does not end the name.
        assert_eq!(
            namespace_of(&parse(&page("by x\n", &["x"]), URL), "x").as_deref(),
            Some("creator")
        );
        assert_eq!(
            namespace_of(&parse(&page("by x\n\n", &["x"]), URL), "x"),
            None
        );
        assert_eq!(
            namespace_of(&parse(&page("by xy", &["x", "xy"]), URL), "x"),
            None
        );
        // `by` inside a word is not a preposition, and a combining mark is not a word character.
        assert_eq!(
            namespace_of(&parse(&page("thereby x", &["x"]), URL), "x"),
            None
        );
        assert_eq!(
            namespace_of(&parse(&page("a\u{301}by x", &["x"]), URL), "x").as_deref(),
            Some("creator")
        );
    }

    #[test]
    fn roles_chain_with_the_next_preposition_or_punctuation() {
        let tags = parse(&page("by a of b from c", &["a", "b", "c"]), URL);
        assert_eq!(
            namespaces(&tags),
            [
                ("a", Some("creator")),
                ("b", Some("character")),
                ("c", Some("series"))
            ]
        );
        let tags = parse(&page("by a, of b.", &["a", "b"]), URL);
        assert_eq!(
            namespaces(&tags),
            [("a", Some("creator")), ("b", Some("character"))]
        );
    }

    #[test]
    fn the_first_description_meta_wins_even_without_content() {
        let html = r#"<head><meta property="og:description"><meta name="description" content="by x"></head><a href="/search?tags=1">x</a>"#;
        assert_eq!(namespace_of(&parse(html, URL), "x"), None);
        let html = r#"<head><meta name="description" content="by x"></head><a href="/search?tags=1">x</a>"#;
        assert_eq!(
            namespace_of(&parse(html, URL), "x").as_deref(),
            Some("creator")
        );
    }

    #[test]
    fn dimensions_fall_back_to_the_element_and_give_up_on_bad_numbers() {
        assert_eq!(
            parse_dimensions(
                r#"<div class="image-dimensions">640 &times; 480</div>"#,
                URL
            ),
            (Some(640), Some(480))
        );
        assert_eq!(
            parse_dimensions("<p>Dimensions: 1,024 x 768</p>", URL),
            (Some(1024), Some(768))
        );
        assert_eq!(
            parse_dimensions("<p>dimensions: 800 X 600</p>", URL),
            (Some(800), Some(600))
        );
        // The label matched but its number is only commas: Python's `int()` raises and it does
        // not go on to the element.
        assert_eq!(
            parse_dimensions(
                r#"<p>Dimensions: ,, x 5</p><div class="dimensions">640 x 480</div>"#,
                URL
            ),
            (None, None)
        );
        assert_eq!(parse_dimensions("<p>nothing here</p>", URL), (None, None));
    }

    #[test]
    fn urls_are_unquoted_and_stripped_like_python() {
        assert_eq!(
            parse_file_url(r#"<a href="  /fullsize/a%20b.jpg  ">f</a>"#, URL).as_deref(),
            Some("https://e-shuushuu.net/fullsize/a b.jpg")
        );
        assert_eq!(
            parse_preview_url(
                r#"<meta property="og:image" content=""><img src="/thumbs/t.webp">"#,
                URL
            )
            .as_deref(),
            Some("https://e-shuushuu.net/thumbs/t.webp")
        );
        assert_eq!(
            parse_file_url(r#"<img src="//c/fullsize/i.jpg">"#, URL).as_deref(),
            Some("https://c/fullsize/i.jpg")
        );
    }
}
