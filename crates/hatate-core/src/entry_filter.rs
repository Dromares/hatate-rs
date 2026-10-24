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
