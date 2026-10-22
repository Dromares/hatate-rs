//! Pixel audits for the three look conditions: is the focus ring really drawn and how strong is
//! it, is an open popover square and shadowless, is a dialog card square and shadowless. They
//! work on rendered pixels, not on the CSS, so they say what the toolkit drew. No display is
//! needed to test them: the tests feed hand-made pixel buffers (a flat rectangle passes, a
//! rounded one with a soft shadow fails).

use crate::theme::{Rgba, contrast};

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
    /// The unfocused colour at that worst pixel, and where it is.
    pub worst_beneath: Rgba,
    pub worst_at: (usize, usize),
}

/// Compare a render without focus to one with focus, inside `area`. `None` if nothing changed,
/// which is the finding "this widget shows no focus ring".
///
/// The ring colour is the colour of the changed pixel that stands furthest from what was there
/// unfocused; the ring pixels are the changed pixels within a few levels of that colour (so
/// anti-aliased edge pixels, which are blends, do not count as ring and do not drag the minimum
/// down). `min_contrast` is the lowest WCAG contrast between a ring pixel and the unfocused
/// pixel under it.
pub fn ring_stats(before: &Pixels, after: &Pixels, area: Rect) -> Option<RingStats> {
    let mut changed: Vec<(usize, usize)> = Vec::new();
    let mut peak: Option<(f64, Rgba)> = None;
    for y in area.y.max(0)..(area.y + area.h).min(after.h as i64) {
        for x in area.x.max(0)..(area.x + area.w).min(after.w as i64) {
            let (x, y) = (x as usize, y as usize);
            let (a, b) = (before.get(x, y), after.get(x, y));
            if a[..3] != b[..3] {
                changed.push((x, y));
                let c = contrast(after.rgb(x, y), before.rgb(x, y));
                if peak.is_none_or(|(best, _)| c > best) {
                    peak = Some((c, after.rgb(x, y)));
                }
            }
        }
    }
    let (_, ring) = peak?;
    let close = |p: Rgba| {
        (p.0 - ring.0).abs() <= 6.0 && (p.1 - ring.1).abs() <= 6.0 && (p.2 - ring.2).abs() <= 6.0
    };
    let mut min_contrast = f64::MAX;
    let mut worst = ring;
    let mut worst_at = (0, 0);
    let mut ring_px = 0;
    for (x, y) in changed.iter().copied() {
        if close(after.rgb(x, y)) {
            ring_px += 1;
            let under = before.rgb(x, y);
            let c = contrast(ring, under);
            if c < min_contrast {
                min_contrast = c;
                worst = under;
                worst_at = (x, y);
            }
        }
    }
    Some(RingStats {
        ring,
        changed: changed.len(),
        ring_px,
        min_contrast,
        worst_beneath: worst,
        worst_at,
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
    /// The corner pixel of the painted card (the bounding box of its border-coloured pixels) is
    /// the border colour, not the backdrop showing through a rounded corner.
    pub corners_square: [bool; 4],
    /// Pixels in the band just outside the card that differ from what the dimmed backdrop
    /// alone would show there: a drop shadow.
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

/// Least-squares line `with = k * under + c` for one channel over the pixels outside `skip`.
fn fit_dim(under: &Pixels, with: &Pixels, skip: Rect, ch: usize) -> (f64, f64) {
    let (mut n, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for y in (0..under.h).step_by(3) {
        for x in (0..under.w).step_by(3) {
            let (xi, yi) = (x as i64, y as i64);
            if xi >= skip.x && xi < skip.x + skip.w && yi >= skip.y && yi < skip.y + skip.h {
                continue;
            }
            let (u, w) = (
                f64::from(under.get(x, y)[ch]),
                f64::from(with.get(x, y)[ch]),
            );
            n += 1.0;
            sx += u;
            sy += w;
            sxx += u * u;
            sxy += u * w;
        }
    }
    let den = n * sxx - sx * sx;
    if den.abs() < 1e-9 {
        return (1.0, 0.0);
    }
    let k = (n * sxy - sx * sy) / den;
    (k, (sy - k * sx) / n)
}

/// Flatness of an in-window card (a dialog). `under` is the same window rendered without the
/// dialog, `with` rendered with it. The backdrop is dimmed uniformly, so away from the card
/// `with` is a straight line of `under`; inside the `band` around the card any pixel that
/// leaves that line is shadow. `own[0]` is the card's border colour.
pub fn card_flatness(
    under: &Pixels,
    with: &Pixels,
    card: Rect,
    own: &[Rgba],
    band: i64,
) -> CardFlatness {
    let near = |a: Rgba, b: Rgba| {
        (a.0 - b.0).abs() < 3.0 && (a.1 - b.1).abs() < 3.0 && (a.2 - b.2).abs() < 3.0
    };
    // The bounds a toolkit reports can sit a few pixels off the painted card, so find the
    // painted card: the bounding box of border-coloured pixels near the reported rectangle.
    let edge = own[0];
    let search = card.grow(4);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in search.y.max(0)..(search.y + search.h).min(with.h as i64) {
        for x in search.x.max(0)..(search.x + search.w).min(with.w as i64) {
            if near(with.rgb(x as usize, y as usize), edge) {
                x0 = x0.min(x as usize);
                y0 = y0.min(y as usize);
                x1 = x1.max(x as usize);
                y1 = y1.max(y as usize);
            }
        }
    }
    if x0 == usize::MAX {
        return CardFlatness {
            corners_square: [false; 4],
            shadow_px: usize::MAX,
        };
    }
    let painted = Rect {
        x: x0 as i64,
        y: y0 as i64,
        w: (x1 - x0 + 1) as i64,
        h: (y1 - y0 + 1) as i64,
    };
    let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    let corners_square = corners.map(|(x, y)| near(with.rgb(x, y), edge));
    let outer = painted.grow(band);
    let fit = [0, 1, 2].map(|ch| fit_dim(under, with, outer, ch));
    let mut shadow_px = 0;
    for y in outer.y.max(0)..(outer.y + outer.h).min(with.h as i64) {
        for x in outer.x.max(0)..(outer.x + outer.w).min(with.w as i64) {
            let inside = x >= painted.x
                && x < painted.x + painted.w
                && y >= painted.y
                && y < painted.y + painted.h;
            if inside {
                continue;
            }
            let (u, w) = (
                under.get(x as usize, y as usize),
                with.get(x as usize, y as usize),
            );
            let off = (0..3).any(|ch| {
                let want = fit[ch].0 * f64::from(u[ch]) + fit[ch].1;
                (want - f64::from(w[ch])).abs() > 4.0
            });
            if off {
                shadow_px += 1;
                if shadow_px <= 6 && std::env::var_os("HATATE_AUDIT_DEBUG").is_some() {
                    eprintln!("AUDITDBG shadow px ({x},{y}) under={u:?} with={w:?} fit={fit:?}");
                }
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
        // `under`: a striped page. `with`: the same page dimmed to 60%, a card on top.
        let mut under = Pixels::new(120, 90, [0, 0, 0, 255]);
        for y in 0..90 {
            for x in 0..120 {
                let v = if (y / 6) % 2 == 0 { 100 } else { 160 };
                under.set(x, y, [v, v, v, 255]);
            }
        }
        let mut dimmed = under.clone();
        for p in dimmed.rgba.chunks_mut(4) {
            for c in &mut p[..3] {
                *c = (f64::from(*c) * 0.6) as u8;
            }
        }
        let edge = [90, 87, 83, 255];
        let card = Rect {
            x: 30,
            y: 25,
            w: 60,
            h: 40,
        };
        let mut flat = dimmed.clone();
        for y in 25..65 {
            for x in 30..90 {
                let border = x == 30 || x == 89 || y == 25 || y == 64;
                flat.set(x, y, if border { edge } else { [20, 20, 20, 255] });
            }
        }
        let own = [Rgba::rgb(90, 87, 83)];
        let f = card_flatness(&under, &flat, card, &own, 12);
        assert!(f.square() && f.shadowless(), "{f:?}");

        let mut soft = flat.clone();
        soft.set(30, 25, dimmed.get(30, 25)); // rounded corner shows the backdrop
        for x in 30..90 {
            soft.set(x, 67, [0, 0, 0, 255]); // shadow band, darker than the dimmed page
        }
        let f = card_flatness(&under, &soft, card, &own, 12);
        assert!(!f.square() && !f.shadowless(), "{f:?}");
        assert_eq!(f.shadow_px, 60);
    }
}
