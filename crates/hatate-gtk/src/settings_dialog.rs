//! Settings > Preferences (`gui/settings_dialog.py` `SettingsDialog` and
//! `MainWindow.action_open_settings` at `76c85c4`), and the Find box over its pages
//! (`gui/settings_search.py`).
//!
//! The pages are data (`settings_spec.rs`), built by `settings_form.rs`; what each control holds
//! and what OK writes is `settings_values.rs`. The Shortcuts and MCP tabs are 6.i's: they are
//! here as tabs (so the tab numbers, and the Find box's "Shortcuts" and "MCP" entries, are
//! Python's) with nothing on them yet, and OK leaves their keys as they are.
//!
//! The six Test buttons and Fetch from Hydrus are network code and 6.h-2's; the buttons are built
//! and insensitive until then.

use crate::dialogs::{Buttons, Kind, Prompter};
use crate::settings_form::{Control, Form};
use crate::settings_node::Node;
use crate::settings_spec::PAGES;
use crate::settings_values::{self, Value, Values};
use crate::widgets;
use crate::window::MainWindow;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::Settings;
use hatate_core::lens_browser::deps::{BrowserEnv, check_dependencies};
use hatate_core::settings_search::{self, SettingEntry};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

pub const TITLE: &str = "Settings";
/// The tabs after the spec's pages, in Python's order (6.i's).
pub const LATER_TABS: &[&str] = &["Shortcuts", "MCP"];
pub const MIN_WIDTH: i32 = 560;
pub const MIN_HEIGHT: i32 = 440;
/// `self.resize(940, 660)`.
pub const DEFAULT_WIDTH: i32 = 940;
pub const DEFAULT_HEIGHT: i32 = 660;
/// `HIGHLIGHT_MS`: how long a jumped-to setting stays marked.
pub const HIGHLIGHT_MS: u64 = 2000;
/// The most results the Find box lists (`found[:40]`).
pub const MAX_RESULTS: usize = 40;

pub struct SettingsDialog {
    pub window: gtk::Window,
    pub find: gtk::SearchEntry,
    pub results: gtk::ListBox,
    results_scroll: gtk::ScrolledWindow,
    pub tabs: gtk::Notebook,
    pub ok_button: gtk::Button,
    pub cancel_button: gtk::Button,
    forms: Vec<Form>,
    scrolls: Vec<gtk::ScrolledWindow>,
    index: Vec<SettingEntry<gtk::Widget>>,
    /// What each results row jumps to: `(tab index, widget)`. Empty for the "Nothing matches" row.
    listed: RefCell<Vec<Option<(usize, gtk::Widget)>>>,
    /// `_deviantart_cookies_snapshot`.
    da_snapshot: String,
    /// What the Lens check reads of the process (`PATH`, `HOME`): a test gives it its own.
    pub lens_env: RefCell<BrowserEnv>,
    highlights: Rc<RefCell<HashMap<usize, u64>>>,
    next_token: Cell<u64>,
    main: MainWindow,
    /// Asks over this window: it is modal, so a box in the main window beneath it could not be
    /// reached (Python parents its boxes to the dialog). The scripted prompter has no windows
    /// and is used as it is.
    pub prompter: Rc<dyn Prompter>,
}

impl SettingsDialog {
    pub fn build(main: &MainWindow) -> Rc<SettingsDialog> {
        let settings = main.s.settings.lock().unwrap().clone();
        let window = gtk::Window::builder()
            .title(TITLE)
            .transient_for(&main.win)
            .modal(true)
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        window.set_size_request(MIN_WIDTH, MIN_HEIGHT);
        window.add_css_class("pick-window");
        window.add_css_class("settings-window");

        let initial = settings_values::initial(&settings);
        let tabs = gtk::Notebook::new();
        tabs.set_scrollable(true);
        tabs.set_vexpand(true);
        let mut forms = Vec::new();
        let mut scrolls = Vec::new();
        for (name, page) in PAGES {
            let form = Form::build(page);
            form.write(&initial);
            let scroll = scrollable(&form.root);
            tabs.append_page(&scroll, Some(&gtk::Label::new(Some(name))));
            scrolls.push(scroll);
            forms.push(form);
        }
        // `_update_engine_pipeline()` runs once at the end of the Engine page's build, so the line
        // is there before the Find box indexes the page
        if let Some(live) = forms.iter().find_map(|f| f.live("engine_pipeline")) {
            let mut all = Values::default();
            for f in &forms {
                all.0.extend(f.read().0);
            }
            live.set_text(&settings_values::pipeline_text(&settings, &all));
        }
        let mut index_pages = Vec::new();
        for (i, form) in forms.iter().enumerate() {
            index_pages.push((
                PAGES[i].0,
                scrolls[i].clone().upcast::<gtk::Widget>(),
                form.names(),
            ));
        }
        for name in LATER_TABS {
            let note = widgets::hint(&format!(
                "The {name} page comes in a later step of the port."
            ));
            let (l, t, r, b) = widgets::PAGE_MARGINS;
            note.set_margin_start(l);
            note.set_margin_top(t);
            note.set_margin_end(r);
            note.set_margin_bottom(b);
            let scroll = scrollable(&note);
            tabs.append_page(&scroll, Some(&gtk::Label::new(Some(name))));
            index_pages.push((*name, scroll.clone().upcast::<gtk::Widget>(), Vec::new()));
            scrolls.push(scroll);
        }
        let index = settings_search::index_tabs(&index_pages);

        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&gtk::Label::new(Some("Find:")));
        let find = gtk::SearchEntry::new();
        find.add_css_class("field");
        find.set_hexpand(true);
        find.set_placeholder_text(Some(
            "a setting's name - \"quota\", \"cookies\", \"thumbnail\"\u{2026}",
        ));
        row.append(&find);
        column.append(&row);

        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::None);
        results.add_css_class("pick-list");
        let results_scroll = gtk::ScrolledWindow::new();
        results_scroll.set_child(Some(&results));
        results_scroll.set_max_content_height(150);
        results_scroll.set_propagate_natural_height(true);
        results_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        results_scroll.add_css_class("field");
        results_scroll.set_visible(false);
        column.append(&results_scroll);
        column.append(&tabs);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
        let cancel_button = widgets::pill_button("Cancel", false, false);
        let ok_button = widgets::pill_button("OK", true, false);
        buttons.append(&cancel_button);
        buttons.append(&ok_button);
        column.append(&buttons);
        window.set_child(Some(&column));
        window.set_default_widget(Some(&ok_button));

        let dialog = Rc::new(SettingsDialog {
            window: window.clone(),
            find: find.clone(),
            results,
            results_scroll,
            tabs,
            ok_button: ok_button.clone(),
            cancel_button: cancel_button.clone(),
            forms,
            scrolls,
            index,
            listed: RefCell::new(Vec::new()),
            da_snapshot: settings.deviantart_cookies.expose().to_owned(),
            lens_env: RefCell::new(BrowserEnv::from_process()),
            highlights: Rc::new(RefCell::new(HashMap::new())),
            next_token: Cell::new(0),
            main: main.clone(),
            prompter: main
                .s
                .prompter
                .over(&window)
                .unwrap_or_else(|| main.s.prompter.clone()),
        });
        dialog.insensitive_network_buttons();
        dialog.wire_controls();
        {
            let w = window.clone();
            cancel_button.connect_clicked(move |_| w.close());
        }
        {
            let d = Rc::downgrade(&dialog);
            ok_button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    d.accept();
                }
            });
        }
        {
            let d = Rc::downgrade(&dialog);
            find.connect_changed(move |e| {
                if let Some(d) = d.upgrade() {
                    d.on_search(&e.text());
                }
            });
        }
        {
            let d = Rc::downgrade(&dialog);
            dialog.results.connect_row_activated(move |_, row| {
                if let Some(d) = d.upgrade() {
                    d.jump_to_setting(row.index() as usize);
                }
            });
        }
        {
            let d = Rc::downgrade(&dialog);
            window.connect_close_request(move |_| {
                if let Some(d) = d.upgrade() {
                    d.main.s.settings_dialog.borrow_mut().take();
                }
                glib::Propagation::Proceed
            });
        }
        {
            // Escape is Cancel
            let controller = gtk::ShortcutController::new();
            let w = window.clone();
            controller.add_shortcut(gtk::Shortcut::new(
                Some(gtk::KeyvalTrigger::new(
                    gtk::gdk::Key::Escape,
                    gtk::gdk::ModifierType::empty(),
                )),
                Some(gtk::CallbackAction::new(move |_, _| {
                    w.close();
                    glib::Propagation::Stop
                })),
            ));
            window.add_controller(controller);
        }
        dialog
    }

    /// The handlers `__init__` connects: the pipeline line, the Lens warning, Browse.
    fn wire_controls(self: &Rc<Self>) {
        for id in settings_values::PIPELINE_CONTROLS {
            for form in &self.forms {
                let d = Rc::downgrade(self);
                form.on_change(id, move || {
                    if let Some(d) = d.upgrade() {
                        d.update_pipeline();
                    }
                });
            }
        }
        for form in &self.forms {
            if let Some(lens) = form.check("enable_google_lens") {
                let d = Rc::downgrade(self);
                lens.connect_toggled(move |c| {
                    if let Some(d) = d.upgrade() {
                        d.warn_if_lens_cannot_run(c.is_active());
                    }
                });
            }
            if let (Some(browse), Some(path)) =
                (form.button("button.browse"), form.entry_of("log_path"))
            {
                let d = Rc::downgrade(self);
                let path = path.clone();
                browse.connect_clicked(move |_| {
                    let Some(d) = d.upgrade() else { return };
                    let path = path.clone();
                    d.prompter.save_file(
                        "Choose log file",
                        &path.text(),
                        "",
                        Box::new(move |picked| {
                            if let Some(p) = picked {
                                path.set_text(&p.to_string_lossy());
                            }
                        }),
                    );
                });
            }
        }
    }

    /// `_update_engine_pipeline`.
    fn update_pipeline(&self) {
        let Some(live) = self.forms.iter().find_map(|f| f.live("engine_pipeline")) else {
            return;
        };
        let base = self.main.s.settings.lock().unwrap().clone();
        live.set_text(&settings_values::pipeline_text(&base, &self.values()));
    }

    /// `_warn_if_lens_cannot_run`: an early warning, not a veto; the box stays ticked.
    fn warn_if_lens_cannot_run(&self, enabled: bool) {
        if !enabled {
            return;
        }
        let env = self.lens_env.borrow().clone();
        let status = check_dependencies(&env);
        if status.missing.is_none() {
            return;
        }
        tracing::info!(
            "Google Lens was enabled but {:?} is not installed",
            status.missing
        );
        let why = if env.appimage {
            "the browser bundled with this AppImage could not be found."
        } else {
            "no Chromium-based browser could be found."
        };
        self.prompter.message_detailed(
            Kind::Warning,
            "Google Lens is not installed yet",
            &format!(
                "Google Lens is on, but it cannot run yet: {why}\n\nSearches will carry on with the other engines and Lens will be skipped until this is fixed."
            ),
            &status.message,
            Buttons::Ok,
            Box::new(|_| {}),
        );
    }

    /// The Test buttons and Fetch from Hydrus (`_test_*`, `_fetch_tag_services`): network code, 6.h-2's.
    fn insensitive_network_buttons(&self) {
        for form in &self.forms {
            for control in form.controls.values() {
                if let Control::Button(b) = control {
                    if b.label().as_deref() != Some("Browse\u{2026}") {
                        b.set_sensitive(false);
                    }
                }
            }
        }
    }

    // -- the controls -------------------------------------------------------------------------

    /// Every control's value as it reads now.
    pub fn values(&self) -> Values {
        let mut all = Values::default();
        for form in &self.forms {
            all.0.extend(form.read().0);
        }
        all
    }

    /// Sets a control as a click or a keystroke would. Test driver and tests only.
    pub fn set_value(&self, id: &str, value: &Value) {
        let mut one = Values::default();
        one.set(id, value.clone());
        for form in &self.forms {
            if form.controls.contains_key(id) {
                form.write(&one);
            }
        }
    }

    /// The control with `id`, wherever it is.
    pub fn control(&self, id: &str) -> Option<&Control> {
        self.forms.iter().find_map(|f| f.controls.get(id))
    }

    /// A live label's text (the pipeline line, a Test answer).
    pub fn live_text(&self, id: &str) -> String {
        self.forms
            .iter()
            .find_map(|f| f.live(id))
            .map(|l| l.text().to_string())
            .unwrap_or_default()
    }

    /// A line edit's text.
    pub fn text_of(&self, id: &str) -> String {
        self.forms
            .iter()
            .find_map(|f| f.entry_of(id))
            .map(|e| e.text().to_string())
            .unwrap_or_default()
    }

    /// Clicks the button `id`. Test driver and tests only.
    pub fn press(&self, id: &str) {
        if let Some(b) = self.forms.iter().find_map(|f| f.button(id)) {
            b.emit_clicked();
        }
    }

    /// `QTabWidget.tabText`, in order.
    pub fn tab_names(&self) -> Vec<String> {
        PAGES
            .iter()
            .map(|(n, _)| (*n).to_owned())
            .chain(LATER_TABS.iter().map(|n| (*n).to_owned()))
            .collect()
    }

    pub fn current_tab(&self) -> usize {
        self.tabs.current_page().unwrap_or(0) as usize
    }

    // -- finding a setting --------------------------------------------------------------------

    fn on_search(&self, text: &str) {
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        let mut listed = self.listed.borrow_mut();
        listed.clear();
        let found = settings_search::matches(&self.index, text);
        let blank = hatate_core::pycompat::strip(text).is_empty();
        if found.is_empty() {
            self.results_scroll.set_visible(!blank);
            if !blank {
                let row = gtk::ListBoxRow::new();
                row.set_activatable(false);
                row.set_selectable(false);
                row.set_child(Some(&label_row("Nothing matches that.")));
                self.results.append(&row);
                listed.push(None);
            }
            return;
        }
        for entry in found.into_iter().take(MAX_RESULTS) {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&label_row(&result_text(entry))));
            self.results.append(&row);
            listed.push(Some((entry.tab_index, entry.widget.clone())));
        }
        self.results_scroll.set_visible(true);
    }

    /// The results list as it reads now.
    pub fn result_texts(&self) -> Vec<String> {
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

    pub fn results_visible(&self) -> bool {
        self.results_scroll.is_visible()
    }

    /// Whether the results row at `i` is one a click would act on.
    pub fn result_is_activatable(&self, i: usize) -> bool {
        self.listed.borrow().get(i).is_some_and(Option::is_some)
    }

    /// `_jump_to_setting`: opens the setting's tab, scrolls to it, marks it for a moment.
    pub fn jump_to_setting(&self, i: usize) {
        let Some(Some((tab, widget))) = self.listed.borrow().get(i).cloned() else {
            return;
        };
        self.tabs.set_current_page(Some(tab as u32));
        if let Some(scroll) = self.scrolls.get(tab) {
            ensure_visible(scroll, &widget, 40.0);
            // the page may not have been laid out yet; try again once it has
            let scroll = scroll.clone();
            let w = widget.clone();
            glib::idle_add_local_once(move || ensure_visible(&scroll, &w, 40.0));
        }
        self.highlight(&widget);
    }

    /// `settings_search.highlight`: marks a widget, then puts it back after [`HIGHLIGHT_MS`]. A
    /// token, bumped on every call, makes only the latest pending revert act.
    pub fn highlight(&self, widget: &gtk::Widget) {
        let key = widget.as_ptr() as usize;
        let token = self.next_token.get() + 1;
        self.next_token.set(token);
        self.highlights.borrow_mut().insert(key, token);
        widget.add_css_class("found");
        let weak = widget.downgrade();
        let highlights = self.highlights.clone();
        glib::timeout_add_local_once(Duration::from_millis(HIGHLIGHT_MS), move || {
            let Some(widget) = weak.upgrade() else { return };
            if highlights.borrow().get(&key) == Some(&token) {
                highlights.borrow_mut().remove(&key);
                widget.remove_css_class("found");
            }
        });
    }

    // -- OK ------------------------------------------------------------------------------------

    /// `accept()` then `MainWindow.action_open_settings`'s `apply_to_settings()` and the refreshes
    /// after it.
    pub fn accept(&self) {
        let values = self.values();
        self.window.close();
        self.main.apply_settings_dialog(&values, &self.da_snapshot);
    }

    pub fn click_ok(&self) {
        self.ok_button.emit_clicked();
    }

    pub fn click_cancel(&self) {
        self.cancel_button.emit_clicked();
    }
}

/// `"{tab_name}  ›  {name}"`.
pub fn result_text(e: &SettingEntry<gtk::Widget>) -> String {
    format!("{}  \u{203a}  {}", e.tab_name, e.name)
}

fn label_row(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_margin_start(8);
    l.set_margin_end(8);
    l.set_margin_top(4);
    l.set_margin_bottom(4);
    l
}

/// `_scrollable`: every tab scrolls vertically, never sideways.
fn scrollable(page: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let area = gtk::ScrolledWindow::new();
    area.set_child(Some(page));
    // Qt's `QScrollArea` scrolls both ways, and Python lets the window shrink to 480 wide: a
    // narrow window scrolls the page sideways, it does not force the window wide.
    area.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    area.set_vexpand(true);
    area
}

/// `QScrollArea.ensureWidgetVisible(widget, 0, ymargin)`: the least scroll that shows the whole
/// widget with `margin` pixels around it.
fn ensure_visible(scroll: &gtk::ScrolledWindow, widget: &gtk::Widget, margin: f64) {
    let Some(content) = scroll.child() else {
        return;
    };
    let content = content
        .downcast_ref::<gtk::Viewport>()
        .and_then(|v| v.child())
        .unwrap_or(content);
    let Some(bounds) = widget.compute_bounds(&content) else {
        return;
    };
    let adj = scroll.vadjustment();
    let (top, bottom) = (
        bounds.y() as f64 - margin,
        (bounds.y() + bounds.height()) as f64 + margin,
    );
    let (value, page) = (adj.value(), adj.page_size());
    if top < value {
        adj.set_value(top);
    } else if bottom > value + page {
        adj.set_value(bottom - page);
    }
}

/// `SettingsDialog.apply_to_settings` and what `action_open_settings` does after it.
impl MainWindow {
    pub fn action_open_settings(&self) {
        if let Some(open) = self.s.settings_dialog.borrow().as_ref() {
            open.window.present();
            return;
        }
        let dialog = SettingsDialog::build(self);
        dialog.window.present();
        *self.s.settings_dialog.borrow_mut() = Some(dialog);
    }

    pub fn apply_settings_dialog(&self, values: &Values, da_snapshot: &str) {
        {
            let mut s = self.s.settings.lock().unwrap();
            settings_values::apply(values, da_snapshot, &mut s);
            tracing::info!("Settings updated from Preferences dialog");
            if let Err(e) = s.save(&self.s.paths) {
                tracing::warn!("could not save the settings: {e}");
            }
        }
        // a changed autosave interval (or an on/off flip) now, not at the next launch
        self.apply_autosave_settings();
        self.install_review_shortcuts();
        self.refresh_review_strip();
        // the Activity page states the engine line-up, which this dialog is what changes
        self.refresh_engines();
    }
}

/// The settings this dialog edits, for a test that wants the file after OK.
pub fn saved(main: &MainWindow) -> Settings {
    main.s.settings.lock().unwrap().clone()
}

/// The page nodes, for tests that walk the spec.
pub fn pages() -> &'static [(&'static str, Node)] {
    PAGES
}
