//! Files > Query Hydrus (`gui/hydrus_query_dialog.py` `HydrusQueryDialog` and
//! `MainWindow.action_query_hydrus` at `76c85c4`): enter tags, list the files Hydrus holds with
//! all of them, import the selected ones into the list as rows (downloaded to a temp directory so
//! the engines can read them from disk).
//!
//! Search and import go through the merged `HydrusClient` off the main thread; the dialog stays
//! open and answers while they run. Python ran both on the GUI thread behind a modal box.
//! `HydrusFile` carries no `hydrus_file_id` onto the row: `Entry` has no such field in this port
//! and nothing reads one (`docs/gtk-6g.md`).

use crate::add_tags::splitlines;
use crate::dialogs::{self, Buttons, Kind};
use crate::widgets;
use crate::window::MainWindow;
use crate::worker::{Ctx, Event};
use gtk4 as gtk;
use gtk4::prelude::*;
use hatate_core::hydrus_client::{HydrusClient, HydrusError, HydrusFile};
use hatate_core::model::{Entry, Tag, TagSource};
use hatate_core::pycompat::strip;
use hatate_core::tag_rules::apply_namespace_remap;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

pub const TITLE: &str = "Query Hydrus";
pub const MIN_WIDTH: i32 = 420;
pub const MIN_HEIGHT: i32 = 480;

/// `str(None)` where Python formats an unknown int or mime.
fn shown<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or_else(|| "None".to_owned(), T::to_string)
}

/// One line of the results list (`_do_search`).
pub fn result_label(m: &HydrusFile, already_added: bool) -> String {
    let hash: String = m.hash.chars().take(12).collect();
    format!(
        "{hash}\u{2026}  ({}, {}x{}, {} tag(s)){}",
        shown(&m.mime),
        shown(&m.width),
        shown(&m.height),
        m.tags.len(),
        if already_added {
            " [already in list]"
        } else {
            ""
        }
    )
}

/// `_parse_tag`: `namespace:name` splits on the first colon.
pub fn parse_hydrus_tag(raw: &str) -> Tag {
    let (namespace, name) = match raw.split_once(':') {
        Some((ns, name)) => (Some(ns.to_owned()), name.to_owned()),
        None => (None, raw.to_owned()),
    };
    Tag {
        name,
        namespace,
        source: TagSource::Hydrus,
    }
}

/// The tags a search asks for: one per non-blank line, stripped.
pub fn query_tags(text: &str) -> Vec<String> {
    splitlines(text)
        .into_iter()
        .map(strip)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What the import worker found for one selected result.
enum Fetched {
    /// Already in the list (or taken earlier in this same selection): not downloaded.
    Duplicate,
    /// `user` is what the box says, `detail` the raw text under it.
    Failed {
        hash: String,
        user: String,
        detail: String,
    },
    Entry(Box<Entry>),
}

pub struct HydrusQueryDialog {
    pub window: gtk::Window,
    pub tags: gtk::TextView,
    pub search_button: gtk::Button,
    pub results: gtk::ListBox,
    pub import_button: gtk::Button,
    pub cancel_button: gtk::Button,
    metadata: RefCell<Vec<HydrusFile>>,
    existing_hashes: RefCell<HashSet<String>>,
    busy: Cell<bool>,
    main: MainWindow,
}

impl HydrusQueryDialog {
    fn build(main: &MainWindow, existing_hashes: HashSet<String>) -> Rc<HydrusQueryDialog> {
        let window = gtk::Window::builder()
            .title(TITLE)
            .transient_for(&main.win)
            .modal(true)
            .default_width(520)
            .default_height(520)
            .build();
        window.set_size_request(MIN_WIDTH, MIN_HEIGHT);
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);
        column.append(&widgets::heading(TITLE));
        let prompt = gtk::Label::new(Some("Tags (one per line, ANDed together):"));
        prompt.set_xalign(0.0);
        column.append(&prompt);
        let tags = gtk::TextView::new();
        tags.set_accepts_tab(false);
        tags.set_monospace(true);
        let tags_scroll = gtk::ScrolledWindow::new();
        tags_scroll.set_child(Some(&tags));
        tags_scroll.set_min_content_height(72);
        tags_scroll.add_css_class("field");
        column.append(&tags_scroll);
        let search_button = widgets::pill_button("Search Hydrus", false, false);
        search_button.set_halign(gtk::Align::Start);
        column.append(&search_button);
        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::Multiple);
        let results_scroll = gtk::ScrolledWindow::new();
        results_scroll.set_child(Some(&results));
        // A row wraps rather than clips, so the "[already in list]" tail shows at the minimum size.
        results_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        results_scroll.set_vexpand(true);
        results_scroll.set_min_content_height(120);
        results_scroll.add_css_class("field");
        column.append(&results_scroll);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row.append(&spacer);
        let cancel_button = widgets::pill_button("Cancel", false, false);
        let import_button = widgets::pill_button("Import selected", true, false);
        row.append(&cancel_button);
        row.append(&import_button);
        column.append(&row);
        window.set_child(Some(&column));
        gtk::prelude::GtkWindowExt::set_focus(&window, Some(&tags));

        let dialog = Rc::new(HydrusQueryDialog {
            window: window.clone(),
            tags,
            search_button: search_button.clone(),
            results,
            import_button: import_button.clone(),
            cancel_button: cancel_button.clone(),
            metadata: RefCell::new(Vec::new()),
            existing_hashes: RefCell::new(existing_hashes),
            busy: Cell::new(false),
            main: main.clone(),
        });
        {
            let w = window.clone();
            cancel_button.connect_clicked(move |_| w.close());
        }
        for (button, import) in [(&search_button, false), (&import_button, true)] {
            let d = Rc::downgrade(&dialog);
            button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    if import {
                        d.import_selected();
                    } else {
                        d.search();
                    }
                }
            });
        }
        dialog
    }

    /// The text of the tags field.
    pub fn tags_text(&self) -> String {
        let buffer = self.tags.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    pub fn set_tags_text(&self, text: &str) {
        self.tags.buffer().set_text(text);
    }

    /// The results list as it reads now.
    pub fn labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut child = self.results.first_child();
        while let Some(row) = child {
            if let Some(label) = row
                .downcast_ref::<gtk::ListBoxRow>()
                .and_then(|r| r.child())
                .and_then(|c| c.downcast::<gtk::Label>().ok())
            {
                out.push(label.text().to_string());
            }
            child = row.next_sibling();
        }
        out
    }

    pub fn select_results(&self, rows: &[usize]) {
        self.results.unselect_all();
        for &i in rows {
            if let Some(row) = self.results.row_at_index(i as i32) {
                self.results.select_row(Some(&row));
            }
        }
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.search_button.set_sensitive(!busy);
        self.import_button.set_sensitive(!busy);
    }

    /// `_do_search`.
    pub fn search(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        let tags = query_tags(&self.tags_text());
        let prompter = self.main.s.prompter.clone();
        if tags.is_empty() {
            dialogs::warning(&*prompter, TITLE, "Enter at least one tag.");
            return;
        }
        tracing::info!("Querying Hydrus for tags: {tags:?}");
        let settings = self.main.s.settings.lock().unwrap().clone();
        let Some(handle) = self.main.s.workers.handle() else {
            return;
        };
        self.set_busy(true);
        let d = Rc::downgrade(self);
        let job = self.main.s.workers.run(
            move |_ctx: &Ctx<()>| -> Result<Vec<HydrusFile>, HydrusError> {
                let client = HydrusClient::new(&settings.hydrus)?;
                let ids = handle.block_on(client.search_files(&tags))?;
                tracing::debug!("Hydrus search_files returned {} file id(s)", ids.len());
                if ids.is_empty() {
                    Ok(Vec::new())
                } else {
                    handle.block_on(client.get_file_metadata(&ids))
                }
            },
            move |event| {
                let Some(d) = d.upgrade() else { return };
                if let Event::Done(found) = event {
                    d.on_searched(found);
                }
            },
        );
        self.main.s.hydrus_query_worker.replace(job);
    }

    fn on_searched(&self, found: Result<Vec<HydrusFile>, HydrusError>) {
        self.set_busy(false);
        let prompter = self.main.s.prompter.clone();
        let metadata = match found {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Query Hydrus search failed: {}", e.detail());
                prompter.message_detailed(
                    Kind::Critical,
                    TITLE,
                    &e.user_message(),
                    e.message(),
                    Buttons::Ok,
                    Box::new(|_| {}),
                );
                return;
            }
        };
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        {
            let existing = self.existing_hashes.borrow();
            for m in &metadata {
                let label = gtk::Label::new(Some(&result_label(m, existing.contains(&m.hash))));
                label.set_xalign(0.0);
                label.set_wrap(true);
                label.set_margin_start(8);
                label.set_margin_end(8);
                label.set_margin_top(4);
                label.set_margin_bottom(4);
                self.results.append(&label);
            }
        }
        let none = metadata.is_empty();
        *self.metadata.borrow_mut() = metadata;
        if none {
            tracing::info!("Query Hydrus: no files matched");
            dialogs::information(&*prompter, TITLE, "No files matched those tags.");
        }
    }

    /// `_do_import`.
    pub fn import_selected(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        let mut selected: Vec<usize> = self
            .results
            .selected_rows()
            .iter()
            .map(|r| r.index() as usize)
            .collect();
        selected.sort_unstable();
        let prompter = self.main.s.prompter.clone();
        if selected.is_empty() {
            dialogs::warning(&*prompter, TITLE, "Select at least one file to import.");
            return;
        }
        let picked: Vec<HydrusFile> = {
            let m = self.metadata.borrow();
            selected.iter().filter_map(|i| m.get(*i).cloned()).collect()
        };
        let settings = self.main.s.settings.lock().unwrap().clone();
        let existing = self.existing_hashes.borrow().clone();
        let Some(handle) = self.main.s.workers.handle() else {
            return;
        };
        self.set_busy(true);
        let d = Rc::downgrade(self);
        let job = self.main.s.workers.run(
            move |_ctx: &Ctx<()>| -> Vec<Fetched> {
                let client = match HydrusClient::new(&settings.hydrus) {
                    Ok(c) => c,
                    Err(e) => {
                        return picked
                            .iter()
                            .map(|m| Fetched::Failed {
                                hash: m.hash.clone(),
                                user: e.user_message(),
                                detail: e.message().to_owned(),
                            })
                            .collect();
                    }
                };
                // Kept after the dialog: the rows point at these files.
                let dir = match tempfile::Builder::new().prefix("hatate-linux-").tempdir() {
                    Ok(d) => d.keep(),
                    Err(_) => return Vec::new(),
                };
                let mut taken = existing;
                let mut out = Vec::with_capacity(picked.len());
                for meta in picked {
                    if taken.contains(&meta.hash) {
                        tracing::debug!(
                            "Skipping {} - already in the list, not downloading again",
                            meta.hash.chars().take(12).collect::<String>()
                        );
                        out.push(Fetched::Duplicate);
                        continue;
                    }
                    let ext = meta
                        .mime
                        .as_deref()
                        .and_then(|m| m.rsplit('/').next())
                        .filter(|e| !e.is_empty())
                        .unwrap_or("jpg");
                    let dest = dir.join(format!("{}.{ext}", meta.hash));
                    let Some(file_id) = meta.file_id else {
                        out.push(Fetched::Failed {
                            hash: meta.hash.clone(),
                            user: "Hydrus gave no id for this file, so it cannot be fetched."
                                .to_owned(),
                            detail: "no file_id in the metadata".to_owned(),
                        });
                        continue;
                    };
                    if let Err(error) = handle.block_on(client.download_file(file_id, &dest)) {
                        tracing::error!("Failed to download Hydrus file: {}", error.detail());
                        out.push(Fetched::Failed {
                            hash: meta.hash.clone(),
                            user: error.user_message(),
                            detail: error.message().to_owned(),
                        });
                        continue;
                    }
                    let mut entry = Entry::new(dest.to_string_lossy().into_owned());
                    entry.hydrus_hash = Some(meta.hash.clone());
                    if !meta.tags.is_empty() {
                        let parsed: Vec<Tag> =
                            meta.tags.iter().map(|n| parse_hydrus_tag(n)).collect();
                        entry.add_tags(
                            apply_namespace_remap(&parsed, &settings),
                            Some(TagSource::Hydrus),
                        );
                    }
                    // Catches a duplicate inside this same selection too.
                    taken.insert(meta.hash);
                    out.push(Fetched::Entry(Box::new(entry)));
                }
                out
            },
            move |event| {
                let Some(d) = d.upgrade() else { return };
                if let Event::Done(fetched) = event {
                    d.on_imported(fetched);
                }
            },
        );
        self.main.s.hydrus_query_worker.replace(job);
    }

    fn on_imported(&self, fetched: Vec<Fetched>) {
        self.set_busy(false);
        let prompter = self.main.s.prompter.clone();
        let (mut imported, mut skipped, mut failed) = (Vec::<Entry>::new(), 0usize, 0usize);
        for f in fetched {
            match f {
                Fetched::Duplicate => skipped += 1,
                Fetched::Failed { hash, user, detail } => {
                    failed += 1;
                    let short: String = hash.chars().take(12).collect();
                    prompter.message_detailed(
                        Kind::Warning,
                        TITLE,
                        &format!("Skipped {short}: {user}"),
                        &detail,
                        Buttons::Ok,
                        Box::new(|_| {}),
                    );
                }
                Fetched::Entry(e) => imported.push(*e),
            }
        }
        tracing::info!(
            "Query Hydrus import complete: {} succeeded, {skipped} duplicates skipped, {failed} failed",
            imported.len()
        );
        self.window.close();
        if !imported.is_empty() || skipped > 0 {
            self.main.on_query_hydrus_done(imported, skipped);
        }
    }
}

impl MainWindow {
    /// `action_query_hydrus()`: the dialog, modal over the window.
    pub fn action_query_hydrus(&self) {
        if !self.hydrus_configured() {
            dialogs::warning(
                &*self.s.prompter,
                TITLE,
                "Set your Hydrus access key in Settings first.",
            );
            return;
        }
        let existing: HashSet<String> = self
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .filter_map(|e| e.hydrus_hash.clone().filter(|h| !h.is_empty()))
            .collect();
        let dialog = HydrusQueryDialog::build(self, existing);
        dialog.window.present();
        *self.s.hydrus_query_dialog.borrow_mut() = Some(dialog);
    }

    /// The part of `action_query_hydrus` after the dialog accepts: the imported rows join the
    /// list, and the status line counts them.
    pub fn on_query_hydrus_done(&self, imported: Vec<Entry>, skipped: usize) {
        let count = imported.len();
        {
            let mut q = self.s.queue.borrow_mut();
            let mut added = Vec::with_capacity(count);
            for entry in imported {
                added.push(q.push(entry));
            }
            self.s.queue_view.register_new(&added);
        }
        self.list_changed();
        let mut status = format!("Imported {count} file(s) from Hydrus");
        if skipped > 0 {
            status.push_str(&format!(
                " ({skipped} duplicate(s) already in the list, skipped)"
            ));
            tracing::info!(
                "Skipped {skipped} duplicate(s) already in the list during Query Hydrus import"
            );
        }
        self.say(&status);
        self.thumbnails_for_new_entries();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        hash: &str,
        mime: Option<&str>,
        w: Option<i64>,
        h: Option<i64>,
        tags: usize,
    ) -> HydrusFile {
        HydrusFile {
            file_id: Some(1),
            hash: hash.to_owned(),
            mime: mime.map(str::to_owned),
            width: w,
            height: h,
            tags: (0..tags).map(|i| format!("t{i}")).collect(),
        }
    }

    #[test]
    fn a_result_line_is_the_twelve_character_hash_then_the_facts() {
        let m = file(&"ab".repeat(32), Some("image/png"), Some(640), Some(480), 3);
        assert_eq!(
            result_label(&m, false),
            "abababababab\u{2026}  (image/png, 640x480, 3 tag(s))"
        );
        assert!(result_label(&m, true).ends_with(" [already in list]"));
    }

    #[test]
    fn an_unknown_mime_or_size_reads_as_none_like_python() {
        let m = file(&"cd".repeat(32), None, None, Some(9), 0);
        assert_eq!(
            result_label(&m, false),
            "cdcdcdcdcdcd\u{2026}  (None, Nonex9, 0 tag(s))"
        );
    }

    #[test]
    fn a_tag_splits_on_its_first_colon_only() {
        let t = parse_hydrus_tag("series:a:b");
        assert_eq!(
            (t.namespace.as_deref(), t.name.as_str()),
            (Some("series"), "a:b")
        );
        assert_eq!(t.source, TagSource::Hydrus);
        let t = parse_hydrus_tag("plain");
        assert_eq!((t.namespace, t.name.as_str()), (None, "plain"));
    }

    #[test]
    fn the_query_is_one_tag_per_non_blank_line() {
        assert_eq!(
            query_tags("  cat \n\n  character:reimu\r\n \n"),
            ["cat", "character:reimu"]
        );
        assert!(query_tags(" \n\t\n").is_empty());
    }
}
