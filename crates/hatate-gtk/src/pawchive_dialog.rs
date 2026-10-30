//! Files > Pawchive Index (`gui/pawchive_index_dialog.py` `PawchiveIndexDialog`,
//! `MainWindow.action_pawchive_index` at `76c85c4`, over the thread shells of
//! `workers/pawchive_index_worker.py`): choose the artists whose images are indexed, index or
//! refresh them one after another, and search pawchive's artists by name.
//!
//! Not modal: indexing an artist takes minutes and the list stays usable meanwhile. One window at a
//! time; asking again raises it. The crawl and the artist search are the merged
//! `hatate_core::pawchive_index` code on the worker bridge; the crawler's hosts are given to the
//! window ([`crate::window::Env::pawchive`]), so a test points it at a local server and the
//! shipped window has the real ones and no way to change them.

use crate::dialogs::{Buttons, Kind, Response};
use crate::widgets;
use crate::window::MainWindow;
use crate::worker::{Ctx, Event};
use gtk4 as gtk;
use gtk4::prelude::*;
use hatate_core::eta::thousands;
use hatate_core::localtime::local_stamp;
use hatate_core::pawchive_index::{CrawlResult, Creator, PawchiveIndex, creator_from_url};
use hatate_core::pawchive_index::{PawchiveCrawler, find_creators_for_ui, run_index_jobs};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

pub const TITLE: &str = "Pawchive Index";
/// The least the window shows without clipping its notes or its button rows (set on the window,
/// and held by `pawchive_and_query_windows_keep_their_stated_minimum`). Python states none.
pub const MIN_WIDTH: i32 = 720;
pub const MIN_HEIGHT: i32 = 680;
pub const COLUMNS: [&str; 5] = ["Artist", "Service", "Images", "Posts", "Indexed"];

/// What the index worker tells the window.
#[derive(Debug)]
enum IndexMsg {
    /// `progress(message, done, total)`; a `total` of 0 is unknown.
    Progress(String, usize, usize),
    /// `creator_done(result)`.
    CreatorDone(Box<CrawlResult>),
}

/// The cells of one row of the list (`_populate`).
pub fn row_cells(c: &Creator) -> [String; 5] {
    let mut images = c.image_count.to_string();
    if c.unfingerprinted > 0 {
        images.push_str(&format!(" ({} without a thumbnail)", c.unfingerprinted));
    }
    let indexed = match c.indexed_at {
        Some(t) if t != 0.0 => local_stamp(t).chars().take(16).collect(),
        _ => "not yet".to_owned(),
    };
    [
        c.name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| c.user_id.clone()),
        c.service.clone(),
        images,
        if c.post_count == 0 {
            String::new()
        } else {
            c.post_count.to_string()
        },
        indexed,
    ]
}

/// The window title: how many artists, how many images are searchable.
pub fn window_title(creators: &[Creator]) -> String {
    let total: i64 = creators
        .iter()
        .map(|c| c.image_count - c.unfingerprinted)
        .sum();
    format!(
        "{TITLE} - {} artist(s), {} image(s) searchable",
        creators.len(),
        thousands(total)
    )
}

/// `_on_creator_done`'s line for one artist.
pub fn summary_line(r: &CrawlResult) -> String {
    let who = r
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| r.user_id.clone());
    if let Some(error) = r.error_user_message().or_else(|| r.error.clone()) {
        return format!("{who}: {error}");
    }
    let mut line = format!(
        "{who}: {} image(s), {} new",
        thousands(r.images as i64),
        thousands(r.new as i64)
    );
    if r.unfingerprinted > 0 {
        line.push_str(&format!(", {} without a thumbnail", r.unfingerprinted));
    }
    if r.stopped {
        line.push_str(" - stopped");
    }
    line
}

/// One line of the artist search results (`_on_found`).
pub fn found_label(c: &serde_json::Value) -> String {
    let text = |k: &str| match c.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => "None".to_owned(),
        Some(other) => other.to_string(),
    };
    let favorited = c
        .get("favorited")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    format!(
        "{}  ({})  -  {} favourites",
        text("name"),
        text("service"),
        thousands(favorited)
    )
}

pub struct PawchiveIndexDialog {
    pub window: gtk::Window,
    pub table: gtk::ListBox,
    pub query: gtk::Entry,
    pub find_button: gtk::Button,
    pub from_matches_button: gtk::Button,
    pub results: gtk::ListBox,
    pub add_checked_button: gtk::Button,
    pub index_selected_button: gtk::Button,
    pub index_all_button: gtk::Button,
    pub remove_button: gtk::Button,
    pub stop_button: gtk::Button,
    pub progress: gtk::ProgressBar,
    pub status: gtk::Label,
    index: PawchiveIndex,
    creators: RefCell<Vec<Creator>>,
    found: RefCell<Vec<serde_json::Value>>,
    summary: RefCell<Vec<String>>,
    busy: Cell<bool>,
    main: MainWindow,
}

impl PawchiveIndexDialog {
    fn build(main: &MainWindow) -> Rc<PawchiveIndexDialog> {
        let window = gtk::Window::builder()
            .title(TITLE)
            .transient_for(&main.win)
            .default_width(760)
            .default_height(560)
            .build();
        window.set_size_request(MIN_WIDTH, MIN_HEIGHT);
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);
        let intro = gtk::Label::new(None);
        intro.set_markup(
            "Finds <b>resized or re-saved</b> copies of pawchive posts by the artists listed \
             here. Exact copies of any artist are already found by the Pawchive lookup in \
             Settings; this catches the ones whose file has changed.\n\nIndexing downloads a \
             small thumbnail of each of an artist's images, about one a second, and refreshing \
             only fetches what is new. Searching the index happens on this computer and sends \
             nothing.",
        );
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        column.append(&intro);

        let sizes: Vec<gtk::SizeGroup> = (0..COLUMNS.len())
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect();
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        for (i, name) in COLUMNS.iter().enumerate() {
            let h = widgets::section_label(name);
            h.set_xalign(0.0);
            h.set_hexpand(i == 0);
            sizes[i].add_widget(&h);
            head.append(&h);
        }
        column.append(&head);
        let table = gtk::ListBox::new();
        table.set_selection_mode(gtk::SelectionMode::Multiple);
        let table_scroll = gtk::ScrolledWindow::new();
        table_scroll.set_child(Some(&table));
        table_scroll.set_vexpand(true);
        table_scroll.set_min_content_height(140);
        table_scroll.add_css_class("field");
        column.append(&table_scroll);

        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let add_row = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let query = gtk::Entry::new();
        query.set_placeholder_text(Some("Artist name, or a pawchive link"));
        query.set_hexpand(true);
        add_row.append(&query);
        let find_button = widgets::pill_button("Find / Add", false, false);
        find_button.set_tooltip_text(Some(
            "A pawchive link (to the artist or any of their posts) is added straight away.\nA \
             name is searched for among pawchive's artists - the first search downloads their \
             list (~15 MB), which is kept for a week.",
        ));
        add_row.append(&find_button);
        let from_matches_button = widgets::pill_button("Add artists from my matches", false, false);
        from_matches_button.set_tooltip_text(Some(
            "Adds every artist with a pawchive match in the current list - most usefully the \
             exact copies the Pawchive lookup found, whose other images are the likeliest to \
             turn up again edited.",
        ));
        add_row.append(&from_matches_button);
        column.append(&add_row);

        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::None);
        let results_scroll = gtk::ScrolledWindow::new();
        results_scroll.set_child(Some(&results));
        results_scroll.set_max_content_height(150);
        results_scroll.set_propagate_natural_height(true);
        results_scroll.set_visible(false);
        results_scroll.add_css_class("field");
        column.append(&results_scroll);
        let add_checked_button = widgets::pill_button("Add the ticked artists", false, false);
        add_checked_button.set_halign(gtk::Align::Start);
        add_checked_button.set_visible(false);
        column.append(&add_checked_button);

        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let index_selected_button = widgets::pill_button("Index selected", false, false);
        let index_all_button = widgets::pill_button("Index / refresh all", false, false);
        let remove_button = widgets::pill_button("Remove selected", false, false);
        let stop_button = widgets::pill_button("Stop", false, false);
        stop_button.set_sensitive(false);
        stop_button.set_tooltip_text(Some(
            "Stops after the current request. What was indexed is kept, and indexing again \
             carries on from there.",
        ));
        for b in [
            &index_selected_button,
            &index_all_button,
            &remove_button,
            &stop_button,
        ] {
            actions.append(b);
        }
        column.append(&actions);

        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let progress = gtk::ProgressBar::new();
        progress.set_visible(false);
        column.append(&progress);
        let status = gtk::Label::new(Some(""));
        status.set_wrap(true);
        status.set_xalign(0.0);
        status.set_selectable(true);
        column.append(&status);
        let close_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        close_row.append(&spacer);
        let close = widgets::pill_button("Close", false, false);
        close_row.append(&close);
        column.append(&close_row);
        window.set_child(Some(&column));

        let paths = &main.s.paths;
        let dialog = Rc::new(PawchiveIndexDialog {
            window: window.clone(),
            table,
            query: query.clone(),
            find_button: find_button.clone(),
            from_matches_button: from_matches_button.clone(),
            results,
            add_checked_button: add_checked_button.clone(),
            index_selected_button: index_selected_button.clone(),
            index_all_button: index_all_button.clone(),
            remove_button: remove_button.clone(),
            stop_button: stop_button.clone(),
            progress,
            status,
            index: PawchiveIndex::new(paths.pawchive_index_db()),
            creators: RefCell::new(Vec::new()),
            found: RefCell::new(Vec::new()),
            summary: RefCell::new(Vec::new()),
            busy: Cell::new(false),
            main: main.clone(),
        });
        let wire = |button: &gtk::Button, f: fn(&Rc<PawchiveIndexDialog>)| {
            let d = Rc::downgrade(&dialog);
            button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    f(&d);
                }
            });
        };
        wire(&find_button, |d| d.find_or_add());
        wire(&from_matches_button, |d| d.add_from_matches());
        wire(&add_checked_button, |d| d.add_checked());
        wire(&index_selected_button, |d| d.start_indexing(true));
        wire(&index_all_button, |d| d.start_indexing(false));
        wire(&remove_button, |d| d.remove_selected());
        wire(&stop_button, |d| d.stop());
        {
            let d = Rc::downgrade(&dialog);
            query.connect_activate(move |_| {
                if let Some(d) = d.upgrade() {
                    d.find_or_add();
                }
            });
        }
        {
            let w = window.clone();
            close.connect_clicked(move |_| w.close());
        }
        {
            let d = Rc::downgrade(&dialog);
            window.connect_close_request(move |_| {
                if let Some(d) = d.upgrade() {
                    d.on_close();
                }
                gtk::glib::Propagation::Proceed
            });
        }
        dialog.populate();
        dialog
    }

    fn set_results_visible(&self, shown: bool) {
        if let Some(scroller) = self.results.parent() {
            scroller.set_visible(shown);
        }
        self.add_checked_button.set_visible(shown);
    }

    /// The artist search results as they read now.
    pub fn found_labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut child = self.results.first_child();
        while let Some(row) = child {
            if let Some(check) = row
                .downcast_ref::<gtk::ListBoxRow>()
                .and_then(|r| r.child())
                .and_then(|c| c.downcast::<gtk::CheckButton>().ok())
            {
                out.push(check.label().map(|l| l.to_string()).unwrap_or_default());
            }
            child = row.next_sibling();
        }
        out
    }

    pub fn status_text(&self) -> String {
        self.status.text().to_string()
    }

    pub fn row_texts(&self) -> Vec<[String; 5]> {
        self.creators.borrow().iter().map(row_cells).collect()
    }

    /// `_populate`.
    pub fn populate(&self) {
        let creators = match self.index.creators() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Pawchive index could not be read: {}", e.detail());
                self.status.set_text(&e.user_message());
                Vec::new()
            }
        };
        while let Some(child) = self.table.first_child() {
            self.table.remove(&child);
        }
        for c in &creators {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            for (i, text) in row_cells(c).iter().enumerate() {
                let label = gtk::Label::new(Some(text));
                label.set_xalign(0.0);
                label.set_hexpand(i == 0);
                row.append(&label);
            }
            self.table.append(&row);
        }
        self.window.set_title(Some(&window_title(&creators)));
        *self.creators.borrow_mut() = creators;
    }

    fn selected_rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .table
            .selected_rows()
            .iter()
            .map(|r| r.index() as usize)
            .collect();
        rows.sort_unstable();
        rows
    }

    /// `_selected_creators`.
    pub fn selected_creators(&self) -> Vec<(String, String)> {
        let creators = self.creators.borrow();
        self.selected_rows()
            .into_iter()
            .filter_map(|r| creators.get(r))
            .map(|c| (c.service.clone(), c.user_id.clone()))
            .collect()
    }

    /// `_select_creators(keys)`: every one of them, not only the last.
    pub fn select_creators(&self, keys: &[(String, String)]) {
        let keys: HashSet<&(String, String)> = keys.iter().collect();
        self.table.unselect_all();
        let creators = self.creators.borrow();
        for (i, c) in creators.iter().enumerate() {
            if keys.contains(&(c.service.clone(), c.user_id.clone())) {
                if let Some(row) = self.table.row_at_index(i as i32) {
                    self.table.select_row(Some(&row));
                }
            }
        }
    }

    pub fn select_rows(&self, rows: &[usize]) {
        self.table.unselect_all();
        for &i in rows {
            if let Some(row) = self.table.row_at_index(i as i32) {
                self.table.select_row(Some(&row));
            }
        }
    }

    /// `_find_or_add`.
    pub fn find_or_add(self: &Rc<Self>) {
        let text = self.query.text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        if let Some(creator) = creator_from_url(&text) {
            let added = self
                .index
                .add_creator(&creator.0, &creator.1, None)
                .unwrap_or(false);
            self.populate();
            self.select_creators(std::slice::from_ref(&creator));
            self.query.set_text("");
            self.status.set_text(&if added {
                format!(
                    "Added {} ({}) - click Index selected to index them.",
                    creator.1, creator.0
                )
            } else {
                "That artist is already in the list.".to_owned()
            });
            return;
        }
        if !self.find_button.is_sensitive() {
            return;
        }
        let Some(crawler) = self.main.pawchive_crawler() else {
            return;
        };
        let Some(handle) = self.main.s.workers.handle() else {
            return;
        };
        self.find_button.set_sensitive(false);
        self.status.set_text(&format!(
            "Searching pawchive's artists for \"{text}\"\u{2026}"
        ));
        let cache = self.main.s.paths.pawchive_creators();
        let d = Rc::downgrade(self);
        let job = self.main.s.workers.run(
            move |_ctx: &Ctx<()>| handle.block_on(find_creators_for_ui(&*crawler, &text, &cache)),
            move |event| {
                let Some(d) = d.upgrade() else { return };
                if let Event::Done((found, error)) = event {
                    d.on_found(found, error.map(|e| e.user_message()));
                }
            },
        );
        self.main.s.pawchive_find_worker.replace(job);
    }

    /// `_on_found(found, error)`.
    fn on_found(&self, found: Vec<serde_json::Value>, error: Option<String>) {
        self.find_button.set_sensitive(true);
        if let Some(error) = error {
            self.status
                .set_text(&format!("Could not search pawchive's artists: {error}"));
            return;
        }
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        for creator in &found {
            let check = gtk::CheckButton::with_label(&found_label(creator));
            self.results.append(&check);
        }
        let shown = !found.is_empty();
        self.set_results_visible(shown);
        self.status.set_text(&if shown {
            format!("{} artist(s) found - tick the ones to add.", found.len())
        } else {
            "No pawchive artist by that name.".to_owned()
        });
        *self.found.borrow_mut() = found;
    }

    /// The tick boxes of the result list.
    pub fn tick_found(&self, rows: &[usize]) {
        for &i in rows {
            if let Some(check) = self
                .results
                .row_at_index(i as i32)
                .and_then(|r| r.child())
                .and_then(|c| c.downcast::<gtk::CheckButton>().ok())
            {
                check.set_active(true);
            }
        }
    }

    /// `_add_checked`.
    pub fn add_checked(&self) {
        let found = self.found.borrow().clone();
        let mut added = Vec::new();
        for (i, creator) in found.iter().enumerate() {
            let ticked = self
                .results
                .row_at_index(i as i32)
                .and_then(|r| r.child())
                .and_then(|c| c.downcast::<gtk::CheckButton>().ok())
                .is_some_and(|c| c.is_active());
            if !ticked {
                continue;
            }
            let field = |k: &str| match creator.get(k) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            };
            let (service, user_id, name) = (field("service"), field("id"), field("name"));
            if self
                .index
                .add_creator(&service, &user_id, Some(&name))
                .unwrap_or(false)
            {
                added.push((service, user_id));
            }
        }
        self.set_results_visible(false);
        self.populate();
        self.select_creators(&added);
        self.status.set_text(&if added.is_empty() {
            "Nothing new was added.".to_owned()
        } else {
            format!(
                "Added {} artist(s) - click Index selected to index them.",
                added.len()
            )
        });
    }

    /// `_add_from_matches`.
    pub fn add_from_matches(&self) {
        let mut found: std::collections::BTreeSet<(String, String)> = Default::default();
        for entry in self.main.s.queue.borrow().entries() {
            for candidate in &entry.candidates {
                if let Some(c) = creator_from_url(&candidate.url) {
                    found.insert(c);
                }
            }
        }
        let added: Vec<(String, String)> = found
            .iter()
            .filter(|c| self.index.add_creator(&c.0, &c.1, None).unwrap_or(false))
            .cloned()
            .collect();
        self.populate();
        self.select_creators(&added);
        self.status.set_text(&if found.is_empty() {
            "No pawchive matches in the current list yet - searching with the Pawchive lookup on \
             finds them."
                .to_owned()
        } else {
            format!(
                "{} artist(s) have pawchive matches; {} were new and are selected - click Index \
                 selected.",
                found.len(),
                added.len()
            )
        });
    }

    /// `_remove_selected`.
    pub fn remove_selected(self: &Rc<Self>) {
        let chosen = self.selected_creators();
        if chosen.is_empty() {
            return;
        }
        let d = self.clone();
        self.main.s.prompter.message(
            Kind::Question,
            "Remove artists",
            &format!(
                "Remove {} artist(s) and everything indexed for them?",
                chosen.len()
            ),
            Buttons::RemoveCancel,
            Box::new(move |r| {
                if r != Response::Remove {
                    return;
                }
                for (service, user_id) in &chosen {
                    let _ = d.index.remove_creator(service, user_id);
                }
                d.populate();
                d.status
                    .set_text(&format!("Removed {} artist(s).", chosen.len()));
            }),
        );
    }

    /// `_start_indexing(selected_only)`.
    pub fn start_indexing(self: &Rc<Self>, selected_only: bool) {
        if self.busy.get() {
            return;
        }
        let jobs: Vec<(String, String)> = if selected_only {
            self.selected_creators()
        } else {
            self.creators
                .borrow()
                .iter()
                .map(|c| (c.service.clone(), c.user_id.clone()))
                .collect()
        };
        if jobs.is_empty() {
            self.status.set_text(if selected_only {
                "Select an artist first."
            } else {
                "Add an artist first."
            });
            return;
        }
        let Some(crawler) = self.main.pawchive_crawler() else {
            return;
        };
        let Some(handle) = self.main.s.workers.handle() else {
            return;
        };
        self.summary.borrow_mut().clear();
        self.set_busy(true);
        let index_path = self.main.s.paths.pawchive_index_db();
        let d = Rc::downgrade(self);
        let job = self.main.s.workers.run(
            move |ctx: &Ctx<IndexMsg>| {
                let index = PawchiveIndex::new(index_path);
                let stop = ctx.stop_flag().clone();
                let mut on_progress = |text: &str, done: usize, total: usize| {
                    ctx.report(IndexMsg::Progress(text.to_owned(), done, total));
                };
                let mut on_done = |r: &CrawlResult| {
                    ctx.report(IndexMsg::CreatorDone(Box::new(r.clone())));
                };
                handle.block_on(run_index_jobs(
                    &*crawler,
                    &index,
                    &jobs,
                    &stop,
                    &mut on_progress,
                    &mut on_done,
                ));
            },
            move |event| {
                let Some(d) = d.upgrade() else { return };
                match event {
                    Event::Progress(IndexMsg::Progress(text, done, total)) => {
                        d.on_progress(&text, done, total)
                    }
                    Event::Progress(IndexMsg::CreatorDone(r)) => d.on_creator_done(&r),
                    Event::Done(()) => d.on_finished(),
                }
            },
        );
        self.main.s.pawchive_index_worker.replace(job);
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        for b in [
            &self.index_selected_button,
            &self.index_all_button,
            &self.remove_button,
            &self.from_matches_button,
            &self.find_button,
            &self.add_checked_button,
        ] {
            b.set_sensitive(!busy);
        }
        self.stop_button.set_sensitive(busy);
        self.progress.set_visible(busy);
        if busy {
            self.progress.pulse();
        }
    }

    /// `_on_progress(text, done, total)`.
    fn on_progress(&self, text: &str, done: usize, total: usize) {
        if total > 0 {
            self.progress.set_fraction(done as f64 / total as f64);
            self.status.set_text(&format!(
                "{text}: {} of {}",
                thousands(done as i64),
                thousands(total as i64)
            ));
        } else {
            self.progress.pulse();
            let mut line = format!("{text}\u{2026}");
            if done > 0 {
                line.push_str(&format!(" ({} so far)", thousands(done as i64)));
            }
            self.status.set_text(&line);
        }
    }

    /// `_on_creator_done(result)`.
    fn on_creator_done(&self, r: &CrawlResult) {
        self.summary.borrow_mut().push(summary_line(r));
        self.populate();
    }

    /// `_on_finished`.
    fn on_finished(&self) {
        self.set_busy(false);
        let summary = self.summary.borrow().join("\n");
        self.status.set_text(&if summary.is_empty() {
            "Nothing was indexed.".to_owned()
        } else {
            summary
        });
    }

    /// `_stop`.
    pub fn stop(&self) {
        if let Some(job) = self.main.s.pawchive_index_worker.running() {
            job.stop();
            self.stop_button.set_sensitive(false);
            self.status
                .set_text("Stopping after the current request\u{2026}");
        }
    }

    /// `closeEvent`: stop and silence both workers (Python waited for them; here nothing waits).
    fn on_close(&self) {
        self.main.s.pawchive_index_worker.retire();
        self.main.s.pawchive_find_worker.retire();
        *self.main.s.pawchive_dialog.borrow_mut() = None;
    }
}

impl MainWindow {
    /// The crawler the window uses: the one it was given, or the real Pawchive over the window's
    /// own network client.
    pub fn pawchive_crawler(&self) -> Option<Arc<PawchiveCrawler>> {
        if let Some(c) = &self.s.pawchive_override {
            return Some(c.clone());
        }
        let backend = self.s.search.backend(&self.s.paths).ok()?;
        Some(Arc::new(PawchiveCrawler::new(backend.engine.net.clone())))
    }

    /// `action_pawchive_index()`: one window; asking again raises it.
    pub fn action_pawchive_index(&self) {
        let existing = self.s.pawchive_dialog.borrow().clone();
        let dialog = match existing {
            Some(d) => d,
            None => {
                let d = PawchiveIndexDialog::build(self);
                *self.s.pawchive_dialog.borrow_mut() = Some(d.clone());
                d
            }
        };
        dialog.window.present();
    }

    /// `closeEvent`: the Pawchive Index window goes with the main window.
    pub fn close_pawchive_dialog(&self) {
        let dialog = self.s.pawchive_dialog.borrow().clone();
        if let Some(d) = dialog {
            d.window.close();
        }
    }
}
