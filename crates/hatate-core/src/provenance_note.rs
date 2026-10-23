//! What this app knows about where a file came from, as text for a Hydrus note
//! (`core/provenance_note.py`). The one place that decides the wording.
//!
//! Three rules, all about not overstating what is known: the similarity carries its honesty
//! marker (see [`crate::similarity_display`]); a missing fact is left out, not written as
//! "unknown"; and with no match at all there is no note.

use crate::engine_ids::reports_real_similarity;
use crate::model::Entry;
use crate::similarity_display::{MEASURED_NOTE, UNMEASURED_NOTE, similarity_label};

/// The note's name in Hydrus, and so the note this app owns and overwrites.
pub const DEFAULT_NOTE_NAME: &str = "hatate: match source";

pub const FIRST_LINE: &str = "Match found by Hatate-Linux-Redux.";

/// `note_name`: the configured name, or the default for a blank one. A blank setting means "I did
/// not choose", not "write the note with no name".
pub fn note_name(configured: &str) -> String {
    let stripped = crate::pycompat::strip(configured);
    if stripped.is_empty() {
        DEFAULT_NOTE_NAME.to_owned()
    } else {
        stripped.to_owned()
    }
}

/// `provenance_note`: the note's text for this entry, or `None` when there is nothing worth
/// writing. `sent_at` is the moment being recorded (epoch seconds), `stamp` renders it as local
/// time (`%Y-%m-%d %H:%M`: to the minute, biggest unit first).
///
/// The entry's `booru_name`, `similarity` and `similarity_measured` are the selected candidate's
/// own (`ImageEntry.select_candidate` copies them), so they are read from it.
pub fn provenance_note(entry: &Entry, local_stamp: impl FnOnce() -> String) -> Option<String> {
    let selected = entry.selected_candidate();
    let engine = selected.map_or("", |c| c.engine.as_str());
    let matched_url = entry.matched_url().filter(|u| !u.is_empty());
    if matched_url.is_none() && engine.is_empty() {
        return None;
    }

    let mut lines: Vec<String> = vec![FIRST_LINE.to_owned()];
    if !engine.is_empty() {
        lines.push(format!("Engine: {engine}"));
    }
    if let Some(site) = selected
        .and_then(|c| c.source_name.as_deref())
        .filter(|s| !s.is_empty())
    {
        lines.push(format!("Site: {site}"));
    }
    if let Some(c) = selected {
        let measured = c.similarity_measured || reports_real_similarity(Some(&c.engine));
        let label = similarity_label(Some(c.similarity), measured);
        let note = if measured {
            MEASURED_NOTE
        } else {
            UNMEASURED_NOTE
        };
        lines.push(format!("Similarity: {label} - {note}"));
    }
    if let Some(url) = matched_url {
        lines.push(format!("Match: {url}"));
    }
    lines.push(format!("Sent to Hydrus: {}", local_stamp()));
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Candidate, Entry};

    fn entry(engine: &str, url: &str) -> Entry {
        let mut e = Entry::new("/x/a.png");
        let mut c = Candidate::new(url);
        c.engine = engine.to_owned();
        c.similarity = 92.0;
        e.candidates.push(c);
        e
    }

    #[test]
    fn blank_name_is_the_default() {
        assert_eq!(note_name(""), DEFAULT_NOTE_NAME);
        assert_eq!(note_name("  \t "), DEFAULT_NOTE_NAME);
        assert_eq!(note_name("  mine "), "mine");
    }

    #[test]
    fn no_match_means_no_note() {
        assert_eq!(
            provenance_note(&Entry::new("/x/a.png"), || "t".into()),
            None
        );
    }

    #[test]
    fn text_has_the_lines_in_order() {
        let text = provenance_note(&entry("SauceNAO", "https://e.example/p/1"), || {
            "2026-10-23 12:34".into()
        })
        .unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], FIRST_LINE);
        assert_eq!(lines[1], "Engine: SauceNAO");
        assert_eq!(lines[2], format!("Similarity: 92% - {MEASURED_NOTE}"));
        assert_eq!(lines[3], "Match: https://e.example/p/1");
        assert_eq!(lines[4], "Sent to Hydrus: 2026-10-23 12:34");
    }
}
