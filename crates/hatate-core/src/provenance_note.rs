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
    use crate::similarity_display::ESTIMATE_MARK;

    const STAMP: &str = "2026-03-04 17:08";

    /// `_Fixture._entry`: one candidate, selected.
    fn entry(
        measured: bool,
        similarity: f64,
        engine: &str,
        site: Option<&str>,
        url: &str,
    ) -> Entry {
        let mut e = Entry::new("/x/a.png");
        let mut c = Candidate::new(url);
        c.engine = engine.to_owned();
        c.similarity = similarity;
        c.similarity_measured = measured;
        c.source_name = site.map(str::to_owned);
        e.candidates.push(c);
        e
    }

    fn text(e: &Entry) -> Option<String> {
        provenance_note(e, || STAMP.to_owned())
    }

    fn iqdb() -> Entry {
        entry(
            true,
            92.0,
            "IQDB",
            Some("Danbooru"),
            "https://danbooru.donmai.us/posts/123",
        )
    }

    #[test]
    fn it_records_the_five_facts_and_says_which_program_wrote_it() {
        let t = text(&iqdb()).unwrap();
        for fact in [
            "Engine: IQDB",
            "Site: Danbooru",
            "92%",
            "https://danbooru.donmai.us/posts/123",
            STAMP,
        ] {
            assert!(t.contains(fact), "{fact}: {t}");
        }
        assert!(t.starts_with("Match found by Hatate-Linux-Redux."));
    }

    #[test]
    fn a_measured_similarity_is_plain_and_an_unmeasured_one_is_marked_and_explained() {
        let t = text(&iqdb()).unwrap();
        assert!(t.contains(&format!("Similarity: 92% - {MEASURED_NOTE}")));
        assert!(!t.contains(ESTIMATE_MARK));
        let t = text(&entry(
            false,
            80.0,
            "ascii2d",
            Some("Danbooru"),
            "https://e.example/1",
        ))
        .unwrap();
        assert!(t.contains("~80%") && t.contains(UNMEASURED_NOTE), "{t}");
    }

    #[test]
    fn a_missing_fact_is_left_out_rather_than_written_as_unknown() {
        let t = text(&entry(true, 92.0, "IQDB", None, "https://e.example/1")).unwrap();
        assert!(!t.contains("Site:") && !t.contains("None"), "{t}");
        assert!(t.contains("Engine: IQDB"));
    }

    #[test]
    fn either_half_of_a_match_on_its_own_is_still_worth_a_note() {
        // A URL with no engine behind it, and an engine with the URL cleared.
        let t = text(&entry(true, 50.0, "", None, "https://example.com/posts/1")).unwrap();
        assert!(
            t.contains("https://example.com/posts/1") && !t.contains("Engine:"),
            "{t}"
        );
        let t = text(&entry(true, 92.0, "IQDB", None, "")).unwrap();
        assert!(t.contains("Engine: IQDB") && !t.contains("Match:"), "{t}");
    }

    #[test]
    fn no_match_means_no_note_at_all() {
        assert_eq!(text(&Entry::new("/x/a.png")), None);
    }

    #[test]
    fn the_note_name_falls_back_when_blank_and_the_default_says_whose_it_is() {
        assert_eq!(note_name(""), DEFAULT_NOTE_NAME);
        assert_eq!(note_name("   "), DEFAULT_NOTE_NAME);
        assert_eq!(note_name(" my notes "), "my notes");
        assert!(DEFAULT_NOTE_NAME.to_lowercase().contains("hatate"));
    }
}
