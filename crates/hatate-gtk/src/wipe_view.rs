//! `WipeView` (`gui/compare_dialog.py` at `76c85c4`; DAN-2340): two pictures in one frame, split by a
//! movable vertical wipe, that zooms and pans. The maths is [`hatate_core::compare_view`]; this is
//! the drawing and the pointer and key handling around it.
//!
//! Both pictures are scaled into the same box first, so the comparison works though the two are
//! almost always different resolutions. Zoom applies to the shared target rectangle, not to either
//! picture, so the two stay registered with each other at every zoom level.

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{cairo, gdk};
use hatate_core::compare_view::{RectF, Rgba, WipeGeometry, visible_part};
use std::cell::RefCell;
use std::rc::Rc;

/// `WipeView.setMinimumSize(420, 420)` asks 886 px of height of the Review page, past its stated
/// minimum (1010 x 790, DAN-2025 line 2.5): the floor is the Side by side panes' (280 x 260), so the
/// stack asks no more for Wipe than it does for the pair.
pub const WIPE_MIN: (i32, i32) = (280, 260);

/// What the frame says where the match has no picture.
pub const MATCH_MISSING: &str = "The match's picture\ncould not be fetched";
/// What it says with nothing loaded.
pub const NOTHING_TO_COMPARE: &str = "Nothing to compare";

/// A picture as a cairo surface (premultiplied ARGB32), with the size it was made at.
#[derive(Clone)]
struct Surf {
    surface: cairo::ImageSurface,
    size: (f64, f64),
}

impl Surf {
    fn new(p: &Rgba) -> Option<Surf> {
        if p.width == 0 || p.height == 0 {
            return None;
        }
        let mut data = Vec::with_capacity(p.rgba.len());
        for px in p.rgba.chunks_exact(4) {
            let a = u32::from(px[3]);
            let pm = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
            // Little-endian ARGB32 is B, G, R, A in memory.
            data.extend_from_slice(&[pm(px[2]), pm(px[1]), pm(px[0]), px[3]]);
        }
        let stride = p.width as i32 * 4;
        let surface = cairo::ImageSurface::create_for_data(
            data,
            cairo::Format::ARgb32,
            p.width as i32,
            p.height as i32,
            stride,
        )
        .ok()?;
        Some(Surf {
            surface,
            size: (f64::from(p.width), f64::from(p.height)),
        })
    }
}

#[derive(Default)]
struct State {
    geom: WipeGeometry,
    left: Option<Surf>,
    right: Option<Surf>,
    diff_local: Option<Surf>,
    diff_match: Option<Surf>,
    diff_overlay: Option<Surf>,
    /// `position`, 0 to 1.
    position: f64,
    /// The pan a drag started from.
    drag_from: Option<(f64, f64)>,
}

/// The callback that reports the zoom.
type ZoomCallback = Rc<RefCell<Option<Rc<dyn Fn(f64)>>>>;

/// The widget, and the callback that reports the zoom.
#[derive(Clone)]
pub struct WipeView {
    pub area: gtk::DrawingArea,
    state: Rc<RefCell<State>>,
    on_zoom: ZoomCallback,
}

impl WipeView {
    pub fn new() -> WipeView {
        let area = gtk::DrawingArea::new();
        area.add_css_class("wipe-view");
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_size_request(WIPE_MIN.0, WIPE_MIN.1);
        area.set_focusable(true);
        area.set_cursor_from_name(Some("grab"));
        area.set_tooltip_text(Some(
            "Scroll to zoom where the pointer is, drag to pan, double-click to fit.\n\
             + / - zoom, 0 fits, 1 shows actual pixels.",
        ));
        let view = WipeView {
            area,
            state: Rc::new(RefCell::new(State {
                position: 0.5,
                ..State::default()
            })),
            on_zoom: Rc::new(RefCell::new(None)),
        };
        view.wire();
        view
    }

    /// `zoom_changed`: the scale as a percentage of the match's real pixels.
    pub fn connect_zoom_changed(&self, f: impl Fn(f64) + 'static) {
        *self.on_zoom.borrow_mut() = Some(Rc::new(f));
    }

    fn changed(&self) {
        self.area.queue_draw();
        self.report_zoom();
    }

    fn report_zoom(&self) {
        let percent = self.state.borrow().geom.zoom_percent();
        let cb = self.on_zoom.borrow().clone();
        if let Some(cb) = cb {
            cb(percent);
        }
    }

    // -- content --------------------------------------------------------------------------------

    /// `set_images`: a new pair starts framed, not wherever the last one was.
    pub fn set_images(&self, left: Option<&Rgba>, right: Option<&Rgba>) {
        {
            let mut s = self.state.borrow_mut();
            s.left = left.and_then(Surf::new);
            s.right = right.and_then(Surf::new);
            // `_reference()`: the local file wins where there is one.
            let reference = s.left.as_ref().or(s.right.as_ref()).map(|p| p.size);
            s.geom.set_reference(reference);
        }
        self.changed();
    }

    /// `set_difference_view`: both pictures side by side, `overlay` swept across both by the wipe.
    /// The pictures arrive already as the Differences ramp (`ramp_picture`, design gate 2.2).
    pub fn set_difference_view(&self, local: &Rgba, matched: &Rgba, overlay: &Rgba) {
        {
            let mut s = self.state.borrow_mut();
            s.diff_local = Surf::new(local);
            s.diff_match = Surf::new(matched);
            s.diff_overlay = Surf::new(overlay);
            let size = s.diff_local.as_ref().map(|p| p.size);
            s.geom.set_differences(size);
        }
        self.changed();
    }

    pub fn clear_difference_view(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.diff_local = None;
            s.diff_match = None;
            s.diff_overlay = None;
            s.geom.set_differences(None);
        }
        self.changed();
    }

    pub fn showing_differences(&self) -> bool {
        self.state.borrow().geom.showing_differences()
    }

    /// `set_position(fraction)`.
    pub fn set_position(&self, fraction: f64) {
        self.state.borrow_mut().position = fraction.clamp(0.0, 1.0);
        self.area.queue_draw();
    }

    pub fn position(&self) -> f64 {
        self.state.borrow().position
    }

    pub fn zoom_percent(&self) -> f64 {
        self.state.borrow().geom.zoom_percent()
    }

    /// `_zoom`: 1.0 is scaled to fit (the differential reads it).
    pub fn zoom_factor(&self) -> f64 {
        self.state.borrow().geom.zoom()
    }

    /// The widget as it paints now, at its allocated size: cairo's premultiplied ARGB32 bytes
    /// (B, G, R, A in memory) and the size. What a test samples, and what a shot would save.
    pub fn render(&self) -> Option<(Vec<u8>, i32, i32)> {
        let (w, h) = (self.area.width(), self.area.height());
        if w <= 0 || h <= 0 {
            return None;
        }
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w, h).ok()?;
        {
            let cr = cairo::Context::new(&surface).ok()?;
            self.paint(&self.area, &cr, f64::from(w), f64::from(h));
        }
        let stride = surface.stride() as usize;
        let data = surface.data().ok()?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h as usize {
            out.extend_from_slice(&data[row * stride..row * stride + w as usize * 4]);
        }
        Some((out, w, h))
    }

    /// Whether a picture of the pair is on show (a test reads it).
    pub fn has_pictures(&self) -> (bool, bool) {
        let s = self.state.borrow();
        (s.left.is_some(), s.right.is_some())
    }

    // -- zoom API -------------------------------------------------------------------------------

    pub fn zoom_in(&self) {
        let moved = self.state.borrow_mut().geom.zoom_in();
        if moved {
            self.changed();
        }
    }

    pub fn zoom_out(&self) {
        let moved = self.state.borrow_mut().geom.zoom_out();
        if moved {
            self.changed();
        }
    }

    pub fn zoom_to_fit(&self) {
        self.state.borrow_mut().geom.zoom_to_fit();
        self.changed();
    }

    pub fn zoom_to_actual(&self) {
        let moved = self.state.borrow_mut().geom.zoom_to_actual();
        if moved {
            self.changed();
        }
    }

    // -- interaction ----------------------------------------------------------------------------

    fn wire(&self) {
        let v = self.clone();
        self.area.connect_resize(move |_, w, h| {
            v.state
                .borrow_mut()
                .geom
                .set_widget_size(f64::from(w), f64::from(h));
            // The fit size just changed, so the same zoom now means another percentage.
            v.changed();
        });

        // `wheelEvent`: Qt's angle delta is 120 per notch, up positive; GTK's is 1 per notch, up negative.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let v = self.clone();
        scroll.connect_scroll(move |c, _, dy| {
            let anchor = c
                .current_event()
                .and_then(|e| e.position())
                .unwrap_or((0.0, 0.0));
            let moved = v.state.borrow_mut().geom.wheel(anchor, -dy * 120.0);
            if moved {
                v.changed();
            }
            gtk::glib::Propagation::Stop
        });
        self.area.add_controller(scroll);

        // Drag to pan; double-click to fit.
        let drag = gtk::GestureDrag::new();
        drag.set_button(gdk::BUTTON_PRIMARY);
        let v = self.clone();
        drag.connect_drag_begin(move |_, _, _| {
            let pan = v.state.borrow().geom.pan();
            v.state.borrow_mut().drag_from = Some(pan);
            v.area.set_cursor_from_name(Some("grabbing"));
            v.area.grab_focus();
        });
        let v = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let from = v.state.borrow().drag_from;
            if let Some(from) = from {
                v.state.borrow_mut().geom.drag_to(from, (dx, dy));
                v.area.queue_draw();
            }
        });
        let v = self.clone();
        drag.connect_drag_end(move |_, _, _| {
            v.state.borrow_mut().drag_from = None;
            v.area.set_cursor_from_name(Some("grab"));
        });
        self.area.add_controller(drag);

        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let v = self.clone();
        click.connect_pressed(move |_, n, _, _| {
            if n == 2 {
                v.zoom_to_fit();
            }
        });
        self.area.add_controller(click);

        // `keyPressEvent`: + / - zoom, 0 fits, 1 shows actual pixels.
        let keys = gtk::EventControllerKey::new();
        let v = self.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            use gdk::Key;
            match key {
                Key::plus | Key::equal | Key::KP_Add => v.zoom_in(),
                Key::minus | Key::underscore | Key::KP_Subtract => v.zoom_out(),
                Key::_0 | Key::KP_0 => v.zoom_to_fit(),
                Key::_1 | Key::KP_1 => v.zoom_to_actual(),
                _ => return gtk::glib::Propagation::Proceed,
            }
            gtk::glib::Propagation::Stop
        });
        self.area.add_controller(keys);

        let v = self.clone();
        self.area.set_draw_func(move |area, cr, w, h| {
            v.paint(area, cr, f64::from(w), f64::from(h));
        });
    }

    // -- painting -------------------------------------------------------------------------------

    /// `_draw_side`: only the part of `surf` that is actually visible. Smooth while shrinking; past
    /// 1:1 the real pixels, since at that point the artefacts are the thing being looked at.
    fn draw_side(
        &self,
        cr: &cairo::Context,
        surf: &Surf,
        target: RectF,
        clip: RectF,
        widget: (f64, f64),
    ) {
        let Some((dest, src)) = visible_part(surf.size, target, clip, widget) else {
            return;
        };
        let _ = cr.save();
        cr.rectangle(dest.x, dest.y, dest.w, dest.h);
        cr.clip();
        cr.translate(dest.x, dest.y);
        cr.scale(dest.w / src.w, dest.h / src.h);
        if cr.set_source_surface(&surf.surface, -src.x, -src.y).is_ok() {
            cr.source().set_filter(if surf.size.0 / target.w >= 1.0 {
                cairo::Filter::Good
            } else {
                cairo::Filter::Nearest
            });
            let _ = cr.paint();
        }
        let _ = cr.restore();
    }

    #[allow(deprecated)]
    fn centred_text(&self, area: &gtk::DrawingArea, cr: &cairo::Context, rect: RectF, text: &str) {
        let layout = area.create_pango_layout(Some(text));
        layout.set_alignment(gtk::pango::Alignment::Center);
        layout.set_width((rect.w * f64::from(gtk::pango::SCALE)) as i32);
        let (_, h) = layout.pixel_size();
        let colour = area.color();
        let _ = cr.save();
        cr.rectangle(rect.x, rect.y, rect.w, rect.h);
        cr.clip();
        cr.set_source_rgba(
            f64::from(colour.red()),
            f64::from(colour.green()),
            f64::from(colour.blue()),
            f64::from(colour.alpha()),
        );
        // The toolkit's own text render; the one pango-with-cairo call this crate has, and the
        // alternative is a dependency for a two-line message.
        gtk::render_layout(
            &area.style_context(),
            cr,
            rect.x,
            rect.y + (rect.h - f64::from(h)) / 2.0,
            &layout,
        );
        let _ = cr.restore();
    }

    fn paint(&self, area: &gtk::DrawingArea, cr: &cairo::Context, w: f64, h: f64) {
        let s = self.state.borrow();
        let widget = (w, h);
        let Some(target) = s.geom.target_rect() else {
            self.centred_text(
                area,
                cr,
                RectF {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                NOTHING_TO_COMPARE,
            );
            return;
        };
        // `on_accent`, the one ink token that is the same fixed white in both schemes: the line is
        // drawn over a photograph of unknown brightness, not over the app's own chrome.
        let line = |alpha: f64| cr.set_source_rgba(1.0, 1.0, 1.0, alpha);
        cr.set_line_width(2.0);
        let (top, bottom) = (target.y.max(0.0), target.bottom().min(h));

        if s.geom.showing_differences() {
            let half = target.w / 2.0;
            let panes = [
                RectF {
                    x: target.x,
                    y: target.y,
                    w: half,
                    h: target.h,
                },
                RectF {
                    x: target.x + half,
                    y: target.y,
                    w: half,
                    h: target.h,
                },
            ];
            for (pane, surf) in panes.iter().zip([&s.diff_local, &s.diff_match]) {
                if let Some(surf) = surf {
                    self.draw_side(cr, surf, *pane, *pane, widget);
                }
            }
            if let Some(overlay) = &s.diff_overlay {
                for pane in &panes {
                    let revealed = RectF {
                        w: pane.w * s.position,
                        ..*pane
                    };
                    if revealed.w > 0.0 {
                        self.draw_side(cr, overlay, *pane, revealed, widget);
                    }
                }
            }
            if bottom > top {
                line(1.0);
                for pane in &panes {
                    let x = pane.x + pane.w * s.position;
                    cr.move_to(x, top);
                    cr.line_to(x, bottom);
                    let _ = cr.stroke();
                }
                // And a divider between the two, so they read as two pictures.
                line(140.0 / 255.0);
                cr.move_to(target.x + half, top);
                cr.line_to(target.x + half, bottom);
                let _ = cr.stroke();
            }
            return;
        }

        let split_x = target.x + target.w * s.position;
        if let Some(left) = &s.left {
            let clip = RectF {
                x: target.x,
                y: target.y,
                w: split_x - target.x,
                h: target.h,
            };
            self.draw_side(cr, left, target, clip, widget);
        }
        let right_clip = RectF {
            x: split_x,
            y: target.y,
            w: target.right() - split_x,
            h: target.h,
        };
        if let Some(right) = &s.right {
            self.draw_side(cr, right, target, right_clip, widget);
        } else if s.left.is_some() {
            // There IS a local picture but no matched one. A blank half looked like the match
            // simply had not changed, so say what happened.
            self.centred_text(area, cr, right_clip, MATCH_MISSING);
        }
        if bottom > top {
            line(1.0);
            cr.move_to(split_x, top);
            cr.line_to(split_x, bottom);
            let _ = cr.stroke();
        }
    }
}

impl Default for WipeView {
    fn default() -> Self {
        WipeView::new()
    }
}
