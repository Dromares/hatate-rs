//! `CompareDialog` (`gui/compare_dialog.py` at `76c85c4`; `_open_compare_dialog` in
//! `gui/main_window.py`; DAN-2340): the local file against its matched image, in a modal window of
//! its own for a second monitor.
//!
//! It fetches the match at FULL resolution rather than reusing the sample the preview shows: the
//! sample is deliberately a downscaled copy, so comparing against it would misrepresent exactly
//! the quality difference this window exists to judge. Python does that fetch before the window
//! shows; here it runs on a worker and the window opens on "Loading…".

use crate::review_compare::{Loaded, Panes, build_panes, load_comparison};
use crate::widgets;
use crate::window::{MainWindow, MainWindowWeak};
use crate::wipe_view::WipeView;
use crate::worker::Event;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::compare_view::{SummaryInput, SummaryLine, Tone, build_summary};
use hatate_core::image_compare::{Source, dhash, hamming_distance};
use hatate_core::queue::EntryId;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// `self.resize(900, 780)`.
pub const DEFAULT_SIZE: (i32, i32) = (900, 780);

/// The text of the summary while the match comes down.
pub const LOADING: &str = "Loading\u{2026}";

/// What the worker hands back: the pair, and what the summary needs that only the files know.
pub struct Opened {
    pub loaded: Loaded,
    pub lines: Vec<SummaryLine>,
}

#[derive(Default)]
struct State {
    local: Option<Arc<hatate_core::compare_view::Rgba>>,
    matched: Option<Arc<hatate_core::compare_view::Rgba>>,
    /// Built the first time Differences is asked for: `Some(None)` is "tried, cannot compare".
    panes: Option<Option<Arc<Panes>>>,
    building: bool,
}

/// The window and its parts (a test reads them).
pub struct CompareDialog {
    pub window: gtk::Window,
    pub summary: gtk::Box,
    pub view: WipeView,
    pub wipe_radio: gtk::CheckButton,
    pub local_radio: gtk::CheckButton,
    pub matched_radio: gtk::CheckButton,
    pub diff_radio: gtk::CheckButton,
    pub zoom_label: gtk::Label,
    pub diff_note: gtk::Label,
    pub slider_left_label: gtk::Label,
    pub slider_right_label: gtk::Label,
    pub slider: gtk::Scale,
    state: RefCell<State>,
    worker: RefCell<Option<crate::worker::JobHandle>>,
    owner: MainWindowWeak,
}

/// One summary line as a label: the bold label first, then the words, toned by its CSS class.
fn line_label(line: &SummaryLine) -> gtk::Label {
    let l = gtk::Label::new(None);
    let text = glib::markup_escape_text(&line.text);
    match &line.label {
        Some(label) => l.set_markup(&format!(
            "<b>{}</b> {text}",
            glib::markup_escape_text(label)
        )),
        None => l.set_text(&line.text),
    }
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.set_selectable(false);
    l.add_css_class("summary-line");
    match line.tone {
        Tone::Good => l.add_css_class("summary-good"),
        Tone::Uncertain => l.add_css_class("summary-uncertain"),
        Tone::Plain => {}
    }
    if line.strong {
        l.add_css_class("summary-strong");
    }
    if line.italic {
        l.add_css_class("summary-italic");
    }
    l
}

/// The four view modes, in Python's order.
pub const MODES: [&str; 4] = ["Wipe", "Local only", "Match only", "Differences"];

/// `Differences`' tooltip.
pub const DIFF_TOOLTIP: &str = "Shows both pictures side by side - your local copy on the left, \
the match on the right - and sweeps a highlight of their differences across both as you drag the \
wipe: a watermark, a signature, added text, a censoring bar.\n\nDeliberately approximate. The two \
are almost always different resolutions, so they are scaled to a common size and compared \
structurally: it shows WHICH AREAS differ, not which exact pixels. Re-encoding noise is filtered \
out, which is why a mere re-save shows as no difference at all.\n\nWhether it is the same image is \
a separate question, and the perceptual match in the summary above answers it better.";

impl CompareDialog {
    pub fn new(
        parent: &impl IsA<gtk::Window>,
        filename: &str,
        owner: MainWindowWeak,
    ) -> Rc<CompareDialog> {
        let window = gtk::Window::builder()
            .title(format!("Compare - {filename}"))
            .transient_for(parent)
            .modal(true)
            .default_width(DEFAULT_SIZE.0)
            .default_height(DEFAULT_SIZE.1)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, widgets::PAGE_SPACING);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);

        // The dialog's one load-bearing readout: promoted off the plain body weight so it reads as
        // the verdict, not more mode and zoom chrome.
        let summary = gtk::Box::new(gtk::Orientation::Vertical, 2);
        summary.add_css_class("compare-summary");
        summary.append(&gtk::Label::builder().label(LOADING).xalign(0.0).build());
        column.append(&summary);

        let view = WipeView::new();
        view.area.set_vexpand(true);
        let frame = widgets::image_frame(&view.area);
        frame.set_vexpand(true);
        column.append(&frame);

        let mode_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let mut radios: Vec<gtk::CheckButton> = Vec::new();
        for mode in MODES {
            let radio = gtk::CheckButton::with_label(mode);
            if let Some(first) = radios.first() {
                radio.set_group(Some(first));
            }
            mode_row.append(&radio);
            radios.push(radio);
        }
        let (wipe_radio, local_radio, matched_radio, diff_radio) = (
            radios[0].clone(),
            radios[1].clone(),
            radios[2].clone(),
            radios[3].clone(),
        );
        wipe_radio.set_active(true);
        diff_radio.set_tooltip_text(Some(DIFF_TOOLTIP));
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        mode_row.append(&spacer);
        // Zoom sits on the row of the view modes: both are "how am I looking at this", as
        // opposed to the slider below, which is "what am I looking at".
        for (glyph, tip, op) in [
            ("\u{2212}", "Zoom out (-)", 0),
            ("+", "Zoom in (+)", 1),
            (
                "Fit",
                "Fit the whole image in the window (0, or double-click)",
                2,
            ),
            (
                "100%",
                "One image pixel per screen pixel (1) - the view that shows compression artefacts \
                 and resampling softness as they really are.",
                3,
            ),
        ] {
            let b = widgets::icon_button(glyph, tip, true);
            let v = view.clone();
            b.connect_clicked(move |_| match op {
                0 => v.zoom_out(),
                1 => v.zoom_in(),
                2 => v.zoom_to_fit(),
                _ => v.zoom_to_actual(),
            });
            mode_row.append(&b);
        }
        let zoom_label = widgets::muted("");
        zoom_label.set_width_chars(5);
        zoom_label.set_xalign(1.0);
        mode_row.append(&zoom_label);
        column.append(&mode_row);

        // What the differences view found. Hidden in the other modes, so it never describes
        // something that is not on screen.
        let diff_note = widgets::hint("");
        diff_note.set_visible(false);
        column.append(&diff_note);

        // Named per mode: the right-hand side is the matched image under the wipe, but the
        // highlighted copy in the differences view, and a label that says "Match" while showing
        // highlights is a small lie the user has to work around.
        let slider_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let slider_left_label = gtk::Label::new(Some("Local"));
        let slider_right_label = gtk::Label::new(Some("Match"));
        let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        slider.set_value(50.0);
        slider.set_draw_value(false);
        slider.set_hexpand(true);
        slider.add_css_class("wipe-slider");
        for l in [&slider_left_label, &slider_right_label] {
            l.add_css_class("wipe-end-label");
            l.add_css_class("caps");
        }
        slider_row.append(&slider_left_label);
        slider_row.append(&slider);
        slider_row.append(&slider_right_label);
        column.append(&slider_row);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, widgets::BUTTON_ROW_SPACING);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        buttons.append(&spacer);
        let close = widgets::pill_button("Close", false, false);
        buttons.append(&close);
        column.append(&buttons);
        window.set_child(Some(&column));
        gtk::prelude::GtkWindowExt::set_focus(&window, Some(&close));

        let dialog = Rc::new(CompareDialog {
            window: window.clone(),
            summary,
            view: view.clone(),
            wipe_radio,
            local_radio,
            matched_radio,
            diff_radio,
            zoom_label: zoom_label.clone(),
            diff_note,
            slider_left_label,
            slider_right_label,
            slider: slider.clone(),
            state: RefCell::new(State::default()),
            worker: RefCell::new(None),
            owner,
        });
        {
            let label = zoom_label;
            view.connect_zoom_changed(move |percent| {
                label.set_text(&if percent == 0.0 {
                    String::new()
                } else {
                    format!("{percent:.0}%")
                });
            });
        }
        {
            let v = view;
            slider.connect_value_changed(move |s| v.set_position(s.value() / 100.0));
        }
        {
            let w = window.clone();
            close.connect_clicked(move |_| w.close());
        }
        for radio in [
            &dialog.wipe_radio,
            &dialog.local_radio,
            &dialog.matched_radio,
            &dialog.diff_radio,
        ] {
            let d = Rc::downgrade(&dialog);
            radio.connect_toggled(move |r| {
                if r.is_active() {
                    if let Some(d) = d.upgrade() {
                        d.apply_mode();
                    }
                }
            });
        }
        dialog
    }

    /// `_apply_mode`: Wipe, Local only, Match only. The single-picture modes exist because
    /// flicking between two full views is often a better way to spot a subtle difference than a
    /// static split.
    pub fn apply_mode(self: &Rc<Self>) {
        if self.diff_radio.is_active() {
            self.slider.set_sensitive(true);
            self.slider_right_label.set_text("Differences");
            self.show_differences();
            return;
        }
        self.slider_right_label.set_text("Match");
        self.diff_note.set_visible(false);
        if self.view.showing_differences() {
            self.view.clear_difference_view();
        }
        if self.local_radio.is_active() {
            self.slider.set_sensitive(false);
            self.view.set_position(1.0);
        } else if self.matched_radio.is_active() {
            self.slider.set_sensitive(false);
            self.view.set_position(0.0);
        } else {
            self.slider.set_sensitive(true);
            self.view.set_position(self.slider.value() / 100.0);
        }
    }

    /// `_show_differences`: built once and reused, since recomputing it on every mode switch would
    /// stall the window for no reason; and off the main loop.
    fn show_differences(self: &Rc<Self>) {
        let (local, matched, cached) = {
            let s = self.state.borrow();
            (s.local.clone(), s.matched.clone(), s.panes.clone())
        };
        let (Some(local), Some(matched)) = (local, matched) else {
            return;
        };
        match cached {
            Some(panes) => self.present_differences(panes),
            None => {
                if self.state.borrow().building {
                    return;
                }
                self.state.borrow_mut().building = true;
                self.diff_note.set_text("Comparing the two\u{2026}");
                self.diff_note.set_visible(true);
                let d = Rc::downgrade(self);
                let Some(owner) = self.owner.upgrade() else {
                    return;
                };
                let job = owner.s.workers.run(
                    move |_ctx: &crate::worker::Ctx<()>| build_panes(&local, &matched),
                    move |event: Event<(), Option<Arc<Panes>>>| {
                        let Some(d) = d.upgrade() else { return };
                        if let Event::Done(panes) = event {
                            {
                                let mut s = d.state.borrow_mut();
                                s.building = false;
                                s.panes = Some(panes.clone());
                            }
                            if d.diff_radio.is_active() {
                                d.present_differences(panes);
                            }
                        }
                    },
                );
                *self.worker.borrow_mut() = Some(job);
            }
        }
    }

    fn present_differences(&self, panes: Option<Arc<Panes>>) {
        match panes {
            None => {
                self.diff_note.set_text("Could not compare these two.");
                self.diff_note.set_visible(true);
            }
            Some(p) => {
                self.view
                    .set_difference_view(&p.local, &p.matched, &p.overlay);
                self.view.set_position(self.slider.value() / 100.0);
                self.diff_note.set_text(&format!(
                    "{}  \u{b7}  Local on the left, match on the right - drag the wipe to sweep \
                     the highlights across both.",
                    p.verdict
                ));
                self.diff_note.set_visible(true);
            }
        }
    }

    /// `_load`'s tail: the pair is in hand.
    fn loaded(self: &Rc<Self>, opened: Opened) {
        let Opened { loaded, lines } = opened;
        let local = loaded.local.ok().map(Arc::new);
        let matched = loaded.matched.map(Arc::new);
        self.view.set_images(local.as_deref(), matched.as_deref());
        while let Some(child) = self.summary.first_child() {
            self.summary.remove(&child);
        }
        for line in &lines {
            self.summary.append(&line_label(line));
        }
        self.diff_radio
            .set_sensitive(local.is_some() && matched.is_some());
        {
            let mut s = self.state.borrow_mut();
            s.local = local;
            s.matched = matched;
            s.panes = None;
        }
        self.apply_mode();
    }

    /// The text of each summary line, in order (a test reads it).
    pub fn summary_text(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut child = self.summary.first_child();
        while let Some(c) = child {
            if let Some(l) = c.downcast_ref::<gtk::Label>() {
                out.push(l.text().to_string());
            }
            child = c.next_sibling();
        }
        out
    }
}

impl MainWindow {
    /// `_open_compare_dialog(entry)`.
    pub fn open_compare_dialog(&self, id: EntryId) {
        let (entry, has_candidate) = {
            let q = self.s.queue.borrow();
            let Some(e) = q.entry(id) else { return };
            (e.clone(), e.selected_candidate().is_some())
        };
        if !has_candidate {
            crate::dialogs::information(
                self.s.prompter.as_ref(),
                "Compare",
                "This image has no match to compare against yet.",
            );
            return;
        }
        if entry.file_missing || !std::path::Path::new(&entry.path).exists() {
            crate::dialogs::information(
                self.s.prompter.as_ref(),
                "Compare",
                "The local file is no longer on disk, so there's nothing to compare the match \
                 against.",
            );
            return;
        }
        tracing::info!("Opening comparison view for {}", entry.filename());
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            tracing::warn!("Could not open the comparison: the engines could not be built");
            return;
        };
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        let dialog = CompareDialog::new(&self.win, entry.filename(), self.downgrade());
        *self.s.compare_open.borrow_mut() = Some(dialog.clone());
        {
            let w = self.downgrade();
            dialog.window.connect_close_request(move |_| {
                if let Some(w) = w.upgrade() {
                    if let Some(d) = w.s.compare_open.borrow_mut().take() {
                        if let Some(job) = d.worker.borrow_mut().take() {
                            job.retire();
                        }
                    }
                }
                glib::Propagation::Proceed
            });
        }
        dialog.window.present();
        let settings = self.s.settings.clone();
        let engine = backend.engine.clone();
        let (path, candidate) = (entry.path.clone(), entry.selected_candidate().cloned());
        let (entry_w, entry_h) = (entry.local_width, entry.local_height);
        let d = Rc::downgrade(&dialog);
        let job = self.s.workers.run(
            move |_ctx: &crate::worker::Ctx<()>| {
                let loaded = load_comparison(&handle, &engine, &settings, &path, candidate);
                let lines = summarize(&loaded, &path, (entry_w, entry_h));
                Opened { loaded, lines }
            },
            move |event: Event<(), Opened>| {
                let Some(d) = d.upgrade() else { return };
                if let Event::Done(opened) = event {
                    d.loaded(opened);
                }
            },
        );
        *dialog.worker.borrow_mut() = Some(job);
    }
}

/// `_build_summary`'s inputs, read from the files.
fn summarize(
    loaded: &Loaded,
    path: &str,
    recorded: (Option<i64>, Option<i64>),
) -> Vec<SummaryLine> {
    let c = loaded.candidate.as_ref();
    let size_of =
        |p: Option<&hatate_core::compare_view::Rgba>, fallback: (Option<i64>, Option<i64>)| match p
        {
            Some(p) => (i64::from(p.width), i64::from(p.height)),
            None => (fallback.0.unwrap_or(0), fallback.1.unwrap_or(0)),
        };
    let distance = loaded.matched_bytes.as_ref().and_then(|bytes| {
        let a = dhash(Source::Path(std::path::Path::new(path)), 8)
            .ok()
            .flatten();
        let b = dhash(Source::Bytes(bytes), 8).ok().flatten();
        hamming_distance(a.as_ref(), b.as_ref()).map(i64::from)
    });
    build_summary(&SummaryInput {
        has_candidate: c.is_some(),
        local_size: size_of(loaded.local.as_ref().ok(), recorded),
        remote_size: size_of(
            loaded.matched.as_ref(),
            (c.and_then(|c| c.width), c.and_then(|c| c.height)),
        ),
        local_bytes: std::fs::metadata(path).ok().map(|m| m.len() as i64),
        matched_bytes: loaded.matched_bytes.as_ref().map(|b| b.len() as i64),
        candidate_remote_bytes: c.and_then(|c| c.remote_size_bytes),
        distance,
        matched_loaded: loaded.matched.is_some(),
        used_fallback: loaded.used_fallback,
    })
}
