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
