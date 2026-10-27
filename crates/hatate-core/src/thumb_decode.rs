//! The decode half of the row thumbnails: a file in, a picture at most `limit` pixels on a side
//! out, or `None` when it cannot be read (`ThumbnailWorker._decode`, `workers/thumbnail_worker.py`
//! at `76c85c4`). Pure code: no GTK, so the worker thread and a differential can both call it.
//!
//! Python asks `QImageReader` for a scaled decode (`setScaledSize`) with `setAutoTransform(True)`,
//! so the EXIF orientation is applied and the size the decoder is asked for is
//! `QSize.scaled(limit x limit, KeepAspectRatio)` of the file's stored size ([`fit_within`]).
//! Here the `image` crate decodes, the orientation is applied, and the picture is resized to the
//! same size with a triangle filter. Qt scales JPEGs inside the decoder (by 1/2, 1/4, 1/8) and
//! other formats smoothly afterwards, so the pixels are not Qt's: that is the decode-library
//! difference DAN-2217 names, measured in its differential.

use crate::thumb::fit_within;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};

/// A decoded thumbnail: straight (non-premultiplied) RGBA8, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The size the Qt reader decodes a `width` x `height` picture at for a `limit`: [`fit_within`],
/// except where that truncates the thin side to 0. `QImageReader` then still decodes at 1 pixel
/// when the exact size rounds to 1 or more (`2 * thin * limit >= long`) and fails below that
/// (probed against `ThumbnailWorker._decode`, DAN-2217 B1). `None` for no picture.
fn reader_size(width: u32, height: u32, limit: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (w, h) = fit_within(width, height, limit);
    if w != 0 && h != 0 {
        return Some((w, h));
    }
    let (thin, long) = (u64::from(width.min(height)), u64::from(width.max(height)));
    if 2 * thin * u64::from(limit) < long {
        return None;
    }
    Some((w.max(1), h.max(1)))
}

/// The thumbnail of the file at `path`, at most `limit` on a side. `None` for a file that is
/// missing, unreadable or not a picture, and for a picture Qt would scale to nothing.
pub fn decode_thumbnail(path: &str, limit: u32) -> Option<ThumbPixels> {
    let mut decoder = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    let (w, h) = reader_size(picture.width(), picture.height(), limit)?;
    if (w, h) != (picture.width(), picture.height()) {
        picture = picture.resize_exact(w, h, FilterType::Triangle);
    }
    let rgba = picture.into_rgba8();
    Some(ThumbPixels {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(dir: &std::path::Path, name: &str, w: u32, h: u32) -> String {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 60, 255]))
            .save(&path)
            .unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_large_picture_is_fitted_to_the_limit_keeping_its_aspect() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_png(dir.path(), "wide.png", 1600, 800);
        let t = decode_thumbnail(&p, 160).unwrap();
        assert_eq!((t.width, t.height), (160, 80));
        assert_eq!(t.rgba.len(), 160 * 80 * 4);
        assert_eq!(
            &t.rgba[..4],
            &[200, 30, 60, 255],
            "a flat colour stays flat"
        );
    }

    #[test]
    fn a_small_picture_is_left_at_its_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_png(dir.path(), "small.png", 50, 30);
        let t = decode_thumbnail(&p, 160).unwrap();
        assert_eq!((t.width, t.height), (50, 30));
    }

    #[test]
    fn a_missing_file_a_text_file_and_a_sliver_are_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(decode_thumbnail("/nonexistent/none.png", 160).is_none());
        let txt = dir.path().join("note.png");
        std::fs::write(&txt, b"not a picture").unwrap();
        assert!(decode_thumbnail(&txt.to_string_lossy(), 160).is_none());
        // Qt truncates 160 * 3 / 1000 to 0, so the scaled size is empty and the read fails.
        let sliver = write_png(dir.path(), "sliver.png", 1000, 3);
        assert!(decode_thumbnail(&sliver, 160).is_none());
    }

    /// The reader does not fail on every empty scaled size (differential, DAN-2217 B1): where
    /// `QSize.scaled` truncates the thin side to 0, `QImageReader` still decodes it at 1 pixel
    /// when the exact size rounds to 1 or more (`2 * thin * limit >= long`), and fails below that.
    /// Probed against `ThumbnailWorker._decode` over PNG, JPEG, GIF, BMP and WebP.
    #[test]
    fn a_sliver_whose_exact_size_rounds_to_a_pixel_is_decoded_at_one() {
        let dir = tempfile::tempdir().unwrap();
        let size = |name: &str, w: u32, h: u32, limit: u32| {
            let p = write_png(dir.path(), name, w, h);
            decode_thumbnail(&p, limit).map(|t| (t.width, t.height))
        };
        assert_eq!(
            size("a.png", 18, 2308, 96),
            Some((1, 96)),
            "96 * 18 / 2308 = 0.75"
        );
        assert_eq!(
            size("b.png", 2308, 18, 96),
            Some((96, 1)),
            "and the same on its side"
        );
        assert_eq!(size("c.png", 1, 192, 96), Some((1, 96)), "0.5 rounds up");
        assert_eq!(size("d.png", 1, 193, 96), None, "0.497 does not");
        assert_eq!(size("e.png", 3, 500, 96), Some((1, 96)));
        assert_eq!(size("f.png", 3, 1000, 96), None);
    }
}
