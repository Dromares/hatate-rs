//! The two background checks the window starts on request (`gui/main_window.py` at `76c85c4`:
//! `_check_match_availability`, `_on_availability_progress`, `_on_availability_finished`,
//! `_remove_dead_candidates`, `_check_rows_for_upscaling`, `_on_upscale_entry_checked`,
//! `_on_upscale_check_finished`) over the thread shells of `workers/availability_worker.py` and
//! `workers/upscale_check_worker.py`. The checks themselves are the merged
//! `hatate_core::availability::check_entry_candidates` and `hatate_core::upscale_detect`; both run
//! on the worker bridge's runtime over **copies** of the rows, and the window applies what they
//! found. Menu entries that start them are 6.d's context menu and 6.f's Review.

use crate::dialogs::{self, Buttons, Kind, Response};
use crate::window::MainWindow;
use hatate_core::availability::{EntryCheck, SettingsAccess, check_entry_candidates};
use hatate_core::model::Entry;
use hatate_core::queue::EntryId;
use hatate_core::upscale_detect::{Real, UpscaleCheckResult, check_entries};

/// `_on_upscale_entry_checked`: the parts of one result, one per line, and the verdict they make.
pub fn upscale_detail(result: &UpscaleCheckResult) -> (String, &'static str) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(sig) = result
        .metadata_signature
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("\u{26A0} {sig}"));
    }
    if let Some(src) = &result.source_comparison {
        let marker = if src.flagged { "\u{26A0}" } else { "\u{2713}" };
        parts.push(format!("{marker} {}", src.message));
    }
    if let Some(own) = &result.self_consistency {
        if own.confidence != "none" {
            let marker = if own.confidence == "likely" {
                "\u{26A0}"
            } else {
                "?"
            };
            parts.push(format!("{marker} {}", own.message));
        } else {
            parts.push(format!("\u{2713} {}", own.message));
        }
    }
    let detail = if parts.is_empty() {
        "No checks could be run for this image.".to_owned()
    } else {
        parts.join("\n")
    };
    let verdict = if detail.contains('\u{26A0}') {
        "flagged"
    } else {
        "clear"
    };
    (detail, verdict)
}

/// `_host_from_url(url)`: the host a match is on, for the detail lines.
fn host_from_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or("").to_owned()
}

impl MainWindow {
    /// `_check_match_availability(entry)`.
    pub fn check_match_availability(&self, id: EntryId) {
        if self.s.availability_worker.is_running() {
            dialogs::information(
                &*self.s.prompter,
                "Check Match Availability",
                "A check is already running.",
            );
            return;
        }
        let Some(entry) = self.s.queue.borrow().entry(id).cloned() else {
            return;
        };
        if entry.candidates.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Check Match Availability",
                "This image has no matches to check.",
            );
            return;
        }
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            dialogs::critical(
                &*self.s.prompter,
                "Check Match Availability",
                "The check could not start. See Help > View Logs.",
            );
            return;
        };
        let total = entry.candidates.len();
        self.say(&format!(
            "Checking availability of {total} match(es)\u{2026}"
        ));
        self.set_progress_gauge(0, total);
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let (settings, paths) = (self.s.settings.clone(), self.s.paths.clone());
        let net = backend.engine.net.clone();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx: &crate::worker::Ctx<(usize, usize)>| {
                let mut copy = entry;
                let access = SettingsAccess {
                    settings: &settings,
                    paths: &paths,
                };
                let report = |d: usize, t: usize| ctx.report((d, t));
                let result = handle.block_on(check_entry_candidates(
                    &net,
                    &mut copy,
                    &access,
                    &report,
                    ctx.stop_flag(),
                ));
                (copy, result)
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    crate::worker::Event::Progress((done, total)) => {
                        w.on_availability_progress(done, total)
                    }
                    crate::worker::Event::Done((copy, result)) => {
                        w.on_availability_finished(id, copy, result)
                    }
                }
            },
        );
        self.s.availability_worker.replace(job);
    }

    /// `_on_availability_progress(done, total)`.
    fn on_availability_progress(&self, done: usize, total: usize) {
        let (_, max) = self.s.run_progress.get();
        self.set_progress_gauge(done, max.max(total as i64) as usize);
        self.say(&format!(
            "Checking match availability\u{2026} ({done}/{total})"
        ));
    }

    /// `_on_availability_finished(entry, gone_count, checked_count)`.
    pub fn on_availability_finished(&self, id: EntryId, checked_copy: Entry, result: EntryCheck) {
        if self.s.closing.get() {
            return; // see `_on_missing_files_checked` (DAN-128)
        }
        let (_, max) = self.s.run_progress.get();
        self.set_progress_gauge(0, max as usize);
        // The verdicts onto the row's own candidates, by address, in case the row changed.
        {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            for done in &checked_copy.candidates {
                if let Some(mine) = entry.candidates.iter_mut().find(|c| c.url == done.url) {
                    mine.remote_available = done.remote_available;
                    mine.availability_checked_at = done.availability_checked_at;
                }
            }
        }
        let entry = self.s.queue.borrow().entry(id).cloned().expect("just seen");
        let unknown = entry
            .candidates
            .iter()
            .filter(|c| c.remote_available.is_none())
            .count();
        let title = "Check Match Availability";
        if result.gone == 0 {
            let mut msg = format!(
                "All {} match(es) still appear to be available.",
                result.checked
            );
            if unknown > 0 {
                msg.push_str(&format!(
                    "\n\n({unknown} couldn't be checked conclusively - unreachable or blocked - \
                     and were left alone.)"
                ));
            }
            self.say("Availability check finished: nothing gone");
            dialogs::information(&*self.s.prompter, title, &msg);
            return;
        }
        let detail: Vec<String> = entry
            .candidates
            .iter()
            .filter(|c| c.remote_available == Some(false))
            .map(|c| {
                let name = c
                    .source_name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .or_else(|| Some(host_from_url(&c.url)).filter(|h| !h.is_empty()))
                    .unwrap_or_else(|| "?".to_owned());
                format!("{name} \u{2014} {}", c.url)
            })
            .collect();
        let mut text = format!(
            "{} of {} match(es) are no longer available.\n\nRemove them from this image's match \
             list?",
            result.gone, result.checked
        );
        if unknown > 0 {
            text.push_str(&format!(
                "\n\n({unknown} other(s) couldn't be checked conclusively and will be kept.)"
            ));
        }
        // The prompter has no detailed-text area: the list follows the question.
        text.push_str("\n\n");
        text.push_str(&detail.join("\n"));
        let w = self.downgrade();
        let gone = result.gone;
        self.s.prompter.message(
            Kind::Question,
            title,
            &text,
            Buttons::YesNo,
            Box::new(move |r| {
                let Some(w) = w.upgrade() else { return };
                if r == Response::Yes {
                    w.remove_dead_candidates(id);
                } else {
                    w.say(&format!("Availability check finished: {gone} gone (kept)"));
                }
            }),
        );
    }

    /// `_remove_dead_candidates(entry)`: drops the confirmed-gone matches and redraws.
    pub fn remove_dead_candidates(&self, id: EntryId) {
        let removed = {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            let removed = entry.drop_unavailable_candidates();
            if entry.candidates.is_empty() {
                tracing::info!(
                    "All matches for {} were gone; entry now has no match",
                    entry.filename()
                );
            }
            tracing::info!("Removed {removed} dead match(es) from {}", entry.filename());
            q.refresh_table();
            q.selection_changed();
            removed
        };
        self.say(&format!("Removed {removed} unavailable match(es)"));
        self.list_changed();
        let entry = self.s.queue.borrow().entry(id).cloned();
        self.populate_candidate_combo(entry.as_ref());
        self.update_preview(Some(id));
        self.refresh_tag_list(Some(id));
    }

    /// `_check_rows_for_upscaling(entries)`.
    pub fn check_rows_for_upscaling(&self, ids: &[EntryId]) {
        let title = "Check for Upscaling";
        if self.s.upscale_check_worker.is_running() {
            dialogs::information(&*self.s.prompter, title, "A check is already running.");
            return;
        }
        let rows: Vec<EntryId> = {
            let q = self.s.queue.borrow();
            ids.iter()
                .copied()
                .filter(|id| q.position_of(*id).is_some())
                .collect()
        };
        if rows.is_empty() {
            return;
        }
        let copies: Vec<Entry> = {
            let q = self.s.queue.borrow();
            rows.iter().filter_map(|id| q.entry(*id).cloned()).collect()
        };
        self.say(&format!(
            "Checking {} image(s) for upscaling\u{2026}",
            rows.len()
        ));
        let ids_for_events = rows.clone();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx: &crate::worker::Ctx<(usize, Entry, UpscaleCheckResult)>| {
                let mut copies = copies;
                let mut index = 0;
                let ran = check_entries(&mut copies, &Real, |entry, result| {
                    ctx.report((index, entry.clone(), result.clone()));
                    index += 1;
                });
                ran.is_ok()
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    crate::worker::Event::Progress((i, entry, result)) => {
                        if let Some(id) = ids_for_events.get(i) {
                            w.on_upscale_entry_checked(*id, entry, &result);
                        }
                    }
                    // A bomb ends the worker without `finished_all`, as in Python.
                    crate::worker::Event::Done(true) => {
                        w.on_upscale_check_finished(&ids_for_events)
                    }
                    crate::worker::Event::Done(false) => {}
                }
            },
        );
        self.s.upscale_check_worker.replace(job);
    }

    /// `_on_upscale_entry_checked(entry, result)`: the verdict and its detail live on the row.
    fn on_upscale_entry_checked(&self, id: EntryId, checked: Entry, result: &UpscaleCheckResult) {
        let (detail, verdict) = upscale_detail(result);
        let mut q = self.s.queue.borrow_mut();
        if let Some(entry) = q.entry_mut(id) {
            entry.local_width = checked.local_width;
            entry.local_height = checked.local_height;
            entry.upscale_verdict = Some(verdict.to_owned());
            entry.upscale_check_detail = Some(detail);
            q.refresh_row(id);
        }
    }

    /// `_on_upscale_check_finished(entries)`.
    fn on_upscale_check_finished(&self, ids: &[EntryId]) {
        if self.s.closing.get() {
            return;
        }
        let (flagged, lines) = {
            let q = self.s.queue.borrow();
            let mut flagged = 0;
            let mut lines = Vec::new();
            for id in ids {
                let Some(e) = q.entry(*id) else { continue };
                if e.upscale_verdict.as_deref() == Some("flagged") {
                    flagged += 1;
                }
                let summary = e
                    .upscale_check_detail
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "No result.".to_owned());
                lines.push(format!("{}:\n{summary}", e.filename()));
            }
            (flagged, lines)
        };
        self.say(&format!(
            "Upscale check finished: {flagged}/{} flagged",
            ids.len()
        ));
        let mut text = format!(
            "{flagged} of {} image(s) show possible signs of upscaling.\n\nThese are heuristics, \
             not proof - see details for each image's specific result.",
            ids.len()
        );
        text.push_str("\n\n");
        text.push_str(&lines.join("\n\n"));
        self.s.prompter.message(
            if flagged > 0 {
                Kind::Warning
            } else {
                Kind::Information
            },
            "Upscale Check Results",
            &text,
            Buttons::Ok,
            Box::new(|_| {}),
        );
    }

    /// `closeEvent`: neither check is heard again.
    pub(crate) fn retire_checks(&self) {
        self.s.availability_worker.retire();
        self.s.upscale_check_worker.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::upscale_detect::{SelfConsistencyResult, SourceComparisonResult};

    fn result(
        sig: Option<&str>,
        src: Option<bool>,
        own: Option<&'static str>,
    ) -> UpscaleCheckResult {
        UpscaleCheckResult {
            metadata_signature: sig.map(str::to_owned),
            source_comparison: src.map(|flagged| SourceComparisonResult {
                flagged,
                local_size: (2, 2),
                source_size: (1, 1),
                message: "src".to_owned(),
            }),
            self_consistency: own.map(|confidence| SelfConsistencyResult {
                confidence,
                message: "own".to_owned(),
                ..SelfConsistencyResult::default()
            }),
        }
    }

    #[test]
    fn the_detail_lines_and_the_verdict_are_pythons() {
        let (d, v) = upscale_detail(&result(Some("Real-ESRGAN"), Some(true), Some("likely")));
        assert_eq!(d, "\u{26A0} Real-ESRGAN\n\u{26A0} src\n\u{26A0} own");
        assert_eq!(v, "flagged");
        let (d, v) = upscale_detail(&result(None, Some(false), Some("none")));
        assert_eq!(d, "\u{2713} src\n\u{2713} own");
        assert_eq!(v, "clear");
        // `possible` is a question mark, not a warning: it does not flag the row.
        let (d, v) = upscale_detail(&result(None, None, Some("possible")));
        assert_eq!(d, "? own");
        assert_eq!(v, "clear");
        let (d, v) = upscale_detail(&result(None, None, None));
        assert_eq!(d, "No checks could be run for this image.");
        assert_eq!(v, "clear");
    }

    #[test]
    fn the_host_of_a_match_is_what_sits_between_the_scheme_and_the_path() {
        assert_eq!(
            host_from_url("https://danbooru.donmai.us/posts/1?x=1"),
            "danbooru.donmai.us"
        );
        assert_eq!(host_from_url(""), "");
    }
}
