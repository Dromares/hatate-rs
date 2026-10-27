//! The Queue's file actions (`gui/main_window.py` at `76c85c4`): Export Results
//! (`action_export_results`, 1657), Write Tag Files (`action_write_tag_files` 1712 with
//! `_ask_about_writing_tag_files` 1757) and Clear Search Cache (`action_clear_search_cache`,
//! 2247). The texts and the decisions are `hatate_core::sidecar` and `hatate_core::export`; this
//! file puts the questions and the results on screen. Each takes the ids of the rows it acts on
//! (`None` is the whole list), the way the Files menu and the right-click menu call them.

use crate::dialogs::{self, Buttons, Kind, Response};
use crate::error::{CacheError, ExportError};
use crate::window::MainWindow;
use hatate_core::cache::{CacheKey, SearchCache};
use hatate_core::export;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::queue::{EntryId, RemoveReason, has_result, needs_review};
use hatate_core::sidecar::{self, Question};
use std::path::{Path, PathBuf};

/// The export dialog's filter (`FILE_DIALOG_FILTER`).
pub const EXPORT_FILTER: &str = "CSV spreadsheet (*.csv);;JSON (*.json);;All files (*)";

/// The format a save dialog's answer means: the filter names it, and the extension only decides
/// it for a name typed by hand under "All files" (`action_export_results`, 1685-1692).
pub fn export_format(chosen_filter: &str, path: &str) -> &'static str {
    let filter = chosen_filter.to_lowercase();
    if filter.contains("json") {
        "json"
    } else if filter.contains("csv") {
        "csv"
    } else {
        export::format_for_path(path)
    }
}

/// `path` with `.fmt` on the end unless it already ends in it (case-insensitively).
pub fn with_export_extension(path: PathBuf, fmt: &str) -> PathBuf {
    if path
        .to_string_lossy()
        .to_lowercase()
        .ends_with(&format!(".{fmt}"))
    {
        return path;
    }
    let mut os = path.into_os_string();
    os.push(format!(".{fmt}"));
    PathBuf::from(os)
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

impl MainWindow {
    /// The entries these ids name that are still in the list, in the order given
    /// (`_live_entries`); `None` is the whole list.
    fn entries_for(&self, ids: Option<&[EntryId]>) -> Vec<Entry> {
        let q = self.s.queue.borrow();
        match ids {
            None => q.entries().to_vec(),
            Some(ids) => ids.iter().filter_map(|id| q.entry(*id).cloned()).collect(),
        }
    }

    /// `action_export_results(entries)`.
    pub fn action_export_results(&self, ids: Option<&[EntryId]>) {
        let rows = self.entries_for(ids);
        if rows.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Export Results",
                "There's nothing in the list to export.",
            );
            return;
        }
        let stamp = hatate_core::localtime::local_stamp((self.s.clock)());
        let (date, time) = stamp.split_once(' ').unwrap_or((&stamp, "00:00:00"));
        let default_name = format!(
            "hatate-results-{}-{}.csv",
            date.replace('-', ""),
            time[..5].replace(':', "")
        );
        let start = std::env::var_os("HOME")
            .map_or_else(|| PathBuf::from("/"), PathBuf::from)
            .join(default_name);
        let w = self.clone();
        self.s.prompter.save_file_with_filter(
            "Export Results",
            &start.to_string_lossy(),
            EXPORT_FILTER,
            Box::new(move |picked| {
                let Some((path, chosen)) = picked else { return };
                let fmt = export_format(&chosen, &path.to_string_lossy());
                let path = with_export_extension(path, fmt);
                let text = {
                    let settings = w.s.settings.lock().unwrap();
                    // `generated_at` follows the window's clock, as the default file name does.
                    if fmt == "json" {
                        export::to_json(rows.iter(), Some((w.s.clock)()), Some(&settings))
                    } else {
                        export::render(rows.iter(), fmt, Some(&settings))
                    }
                };
                // newline="": the writer's \r\n goes out as it is.
                match std::fs::write(&path, text.as_bytes()) {
                    Ok(()) => {
                        tracing::info!(
                            "Exported {} result(s) as {} to {}",
                            rows.len(),
                            fmt.to_uppercase(),
                            path.display()
                        );
                        w.say(&format!(
                            "Exported {} image(s) to {}",
                            rows.len(),
                            base_name(&path)
                        ));
                    }
                    Err(e) => {
                        let err = ExportError::new(&path, e);
                        tracing::error!("Could not export results: {}", err.detail());
                        dialogs::warning(
                            &*w.s.prompter,
                            "Export Results",
                            &format!(
                                "Could not write the results to:\n{}\n\n{}",
                                path.display(),
                                err.user_message()
                            ),
                        );
                    }
                }
            }),
        );
    }

    /// `action_write_tag_files(entries)`.
    pub fn action_write_tag_files(&self, ids: Option<&[EntryId]>) {
        let rows = self.entries_for(ids);
        if rows.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Write Tag Files",
                "There's nothing in the list to write tag files for.",
            );
            return;
        }
        let settings = self.s.settings.lock().unwrap().clone();
        let plan = sidecar::plan(rows.iter(), Some(&settings), sidecar::DEFAULT_EXTENSION);
        if plan.writes.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Write Tag Files",
                &sidecar::nothing_to_write_text(&plan),
            );
            return;
        }
        let confirmation =
            sidecar::confirmation(&plan, sidecar::overwrite_policy_of(Some(&settings)));
        let (buttons, text) = match &confirmation.question {
            Question::YesNo { .. } => (Buttons::YesNo, confirmation.text.clone()),
            // Qt's informative text sits under the body; the alert has one body.
            Question::ThreeWay { informative } => (
                Buttons::KeepReplaceCancel,
                format!("{}\n\n{informative}", confirmation.text),
            ),
        };
        let w = self.clone();
        let question = confirmation.question.clone();
        let rows_for_write = rows.clone();
        self.s.prompter.message(
            Kind::Question,
            "Write Tag Files",
            &text,
            buttons,
            Box::new(move |answer| {
                let overwrite = match (&question, answer) {
                    (Question::YesNo { yes_overwrites }, Response::Yes) => Some(*yes_overwrites),
                    (Question::ThreeWay { .. }, Response::Keep) => Some(false),
                    (Question::ThreeWay { .. }, Response::Replace) => Some(true),
                    _ => None,
                };
                let Some(overwrite) = overwrite else { return };
                // The plan borrowed the rows; plan again over the copy this closure owns.
                let settings = w.s.settings.lock().unwrap().clone();
                let plan = sidecar::plan(
                    rows_for_write.iter(),
                    Some(&settings),
                    sidecar::DEFAULT_EXTENSION,
                );
                let result = sidecar::write(&plan.writes, overwrite);
                if let Some(first) = result.failures.first() {
                    dialogs::warning(
                        &*w.s.prompter,
                        "Write Tag Files",
                        &format!(
                            "Wrote {} tag file(s), but {} could not be written.\n\nFor \
                             example:\n{}\n{}\n\nSee Help > View Logs for the rest.",
                            result.written.len(),
                            result.failures.len(),
                            first.path.display(),
                            first.user_message()
                        ),
                    );
                }
                w.say(&sidecar::written_summary(&plan, &result));
            }),
        );
    }

    /// The ids of `ids` that are still in the list, in the order given (`_live_entries`).
    fn live_ids(&self, ids: &[EntryId]) -> Vec<EntryId> {
        let q = self.s.queue.borrow();
        ids.iter()
            .copied()
            .filter(|id| q.entry(*id).is_some())
            .collect()
    }

    /// `_select_rows_by_status(status)`.
    pub fn select_rows_by_status(&self, status: MatchStatus) {
        self.select_rows_by_predicate(&format!("status \"{}\"", status.label()), |e| {
            e.status == status
        });
    }

    /// `_select_rows_by_predicate(description, predicate)`: every row the predicate matches,
    /// replacing the selection. A row the filter hides cannot be selected, so it is not counted.
    pub fn select_rows_by_predicate(&self, description: &str, predicate: impl Fn(&Entry) -> bool) {
        let matching: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            self.s
                .queue_view
                .visible_ids()
                .into_iter()
                .filter(|id| q.entry(*id).is_some_and(&predicate))
                .collect()
        };
        self.s.queue_view.select_ids(&[]);
        if matching.is_empty() {
            self.say(&format!("No images matching {description}"));
            return;
        }
        self.s.queue_view.select_ids(&matching);
        self.say(&format!(
            "Selected {} image(s) matching {description}",
            matching.len()
        ));
    }

    /// `_reset_rows(entries)`: back to never-searched, after a question when anything would be
    /// lost, and the saved result is dropped too so the next search really searches.
    pub fn reset_rows(&self, ids: &[EntryId]) {
        let live = self.live_ids(ids);
        let with_results: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            live.into_iter()
                .filter(|id| q.entry(*id).is_some_and(has_result))
                .collect()
        };
        if with_results.is_empty() {
            self.say("Nothing to reset - none of those have been searched yet");
            return;
        }
        let count = with_results.len();
        let noun = if count == 1 { "image" } else { "images" };
        let w = self.clone();
        dialogs::question(
            &*self.s.prompter,
            "Reset results",
            &format!(
                "Discard the search results for {count} {noun}?\n\n\
                 The match, its tags and any error are cleared and they go back to \
                 Not searched. Tags you typed and tags from Hydrus are kept, and \
                 nothing on disk or in Hydrus is touched.\n\n\
                 Searching them again takes as long as it did the first time."
            ),
            Box::new(move |yes| {
                if yes {
                    w.do_reset_rows(&with_results);
                }
            }),
        );
    }

    fn do_reset_rows(&self, ids: &[EntryId]) {
        let cache = SearchCache::new(&self.s.paths);
        let mut dropped = 0;
        for id in ids {
            let hash = self.s.queue.borrow_mut().reset_result(*id);
            if let Some(Some(hash)) = hash
                && let Ok(key) = CacheKey::parse(&hash)
                && cache.drop_entry(&key).unwrap_or(false)
            {
                dropped += 1;
            }
        }
        self.s.queue.borrow_mut().take_events();
        let count = ids.len();
        tracing::info!("Reset {count} result(s), dropping {dropped} cached search result(s)");
        self.list_changed();
        let noun = if count == 1 { "image" } else { "images" };
        let mut status = format!("Reset {count} {noun} to Not searched");
        if dropped > 0 {
            status.push_str(&format!(" ({dropped} cached result(s) dropped)"));
        }
        self.say(&status);
    }

    /// `_set_rows_reviewed(entries, reviewed)`: no question, and nothing if nothing changes.
    pub fn set_rows_reviewed(&self, ids: &[EntryId], reviewed: bool) {
        let changed: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            self.live_ids(ids)
                .into_iter()
                .filter(|id| q.entry(*id).is_some_and(|e| e.reviewed != reviewed))
                .collect()
        };
        if changed.is_empty() {
            return;
        }
        for id in &changed {
            self.s.queue.borrow_mut().set_reviewed(*id, reviewed);
            self.s.queue_view.refresh_row(*id);
        }
        self.s.queue.borrow_mut().take_events();
        self.refresh_sent_count_label();
        let remaining = self
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .filter(|e| needs_review(e))
            .count();
        self.say(&format!(
            "{} {} image(s) as reviewed \u{2014} {remaining} still to review",
            if reviewed { "Marked" } else { "Unmarked" },
            changed.len()
        ));
    }

    /// `_remove_rows(entries)`: out of this list only, after a question whose default is
    /// Cancel (design gate 2.6; Python removes without one).
    pub fn remove_rows(&self, ids: &[EntryId]) {
        let live = self.live_ids(ids);
        if live.is_empty() {
            return;
        }
        let count = live.len();
        let w = self.clone();
        self.s.prompter.message(
            Kind::Question,
            "Remove from list",
            &format!(
                "Remove {count} {} from the list?\n\nThe files on disk and in Hydrus are not touched.",
                if count == 1 { "file" } else { "files" }
            ),
            Buttons::RemoveCancel,
            Box::new(move |r| {
                if r == Response::Remove {
                    w.do_remove_rows(&live);
                }
            }),
        );
    }

    fn do_remove_rows(&self, ids: &[EntryId]) {
        let live = self.live_ids(ids);
        if live.is_empty() {
            return;
        }
        self.s
            .queue
            .borrow_mut()
            .remove(&live, RemoveReason::Requested);
        self.s.queue.borrow_mut().take_events();
        self.list_changed();
        self.say(&format!("Removed {} file(s) from the list", live.len()));
    }

    /// `action_clear_search_cache()`.
    pub fn action_clear_search_cache(&self) {
        let w = self.clone();
        dialogs::question(
            &*self.s.prompter,
            "Clear Search Cache",
            "This deletes all saved search results. Images you've already searched will hit \
             IQDB/SauceNAO fresh again next time. Continue?",
            Box::new(move |yes| {
                if !yes {
                    return;
                }
                match SearchCache::new(&w.s.paths).clear() {
                    Ok(removed) => {
                        tracing::info!(
                            "Search cache cleared via menu action ({removed} entries removed)"
                        );
                        w.update_clear_cache_label();
                        dialogs::information(
                            &*w.s.prompter,
                            "Clear Search Cache",
                            &format!("Removed {removed} cached search result(s)."),
                        );
                    }
                    Err(e) => {
                        let err = CacheError::new(e);
                        tracing::error!("Could not clear the search cache: {}", err.detail());
                        dialogs::warning(&*w.s.prompter, "Clear Search Cache", &err.user_message());
                    }
                }
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_names_the_format_and_the_extension_only_decides_under_all_files() {
        assert_eq!(export_format("JSON (*.json)", "/h/x.csv"), "json");
        assert_eq!(export_format("CSV spreadsheet (*.csv)", "/h/x.json"), "csv");
        assert_eq!(export_format("All files (*)", "/h/x.JSON"), "json");
        assert_eq!(export_format("", "/h/x.txt"), "csv");
    }

    #[test]
    fn the_extension_is_added_unless_it_is_already_there() {
        assert_eq!(
            with_export_extension(PathBuf::from("/h/out"), "csv"),
            PathBuf::from("/h/out.csv")
        );
        assert_eq!(
            with_export_extension(PathBuf::from("/h/OUT.CSV"), "csv"),
            PathBuf::from("/h/OUT.CSV")
        );
        assert_eq!(
            with_export_extension(PathBuf::from("/h/out.csv"), "json"),
            PathBuf::from("/h/out.csv.json")
        );
    }
}
