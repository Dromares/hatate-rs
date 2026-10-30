//! Finding one setting among the Settings dialog's tabs: `gui/settings_search.py` at `76c85c4`
//! without the Qt widgets: the index over the names the built pages carry (`index_tabs`,
//! `_name_of`) and the matching (`matches`). The highlight (`highlight`) draws, so it is the
//! window's (`hatate-gtk`, `settings_dialog.rs`).
//!
//! Matching is on words, not the whole phrase: `quota pause` finds "Pause searching when the daily
//! quota runs out". The tab's name counts as part of what an entry is called, so `hydrus key`
//! finds the Hydrus tab's "Access key".

use crate::pycase::casefold;
use crate::pycompat;

/// One indexed setting: Python's `(tab index, tab name, name, widget)` tuple. `widget` is
/// whatever the window indexes, carried through untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingEntry<W> {
    pub tab_index: usize,
    pub tab_name: String,
    pub name: String,
    pub widget: W,
}

/// A label longer than this is an explanation, not a name (`MAX_NAME_LENGTH`).
pub const MAX_NAME_LENGTH: usize = 90;

/// `_name_of`: what a label, check box or group title is called in the results, or `None` when it
/// names nothing: empty, longer than [`MAX_NAME_LENGTH`], or fewer than three characters once the punctuation around them is gone (`:`, `…`, `ab`). Python's
/// comment lists `min` among these; the code keeps it, and so does this.
pub fn name_of(text: &str) -> Option<String> {
    let text = text.replace('&', "");
    let text = pycompat::strip(&text);
    if text.is_empty() || text.chars().count() > MAX_NAME_LENGTH {
        return None;
    }
    if text
        .trim_matches(|c| matches!(c, ' ' | ':' | '.' | '\u{2026}'))
        .chars()
        .count()
        < 3
    {
        return None;
    }
    Some(text.trim_end_matches(':').to_owned())
}

/// `index_tabs`: `pages` is, per tab, its name, the widget standing for the whole page, and the
/// `(text, widget)` of every label, check box and group title in it, in tree order. Every tab is
/// reachable by its own name, then each name once per tab (the first widget that carries it).
pub fn index_tabs<W: Clone>(pages: &[(&str, W, Vec<(String, W)>)]) -> Vec<SettingEntry<W>> {
    let mut found = Vec::new();
    for (tab_index, (tab_name, page, widgets)) in pages.iter().enumerate() {
        found.push(SettingEntry {
            tab_index,
            tab_name: (*tab_name).to_owned(),
            name: (*tab_name).to_owned(),
            widget: page.clone(),
        });
        let mut seen = std::collections::HashSet::new();
        for (text, widget) in widgets {
            let Some(name) = name_of(text) else { continue };
            if !seen.insert(casefold(&name)) {
                continue;
            }
            found.push(SettingEntry {
                tab_index,
                tab_name: (*tab_name).to_owned(),
                name,
                widget: widget.clone(),
            });
        }
    }
    found
}

/// `matches`: the entries that match every word of the query, in any order, shorter names first
/// (an exact-ish match is almost always a shorter string than a sentence that holds the same
/// words), ties by name. An empty query matches nothing.
pub fn matches<'a, W>(entries: &'a [SettingEntry<W>], query: &str) -> Vec<&'a SettingEntry<W>> {
    let folded = casefold(query);
    let words: Vec<&str> = pycompat::split(&folded).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<&SettingEntry<W>> = entries
        .iter()
        .filter(|e| {
            let haystack = casefold(&format!("{} {}", e.tab_name, e.name));
            words.iter().all(|w| haystack.contains(w))
        })
        .collect();
    out.sort_by(|a, b| (a.name.chars().count(), &a.name).cmp(&(b.name.chars().count(), &b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Vec<SettingEntry<u32>> {
        [
            (0, "General", "General", 1),
            (1, "Hydrus", "Access key", 2),
            (1, "Hydrus", "Hydrus API URL", 3),
            (
                2,
                "SauceNAO",
                "Pause searching when the daily quota runs out",
                4,
            ),
            (2, "SauceNAO", "Quota", 5),
        ]
        .into_iter()
        .map(|(tab_index, tab_name, name, widget)| SettingEntry {
            tab_index,
            tab_name: tab_name.to_owned(),
            name: name.to_owned(),
            widget,
        })
        .collect()
    }

    #[test]
    fn names_follow_name_of() {
        assert_eq!(name_of("&Hydrus key:").as_deref(), Some("Hydrus key"));
        assert_eq!(
            name_of("  Delay between searches:  ").as_deref(),
            Some("Delay between searches")
        );
        assert_eq!(name_of("min"), Some("min".to_owned()));
        assert_eq!(name_of("..."), None);
        assert_eq!(name_of(" : \u{2026} "), None);
        assert_eq!(name_of("ab:"), None);
        assert_eq!(name_of(""), None);
        assert_eq!(name_of(&"x".repeat(90)).map(|s| s.len()), Some(90));
        assert_eq!(name_of(&"x".repeat(91)), None);
        // the length is counted after the ampersands go
        assert_eq!(
            name_of(&format!("&{}", "x".repeat(90))).map(|s| s.len()),
            Some(90)
        );
    }

    #[test]
    fn the_index_lists_each_tab_then_each_name_once() {
        let pages = [
            (
                "General",
                0u32,
                vec![
                    ("Delay:".to_owned(), 1),
                    ("delay".to_owned(), 2),
                    ("min".to_owned(), 3),
                    ("..".to_owned(), 4),
                ],
            ),
            ("Hydrus", 10, vec![("Delay".to_owned(), 11)]),
        ];
        let index = index_tabs(&pages);
        let names: Vec<(usize, &str, &str, u32)> = index
            .iter()
            .map(|e| (e.tab_index, e.tab_name.as_str(), e.name.as_str(), e.widget))
            .collect();
        assert_eq!(
            names,
            vec![
                (0, "General", "General", 0),
                (0, "General", "Delay", 1),
                (0, "General", "min", 3),
                (1, "Hydrus", "Hydrus", 10),
                (1, "Hydrus", "Delay", 11),
            ]
        );
    }

    fn found(query: &str) -> Vec<u32> {
        matches(&index(), query).iter().map(|e| e.widget).collect()
    }

    #[test]
    fn every_word_must_match_in_any_order() {
        assert_eq!(found("quota pause"), vec![4]);
        assert_eq!(found("pause quota daily"), vec![4]);
        assert_eq!(found("pause nonsense"), Vec::<u32>::new());
    }

    #[test]
    fn the_tab_name_is_part_of_what_an_entry_is_called() {
        assert_eq!(found("hydrus key"), vec![2]);
        assert_eq!(found("saucenao"), vec![5, 4], "shorter names first");
    }

    #[test]
    fn an_empty_query_matches_nothing_and_case_is_folded() {
        assert!(found("").is_empty());
        assert!(found("   ").is_empty());
        assert_eq!(found("ACCESS"), vec![2]);
    }

    #[test]
    fn equal_lengths_sort_by_name() {
        let entries: Vec<SettingEntry<()>> = ["bb", "ab", "ba"]
            .iter()
            .map(|n| SettingEntry {
                tab_index: 0,
                tab_name: "T".to_owned(),
                name: (*n).to_owned(),
                widget: (),
            })
            .collect();
        let names: Vec<&str> = matches(&entries, "t")
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, vec!["ab", "ba", "bb"]);
    }
}
