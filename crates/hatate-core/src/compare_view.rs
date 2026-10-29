//! The pure half of the comparison view (`WipeView`, `build_difference_overlay`, `gui/compare_dialog.py`
//! at `76c85c4`; DAN-2340): where the shared target rectangle sits at a zoom and a pan, and the
//! Differences pane's pixels. No GTK, so the widget and a test can both call it.
//!
//! Zoom applies to the shared target rectangle and not to either picture, so the two stay
//! registered at every zoom level and the wipe keeps landing on the same feature in both.
//!
//! **Differences pane (design gate 2.2).** Python paints the two pictures in colour and sweeps a red
//! overlay across them. The gate asks for a pane with zero chroma, an R=G=B ramp from `#1a1a1a` up to
//! the luma of the mockup's `#cdc4ba`, so here each picture is drawn as its luma on that ramp and the
//! overlay is the ramp's top grey. Alpha follows the mask exactly as Python's does (`int(v * 0.82)`).
//! What the pane is, region by region, is unchanged; only the hue is.

use crate::error::ErrorKind;
use crate::image_diff::{Diff, difference_map_images};
use crate::pyraster::{Filter, Image, Mode};

/// `WipeView.MIN_ZOOM`, relative to "fits the box".
pub const MIN_ZOOM: f64 = 0.25;
/// `WipeView.MAX_ZOOM`.
pub const MAX_ZOOM: f64 = 40.0;
/// The Differences ramp's dark end, `#1a1a1a`.
pub const RAMP_LOW: u8 = 0x1a;
/// The ramp's light end: Pillow's `L` (`(299 R + 587 G + 114 B) / 1000`) of `#cdc4ba`.
pub const RAMP_HIGH: u8 = 197;

/// `QRectF`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    /// `QRectF.intersected`; an empty result has zero width or height.
    pub fn intersected(&self, other: &RectF) -> RectF {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= x || b <= y {
            return RectF {
                x,
                y,
                w: 0.0,
                h: 0.0,
            };
        }
        RectF {
            x,
            y,
            w: r - x,
            h: b - y,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// `QSizeF.scaled(bound, KeepAspectRatio)`.
fn scaled_to_fit(size: (f64, f64), bound: (f64, f64)) -> (f64, f64) {
    if size.0 <= 0.0 || size.1 <= 0.0 {
        return (0.0, 0.0);
    }
    let rw = bound.1 * size.0 / size.1;
    if rw <= bound.0 {
        (rw, bound.1)
    } else {
        (bound.0, bound.0 * size.1 / size.0)
    }
}

/// `WipeView`'s `_zoom` and `_pan` and the maths around them.
#[derive(Debug, Clone)]
pub struct WipeGeometry {
    widget: (f64, f64),
    /// `_reference()`: the picture whose aspect defines the box (the local one where there is one).
    reference: Option<(f64, f64)>,
    /// `diff_local`'s size while Differences is showing.
    differences: Option<(f64, f64)>,
    zoom: f64,
    pan: (f64, f64),
}

impl Default for WipeGeometry {
    fn default() -> Self {
        WipeGeometry {
            widget: (0.0, 0.0),
            reference: None,
            differences: None,
            zoom: 1.0,
            pan: (0.0, 0.0),
        }
    }
}

impl WipeGeometry {
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    pub fn pan(&self) -> (f64, f64) {
        self.pan
    }

    /// `set_images`: a new pair starts framed, not wherever the last one was.
    pub fn set_reference(&mut self, reference: Option<(f64, f64)>) {
        self.reference = reference;
        self.zoom_to_fit();
    }

    /// `set_difference_view` (`Some`) and `clear_difference_view` (`None`).
    pub fn set_differences(&mut self, local: Option<(f64, f64)>) {
        self.differences = local;
        self.zoom_to_fit();
    }

    pub fn showing_differences(&self) -> bool {
        self.differences.is_some()
    }

    /// `resizeEvent`: the fit size changed, so the pan is clamped again.
    pub fn set_widget_size(&mut self, width: f64, height: f64) {
        self.widget = (width, height);
        self.clamp_pan();
    }

    /// `_reference_size`: side by side the box is twice as wide as one picture, otherwise fit
    /// would frame a single image and push half the comparison off screen.
    fn frame_shape(&self) -> Option<(f64, f64)> {
        if let Some((w, h)) = self.differences {
            return Some((w * 2.0, h));
        }
        self.reference
    }

    fn fit_size(&self) -> Option<(f64, f64)> {
        self.frame_shape().map(|s| scaled_to_fit(s, self.widget))
    }

    /// `_target_rect`.
    pub fn target_rect(&self) -> Option<RectF> {
        let (fw, fh) = self.fit_size()?;
        let (w, h) = (fw * self.zoom, fh * self.zoom);
        Some(RectF {
            x: (self.widget.0 - w) / 2.0 + self.pan.0,
            y: (self.widget.1 - h) / 2.0 + self.pan.1,
            w,
            h,
        })
    }

    /// `_clamp_pan`: an axis that already fits is pinned to centre.
    fn clamp_pan(&mut self) {
        let Some((fw, fh)) = self.fit_size() else {
            return;
        };
        let slack_x = ((fw * self.zoom - self.widget.0) / 2.0).max(0.0);
        let slack_y = ((fh * self.zoom - self.widget.1) / 2.0).max(0.0);
        self.pan = (
            self.pan.0.clamp(-slack_x, slack_x),
            self.pan.1.clamp(-slack_y, slack_y),
        );
    }

    /// `zoom_percent`: the scale as a percentage of the reference picture's real pixels; `0.0`
    /// with nothing loaded (the caller shows no figure at all).
    pub fn zoom_percent(&self) -> f64 {
        match (self.reference, self.target_rect()) {
            (Some((rw, _)), Some(t)) if rw > 0.0 => t.w / rw * 100.0,
            _ => 0.0,
        }
    }

    /// `_apply_zoom`: zooms so whatever sits under `anchor` stays under it. `true` when the zoom
    /// changed (the caller repaints and reports the percentage).
    pub fn apply_zoom(&mut self, anchor: (f64, f64), new_zoom: f64) -> bool {
        let Some((fw, fh)) = self.fit_size() else {
            return false;
        };
        let new_zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let Some(target) = self.target_rect() else {
            return false;
        };
        if (new_zoom - self.zoom).abs() < 1e-9 || target.w == 0.0 || target.h == 0.0 {
            return false;
        }
        let fx = (anchor.0 - target.x) / target.w;
        let fy = (anchor.1 - target.y) / target.h;
        self.zoom = new_zoom;
        let (w, h) = (fw * new_zoom, fh * new_zoom);
        self.pan = (
            anchor.0 - fx * w - (self.widget.0 - w) / 2.0,
            anchor.1 - fy * h - (self.widget.1 - h) / 2.0,
        );
        self.clamp_pan();
        true
    }

    fn centre(&self) -> (f64, f64) {
        (self.widget.0 / 2.0, self.widget.1 / 2.0)
    }

    pub fn zoom_in(&mut self) -> bool {
        self.apply_zoom(self.centre(), self.zoom * 1.25)
    }

    pub fn zoom_out(&mut self) -> bool {
        self.apply_zoom(self.centre(), self.zoom / 1.25)
    }

    pub fn zoom_to_fit(&mut self) {
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
    }

    /// `zoom_to_actual`: one picture pixel per screen pixel.
    pub fn zoom_to_actual(&mut self) -> bool {
        let (Some((fw, _)), Some((rw, _))) = (self.fit_size(), self.reference) else {
            return false;
        };
        if fw == 0.0 {
            return false;
        }
        self.apply_zoom(self.centre(), rw / fw)
    }

    /// `wheelEvent`: exponential, so each notch feels the same at every zoom level and a
    /// trackpad's many small deltas stay smooth. `angle_delta` is Qt's (120 per notch, up is +).
    pub fn wheel(&mut self, anchor: (f64, f64), angle_delta: f64) -> bool {
        if angle_delta == 0.0 {
            return false;
        }
        self.apply_zoom(anchor, self.zoom * 1.0015f64.powf(angle_delta))
    }

    /// `mouseMoveEvent` while dragging: the pan the drag started from plus how far the pointer has moved.
    pub fn drag_to(&mut self, started_pan: (f64, f64), moved: (f64, f64)) {
        self.pan = (started_pan.0 + moved.0, started_pan.1 + moved.1);
        self.clamp_pan();
    }
}

/// The part of `pixmap_size` that `_draw_side` has to paint for `target` clipped to `clip`
/// inside a widget of `widget`: `(destination, source)` in widget and picture pixels. `None` when
/// nothing of it is visible. Handing the toolkit a target far larger than the window would make it
/// scale the whole picture every repaint just to throw most of it away.
pub fn visible_part(
    pixmap_size: (f64, f64),
    target: RectF,
    clip: RectF,
    widget: (f64, f64),
) -> Option<(RectF, RectF)> {
    let area = target.intersected(&clip).intersected(&RectF {
        x: 0.0,
        y: 0.0,
        w: widget.0,
        h: widget.1,
    });
    if area.is_empty() || target.w <= 0.0 || target.h <= 0.0 {
        return None;
    }
    let (xs, ys) = (pixmap_size.0 / target.w, pixmap_size.1 / target.h);
    Some((
        area,
        RectF {
            x: (area.x - target.x) * xs,
            y: (area.y - target.y) * ys,
            w: area.w * xs,
            h: area.h * ys,
        },
    ))
}

/// A straight-alpha RGBA8 picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A picture as the Differences ramp shows it: its luma (Pillow's `L`) mapped onto `RAMP_LOW`..
/// `RAMP_HIGH`, R=G=B, alpha kept.
pub fn ramp_picture(p: &Rgba) -> Rgba {
    let span = u32::from(RAMP_HIGH - RAMP_LOW);
    let mut out = Vec::with_capacity(p.rgba.len());
    for px in p.rgba.chunks_exact(4) {
        let l = (299 * u32::from(px[0]) + 587 * u32::from(px[1]) + 114 * u32::from(px[2])) / 1000;
        let v = RAMP_LOW + (l * span / 255) as u8;
        out.extend_from_slice(&[v, v, v, px[3]]);
    }
    Rgba {
        width: p.width,
        height: p.height,
        rgba: out,
    }
}

/// `Image.frombytes("RGB", ...)` of a pixmap converted to `Format_RGB888`: the alpha is dropped.
pub fn rgb_of(p: &Rgba) -> Image {
    let mut data = Vec::with_capacity(p.rgba.len() / 4 * 3);
    for px in p.rgba.chunks_exact(4) {
        data.extend_from_slice(&px[..3]);
    }
    Image {
        width: p.width as usize,
        height: p.height as usize,
        mode: Mode::Rgb,
        data,
    }
}

/// What `build_difference_overlay` hands back.
#[derive(Debug, Clone)]
pub struct DifferenceView {
    /// The overlay at the local picture's size: the ramp's top grey, alpha following the mask.
    pub overlay: Rgba,
    pub diff: Diff,
}

/// `build_difference_overlay(local, matched)`: `None` where Python's `(None, None)` is (no mask,
/// so "Nothing to compare" or too much of the frame moved). Composed at the LOCAL picture's own
/// size, not the smaller size the comparison ran at, so the clean and highlighted views are the
/// same picture at the same resolution.
pub fn build_difference_overlay(local: &Rgba, matched: &Rgba) -> Option<DifferenceView> {
    if local.width == 0 || local.height == 0 || matched.width == 0 || matched.height == 0 {
        return None;
    }
    let diff = difference_map_images(Some(&rgb_of(local)), Some(&rgb_of(matched)));
    let mask = diff.mask.as_ref()?;
    let mask = Image {
        width: diff.width,
        height: diff.height,
        mode: Mode::L,
        data: mask.clone(),
    };
    let (w, h) = (local.width as usize, local.height as usize);
    let mask = if (mask.width, mask.height) != (w, h) {
        mask.resize_filtered(w, h, Filter::Lanczos)
    } else {
        mask
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &v in &mask.data {
        // `mask.point(lambda v: int(v * 0.82))`: never fully opaque, the picture underneath is
        // the point.
        rgba.extend_from_slice(&[RAMP_HIGH, RAMP_HIGH, RAMP_HIGH, (f64::from(v) * 0.82) as u8]);
    }
    Some(DifferenceView {
        overlay: Rgba {
            width: local.width,
            height: local.height,
            rgba,
        },
        diff,
    })
}

/// Why a picture could not be opened for the comparison. The window shows [`user_message`]
/// (what failed and what to do; no path, no OS text, design gate 2.8); [`detail`] is for the log or a
/// detail line.
///
/// [`user_message`]: PictureError::user_message
/// [`detail`]: PictureError::detail
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureError {
    kind: ErrorKind,
    detail: String,
}

impl PictureError {
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn user_message(&self) -> String {
        match self.kind {
            ErrorKind::ImageUnreadable => "The local picture could not be opened. Check that the \
                 file is still there and that Hatate may read it, then select the row again."
                .to_owned(),
            _ => "The local picture could not be read as an image. Check that the file is a \
                 picture and is not damaged, then select the row again."
                .to_owned(),
        }
    }

    /// The raw OS error and the path.
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

/// `QPixmap(entry.path)`, full size: the local picture, or why not.
pub fn open_local(path: &str) -> Result<Rgba, PictureError> {
    let bytes = std::fs::read(path).map_err(|e| PictureError {
        kind: ErrorKind::ImageUnreadable,
        detail: format!("{path}: {e}"),
    })?;
    decode_rgba(&bytes).ok_or_else(|| PictureError {
        kind: ErrorKind::ImageUndecodable,
        detail: format!("{path}: not a picture the decoder reads"),
    })
}

/// `QImage.loadFromData` then `QPixmap.fromImage`, for the match's bytes.
pub fn decode_rgba(bytes: &[u8]) -> Option<Rgba> {
    let p = crate::thumb_decode::decode_bytes(bytes, u32::MAX)?;
    Some(Rgba {
        width: p.width,
        height: p.height,
        rgba: p.rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(widget: (f64, f64), reference: (f64, f64)) -> WipeGeometry {
        let mut g = WipeGeometry::default();
        g.set_widget_size(widget.0, widget.1);
        g.set_reference(Some(reference));
        g
    }

    #[test]
    fn a_new_pair_is_framed_centred_and_fit() {
        let g = geom((400.0, 400.0), (800.0, 400.0));
        let t = g.target_rect().unwrap();
        assert_eq!((t.x, t.y, t.w, t.h), (0.0, 100.0, 400.0, 200.0));
        assert_eq!(g.zoom_percent(), 50.0);
    }

    #[test]
    fn side_by_side_frames_twice_the_width() {
        let mut g = geom((400.0, 400.0), (400.0, 400.0));
        g.set_differences(Some((400.0, 400.0)));
        let t = g.target_rect().unwrap();
        assert_eq!(
            (t.w, t.h),
            (400.0, 200.0),
            "the pair fits the box: 800 x 400 scaled to 400 wide"
        );
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut g = geom((400.0, 400.0), (400.0, 400.0));
        let anchor = (300.0, 100.0);
        let before = g.target_rect().unwrap();
        let (fx, fy) = (
            (anchor.0 - before.x) / before.w,
            (anchor.1 - before.y) / before.h,
        );
        assert!(g.apply_zoom(anchor, 2.0));
        let after = g.target_rect().unwrap();
        assert!(((anchor.0 - after.x) / after.w - fx).abs() < 1e-9);
        assert!(((anchor.1 - after.y) / after.h - fy).abs() < 1e-9);
        assert_eq!(g.zoom_percent(), 200.0);
    }

    #[test]
    fn zoom_is_bounded_and_a_no_op_at_the_bound() {
        let mut g = geom((400.0, 400.0), (400.0, 400.0));
        assert!(g.apply_zoom((200.0, 200.0), 1000.0));
        assert_eq!(g.zoom(), MAX_ZOOM);
        assert!(
            !g.apply_zoom((200.0, 200.0), 1000.0),
            "already at the ceiling"
        );
        assert!(g.apply_zoom((200.0, 200.0), 0.0001));
        assert_eq!(g.zoom(), MIN_ZOOM);
    }

    #[test]
    fn the_pan_cannot_drag_an_edge_inside_the_viewport() {
        let mut g = geom((400.0, 400.0), (400.0, 400.0));
        g.apply_zoom((200.0, 200.0), 2.0);
        g.drag_to((0.0, 0.0), (10_000.0, -10_000.0));
        // 800 px picture in a 400 px view: 200 px of slack each way.
        assert_eq!(g.pan(), (200.0, -200.0));
        // An axis that fits is pinned to the centre.
        let mut wide = geom((400.0, 400.0), (800.0, 400.0));
        wide.drag_to((0.0, 0.0), (50.0, 50.0));
        assert_eq!(wide.pan(), (0.0, 0.0));
    }

    #[test]
    fn actual_pixels_is_one_picture_pixel_per_screen_pixel() {
        let mut g = geom((400.0, 400.0), (1000.0, 1000.0));
        assert!(g.zoom_to_actual());
        assert_eq!(g.zoom_percent(), 100.0);
        g.zoom_to_fit();
        assert_eq!(g.zoom_percent(), 40.0);
    }

    #[test]
    fn nothing_loaded_has_no_figure() {
        let g = WipeGeometry::default();
        assert!(g.target_rect().is_none());
        assert_eq!(g.zoom_percent(), 0.0);
    }

    #[test]
    fn only_the_visible_part_is_drawn() {
        let target = RectF {
            x: -1000.0,
            y: 0.0,
            w: 4000.0,
            h: 400.0,
        };
        let (dest, src) =
            visible_part((8000.0, 800.0), target, target, (400.0, 400.0)).expect("visible");
        assert_eq!((dest.x, dest.w), (0.0, 400.0));
        assert_eq!((src.x, src.w), (2000.0, 800.0));
        let off = RectF {
            x: 500.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        assert!(visible_part((10.0, 10.0), off, off, (400.0, 400.0)).is_none());
    }

    #[test]
    fn the_ramp_has_no_chroma_and_spans_its_ends() {
        let p = Rgba {
            width: 3,
            height: 1,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255, 200, 30, 90, 128],
        };
        let r = ramp_picture(&p);
        for px in r.rgba.chunks_exact(4) {
            assert!(px[0] == px[1] && px[1] == px[2], "R=G=B: {px:?}");
        }
        assert_eq!(r.rgba[0], RAMP_LOW);
        assert_eq!(r.rgba[4], RAMP_HIGH);
        assert_eq!(r.rgba[11], 128, "alpha is kept");
    }

    fn flat(w: u32, h: u32, v: u8) -> Rgba {
        Rgba {
            width: w,
            height: h,
            rgba: [v, v, v, 255].repeat((w * h) as usize),
        }
    }

    #[test]
    fn identical_pictures_have_a_clear_overlay_and_a_changed_patch_does_not() {
        let a = flat(120, 90, 90);
        let same = build_difference_overlay(&a, &a).expect("comparable");
        assert!(
            same.overlay.rgba.chunks_exact(4).all(|px| px[3] == 0),
            "nothing changed"
        );
        assert_eq!(
            (same.overlay.width, same.overlay.height),
            (120, 90),
            "at the local size"
        );

        let mut b = a.clone();
        for y in 20..60 {
            for x in 30..80 {
                let i = ((y * 120 + x) * 4) as usize;
                b.rgba[i..i + 3].copy_from_slice(&[240, 240, 240]);
            }
        }
        let view = build_difference_overlay(&a, &b).expect("comparable");
        let alpha = |x: usize, y: usize| view.overlay.rgba[(y * 120 + x) * 4 + 3];
        assert!(
            alpha(55, 40) > 100,
            "the patch is marked: {}",
            alpha(55, 40)
        );
        assert_eq!(alpha(5, 5), 0, "the rest is clear");
        assert!(alpha(55, 40) <= (255.0 * 0.82) as u8, "never fully opaque");
        assert!(
            view.overlay
                .rgba
                .chunks_exact(4)
                .all(|px| px[0] == px[1] && px[1] == px[2])
        );
    }

    #[test]
    fn nothing_to_compare_gives_no_overlay() {
        let empty = Rgba {
            width: 0,
            height: 0,
            rgba: vec![],
        };
        assert!(build_difference_overlay(&empty, &flat(4, 4, 9)).is_none());
    }

    #[test]
    fn a_missing_file_says_what_to_do_and_leaks_neither_path_nor_os_text() {
        let path = "/nonexistent/dir/secret-name.png";
        let e = open_local(path).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ImageUnreadable);
        let m = e.user_message();
        assert!(
            !m.contains(path) && !m.contains("secret-name") && !m.contains("os error"),
            "{m}"
        );
        assert!(m.contains("select the row again"), "says what to do: {m}");
        assert!(
            e.detail().contains(path) && e.detail().contains("os error"),
            "{}",
            e.detail()
        );
    }

    #[test]
    fn a_file_that_is_not_a_picture_is_undecodable_not_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("notes-for-me.png");
        std::fs::write(&p, b"just some words").unwrap();
        let e = open_local(p.to_str().unwrap()).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ImageUndecodable);
        assert!(!e.user_message().contains("notes-for-me"));
        assert!(e.detail().contains("notes-for-me"));
    }

    #[test]
    fn a_picture_opens_at_its_own_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ok.png");
        image::RgbaImage::from_pixel(7, 3, image::Rgba([1, 2, 3, 255]))
            .save(&p)
            .unwrap();
        let r = open_local(p.to_str().unwrap()).unwrap();
        assert_eq!((r.width, r.height, r.rgba.len()), (7, 3, 84));
    }
}
