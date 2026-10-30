//! The Add tags dialog's parse (`gui/add_tags_dialog.py` `AddTagsDialog.get_tags` at `76c85c4`;
//! DAN-2219). The dialog itself is the prompter's ([`crate::dialogs::Prompter::add_tags`]), so a
//! test can answer it with text.

use hatate_core::model::{Tag, TagSource};
use hatate_core::pycompat::strip;

/// `str.splitlines()`: the line boundaries Python knows, not only `\n`.
pub(crate) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        lines.push(&text[start..i]);
        let mut end = i + c.len_utf8();
        if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
            chars.next();
            end += 1;
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// `AddTagsDialog.get_tags`: one tag per non-blank line, `namespace:name` or just `name`. Both
/// halves are stripped, so `character : reimu` is `character:reimu` and not a second namespace
/// `character ` beside the real one. An empty left half is no namespace at all, so the tag
/// de-duplicates against the same one typed without a colon.
pub fn parse_added_tags(text: &str) -> Vec<Tag> {
    splitlines(text)
        .into_iter()
        .map(strip)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (namespace, name) = match line.split_once(':') {
                Some((head, name)) => {
                    let head = strip(head);
                    (
                        (!head.is_empty()).then(|| head.to_owned()),
                        strip(name).to_owned(),
                    )
                }
                None => (None, line.to_owned()),
            };
            Tag {
                name,
                namespace,
                source: TagSource::User,
            }
        })
        .collect()
}

/// `Add tags to {target_description} (one per line, optionally namespace:tag):`.
pub fn prompt(target_description: &str) -> String {
    format!("Add tags to {target_description} (one per line, optionally namespace:tag):")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(text: &str) -> Vec<(Option<String>, String)> {
        parse_added_tags(text)
            .into_iter()
            .map(|t| (t.namespace, t.name))
            .collect()
    }

    #[test]
    fn blank_lines_are_skipped_and_both_halves_are_stripped() {
        assert_eq!(
            pairs("  cat \n\n character : reimu \n:bare\n"),
            vec![
                (None, "cat".to_owned()),
                (Some("character".to_owned()), "reimu".to_owned()),
                (None, "bare".to_owned()),
            ]
        );
    }

    #[test]
    fn only_the_first_colon_splits() {
        assert_eq!(
            pairs("series:a:b"),
            vec![(Some("series".to_owned()), "a:b".to_owned())]
        );
    }

    #[test]
    fn python_line_boundaries_end_a_tag() {
        assert_eq!(
            pairs("a\r\nb\rc\u{2028}d\u{85}e"),
            ["a", "b", "c", "d", "e"]
                .iter()
                .map(|n| (None, (*n).to_owned()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_tag_is_a_user_tag() {
        assert!(
            parse_added_tags("x\ny")
                .iter()
                .all(|t| t.source == TagSource::User)
        );
    }

    #[test]
    fn the_target_is_named_in_the_prompt() {
        assert_eq!(
            prompt("7 selected images"),
            "Add tags to 7 selected images (one per line, optionally namespace:tag):"
        );
    }

    #[test]
    fn a_colon_alone_is_an_empty_name() {
        // Python keeps `Tag(name="", namespace=None)` for a line that is only ":".
        assert_eq!(pairs(":"), vec![(None, String::new())]);
    }
}
