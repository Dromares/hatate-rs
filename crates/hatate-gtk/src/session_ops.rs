//! The window's list and session handling (`gui/main_window.py` at `76c85c4`): adding files and
//! folders, the hash pass, restoring the last session at start-up, Open / Save As / Clear Session,
//! the autosave timer (`gui/session_autosave.py`), the missing-file check's result, and the
//! close teardown (`closeEvent`).
//!
//! Every dialog goes through the window's [`Prompter`](crate::dialogs::Prompter), every long job
//! through its [`Workers`](crate::worker::Workers); a continuation never runs after the window
//! began closing (`self._closing`, DAN-128).

use crate::banners::thousands;
use crate::dialogs;
use crate::error::SessionError;
use crate::shell::{IDLE_ETA, IDLE_PROGRESS, QUOTA_PAUSED_ETA};
use crate::text::format_duration;
use crate::window::MainWindow;
use crate::worker::Event;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::batch::hashing::hash_files;
use hatate_core::batch::intake::{self, filter_duplicate_paths, scan_folder};
use hatate_core::batch::{Autosave, Silent};
use hatate_core::crash_marker::CrashMarkers;
use hatate_core::missing_files;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::queue::QueueModel;
use hatate_core::session::{self, LoadOptions, SessionStore, named};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// `FILE_DIALOG_FILTER` (`core/formats.py`): the file dialog's filter, kept in step with
/// `IMAGE_EXTENSIONS`.
pub const FILE_DIALOG_FILTER: &str = "Images and video (*.jpg *.jpeg *.jfif *.png *.apng *.gif \
    *.bmp *.webp *.avif *.jxl *.tif *.tiff *.webm *.mp4 *.mkv);;All files (*)";

/// The Open and Save Session dialogs' filter.
pub const SESSION_FILTER: &str = "Session files (*.json);;All files (*)";

/// `MIN_INTERVAL_SECONDS`: below this the timer would spend more time saving than not.
pub const MIN_AUTOSAVE_INTERVAL_SECONDS: i64 = 10;
/// `SHUTDOWN_WAIT_MS`: how long to let an in-flight write finish on quit.
pub const AUTOSAVE_SHUTDOWN_WAIT: Duration = Duration::from_millis(10_000);
/// The grace `closeEvent` gives every worker (`grace_ms=2000`).
pub const WORKER_GRACE: Duration = Duration::from_millis(2_000);

impl MainWindow {
    // ---- start-up -------------------------------------------------------------------------

    /// The tail of `MainWindow.__init__`: restore, autosave, the persisted quota pause.
    pub(crate) fn startup(&self) {
        self.accept_drops();
        if self.s.settings.lock().unwrap().restore_session_on_start {
            self.restore_saved_session();
        }
        self.apply_autosave_settings();
        self.show_persisted_quota_pause();
    }

    // ---- add files and folders ------------------------------------------------------------

    /// `action_add_files`.
    pub fn action_add_files(&self) {
        let w = self.clone();
        self.s.prompter.open_files(
            "Add images",
            FILE_DIALOG_FILTER,
            Box::new(move |paths| {
                if !paths.is_empty() {
                    w.import_files(paths);
                }
            }),
        );
    }

    /// `action_add_folder`.
    pub fn action_add_folder(&self) {
        let w = self.clone();
        self.s.prompter.open_folder(
            "Add folder",
            Box::new(move |folder| {
                if let Some(folder) = folder {
                    w.import_files(scan_folder(&folder));
                }
            }),
        );
    }

    /// `dropEvent`: folders are scanned, files with an image extension are kept.
    pub fn drop_paths(&self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        for p in paths {
            if p.is_dir() {
                files.extend(scan_folder(&p));
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(intake::has_image_extension)
            {
                files.push(p);
            }
        }
        self.import_files(files);
    }

    fn accept_drops(&self) {
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let w = self.downgrade();
        target.connect_drop(move |_, value, _, _| {
            let (Some(w), Ok(list)) = (w.upgrade(), value.get::<gdk::FileList>()) else {
                return false;
            };
            w.drop_paths(list.files().into_iter().filter_map(|f| f.path()).collect());
            true
        });
        self.win.add_controller(target);
    }

    /// `_import_files(paths)`: hash in the background with real progress, then add.
    pub fn import_files(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.s.file_hash_worker.is_running() {
            dialogs::information(
                &*self.s.prompter,
                "Add Files",
                "Please wait for the current batch to finish being added.",
            );
            return;
        }
        // Covers both the new files and any already-added entry that has no cached hash yet
        // (opportunistic maintenance, piggybacking on this pass).
        let mut to_hash: Vec<PathBuf> = self
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .filter(|e| e.hydrus_hash.as_deref().is_none_or(str::is_empty))
            .map(|e| PathBuf::from(&e.path))
            .collect();
        to_hash.extend(paths.iter().cloned());

        *self.s.pending_import_paths.borrow_mut() = paths;
        self.say(&format!("Hashing {} file(s)\u{2026}", to_hash.len()));
        self.set_progress(0, to_hash.len());

        let (workers, hash_source) = {
            let s = self.s.settings.lock().unwrap();
            (s.hash_workers, s.hash_source.clone())
        };
        let total = to_hash.len();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx| {
                let progress = |done: usize, total: usize, eta: f64| ctx.report((done, total, eta));
                hash_files(
                    &to_hash,
                    workers,
                    &hash_source,
                    ctx.stop_flag(),
                    Some(&progress),
                )
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    Event::Progress((done, total, eta)) => {
                        w.on_file_hash_progress(done, total, eta)
                    }
                    Event::Done(Some(hashes)) => w.on_file_hash_finished(hashes),
                    // The worker was stopped: Python's emits nothing.
                    Event::Done(None) => {}
                }
            },
        );
        let _ = total;
        self.s.file_hash_worker.replace(job);
    }

    fn set_progress(&self, value: usize, maximum: usize) {
        self.s.run_progress.set((value as i64, maximum as i64));
        let fraction = if maximum == 0 {
            0.0
        } else {
            value as f64 / maximum as f64
        };
        self.s
            .shell
            .progress_bar
            .set_fraction(fraction.clamp(0.0, 1.0));
    }

    /// `_on_file_hash_progress(done, total, eta)`.
    pub fn on_file_hash_progress(&self, done: usize, total: usize, eta_seconds: f64) {
        self.set_progress(done, total);
        self.say(&format!("Hashing files\u{2026} ({done}/{total})"));
        // Shown separately from the main status text so the countdown sits to the right of the
        // progress numbers rather than jostling them.
        let left = if eta_seconds >= 0.0 {
            format!("~{} left", format_duration(eta_seconds, false))
        } else {
            String::new()
        };
        self.s.shell.wait_countdown_label.set_text(&left);
    }

    /// `_on_file_hash_finished(path_to_hash)`.
    pub fn on_file_hash_finished(&self, path_to_hash: HashMap<PathBuf, String>) {
        if self.s.closing.get() {
            return; // see `_on_missing_files_checked` (DAN-128)
        }
        self.s.shell.wait_countdown_label.set_text("");
        let paths = std::mem::take(&mut *self.s.pending_import_paths.borrow_mut());
        self.s.shell.progress_bar.set_fraction(0.0);
        let (_, max) = self.s.run_progress.get();
        self.s.run_progress.set((0, max));

        // Apply freshly computed hashes to already-added entries that had none cached.
        {
            let mut q = self.s.queue.borrow_mut();
            let ids: Vec<_> = (0..q.len() as i64).filter_map(|r| q.id_at(r)).collect();
            for id in ids {
                if let Some(e) = q.entry_mut(id) {
                    if e.hydrus_hash.as_deref().is_none_or(str::is_empty) {
                        if let Some(h) = path_to_hash.get(Path::new(&e.path)) {
                            e.hydrus_hash = Some(h.clone());
                        }
                    }
                }
            }
        }

        let (unique, duplicates) = {
            let q = self.s.queue.borrow();
            filter_duplicate_paths(&paths, &path_to_hash, q.entries())
        };
        if unique.is_empty() {
            if duplicates > 0 {
                tracing::info!("All {duplicates} file(s) were already in the list, nothing added");
                dialogs::information(
                    &*self.s.prompter,
                    "Add Files",
                    &format!("All {duplicates} file(s) are already in the list."),
                );
            } else {
                self.say("Ready");
            }
            return;
        }
        tracing::info!(
            "Adding {} file(s) to the list ({duplicates} duplicate(s) skipped)",
            unique.len()
        );
        {
            let mut q = self.s.queue.borrow_mut();
            let mut added = Vec::with_capacity(unique.len());
            for p in &unique {
                let mut entry = Entry::new(p.to_string_lossy().into_owned());
                // Already computed: avoid re-hashing.
                entry.hydrus_hash = path_to_hash.get(p).cloned();
                added.push(q.push(entry));
            }
            // `_register_new_entries`: so the header cycle can restore insertion order.
            self.s.queue_view.register_new(&added);
        }
        self.list_changed();
        let total = self.s.queue.borrow().len();
        let mut status = format!("Added {} file(s), {total} total", unique.len());
        if duplicates > 0 {
            status.push_str(&format!(" ({duplicates} duplicate(s) skipped)"));
        }
        self.say(&status);
        // DEFERRED(6.g): `_lookup_existing_hydrus_tags(new_entries)`.
        // DEFERRED(6.d): `_schedule_viewport_thumbnails` / `_start_thumbnail_generation`.
    }

    /// What `_refresh_table()` does for this ticket: the list changed, so the empty state, the
    /// ghost glyph, the table and the count follow it (`_refresh_table`).
    pub fn list_changed(&self) {
        self.refresh_queue_empty_state();
        self.refresh_sent_count_label();
    }

    /// `_idle_run_text()`: what the strip's count says when no run is under way. "nothing
    /// queued" is only true of an empty list.
    pub fn idle_run_text(&self) -> String {
        let waiting = self
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .filter(|e| e.status == MatchStatus::NotSearched)
            .count();
        if waiting > 0 {
            format!("{} not searched", thousands(waiting as i64))
        } else {
            IDLE_PROGRESS.to_owned()
        }
    }

    /// `_refresh_run_progress_label()`: the count, and the estimate that joins it once a run has
    /// one. With no run to report the strip says so rather than going blank (R-05).
    /// DEFERRED(6.e): the pace estimate of a live run (`_run_estimate`); here a run that is
    /// active reports its count and `ETA paused`/`ETA \u{2014}` until 6.e supplies the estimate.
    pub fn refresh_run_progress_label(&self) {
        let (done, total) = self.s.run_progress.get();
        let strip = &self.s.shell;
        // A run paused on the quota is not idle and is not finishing: the estimate would be a lie
        // either way (S-04).
        let paused_eta = if self.s.quota_paused.get() {
            QUOTA_PAUSED_ETA
        } else {
            IDLE_ETA
        };
        if total <= 0 || (done <= 0 && !self.s.run_active.get()) {
            strip.run_progress_label.set_text(&self.idle_run_text());
            strip.run_eta_label.set_text(paused_eta);
            return;
        }
        self.s.run_counted.set(true);
        strip.run_progress_label.set_text(&format!(
            "{}/{} searched",
            thousands(done),
            thousands(total)
        ));
        strip.run_eta_label.set_text(paused_eta);
    }

    /// `_refresh_sent_count_label()`: keeps the status-bar "N sent" readout in step with the list.
    pub fn refresh_sent_count_label(&self) {
        let text = {
            let q = self.s.queue.borrow();
            let entries = q.entries();
            if entries.is_empty() {
                None
            } else {
                let sent = entries
                    .iter()
                    .filter(|e| e.sent_to_hydrus && e.hydrus_import_confirmed)
                    .count();
                let queued = entries
                    .iter()
                    .filter(|e| e.sent_to_hydrus && !e.hydrus_import_confirmed)
                    .count();
                let mut text = format!("{sent} sent");
                if queued > 0 {
                    text.push_str(&format!(" ({queued} queued)"));
                }
                // Only once something has actually been reviewed (R-04).
                if entries.iter().any(|e| e.reviewed) {
                    let to_review = entries
                        .iter()
                        .filter(|e| !e.sent_to_hydrus && !e.reviewed)
                        .count();
                    text.push_str(&format!(" \u{b7} {to_review} to review"));
                }
                Some(text)
            }
        };
        self.s
            .shell
            .sent_count_label
            .set_text(text.as_deref().unwrap_or(""));
        if !self.s.run_counted.get() && !self.s.run_active.get() {
            self.s
                .shell
                .run_progress_label
                .set_text(&self.idle_run_text());
        }
    }

    // ---- restore --------------------------------------------------------------------------

    /// `_restore_saved_session()`: reloads the working list from the previous run, so the re-hash
    /// is skipped.
    pub fn restore_saved_session(&self) {
        let store = SessionStore::new(&self.s.paths);
        // Taken before loading, which does not say why it came back empty: this tells "never
        // saved anything" from "a session exists but is empty or unreadable" (DAN-487).
        let had_session_file = store.has_saved_session();
        let opts = LoadOptions {
            now: (self.s.clock)(),
            unclean_shutdown: self.s.unclean_shutdown,
        };
        let loaded = match store.open_writer() {
            Ok(writer) => writer.restore(&opts),
            Err(refused) => {
                tracing::warn!("{refused}");
                store.load(&opts)
            }
        };
        for notice in &loaded.report.notices {
            tracing::info!("session: {notice:?}");
        }
        let entries = loaded.entries;
        if entries.is_empty() {
            if had_session_file && self.s.unclean_shutdown {
                // The status-bar line stays (G-06): a transient message needs a home even once
                // the page itself says it.
                self.say("Run interrupted \u{2014} nothing survived from the last session");
                self.show_nothing_survived();
            }
            return;
        }
        let count = entries.len();
        {
            let mut q = self.s.queue.borrow_mut();
            let added: Vec<_> = entries.iter().cloned().map(|e| q.push(e)).collect();
            self.s.queue_view.register_new(&added);
        }
        self.list_changed();
        if self.s.unclean_shutdown {
            // The banner replaces the status-bar line rather than joining it: two notices for the
            // same event is noise (commit 31478d5). `session_restore_status` stays unset, so the
            // quota text shows on its own if a pause is also pending.
            self.show_run_banner(&entries);
        } else {
            let status = format!("Restored {count} image(s) from your last session");
            *self.s.session_restore_status.borrow_mut() = Some(status.clone());
            self.say(&status);
        }
        self.start_missing_file_check(&entries);
        // DEFERRED(6.g): `_start_hydrus_reconcile(quiet=True)`.
        // DEFERRED(6.d): the viewport pass or `_start_thumbnail_generation(entries)`.
    }

    /// `_start_missing_file_check(entries)`: whether a restored session's files are still on disk,
    /// in the background.
    pub fn start_missing_file_check(&self, entries: &[Entry]) {
        if entries.is_empty() {
            return;
        }
        let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
        let check = self.s.missing_files.clone();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx| check(&paths, ctx.stop_flag()),
            move |event: Event<(), _>| {
                if let (Some(w), Event::Done(missing)) = (w.upgrade(), event) {
                    w.on_missing_files_checked(missing);
                }
            },
        );
        self.s.missing_file_worker.replace(job);
    }

    /// `_on_missing_files_checked(missing_paths)`.
    pub fn on_missing_files_checked(&self, missing: std::collections::HashSet<String>) {
        if self.s.closing.get() {
            // The check was running when the window started closing: a modal box belonging to a
            // window on its way out would startle a quitting user (DAN-128).
            return;
        }
        if missing.is_empty() {
            tracing::debug!("Missing-file check: every file in the session is still on disk");
            return;
        }
        let (affected, total) = {
            let mut q = self.s.queue.borrow_mut();
            let total = q.len();
            let ids: Vec<_> = (0..total as i64)
                .filter(|&row| q.entry_at(row).is_some_and(|e| missing.contains(&e.path)))
                .filter_map(|row| q.id_at(row))
                .collect();
            let affected = ids.len();
            for id in ids {
                if let Some(e) = q.entry_mut(id) {
                    e.file_missing = true;
                }
            }
            tracing::info!("Flagged {affected} entry(s) whose file is no longer on disk");
            (affected, total)
        };
        self.list_changed();
        self.say(&missing_files::missing_status_text(affected));
        // Said once, plainly, rather than left for the user to discover as a string of failures
        // later. The URL-importer note matters: those rows are still useful with the file gone.
        dialogs::information(
            &*self.s.prompter,
            "Missing files",
            &missing_files::missing_dialog_text(affected, total),
        );
    }

    // ---- Save As / Open / Clear -----------------------------------------------------------

    /// `action_save_session_as`: writes the current list to a file the user names, so several
    /// sessions can be kept side by side. Never disturbs the automatic session.
    pub fn action_save_session_as(&self) {
        if self.s.queue.borrow().is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Save Session",
                "There's nothing in the list to save.",
            );
            return;
        }
        let default_name = format!(
            "hatate-session-{}.json",
            local_stamp_compact((self.s.clock)())
        );
        let home = home_dir();
        let start = home.join(&default_name);
        let w = self.clone();
        self.s.prompter.save_file(
            "Save Session As",
            &start.to_string_lossy(),
            SESSION_FILTER,
            Box::new(move |path| {
                let Some(mut path) = path else { return };
                if !path.to_string_lossy().to_lowercase().ends_with(".json") {
                    // Saves the user a surprise later: the Open dialog filters on .json.
                    let mut os = path.into_os_string();
                    os.push(".json");
                    path = PathBuf::from(os);
                }
                let entries: Vec<Entry> = w.s.queue.borrow().entries().to_vec();
                match named::save(&path, &entries, (w.s.clock)()) {
                    Ok(()) => {
                        tracing::info!(
                            "Saved session to {} ({} entries)",
                            path.display(),
                            entries.len()
                        );
                        let base = path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        w.say(&format!("Saved {} image(s) to {base}", entries.len()));
                    }
                    Err(e) => {
                        let err = SessionError::from_save(&e);
                        tracing::warn!("{}", err.detail());
                        dialogs::warning(
                            &*w.s.prompter,
                            "Save Session",
                            &format!(
                                "Could not write the session to:\n{}\n\nSee Help > View Logs for \
                                 details.",
                                path.display()
                            ),
                        );
                    }
                }
            }),
        );
    }

    /// `action_open_session`: replaces the current list with a saved session file.
    pub fn action_open_session(&self) {
        if self.search_running() {
            dialogs::information(
                &*self.s.prompter,
                "Open Session",
                "A search is running. Stop it before opening a different session.",
            );
            return;
        }
        let w = self.clone();
        self.s.prompter.open_file(
            "Open Session",
            SESSION_FILTER,
            Box::new(move |path| {
                if let Some(path) = path {
                    w.open_session_file(path);
                }
            }),
        );
    }

    /// Whether a search is running (`self.worker.isRunning()`); 6.e owns the worker.
    pub fn search_running(&self) -> bool {
        self.s.queue.borrow().search_running()
    }

    fn open_session_file(&self, path: PathBuf) {
        let Some(incoming) = named::entry_count(&path) else {
            let e = SessionError::unreadable(&path, "not a readable version-1 session");
            tracing::warn!("{}", e.detail());
            dialogs::warning(
                &*self.s.prompter,
                "Open Session",
                &format!(
                    "That doesn't look like a session file this build can read:\n{}\n\nSee Help \
                     > View Logs for details.",
                    path.display()
                ),
            );
            return;
        };
        let proceed = {
            let w = self.clone();
            move |path: PathBuf| w.load_and_replace(path)
        };
        // Replacing the list is destructive and not undoable, so say what is traded for what.
        let current = self.s.queue.borrow().len();
        if current > 0 {
            let base = base_name(&path);
            dialogs::question(
                &*self.s.prompter,
                "Open Session",
                &format!(
                    "Replace the current list of {current} image(s) with {incoming} image(s) \
                     from {base}?\n\nAnything unsaved in the current list will be lost. Your \
                     files and anything already sent to Hydrus are untouched."
                ),
                Box::new(move |yes| {
                    if yes {
                        proceed(path);
                    }
                }),
            );
        } else {
            proceed(path);
        }
    }

    fn load_and_replace(&self, path: PathBuf) {
        let opts = LoadOptions::at((self.s.clock)());
        let loaded = named::load(&path, &opts);
        if loaded.entries.is_empty() {
            dialogs::warning(
                &*self.s.prompter,
                "Open Session",
                &format!("No usable entries were found in:\n{}", path.display()),
            );
            return;
        }
        let entries = loaded.entries;
        self.stop_background_workers();
        self.replace_entries(entries.clone());
        tracing::info!(
            "Opened session {} with {} entries",
            path.display(),
            entries.len()
        );
        self.say(&format!(
            "Opened {} - {} image(s)",
            base_name(&path),
            entries.len()
        ));
        self.start_missing_file_check(&entries);
        // DEFERRED(6.d): the viewport pass or `_start_thumbnail_generation(entries)`.
    }

    /// Replaces the working list wholesale (Python rebinds `self.entries`).
    fn replace_entries(&self, entries: Vec<Entry>) {
        *self.s.queue.borrow_mut() = QueueModel::new(entries, self.s.settings.clone());
        let ids: Vec<_> = {
            let q = self.s.queue.borrow();
            (0..q.len() as i64).filter_map(|r| q.id_at(r)).collect()
        };
        // `_register_new_entries(entries)`; the sort column and direction are left as they were.
        self.s.queue_view.register_new(&ids);
        self.list_changed();
    }

    /// `_stop_background_workers()`: stops anything still working on the OLD list before it is
    /// replaced, and drops its results (`retire_attrs(..., clear=True)`).
    pub fn stop_background_workers(&self) {
        self.s.file_hash_worker.retire();
        // DEFERRED(6.d): `thumbnail_worker`; DEFERRED(6.e): `availability_worker`.
    }

    /// `clear_session()`: discards the automatic session, both stores. `true` when nothing went
    /// wrong (it says nothing about whether anything was there).
    fn clear_saved_session(&self) -> bool {
        match SessionStore::new(&self.s.paths).open_writer() {
            Ok(writer) => match writer.clear() {
                Ok(_) => {
                    tracing::info!("Cleared the saved session");
                    true
                }
                Err(e) => {
                    tracing::warn!("{}", SessionError::from_save(&e).detail());
                    false
                }
            },
            Err(refused) => {
                tracing::warn!("{refused}");
                false
            }
        }
    }

    /// `action_clear_session`.
    ///
    /// Python's first line is `if not self.entries and not clear_session(): return`. With an empty
    /// list that **calls** `clear_session()`, which discards the saved session before anything is
    /// asked, and carries on to the question when it succeeded. Answering No then leaves the
    /// session already gone. This does the same, so the session on disk matches Python's after
    /// every answer.
    pub fn action_clear_session(&self) {
        if self.s.queue.borrow().is_empty() && !self.clear_saved_session() {
            return;
        }
        let w = self.clone();
        dialogs::question(
            &*self.s.prompter,
            "Clear Saved Session",
            "Empty the current list and discard the saved session?\n\nThis only affects this \
             app's list - your files and anything already sent to Hydrus are untouched.",
            Box::new(move |yes| {
                if !yes {
                    return;
                }
                w.replace_entries(Vec::new());
                w.clear_saved_session();
                w.s.shell.run_note_label.set_text("");
                w.hide_quota_banner();
                w.say("Cleared the list and the saved session");
            }),
        );
    }

    // ---- logs -----------------------------------------------------------------------------

    /// `action_open_log`: the matched-URLs log, with the desktop's handler.
    pub fn action_open_log(&self) {
        let path = PathBuf::from(self.s.settings.lock().unwrap().log_file_path.clone());
        if !path.exists() {
            dialogs::information(
                &*self.s.prompter,
                "Matched URLs Log",
                "The log file doesn't exist yet.",
            );
            return;
        }
        (self.s.open_path)(&path);
    }

    /// `action_view_logs`.
    pub fn action_view_logs(&self) {
        self.show_log_viewer(false);
    }

    /// `_open_crash_log`: the same dialog, pre-switched to the crash log.
    pub fn open_crash_log(&self) {
        self.show_log_viewer(true);
    }

    fn show_log_viewer(&self, crash: bool) {
        let opener = self.s.open_path.clone();
        let viewer = crate::logview::LogViewer::new(&self.win, self.s.logs.clone(), opener, crash);
        viewer.present();
        *self.s.log_viewer.borrow_mut() = Some(viewer);
    }

    // ---- autosave -------------------------------------------------------------------------

    /// `SessionAutosaver.apply_settings`: (re)starts or stops the timer to match the settings.
    pub fn apply_autosave_settings(&self) {
        if let Some(old) = self.s.autosave_timer.borrow_mut().take() {
            old.remove();
        }
        let (on, interval) = {
            let s = self.s.settings.lock().unwrap();
            (s.autosave_session, s.autosave_interval_seconds)
        };
        let Some(interval) = autosave_interval(on, interval) else {
            tracing::info!("Session autosave is off");
            return;
        };
        let w = self.downgrade();
        let id = glib::timeout_add_seconds_local(interval, move || {
            if let Some(w) = w.upgrade() {
                w.autosave_now();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        *self.s.autosave_timer.borrow_mut() = Some(id);
        tracing::info!("Session autosave every {interval}s");
    }

    /// `SessionAutosaver.save_now`: starts a background write, if one is warranted. Only the
    /// app's own session file; an empty list is skipped so clearing does not quietly destroy a
    /// session that still had useful contents.
    pub fn autosave_now(&self) {
        let entries: Vec<Entry> = self.s.queue.borrow().entries().to_vec();
        if entries.is_empty() {
            return;
        }
        if self.s.autosave_worker.is_running() {
            tracing::debug!("Skipping autosave - the previous one is still writing");
            return;
        }
        let writer = match SessionStore::new(&self.s.paths).open_writer() {
            Ok(w) => w,
            Err(refused) => {
                tracing::warn!("Autosave failed - {refused}");
                return;
            }
        };
        let now = (self.s.clock)();
        let inflight = self.s.autosave_inflight.clone();
        inflight.store(true, Ordering::SeqCst);
        let job = self.s.workers.run(
            move |_ctx| {
                let mut autosave = Autosave::new(&writer, &Silent, now);
                autosave.save(&entries, now);
                inflight.store(false, Ordering::SeqCst);
                autosave.failed
            },
            move |event: Event<(), bool>| {
                if let Event::Done(failed) = event {
                    if failed {
                        tracing::warn!("Autosave failed - see earlier log lines for why");
                    }
                }
            },
        );
        self.s.autosave_worker.replace(job);
    }

    // ---- close ----------------------------------------------------------------------------

    pub(crate) fn connect_close(&self) {
        let w = self.downgrade();
        self.win.connect_close_request(move |_| {
            if let Some(w) = w.upgrade() {
                w.close_teardown();
            }
            glib::Propagation::Proceed
        });
    }

    /// `closeEvent`, in Python's order.
    pub fn close_teardown(&self) {
        if self.s.closing.replace(true) {
            return;
        }
        // DEFERRED(6.i): stop the MCP server first. DEFERRED(6.g): close the Pawchive Index window.
        // DEFERRED(6.e): `engine_alerts.unsubscribe`.
        // Ask every worker to stop and silence it: results for a window on its way out are
        // never shown (`retire_attrs(..., grace_ms=2000)`).
        self.s.file_hash_worker.retire();
        self.s.missing_file_worker.retire();
        // Waits for an in-flight write and stops the timer, before the final save: a background
        // write landing afterwards would put a slightly older session on top of the one meant to
        // be authoritative (`SessionAutosaver.shutdown`).
        let t = std::time::Instant::now();
        while self.s.autosave_inflight.load(Ordering::SeqCst)
            && t.elapsed() < AUTOSAVE_SHUTDOWN_WAIT
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.s.autosave_worker.retire();
        if let Some(id) = self.s.autosave_timer.borrow_mut().take() {
            id.remove();
        }
        // DEFERRED(6.e): `lens_browser.shutdown()`. DEFERRED(6.d): `_save_table_header_state()`.
        let restore = self.s.settings.lock().unwrap().restore_session_on_start;
        if restore {
            self.save_session_now();
        }
        {
            let s = self.s.settings.lock().unwrap();
            if let Err(e) = s.save(&self.s.paths) {
                tracing::warn!("could not save the settings: {e}");
            }
        }
        self.s.workers.shutdown(WORKER_GRACE);
        // Last, now that everything that could still fail has already run: this is what tells the
        // next launch that this one ended cleanly (DAN-485).
        if let Some(run) = self.s.run_session.borrow_mut().take() {
            if let Err(e) = run.finish_clean((self.s.clock)()) {
                tracing::warn!("could not record the clean exit: {e}");
            }
        }
    }

    /// `save_session(self.entries)`: the final save of the automatic session. A full write, and
    /// not skipped for an empty list (unlike the autosave): Python writes the empty list too, so
    /// a list emptied since the last save leaves an empty session behind, not the old rows.
    fn save_session_now(&self) {
        let entries: Vec<Entry> = self.s.queue.borrow().entries().to_vec();
        match SessionStore::new(&self.s.paths).open_writer() {
            Ok(writer) => {
                if let Err(e) = writer.save(&entries, (self.s.clock)()) {
                    tracing::warn!("{}", SessionError::from_save(&e).detail());
                }
            }
            Err(refused) => tracing::warn!("{refused}"),
        }
    }

    /// The clean-exit age for the banner.
    pub fn last_clean_exit_age(&self) -> Option<f64> {
        CrashMarkers::new(&self.s.paths).last_clean_exit_age((self.s.clock)())
    }
}

/// The timer's period in seconds, or `None` when autosave is off: `max(MIN_INTERVAL_SECONDS,
/// int(settings.autosave_interval_seconds))`, so a self-defeating setting cannot make the timer
/// fire as fast as the loop turns.
pub fn autosave_interval(on: bool, seconds: i64) -> Option<u32> {
    on.then(|| {
        seconds
            .max(MIN_AUTOSAVE_INTERVAL_SECONDS)
            .min(i64::from(u32::MAX)) as u32
    })
}

/// The user's home folder (`Path.home()`).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// `time.strftime("%Y%m%d-%H%M")` in local time.
fn local_stamp_compact(epoch: f64) -> String {
    // `local_stamp` is `%Y-%m-%d %H:%M:%S` in the local zone.
    let stamp = hatate_core::localtime::local_stamp(epoch);
    let (date, time) = stamp.split_once(' ').unwrap_or((&stamp, "00:00:00"));
    format!("{}-{}", date.replace('-', ""), time[..5].replace(':', ""))
}

#[allow(dead_code)]
fn _session_now() -> f64 {
    session::now_epoch()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosave_interval_is_floored_and_off_stops_it() {
        assert_eq!(autosave_interval(true, 120), Some(120));
        assert_eq!(autosave_interval(true, 10), Some(10));
        // `test_a_self_defeating_interval_is_floored`: 0, 1 and a negative all become 10.
        assert_eq!(autosave_interval(true, 0), Some(10));
        assert_eq!(autosave_interval(true, 1), Some(10));
        assert_eq!(autosave_interval(true, -5), Some(10));
        // `test_turning_it_off_stops_the_timer`.
        assert_eq!(autosave_interval(false, 120), None);
    }

    #[test]
    fn the_default_session_name_is_pythons_strftime() {
        // 2026-10-23 08:05:09 UTC; the test zone is whatever TZ says, so pin it.
        let stamp = local_stamp_compact(1_792_742_709.0);
        assert_eq!(stamp.len(), 13, "{stamp}");
        assert_eq!(&stamp[8..9], "-");
    }

    #[test]
    fn the_file_filter_lists_every_extension_python_does() {
        for ext in intake::IMAGE_EXTENSIONS {
            assert!(
                FILE_DIALOG_FILTER.contains(&format!("*{ext}")),
                "{ext} missing from the file dialog filter"
            );
        }
        assert!(FILE_DIALOG_FILTER.ends_with(";;All files (*)"));
    }
}
