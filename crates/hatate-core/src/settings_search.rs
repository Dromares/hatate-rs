//! Finding one setting among the Settings dialog's tabs: the matching half of
//! `gui/settings_search.py` (`matches`). The index over the built widgets (`index_tabs`,
//! `_name_of`) and the highlight are Phase 6.h's.
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
