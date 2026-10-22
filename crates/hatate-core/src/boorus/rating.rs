//! A post's age rating, where the site actually states one (`core/boorus/_rating.py`).
//!
//! The same one-letter code does not mean the same thing everywhere: Danbooru's "s" is
//! *sensitive* and e621's "s" is *safe*, so a single shared letter table would silently
//! mislabel one of them. Each parser hands over its OWN vocabulary and this module only
//! normalises the result into tag-shaped text. A site whose response has no rating field this
//! app can point at contributes nothing: guessing a rating is worse than having none.
//!
//! Two ways in:
//!
//! - [`from_code`] for the JSON APIs, which state the rating as a code in a named field
//!   (Danbooru, e621).
//! - [`from_label`] for the Danbooru 1.x descendants, whose Statistics sidebar spells it out
//!   (`Rating: Explicit`) in the block the `Size: WxH` scrape already walks.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::pycompat;

/// Anchored to the sidebar's own label rather than hunting the page for a rating word: a booru
/// page is full of prose (comments, related tags). A single word only, so a one-line layout does
/// not swallow the next element.
static RATING_LABEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bRating:\s*([A-Za-z]+)").expect("rating label regex"));

/// Tag-shaped: lowercase, underscores for spaces. `None` if nothing usable is left, so a blank
/// field never becomes a blank tag.
fn as_tag_name(value: &str) -> Option<String> {
    let name = pycompat::strip(value).to_lowercase().replace(' ', "_");
    (!name.is_empty()).then_some(name)
}

/// A site's own rating code, translated through that site's own table (`(code, name)` pairs).
///
/// A value already spelled out in full ("explicit") passes through, since several of these APIs
/// have widened from letters to words and both forms are unambiguous. An unrecognised code
/// yields nothing rather than a guess: a new code is a category this app has never seen, and
/// inventing a name for it is the mislabelling this module exists to avoid. Anything that is not
/// a JSON string (`null`, a number) is `None`, as in Python.
pub fn from_code(raw: &Value, vocabulary: &[(&str, &str)]) -> Option<String> {
    let code = pycompat::strip(raw.as_str()?).to_lowercase();
    if code.is_empty() {
        return None;
    }
    if let Some((_, name)) = vocabulary.iter().find(|(k, _)| *k == code) {
        return Some((*name).to_string());
    }
    vocabulary
        .iter()
        .any(|(_, name)| *name == code)
        .then_some(code)
}

/// The rating off a `Rating: Explicit` Statistics line, or `None`.
pub fn from_label(text: &str) -> Option<String> {
    as_tag_name(RATING_LABEL_RE.captures(text)?.get(1)?.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DANBOORU: &[(&str, &str)] = &[
        ("g", "general"),
        ("s", "sensitive"),
        ("q", "questionable"),
        ("e", "explicit"),
    ];

    #[test]
    fn a_code_goes_through_the_sites_own_table() {
        assert_eq!(
            from_code(&json!("s"), DANBOORU).as_deref(),
            Some("sensitive")
        );
        assert_eq!(
            from_code(&json!(" E "), DANBOORU).as_deref(),
            Some("explicit")
        );
        // Spelled out in full passes through, in any case.
        assert_eq!(
            from_code(&json!("Explicit"), DANBOORU).as_deref(),
            Some("explicit")
        );
        // A table where "s" means safe gives another answer for the same letter.
        assert_eq!(
            from_code(&json!("s"), &[("s", "safe")]).as_deref(),
            Some("safe")
        );
    }

    #[test]
    fn an_unknown_or_non_string_code_is_nothing() {
        for raw in [
            json!("z"),
            json!(""),
            json!("   "),
            json!(null),
            json!(3),
            json!(["s"]),
        ] {
            assert_eq!(from_code(&raw, DANBOORU), None, "{raw}");
        }
    }

    #[test]
    fn the_statistics_label_is_read_in_its_own_block() {
        assert_eq!(
            from_label("Id: 1\nSize: 2x3\nRating: Explicit\n").as_deref(),
            Some("explicit")
        );
        assert_eq!(from_label("rating:   SAFE").as_deref(), Some("safe"));
        // One word only: the element after it is not swallowed.
        assert_eq!(
            from_label("Rating: Explicit Score: 3").as_deref(),
            Some("explicit")
        );
        assert_eq!(from_label("a Rating: 12"), None);
        assert_eq!(from_label("no label here"), None);
        assert_eq!(from_label(""), None);
        // A longer word that merely ends in the label does not count (`\b`).
        assert_eq!(from_label("Prerating: Explicit"), None);
    }
}
