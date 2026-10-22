//! The Review view: local and match images drawn by a custom `gtk::Widget`
//! (side by side, wipe, differences), a wipe slider and a mode switch.

use crate::diff::{self, Diff};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, gsk};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::path::Path;

/// How the two images are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    SideBySide,
    Wipe,
    Differences,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "side" => Some(Mode::SideBySide),
            "wipe" => Some(Mode::Wipe),
            "diff" => Some(Mode::Differences),
            _ => None,
        }
    }
}

/// One side of the comparison: decoded pixels, or why decoding failed.
pub type Loaded = Result<RgbaImage, String>;

/// Decode an image file; the error string is what the error state shows.
pub fn load(path: &Path) -> Loaded {
    image::open(path)
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn texture(im: &RgbaImage) -> gdk::Texture {
    let bytes = glib::Bytes::from(im.as_raw());
    gdk::MemoryTexture::new(
        im.width() as i32,
        im.height() as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        im.width() as usize * 4,
    )
    .upcast()
}

mod imp {
    use super::*;

    #[derive(glib::Properties, Default)]
    #[properties(wrapper_type = super::CompareCanvas)]
    pub struct CompareCanvas {
        /// Wipe position, 0.0 (all match) to 1.0 (all local).
        #[property(get, set = Self::set_split, minimum = 0.0, maximum = 1.0, default = 0.5)]
        pub split: Cell<f64>,
        pub mode: Cell<Mode>,
        pub local: RefCell<Option<gdk::Texture>>,
        pub matched: RefCell<Option<gdk::Texture>>,
        pub diff: RefCell<Option<gdk::Texture>>,
        pub local_err: RefCell<Option<String>>,
        pub match_err: RefCell<Option<String>>,
    }

    impl CompareCanvas {
        fn set_split(&self, v: f64) {
            self.split.set(v.clamp(0.0, 1.0));
            self.obj().queue_draw();
        }

        fn paint_text(&self, snapshot: &gtk4::Snapshot, text: &str, area: &graphene::Rect) {
            let widget = self.obj();
            let layout = widget.create_pango_layout(Some(text));
            layout.set_width(
                (area.width() * gtk4::pango::SCALE as f32) as i32 - 32 * gtk4::pango::SCALE,
            );
            layout.set_wrap(gtk4::pango::WrapMode::WordChar);
            layout.set_alignment(gtk4::pango::Alignment::Center);
            let (_, h) = layout.pixel_size();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                area.x() + 16.0,
                area.y() + (area.height() - h as f32) / 2.0,
            ));
            snapshot.append_layout(&layout, &widget.color());
            snapshot.restore();
        }

        fn paint_image(
            &self,
            snapshot: &gtk4::Snapshot,
            tex: &gdk::Texture,
            area: &graphene::Rect,
        ) {
            snapshot.append_scaled_texture(tex, gsk::ScalingFilter::Linear, &fit(tex, area));
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CompareCanvas {
        const NAME: &'static str = "HatateCompareCanvas";
        type Type = super::CompareCanvas;
        type ParentType = gtk4::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("compare-canvas");
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CompareCanvas {
        fn constructed(&self) {
            self.parent_constructed();
            self.split.set(0.5);
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.add_css_class("canvas");
        }
    }

    impl WidgetImpl for CompareCanvas {
        fn measure(&self, _o: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (240, 480, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let full = graphene::Rect::new(0.0, 0.0, obj.width() as f32, obj.height() as f32);
            let area = full.inset_r(8.0, 8.0);
            let (local, matched) = (self.local.borrow(), self.matched.borrow());
            let (lerr, merr) = (self.local_err.borrow(), self.match_err.borrow());
            let mode = self.mode.get();

            // A side that failed to load cannot be wiped or diffed: say so once.
            if mode != Mode::SideBySide && (lerr.is_some() || merr.is_some()) {
                let msg = lerr.as_deref().or(merr.as_deref()).unwrap_or_default();
                self.paint_text(snapshot, &format!("Cannot compare\n{msg}"), &area);
                return;
            }
            match mode {
                Mode::SideBySide => {
                    let half = (area.width() - 16.0) / 2.0;
                    let l = graphene::Rect::new(area.x(), area.y(), half, area.height());
                    let r =
                        graphene::Rect::new(area.x() + half + 16.0, area.y(), half, area.height());
                    for (tex, err, rect, side) in [
                        (&*local, &*lerr, &l, "local"),
                        (&*matched, &*merr, &r, "match"),
                    ] {
                        match (tex, err) {
                            (_, Some(e)) => self.paint_text(
                                snapshot,
                                &format!("Could not load {side} image\n{e}"),
                                rect,
                            ),
                            (Some(t), None) => self.paint_image(snapshot, t, rect),
                            _ => {}
                        }
                    }
                }
                Mode::Wipe => {
                    let (Some(l), Some(m)) = (&*local, &*matched) else {
                        return;
                    };
                    let r = fit(l, &area);
                    snapshot.append_scaled_texture(m, gsk::ScalingFilter::Linear, &r);
                    let x = r.x() + r.width() * self.split.get() as f32;
                    snapshot.push_clip(&graphene::Rect::new(r.x(), r.y(), x - r.x(), r.height()));
                    snapshot.append_scaled_texture(l, gsk::ScalingFilter::Linear, &r);
                    snapshot.pop();
                    let ink = gdk::RGBA::new(0.804, 0.769, 0.729, 1.0);
                    snapshot
                        .append_color(&ink, &graphene::Rect::new(x - 1.0, r.y(), 2.0, r.height()));
                    let cy = r.y() + r.height() / 2.0;
                    snapshot
                        .append_color(&ink, &graphene::Rect::new(x - 10.0, cy - 16.0, 20.0, 32.0));
                }
                Mode::Differences => {
                    let Some(d) = &*self.diff.borrow() else {
                        return;
                    };
                    self.paint_image(snapshot, d, &area);
                }
            }
        }
    }
}

/// Largest rectangle of the texture's aspect ratio centred in `area`.
fn fit(tex: &gdk::Texture, area: &graphene::Rect) -> graphene::Rect {
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    let s = (area.width() / tw).min(area.height() / th);
    let (w, h) = (tw * s, th * s);
    graphene::Rect::new(
        area.x() + (area.width() - w) / 2.0,
        area.y() + (area.height() - h) / 2.0,
        w,
        h,
    )
}

glib::wrapper! {
    /// Draws the local and match images in one of three modes.
    pub struct CompareCanvas(ObjectSubclass<imp::CompareCanvas>) @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl CompareCanvas {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn set_mode(&self, mode: Mode) {
        self.imp().mode.set(mode);
        self.queue_draw();
    }

    /// Replace both sides. Returns the diff stats when both decoded.
    pub fn set_images(&self, local: Loaded, matched: Loaded) -> Option<Diff> {
        let imp = self.imp();
        let mut stats = None;
        *imp.diff.borrow_mut() = None;
        *imp.local.borrow_mut() = local.as_ref().ok().map(texture);
        *imp.matched.borrow_mut() = matched.as_ref().ok().map(texture);
        *imp.local_err.borrow_mut() = local.as_ref().err().cloned();
        *imp.match_err.borrow_mut() = matched.as_ref().err().cloned();
        if let (Ok(a), Ok(b)) = (&local, &matched) {
            let d = diff::abs_diff(a, b);
            *imp.diff.borrow_mut() = Some(texture(&d.image));
            stats = Some(d);
        }
        self.queue_draw();
        stats
    }
}

impl Default for CompareCanvas {
    fn default() -> Self {
        Self::new()
    }
}

/// The Review page: mode switch, canvas, wipe slider, explanatory note.
pub struct Review {
    pub root: gtk4::Box,
    pub canvas: CompareCanvas,
    pub slider: gtk4::Scale,
    pub mode_buttons: Vec<(Mode, gtk4::ToggleButton)>,
    pub summary: gtk4::Label,
}

impl Review {
    pub fn new(local: Loaded, matched: Loaded, mode: Mode) -> Self {
        let canvas = CompareCanvas::new();
        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        root.set_vexpand(true);

        let top = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
        let eyebrow = gtk4::Label::new(Some("01 // COMPARE"));
        eyebrow.add_css_class("eyebrow");
        top.append(&eyebrow);
        let segs = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        segs.add_css_class("linked");
        let mut mode_buttons: Vec<(Mode, gtk4::ToggleButton)> = Vec::new();
        for (m, label) in [
            (Mode::SideBySide, "SIDE BY SIDE"),
            (Mode::Wipe, "WIPE"),
            (Mode::Differences, "DIFFERENCES"),
        ] {
            let b = gtk4::ToggleButton::with_label(label);
            b.add_css_class("mode");
            b.set_active(m == mode);
            if let Some((_, first)) = mode_buttons.first() {
                b.set_group(Some(first));
            }
            let canvas = canvas.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    canvas.set_mode(m);
                }
            });
            segs.append(&b);
            mode_buttons.push((m, b));
        }
        top.append(&segs);
        let summary = gtk4::Label::new(None);
        summary.add_css_class("stat");
        summary.set_hexpand(true);
        summary.set_halign(gtk4::Align::End);
        top.append(&summary);
        root.append(&top);
        root.append(&canvas);

        let slider_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let (l, r) = (
            gtk4::Label::new(Some("LOCAL")),
            gtk4::Label::new(Some("MATCH")),
        );
        l.add_css_class("eyebrow");
        r.add_css_class("eyebrow");
        let slider = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.0, 0.01);
        slider.set_draw_value(false);
        slider.set_hexpand(true);
        slider.set_value(0.5);
        slider
            .adjustment()
            .bind_property("value", &canvas, "split")
            .bidirectional()
            .sync_create()
            .build();
        slider_row.append(&l);
        slider_row.append(&slider);
        slider_row.append(&r);
        root.append(&slider_row);

        let note = gtk4::Label::new(Some(
            "Local on the left, match on the right. Drag the divider, or use the slider, to sweep one over the other.",
        ));
        note.add_css_class("note");
        note.set_wrap(true);
        root.append(&note);

        // Dragging on the canvas moves the divider too.
        let drag = gtk4::GestureDrag::new();
        let start = std::rc::Rc::new(Cell::new(0.0));
        {
            let (c, s) = (canvas.clone(), start.clone());
            drag.connect_drag_begin(move |_, x, _| {
                s.set(x);
                c.set_split(x / c.width().max(1) as f64);
            });
        }
        {
            let (c, s) = (canvas.clone(), start);
            drag.connect_drag_update(move |_, dx, _| {
                c.set_split((s.get() + dx) / c.width().max(1) as f64);
            });
        }
        canvas.add_controller(drag);
        // Keyboard: arrows nudge the divider when the canvas has focus.
        let keys = gtk4::EventControllerKey::new();
        {
            let c = canvas.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                let step = match key {
                    gdk::Key::Left => -0.02,
                    gdk::Key::Right => 0.02,
                    _ => return glib::Propagation::Proceed,
                };
                c.set_split(c.split() + step);
                glib::Propagation::Stop
            });
        }
        canvas.add_controller(keys);

        let this = Review {
            root,
            canvas,
            slider,
            mode_buttons,
            summary,
        };
        this.set_images(local, matched, mode);
        this
    }

    pub fn set_images(&self, local: Loaded, matched: Loaded, mode: Mode) {
        let stats = self.canvas.set_images(local, matched);
        self.canvas.set_mode(mode);
        self.summary.set_text(&match stats {
            Some(d) => format!("{} px differ \u{00B7} max \u{0394} {}", d.changed, d.max),
            None => "load failed".to_string(),
        });
    }
}
