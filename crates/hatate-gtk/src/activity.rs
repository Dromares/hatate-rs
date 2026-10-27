//! Activity (`gui/activity_view.py` at `76c85c4`, `activity.html`): what the unattended half of the
//! app is doing. The engine line-up stated rather than remembered, and the log as a live pane that
//! tails itself while the page is on screen. It does not repeat the run strip (progress, the
//! estimate, the countdown, the quota, the sent count): those are above, in every mode.

use crate::text::round_half_even;
use crate::widgets;
use crate::window::MainWindow;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::Settings;
use hatate_core::engine_ids::{
    self, ALL_ENGINES, ASCII2D, GOOGLE_IMAGES, GOOGLE_LENS, IQDB3D, PAWCHIVE, PAWCHIVE_INDEX,
    PRIMARY_ENGINES, TRACEMOE, YANDEX, effective_primary, label_for, pipeline_summary,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// `TAIL_INTERVAL_MS`: while the page is on screen the log tails itself; off it the timer is
/// stopped.
pub const TAIL_INTERVAL: Duration = Duration::from_millis(2000);
/// `ENGINE_ROW_GAP`, `ENGINE_ACTIONS_GAP`, `ACTIVITY_BODY_GAP`.
pub const ENGINE_ROW_GAP: i32 = 6;
pub const ENGINE_ACTIONS_GAP: i32 = 8;
pub const ACTIVITY_BODY_GAP: i32 = 32;
/// What the log pane says before there is a line.
pub const NO_LOG_YET: &str = "(no log entries yet)";

/// `_engine_state(engine_id)`: `(on, note)` for one engine, read from the settings.
pub fn engine_state(s: &Settings, engine_id: &str) -> (bool, &'static str) {
    if PRIMARY_ENGINES.contains(&engine_id) {
        let primary = effective_primary(&s.primary_engine);
        if engine_id == primary {
            return (true, "primary");
        }
        return match s.secondary_engine_mode.as_str() {
            "disabled" => (false, "off"),
            "always" => (true, "always"),
            _ => (true, "fallback"),
        };
    }
    if engine_id == PAWCHIVE {
        // Never held back (`search_engine._engine_waves`).
        return if s.enable_pawchive {
            (true, "on")
        } else {
            (false, "off")
        };
    }
    if engine_id == PAWCHIVE_INDEX {
        return if s.enable_pawchive_index {
            (true, "on")
        } else {
            (false, "off")
        };
    }
    let enabled = match engine_id {
        e if e == ASCII2D => s.enable_ascii2d,
        e if e == TRACEMOE => s.enable_tracemoe,
        e if e == IQDB3D => s.enable_iqdb3d,
        e if e == GOOGLE_IMAGES => s.enable_google_images,
        e if e == GOOGLE_LENS => s.enable_google_lens,
        e if e == YANDEX => s.enable_yandex,
        _ => false,
    };
    if !enabled {
        return (false, "off");
    }
    (
        true,
        if s.extras_only_as_fallback {
            "fallback"
        } else {
            "on"
        },
    )
}

/// The chip's tooltip for a note.
pub fn chip_tooltip(note: &str) -> &'static str {
    match note {
        "primary" | "always" | "on" => "Runs on every image.",
        "fallback" => {
            "Held back for the fallback wave - only run when the engines before it found nothing \
             good enough."
        }
        _ => "Switched off in Settings > Engine.",
    }
}

/// The line under the chips: `Paced at 45\u{2013}75s an image.  Cached results are reused for 30
/// days.` A range whose ends are equal is not a range.
pub fn engine_summary(s: &Settings) -> String {
    let (low, high) = (s.delay_min_seconds, s.delay_max_seconds);
    let pace = if (high - low).abs() < 0.5 {
        format!("{}s", round_half_even(low))
    } else {
        format!("{}\u{2013}{}s", round_half_even(low), round_half_even(high))
    };
    let cache = if s.use_search_cache {
        format!(
            "Cached results are reused for {} days.",
            round_half_even(s.search_cache_ttl_days)
        )
    } else {
        "The search cache is off.".to_owned()
    };
    format!("Paced at {pace} an image.  {cache}")
}

/// The Activity page's widgets.
pub struct ActivityPage {
    pub root: gtk::Box,
    pub chips: gtk::FlowBox,
    pub pipeline: gtk::Label,
    pub summary: gtk::Label,
    pub parser_health: gtk::Button,
    pub clear_cache: gtk::Button,
    pub show_in_queue: gtk::Button,
    pub follow: gtk::ToggleButton,
    pub copy: gtk::Button,
    pub open_folder: gtk::Button,
    pub log_view: gtk::TextView,
    pub log_scroll: gtk::ScrolledWindow,
    pub footer: gtk::Label,
    pub timer: RefCell<Option<glib::SourceId>>,
    pub live: Cell<bool>,
}

fn boxed_toggle(glyph: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::with_label(glyph);
    b.add_css_class("icon-boxed");
    b.set_size_request(widgets::ICON_BOX, widgets::ICON_BOX);
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    b
}

/// `_build_activity_page()`.
pub fn build(log_path: &std::path::Path) -> Rc<ActivityPage> {
    let root = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
    // U1: the title sits where Queue's and Review's do.
    root.append(&widgets::screen_title("Activity."));
    let gap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    gap.set_size_request(
        -1,
        (widgets::SCREEN_TITLE_GAP + widgets::QUEUE_BODY_GAP - widgets::PAGE_SPACING).max(0),
    );
    root.append(&gap);

    // -- 01 // Engines
    let engines = widgets::card(None, false);
    engines.set_margin_start(0);
    engines.set_margin_end(0);
    engines.set_margin_top(0);
    engines.set_margin_bottom(0);
    engines.append(&widgets::section_header("01 // Engines", &[]));
    let chips = gtk::FlowBox::new();
    chips.set_selection_mode(gtk::SelectionMode::None);
    chips.set_homogeneous(false);
    chips.set_max_children_per_line(10);
    chips.set_column_spacing(8);
    chips.set_row_spacing(8);
    chips.set_margin_bottom(ENGINE_ROW_GAP);
    engines.append(&chips);
    let pipeline_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let pipeline_label = gtk::Label::new(Some("Pipeline:"));
    pipeline_label.add_css_class("pipeline-line");
    pipeline_label.set_valign(gtk::Align::Start);
    pipeline_row.append(&pipeline_label);
    let pipeline = gtk::Label::new(None);
    pipeline.add_css_class("pipeline-names");
    pipeline.set_wrap(true);
    pipeline.set_xalign(0.0);
    pipeline.set_hexpand(true);
    pipeline_row.append(&pipeline);
    engines.append(&pipeline_row);
    let summary = widgets::hint("");
    summary.set_margin_bottom(ENGINE_ACTIONS_GAP);
    engines.append(&summary);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
    let parser_health = widgets::pill_button("Parser health\u{2026}", false, true);
    let clear_cache = widgets::pill_button("Clear search cache\u{2026}", false, true);
    actions.append(&parser_health);
    actions.append(&clear_cache);
    let stretch = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    stretch.set_hexpand(true);
    actions.append(&stretch);
    // The glyph alone is ambiguous, so it is named.
    let show_label = gtk::Label::new(Some("Show in Queue"));
    show_label.add_css_class("button-hint");
    show_label.add_css_class("caps");
    actions.append(&show_label);
    let show_in_queue =
        widgets::icon_button("\u{2B10}", "Switch to Queue and focus the list.", true);
    actions.append(&show_in_queue);
    engines.append(&actions);
    root.append(&engines);
    let body_gap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body_gap.set_size_request(-1, ACTIVITY_BODY_GAP - widgets::PAGE_SPACING);
    root.append(&body_gap);

    // -- 02 // Log
    let log = widgets::card(None, false);
    log.set_margin_start(0);
    log.set_margin_end(0);
    log.set_margin_top(0);
    log.set_margin_bottom(0);
    log.set_vexpand(true);
    let follow = boxed_toggle(
        "\u{21BB}",
        "Keep the log up to date while this page is open.",
    );
    follow.set_active(true);
    let copy = widgets::icon_button("\u{29C9}", "Copy the whole log.", true);
    let open_folder = widgets::icon_button("\u{2922}", "Open the folder the log file is in.", true);
    log.append(&widgets::section_header(
        "02 // Log",
        &[
            follow.upcast_ref(),
            copy.upcast_ref(),
            open_folder.upcast_ref(),
        ],
    ));
    let log_view = gtk::TextView::new();
    log_view.add_css_class("log");
    log_view.set_editable(false);
    log_view.set_cursor_visible(false);
    log_view.set_monospace(true);
    log_view.set_wrap_mode(gtk::WrapMode::None);
    let log_scroll = gtk::ScrolledWindow::new();
    log_scroll.set_vexpand(true);
    log_scroll.set_child(Some(&log_view));
    log.append(&log_scroll);
    let footer = widgets::muted(&format!("Writing to {}", log_path.display()));
    footer.add_css_class("log-footer");
    log.append(&footer);
    root.append(&log);

    Rc::new(ActivityPage {
        root,
        chips,
        pipeline,
        summary,
        parser_health,
        clear_cache,
        show_in_queue,
        follow,
        copy,
        open_folder,
        log_view,
        log_scroll,
        footer,
        timer: RefCell::new(None),
        live: Cell::new(false),
    })
}

impl ActivityPage {
    /// The chips as the page shows them: `(text, on, tooltip)`.
    pub fn chip_texts(&self) -> Vec<(String, bool, String)> {
        let mut out = Vec::new();
        let mut child = self.chips.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if let Some(label) = c
                .downcast_ref::<gtk::FlowBoxChild>()
                .and_then(|fc| fc.child())
                .and_downcast::<gtk::Label>()
            {
                out.push((
                    label.text().to_string(),
                    label.has_css_class("on"),
                    label
                        .tooltip_text()
                        .map(|t| t.to_string())
                        .unwrap_or_default(),
                ));
            }
        }
        out
    }

    /// What the log pane holds.
    pub fn log_text(&self) -> String {
        let buf = self.log_view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string()
    }
}

impl MainWindow {
    /// `refresh_engines()`: restates the line-up. Called on build, when the page comes to the
    /// front, and after Settings closes.
    pub fn refresh_engines(&self) {
        let page = &self.s.activity;
        while let Some(child) = page.chips.first_child() {
            page.chips.remove(&child);
        }
        let settings = self.s.settings.lock().unwrap().clone();
        for engine_id in ALL_ENGINES {
            let (on, note) = engine_state(&settings, engine_id);
            let chip = widgets::chip(&format!("{} \u{b7} {note}", label_for(engine_id)), on);
            chip.set_tooltip_text(Some(chip_tooltip(note)));
            page.chips.append(&chip);
        }
        // DAN-2297 ruling 2: the chips say how the engines are set, and Settings is where that is
        // changed, so none of them is a Tab stop. The tooltips stay for the pointer.
        // `can-focus` is the one that takes the whole subtree out of the Tab chain; with only the
        // wrappers made unfocusable the FlowBox still swallows the Tab and the walk never leaves it.
        page.chips.set_can_focus(false);
        page.chips.set_focusable(false);
        page.pipeline.set_text(&pipeline_summary(&settings));
        page.summary.set_text(&engine_summary(&settings));
        let _ = engine_ids::IQDB;
    }

    /// `_tail_log()`: re-reads the log, keeping the view where the reader put it. It only follows
    /// when already at the bottom, because jumping on every tick would make the pane unusable for
    /// scrolling back to find where something went wrong.
    pub fn tail_log(&self) {
        let page = &self.s.activity;
        let mut text = self.s.logs.recent_log();
        if text.is_empty() {
            text = NO_LOG_YET.to_owned();
        }
        if text == page.log_text() {
            return;
        }
        let adj = page.log_scroll.vadjustment();
        let was_at_bottom = adj.value() >= adj.upper() - adj.page_size() - 4.0;
        let position = adj.value();
        page.log_view.buffer().set_text(&text);
        // The new height is known after the next layout pass.
        let adj2 = adj.clone();
        glib::idle_add_local_once(move || {
            let max = (adj2.upper() - adj2.page_size()).max(0.0);
            adj2.set_value(if was_at_bottom {
                max
            } else {
                position.min(max)
            });
        });
    }

    /// `set_activity_live(live)`: starts or stops the tail as the page comes and goes.
    pub fn set_activity_live(&self, live: bool) {
        let page = &self.s.activity;
        page.live.set(live);
        if let Some(id) = page.timer.borrow_mut().take() {
            id.remove();
        }
        if live && page.follow.is_active() {
            self.tail_log();
            self.refresh_engines();
            let w = self.downgrade();
            let id = glib::timeout_add_local(TAIL_INTERVAL, move || match w.upgrade() {
                Some(w) => {
                    w.tail_log();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            *page.timer.borrow_mut() = Some(id);
        }
    }

    /// `_on_follow_toggled()`.
    fn on_follow_toggled(&self) {
        let page = &self.s.activity;
        if page.follow.is_active() {
            self.tail_log();
            if self.s.shell.current_mode() == "activity" {
                self.set_activity_live(true);
            }
        } else if let Some(id) = page.timer.borrow_mut().take() {
            id.remove();
        }
    }

    /// `_copy_log()`.
    fn copy_log(&self) {
        let text = self.s.activity.log_text();
        self.win.clipboard().set_text(&text);
    }

    /// `_open_log_folder()`.
    fn open_log_folder(&self) {
        let path = self.s.logs.log_path();
        if let Some(dir) = path.parent() {
            (self.s.open_path)(dir);
        }
    }

    /// `action_show_in_queue()`: Queue mode. DEFERRED(6.d): select the current image's row.
    pub fn action_show_in_queue(&self) {
        self.set_mode("queue");
    }

    /// The page's buttons, wired once the window exists.
    pub(crate) fn wire_activity(&self) {
        let a = &self.s.activity;
        let w = self.downgrade();
        a.parser_health.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.action_parser_health();
                }
            }
        });
        // Follows the (6.d) Clear Search Cache action, disabled until it lands.
        a.clear_cache
            .set_action_name(Some("win.clear-search-cache"));
        a.show_in_queue.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.action_show_in_queue();
                }
            }
        });
        a.follow.connect_toggled({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.on_follow_toggled();
                }
            }
        });
        a.copy.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.copy_log();
                }
            }
        });
        a.open_folder.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.open_log_folder();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::paths::Paths;

    fn settings() -> Settings {
        let paths = Paths::from_lookup(|_| Some("/".into())).unwrap();
        Settings::with_defaults(&paths)
    }

    #[test]
    fn the_primary_and_the_secondary_read_from_the_mode() {
        let mut s = settings();
        s.primary_engine = "iqdb".into();
        s.secondary_engine_mode = "fallback".into();
        assert_eq!(engine_state(&s, "iqdb"), (true, "primary"));
        assert_eq!(engine_state(&s, "saucenao"), (true, "fallback"));
        s.secondary_engine_mode = "always".into();
        assert_eq!(engine_state(&s, "saucenao"), (true, "always"));
        s.secondary_engine_mode = "disabled".into();
        assert_eq!(engine_state(&s, "saucenao"), (false, "off"));
        // A primary that is not one of the two reads as IQDB.
        s.primary_engine = "bogus".into();
        assert_eq!(engine_state(&s, "iqdb"), (true, "primary"));
    }

    #[test]
    fn extras_are_on_or_held_back_and_pawchive_is_never_held_back() {
        let mut s = settings();
        s.enable_ascii2d = true;
        s.extras_only_as_fallback = false;
        assert_eq!(engine_state(&s, "ascii2d"), (true, "on"));
        s.extras_only_as_fallback = true;
        assert_eq!(engine_state(&s, "ascii2d"), (true, "fallback"));
        s.enable_tracemoe = false;
        assert_eq!(engine_state(&s, "tracemoe"), (false, "off"));
        s.enable_pawchive_index = true;
        assert_eq!(engine_state(&s, "pawchiveindex"), (true, "on"));
        s.enable_pawchive = true;
        assert_eq!(engine_state(&s, "pawchive"), (true, "on"));
    }

    #[test]
    fn the_summary_does_not_call_equal_ends_a_range() {
        let mut s = settings();
        s.delay_min_seconds = 5.0;
        s.delay_max_seconds = 5.2;
        s.use_search_cache = true;
        s.search_cache_ttl_days = 30.0;
        assert_eq!(
            engine_summary(&s),
            "Paced at 5s an image.  Cached results are reused for 30 days."
        );
        s.delay_min_seconds = 45.0;
        s.delay_max_seconds = 75.0;
        s.use_search_cache = false;
        assert_eq!(
            engine_summary(&s),
            "Paced at 45\u{2013}75s an image.  The search cache is off."
        );
    }

    #[test]
    fn a_chip_says_what_its_note_means() {
        assert_eq!(chip_tooltip("primary"), "Runs on every image.");
        assert!(chip_tooltip("fallback").starts_with("Held back for the fallback wave"));
        assert_eq!(chip_tooltip("off"), "Switched off in Settings > Engine.");
    }
}
