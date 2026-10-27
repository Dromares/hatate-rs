//! The Queue's file actions (`gui/main_window.py` at `76c85c4`): Export Results
//! (`action_export_results`, 1657), Write Tag Files (`action_write_tag_files` 1712 with
//! `_ask_about_writing_tag_files` 1757) and Clear Search Cache (`action_clear_search_cache`,
//! 2247). The texts and the decisions are `hatate_core::sidecar` and `hatate_core::export`; this
//! file puts the questions and the results on screen. Each takes the ids of the rows it acts on
//! (`None` is the whole list), the way the Files menu and the right-click menu call them.

use crate::dialogs::{self, Buttons, Kind, Response};
use crate::error::{CacheError, ExportError};
use crate::window::MainWindow;
use hatate_core::cache::SearchCache;
use hatate_core::export;
use hatate_core::model::Entry;
use hatate_core::queue::EntryId;
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
                    export::render(rows.iter(), fmt, Some(&settings))
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
