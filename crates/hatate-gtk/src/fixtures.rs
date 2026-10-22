//! Fixture images. The Python repo commits no binary test images: its tests draw
//! them with PIL. `artwork` is a port of `artwork(size)` in
//! `Hatate-Linux-Redux/tests/test_image_compare.py` (at 76c85c4), so the spike
//! uses the same structured picture the Python tests use. Nothing binary is
//! committed here either.

use image::{Rgba, RgbaImage};

fn fill_ellipse(im: &mut RgbaImage, x0: f64, y0: f64, x1: f64, y1: f64, c: [u8; 3]) {
    let (cx, cy, rx, ry) = (
        (x0 + x1) / 2.0,
        (y0 + y1) / 2.0,
        (x1 - x0) / 2.0,
        (y1 - y0) / 2.0,
    );
    for y in y0.floor().max(0.0) as u32..=(y1.ceil() as u32).min(im.height() - 1) {
        for x in x0.floor().max(0.0) as u32..=(x1.ceil() as u32).min(im.width() - 1) {
            let (dx, dy) = (
                (f64::from(x) + 0.5 - cx) / rx,
                (f64::from(y) + 0.5 - cy) / ry,
            );
            if dx * dx + dy * dy <= 1.0 {
                im.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
    }
}

fn fill_rect(im: &mut RgbaImage, x0: f64, y0: f64, x1: f64, y1: f64, c: [u8; 3]) {
    for y in y0.floor() as u32..=(y1.floor() as u32).min(im.height() - 1) {
        for x in x0.floor() as u32..=(x1.floor() as u32).min(im.width() - 1) {
            im.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
}

/// Port of the Python tests' `artwork(size)`.
pub fn artwork(w: u32, h: u32) -> RgbaImage {
    let mut im = RgbaImage::from_pixel(w, h, Rgba([238, 220, 205, 255]));
    let (wf, hf) = (f64::from(w), f64::from(h));
    fill_ellipse(
        &mut im,
        wf * 0.25,
        hf * 0.08,
        wf * 0.75,
        hf * 0.55,
        [30, 32, 48],
    );
    fill_rect(
        &mut im,
        wf * 0.15,
        hf * 0.55,
        wf * 0.85,
        hf * 0.95,
        [58, 92, 176],
    );
    let thick = (h / 300).max(1);
    for i in 0..12 {
        let y = (hf * 0.6 + f64::from(i) * hf * 0.025) as u32;
        for t in 0..thick {
            fill_rect(
                &mut im,
                wf * 0.2,
                f64::from(y + t),
                wf * 0.8,
                f64::from(y + t),
                [230, 230, 240],
            );
        }
    }
    fill_ellipse(
        &mut im,
        wf * 0.38,
        hf * 0.22,
        wf * 0.44,
        hf * 0.28,
        [250, 250, 250],
    );
    im
}

/// Local and match images for the Review spike: the same artwork, the match a
/// re-encode with a brightness shift on the lower half and a moved highlight.
pub fn sample_pair() -> (RgbaImage, RgbaImage) {
    let local = artwork(960, 720);
    let mut matched = local.clone();
    for y in 360..720 {
        for x in 0..960 {
            let p = matched.get_pixel_mut(x, y);
            for c in 0..3 {
                p[c] = p[c].saturating_add(40);
            }
        }
    }
    fill_ellipse(&mut matched, 520.0, 190.0, 580.0, 250.0, [250, 250, 250]);
    fill_ellipse(&mut matched, 365.0, 158.0, 423.0, 202.0, [238, 220, 205]);
    (local, matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_has_the_python_recipe_colours() {
        let a = artwork(400, 300);
        assert_eq!(a.get_pixel(2, 2).0, [238, 220, 205, 255]); // background
        assert_eq!(a.get_pixel(200, 100).0, [30, 32, 48, 255]); // ellipse
        assert_eq!(a.get_pixel(200, 290 - 40).0[..3], [58, 92, 176]); // rectangle
    }

    #[test]
    fn sample_pair_differs_but_shares_size() {
        let (a, b) = sample_pair();
        assert_eq!(a.dimensions(), b.dimensions());
        assert!(crate::diff::abs_diff(&a, &b).changed > 0);
    }
}
