//! The window's chrome: what surrounds whichever half of the app you are in (`gui/shell.py` at
//! `76c85c4`).
//!
//! Three pieces, top to bottom: **the top bar** (identity, the mode switcher, the one button that
//! starts work), **the run strip** (everything about a run in progress, visible from every mode)
//! and **the mode stack** (the rest of the window). The status bar keeps the one thing a status
//! bar is for: the last thing that happened.

use crate::widgets::{self, GhostKanji};
use gtk4 as gtk;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

/// The mockup's run strip is 220 px of gauge and a hairline of line.
pub const GAUGE_WIDTH: i32 = 220;
pub const GAUGE_HEIGHT: i32 = 2;

/// What the strip says when no run has anything to report (R-05).
pub const IDLE_PROGRESS: &str = "nothing queued";
pub const IDLE_ETA: &str = "ETA \u{2014}";
/// What it says after a run that left nothing behind (R-06).
pub const RUN_FAILED_NOTE: &str = "last run failed \u{2014} see crash log";
/// What it says while SauceNAO's daily quota holds the run (S-04, R-06).
pub const QUOTA_PAUSED_NOTE: &str = "paused \u{2014} quota exhausted";
pub const QUOTA_PAUSED_ETA: &str = "ETA paused";

/// The kanji 鏡 (mirror) beside HATATE.
pub const MARK_GLYPH: &str = "\u{93E1}";
/// The ghost kanji behind each mode (G-07). The Empty state is a page of Queue, so its glyph is
/// asked for through [`ghost_glyph_for`] while that page is showing.
pub const EMPTY_GHOST_GLYPH: &str = "\u{7A7A}";

/// `(key, label, tooltip)`. The order is the order of the work.
pub const MODES: [(&str, &str, &str); 3] = [
    (
        "queue",
        "Queue",
        "The list: add images, filter them, and run searches over them.",
    ),
    (
        "review",
        "Review",
        "One image at a time, your copy against the match, with the keys to decide.",
    ),
    (
        "activity",
        "Activity",
        "How the run is going, what each engine is doing, and the log.",
    ),
];

pub fn ghost_glyph(mode: &str) -> &'static str {
    match mode {
        "queue" => "\u{529B}",
        "review" => "\u{93E1}",
        "activity" => "\u{52D5}",
        _ => "",
    }
}

/// `split_figures(text)`: `text` as `[(fragment, is_figure), ...]`, in order, losing nothing. A
/// "figure" is the part the mockup sets in bold: a number with the punctuation and unit that
/// travel with it (`1,234/24,000`, `~16d`, `38s`, `14:20`): `~?\d[\d,.:/]*[A-Za-z]*`.
pub fn split_figures(text: &str) -> Vec<(String, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < chars.len() {
        let start = i;
        let mut j = i;
        if chars[j] == '~' {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
            while j < chars.len() && (chars[j].is_ascii_digit() || ",.:/".contains(chars[j])) {
                j += 1;
            }
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            if !plain.is_empty() {
                out.push((std::mem::take(&mut plain), false));
            }
            out.push((chars[start..j].iter().collect(), true));
            i = j;
        } else {
            plain.push(chars[start]);
            i = start + 1;
        }
    }
    if !plain.is_empty() {
        out.push((plain, false));
    }
    out
}

/// One `label  FIGURE  label` reading in the run strip. The mockup sets the figures bold against
/// dim mono labels, so each run of text is its own label and the sheet colours them. `text()` is
/// the plain string, not markup.
pub struct Readout {
    pub root: gtk::Box,
    plain: RefCell<String>,
    parts: RefCell<Vec<gtk::Label>>,
    tooltip: RefCell<Option<String>>,
}

impl Readout {
    pub fn new(text: &str) -> Rc<Readout> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let r = Rc::new(Readout {
            root,
            plain: RefCell::new(String::from("\u{0}")),
            parts: RefCell::new(Vec::new()),
            tooltip: RefCell::new(None),
        });
        r.set_text(text);
        r
    }

    pub fn text(&self) -> String {
        self.plain.borrow().clone()
    }

    pub fn set_text(&self, text: &str) {
        if *self.plain.borrow() == text {
            return;
        }
        *self.plain.borrow_mut() = text.to_owned();
        for part in self.parts.borrow_mut().drain(..) {
            self.root.remove(&part);
        }
        for (fragment, is_figure) in split_figures(text) {
            let label = gtk::Label::new(Some(&fragment));
            label.add_css_class(if is_figure { "figure" } else { "readout" });
            label.add_css_class("caps");
            self.root.append(&label);
            self.parts.borrow_mut().push(label);
        }
        self.root
            .update_property(&[gtk::accessible::Property::Label(text)]);
        // An empty reading takes no room, so the strip's spacing does not leave a gap.
        self.root.set_visible(!text.is_empty());
    }

    pub fn set_tooltip(&self, text: &str) {
        *self.tooltip.borrow_mut() = Some(text.to_owned());
        self.root.set_tooltip_text(Some(text));
    }
}

/// The chrome's widgets, under the names Python gave them.
pub struct Shell {
    pub root: GhostKanji,
    pub mode_stack: gtk::Stack,
    pub mode_buttons: BTreeMap<String, gtk::ToggleButton>,
    pub search_toggle_btn: gtk::Button,
    pub settings_btn: gtk::Button,
    pub run_strip: gtk::Box,
    pub run_tag: gtk::Label,
    pub progress_bar: gtk::ProgressBar,
    pub run_progress_label: Rc<Readout>,
    pub sent_count_label: Rc<Readout>,
    pub run_note_label: Rc<Readout>,
    pub wait_countdown_label: Rc<Readout>,
    pub saucenao_quota_label: Rc<Readout>,
    pub run_eta_label: Rc<Readout>,
    pub status_bar: gtk::Box,
    pub status_label: gtk::Label,
    current_mode: RefCell<String>,
    suppress: Cell<bool>,
}

impl Shell {
    /// `_build_shell(pages)`: puts the chrome around `pages`, a mode key to widget mapping.
    /// `on_mode` runs when the user (or `set_mode`) changes mode; `on_search` and `on_settings`
    /// are the two top-bar buttons.
    pub fn new(
        pages: &[(&str, gtk::Widget)],
        on_mode: impl Fn(&str) + 'static,
        on_search: impl Fn() + 'static,
        on_settings: impl Fn() + 'static,
    ) -> Rc<Shell> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::SHELL_SPACING);
        let (l, t, r, b) = widgets::SHELL_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);

        // -- top bar
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        let mark = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        mark.update_property(&[gtk::accessible::Property::Label("Hatate")]);
        let glyph = gtk::Label::new(Some(MARK_GLYPH));
        glyph.add_css_class("mark-glyph");
        mark.append(&glyph);
        let word = gtk::Label::new(Some("Hatate"));
        word.add_css_class("mark-word");
        word.add_css_class("caps");
        mark.append(&word);
        bar.append(&mark);
        mark.set_margin_end(8);

        let mode_cb: Rc<dyn Fn(&str)> = Rc::new(on_mode);
        let (mode_switch, mode_buttons) = widgets::segmented(&MODES, "queue", {
            let cb = mode_cb.clone();
            move |k| cb(k)
        });
        bar.append(&mode_switch);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        bar.append(&spacer);

        let search_toggle_btn = widgets::pill_button("\u{25B6} Start Search", true, true);
        search_toggle_btn.connect_clicked(move |_| on_search());
        bar.append(&search_toggle_btn);
        let settings_btn = widgets::icon_button("\u{2699}", "Preferences\u{2026}", true);
        settings_btn.connect_clicked(move |_| on_settings());
        bar.append(&settings_btn);
        column.append(&bar);

        // -- run strip
        let run_strip = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        run_strip.add_css_class("run-strip");
        let run_tag = widgets::section_label("00 // Run");
        run_strip.append(&run_tag);
        let progress_bar = gtk::ProgressBar::new();
        progress_bar.add_css_class("run-gauge");
        progress_bar.set_size_request(GAUGE_WIDTH, GAUGE_HEIGHT);
        progress_bar.set_valign(gtk::Align::Center);
        progress_bar.set_hexpand(false);
        run_strip.append(&progress_bar);

        let run_progress_label = Readout::new("");
        run_progress_label.set_tooltip(
            "How far through the run: images searched out of the images in this run. This is \
             what the gauge to the left draws. It is not the number sent to Hydrus, which is \
             counted separately.",
        );
        let sent_count_label = Readout::new("");
        sent_count_label.set_tooltip(
            "How many images in the list have been sent to Hydrus. Counts images Hydrus \
             acknowledged holding; anything still queued with Hydrus's own downloader is shown \
             separately until it's confirmed.",
        );
        let run_note_label = Readout::new("");
        let wait_countdown_label = Readout::new("");
        wait_countdown_label.set_tooltip(
            "Live countdown for whatever the app is currently waiting on - the rate-limit delay \
             between searches, or confirming a Hydrus URL-importer download during auto-import.",
        );
        let saucenao_quota_label = Readout::new("");
        for r in [
            &run_progress_label,
            &sent_count_label,
            &run_note_label,
            &wait_countdown_label,
            &saucenao_quota_label,
        ] {
            run_strip.append(&r.root);
        }
        let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fill.set_hexpand(true);
        run_strip.append(&fill);
        let run_eta_label = Readout::new("");
        run_eta_label.set_tooltip(
            "How much longer the run has to go at the pace measured so far. A search is paced at \
             45-75s an image by default, so this is the estimate that says whether a batch \
             finishes tonight or next week.\n\nThe pace is averaged over the whole run. Images \
             served from the search cache cost almost nothing, so a batch with many of them \
             finishes sooner than the delay alone would suggest, and the estimate follows as it \
             learns.",
        );
        run_strip.append(&run_eta_label.root);
        // Never blank: an idle strip says so (R-05).
        run_progress_label.set_text(IDLE_PROGRESS);
        run_eta_label.set_text(IDLE_ETA);
        column.append(&run_strip);

        // -- mode stack
        let mode_stack = gtk::Stack::new();
        mode_stack.set_vexpand(true);
        mode_stack.set_hhomogeneous(false);
        mode_stack.set_vhomogeneous(false);
        // The footprint follows only the page on screen (`_ModeStack`): GTK's stack measures
        // only the visible child when it is not homogeneous.
        for (key, _label, _tip) in MODES {
            let page = pages
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, w)| w.clone())
                .unwrap_or_else(|| placeholder_page(key));
            mode_stack.add_named(&page, Some(key));
        }
        column.append(&mode_stack);

        // -- status bar
        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_bar.add_css_class("statusbar");
        status_bar.set_size_request(-1, widgets::STATUSBAR_HEIGHT);
        let status_label = gtk::Label::new(Some("Ready"));
        status_label.add_css_class("caps-none");
        status_label.set_xalign(0.0);
        status_label.set_hexpand(true);
        status_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status_bar.append(&status_label);

        let root = GhostKanji::new(ghost_glyph("queue"), &column);
        let shell = Rc::new(Shell {
            root,
            mode_stack,
            mode_buttons,
            search_toggle_btn,
            settings_btn,
            run_strip,
            run_tag,
            progress_bar,
            run_progress_label,
            sent_count_label,
            run_note_label,
            wait_countdown_label,
            saucenao_quota_label,
            run_eta_label,
            status_bar,
            status_label,
            current_mode: RefCell::new("queue".to_owned()),
            suppress: Cell::new(false),
        });
        shell.mode_stack.set_visible_child_name("queue");
        shell
    }

    /// `current_mode()`.
    pub fn current_mode(&self) -> String {
        self.current_mode.borrow().clone()
    }

    /// `set_mode(key)`: shows one mode and syncs the switch. Returns whether the mode changed
    /// to a known page.
    pub fn show_mode(&self, key: &str, ghost: &str) -> bool {
        if self.mode_stack.child_by_name(key).is_none() {
            return false;
        }
        self.mode_stack.set_visible_child_name(key);
        *self.current_mode.borrow_mut() = key.to_owned();
        self.root.set_glyph(ghost);
        if let Some(button) = self.mode_buttons.get(key) {
            if !button.is_active() {
                self.suppress.set(true);
                button.set_active(true);
                self.suppress.set(false);
            }
        }
        true
    }

    /// `status_label.setText`.
    pub fn say(&self, text: &str) {
        self.status_label.set_text(text);
    }

    pub fn status_text(&self) -> String {
        self.status_label.text().to_string()
    }
}

/// `_placeholder_page(key)`: a mode that has not been built yet still has to be something.
pub fn placeholder_page(_key: &str) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let label = widgets::muted("Not built yet.");
    label.set_halign(gtk::Align::Center);
    label.set_valign(gtk::Align::Center);
    label.set_vexpand(true);
    page.append(&label);
    page.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig(s: &str) -> Vec<(String, bool)> {
        split_figures(s)
    }

    /// `_FIGURE = ~?\d[\d,.:/]*[A-Za-z]*` (`gui/shell.py:66`).
    #[test]
    fn figures_are_the_numbers_with_their_punctuation_and_unit() {
        assert_eq!(
            fig("7/8 searched"),
            [("7/8".into(), true), (" searched".into(), false)]
        );
        assert_eq!(
            fig("waiting 38s"),
            [("waiting ".into(), false), ("38s".into(), true)]
        );
        assert_eq!(
            fig("ETA ~2 min"),
            [
                ("ETA ".into(), false),
                ("~2".into(), true),
                (" min".into(), false)
            ]
        );
        assert_eq!(
            fig("1,234/24,000 sent"),
            [("1,234/24,000".into(), true), (" sent".into(), false)]
        );
        assert_eq!(
            fig("ETA ~16d"),
            [("ETA ".into(), false), ("~16d".into(), true)]
        );
        assert_eq!(
            fig("until 14:20"),
            [("until ".into(), false), ("14:20".into(), true)]
        );
    }

    #[test]
    fn splitting_loses_nothing() {
        for s in [
            "nothing queued",
            "ETA \u{2014}",
            "~3 left",
            "a~b 12.5MB c",
            "~",
            "~~4x",
            "0",
            "",
        ] {
            let joined: String = fig(s).into_iter().map(|(f, _)| f).collect();
            assert_eq!(joined, s);
        }
    }

    #[test]
    fn a_tilde_without_a_digit_is_plain_text() {
        assert_eq!(fig("~ x"), [("~ x".into(), false)]);
    }

    #[test]
    fn modes_and_ghosts_are_pythons() {
        let keys: Vec<_> = MODES.iter().map(|m| m.0).collect();
        assert_eq!(keys, ["queue", "review", "activity"]);
        assert_eq!(ghost_glyph("queue"), "力");
        assert_eq!(ghost_glyph("review"), "鏡");
        assert_eq!(ghost_glyph("activity"), "動");
        assert_eq!(EMPTY_GHOST_GLYPH, "空");
        assert_eq!(MARK_GLYPH, "鏡");
        // `test_ghost_kanji.py::test_the_empty_glyph_is_in_the_bundled_subset`: every ghost glyph
        // is one the bundled kanji face carries.
        for g in ["queue", "review", "activity"]
            .map(ghost_glyph)
            .into_iter()
            .chain([EMPTY_GHOST_GLYPH])
        {
            let c = g.chars().next().unwrap();
            assert_eq!(g.chars().count(), 1);
            assert!(crate::fonts::KANJI_GLYPHS.contains(&c), "{g:?}");
        }
    }
}
