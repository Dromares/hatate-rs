//! The main window (`gui/main_window.py` `MainWindow` at `76c85c4`): the frame, the menubar and
//! its actions, the Queue page's shell (title, banners, drop zone), and the state the later
//! tickets hang their screens on. The session, banner and close logic is in
//! [`crate::session_ops`] and [`crate::banners`], on the same type.
//!
//! GTK's `PopoverMenuBar` takes a `gio::Menu` and the `win.*` actions it names. An action a later
//! ticket owns is registered **disabled** (see [`crate::menu`]), so the menu reads like Python's
//! and nothing in it can be clicked into an empty handler.

use crate::dialogs::{AdwPrompter, Prompter};
use crate::logview::{LogSource, RingLogs};
use crate::menu::{self, Owner};
use crate::queue_view::QueueView;
use crate::shell::{self, Shell};
use crate::theme;
use crate::tokens::Scheme;
use crate::widgets;
use crate::worker::{Slot, Workers};
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::applog::LogRing;
use hatate_core::config::Settings;
use hatate_core::crash_marker::RunSession;
use hatate_core::paths::Paths;
use hatate_core::queue::QueueModel;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// The window title says what this is (work in progress, not a replacement). Python's
/// `setWindowTitle` (`main_window.py:422`) reads "Hatate-linux - IQDB/SauceNAO tagger for Hydrus".
pub const WINDOW_TITLE: &str = "Hatate (work in progress Rust rewrite)";

/// The window's minimum size (design gate 2.5, DAN-1974 item 2): the width and height below which a
/// frame of the Queue or the Review (right rail in) would clip, **measured** on the CI toolkit
/// (GTK 4.14 / libadwaita 1.5) in both schemes by the `minimum_size_*` scenarios of
/// `tests/smoke.rs`, which fail if any state needs more. `docs/gtk-shell.md` has the table.
pub const MIN_WIDTH: i32 = 1010;
pub const MIN_HEIGHT: i32 = 790;

/// `workers/missing_file_worker.MissingFileWorker.run`: the subset of `paths` that no longer
/// exist. The shipped window uses [`default_missing_files`] (`hatate_core::missing_files`, 6.b);
/// a test gives its own.
pub type MissingFiles = Arc<dyn Fn(&[String], &AtomicBool) -> HashSet<String> + Send + Sync>;

/// The check over `hatate_core::missing_files::check_session_files`, which lists each directory
/// once rather than stat-ing each file.
pub fn default_missing_files() -> MissingFiles {
    Arc::new(|paths, stop| {
        hatate_core::missing_files::check_session_files(paths, &|| {
            stop.load(std::sync::atomic::Ordering::Relaxed)
        })
    })
}

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
    pub missing_files: MissingFiles,
    /// `QDesktopServices.openUrl(QUrl.fromLocalFile(path))`.
    pub open_path: Rc<dyn Fn(&Path)>,
    /// The log viewer's source.
    pub logs: Option<Rc<dyn LogSource>>,
    /// The ring the log viewer reads when `logs` is not given; `main` fills it from the log layer.
    pub log_ring: LogRing,
    /// This run's crash marker; consumed at a clean close (`mark_clean_shutdown`).
    pub run_session: Option<RunSession>,
}

impl Env {
    pub fn new(paths: Paths, unclean_shutdown: bool) -> Env {
        Env {
            paths,
            unclean_shutdown,
            prompter: None,
            clock: Rc::new(hatate_core::session::now_epoch),
            missing_files: default_missing_files(),
            open_path: Rc::new(open_with_desktop),
            logs: None,
            log_ring: LogRing::new(),
            run_session: None,
        }
    }
}

/// Hands a file or folder to the desktop's default handler.
fn open_with_desktop(path: &Path) {
    let file = gio::File::for_path(path);
    gtk::FileLauncher::new(Some(&file)).launch(None::<&gtk::Window>, gio::Cancellable::NONE, |r| {
        if let Err(e) = r {
            tracing::warn!("could not open a file with the desktop: {e}");
        }
    });
}

/// The window's state: what Python keeps on `self`.
pub struct State {
    pub paths: Paths,
    pub settings: Arc<Mutex<Settings>>,
    pub queue: Rc<RefCell<QueueModel>>,
    /// The Queue table (`self.table`, `self.table_model`).
    pub queue_view: Rc<QueueView>,
    pub workers: Workers,
    pub file_hash_worker: Slot,
    /// The thumbnail scheduler (`thumbnail_worker`, `_thumbs_in_flight`).
    pub thumbs: crate::thumbnails::Scheduler,
    pub missing_file_worker: Slot,
    pub autosave_worker: Slot,
    /// Set while an autosave write is on the pool, so close can wait for it.
    pub autosave_inflight: Arc<AtomicBool>,
    pub autosave_timer: RefCell<Option<glib::SourceId>>,
    pub prompter: Rc<dyn Prompter>,
    pub clock: Rc<dyn Fn() -> f64>,
    pub missing_files: MissingFiles,
    pub open_path: Rc<dyn Fn(&Path)>,
    pub logs: Rc<dyn LogSource>,
    pub run_session: RefCell<Option<RunSession>>,
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
    /// `self._pending_import_paths`.
    pub pending_import_paths: RefCell<Vec<PathBuf>>,
    /// `self._session_restore_status`: what the restore had to say, for the quota pause to join.
    pub session_restore_status: RefCell<Option<String>>,
    /// `self._quota_paused`.
    pub quota_paused: Cell<bool>,
    /// The run strip's gauge as Qt keeps it: `(progress_bar.value(), progress_bar.maximum())`.
    pub run_progress: Cell<(i64, i64)>,
    /// `self._run_active`: a search is under way (6.e sets it).
    pub run_active: Cell<bool>,
    /// `self._run_counted`: the strip has shown a run's count, which then stays on screen.
    pub run_counted: Cell<bool>,
    pub scheme: Cell<Scheme>,
    pub win: adw::ApplicationWindow,
    /// The dialogs that are open (the log viewer), so a test can find them.
    pub log_viewer: RefCell<Option<Rc<crate::logview::LogViewer>>>,
}

/// The window and its state.
#[derive(Clone)]
pub struct MainWindow {
    pub win: adw::ApplicationWindow,
    pub s: Rc<State>,
}

/// The weak handle the chrome's callbacks hold, so they cannot keep the window alive.
#[derive(Clone)]
pub struct MainWindowWeak {
    win: glib::WeakRef<adw::ApplicationWindow>,
    s: std::rc::Weak<State>,
}

impl MainWindowWeak {
    pub fn upgrade(&self) -> Option<MainWindow> {
        Some(MainWindow {
            win: self.win.upgrade()?,
            s: self.s.upgrade()?,
        })
    }
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

/// `_restore_table_header_state`: applies the saved column layout, or resets the settings key
/// the way Python does when the value cannot be used (the Qt blob a Python build wrote included).
fn restore_table_header_state(settings: &Mutex<Settings>, view: &QueueView) {
    use hatate_core::table_cells::{HeaderRestore, restore_header_state};
    let mut s = settings.lock().unwrap();
    match restore_header_state(&s.table_header_state, s.table_header_columns) {
        HeaderRestore::NothingSaved => {}
        HeaderRestore::WrongColumnCount => {
            tracing::info!(
                "Discarding saved column layout: saved with {} column(s), this build has {}",
                s.table_header_columns,
                hatate_core::table_cells::COLUMNS.len()
            );
            s.table_header_state.clear();
            s.table_header_columns = 0;
        }
        HeaderRestore::Unreadable => {
            tracing::warn!("Could not restore table column layout (invalid saved state)");
            s.table_header_state.clear();
        }
        HeaderRestore::Restored { sections, repaired } => {
            view.apply_header_sections(&sections);
            if !repaired.is_empty() {
                tracing::info!("Repaired restored column layout: {}", repaired.join(", "));
            }
        }
    }
}

impl MainWindow {
    pub fn new(app: &adw::Application, env: Env) -> MainWindow {
        let loaded = Settings::load(&env.paths);
        let settings = Arc::new(Mutex::new(loaded.settings));
        let queue = Rc::new(RefCell::new(QueueModel::new(Vec::new(), settings.clone())));
        let queue_view = QueueView::new(queue.clone());
        restore_table_header_state(&settings, &queue_view);

        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title(WINDOW_TITLE)
            .default_width(1100)
            .default_height(650)
            .build();
        win.set_size_request(MIN_WIDTH, MIN_HEIGHT);
        let prompter: Rc<dyn Prompter> = env
            .prompter
            .clone()
            .unwrap_or_else(|| Rc::new(AdwPrompter::new(&win)));
        let logs: Rc<dyn LogSource> = env
            .logs
            .clone()
            .unwrap_or_else(|| Rc::new(RingLogs::new(&env.paths, env.log_ring.clone())));

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
        queue_stack.add_named(&queue_view.root, Some("rows"));
        let drop_zone = build_drop_zone();
        queue_stack.add_named(&drop_zone, Some("empty"));
        queue_stack.set_visible_child_name("empty");
        queue_page.append(&queue_stack);

        let handle: Rc<OnceCell<MainWindowWeak>> = Rc::new(OnceCell::new());
        let call = |handle: &Rc<OnceCell<MainWindowWeak>>, f: fn(&MainWindow)| {
            let handle = handle.clone();
            move || {
                if let Some(w) = handle.get().and_then(MainWindowWeak::upgrade) {
                    f(&w);
                }
            }
        };
        let mode_handle = handle.clone();
        let shell = Shell::new(
            &[
                ("queue", queue_page.clone().upcast()),
                ("review", review_frame()),
            ],
            move |key| {
                if let Some(w) = mode_handle.get().and_then(MainWindowWeak::upgrade) {
                    if w.s.shell.current_mode() != key {
                        w.set_mode(key);
                    }
                }
            },
            call(&handle, |w| w.activate_action("toggle-search")),
            call(&handle, |w| w.activate_action("preferences")),
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
            queue,
            queue_view,
            workers: Workers::new(),
            file_hash_worker: Slot::new(),
            thumbs: Default::default(),
            missing_file_worker: Slot::new(),
            autosave_worker: Slot::new(),
            autosave_inflight: Arc::new(AtomicBool::new(false)),
            autosave_timer: RefCell::new(None),
            prompter,
            clock: env.clock.clone(),
            missing_files: env.missing_files.clone(),
            open_path: env.open_path.clone(),
            logs,
            run_session: RefCell::new(env.run_session),
            shell,
            queue_stack,
            drop_zone,
            run_banner,
            quota_banner,
            nothing_survived: RefCell::new(None),
            unclean_shutdown: env.unclean_shutdown,
            closing: Cell::new(false),
            pending_import_paths: RefCell::new(Vec::new()),
            session_restore_status: RefCell::new(None),
            quota_paused: Cell::new(false),
            run_progress: Cell::new((0, 0)),
            run_active: Cell::new(false),
            run_counted: Cell::new(false),
            scheme: Cell::new(Scheme::Dark),
            win: win.clone(),
            log_viewer: RefCell::new(None),
        });
        let window = MainWindow {
            win: win.clone(),
            s: state,
        };
        let _ = handle.set(window.downgrade());
        window.install_actions(app);
        window.wire_thumbnails();
        window.wire_table_context_menu();
        window.wire_banners();
        window.apply_theme(&theme_setting(&window.s));
        window.refresh_queue_empty_state();
        window.connect_close();
        window.startup();
        window
    }

    /// Rows ask for their thumbnail as they are drawn.
    fn wire_thumbnails(&self) {
        let w = self.downgrade();
        self.s.queue_view.connect_row_bound(move |id| {
            if let Some(w) = w.upgrade() {
                w.on_row_drawn(id);
            }
        });
    }

    pub fn downgrade(&self) -> MainWindowWeak {
        MainWindowWeak {
            win: self.win.downgrade(),
            s: Rc::downgrade(&self.s),
        }
    }

    /// `activate_action`: what a menu item or a shortcut does.
    pub fn activate_action(&self, name: &str) {
        let _ = gtk::prelude::WidgetExt::activate_action(&self.win, &format!("win.{name}"), None);
    }

    /// `action_set_theme(mode)`: switches the theme now, without a restart.
    pub fn set_theme(&self, mode: &str) {
        if theme::Mode::parse(mode).is_none() {
            return;
        }
        {
            let mut s = self.s.settings.lock().unwrap();
            s.theme = mode.to_owned();
            if let Err(e) = s.save(&self.s.paths) {
                tracing::warn!("could not save the settings: {e}");
            }
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

    /// `ghost_glyph_for(key)`: the Queue swaps in the Empty glyph while it shows its drop zone
    /// (stack index 1). The "Nothing survived." page is a third page, so it keeps the list's
    /// glyph, as Python does.
    pub fn ghost_glyph_for(&self, key: &str) -> String {
        if key == "queue" && self.s.queue_stack.visible_child_name().as_deref() == Some("empty") {
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
            // Something to show: the notice has done its job, and a later emptied list is an
            // ordinary empty one.
            self.dismiss_nothing_survived();
            self.s.queue_stack.set_visible_child_name("rows");
        } else {
            let survived = self.s.nothing_survived.borrow();
            match survived.as_ref() {
                Some(p) => self.s.queue_stack.set_visible_child(&p.root),
                None => self.s.queue_stack.set_visible_child_name("empty"),
            }
        }
        // The Empty page has its own watermark (G-07: 空 where the list is 力); only restyle it
        // while Queue is the page on show.
        if self.s.shell.current_mode() == "queue" {
            let ghost = self.ghost_glyph_for("queue");
            self.s.shell.root.set_glyph(&ghost);
        }
        self.s.queue_view.rebuild();
    }

    /// `status_label.setText`.
    pub fn say(&self, text: &str) {
        self.s.shell.say(text);
    }

    /// Registers every action in the menus: the working ones with their handler, the later
    /// tickets' ones disabled.
    fn install_actions(&self, app: &adw::Application) {
        for (name, owner) in menu::actions() {
            if matches!(name, "theme" | "site") {
                continue;
            }
            let action = gio::SimpleAction::new(name, None);
            match owner {
                Owner::Later(_) => action.set_enabled(false),
                Owner::Shell => self.connect_shell_action(&action, name),
            }
            self.win.add_action(&action);
        }
        // Settings > Theme: a stateful radio action holding the stored mode.
        let current = theme_setting(&self.s);
        let theme_action = gio::SimpleAction::new_stateful(
            "theme",
            Some(glib::VariantTy::STRING),
            &current.to_variant(),
        );
        {
            let w = self.downgrade();
            theme_action.connect_activate(move |_, param| {
                if let (Some(w), Some(mode)) = (w.upgrade(), param.and_then(|p| p.get::<String>()))
                {
                    w.set_theme(&mode);
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
        // `toggle-search` is the Start Search button's action (6.e wires it).
        let toggle = gio::SimpleAction::new("toggle-search", None);
        toggle.set_enabled(false); // DEFERRED(6.e)
        self.win.add_action(&toggle);

        for (action, accel) in menu::accelerators() {
            app.set_accels_for_action(&action, &[accel]);
        }
        self.s
            .shell
            .search_toggle_btn
            .set_action_name(Some("win.toggle-search"));
        self.s
            .shell
            .settings_btn
            .set_action_name(Some("win.preferences"));
    }

    fn connect_shell_action(&self, action: &gio::SimpleAction, name: &str) {
        let w = self.downgrade();
        let name = name.to_owned();
        action.connect_activate(move |_, _| {
            let Some(w) = w.upgrade() else { return };
            match name.as_str() {
                "add-files" => w.action_add_files(),
                "add-folder" => w.action_add_folder(),
                "save-session-as" => w.action_save_session_as(),
                "open-session" => w.action_open_session(),
                "clear-session" => w.action_clear_session(),
                "open-log" => w.action_open_log(),
                "view-logs" => w.action_view_logs(),
                "export-results" => w.action_export_results(None),
                "write-tag-files" => w.action_write_tag_files(None),
                "clear-search-cache" => w.action_clear_search_cache(),
                "quit" => w.win.close(),
                other => tracing::warn!("no handler for action {other}"),
            }
        });
    }

    /// The banner buttons Python wires in `_build_queue_page`. 6.e wires Resume Queue and the two
    /// quota actions; the ones that belong to the shell are wired here.
    fn wire_banners(&self) {
        let w = self.downgrade();
        self.s.run_banner.crash_log.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.open_crash_log();
                }
            }
        });
        self.s.run_banner.discard.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.action_clear_session();
                }
            }
        });
        // DEFERRED(6.e): Resume Queue, "Review the waiting", "Continue with other engines".
        self.s.run_banner.resume.set_sensitive(false);
        self.s.quota_banner.review.set_sensitive(false);
        self.s.quota_banner.keep_going.set_sensitive(false);
        // The empty state's three buttons. Query Hydrus follows its (disabled) action.
    }
}

/// The stored `theme` setting.
fn theme_setting(s: &State) -> String {
    s.settings.lock().unwrap().theme.clone()
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
