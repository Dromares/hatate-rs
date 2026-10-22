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
