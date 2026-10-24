//! The shared widgets (`gui/widgets.py` at `76c85c4`): the parts 6.c's own screens use.
//!
//! GTK CSS has no `text-transform` that this module can rely on across the toolkit versions in
//! play, so an uppercase voice is a style class (`caps`) the sheet maps to `text-transform`, and
//! the label keeps the sentence-case string Python passes (the accessible name stays "Hatate").
//! Corner marks and the ghost kanji are small widgets in an overlay, since GTK CSS has no
//! pseudo-elements (design gate 2.9).

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

// Layout constants (`gui/widgets.py:24-67`), in px.
pub const CARD_MARGINS: (i32, i32, i32, i32) = (18, 16, 18, 16);
pub const CARD_SPACING: i32 = 10;
pub const PAGE_MARGINS: (i32, i32, i32, i32) = (22, 18, 22, 18);
pub const PAGE_GUTTER: i32 = 32;
pub const SHELL_MARGINS: (i32, i32, i32, i32) = (PAGE_GUTTER, 24, PAGE_GUTTER, 24);
pub const SHELL_SPACING: i32 = 16;
pub const QUEUE_BODY_GAP: i32 = 16;
pub const SCREEN_TITLE_GAP: i32 = 4;
pub const MENUBAR_HEIGHT: i32 = 28;
pub const STATUSBAR_HEIGHT: i32 = 24;
pub const PAGE_SPACING: i32 = 14;
pub const BUTTON_MIN_HEIGHT: i32 = 40;
pub const ICON_BOX: i32 = 36;
pub const BUTTON_ROW_SPACING: i32 = 12;
pub const RUN_BANNER_PROSE_WIDTH: i32 = 840;
pub const RUN_BANNER_GAP: i32 = 24;
pub const RUN_BANNER_GLYPH_COLUMN: i32 = 17;
pub const EMPTY_PROSE_WIDTH: i32 = 560;
pub const SURVIVED_GLYPH_BOX: i32 = 42;
pub const BRACKET_ARM: i32 = 12;
pub const BRACKET_WEIGHT: i32 = 2;
pub const GHOST_SIZE: i32 = 420;

/// `card(title, framed)`: a region of the page with an optional heading. The content goes into
/// the box itself.
pub fn card(title: Option<&str>, framed: bool) -> gtk::Box {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, CARD_SPACING);
    frame.add_css_class("card");
    if framed {
        frame.add_css_class("framed");
    }
    let (l, t, r, b) = CARD_MARGINS;
    frame.set_margin_start(l);
    frame.set_margin_top(t);
    frame.set_margin_end(r);
    frame.set_margin_bottom(b);
    if let Some(title) = title {
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("heading");
        heading.set_xalign(0.0);
        frame.append(&heading);
    }
    frame
}

/// An uppercase, tracked label in the given style class.
pub fn caps_label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class(class);
    label.add_css_class("caps");
    label.set_xalign(0.0);
    label
}

/// `section_label("01 // Filter")`.
pub fn section_label(text: &str) -> gtk::Label {
    caps_label(text, "section-label")
}

/// `section_header(text, *trailing)`: a section label with a hairline to the right edge, then the
/// trailing widgets.
pub fn section_header(text: &str, trailing: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.append(&section_label(text));
    let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
    rule.add_css_class("section-rule");
    rule.set_hexpand(true);
    rule.set_valign(gtk::Align::Center);
    row.append(&rule);
    for w in trailing {
        row.append(*w);
    }
    row
}

pub fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("heading");
    l.set_xalign(0.0);
    l
}

/// `screen_title("Queue.")`.
pub fn screen_title(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("screen-title");
    l.set_xalign(0.0);
    l
}

/// `display_title("Drop images here.")`: the screen-title face at the display size.
pub fn display_title(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("display-title");
    l
}

pub fn muted(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("muted");
    l.set_xalign(0.0);
    l
}

pub fn hint(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("hint");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

/// `pill_button`: `primary` gives the flat ink-stamp fill; page-level buttons speak the uppercase
/// mono voice, dialogs pass `uppercase = false`.
pub fn pill_button(text: &str, primary: bool, uppercase: bool) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("pill");
    if primary {
        b.add_css_class("primary");
    }
    if uppercase {
        b.add_css_class("caps");
        b.set_height_request(BUTTON_MIN_HEIGHT);
    } else {
        b.add_css_class("plain");
    }
    b.set_cursor_from_name(Some("pointer"));
    b
}

/// `icon_button(glyph, tooltip, boxed)`.
pub fn icon_button(glyph: &str, tooltip: &str, boxed: bool) -> gtk::Button {
    let b = gtk::Button::with_label(glyph);
    if boxed {
        b.add_css_class("icon-boxed");
        b.set_size_request(ICON_BOX, ICON_BOX);
    } else {
        b.add_css_class("flat");
    }
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    b
}

/// `link_button(text)`: text that acts like a button (`Discard & start fresh`).
pub fn link_button(text: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("link");
    b.add_css_class("caps");
    b.set_cursor_from_name(Some("pointer"));
    b
}

/// `KeyCap`: a key name in a hairline box. The capitals are the sheet's, so the text stays the
/// key as the registry spells it ("Return", "Space").
pub fn keycap(label: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(label));
    l.add_css_class("keycap");
    l.add_css_class("caps");
    l.set_halign(gtk::Align::Center);
    l.set_valign(gtk::Align::Center);
    l
}

/// A button that carries its key as a keycap: `SEND URL  [U]`. Returns the button and the cap.
pub fn keyed_button(text: &str, key: &str, primary: bool) -> (gtk::Button, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some(text));
    label.add_css_class("caps");
    label.set_hexpand(true);
    label.set_xalign(0.0);
    row.append(&label);
    let cap = keycap(key);
    cap.set_visible(!key.is_empty());
    row.append(&cap);
    let b = gtk::Button::new();
    b.set_child(Some(&row));
    b.add_css_class("pill");
    if primary {
        b.add_css_class("primary");
    }
    b.set_height_request(BUTTON_MIN_HEIGHT);
    b.set_cursor_from_name(Some("pointer"));
    b.update_property(&[gtk::accessible::Property::Label(text)]);
    (b, cap)
}

/// A piece of a key hint: text, a bold figure, or a keycap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Strong(String),
    Key(String),
}

/// `KeyHintBar`: the line of keys along the bottom of Review: `1 OF 8 · [J]/[K] MOVE`. Built from
/// groups so it can lose its tail, not its head: when the bar is narrower than the groups need,
/// the last groups are hidden until the rest fit.
pub struct KeyHintBar {
    pub root: gtk::Box,
    groups: Vec<gtk::Box>,
    separators: Vec<gtk::Label>,
}

impl KeyHintBar {
    pub fn new(groups: &[Vec<Piece>]) -> Rc<KeyHintBar> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("hint-bar");
        let mut boxes = Vec::new();
        let mut seps = Vec::new();
        for (i, group) in groups.iter().enumerate() {
            if i > 0 {
                let dot = gtk::Label::new(Some("\u{00B7}"));
                root.append(&dot);
                seps.push(dot);
            }
            let g = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            for piece in group {
                match piece {
                    Piece::Text(t) => {
                        let l = gtk::Label::new(Some(t));
                        l.add_css_class("caps");
                        g.append(&l)
                    }
                    Piece::Strong(t) => {
                        let l = gtk::Label::new(Some(t));
                        l.add_css_class("strong");
                        l.add_css_class("caps");
                        g.append(&l)
                    }
                    Piece::Key(k) => g.append(&keycap(k)),
                }
            }
            root.append(&g);
            boxes.push(g);
        }
        let bar = Rc::new(KeyHintBar {
            root,
            groups: boxes,
            separators: seps,
        });
        let weak = Rc::downgrade(&bar);
        let last = Rc::new(Cell::new(-1));
        bar.root.add_tick_callback(move |root, _| {
            if let Some(bar) = weak.upgrade() {
                let w = root.width();
                if w != last.get() {
                    last.set(w);
                    bar.fit(w);
                }
            }
            glib::ControlFlow::Continue
        });
        bar
    }

    /// Hides trailing groups until the bar fits `width` px; the first group always stays.
    pub fn fit(&self, width: i32) {
        for g in &self.groups {
            g.set_visible(true);
        }
        for s in &self.separators {
            s.set_visible(true);
        }
        let need = |upto: usize| -> i32 {
            let mut total = 0;
            for (i, g) in self.groups.iter().enumerate().take(upto) {
                total += g.measure(gtk::Orientation::Horizontal, -1).1;
                if i > 0 {
                    total += 8 + 8 + 6;
                }
            }
            total
        };
        let mut shown = self.groups.len();
        while shown > 1 && need(shown) > width {
            shown -= 1;
        }
        for (i, g) in self.groups.iter().enumerate() {
            g.set_visible(i < shown);
        }
        for (i, s) in self.separators.iter().enumerate() {
            s.set_visible(i + 1 < shown);
        }
    }

    /// How many groups are showing (tests, and the shot harness).
    pub fn visible_groups(&self) -> usize {
        self.groups.iter().filter(|g| g.is_visible()).count()
    }
}

/// `segmented(choices, on_change, current)`: the mode switch, a row of mutually exclusive buttons.
/// `choices` are `(key, label, tooltip)`. Returns the row and the buttons by key.
pub fn segmented(
    choices: &[(&str, &str, &str)],
    current: &str,
    on_change: impl Fn(&str) + 'static,
) -> (gtk::Box, BTreeMap<String, gtk::ToggleButton>) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("mode-switch");
    let on_change = Rc::new(on_change);
    let mut buttons: BTreeMap<String, gtk::ToggleButton> = BTreeMap::new();
    let mut first: Option<gtk::ToggleButton> = None;
    for (key, label, tip) in choices {
        let b = gtk::ToggleButton::with_label(label);
        b.add_css_class("caps");
        b.set_tooltip_text(Some(tip));
        b.set_active(*key == current);
        match &first {
            Some(f) => b.set_group(Some(f)),
            None => first = Some(b.clone()),
        }
        let key_owned = (*key).to_owned();
        let cb = on_change.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                cb(&key_owned);
            }
        });
        row.append(&b);
        buttons.insert((*key).to_owned(), b);
    }
    (row, buttons)
}

/// `chip(text, on)`: an outlined hairline tag.
pub fn chip(text: &str, on: bool) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("chip");
    l.add_css_class("caps");
    if on {
        l.add_css_class("on");
    }
    l
}

/// `disclosure(title)`: a header button that shows and hides a body. Returns the box, the body
/// and the button.
pub fn disclosure(title: &str, open: bool) -> (gtk::Box, gtk::Box, gtk::Button) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let marker = |open: bool| if open { "\u{25BE}" } else { "\u{25B8}" };
    let button = gtk::Button::with_label(&format!("{} {title}", marker(open)));
    button.add_css_class("disclosure");
    button.add_css_class("caps");
    button.set_halign(gtk::Align::Start);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    body.set_visible(open);
    let title = title.to_owned();
    {
        let body = body.clone();
        button.connect_clicked(move |b| {
            let now = !body.is_visible();
            body.set_visible(now);
            b.set_label(&format!("{} {title}", marker(now)));
        });
    }
    root.append(&button);
    root.append(&body);
    (root, body, button)
}

/// `Brackets(frame)`: four corner crop-marks over a framed region. The marks sit on the frame's
/// own hairline, 12 px arms 2 px thick, and take no input.
pub fn brackets(child: &impl IsA<gtk::Widget>) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(child));
    for (class, h, v) in [
        ("tl", gtk::Align::Start, gtk::Align::Start),
        ("tr", gtk::Align::End, gtk::Align::Start),
        ("bl", gtk::Align::Start, gtk::Align::End),
        ("br", gtk::Align::End, gtk::Align::End),
    ] {
        let corner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        corner.add_css_class("corner");
        corner.add_css_class(class);
        corner.set_halign(h);
        corner.set_valign(v);
        corner.set_can_target(false);
        corner.set_can_focus(false);
        overlay.add_overlay(&corner);
    }
    overlay
}

/// `image_frame(content)`: `content` inside the dossier frame, a hairline box with corner marks.
pub fn image_frame(content: &impl IsA<gtk::Widget>) -> gtk::Overlay {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("frame");
    frame.append(content);
    brackets(&frame)
}

/// `FrameCaption`: the mono line under an image frame: what the picture is at the left, its
/// dimensions, format and size at the right.
pub struct FrameCaption {
    pub root: gtk::Box,
    pub left: gtk::Label,
    pub right: gtk::Label,
}

impl FrameCaption {
    pub fn new(left: &str, right: &str) -> FrameCaption {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let l = gtk::Label::new(Some(left));
        let r = gtk::Label::new(Some(right));
        for label in [&l, &r] {
            label.add_css_class("frame-caption");
            label.add_css_class("caps");
        }
        // A caption's wording is data (a booru name, a ranking qualifier) and must not set the
        // window's minimum width: it elides instead.
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        l.set_xalign(0.0);
        l.set_hexpand(true);
        l.set_width_chars(0);
        r.set_xalign(1.0);
        root.append(&l);
        root.append(&r);
        FrameCaption {
            root,
            left: l,
            right: r,
        }
    }
}

/// The ghost kanji behind a page (`GhostKanji`): one very faint 420 px glyph, 40 px in from the
/// right and 40 px above the top, behind whatever is on the page. The content goes over it.
pub struct GhostKanji {
    pub root: gtk::Overlay,
    glyph: gtk::Label,
}

impl GhostKanji {
    pub fn new(glyph: &str, content: &impl IsA<gtk::Widget>) -> GhostKanji {
        let root = gtk::Overlay::new();
        // The glyph is the overlay's main child so the content is drawn over it. It takes no part
        // in the page's size (its natural size is huge) and no input.
        let label = gtk::Label::new(Some(glyph));
        label.add_css_class("ghost-kanji");
        label.set_halign(gtk::Align::End);
        label.set_valign(gtk::Align::Start);
        label.set_can_target(false);
        label.set_can_focus(false);
        label.set_overflow(gtk::Overflow::Hidden);
        let holder = gtk::ScrolledWindow::new();
        holder.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
        holder.set_child(Some(&label));
        holder.set_can_target(false);
        holder.set_can_focus(false);
        holder.set_min_content_width(0);
        holder.set_min_content_height(0);
        root.set_child(Some(&holder));
        root.add_overlay(content);
        root.set_measure_overlay(content, true);
        GhostKanji { root, glyph: label }
    }

    pub fn glyph(&self) -> String {
        self.glyph.text().to_string()
    }

    pub fn set_glyph(&self, glyph: &str) {
        if self.glyph.text() != glyph {
            self.glyph.set_text(glyph);
        }
    }
}

/// `meta_line(items, centered, anchor)`: a mono-caps row of `label  FIGURE` pairs, the figure
/// bright and bold against its dim label. `anchor` is the index of the one figure that stays
/// bright; the rest read as plain muted text. `None` bolds every figure.
pub fn meta_line(items: &[(String, String)], centered: bool, anchor: Option<usize>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    row.add_css_class("meta-line");
    if centered {
        row.set_halign(gtk::Align::Center);
    }
    for (i, (label, figure)) in items.iter().enumerate() {
        let pair = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let l = gtk::Label::new(Some(label));
        l.add_css_class("meta-label");
        l.add_css_class("caps");
        let f = gtk::Label::new(Some(figure));
        f.add_css_class("meta-figure");
        f.add_css_class("caps");
        if !(anchor.is_none() || anchor == Some(i)) {
            f.add_css_class("quiet");
        }
        pair.append(&l);
        pair.append(&f);
        row.append(&pair);
    }
    row
}

/// The pieces of a banner the caller fills in and wires.
pub struct BannerParts {
    pub glyph: gtk::Label,
    pub head: gtk::Label,
    pub body: gtk::Label,
    pub meta_slot: gtk::Box,
    pub actions: gtk::Box,
}

/// The frame both Queue banners share: a bracketed card with a status glyph, a mono kicker,
/// wrapped prose, a meta-line slot, and an empty row for the actions.
pub fn banner_shell(glyph_text: &str, head_text: &str) -> (gtk::Overlay, BannerParts) {
    let frame = gtk::Box::new(gtk::Orientation::Horizontal, RUN_BANNER_GAP);
    frame.add_css_class("banner");
    let glyph = gtk::Label::new(Some(glyph_text));
    glyph.add_css_class("banner-glyph");
    glyph.set_width_request(RUN_BANNER_GLYPH_COLUMN);
    glyph.set_valign(gtk::Align::Start);
    frame.append(&glyph);

    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    col.set_hexpand(true);
    let head = caps_label(head_text, "banner-head");
    col.append(&head);
    let body = gtk::Label::new(None);
    body.add_css_class("banner-body");
    body.set_wrap(true);
    body.set_xalign(0.0);
    body.set_halign(gtk::Align::Start);
    body.set_max_width_chars(0);
    body.set_width_request(0);
    col.append(&body);
    let meta_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.append(&meta_slot);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, BUTTON_ROW_SPACING);
    actions.set_margin_top(4);
    col.append(&actions);
    frame.append(&col);
    let overlay = brackets(&frame);
    (
        overlay,
        BannerParts {
            glyph,
            head,
            body,
            meta_slot,
            actions,
        },
    )
}

/// The Crashed banner's widgets (`run_banner`).
pub struct RunBanner {
    pub root: gtk::Overlay,
    pub parts: BannerParts,
    pub resume: gtk::Button,
    pub crash_log: gtk::Button,
    pub discard: gtk::Button,
}

/// `run_banner()`: the interrupted-run notice.
pub fn run_banner() -> RunBanner {
    let (root, parts) = banner_shell(
        "\u{25D0}",
        "Run interrupted \u{2014} partial results survived",
    );
    let resume = pill_button("Resume Queue", true, true);
    let crash_log = pill_button("View Crash Log", false, true);
    let discard = link_button("Discard & start fresh");
    parts.actions.append(&resume);
    parts.actions.append(&crash_log);
    parts.actions.append(&discard);
    RunBanner {
        root,
        parts,
        resume,
        crash_log,
        discard,
    }
}

/// The quota banner's widgets (`quota_pause_banner`).
pub struct QuotaBanner {
    pub root: gtk::Overlay,
    pub parts: BannerParts,
    pub review: gtk::Button,
    pub keep_going: gtk::Button,
}

/// `quota_pause_banner()`: the quota-paused notice.
pub fn quota_pause_banner() -> QuotaBanner {
    let (root, parts) = banner_shell(
        "\u{2026}",
        "Run paused \u{2014} SauceNAO's daily quota is exhausted",
    );
    let review = pill_button("Review the waiting \u{2192}", true, true);
    let keep_going = pill_button("Continue with other engines", false, true);
    parts.actions.append(&review);
    parts.actions.append(&keep_going);
    QuotaBanner {
        root,
        parts,
        review,
        keep_going,
    }
}

/// `empty_prose(text)`: the empty state's explanatory paragraph, centred, 560 px wide.
pub fn empty_prose(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("drop-copy");
    l.set_wrap(true);
    l.set_justify(gtk::Justification::Center);
    l.set_halign(gtk::Align::Center);
    l.set_max_width_chars(60);
    l.set_width_request(0);
    l
}

/// Everything `nothing_survived` hands back.
pub struct NothingSurvived {
    pub root: gtk::Overlay,
    pub new_search: gtk::Button,
    pub crash_log: gtk::Button,
    pub add_files: gtk::Button,
    pub title: gtk::Label,
    pub body: gtk::Label,
}

/// `nothing_survived(meta_items, below_weight)`: the Queue's "Nothing survived." variant of the
/// empty state. `below_weight` is the drop zone's top-weighting, so the two variants put their
/// block at the same height.
pub fn nothing_survived(meta_items: &[(String, String)], below_weight: u32) -> NothingSurvived {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("drop-zone");
    panel.set_margin_start(0);
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
    inner.set_margin_start(32);
    inner.set_margin_end(32);
    inner.set_margin_top(24);
    inner.set_margin_bottom(24);
    inner.set_vexpand(true);
    let section = section_label("01 // Queue");
    section.set_halign(gtk::Align::Start);
    inner.append(&section);
    let block = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let glyph = gtk::Label::new(Some("\u{2715}"));
    glyph.add_css_class("survived-glyph");
    glyph.set_halign(gtk::Align::Center);
    glyph.set_size_request(SURVIVED_GLYPH_BOX, SURVIVED_GLYPH_BOX);
    block.append(&glyph);
    let title = display_title("Nothing survived.");
    title.set_margin_top(10);
    block.append(&title);
    let body = empty_prose(
        "Hatate stopped without a clean exit, and the session it had saved was empty or could \
         not be read, so there is no queue to restore. Anything already sent to Hydrus is \
         unaffected.",
    );
    body.set_margin_top(10);
    block.append(&body);
    let meta = meta_line(meta_items, true, None);
    meta.set_margin_top(12);
    block.append(&meta);

    let new_search = pill_button("Start new search", true, true);
    let crash_log = pill_button("View crash log", false, true);
    let add_files = pill_button("Add files\u{2026}", false, true);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, BUTTON_ROW_SPACING);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(22);
    for b in [&new_search, &crash_log, &add_files] {
        buttons.append(b);
    }
    block.append(&buttons);
    inner.append(&top_weighted(&block, below_weight));
    panel.append(&inner);
    NothingSurvived {
        root: brackets(&panel),
        new_search,
        crash_log,
        add_files,
        title,
        body,
    }
}

/// `DROP_ZONE_BELOW_WEIGHT` (`gui/main_window.py:299`): the empty Queue's headline sits 1 : this of
/// the free height above : below it.
pub const DROP_ZONE_BELOW_WEIGHT: u32 = 6;

/// Qt's `addStretch(1)` / `addStretch(below_weight)` around a block: GTK has no stretch factors,
/// so the space above the block is set to `1 / (1 + below_weight)` of the free height whenever
/// the column's height changes, and the space below takes the rest.
pub fn top_weighted(block: &impl IsA<gtk::Widget>, below_weight: u32) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.set_vexpand(true);
    let above = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let below = gtk::Box::new(gtk::Orientation::Vertical, 0);
    below.set_vexpand(true);
    col.append(&above);
    col.append(block);
    col.append(&below);
    let block = block.clone().upcast::<gtk::Widget>();
    let last = Cell::new((-1, -1));
    col.add_tick_callback(move |col, _| {
        let h = col.height();
        let for_width = if block.width() > 0 { block.width() } else { -1 };
        let bh = block.measure(gtk::Orientation::Vertical, for_width).1;
        if (h, bh) != last.get() {
            last.set((h, bh));
            let free = (h - bh).max(0);
            above.set_height_request(free / (1 + below_weight as i32));
        }
        glib::ControlFlow::Continue
    });
    col
}
