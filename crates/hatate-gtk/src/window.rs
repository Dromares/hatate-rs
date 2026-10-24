//! The main window (`gui/main_window.py` `MainWindow` at `76c85c4`): the frame, the menubar and
//! its actions, the Queue page's shell (title, banners, drop zone), and the state the later
//! tickets hang their screens on.
//!
//! GTK's `PopoverMenuBar` takes a `gio::Menu` and the `win.*` actions it names. An action a later
//! ticket owns is registered **disabled** (see [`crate::menu`]), so the menu reads like Python's
//! and nothing in it can be clicked into an empty handler.

use crate::dialogs::{AdwPrompter, Prompter};
use crate::menu::{self, Owner};
use crate::shell::{self, Shell};
use crate::theme;
use crate::tokens::Scheme;
use crate::widgets;
use crate::worker::{Slot, Workers};
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::Settings;
use hatate_core::paths::Paths;
use hatate_core::queue::QueueModel;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// The window title says what this is (work in progress, not a replacement). `setWindowTitle` at
/// `main_window.py:422` reads "Hatate-linux - IQDB/SauceNAO tagger for Hydrus".
pub const WINDOW_TITLE: &str = "Hatate (work in progress Rust rewrite)";

/// Everything the window is given, so a test can stand in for each of them. The shipped binary
/// builds this from the process environment and nothing else; no field is read from a flag or a
/// variable that redirects a host.
pub struct Env {
    pub paths: Paths,
    /// `check_unclean_shutdown()` as `main.py` read it before this run's marker went down.
    pub unclean_shutdown: bool,
    /// How questions and file choosers are put to the user.
    pub prompter: Option<Rc<dyn Prompter>>,
    /// Seconds since the epoch (`time.time()`).
    pub clock: Rc<dyn Fn() -> f64>,
}

impl Env {
    pub fn new(paths: Paths, unclean_shutdown: bool) -> Env {
        Env {
            paths,
            unclean_shutdown,
            prompter: None,
            clock: Rc::new(hatate_core::session::now_epoch),
        }
    }
}

/// The window's state: what Python keeps on `self`.
pub struct State {
    pub paths: Paths,
    pub settings: Arc<Mutex<Settings>>,
    pub queue: RefCell<QueueModel>,
    pub workers: Workers,
    pub file_hash_worker: Slot,
    pub missing_file_worker: Slot,
    pub prompter: Rc<dyn Prompter>,
    pub clock: Rc<dyn Fn() -> f64>,
    pub shell: Rc<Shell>,
    pub queue_stack: gtk::Stack,
    pub drop_zone: gtk::Overlay,
    pub run_banner: widgets::RunBanner,
    pub quota_banner: widgets::QuotaBanner,
    /// The Crashed "Nothing survived." variant of the empty state, while it is showing.
    pub nothing_survived: RefCell<Option<widgets::NothingSurvived>>,
    /// `self._unclean_shutdown`.
    pub unclean_shutdown: bool,
    /// `self._closing`: set at the top of close, so a worker finishing after it shows nothing.
    pub closing: Cell<bool>,
    pub scheme: Cell<Scheme>,
    pub win: adw::ApplicationWindow,
    /// A hook the Queue view (6.d) installs to learn the list changed.
    pub on_list_changed: RefCell<Option<Box<dyn Fn()>>>,
}

/// The window and its state.
pub struct MainWindow {
    pub win: adw::ApplicationWindow,
    pub s: Rc<State>,
}

/// Settings > Theme: the scheme to draw for the stored setting, asking the desktop for `system`.
fn scheme_for(setting: &str) -> Scheme {
    let system = if matches!(setting, "dark" | "light") {
        None
    } else {
        theme::system_prefers_dark()
    };
    theme::resolve_scheme(setting, system)
}

impl MainWindow {
    pub fn new(app: &adw::Application, env: Env) -> MainWindow {
        let loaded = Settings::load(&env.paths);
        let settings = Arc::new(Mutex::new(loaded.settings));
        let queue = QueueModel::new(Vec::new(), settings.clone());

        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title(WINDOW_TITLE)
            .default_width(1100)
            .default_height(650)
            .build();
        let prompter: Rc<dyn Prompter> = env
            .prompter
            .clone()
            .unwrap_or_else(|| Rc::new(AdwPrompter::new(&win)));

        // -- the Queue page: title, the two banners, and the list or the empty state
        let queue_page = widgets::card(None, false);
        queue_page.set_margin_start(0);
        queue_page.set_margin_end(0);
        queue_page.set_margin_top(0);
        queue_page.set_margin_bottom(0);
        queue_page.set_spacing(widgets::QUEUE_BODY_GAP);
        queue_page.append(&widgets::screen_title("Queue."));
        let run_banner = widgets::run_banner();
        run_banner.root.set_visible(false);
        queue_page.append(&run_banner.root);
        let quota_banner = widgets::quota_pause_banner();
        quota_banner.root.set_visible(false);
        queue_page.append(&quota_banner.root);

        let queue_stack = gtk::Stack::new();
        queue_stack.set_vexpand(true);
        let rows = placeholder_rows();
        queue_stack.add_named(&rows, Some("rows"));
        let drop_zone = build_drop_zone();
        queue_stack.add_named(&drop_zone, Some("empty"));
        queue_stack.set_visible_child_name("empty");
        queue_page.append(&queue_stack);

        let review_page = review_frame();

        let shell_cell: Rc<RefCell<Option<Rc<MainWindowWeak>>>> = Rc::new(RefCell::new(None));
        let (mode_cell, search_cell, settings_cell) =
            (shell_cell.clone(), shell_cell.clone(), shell_cell.clone());
        let shell = Shell::new(
            &[
                ("queue", queue_page.clone().upcast()),
                ("review", review_page.upcast()),
            ],
            move |key| {
                if let Some(w) = mode_cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    w.set_mode(key);
                }
            },
            move || {
                if let Some(w) = search_cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    w.activate_action("toggle-search");
                }
            },
            move || {
                if let Some(w) = settings_cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    w.activate_action("preferences");
                }
            },
        );

        let menubar = gtk::PopoverMenuBar::from_model(Some(&menu::model()));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&menubar);
        shell.root.root.set_vexpand(true);
        content.append(&shell.root.root);
        content.append(&shell.status_bar);
        win.set_content(Some(&content));

        let state = Rc::new(State {
            paths: env.paths.clone(),
            settings: settings.clone(),
            queue: RefCell::new(queue),
            workers: Workers::new(),
            file_hash_worker: Slot::new(),
            missing_file_worker: Slot::new(),
            prompter,
            clock: env.clock.clone(),
            shell,
            queue_stack,
            drop_zone,
            run_banner,
            quota_banner,
            nothing_survived: RefCell::new(None),
            unclean_shutdown: env.unclean_shutdown,
            closing: Cell::new(false),
            scheme: Cell::new(Scheme::Dark),
            win: win.clone(),
            on_list_changed: RefCell::new(None),
        });
        let window = MainWindow {
            win: win.clone(),
            s: state,
        };
        *shell_cell.borrow_mut() = Some(Rc::new(MainWindowWeak {
            win: win.downgrade(),
            s: Rc::downgrade(&window.s),
        }));
        window.install_actions(app);
        window.apply_theme(&scheme_name(&window.s));
        window.refresh_queue_empty_state();
        window
    }

    /// `action_set_theme(mode)`: switches the theme now, without a restart.
    pub fn set_theme(&self, mode: &str) {
        if theme::Mode::parse(mode).is_none() {
            return;
        }
        {
            let mut s = self.s.settings.lock().unwrap();
            s.theme = mode.to_owned();
            let _ = s.save(&self.s.paths);
        }
        self.apply_theme(mode);
        if let Some(action) = self.win.lookup_action("theme") {
            action.change_state(&mode.to_variant());
        }
        tracing::debug!("Theme set to {mode}");
    }

    fn apply_theme(&self, mode: &str) {
        let scheme = scheme_for(mode);
        self.s.scheme.set(scheme);
        theme::install(scheme);
    }

    /// `set_mode(key)`.
    pub fn set_mode(&self, key: &str) {
        let ghost = self.ghost_glyph_for(key);
        if self.s.shell.show_mode(key, &ghost) {
            self.on_mode_changed(key);
        }
    }

    /// `ghost_glyph_for(key)`: the Queue swaps in the Empty glyph while it shows its drop zone.
    pub fn ghost_glyph_for(&self, key: &str) -> String {
        if key == "queue" && self.s.queue.borrow().is_empty() {
            return shell::EMPTY_GHOST_GLYPH.to_owned();
        }
        shell::ghost_glyph(key).to_owned()
    }

    /// `_on_mode_changed(key)`: a hook for the views (Review, 6.f).
    fn on_mode_changed(&self, _key: &str) {}

    /// `_refresh_queue_empty_state()`: an empty list shows the drop zone instead of an empty grid.
    /// Keyed on the list and not on how many rows are visible: a filter that matches nothing still
    /// has a list behind it.
    pub fn refresh_queue_empty_state(&self) {
        let has_rows = !self.s.queue.borrow().is_empty();
        if has_rows {
            self.dismiss_nothing_survived();
            self.s.queue_stack.set_visible_child_name("rows");
        } else {
            let survived = self.s.nothing_survived.borrow();
            match survived.as_ref() {
                Some(p) => {
                    self.s.queue_stack.set_visible_child(&p.root);
                }
                None => self.s.queue_stack.set_visible_child_name("empty"),
            }
        }
        // The Empty page has its own watermark (G-07: 空 where the list is 力); only restyle it
        // while Queue is the page on show.
        if self.s.shell.current_mode() == "queue" {
            let ghost = self.ghost_glyph_for("queue");
            self.s.shell.root.set_glyph(&ghost);
        }
    }

    /// `_dismiss_nothing_survived`: back to the ordinary drop zone.
    pub fn dismiss_nothing_survived(&self) {
        let Some(panel) = self.s.nothing_survived.borrow_mut().take() else {
            return;
        };
        self.s.queue_stack.remove(&panel.root);
        self.s.shell.run_note_label.set_text("");
        self.refresh_queue_empty_state();
    }

    /// Registers every action in the menus: the working ones with their handler, the later
    /// tickets' ones disabled.
    fn install_actions(&self, app: &adw::Application) {
        for (name, owner) in menu::actions() {
            if matches!(name, "theme" | "site") {
                continue;
            }
            let action = gio::SimpleAction::new(name, None);
            if let Owner::Later(_) = owner {
                action.set_enabled(false);
            }
            self.win.add_action(&action);
        }
        // Settings > Theme: a stateful radio action holding the stored mode.
        let current = self.s.settings.lock().unwrap().theme.clone();
        let theme_action = gio::SimpleAction::new_stateful(
            "theme",
            Some(glib::VariantTy::STRING),
            &current.to_variant(),
        );
        {
            let weak = Rc::downgrade(&self.s);
            let win = self.win.downgrade();
            theme_action.connect_activate(move |_, param| {
                let (Some(s), Some(win)) = (weak.upgrade(), win.upgrade()) else {
                    return;
                };
                if let Some(mode) = param.and_then(|p| p.get::<String>()) {
                    MainWindow { win, s }.set_theme(&mode);
                }
            });
        }
        self.win.add_action(&theme_action);
        // The Sites menu's per-site toggles: a stateful action with the site as its target.
        // DEFERRED(6.e): disabled until the Sites menu is wired.
        let site = gio::SimpleAction::new_stateful(
            "site",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        site.set_enabled(false);
        self.win.add_action(&site);

        for (action, accel) in menu::accelerators() {
            app.set_accels_for_action(&action, &[accel]);
        }
    }
}

/// The weak handle the chrome's callbacks hold, so they cannot keep the window alive.
pub struct MainWindowWeak {
    win: glib::WeakRef<adw::ApplicationWindow>,
    s: std::rc::Weak<State>,
}

impl MainWindowWeak {
    fn upgrade(&self) -> Option<MainWindow> {
        Some(MainWindow {
            win: self.win.upgrade()?,
            s: self.s.upgrade()?,
        })
    }
}

impl MainWindow {
    fn activate_action(&self, name: &str) {
        let _ = gtk::prelude::WidgetExt::activate_action(&self.win, &format!("win.{name}"), None);
    }
}

fn scheme_name(s: &State) -> String {
    s.settings.lock().unwrap().theme.clone()
}

/// The Queue's list page until 6.d replaces it with the `ColumnView`.
fn placeholder_rows() -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let label = widgets::muted("The list view arrives in 6.d.");
    label.set_halign(gtk::Align::Center);
    label.set_valign(gtk::Align::Center);
    label.set_vexpand(true);
    page.append(&label);
    page.upcast()
}

/// Review's frame at its minimum footprint, until 6.f fills it: two previews of at least
/// 280 x 260 (`review_view.py:308,321`) beside the 340 px right rail (`review.html`).
fn review_frame() -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_vexpand(true);
    for caption in ["Your copy", "The match"] {
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.set_size_request(280, 260);
        frame.set_hexpand(true);
        frame.append(&widgets::muted(caption));
        row.append(&widgets::image_frame(&frame));
    }
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 12);
    rail.set_size_request(340, -1);
    rail.add_css_class("review-rail");
    rail.append(&widgets::section_label("01 // Review"));
    row.append(&rail);
    let page = gtk::Box::new(gtk::Orientation::Vertical, widgets::QUEUE_BODY_GAP);
    page.append(&widgets::screen_title("Review."));
    page.append(&row);
    page.upcast()
}

/// `_build_drop_zone()`: what an empty Queue says instead of an empty grid.
fn build_drop_zone() -> gtk::Overlay {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("drop-zone");
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
    inner.set_margin_start(32);
    inner.set_margin_end(32);
    inner.set_margin_top(24);
    inner.set_margin_bottom(24);
    inner.set_vexpand(true);
    let section = widgets::section_label("01 // Queue");
    section.set_halign(gtk::Align::Start);
    inner.append(&section);

    let block = gtk::Box::new(gtk::Orientation::Vertical, 0);
    block.append(&widgets::display_title("Drop images here."));
    let blurb = widgets::empty_prose(
        "Or add them from a folder, or pull them out of a running Hydrus client. Anything \
         added here is searched on IQDB and SauceNAO, and what comes back is yours to review \
         before any of it reaches Hydrus.",
    );
    blurb.set_margin_top(10);
    block.append(&blurb);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(24);
    let add_files = widgets::pill_button("Add files\u{2026}", true, true);
    add_files.set_action_name(Some("win.add-files"));
    let add_folder = widgets::pill_button("Add folder\u{2026}", false, true);
    add_folder.set_action_name(Some("win.add-folder"));
    // DEFERRED(6.g): Query Hydrus. The button follows the (disabled) action.
    let query = widgets::pill_button("Query Hydrus\u{2026}", false, true);
    query.set_action_name(Some("win.query-hydrus"));
    for b in [&add_files, &add_folder, &query] {
        buttons.append(b);
    }
    block.append(&buttons);
    inner.append(&widgets::top_weighted(
        &block,
        widgets::DROP_ZONE_BELOW_WEIGHT,
    ));
    panel.append(&inner);
    widgets::brackets(&panel)
}

/// Whether `path` is a file this build can put in a log dialog (exists and is a regular file).
pub fn is_readable_file(path: &Path) -> bool {
    path.is_file()
}
