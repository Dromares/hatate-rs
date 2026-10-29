//! Hydrus from the window (`gui/main_window.py` at `76c85c4`): `_lookup_existing_hydrus_tags`,
//! `_on_hydrus_tags_looked_up`, `action_reconcile_with_hydrus`, `_start_hydrus_reconcile`,
//! `_on_hydrus_reconciled`, `action_send_to_hydrus`, `action_import_url_to_hydrus`,
//! `_on_hydrus_import_resolved`, `_on_hydrus_import_poll_finished`,
//! `action_download_and_send_to_hydrus`, `_show_rows_in_hydrus`,
//! `_explain_show_in_hydrus_failure` and `_delete_rows_from_hydrus`, over the thread shells of
//! `workers/hydrus_lookup_worker.py`, `hydrus_reconcile_worker.py` and
//! `hydrus_import_poll_worker.py`.
//!
//! The Hydrus work itself is the merged `hatate_core` code (`hydrus_import`, `hydrus_import_poll`,
//! `hydrus_reconcile`, `hydrus_tag_lookup`, `hydrus_client`); this file runs it **off the GTK
//! main thread** over **copies** of the rows and applies what it found when it is done. Python's
//! send loops ran on the GUI thread and froze the window for the length of a batch; here the
//! window keeps handling actions, so a second send while one is under way is refused with a
//! message (Python could not be asked twice). Every Hydrus host is `settings.hydrus.api_url`, as
//! in Python: there is no other way to point this at a server.
//!
//! The text of a failure the window shows is `HydrusError::user_message` (what failed and what to
//! do, no URL, key or OS text, DAN-2116 item 1); the raw text Python shows (`str(exc)`) goes in
//! the box's detail lines and into `Entry.error_message`, which stays byte-compatible.

use crate::dialogs::{self, Buttons, Kind, Response};
use crate::window::MainWindow;
use crate::worker::{Ctx, Event};
use hatate_core::clock::SystemClock;
use hatate_core::config::Settings;
use hatate_core::hydrus_client::{DeletionState, HydrusClient, HydrusError};
use hatate_core::hydrus_import::{
    ImportResult, download_and_send, send_file_upload, send_url_or_download,
};
use hatate_core::hydrus_import_poll::{PollEvents, PollItem, PollWorkerOptions, run_import_poll};
use hatate_core::hydrus_reconcile::{
    ReconcileResult, entries_awaiting_confirmation, run_reconcile,
};
use hatate_core::hydrus_tag_lookup::{TagLookupResult, run_tag_lookup};
use hatate_core::hydrus_url::normalize_url_for_hydrus;
use hatate_core::model::Entry;
use hatate_core::queue::{EntryId, RemoveReason};
use std::sync::Arc;

/// `self._pending_poll_confirmed`, `_pending_poll_unconfirmed` and `_pending_poll_warnings`.
#[derive(Default, Debug)]
pub struct PollPending {
    pub confirmed: Vec<EntryId>,
    pub unconfirmed: Vec<EntryId>,
    pub warnings: Vec<String>,
}

const SEND: &str = "Send to Hydrus";
const IMPORT: &str = "Send URL to Hydrus's Importer";
const DOWNLOAD: &str = "Download + Send to Hydrus";
const SHOW: &str = "Show in Hydrus";
const DELETE: &str = "Delete from Hydrus";
const NO_KEY: &str = "Set your Hydrus access key in Settings first.";

/// The first ten problems, one per line, and a pointer to the logs for the rest.
pub fn problem_lines(problems: &[String]) -> String {
    let mut detail = problems
        .iter()
        .take(10)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if problems.len() > 10 {
        detail.push_str(&format!(
            "\n\u{2026} and {} more (see Help > View Logs)",
            problems.len() - 10
        ));
    }
    detail
}

/// What a failed Hydrus step looks like as an [`ImportResult`] (Python let such an exception
/// out of the loop; the window reports it as that file's failure and goes on).
fn failure(message: String) -> ImportResult {
    ImportResult {
        success: false,
        error: Some(message),
        ..ImportResult::default()
    }
}

/// The fields the Hydrus steps change, copied from the worker's copy onto the row.
fn apply_hydrus_fields(row: &mut Entry, done: &Entry) {
    row.hydrus_hash.clone_from(&done.hydrus_hash);
    row.sent_to_hydrus = done.sent_to_hydrus;
    row.hydrus_import_confirmed = done.hydrus_import_confirmed;
    row.error_message.clone_from(&done.error_message);
    row.stale_matched_url.clone_from(&done.stale_matched_url);
    // `_repoint` rewrote the selected match's address; the row may have been re-searched since.
    if row.candidates.len() == done.candidates.len() {
        for (mine, theirs) in row.candidates.iter_mut().zip(&done.candidates) {
            mine.url.clone_from(&theirs.url);
            mine.tag_version.clone_from(&theirs.tag_version);
        }
    }
}

/// What a send action starts with: the selected rows (copies), a snapshot of the settings and
/// the client built from them.
struct Batch {
    rows: Vec<(EntryId, Entry)>,
    settings: Settings,
    client: HydrusClient,
}

/// One row's outcome from a send loop.
struct Sent {
    id: EntryId,
    entry: Entry,
    result: ImportResult,
}

/// What the show worker found.
enum Showed {
    /// `filter_known_hashes` failed.
    Unreachable(HydrusError),
    /// None of the hashes is one Hydrus holds.
    NoneKnown,
    /// `show_files_in_client` failed.
    Refused(HydrusError),
    Shown {
        count: usize,
        skipped: usize,
    },
}

/// What the delete worker's first question found.
enum Asked {
    Unreachable(HydrusError),
    States(hatate_core::hydrus_client::DeletionStates),
}

/// What the delete worker's second step found.
enum Deleted {
    Refused(HydrusError),
    Asked {
        asked: Vec<String>,
        gone: std::collections::BTreeSet<String>,
    },
}

/// The messages `_explain_show_in_hydrus_failure` gives for the two failures peculiar to the
/// action, and the generic one.
pub fn show_failure_text(exc: &HydrusError) -> String {
    let text = exc.message();
    if text.contains("403") {
        "Hydrus refused this because the access key lacks the \"manage pages\" permission.\n\n\
         This is the only thing here that needs it, so a key set up for importing won't have it \
         yet. Add it in Hydrus under services > review services > client api, then try again."
            .to_owned()
    } else if text.contains("404") {
        "This Hydrus client is too old for this: opening a page through the API was added after \
         your version.\n\nEverything else here works as before - only this one action needs the \
         newer endpoint."
            .to_owned()
    } else {
        format!("Hydrus could not open the page: {}", exc.user_message())
    }
}

impl MainWindow {
    fn hydrus_settings(&self) -> Settings {
        self.s.settings.lock().unwrap().clone()
    }

    /// `self.settings.hydrus.access_key` is set (`HydrusSettings::is_configured`).
    pub fn hydrus_configured(&self) -> bool {
        self.s.settings.lock().unwrap().hydrus.is_configured()
    }

    /// The common opening of the send actions: a selection, a key, a client. `None` after
    /// saying why not.
    fn hydrus_batch_start(&self, title: &str, ids: &[EntryId]) -> Option<Batch> {
        if ids.is_empty() {
            dialogs::information(&*self.s.prompter, title, "Select at least one row.");
            return None;
        }
        if !self.hydrus_configured() {
            dialogs::warning(&*self.s.prompter, title, NO_KEY);
            return None;
        }
        if self.s.hydrus_batch_worker.is_running() {
            dialogs::information(
                &*self.s.prompter,
                title,
                "A Hydrus request is already under way. Wait for it to finish, then try again.",
            );
            return None;
        }
        let settings = self.hydrus_settings();
        let client = match HydrusClient::new(&settings.hydrus) {
            Ok(client) => client,
            Err(e) => {
                tracing::error!("Could not build the Hydrus client: {}", e.detail());
                dialogs::warning(&*self.s.prompter, title, &e.user_message());
                return None;
            }
        };
        let rows: Vec<(EntryId, Entry)> = {
            let q = self.s.queue.borrow();
            self.live_ids(ids)
                .into_iter()
                .filter_map(|id| q.entry(id).map(|e| (id, e.clone())))
                .collect()
        };
        Some(Batch {
            rows,
            settings,
            client,
        })
    }

    /// Runs `work` on the worker bridge and hands its answer to `done` on the main thread.
    fn run_hydrus_batch<T: Send + 'static>(
        &self,
        work: impl FnOnce(tokio::runtime::Handle) -> T + Send + 'static,
        done: impl FnOnce(&MainWindow, T) + 'static,
    ) {
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let w = self.downgrade();
        let mut done = Some(done);
        let job = self.s.workers.run(
            move |_ctx: &Ctx<()>| work(handle),
            move |event| {
                let Some(w) = w.upgrade() else { return };
                if let Event::Done(out) = event {
                    if let Some(done) = done.take() {
                        done(&w, out);
                    }
                }
            },
        );
        self.s.hydrus_batch_worker.replace(job);
    }

    /// `_remove_entries(entries, reason, keep_place)`: out of the list by identity, and the
    /// selection lands on the row that followed them.
    pub fn remove_entries(&self, ids: &[EntryId], reason: &'static str) {
        let live = self.live_ids(ids);
        if live.is_empty() {
            return;
        }
        let successor = self.s.queue_view.successor_surviving(&live);
        self.s
            .queue
            .borrow_mut()
            .remove(&live, RemoveReason::After(reason));
        self.s.queue.borrow_mut().take_events();
        self.list_changed();
        if let Some(next) = successor {
            if self.s.queue_view.selected_ids().is_empty() {
                if let Some(row) = self.s.queue_view.row_of(next) {
                    self.s.queue_view.select_row(row);
                }
            }
        }
    }

    /// Puts the worker's copies back on their rows (the rows that are still there) and repaints.
    fn apply_sent(&self, sent: &[Sent]) {
        {
            let mut q = self.s.queue.borrow_mut();
            for s in sent {
                if let Some(row) = q.entry_mut(s.id) {
                    apply_hydrus_fields(row, &s.entry);
                }
            }
        }
        self.repaint_rows(&sent.iter().map(|s| s.id).collect::<Vec<_>>());
    }

    /// `_refresh_table(keep_place=...)` for these rows.
    fn repaint_rows(&self, ids: &[EntryId]) {
        self.s.queue.borrow_mut().take_events();
        for id in ids {
            self.s.queue_view.refresh_row(*id);
        }
        self.list_changed();
    }

    // ------------------------------------------------------------------
    // Lookup and reconcile
    // ------------------------------------------------------------------

    /// `_lookup_existing_hydrus_tags(entries)`: for each new entry, if Hydrus already has this
    /// exact file, pull in whatever tags it already has.
    pub fn lookup_existing_hydrus_tags(&self, ids: &[EntryId]) {
        if ids.is_empty() || !self.hydrus_configured() {
            return;
        }
        self.s.hydrus_lookup_worker.retire();
        let rows: Vec<(EntryId, Entry)> = {
            let q = self.s.queue.borrow();
            ids.iter()
                .filter_map(|id| q.entry(*id).map(|e| (*id, e.clone())))
                .collect()
        };
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let settings = self.hydrus_settings();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |_ctx: &Ctx<()>| {
                let (ids, mut copies): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
                let result = handle.block_on(run_tag_lookup(&mut copies, &settings));
                (ids, copies, result)
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                if let Event::Done((ids, copies, result)) = event {
                    w.on_hydrus_tags_looked_up(&ids, copies, &result);
                }
            },
        );
        self.s.hydrus_lookup_worker.replace(job);
    }

    /// `_on_hydrus_tags_looked_up(result)`.
    fn on_hydrus_tags_looked_up(
        &self,
        ids: &[EntryId],
        copies: Vec<Entry>,
        result: &TagLookupResult,
    ) {
        if self.s.closing.get() {
            return;
        }
        {
            let mut q = self.s.queue.borrow_mut();
            for (id, done) in ids.iter().zip(copies) {
                if let Some(row) = q.entry_mut(*id) {
                    row.hydrus_hash.clone_from(&done.hydrus_hash);
                    row.tags = done.tags;
                }
            }
        }
        self.repaint_rows(ids);
        self.refresh_tag_list(self.current_id());
        if let Some(error) = &result.error {
            self.say(&format!("Could not look up existing Hydrus tags: {error}"));
        } else if result.tagged_count > 0 {
            self.say(&format!(
                "Auto-imported existing Hydrus tags for {} file(s)",
                result.tagged_count
            ));
        }
    }

    /// `action_reconcile_with_hydrus()`: Files > Re-check Queued Imports, on request.
    pub fn action_reconcile_with_hydrus(&self) {
        let waiting = entries_awaiting_confirmation(self.s.queue.borrow().entries());
        if waiting.is_empty() {
            self.say("Nothing is waiting on Hydrus - every sent file is confirmed");
            return;
        }
        self.start_hydrus_reconcile(false);
    }

    /// `_start_hydrus_reconcile(quiet)`: asks Hydrus to settle entries still showing Queued.
    /// `quiet` is for the automatic pass after a session restore: it says something only when it
    /// changed the list.
    pub fn start_hydrus_reconcile(&self, quiet: bool) {
        let (ids, rows): (Vec<EntryId>, Vec<Entry>) = {
            let q = self.s.queue.borrow();
            entries_awaiting_confirmation(q.entries())
                .into_iter()
                .filter_map(|i| {
                    let id = q.id_at(i as i64)?;
                    Some((id, q.entries()[i].clone()))
                })
                .unzip()
        };
        if rows.is_empty() {
            return;
        }
        self.s.hydrus_reconcile_worker.retire();
        self.s.reconcile_was_quiet.set(quiet);
        if !quiet {
            self.say(&format!(
                "Asking Hydrus about {} queued import(s)\u{2026}",
                rows.len()
            ));
        }
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let settings = self.hydrus_settings();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx: &Ctx<()>| {
                let mut copies = rows;
                let stop = ctx.stop_flag().clone();
                let should_stop = move || stop.load(std::sync::atomic::Ordering::Relaxed);
                let result = handle.block_on(run_reconcile(
                    &mut copies,
                    &settings,
                    Some(&should_stop),
                    None,
                ));
                (copies, result)
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                if let Event::Done((copies, result)) = event {
                    w.on_hydrus_reconciled(&ids, &copies, &result);
                }
            },
        );
        self.s.hydrus_reconcile_worker.replace(job);
    }

    /// `_on_hydrus_reconciled(result)`.
    fn on_hydrus_reconciled(&self, ids: &[EntryId], copies: &[Entry], result: &ReconcileResult) {
        if self.s.closing.get() {
            return;
        }
        let confirmed: Vec<EntryId> = result.confirmed.iter().map(|i| ids[*i]).collect();
        {
            let mut q = self.s.queue.borrow_mut();
            for i in &result.confirmed {
                if let Some(row) = q.entry_mut(ids[*i]) {
                    row.hydrus_import_confirmed = copies[*i].hydrus_import_confirmed;
                }
            }
        }
        if !confirmed.is_empty() {
            // The Sent column and its running count both describe this.
            self.repaint_rows(&confirmed);
        }
        if !confirmed.is_empty() || !self.s.reconcile_was_quiet.get() {
            self.say(&result.summary());
        }
        tracing::info!("Hydrus reconcile: {}", result.summary());
    }

    // ------------------------------------------------------------------
    // Send, URL import, download and send
    // ------------------------------------------------------------------

    /// `action_send_to_hydrus()` on the selected rows.
    pub fn action_send_to_hydrus(&self, ids: &[EntryId]) {
        let Some(Batch {
            rows,
            settings,
            client,
        }) = self.hydrus_batch_start(SEND, ids)
        else {
            return;
        };
        self.run_hydrus_batch(
            move |handle| {
                let clock = SystemClock::default();
                let mut out = Vec::with_capacity(rows.len());
                for (id, mut entry) in rows {
                    let result = match handle.block_on(send_file_upload(
                        &mut entry,
                        &client,
                        Some(&settings),
                        &clock,
                    )) {
                        Ok(r) => r,
                        Err(e) => failure(e.message().to_owned()),
                    };
                    out.push(Sent { id, entry, result });
                }
                out
            },
            |w, sent| w.on_files_sent(sent),
        );
    }

    fn on_files_sent(&self, sent: Vec<Sent>) {
        if self.s.closing.get() {
            return;
        }
        let (mut ok, mut warned, mut failed) = (0usize, 0usize, 0usize);
        let (mut failures, mut warnings) = (Vec::<String>::new(), Vec::<String>::new());
        let mut succeeded: Vec<EntryId> = Vec::new();
        for s in &sent {
            if !s.result.success {
                failed += 1;
                failures.push(format!(
                    "{}: {}",
                    s.entry.filename(),
                    s.result.error.as_deref().unwrap_or("None")
                ));
                continue;
            }
            succeeded.push(s.id);
            match &s.result.warning {
                Some(w) => {
                    warned += 1;
                    warnings.push(format!("{}: {w}", s.entry.filename()));
                }
                None => ok += 1,
            }
        }
        self.apply_sent(&sent);
        let remove = self.s.settings.lock().unwrap().remove_after_import;
        if remove {
            self.remove_entries(&succeeded, "sent-to-Hydrus");
        }
        let mut status = format!("Sent {ok} file(s) to Hydrus");
        if warned > 0 {
            status.push_str(&format!(", {warned} with warnings"));
        }
        if failed > 0 {
            status.push_str(&format!(", {failed} failed"));
        }
        if remove && !succeeded.is_empty() {
            status.push_str(&format!(" ({} removed from list)", succeeded.len()));
        }
        self.say(&status);

        let problems: Vec<String> = failures.into_iter().chain(warnings).collect();
        if !problems.is_empty() {
            let mut parts = Vec::new();
            if failed > 0 {
                parts.push(format!("{failed} file(s) failed to import"));
            }
            if warned > 0 {
                parts.push(format!("{warned} file(s) sent with warnings"));
            }
            self.s.prompter.message_detailed(
                Kind::Warning,
                SEND,
                &format!("{}.", parts.join(", ")),
                &problem_lines(&problems),
                Buttons::Ok,
                Box::new(|_| {}),
            );
        }
    }

    /// `action_import_url_to_hydrus()`: sends the matched URL to Hydrus's own downloader instead
    /// of uploading the local copy; where Hydrus has no downloader but Hatate can reach the
    /// original, Hatate downloads and sends it.
    pub fn action_import_url_to_hydrus(&self, ids: &[EntryId]) {
        let Some(Batch {
            rows,
            settings,
            client,
        }) = self.hydrus_batch_start(IMPORT, ids)
        else {
            return;
        };
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            dialogs::critical(
                &*self.s.prompter,
                IMPORT,
                "The send could not start. See Help > View Logs.",
            );
            return;
        };
        let engine = backend.engine.clone();
        let shared = self.s.settings.clone();
        let _ = settings;
        self.run_hydrus_batch(
            move |handle| {
                let tmp = tempfile::Builder::new()
                    .prefix("hatate-linux-dl-")
                    .tempdir();
                let mut out = Vec::with_capacity(rows.len());
                for (id, mut entry) in rows {
                    let result = match &tmp {
                        Ok(dir) => match handle.block_on(send_url_or_download(
                            &mut entry,
                            &client,
                            &engine,
                            Some(&*shared),
                            dir.path(),
                        )) {
                            Ok(r) => r,
                            Err(e) => failure(e.message()),
                        },
                        Err(e) => failure(e.to_string()),
                    };
                    out.push(Sent { id, entry, result });
                }
                out
            },
            |w, sent| w.on_urls_sent(sent),
        );
    }

    fn on_urls_sent(&self, sent: Vec<Sent>) {
        if self.s.closing.get() {
            return;
        }
        let (mut queued, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        let mut problems: Vec<String> = Vec::new();
        let (mut queued_ids, mut downloaded_ids) = (Vec::<EntryId>::new(), Vec::<EntryId>::new());
        for s in &sent {
            if s.result.skipped_reason.is_some() {
                skipped += 1;
                continue;
            }
            if !s.result.success {
                failed += 1;
                problems.push(format!(
                    "{}: {}",
                    s.entry.filename(),
                    s.result.error.as_deref().unwrap_or("None")
                ));
                continue;
            }
            if s.result.confirmed == Some(true) {
                downloaded_ids.push(s.id);
                continue;
            }
            queued += 1;
            queued_ids.push(s.id);
        }
        self.apply_sent(&sent);
        let downloaded = downloaded_ids.len();
        let (remove, timeout, interval) = {
            let s = self.s.settings.lock().unwrap();
            (
                s.remove_after_import,
                s.url_import_confirm_timeout,
                s.url_import_confirm_interval,
            )
        };
        if downloaded > 0 && remove {
            self.remove_entries(&downloaded_ids, "downloaded-and-sent-to-Hydrus");
        }
        if !problems.is_empty() {
            self.s.prompter.message_detailed(
                Kind::Warning,
                IMPORT,
                &format!("{failed} URL(s) were not sent to Hydrus."),
                &problem_lines(&problems),
                Buttons::Ok,
                Box::new(|_| {}),
            );
        }
        if !remove || queued_ids.is_empty() {
            let mut status = format!("Queued {queued} URL(s) with Hydrus's importer");
            if downloaded > 0 {
                status.push_str(&format!(
                    ", {downloaded} downloaded by Hatate (Hydrus has no downloader for the site)"
                ));
            }
            if skipped > 0 {
                status.push_str(&format!(", {skipped} skipped (no matched URL)"));
            }
            if failed > 0 {
                status.push_str(&format!(", {failed} not sent"));
            }
            self.say(&status);
            if problems.is_empty() && (queued > 0 || downloaded > 0) {
                let mut lines = Vec::new();
                if queued > 0 {
                    lines.push(format!(
                        "Queued {queued} URL(s). Check Hydrus's own downloader page for progress \
                         - it fetches and imports the file itself, which can take a moment."
                    ));
                }
                if downloaded > 0 {
                    lines.push(format!(
                        "Hydrus has no downloader for {downloaded} of them, so Hatate downloaded \
                         the original itself and sent it with its tags and link."
                    ));
                }
                dialogs::information(&*self.s.prompter, IMPORT, &lines.join("\n\n"));
            }
            return;
        }

        // `remove_after_import` is on: ask Hydrus whether the files arrived before taking
        // anything out of the list ("queued" is not "done").
        self.say(&format!(
            "Queued {queued} URL(s){} - confirming with Hydrus before removing from the list\u{2026}",
            if downloaded > 0 {
                format!(", {downloaded} downloaded by Hatate")
            } else {
                String::new()
            }
        ));
        let items: Vec<(EntryId, Entry, String)> = {
            let q = self.s.queue.borrow();
            queued_ids
                .iter()
                .filter_map(|id| {
                    let e = q.entry(*id)?;
                    let url = e.matched_url().unwrap_or("");
                    let url = normalize_url_for_hydrus(url).unwrap_or_else(|_| url.to_owned());
                    Some((*id, e.clone(), url))
                })
                .collect()
        };
        self.start_import_poll(items, timeout, interval);
    }

    /// `HydrusImportPollWorker` started by `action_import_url_to_hydrus`.
    fn start_import_poll(&self, items: Vec<(EntryId, Entry, String)>, timeout: f64, interval: f64) {
        *self.s.poll_pending.borrow_mut() = PollPending::default();
        self.s.hydrus_import_poll_worker.retire();
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let settings = self.hydrus_settings();
        let client = match HydrusClient::new(&settings.hydrus) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Could not build the Hydrus client: {}", e.detail());
                return;
            }
        };
        let ids: Vec<EntryId> = items.iter().map(|(id, _, _)| *id).collect();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx: &Ctx<PollMsg>| {
                let poll_items: Vec<PollItem<'_>> = items
                    .iter()
                    .map(|(_, e, url)| PollItem {
                        entry: e,
                        url: url.clone(),
                    })
                    .collect();
                let options = PollWorkerOptions {
                    timeout,
                    interval,
                    settings: Some(&settings),
                };
                let stop = ctx.stop_flag().clone();
                let stopped = move || stop.load(std::sync::atomic::Ordering::Relaxed);
                let mut events = Bridge { ctx };
                let clock = SystemClock::default();
                let outcome = handle.block_on(run_import_poll(
                    &client,
                    &poll_items,
                    &options,
                    &stopped,
                    &mut events,
                    &clock,
                ));
                if let Err(e) = outcome {
                    tracing::error!("Hydrus import poll failed: {}", e.detail());
                }
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    Event::Progress(PollMsg::Resolved {
                        index,
                        hash,
                        warning,
                    }) => w.on_hydrus_import_resolved(ids[index], hash, warning),
                    Event::Progress(PollMsg::Countdown {
                        label,
                        remaining,
                        total,
                    }) => w.on_wait_countdown(&label, remaining, total),
                    Event::Done(()) => w.on_hydrus_import_poll_finished(),
                }
            },
        );
        self.s.hydrus_import_poll_worker.replace(job);
    }

    /// `_on_hydrus_import_resolved(entry, confirmed_hash, warning)`.
    fn on_hydrus_import_resolved(
        &self,
        id: EntryId,
        hash: Option<String>,
        warning: Option<String>,
    ) {
        let name = {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            let name = entry.filename().to_owned();
            if let Some(hash) = hash {
                entry.hydrus_hash = Some(hash);
                entry.hydrus_import_confirmed = true;
                if let Some(w) = &warning {
                    // The import stands; this says only that something after it did not (a
                    // duplicate relationship that could not be recorded). Collected for the
                    // summary rather than raised per entry.
                    entry.error_message = Some(w.clone());
                }
                let mut pending = self.s.poll_pending.borrow_mut();
                if let Some(w) = warning {
                    pending.warnings.push(format!("{name}: {w}"));
                }
                pending.confirmed.push(id);
            } else {
                entry.error_message =
                    Some("Hydrus hadn't confirmed the import within the poll window".to_owned());
                self.s.poll_pending.borrow_mut().unconfirmed.push(id);
            }
            name
        };
        let _ = name;
        self.refresh_entry_row(id);
    }

    /// `_on_hydrus_import_poll_finished()`.
    fn on_hydrus_import_poll_finished(&self) {
        if self.s.closing.get() {
            // The session being saved carries these as "still queued", and the automatic
            // reconcile on the next launch settles that (DAN-128).
            return;
        }
        let pending = std::mem::take(&mut *self.s.poll_pending.borrow_mut());
        let (confirmed, unconfirmed, warnings) =
            (pending.confirmed, pending.unconfirmed, pending.warnings);
        if !confirmed.is_empty() {
            self.remove_entries(&confirmed, "confirmed-imported-by-Hydrus");
        }
        let mut status = format!(
            "Confirmed {} Hydrus import(s), removed from list",
            confirmed.len()
        );
        if !unconfirmed.is_empty() {
            status.push_str(&format!(
                "; {} still processing - kept in list, check Hydrus's downloader page",
                unconfirmed.len()
            ));
        }
        if !warnings.is_empty() {
            status.push_str(&format!("; {} not marked as duplicates", warnings.len()));
        }
        self.say(&status);
        if !warnings.is_empty() {
            // The rows imported fine and are gone, so this is the last chance to say the
            // relationship that was asked for was not recorded: once, for the batch.
            self.s.prompter.message_detailed(
                Kind::Warning,
                IMPORT,
                &format!(
                    "{} file(s) imported, but were not marked as duplicates of your copies.",
                    warnings.len()
                ),
                &problem_lines(&warnings),
                Buttons::Ok,
                Box::new(|_| {}),
            );
        }
    }

    /// `action_download_and_send_to_hydrus()`: downloads each matched image's full-resolution
    /// file itself and uploads those bytes with the source URL and the tags.
    pub fn action_download_and_send_to_hydrus(&self, ids: &[EntryId]) {
        let Some(Batch {
            rows,
            settings,
            client,
        }) = self.hydrus_batch_start(DOWNLOAD, ids)
        else {
            return;
        };
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            dialogs::critical(
                &*self.s.prompter,
                DOWNLOAD,
                "The download could not start. See Help > View Logs.",
            );
            return;
        };
        let net = backend.engine.net.clone();
        self.run_hydrus_batch(
            move |handle| {
                let clock = SystemClock::default();
                // Each file is downloaded at full resolution before it goes to Hydrus, and /tmp is
                // often RAM-backed: the directory goes when the loop is done, however it ends.
                let tmp = tempfile::Builder::new()
                    .prefix("hatate-linux-dl-")
                    .tempdir();
                let mut out = Vec::with_capacity(rows.len());
                for (id, mut entry) in rows {
                    let result = match &tmp {
                        Ok(dir) => match handle.block_on(download_and_send(
                            &mut entry,
                            &client,
                            &net,
                            settings.search_timeout,
                            dir.path(),
                            Some(&settings),
                            &clock,
                        )) {
                            Ok(r) => r,
                            Err(e) => failure(e.message()),
                        },
                        Err(e) => failure(e.to_string()),
                    };
                    out.push(Sent { id, entry, result });
                }
                out
            },
            |w, sent| w.on_downloaded_and_sent(sent),
        );
    }

    fn on_downloaded_and_sent(&self, sent: Vec<Sent>) {
        if self.s.closing.get() {
            return;
        }
        let (mut ok, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        let mut problems: Vec<String> = Vec::new();
        let mut succeeded: Vec<EntryId> = Vec::new();
        for s in &sent {
            if s.result.skipped_reason.is_some() {
                skipped += 1;
                tracing::debug!(
                    "No direct file URL available for {}, skipping download-and-send (needs \
                     'Retrieve tags from booru page' enabled and a supported site)",
                    s.entry.filename()
                );
                continue;
            }
            if !s.result.success {
                failed += 1;
                problems.push(format!(
                    "{}: {}",
                    s.entry.filename(),
                    s.result.error.as_deref().unwrap_or("None")
                ));
                continue;
            }
            ok += 1;
            succeeded.push(s.id);
        }
        self.apply_sent(&sent);
        let remove = self.s.settings.lock().unwrap().remove_after_import;
        if remove {
            self.remove_entries(&succeeded, "downloaded-and-sent-to-Hydrus");
        }
        let mut status = format!("Downloaded + sent {ok} file(s) to Hydrus");
        if skipped > 0 {
            status.push_str(&format!(
                ", {skipped} skipped (no direct image URL available)"
            ));
        }
        if remove && !succeeded.is_empty() {
            status.push_str(&format!(" ({} removed from list)", succeeded.len()));
        }
        if failed > 0 {
            status.push_str(&format!(", {failed} failed"));
        }
        self.say(&status);
        if !problems.is_empty() {
            self.s.prompter.message_detailed(
                Kind::Warning,
                DOWNLOAD,
                &format!("{failed} file(s) failed."),
                &problem_lines(&problems),
                Buttons::Ok,
                Box::new(|_| {}),
            );
        } else if skipped > 0 && ok == 0 {
            dialogs::information(
                &*self.s.prompter,
                DOWNLOAD,
                "None of the selected images have a known direct image URL to download. This needs \
                 'Retrieve tags from booru page' enabled in Settings and a match from a supported \
                 site.",
            );
        }
    }

    // ------------------------------------------------------------------
    // Show and delete in Hydrus
    // ------------------------------------------------------------------

    /// `_show_rows_in_hydrus(entries)`: opens a Hydrus page showing the selected local files.
    /// Only files Hydrus holds are shown; the rest are filtered out first.
    pub fn show_rows_in_hydrus(&self, ids: &[EntryId]) {
        let live = self.live_ids(ids);
        if live.is_empty() {
            return;
        }
        if !self.hydrus_configured() {
            dialogs::warning(&*self.s.prompter, SHOW, NO_KEY);
            return;
        }
        let (hashes, first_name, total) = {
            let q = self.s.queue.borrow();
            let entries: Vec<&Entry> = live.iter().filter_map(|id| q.entry(*id)).collect();
            let hashes: Vec<String> = entries
                .iter()
                .filter_map(|e| e.hydrus_hash.clone().filter(|h| !h.is_empty()))
                .collect();
            (hashes, entries[0].filename().to_owned(), entries.len())
        };
        if hashes.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                SHOW,
                "None of the selected files have a hash yet, so there is nothing to look up.\n\n\
                 Hashes are worked out when files are added; Settings > General > Hash source \
                 controls where they come from.",
            );
            return;
        }
        if self.s.hydrus_batch_worker.is_running() {
            dialogs::information(
                &*self.s.prompter,
                SHOW,
                "A Hydrus request is already under way. Wait for it to finish, then try again.",
            );
            return;
        }
        let settings = self.hydrus_settings();
        let client = match HydrusClient::new(&settings.hydrus) {
            Ok(c) => c,
            Err(e) => {
                dialogs::warning(&*self.s.prompter, SHOW, &e.user_message());
                return;
            }
        };
        self.run_hydrus_batch(
            move |handle| {
                let known = match handle.block_on(client.filter_known_hashes(&hashes)) {
                    Ok(k) => k,
                    Err(e) => return Showed::Unreachable(e),
                };
                let showable: Vec<String> = hashes
                    .iter()
                    .filter(|h| known.contains(*h))
                    .cloned()
                    .collect();
                if showable.is_empty() {
                    return Showed::NoneKnown;
                }
                let page_name = if showable.len() == 1 {
                    first_name
                } else {
                    format!("Hatate: {} files", showable.len())
                };
                match handle.block_on(client.show_files_in_client(&showable, &page_name)) {
                    Ok(_) => Showed::Shown {
                        count: showable.len(),
                        skipped: total - showable.len(),
                    },
                    Err(e) => Showed::Refused(e),
                }
            },
            |w, outcome| w.on_shown_in_hydrus(outcome),
        );
    }

    fn on_shown_in_hydrus(&self, outcome: Showed) {
        if self.s.closing.get() {
            return;
        }
        match outcome {
            Showed::Unreachable(e) => {
                tracing::warn!("Could not reach Hydrus: {}", e.detail());
                self.s.prompter.message_detailed(
                    Kind::Warning,
                    SHOW,
                    &e.user_message(),
                    e.message(),
                    Buttons::Ok,
                    Box::new(|_| {}),
                );
            }
            Showed::NoneKnown => dialogs::information(
                &*self.s.prompter,
                SHOW,
                "Hydrus doesn't have any of the selected files, so there is nothing for it to \
                 show.\n\nThat usually means they were added from outside Hydrus and haven't \
                 been sent to it yet.",
            ),
            Showed::Refused(e) => {
                tracing::warn!("Show in Hydrus failed: {}", e.detail());
                dialogs::warning(&*self.s.prompter, SHOW, &show_failure_text(&e));
            }
            Showed::Shown { count, skipped } => {
                let note = if skipped > 0 {
                    format!(" ({skipped} not in Hydrus)")
                } else {
                    String::new()
                };
                tracing::info!("Opened a Hydrus page for {count} file(s){note}");
                self.say(&format!("Showing {count} file(s) in Hydrus{note}"));
            }
        }
    }

    /// `_delete_rows_from_hydrus(entries)`: asks Hydrus to delete these files from its own
    /// database (into its trash), so its database and its storage stay in agreement. Asks once,
    /// and Cancel is the default (design gate 2.6; Python's default was already No).
    pub fn delete_rows_from_hydrus(&self, ids: &[EntryId]) {
        let live = self.live_ids(ids);
        if live.is_empty() {
            return;
        }
        if !self.hydrus_configured() {
            dialogs::warning(&*self.s.prompter, DELETE, NO_KEY);
            return;
        }
        if self.s.hydrus_batch_worker.is_running() {
            dialogs::information(
                &*self.s.prompter,
                DELETE,
                "A Hydrus request is already under way. Wait for it to finish, then try again.",
            );
            return;
        }
        let settings = self.hydrus_settings();
        let client = match HydrusClient::new(&settings.hydrus) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                dialogs::warning(&*self.s.prompter, DELETE, &e.user_message());
                return;
            }
        };
        let hashes: Vec<String> = {
            let q = self.s.queue.borrow();
            live.iter()
                .filter_map(|id| q.entry(*id))
                .filter_map(|e| e.hydrus_hash.clone().filter(|h| !h.is_empty()))
                .collect()
        };
        let c = client.clone();
        self.run_hydrus_batch(
            move |handle| {
                if hashes.is_empty() {
                    return Asked::States(Default::default());
                }
                match handle.block_on(c.deletion_states(&hashes)) {
                    Ok(s) => Asked::States(s),
                    Err(e) => Asked::Unreachable(e),
                }
            },
            move |w, asked| w.on_delete_states(live, client, asked),
        );
    }

    fn on_delete_states(&self, live: Vec<EntryId>, client: Arc<HydrusClient>, asked: Asked) {
        if self.s.closing.get() {
            return;
        }
        let states = match asked {
            Asked::Unreachable(e) => {
                tracing::warn!("Could not reach Hydrus: {}", e.detail());
                self.s.prompter.message_detailed(
                    Kind::Warning,
                    DELETE,
                    &e.user_message(),
                    e.message(),
                    Buttons::Ok,
                    Box::new(|_| {}),
                );
                return;
            }
            Asked::States(s) => s,
        };
        let deletable: Vec<(EntryId, String, String)> = {
            let q = self.s.queue.borrow();
            live.iter()
                .filter_map(|id| q.entry(*id).map(|e| (*id, e)))
                .filter_map(|(id, e)| {
                    let h = e.hydrus_hash.clone().unwrap_or_default();
                    (states.get(&h) == Some(DeletionState::Present))
                        .then(|| (id, h, e.filename().to_owned()))
                })
                .collect()
        };
        if deletable.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                DELETE,
                "Hydrus isn't holding any of the selected files, so there is nothing for it to \
                 delete.\n\nThat usually means they were added from outside Hydrus, or have \
                 already been deleted from it.",
            );
            return;
        }
        let skipped = live.len() - deletable.len();
        let mut detail = deletable
            .iter()
            .take(15)
            .map(|(_, _, n)| n.clone())
            .collect::<Vec<_>>()
            .join("\n");
        if deletable.len() > 15 {
            detail.push_str(&format!("\n\u{2026} and {} more", deletable.len() - 15));
        }
        let mut text = format!("Tell Hydrus to delete {} file(s)?", deletable.len());
        if skipped > 0 {
            text.push_str(&format!(
                "\n\n{skipped} of the selected file(s) aren't in Hydrus and will be left alone."
            ));
        }
        text.push_str(
            "\n\nHydrus moves them to its trash, so you can still get them back from Hydrus \
             until the trash is emptied.\n\nThey'll also be taken out of this list.",
        );
        let w = self.clone();
        self.s.prompter.message_detailed(
            Kind::Warning,
            DELETE,
            &text,
            &detail,
            Buttons::DeleteCancel,
            Box::new(move |r| {
                if r != Response::Delete {
                    w.say("Delete cancelled");
                    return;
                }
                w.delete_in_hydrus(deletable, skipped, client);
            }),
        );
    }

    fn delete_in_hydrus(
        &self,
        deletable: Vec<(EntryId, String, String)>,
        skipped: usize,
        client: Arc<HydrusClient>,
    ) {
        let hashes: Vec<String> = deletable.iter().map(|(_, h, _)| h.clone()).collect();
        self.run_hydrus_batch(
            move |handle| {
                let asked = match handle
                    .block_on(client.delete_files(&hashes, Some("Deleted from hatate-linux")))
                {
                    Ok(a) => a,
                    Err(e) => return Deleted::Refused(e),
                };
                // The delete endpoint answers with an empty body whatever happened, so the only
                // way to know is to ask Hydrus what it now thinks of those files.
                let after = handle.block_on(client.deletion_states(&asked)).ok();
                let gone = after
                    .map(|s| {
                        s.iter()
                            .filter(|(_, st)| {
                                matches!(st, DeletionState::Trashed | DeletionState::Deleted)
                            })
                            .map(|(h, _)| h.to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                Deleted::Asked { asked, gone }
            },
            move |w, outcome| w.on_deleted_from_hydrus(&deletable, skipped, outcome),
        );
    }

    fn on_deleted_from_hydrus(
        &self,
        deletable: &[(EntryId, String, String)],
        skipped: usize,
        outcome: Deleted,
    ) {
        if self.s.closing.get() {
            return;
        }
        let (asked, gone) = match outcome {
            Deleted::Refused(e) => {
                tracing::error!("Delete from Hydrus failed: {}", e.detail());
                self.s.prompter.message_detailed(
                    Kind::Warning,
                    DELETE,
                    &format!("Hydrus refused the delete: {}", e.user_message()),
                    e.message(),
                    Buttons::Ok,
                    Box::new(|_| {}),
                );
                return;
            }
            Deleted::Asked { asked, gone } => (asked, gone),
        };
        let confirmed: Vec<EntryId> = deletable
            .iter()
            .filter(|(_, h, _)| gone.contains(h))
            .map(|(id, _, _)| *id)
            .collect();
        let stubborn = asked.len() - confirmed.len();
        tracing::info!(
            "Hydrus deleted {} of {} requested file(s)",
            confirmed.len(),
            asked.len()
        );
        if !confirmed.is_empty() {
            self.remove_entries(&confirmed, "deleted from Hydrus");
        }
        let mut summary = format!(
            "Hydrus deleted {} file(s) (now in its trash).",
            confirmed.len()
        );
        if stubborn > 0 {
            summary.push_str(&format!(
                "\n\n{stubborn} did not end up deleted - Hydrus accepted the request but still \
                 reports holding them. See Help > View Logs."
            ));
        }
        if skipped > 0 {
            summary.push_str(&format!(
                "\n\n{skipped} were left alone because Hydrus doesn't hold them."
            ));
        }
        dialogs::information(&*self.s.prompter, DELETE, &summary);
        self.say(&format!("Hydrus deleted {} file(s)", confirmed.len()));
    }
}

/// What the poll worker tells the window.
#[derive(Debug)]
pub enum PollMsg {
    Resolved {
        index: usize,
        hash: Option<String>,
        warning: Option<String>,
    },
    Countdown {
        label: String,
        remaining: f64,
        total: f64,
    },
}

/// `HydrusImportPollWorker`'s three signals, as messages to the main thread.
struct Bridge<'a> {
    ctx: &'a Ctx<PollMsg>,
}

impl PollEvents for Bridge<'_> {
    fn entry_resolved(&mut self, index: usize, hash: Option<&str>, warning: Option<&str>) {
        self.ctx.report(PollMsg::Resolved {
            index,
            hash: hash.map(str::to_owned),
            warning: warning.map(str::to_owned),
        });
    }

    fn wait_countdown(&mut self, label: &str, remaining: f64, total: f64) {
        self.ctx.report(PollMsg::Countdown {
            label: label.to_owned(),
            remaining,
            total,
        });
    }

    fn finished_all(&mut self) {}
}
