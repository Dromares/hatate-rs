//! Absolute per-pixel difference between two images, the data behind the Review
//! "Differences" overlay.

use image::{RgbaImage, imageops};

/// Result of [`abs_diff`].
pub struct Diff {
    /// `|a - b|` per colour channel, opaque. Black means identical.
    pub image: RgbaImage,
    /// Pixels where any channel differs.
    pub changed: u64,
    /// Largest single-channel difference.
    pub max: u8,
}

/// Absolute per-pixel, per-channel difference. When the sizes differ, `b` is
/// resized to `a` first (Triangle filter), so the overlay always covers `a`.
pub fn abs_diff(a: &RgbaImage, b: &RgbaImage) -> Diff {
    let resized;
    let b = if a.dimensions() == b.dimensions() {
        b
    } else {
        resized = imageops::resize(b, a.width(), a.height(), imageops::FilterType::Triangle);
        &resized
    };
    let mut out = RgbaImage::new(a.width(), a.height());
    let (mut changed, mut max) = (0u64, 0u8);
    for ((pa, pb), po) in a.pixels().zip(b.pixels()).zip(out.pixels_mut()) {
        let mut any = false;
        for c in 0..3 {
            let d = pa[c].abs_diff(pb[c]);
            po[c] = d;
            any |= d != 0;
            max = max.max(d);
        }
        po[3] = 255;
        changed += u64::from(any);
    }
    Diff {
        image: out,
        changed,
        max,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn identical_images_have_no_difference() {
        let a = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
        let d = abs_diff(&a, &a);
        assert_eq!((d.changed, d.max), (0, 0));
        assert!(d.image.pixels().all(|p| p.0 == [0, 0, 0, 255]));
    }

    #[test]
    fn difference_is_absolute_and_symmetric() {
        let mut a = RgbaImage::from_pixel(2, 2, Rgba([100, 100, 100, 255]));
        let mut b = a.clone();
        a.put_pixel(1, 0, Rgba([90, 100, 130, 255]));
        b.put_pixel(1, 0, Rgba([100, 140, 100, 255]));
        let ab = abs_diff(&a, &b);
        let ba = abs_diff(&b, &a);
        assert_eq!(ab.image.get_pixel(1, 0).0, [10, 40, 30, 255]);
        assert_eq!(ab.image, ba.image);
        assert_eq!((ab.changed, ab.max), (1, 40));
    }

    #[test]
    fn mismatched_sizes_are_resized_to_the_first() {
        let a = RgbaImage::from_pixel(8, 8, Rgba([50, 50, 50, 255]));
        let b = RgbaImage::from_pixel(3, 5, Rgba([50, 50, 50, 255]));
        let d = abs_diff(&a, &b);
        assert_eq!(d.image.dimensions(), (8, 8));
        assert_eq!(d.changed, 0);
    }
}
