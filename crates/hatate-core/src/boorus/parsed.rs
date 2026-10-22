//! One way into a page body for every hook (`core/boorus/_parsed.py`).
//!
//! A parser is a set of hooks (`parse`, `parse_file_url`, `parse_dimensions`, ...), each handed
//! the same raw body. Python caches the parse by the body string, because rule34us built four
//! BeautifulSoup trees per page. Here each hook parses on its own: the functions are pure, take
//! the body as a `&str` and hold nothing between calls. If a profile ever shows the repeated
//! parse matters, cache behind these two functions; no hook signature has to change.
//!
//! HTML is parsed by html5ever (through `scraper`), JSON by `serde_json`. Python used lxml and
//! `json.loads`. Where they differ, `docs/parsers.md` says how.

use scraper::{ElementRef, Html, Node};
use serde_json::Value;

use crate::pycompat;

/// The tree of this page.
pub fn soup_of(html: &str) -> Html {
    Html::parse_document(html)
}

/// `json.loads(body)`. An error means "this is not JSON", which the hooks that fall back to HTML
/// read as a signal, so it is returned and not logged here.
///
/// Differences from `json.loads` that no corpus item reaches: Python also accepts `NaN`,
/// `Infinity` and `-Infinity`, and lone surrogate escapes such as `"\ud800"`; this does not, and
/// the body then counts as "not JSON".
pub fn json_of(body: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(body)
}

/// Every string a page shows under `el`, in document order. A string that bs4 files under
/// `<script>`, `<style>` or `<template>` is not page text and is left out, however deep inside it
/// sits and wherever the tag being read is (bs4 decides when it parses, from the innermost such
/// tag still open). A comment is not text either.
fn shown_strings(el: ElementRef<'_>) -> impl Iterator<Item = &str> {
    el.descendants().filter_map(|node| match node.value() {
        Node::Text(text) => {
            let hidden = node.ancestors().any(|a| {
                a.value()
                    .as_element()
                    .is_some_and(|e| matches!(e.name(), "script" | "style" | "template"))
            });
            (!hidden).then_some(&**text)
        }
        _ => None,
    })
}

/// bs4's `tag.get_text(strip=True)`: every string under the tag, each stripped, empty ones
/// dropped, joined with nothing.
pub fn text_of(el: ElementRef<'_>) -> String {
    shown_strings(el)
        .map(pycompat::strip)
        .filter(|s| !s.is_empty())
        .collect()
}

/// bs4's `tag.get_text(separator)`: every string under the tag, unstripped, joined with
/// `separator`. The Danbooru 1.x sidebar scrapes read the whole page this way.
pub fn text_joined(el: ElementRef<'_>, separator: &str) -> String {
    shown_strings(el).collect::<Vec<_>>().join(separator)
}

/// bs4's `tag.get("class")`: the class names, split as Python splits a string.
pub fn classes_of(el: ElementRef<'_>) -> Vec<&str> {
    el.value()
        .attr("class")
        .map(|value| pycompat::split(value).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Selector;

    fn first<'a>(html: &'a Html, css: &str) -> ElementRef<'a> {
        html.select(&Selector::parse(css).unwrap()).next().unwrap()
    }

    #[test]
    fn text_is_stripped_per_string_and_joined_with_nothing() {
        let html = soup_of("<a id=a> some <b> artist </b>\n <i>x</i> </a>");
        assert_eq!(text_of(first(&html, "#a")), "someartistx");
    }

    #[test]
    fn joined_text_keeps_every_string_unstripped() {
        let html = soup_of("<div id=d> a <b>b</b><script>x</script> c</div>");
        assert_eq!(text_joined(first(&html, "#d"), " "), " a  b  c");
        assert_eq!(text_joined(first(&html, "#d"), ""), " a b c");
    }

    #[test]
    fn script_style_and_comments_are_not_text() {
        let html = soup_of("<div id=d>a<script>var x</script><style>p{}</style><!-- c -->b</div>");
        assert_eq!(text_of(first(&html, "#d")), "ab");
    }

    #[test]
    fn template_text_is_hidden_whatever_the_tag_being_read() {
        let html = soup_of(
            "<template><p id=p>in <b>deep</b></p></template><div id=d><template>t</template>u</div>",
        );
        // html5ever keeps template content in a fragment; bs4 marks it by the tag it was inside.
        assert_eq!(text_of(first(&html, "#d")), "u");
        assert_eq!(text_of(first(&html, "#p")), "");
    }

    #[test]
    fn classes_split_on_python_whitespace() {
        let html = soup_of("<li id=l class=' category-1\u{1f}tag  x '></li><li id=m></li>");
        assert_eq!(classes_of(first(&html, "#l")), ["category-1", "tag", "x"]);
        assert!(classes_of(first(&html, "#m")).is_empty());
    }

    #[test]
    fn json_errors_mean_not_json() {
        assert!(json_of("<html></html>").is_err());
        assert!(json_of("").is_err());
        assert_eq!(json_of(" {\"id\": 1} \n").unwrap()["id"], 1);
    }
}
