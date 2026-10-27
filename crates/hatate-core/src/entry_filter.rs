//! Which entries the list shows (`core/entry_filter.py`): a plain predicate over [`Entry`], with
//! no widget in it. A filter narrows what is displayed and nothing else; it never removes or
//! changes an entry. The filter bar of the window is a view of [`EntryFilter`].
//!
//! `None` for `statuses`, `sites` or `upscale_verdicts` means "not filtering on this", which is
//! different from an empty set: that means "nothing is allowed" and matches no row.
//!
//! Not the same language as the MCP tools' `_matches_filter` (`mcp/tool_handlers.rs`).

use std::collections::HashSet;

use crate::model::{Entry, MatchStatus};
use crate::pycompat;
use crate::sites::classify_site;

/// `NO_SITE`: the site shown for an entry with no match yet. Distinct from `"Other"`, which
/// means "matched something we have no name for".
pub const NO_SITE: &str = "(no match)";

/// `UPSCALE_NOT_CHECKED`: the label for an entry the upscale check has never run on.
pub const UPSCALE_NOT_CHECKED: &str = "Not checked";

/// `_UPSCALE_ORDER`: worst news first.
const UPSCALE_ORDER: [&str; 3] = ["Flagged", "Clear", UPSCALE_NOT_CHECKED];

/// `EntryFilter`. An empty filter (the default) matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryFilter {
    pub statuses: Option<HashSet<MatchStatus>>,
    pub sites: Option<HashSet<String>>,
    pub upscale_verdicts: Option<HashSet<String>>,
    pub text: String,
}

impl EntryFilter {
    /// `is_active`: whether this filter hides anything. Drives the "showing N of M" readout.
    pub fn is_active(&self) -> bool {
        self.statuses.is_some()
            || self.sites.is_some()
            || self.upscale_verdicts.is_some()
            || !pycompat::strip(&self.text).is_empty()
    }

    /// `matches`.
    pub fn matches(&self, entry: &Entry) -> bool {
        if let Some(statuses) = &self.statuses {
            if !statuses.contains(&entry.status) {
                return false;
            }
        }
        if let Some(sites) = &self.sites {
            if !sites.contains(&site_of(entry)) {
                return false;
            }
        }
        if let Some(verdicts) = &self.upscale_verdicts {
            if !verdicts.contains(upscale_verdict_label(entry)) {
                return false;
            }
        }
        let needle = pycompat::strip(&self.text).to_lowercase();
        if !needle.is_empty() && !entry.filename().to_lowercase().contains(&needle) {
            return false;
        }
        true
    }

    /// `apply`: the entries that pass, in list order.
    pub fn apply<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        if !self.is_active() {
            return entries.iter().collect();
        }
        entries.iter().filter(|e| self.matches(e)).collect()
    }

    /// The list positions of [`EntryFilter::apply`]'s result. The window never maps a row number
    /// to an entry except through this view, so an action cannot reach a row the user cannot see.
    pub fn apply_indices(&self, entries: &[Entry]) -> Vec<usize> {
        if !self.is_active() {
            return (0..entries.len()).collect();
        }
        entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.matches(e))
            .map(|(i, _)| i)
            .collect()
    }
}

/// `site_of`: which site an entry's current match came from (the selected candidate, because the
/// list shows one match per row).
pub fn site_of(entry: &Entry) -> String {
    match entry.selected_candidate() {
        Some(c) if !c.url.is_empty() => classify_site(c.source_name.as_deref(), &c.url),
        _ => NO_SITE.to_owned(),
    }
}

/// `sites_present`: the sites represented in the list, sorted, `NO_SITE` last.
pub fn sites_present(entries: &[Entry]) -> Vec<String> {
    let seen: HashSet<String> = entries.iter().map(site_of).collect();
    let mut named: Vec<String> = seen.iter().filter(|s| *s != NO_SITE).cloned().collect();
    named.sort();
    if seen.contains(NO_SITE) {
        named.push(NO_SITE.to_owned());
    }
    named
}

/// `statuses_present`: the statuses represented, in the enum's own order.
pub fn statuses_present(entries: &[Entry]) -> Vec<MatchStatus> {
    MatchStatus::ALL
        .into_iter()
        .filter(|s| entries.iter().any(|e| e.status == *s))
        .collect()
}

/// `upscale_verdict_label`: `"Flagged"`, `"Clear"`, or `"Not checked"` for the never-run case and
/// for any other value.
pub fn upscale_verdict_label(entry: &Entry) -> &'static str {
    match entry.upscale_verdict.as_deref() {
        Some("flagged") => "Flagged",
        Some("clear") => "Clear",
        _ => UPSCALE_NOT_CHECKED,
    }
}

/// `upscale_verdicts_present`: the labels represented, worst news first.
pub fn upscale_verdicts_present(entries: &[Entry]) -> Vec<&'static str> {
    UPSCALE_ORDER
        .into_iter()
        .filter(|label| entries.iter().any(|e| upscale_verdict_label(e) == *label))
        .collect()
}

/// `summarise_selection` (`gui/filter_bar.py`): the text of a filter button, `"Status: All"`,
/// `"Site: Danbooru"`, `"Status: 3 selected"`. `chosen` of `None` means "not filtering", and so
/// does a chosen set that covers everything present.
pub fn summarise_selection(
    prefix: &str,
    chosen: Option<&HashSet<String>>,
    present: &HashSet<String>,
) -> String {
    match chosen {
        Some(chosen) if !chosen.is_superset(present) => match chosen.len() {
            1 => format!("{prefix}: {}", chosen.iter().next().unwrap()),
            n => format!("{prefix}: {n} selected"),
        },
        _ => format!("{prefix}: All"),
    }
}

/// `describe_hidden`: says plainly that the rest are hidden, not gone (the count's tooltip).
pub fn describe_hidden(shown: usize, total: usize) -> String {
    format!(
        "showing {} of {} (the rest are hidden, not removed)",
        crate::eta::thousands(shown as i64),
        crate::eta::thousands(total as i64)
    )
}

/// `hidden_count_text`: the reading on the right of the bar, `"3 hidden"`.
pub fn hidden_count_text(shown: usize, total: usize) -> String {
    format!(
        "{} hidden",
        crate::eta::thousands(total.saturating_sub(shown) as i64)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Candidate;

    fn strs(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_filter_button_says_all_for_none_and_for_everything_ticked() {
        let present = strs(&["Danbooru", "Safebooru"]);
        assert_eq!(summarise_selection("Site", None, &present), "Site: All");
        assert_eq!(
            summarise_selection(
                "Site",
                Some(&strs(&["Danbooru", "Safebooru", "Gone"])),
                &present
            ),
            "Site: All"
        );
        assert_eq!(
            summarise_selection("Site", Some(&strs(&["Danbooru"])), &present),
            "Site: Danbooru"
        );
        assert_eq!(
            summarise_selection("Status", Some(&HashSet::new()), &present),
            "Status: 0 selected"
        );
        assert_eq!(
            summarise_selection("Status", Some(&strs(&["a", "b"])), &strs(&["a", "b", "c"])),
            "Status: 2 selected"
        );
        // Nothing present and nothing chosen is still "everything covered".
        assert_eq!(
            summarise_selection("Status", Some(&HashSet::new()), &HashSet::new()),
            "Status: All"
        );
    }

    #[test]
    fn the_hidden_count_is_total_minus_shown_with_thousands() {
        assert_eq!(hidden_count_text(3, 5), "2 hidden");
        assert_eq!(hidden_count_text(5, 5), "0 hidden");
        assert_eq!(hidden_count_text(2, 5_000), "4,998 hidden");
        assert_eq!(hidden_count_text(9, 3), "0 hidden");
        assert_eq!(
            describe_hidden(1_200, 5_000),
            "showing 1,200 of 5,000 (the rest are hidden, not removed)"
        );
    }

    fn entry(path: &str, status: MatchStatus) -> Entry {
        let mut e = Entry::new(path);
        e.status = status;
        e
    }

    fn with_match(path: &str, url: &str, source: Option<&str>) -> Entry {
        let mut e = entry(path, MatchStatus::Good);
        let mut c = Candidate::new(url);
        c.source_name = source.map(str::to_owned);
        e.candidates.push(c);
        e
    }

    fn set<T: std::hash::Hash + Eq>(items: impl IntoIterator<Item = T>) -> Option<HashSet<T>> {
        Some(items.into_iter().collect())
    }

    #[test]
    fn an_empty_filter_matches_everything_and_is_inactive() {
        let f = EntryFilter::default();
        assert!(!f.is_active());
        let list = vec![entry("/a/x.png", MatchStatus::Error)];
        assert_eq!(f.apply_indices(&list), vec![0]);
        let blank = EntryFilter {
            text: " \t ".to_owned(),
            ..EntryFilter::default()
        };
        assert!(!blank.is_active(), "whitespace is not a filter");
    }

    #[test]
    fn an_empty_set_is_not_the_same_as_no_filter() {
        let list = vec![entry("/a/x.png", MatchStatus::Good)];
        let none_allowed = EntryFilter {
            statuses: set([]),
            ..EntryFilter::default()
        };
        assert!(none_allowed.is_active());
        assert!(none_allowed.apply(&list).is_empty());
    }

    #[test]
    fn status_site_and_verdict_each_narrow_the_list() {
        let mut flagged = with_match("/a/one.png", "https://danbooru.donmai.us/posts/1", None);
        flagged.upscale_verdict = Some("flagged".to_owned());
        let list = vec![
            flagged,
            entry("/a/two.png", MatchStatus::NotFound),
            with_match("/a/three.png", "https://example.invalid/x", Some("Nowhere")),
        ];
        let by_status = EntryFilter {
            statuses: set([MatchStatus::NotFound]),
            ..EntryFilter::default()
        };
        assert_eq!(by_status.apply_indices(&list), vec![1]);
        let by_site = EntryFilter {
            sites: set(["Danbooru".to_owned()]),
            ..EntryFilter::default()
        };
        assert_eq!(by_site.apply_indices(&list), vec![0]);
        let no_match = EntryFilter {
            sites: set([NO_SITE.to_owned()]),
            ..EntryFilter::default()
        };
        assert_eq!(no_match.apply_indices(&list), vec![1]);
        let by_verdict = EntryFilter {
            upscale_verdicts: set(["Flagged".to_owned()]),
            ..EntryFilter::default()
        };
        assert_eq!(by_verdict.apply_indices(&list), vec![0]);
        let unchecked = EntryFilter {
            upscale_verdicts: set([UPSCALE_NOT_CHECKED.to_owned()]),
            ..EntryFilter::default()
        };
        assert_eq!(unchecked.apply_indices(&list), vec![1, 2]);
    }

    #[test]
    fn text_is_a_trimmed_case_insensitive_filename_substring() {
        let list = vec![
            entry("/pics/Cat Photo.PNG", MatchStatus::Good),
            entry("/cat/dog.png", MatchStatus::Good),
        ];
        let f = EntryFilter {
            text: "  cat p ".to_owned(),
            ..EntryFilter::default()
        };
        assert_eq!(
            f.apply_indices(&list),
            vec![0],
            "the folder name is not searched"
        );
    }

    #[test]
    fn the_menus_offer_only_what_is_present_in_a_fixed_order() {
        let list = vec![
            entry("/a", MatchStatus::Error),
            with_match("/b", "https://safebooru.org/index.php?id=1", None),
            with_match("/c", "https://danbooru.donmai.us/posts/2", None),
            with_match("/d", "https://example.invalid/", None),
        ];
        assert_eq!(
            sites_present(&list),
            vec!["Danbooru", "Other", "Safebooru", NO_SITE]
        );
        assert_eq!(
            statuses_present(&list),
            vec![MatchStatus::Good, MatchStatus::Error]
        );
        assert_eq!(upscale_verdicts_present(&list), vec![UPSCALE_NOT_CHECKED]);
        let mut clear = entry("/e", MatchStatus::Good);
        clear.upscale_verdict = Some("clear".to_owned());
        let mut flagged = entry("/f", MatchStatus::Good);
        flagged.upscale_verdict = Some("flagged".to_owned());
        assert_eq!(
            upscale_verdicts_present(&[clear, flagged, entry("/g", MatchStatus::Good)]),
            vec!["Flagged", "Clear", UPSCALE_NOT_CHECKED]
        );
    }

    #[test]
    fn an_unknown_verdict_reads_as_not_checked() {
        let mut e = entry("/a", MatchStatus::Good);
        e.upscale_verdict = Some("maybe".to_owned());
        assert_eq!(upscale_verdict_label(&e), UPSCALE_NOT_CHECKED);
    }

    #[test]
    fn only_the_selected_candidates_site_counts() {
        let mut e = with_match("/a/x.png", "https://danbooru.donmai.us/posts/1", None);
        e.candidates
            .push(Candidate::new("https://safebooru.org/index.php?id=2"));
        assert_eq!(site_of(&e), "Danbooru");
        e.select_candidate(1);
        assert_eq!(site_of(&e), "Safebooru");
        let by_site = EntryFilter {
            sites: set(["Danbooru".to_owned()]),
            ..EntryFilter::default()
        };
        assert!(
            !by_site.matches(&e),
            "the first candidate's site is not shown, so not matched"
        );
    }
}
