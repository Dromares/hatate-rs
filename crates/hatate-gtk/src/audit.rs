//! Pixel audits for the three look conditions: is the focus ring really drawn and how strong is
//! it, is an open popover square and shadowless, is a dialog card square and shadowless. They
//! work on rendered pixels, not on the CSS, so they say what the toolkit drew. No display is
//! needed to test them: the tests feed hand-made pixel buffers (a flat rectangle passes, a
//! rounded one with a soft shadow fails).

use crate::theme::{Rgba, contrast};
use std::collections::HashMap;

/// Straight-alpha RGBA8 pixels.
#[derive(Clone)]
pub struct Pixels {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl Pixels {
    pub fn new(w: usize, h: usize, fill: [u8; 4]) -> Pixels {
        Pixels {
            w,
            h,
            rgba: fill.iter().copied().cycle().take(w * h * 4).collect(),
        }
    }

    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    pub fn set(&mut self, x: usize, y: usize, p: [u8; 4]) {
        let i = (y * self.w + x) * 4;
        self.rgba[i..i + 4].copy_from_slice(&p);
    }

    pub fn rgb(&self, x: usize, y: usize) -> Rgba {
        let p = self.get(x, y);
        Rgba::rgb(p[0], p[1], p[2])
    }
}

/// An integer pixel rectangle, clipped on use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Rect {
    pub fn grow(self, by: i64) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2 * by,
            h: self.h + 2 * by,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RingStats {
    /// The most common colour among the pixels the focus changed.
    pub ring: Rgba,
    /// Pixels that changed colour inside the widget's neighbourhood.
    pub changed: usize,
    /// Of those, the ones that are exactly the ring colour.
    pub ring_px: usize,
    /// Lowest WCAG contrast between a ring pixel and the pixel that was there unfocused.
    pub min_contrast: f64,
    /// The unfocused colour at that worst pixel.
    pub worst_beneath: Rgba,
}

/// Compare a render without focus to one with focus, inside `area`. `None` if nothing changed,
/// which is the finding "this widget shows no focus ring".
pub fn ring_stats(before: &Pixels, after: &Pixels, area: Rect) -> Option<RingStats> {
    let mut changed: Vec<(usize, usize)> = Vec::new();
    let mut by_colour: HashMap<[u8; 3], usize> = HashMap::new();
    for y in area.y.max(0)..(area.y + area.h).min(after.h as i64) {
        for x in area.x.max(0)..(area.x + area.w).min(after.w as i64) {
            let (x, y) = (x as usize, y as usize);
            let (a, b) = (before.get(x, y), after.get(x, y));
            if a[..3] != b[..3] {
                changed.push((x, y));
                *by_colour.entry([b[0], b[1], b[2]]).or_default() += 1;
            }
        }
    }
    let (&ring_rgb, _) = by_colour.iter().max_by_key(|(c, n)| (**n, **c))?;
    let ring = Rgba::rgb(ring_rgb[0], ring_rgb[1], ring_rgb[2]);
    let mut min_contrast = f64::MAX;
    let mut worst = ring;
    let mut ring_px = 0;
    for (x, y) in changed.iter().copied() {
        let b = after.get(x, y);
        if [b[0], b[1], b[2]] == ring_rgb {
            ring_px += 1;
            let under = before.rgb(x, y);
            let c = contrast(ring, under);
            if c < min_contrast {
                min_contrast = c;
                worst = under;
            }
        }
    }
    Some(RingStats {
        ring,
        changed: changed.len(),
        ring_px,
        min_contrast,
        worst_beneath: worst,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flatness {
    /// Bounding box of the opaque (alpha 255) pixels.
    pub body: Rect,
    /// The four corner pixels of `body` are opaque: no rounded corner cut them away.
    pub corners_opaque: [bool; 4],
    /// Pixels that are neither fully transparent nor fully opaque: a soft shadow or an
    /// anti-aliased rounded edge.
    pub partial_alpha_px: usize,
}

impl Flatness {
    pub fn square(&self) -> bool {
        self.corners_opaque.iter().all(|c| *c)
    }

    pub fn shadowless(&self) -> bool {
        self.partial_alpha_px == 0
    }
}

/// Flatness of a popover rendered on its own (transparent outside its body).
pub fn popover_flatness(px: &Pixels) -> Option<Flatness> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let mut partial = 0;
    for y in 0..px.h {
        for x in 0..px.w {
            match px.get(x, y)[3] {
                255 => {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
                0 => {}
                _ => partial += 1,
            }
        }
    }
    if x0 == usize::MAX {
        return None;
    }
    let opaque = |x: usize, y: usize| px.get(x, y)[3] == 255;
    Some(Flatness {
        body: Rect {
            x: x0 as i64,
            y: y0 as i64,
            w: (x1 - x0 + 1) as i64,
            h: (y1 - y0 + 1) as i64,
        },
        corners_opaque: [
            opaque(x0, y0),
            opaque(x1, y0),
            opaque(x0, y1),
            opaque(x1, y1),
        ],
        partial_alpha_px: partial,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFlatness {
    /// The four corner pixels of the card are the card's own edge colour, not the backdrop
    /// showing through a rounded corner.
    pub corners_square: [bool; 4],
    /// Pixels in the band just outside the card that differ from the backdrop's colour far
    /// away: a drop shadow.
    pub shadow_px: usize,
}

impl CardFlatness {
    pub fn square(&self) -> bool {
        self.corners_square.iter().all(|c| *c)
    }

    pub fn shadowless(&self) -> bool {
        self.shadow_px == 0
    }
}

/// Flatness of an in-window card (a dialog) at `card` in a window render whose backdrop is
/// otherwise uniform. `edge` is the card's border colour.
pub fn card_flatness(px: &Pixels, card: Rect, edge: Rgba, band: i64) -> CardFlatness {
    let near = |a: Rgba, b: Rgba| {
        (a.0 - b.0).abs() < 3.0 && (a.1 - b.1).abs() < 3.0 && (a.2 - b.2).abs() < 3.0
    };
    let (x0, y0) = (card.x.max(0) as usize, card.y.max(0) as usize);
    let (x1, y1) = (
        ((card.x + card.w - 1) as usize).min(px.w - 1),
        ((card.y + card.h - 1) as usize).min(px.h - 1),
    );
    let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    let corners_square = corners.map(|(x, y)| near(px.rgb(x, y), edge));
    // The reference backdrop colour: the window's own corner, far from any card.
    let backdrop = px.rgb(2, px.h - 3);
    let outer = card.grow(band);
    let mut shadow_px = 0;
    for y in outer.y.max(0)..(outer.y + outer.h).min(px.h as i64) {
        for x in outer.x.max(0)..(outer.x + outer.w).min(px.w as i64) {
            let inside = x >= card.x && x < card.x + card.w && y >= card.y && y < card.y + card.h;
            if !inside && !near(px.rgb(x as usize, y as usize), backdrop) {
                shadow_px += 1;
            }
        }
    }
    CardFlatness {
        corners_square,
        shadow_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_popover() -> Pixels {
        let mut p = Pixels::new(60, 40, [0, 0, 0, 0]);
        for y in 5..35 {
            for x in 5..55 {
                p.set(x, y, [20, 20, 20, 255]);
            }
        }
        p
    }

    #[test]
    fn a_flat_square_popover_passes() {
        let f = popover_flatness(&flat_popover()).unwrap();
        assert!(f.square() && f.shadowless(), "{f:?}");
        assert_eq!(
            f.body,
            Rect {
                x: 5,
                y: 5,
                w: 50,
                h: 30
            }
        );
    }

    #[test]
    fn a_rounded_corner_fails_square() {
        let mut p = flat_popover();
        for (x, y) in [(5, 5), (6, 5), (5, 6)] {
            p.set(x, y, [0, 0, 0, 0]);
        }
        let f = popover_flatness(&p).unwrap();
        assert!(!f.square());
    }

    #[test]
    fn a_soft_shadow_fails_shadowless() {
        let mut p = flat_popover();
        for x in 5..55 {
            p.set(x, 36, [0, 0, 0, 90]);
            p.set(x, 37, [0, 0, 0, 40]);
        }
        let f = popover_flatness(&p).unwrap();
        assert!(!f.shadowless());
        assert_eq!(f.partial_alpha_px, 100);
    }

    #[test]
    fn an_empty_render_is_none() {
        assert!(popover_flatness(&Pixels::new(10, 10, [0, 0, 0, 0])).is_none());
    }

    #[test]
    fn a_focus_ring_is_found_and_measured() {
        let page = [13, 13, 13, 255];
        let before = Pixels::new(40, 30, page);
        let mut after = before.clone();
        let ink = [205, 196, 186, 255];
        for x in 8..32 {
            for y in [8, 9, 20, 21] {
                after.set(x, y, ink);
            }
        }
        let r = ring_stats(
            &before,
            &after,
            Rect {
                x: 4,
                y: 4,
                w: 32,
                h: 22,
            },
        )
        .unwrap();
        assert_eq!(r.ring_px, 96);
        assert!(r.min_contrast > 10.0, "{}", r.min_contrast);
    }

    #[test]
    fn no_change_means_no_ring() {
        let a = Pixels::new(10, 10, [1, 2, 3, 255]);
        assert!(
            ring_stats(
                &a,
                &a.clone(),
                Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 10
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_ring_the_colour_of_its_surface_has_contrast_one() {
        // The selected stamp row with a plain ink ring: ink on ink-filled stamp.
        let ink = [205, 196, 186, 255];
        let before = Pixels::new(20, 20, ink);
        let mut after = before.clone();
        after.set(5, 5, [205, 196, 187, 255]);
        let r = ring_stats(
            &before,
            &after,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 20,
            },
        )
        .unwrap();
        assert!(r.min_contrast < 1.1);
    }

    #[test]
    fn card_flatness_tells_a_flat_card_from_a_shadowed_round_one() {
        let bg = [9, 9, 9, 255];
        let edge = [90, 87, 83, 255];
        let mut flat = Pixels::new(100, 80, bg);
        let card = Rect {
            x: 20,
            y: 20,
            w: 60,
            h: 40,
        };
        for y in 20..60 {
            for x in 20..80 {
                let border = x == 20 || x == 79 || y == 20 || y == 59;
                flat.set(x, y, if border { edge } else { [20, 20, 20, 255] });
            }
        }
        let f = card_flatness(&flat, card, Rgba::rgb(90, 87, 83), 12);
        assert!(f.square() && f.shadowless(), "{f:?}");

        let mut soft = flat.clone();
        soft.set(20, 20, bg); // rounded corner shows the backdrop
        for x in 20..80 {
            soft.set(x, 62, [0, 0, 0, 255]); // shadow band, darker than the backdrop
        }
        let f = card_flatness(&soft, card, Rgba::rgb(90, 87, 83), 12);
        assert!(!f.square() && !f.shadowless(), "{f:?}");
    }
}
