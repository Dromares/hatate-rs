//! The Review page's widgets (`gui/review_view.py` `ReviewViewMixin._build_*` at `76c85c4`;
//! DAN-2219): the comparison card (view switch, readout, the two pictures with their captions),
//! the decision card (match stepper, candidate picker, tag list, the Hydrus buttons) and the
//! position strip. What the page does is in [`crate::review_ops`], on `MainWindow`.
//!
//! Wipe and Differences (DAN-2340) share the second stack page: the [`WipeView`] in its frame with the
//! zoom tools floating top right, the slider row under it and the note under that.

use crate::widgets::{self, FrameCaption, KeyHintBar, Piece};
use crate::wipe_view::WipeView;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

/// `VIEWS`: key, label, tooltip.
pub const VIEWS: [(&str, &str, &str); 3] = [
    (
        "pair",
        "Side by side",
        "Your copy and the match, each shown whole. Already loaded, so it costs nothing.",
    ),
    (
        "wipe",
        "Wipe",
        "The two in one frame, split by a slider you drag across them.\nFetches the match at full \
         resolution the first time, so the quality\ndifference is the real one and not a \
         downscaled sample's.",
    ),
    (
        "diff",
        "Differences",
        "Both pictures side by side with a red highlight of where they differ -\na watermark, a \
         signature, added text, a censoring bar - swept across\nboth at once, so the same region \
         is marked on each.\n\nDeliberately structural: the two are almost always different\n\
         resolutions, so it says which AREAS differ, never which exact pixels.",
    ),
];

/// The Wipe view's one-line how-to (mockup `review-wipe.html` `.review-note`).
pub const WIPE_HINT: &str = "Local on the left, match on the right - drag the divider to sweep one \
                             over the other. Scroll to zoom, drag to pan, double-click to fit.";

/// The tooltip the readout has when it has nothing to say.
pub const READOUT_TOOLTIP: &str = "How the selected match compares with your local file.";

/// `local_preview.setMinimumSize(280, 260)` and the matched pane's: the floor of both pictures.
pub const PREVIEW_MIN: (i32, i32) = (280, 260);

/// The rail's width (`review.html` `.side-rail`).
pub const RAIL_WIDTH: i32 = 340;

/// `ScaledImageLabel`: a picture that fits its pane, or the words that stand in for it.
pub struct PreviewPane {
    pub root: gtk::Stack,
    pub picture: gtk::Picture,
    pub text: gtk::Label,
}

impl PreviewPane {
    fn new(placeholder: &str) -> PreviewPane {
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        let text = gtk::Label::new(Some(placeholder));
        text.add_css_class("preview-text");
        text.set_wrap(true);
        text.set_justify(gtk::Justification::Center);
        let root = gtk::Stack::new();
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_size_request(PREVIEW_MIN.0, PREVIEW_MIN.1);
        root.add_named(&text, Some("text"));
        root.add_named(&picture, Some("picture"));
        root.set_visible_child_name("text");
        PreviewPane {
            root,
            picture,
            text,
        }
    }

    /// `setPixmap(QPixmap())` then `setText(text)`.
    pub fn show_text(&self, text: &str) {
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.text.set_text(text);
        self.root.set_visible_child_name("text");
    }

    /// `setPixmap(pixmap)`.
    pub fn show_texture(&self, texture: &gdk::Texture) {
        self.picture.set_paintable(Some(texture));
        self.root.set_visible_child_name("picture");
    }

    /// Whether a picture (and not the words) is on show.
    pub fn shows_picture(&self) -> bool {
        self.root.visible_child_name().as_deref() == Some("picture")
    }

    /// The words on show, `""` while a picture is.
    pub fn shown_text(&self) -> String {
        if self.shows_picture() {
            String::new()
        } else {
            self.text.text().to_string()
        }
    }
}

/// Whether a keyed button drops its key under the label: the pair does not fit in the row's `width`.
/// A row not yet allocated (width 0) is not "too narrow": read that way, every key wrapped for the
/// first frames and the Review page asked 809 px of height where it asks 755 once laid out. A window
/// does not shrink on every toolkit, so hosted `ci` kept 812 for the Not found row and for Add tags.
fn key_wraps(beside: i32, width: i32) -> bool {
    width > 0 && beside > width
}

/// A button that carries its key (`widgets.keyed_button`), and the cap to re-point.
pub struct Keyed {
    pub button: gtk::Button,
    pub cap: gtk::Label,
    row: gtk::Box,
    label: gtk::Label,
}

impl Keyed {
    fn new(text: &str, key: &str, primary: bool) -> Keyed {
        let (button, cap) = widgets::keyed_button(text, key, primary);
        let row = button
            .child()
            .and_downcast::<gtk::Box>()
            .expect("keyed_button's row");
        let label = row
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("keyed_button's label");
        let k = Keyed {
            button,
            cap,
            row,
            label,
        };
        k.wrap_when_narrow();
        k
    }

    /// `.btn-grid .btn--keyed { flex-wrap: wrap; row-gap: 2px }` (review.html): in a column too
    /// narrow for both, the key drops under the label instead of widening the rail. The button
    /// asks for no width of its own, so the grid's 50/50 split decides.
    ///
    /// The label may ellipsize, so its minimum is not its text: with the key beside it the button's
    /// minimum would be the full line, and a window that first opened wide (1100 px by default)
    /// could never be narrowed to the stated minimum, because the wrap below only fires when the
    /// column is narrower than that line.
    fn wrap_when_narrow(&self) {
        self.label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        self.button.set_size_request(1, -1);
        self.row.set_size_request(1, -1);
        let (row, label, cap) = (self.row.clone(), self.label.clone(), self.cap.clone());
        let seen = Cell::new(-1);
        self.button.add_tick_callback(move |_, _| {
            let width = row.width();
            if width != seen.get() {
                seen.set(width);
                let nat = |w: &gtk::Label| w.measure(gtk::Orientation::Horizontal, -1).1;
                let beside = nat(&label)
                    + if cap.is_visible() {
                        nat(&cap) + row.spacing()
                    } else {
                        0
                    };
                let wrapped = key_wraps(beside, width);
                row.set_orientation(if wrapped {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                });
                row.set_spacing(if wrapped { 2 } else { 10 });
                cap.set_halign(if wrapped {
                    gtk::Align::Start
                } else {
                    gtk::Align::Center
                });
            }
            glib::ControlFlow::Continue
        });
    }

    /// `KeyedButton.set_key(key)`: an unbound action shows no cap.
    pub fn set_key(&self, key: &str) {
        self.cap.set_text(key);
        self.cap.set_visible(!key.is_empty());
    }

    /// The label text (without the cap).
    pub fn text(&self) -> String {
        self.label.text().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.label.set_text(text);
        self.button
            .update_property(&[gtk::accessible::Property::Label(text)]);
    }
}

/// `child` asks its natural width and no minimum: it is clipped, never ellipsized, when the row it
/// sits in is squeezed. A label that may ellipsize is cut with room to spare, because GTK allocates
/// it its natural width and Pango ellipsizes a tracked label (CSS `letter-spacing`) at that very
/// width: "01 // COMPAR…" and "SIMILARIT…" on the host and hosted `ci` toolkits alike, in the
/// 1440 x 900 shot (DAN-2219).
fn give_way(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let sw = gtk::ScrolledWindow::new();
    sw.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
    sw.set_propagate_natural_width(true);
    sw.set_propagate_natural_height(true);
    // GTK4 scrolled windows take the Tab focus by default; this one only clips a label (DAN-2366).
    sw.set_focusable(false);
    sw.set_child(Some(child));
    sw
}

/// Every widget of the page, by the name Python gave it.
pub struct ReviewPage {
    pub root: gtk::Box,
    /// `review_stack`: Side by side (page 0), then the comparison pages.
    pub stack: gtk::Stack,
    pub title: gtk::Label,
    pub view_buttons: BTreeMap<String, gtk::ToggleButton>,
    pub readout_root: gtk::Box,
    /// The header row, and the line under it the readout drops to when the column is too narrow
    /// for both ([`ReviewPage::fit_readout`]).
    header: gtk::Box,
    spacer: gtk::Box,
    readout_row: gtk::Box,
    compare_col: gtk::Box,
    pub readout_glyph: gtk::Label,
    pub readout_value: gtk::Label,
    pub readout_label: gtk::Label,
    pub readout_diff: gtk::Label,
    readout_tier: RefCell<String>,
    /// Bumped when the readout's words change, so the fit is measured again.
    readout_generation: Cell<u32>,
    pub pop_out_btn: gtk::Button,
    pub shortcuts_btn: gtk::Button,
    pub show_in_queue_btn: gtk::Button,
    pub local: PreviewPane,
    pub matched: PreviewPane,
    pub local_caption: FrameCaption,
    pub matched_caption: FrameCaption,
    pub matched_note: gtk::Label,
    pub note: gtk::Label,
    // -- Wipe and Differences
    pub wipe: WipeView,
    /// `wipe_slider` and its row (`review_slider_row`): `Local ---|--- Match`.
    pub wipe_slider: gtk::Scale,
    pub slider_row: gtk::Box,
    /// The error state's detail line (design gate 2.8): the raw OS text, never the message.
    pub error_detail: gtk::Label,
    pub zoom_tools: gtk::Box,
    pub zoom_label: gtk::Label,
    // -- the decision card
    pub match_nav: gtk::Box,
    pub match_label: gtk::Label,
    pub prev_match_btn: gtk::Button,
    pub next_match_btn: gtk::Button,
    pub prev_match_key: gtk::Label,
    pub next_match_key: gtk::Label,
    pub candidate_model: gtk::StringList,
    pub candidate_combo: gtk::DropDown,
    /// Set while the model is rebuilt, so `selected` does not read as the user's choice
    /// (`blockSignals(True)`).
    pub combo_blocked: Cell<bool>,
    pub tag_list: gtk::ListBox,
    pub add_tags_btn: gtk::Button,
    pub remove_tags_btn: gtk::Button,
    pub edit_tags_btn: gtk::ToggleButton,
    pub send_btn: Keyed,
    pub send_caption: gtk::Label,
    pub url_btn: Keyed,
    pub download_btn: Keyed,
    pub mark_btn: Keyed,
    pub open_btn: Keyed,
    // -- the position strip
    pub prev_btn: Keyed,
    pub next_unreviewed_btn: Keyed,
    pub next_btn: Keyed,
    hint_slot: gtk::Box,
    hint_bar: RefCell<Rc<KeyHintBar>>,
}

/// `Uploads file + tags to Hydrus - not easily undone`.
pub const COMMIT_CAPTION: &str = "Uploads file + tags to Hydrus \u{2014} not easily undone";

impl ReviewPage {
    /// `_build_review_page`. `key_for(action_id)` is `_key_for`.
    pub fn new(key_for: &dyn Fn(&str) -> String) -> Rc<ReviewPage> {
        // -- the comparison card
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        // It gives way before the header asks for more than the window's minimum: hosted `ci`'s
        // fonts are wider than the author's and the header asked 1016 px of 1010 (DAN-2219).
        let title = widgets::section_label("01 // Compare");
        header.append(&give_way(&title));
        let (switch, view_buttons) = widgets::segmented(&VIEWS, "pair", |_| {});
        switch.add_css_class("review-switch");
        header.append(&switch);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        header.append(&spacer);

        let readout_parts = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        readout_parts.set_valign(gtk::Align::Center);
        let mk = |class: &str| {
            let l = gtk::Label::new(None);
            l.add_css_class(class);
            l.set_visible(false);
            readout_parts.append(&l);
            l
        };
        let readout_glyph = mk("readout-glyph");
        let readout_value = mk("readout-value");
        let readout_label = mk("readout-label");
        readout_label.add_css_class("caps");
        let readout_diff = mk("readout-diff");
        // The words are never ellipsized: they take their natural width, and beside the tools the
        // header's minimum does not include them (`give_way`).
        let readout_root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        readout_root.set_valign(gtk::Align::Center);
        readout_root.append(&give_way(&readout_parts));
        readout_root.set_tooltip_text(Some(READOUT_TOOLTIP));
        readout_root.set_visible(false);
        // Under the header until the column is wide enough to hold it beside the tools.
        let readout_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        readout_row.append(&readout_root);

        let pop_out_btn = widgets::icon_button(
            "\u{29c9}",
            "Open the comparison in its own window - for a second monitor.",
            true,
        );
        let shortcuts_btn =
            widgets::icon_button("\u{2328}", "Show all review keyboard shortcuts.", true);
        let show_in_queue_btn =
            widgets::icon_button("\u{2b90}", "Show this image in the Queue list.", true);
        for b in [&pop_out_btn, &shortcuts_btn, &show_in_queue_btn] {
            header.append(b);
        }

        let local = PreviewPane::new("No image selected");
        let matched = PreviewPane::new("No match yet");
        matched.root.set_cursor_from_name(Some("pointer"));
        let local_caption = FrameCaption::new("Local", "");
        let matched_caption = FrameCaption::new("Match", "");
        let matched_note = widgets::hint("");
        matched_note.set_visible(false);
        let pair = gtk::Grid::new();
        pair.set_column_spacing(16);
        pair.set_row_spacing(8);
        pair.set_column_homogeneous(true);
        pair.set_vexpand(true);
        pair.attach(&widgets::image_frame(&local.root), 0, 0, 1, 1);
        pair.attach(&local_caption.root, 0, 1, 1, 1);
        pair.attach(&widgets::image_frame(&matched.root), 1, 0, 1, 1);
        pair.attach(&matched_caption.root, 1, 1, 1, 1);
        pair.attach(&matched_note, 1, 2, 1, 1);

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        // The comparison page asks for its own height, not the pair's: the slider, the note and
        // the error detail sit under it, and the sum has to fit the stated minimum (DAN-2340).
        stack.set_vhomogeneous(false);
        stack.add_named(&pair, Some("pair"));
        let wipe = WipeView::new();
        let wipe_host = gtk::Overlay::new();
        wipe_host.set_child(Some(&wipe.area));
        // Floats inside the frame, top right (ruling R-3 on DAN-1178): in the header it would push
        // the readout past the compare column, and showing it per view would move the pictures.
        let zoom_tools = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        zoom_tools.add_css_class("zoom-tools");
        zoom_tools.set_halign(gtk::Align::End);
        zoom_tools.set_valign(gtk::Align::Start);
        zoom_tools.set_visible(false);
        zoom_tools.set_margin_top(8);
        zoom_tools.set_margin_end(8);
        for (glyph, tip, op) in [
            ("\u{2212}", "Zoom out (-)", 0),
            ("+", "Zoom in (+)", 1),
            ("Fit", "Fit the whole image (0, or double-click)", 2),
            (
                "100%",
                "One image pixel per screen pixel (1) - the view that shows\ncompression artefacts \
                 and resampling softness as they really are.",
                3,
            ),
        ] {
            let b = widgets::icon_button(glyph, tip, true);
            let w = wipe.clone();
            b.connect_clicked(move |_| match op {
                0 => w.zoom_out(),
                1 => w.zoom_in(),
                2 => w.zoom_to_fit(),
                _ => w.zoom_to_actual(),
            });
            zoom_tools.append(&b);
        }
        let zoom_label = widgets::muted("");
        zoom_label.add_css_class("zoom-percent");
        zoom_label.set_width_chars(5);
        zoom_label.set_xalign(1.0);
        zoom_tools.append(&zoom_label);
        wipe_host.add_overlay(&zoom_tools);
        {
            let label = zoom_label.clone();
            wipe.connect_zoom_changed(move |percent| {
                label.set_text(&if percent == 0.0 {
                    String::new()
                } else {
                    format!("{percent:.0}%")
                });
            });
        }
        stack.add_named(&widgets::image_frame(&wipe_host), Some("comparison"));

        // `LOCAL ---|--- MATCH` under the frame (mockup `.wipe-slider`).
        let wipe_slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        wipe_slider.set_value(50.0);
        wipe_slider.set_draw_value(false);
        wipe_slider.set_hexpand(true);
        wipe_slider.add_css_class("wipe-slider");
        wipe_slider.set_tooltip_text(Some("Drag to sweep the wipe across the pictures."));
        {
            let w = wipe.clone();
            wipe_slider.connect_value_changed(move |s| w.set_position(s.value() / 100.0));
        }
        let slider_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        for text in ["Local", "Match"] {
            let l = gtk::Label::new(Some(text));
            l.add_css_class("wipe-end-label");
            l.add_css_class("caps");
            if text == "Local" {
                slider_row.append(&l);
                slider_row.append(&wipe_slider);
            } else {
                slider_row.append(&l);
            }
        }
        slider_row.set_visible(false);
        let error_detail = widgets::hint("");
        error_detail.set_halign(gtk::Align::Center);
        // One line, cut in the middle (the path's two ends are what tell it apart); the whole text
        // is the tooltip and the selection (DAN-2340, design gate 2.5).
        error_detail.set_wrap(false);
        error_detail.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        error_detail.set_selectable(true);
        error_detail.set_visible(false);

        let note = widgets::hint("");
        note.set_halign(gtk::Align::Center);
        note.set_justify(gtk::Justification::Center);
        note.set_visible(false);

        let compare_col = gtk::Box::new(gtk::Orientation::Vertical, widgets::QUEUE_BODY_GAP);
        compare_col.set_hexpand(true);
        compare_col.append(&header);
        compare_col.append(&readout_row);
        compare_col.append(&stack);
        compare_col.append(&slider_row);
        compare_col.append(&note);
        compare_col.append(&error_detail);

        // -- the decision card
        let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
        rail.set_size_request(RAIL_WIDTH, -1);
        rail.add_css_class("review-rail");

        let match_nav = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        match_nav.add_css_class("match-nav");
        let match_label = gtk::Label::new(Some("No match"));
        match_label.add_css_class("match-label");
        match_label.add_css_class("caps");
        match_label.set_xalign(0.0);
        match_label.set_hexpand(true);
        match_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        match_nav.append(&match_label);
        let prev_match_btn = widgets::icon_button("\u{25c0}", "Previous match candidate ([)", true);
        let next_match_btn = widgets::icon_button("\u{25b6}", "Next match candidate (])", true);
        let prev_match_key = widgets::keycap(&key_for("prev_candidate"));
        let next_match_key = widgets::keycap(&key_for("next_candidate"));
        for w in [
            prev_match_btn.upcast_ref::<gtk::Widget>(),
            prev_match_key.upcast_ref(),
            next_match_btn.upcast_ref(),
            next_match_key.upcast_ref(),
        ] {
            match_nav.append(w);
        }
        rail.append(&match_nav);

        let candidate_model = gtk::StringList::new(&[]);
        let candidate_combo =
            gtk::DropDown::new(Some(candidate_model.clone()), gtk::Expression::NONE);
        candidate_combo.add_css_class("candidate-picker");
        candidate_combo.set_sensitive(false);
        candidate_combo.set_hexpand(true);
        rail.append(&candidate_combo);

        rail.append(&widgets::section_label("02 // Tags"));
        let tag_list = gtk::ListBox::new();
        tag_list.add_css_class("tag-list");
        tag_list.set_selection_mode(gtk::SelectionMode::Multiple);
        tag_list.set_activate_on_single_click(false);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&tag_list));
        scroller.set_vexpand(true);
        scroller.set_min_content_height(72);
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.add_css_class("tag-scroller");
        rail.append(&scroller);

        let tag_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        tag_row.set_homogeneous(true);
        let add_tags_btn = widgets::pill_button("Add tags\u{2026}", false, false);
        let remove_tags_btn = widgets::pill_button("Remove", false, false);
        let edit_tags_btn = gtk::ToggleButton::with_label("Edit Tags");
        edit_tags_btn.add_css_class("pill");
        edit_tags_btn.add_css_class("plain");
        // "Editing Tags (click to stop)" elides rather than widen the rail.
        if let Some(l) = edit_tags_btn.child().and_downcast::<gtk::Label>() {
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            l.set_width_chars(0);
        }
        edit_tags_btn.set_size_request(1, -1);
        edit_tags_btn.set_tooltip_text(Some(
            "When on, double-click a tag to edit it in place (namespace:name, or just name). \
             Clear a tag's text entirely to delete it. Its source (User/Booru/etc.) is kept \
             as-is. Your Tag Namespaces remap rules (Settings) apply to what you type here too.",
        ));
        for b in [
            add_tags_btn.upcast_ref::<gtk::Widget>(),
            remove_tags_btn.upcast_ref(),
            edit_tags_btn.upcast_ref(),
        ] {
            tag_row.append(b);
        }
        rail.append(&tag_row);
        rail.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let send_btn = Keyed::new("Send to Hydrus", &key_for("send_upload"), true);
        send_btn
            .button
            .set_tooltip_text(Some("Send the file, its URL and its tags to Hydrus."));
        rail.append(&send_btn.button);
        let send_caption = gtk::Label::new(Some(COMMIT_CAPTION));
        send_caption.add_css_class("commit-caption");
        send_caption.add_css_class("caps");
        send_caption.set_wrap(true);
        send_caption.set_justify(gtk::Justification::Center);
        rail.append(&send_caption);

        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(8);
        grid.set_column_homogeneous(true);
        let url_btn = Keyed::new("Send URL", &key_for("send_url"), false);
        url_btn
            .button
            .set_tooltip_text(Some("Hand the URL to Hydrus's own downloader."));
        let download_btn = Keyed::new("Download", &key_for("download_send"), false);
        download_btn.button.set_tooltip_text(Some(
            "Download the full-resolution match and send that to Hydrus with the tags.",
        ));
        let mark_btn = Keyed::new("Mark reviewed", &key_for("toggle_reviewed"), false);
        mark_btn.button.set_tooltip_text(Some(
            "Records that you looked at this one and decided against sending it.\n\nSeparate from \
             Sent because both are decisions, and a row with neither mark is one nobody has got to \
             yet - which is what makes a long review pass resumable.",
        ));
        let open_btn = Keyed::new("Open match", &key_for("open_match"), false);
        open_btn
            .button
            .set_tooltip_text(Some("Open the matched page in your browser."));
        grid.attach(&url_btn.button, 0, 0, 1, 1);
        grid.attach(&download_btn.button, 1, 0, 1, 1);
        grid.attach(&mark_btn.button, 0, 1, 1, 1);
        grid.attach(&open_btn.button, 1, 1, 1, 1);
        rail.append(&grid);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, widgets::PAGE_GUTTER / 2);
        body.set_vexpand(true);
        body.append(&compare_col);
        body.append(&rail);

        // -- the position strip
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let prev_btn = Keyed::new("\u{25c2} Previous", &key_for("prev_row"), false);
        prev_btn.button.set_tooltip_text(Some("Previous image."));
        let hint_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hint_slot.set_hexpand(true);
        let hint_bar = KeyHintBar::new(&[]);
        hint_slot.append(&hint_bar.root);
        let next_unreviewed_btn = Keyed::new("Next unreviewed", &key_for("next_unreviewed"), false);
        next_unreviewed_btn.button.set_tooltip_text(Some(
            "Skip ahead to the next image you haven't sent or marked reviewed.",
        ));
        let next_btn = Keyed::new("Next \u{25b8}", &key_for("next_row"), false);
        next_btn.button.set_tooltip_text(Some("Next image."));
        strip.append(&prev_btn.button);
        strip.append(&hint_slot);
        strip.append(&next_unreviewed_btn.button);
        strip.append(&next_btn.button);
        let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
        rule.add_css_class("footer-rule");
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        footer.append(&rule);
        footer.append(&strip);

        let root = gtk::Box::new(gtk::Orientation::Vertical, widgets::QUEUE_BODY_GAP);
        root.append(&widgets::screen_title("Review."));
        root.append(&body);
        root.append(&footer);
        // The review keys are bound to this page as well as the table, so it can take focus when
        // the mode is entered (`setFocusPolicy(ClickFocus)`).
        root.set_focusable(true);
        root.set_focus_on_click(false);

        let page = Rc::new(ReviewPage {
            root,
            stack,
            title: title.clone(),
            view_buttons,
            readout_root,
            header: header.clone(),
            spacer: spacer.clone(),
            readout_row,
            compare_col: compare_col.clone(),
            readout_glyph,
            readout_value,
            readout_label,
            readout_diff,
            readout_tier: RefCell::new(String::new()),
            readout_generation: Cell::new(0),
            pop_out_btn,
            shortcuts_btn,
            show_in_queue_btn,
            local,
            matched,
            local_caption,
            matched_caption,
            matched_note,
            note,
            wipe,
            wipe_slider,
            slider_row,
            error_detail,
            zoom_tools,
            zoom_label,
            match_nav,
            match_label,
            prev_match_btn,
            next_match_btn,
            prev_match_key,
            next_match_key,
            candidate_model,
            candidate_combo,
            combo_blocked: Cell::new(false),
            tag_list,
            add_tags_btn,
            remove_tags_btn,
            edit_tags_btn,
            send_btn,
            send_caption,
            url_btn,
            download_btn,
            mark_btn,
            open_btn,
            prev_btn,
            next_unreviewed_btn,
            next_btn,
            hint_slot,
            hint_bar: RefCell::new(hint_bar),
        });
        let weak = Rc::downgrade(&page);
        let seen = Cell::new((-1, 0u32));
        page.compare_col.add_tick_callback(move |col, _| {
            if let Some(page) = weak.upgrade() {
                let key = (col.width(), page.readout_generation.get());
                if key != seen.get() {
                    seen.set(key);
                    page.fit_readout();
                }
            }
            glib::ControlFlow::Continue
        });
        page
    }

    /// `set_comparison_readout`: a part with nothing to say is hidden, and with nothing at all
    /// the whole readout is.
    pub fn set_readout(&self, r: &hatate_core::preview_text::ComparisonReadout) {
        for (label, text) in [
            (&self.readout_glyph, &r.glyph),
            (&self.readout_value, &r.value),
            (&self.readout_label, &r.label),
            (&self.readout_diff, &r.diff),
        ] {
            label.set_text(text);
            label.set_visible(!text.is_empty());
        }
        let old = self.readout_tier.replace(r.tier.to_owned());
        if !old.is_empty() {
            self.readout_diff.remove_css_class(&format!("tier-{old}"));
        }
        self.readout_diff.add_css_class(&format!("tier-{}", r.tier));
        self.readout_root
            .set_tooltip_text(Some(if r.tooltip.is_empty() {
                READOUT_TOOLTIP
            } else {
                &r.tooltip
            }));
        self.readout_root.set_visible(
            [&r.glyph, &r.value, &r.label, &r.diff]
                .iter()
                .any(|t| !t.is_empty()),
        );
        // The row under the header follows now, not a frame later when `fit_readout` next runs: a
        // readout set while it is there would otherwise sit in a hidden row for that frame.
        self.readout_row
            .set_visible(!self.readout_in_header() && self.readout_root.get_visible());
        self.readout_generation
            .set(self.readout_generation.get() + 1);
    }

    /// Whether the readout sits in the header row (`true`) or on the line under it.
    pub fn readout_in_header(&self) -> bool {
        self.readout_root.parent().as_ref() == Some(self.header.upcast_ref())
    }

    /// The mockup puts the readout in the header beside the tools; at the window's minimum width
    /// that does not fit, and the readout takes the line under the header instead.
    pub fn fit_readout(&self) {
        let nat = |w: &gtk::Box| w.measure(gtk::Orientation::Horizontal, -1).1;
        let inside = self.readout_in_header();
        let need = if inside {
            nat(&self.header)
        } else {
            nat(&self.header) + self.header.spacing() + nat(&self.readout_root)
        };
        let fits = self.compare_col.width() >= need;
        if fits && !inside {
            self.readout_row.remove(&self.readout_root);
            self.header
                .insert_child_after(&self.readout_root, Some(&self.spacer));
        } else if !fits && inside {
            self.header.remove(&self.readout_root);
            self.readout_row.append(&self.readout_root);
        }
        // `get_visible`, not `is_visible`: the latter also asks the row, which would keep itself hidden.
        self.readout_row
            .set_visible(!self.readout_in_header() && self.readout_root.get_visible());
    }

    /// The footer's hint bar, rebuilt from `groups` (`KeyHintBar.set_groups`).
    pub fn set_hint_groups(&self, groups: &[Vec<Piece>]) {
        let fresh = KeyHintBar::new(groups);
        let old = self.hint_bar.replace(fresh.clone());
        self.hint_slot.remove(&old.root);
        self.hint_slot.append(&fresh.root);
    }

    /// The hint bar on screen (a test reads its groups).
    pub fn hint_bar(&self) -> Rc<KeyHintBar> {
        self.hint_bar.borrow().clone()
    }

    /// `review_view_buttons[key]` that is checked.
    pub fn current_view(&self) -> String {
        self.view_buttons
            .iter()
            .find(|(_, b)| b.is_active())
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| "pair".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::key_wraps;

    /// A row with no width yet is not too narrow for its key (DAN-2219, design read).
    #[test]
    fn a_row_with_no_width_yet_does_not_wrap_its_key() {
        assert!(!key_wraps(120, 0), "width 0 is not allocated, not narrow");
        assert!(key_wraps(120, 119), "too narrow for both: the key drops");
        assert!(!key_wraps(120, 120), "exactly enough: the key stays beside");
    }
}
