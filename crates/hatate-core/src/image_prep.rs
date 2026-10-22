//! `core/image_prep.py`: prepares an image for uploading to IQDB, SauceNAO and trace.moe.
//!
//! The services enforce their own upload caps and answer 413 above them. They only need enough
//! detail to match perceptually, so an oversized file is downscaled and re-encoded as JPEG here
//! instead of being sent as it is.
//!
//! **What is matched with Python.** The decision (send the file as it is, or re-encode), the
//! output dimensions, the output format (JPEG, with a `.jpg` name) and staying under the byte
//! cap whenever Python does. **What is not:** the re-encoded JPEG's bytes. Pillow's decoder,
//! Lanczos resampler and libjpeg are not bit-reproduced; that difference is granted by name on
//! DAN-2057. The encoder here does optimise its Huffman tables and subsample 4:2:0 like Pillow's
//! `optimize=True`, so the sizes it produces sit close to Pillow's and the quality ladder stops
//! on the same rung for ordinary inputs; `docs/net.md` lists the measured divergence.
//!
//! Never fails for image-processing reasons: it falls back to the raw file bytes if resizing
//! fails for any reason, so a bug here degrades to the old (occasionally 413) behaviour rather
//! than blocking search.

use std::io;
use std::path::Path;
use std::sync::Mutex;

use bytes::Bytes;
use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::DynamicImage;
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::pyrepr::py_repr;

/// IQDB's and SauceNAO's own limits are commonly cited around 8 MiB; this leaves headroom.
pub const DEFAULT_MAX_BYTES: u64 = 7 * 1024 * 1024;
/// Plenty of detail for perceptual matching.
pub const DEFAULT_MAX_DIMENSION: u32 = 3000;

/// The quality the first attempt is made at, and the lowest rung of the ladder.
const FIRST_QUALITY: u8 = 90;
const LOWEST_QUALITY: u8 = 40;
/// The quality the shrink loop re-encodes at.
const SHRINK_QUALITY: u8 = 70;
/// The shrink loop stops at this long side: below it there is too little detail left to match.
const SHRINK_FLOOR: u32 = 500;

/// The single most recent prepared upload. IQDB and SauceNAO each prepare the same file seconds
/// apart within one search; re-decoding a large PNG a second time is pure waste.
///
/// Keyed on `(path, file size, max_bytes, max_dimension)`; the file size doubles as a cheap check
/// that the file did not change between the calls.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub path: String,
    pub file_size: u64,
    pub max_bytes: u64,
    pub max_dimension: u32,
    pub data: Bytes,
    pub filename: String,
}

/// The one-entry cache and the lock around it. The process has one ([`prepare_upload_bytes`]);
/// tests make their own so they do not share state.
#[derive(Debug, Default)]
pub struct UploadPrep {
    last: Mutex<Option<Prepared>>,
}

/// What a downscale produced: the JPEG and its filename.
type Downscaled = (Vec<u8>, String);

static GLOBAL: UploadPrep = UploadPrep {
    last: Mutex::new(None),
};

/// `prepare_upload_bytes`: `(bytes, filename)` for an upload.
///
/// A file already at or under `max_bytes` is sent as it is (original format and name). Otherwise
/// it is resized down and re-encoded as JPEG, reducing the quality and then the dimensions until
/// it fits. `Err` only when the file cannot be read at all; that is Python's `OSError`.
///
/// Blocking: decoding and encoding a large image takes seconds. Async callers use
/// `tokio::task::spawn_blocking`.
pub fn prepare_upload_bytes(
    path: &Path,
    max_bytes: u64,
    max_dimension: u32,
) -> io::Result<(Bytes, String)> {
    GLOBAL.prepare(path, max_bytes, max_dimension)
}

/// What the process-wide cache holds: Python's `image_prep._last_prepared`.
pub fn last_prepared() -> Option<Prepared> {
    GLOBAL.last_prepared()
}

/// Empties the process-wide cache (Python tests set `_last_prepared = None`).
pub fn reset_cache() {
    GLOBAL.reset();
}

impl UploadPrep {
    pub const fn new() -> Self {
        UploadPrep {
            last: Mutex::new(None),
        }
    }

    pub fn last_prepared(&self) -> Option<Prepared> {
        self.lock().clone()
    }

    pub fn reset(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Prepared>> {
        // A panic while the lock was held cannot leave the entry half-written: it is replaced
        // whole. So a poisoned lock is as good as a clean one.
        self.last.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// [`prepare_upload_bytes`] on this cache.
    pub fn prepare(
        &self,
        path: &Path,
        max_bytes: u64,
        max_dimension: u32,
    ) -> io::Result<(Bytes, String)> {
        self.prepare_with(
            path,
            max_bytes,
            max_dimension,
            |p| std::fs::metadata(p).map(|m| m.len()),
            downscale_to_fit,
        )
    }

    /// `prepare`, with the stat and the downscale step as parameters so a test can watch them or
    /// make them fail (Python's tests patch `os.path.getsize` and `_downscale_to_fit`).
    pub(crate) fn prepare_with(
        &self,
        path: &Path,
        max_bytes: u64,
        max_dimension: u32,
        stat: impl FnOnce(&Path) -> io::Result<u64>,
        downscale: impl FnOnce(&Path, &str, u64, u32) -> Result<Downscaled, String>,
    ) -> io::Result<(Bytes, String)> {
        let filename = basename(path);
        let file_size = match stat(path) {
            Ok(size) => size,
            Err(exc) => {
                warn!(
                    "Could not stat {} before upload prep: {}",
                    path.display(),
                    os_error_text(&exc, path)
                );
                // The raw bytes, and not cached: the key would hold a size nobody managed to read.
                return Ok((Bytes::from(std::fs::read(path)?), filename));
            }
        };

        let mut last = self.lock();
        if let Some(c) = last.as_ref()
            && c.path == path.to_string_lossy()
            && c.file_size == file_size
            && c.max_bytes == max_bytes
            && c.max_dimension == max_dimension
        {
            debug!(
                "Reusing already-prepared upload bytes for {filename} - avoids re-decoding the \
                 original a second time for the other engine"
            );
            return Ok((c.data.clone(), c.filename.clone()));
        }

        let remember = |last: &mut Option<Prepared>, data: &Bytes, name: &str| {
            *last = Some(Prepared {
                path: path.to_string_lossy().into_owned(),
                file_size,
                max_bytes,
                max_dimension,
                data: data.clone(),
                filename: name.to_owned(),
            });
        };

        if file_size <= max_bytes {
            let data = Bytes::from(std::fs::read(path)?);
            remember(&mut last, &data, &filename);
            return Ok((data, filename));
        }

        info!(
            "{filename} is {:.1} MB (over the {:.1} MB upload cap), downscaling before search",
            file_size as f64 / 1024.0 / 1024.0,
            max_bytes as f64 / 1024.0 / 1024.0
        );
        // Python's `except Exception`: any failure, including a decoder that panics.
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            downscale(path, &filename, max_bytes, max_dimension)
        }))
        .unwrap_or_else(|_| Err("the image decoder panicked".to_owned()));
        match attempt {
            Ok((data, out_filename)) => {
                info!(
                    "{filename} downscaled for upload: {:.1} MB -> {:.1} MB",
                    file_size as f64 / 1024.0 / 1024.0,
                    data.len() as f64 / 1024.0 / 1024.0
                );
                let data = Bytes::from(data);
                remember(&mut last, &data, &out_filename);
                Ok((data, out_filename))
            }
            Err(exc) => {
                warn!(
                    "Could not downscale {filename} for upload ({exc}), sending original bytes - \
                     the service may reject it with HTTP 413"
                );
                // Not cached: one transient failure must not stick for every later engine.
                Ok((Bytes::from(std::fs::read(path)?), filename))
            }
        }
    }
}

/// `os.path.basename` on a POSIX path: everything after the last `/`.
pub fn basename(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.rfind('/') {
        Some(i) => text[i + 1..].to_owned(),
        None => text.into_owned(),
    }
}

/// `os.path.splitext(name)[0]` for a name with no directory part.
fn splitext_stem(name: &str) -> &str {
    if let Some(dot) = name.rfind('.') {
        // A leading run of dots belongs to the name ("..png" has no extension).
        if name[..dot].chars().any(|c| c != '.') {
            return &name[..dot];
        }
    }
    name
}

/// What Python's `str(OSError)` says for a failed `open`/`stat` of `path`:
/// `[Errno 2] No such file or directory: '/x'`.
pub fn os_error_text(err: &io::Error, path: &Path) -> String {
    match err.raw_os_error() {
        Some(code) => {
            let shown = err.to_string();
            let reason = shown
                .strip_suffix(&format!(" (os error {code})"))
                .unwrap_or(&shown);
            format!(
                "[Errno {code}] {reason}: {}",
                py_repr(&Value::String(path.to_string_lossy().into_owned()))
            )
        }
        None => err.to_string(),
    }
}

/// The pixels the encoder is fed: 8-bit RGB, or 8-bit grey (Pillow's modes `RGB` and `L`, the two
/// it leaves alone; every other mode is converted to RGB, which drops an alpha channel).
struct Pixels {
    width: u32,
    height: u32,
    grey: bool,
    data: Vec<u8>,
}

/// How Pillow's mode for a file differs from the pixels the `image` crate hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PillowMode {
    /// The decoder's own: 8-bit grey stays grey, everything else is RGB.
    AsDecoded,
    /// Pillow reads it as mode `1` (a bilevel picture), which it converts to RGB.
    Rgb,
    /// Pillow reads it as mode `L`: a palette that is the identity grey ramp.
    Grey,
}

impl Pixels {
    fn from_image(img: DynamicImage, mode: PillowMode) -> Pixels {
        match (img, mode) {
            (DynamicImage::ImageLuma8(g), PillowMode::AsDecoded | PillowMode::Grey) => Pixels {
                width: g.width(),
                height: g.height(),
                grey: true,
                data: g.into_raw(),
            },
            (other, PillowMode::Grey) => {
                // A grey-ramp palette decodes to RGB with every channel equal to the index.
                let rgb = other.to_rgb8();
                Pixels {
                    width: rgb.width(),
                    height: rgb.height(),
                    grey: true,
                    data: rgb.pixels().map(|p| p.0[0]).collect(),
                }
            }
            (other, _) => {
                let rgb = other.to_rgb8();
                Pixels {
                    width: rgb.width(),
                    height: rgb.height(),
                    grey: false,
                    data: rgb.into_raw(),
                }
            }
        }
    }

    fn channels(&self) -> usize {
        if self.grey { 1 } else { 3 }
    }

    /// `Image.thumbnail(size, Image.LANCZOS)`, in place.
    fn thumbnail(&mut self, size: (i64, i64)) -> Result<(), String> {
        let Some((w, h)) = thumbnail_size(self.width, self.height, size)? else {
            return Ok(());
        };
        if (w, h) == (self.width, self.height) {
            return Ok(());
        }
        if w == 0 || h == 0 {
            // Pillow's `resize` refuses a zero side ("height and width must be > 0").
            return Err("height and width must be > 0".to_owned());
        }
        let pixel_type = if self.grey {
            PixelType::U8
        } else {
            PixelType::U8x3
        };
        let src = FirImage::from_vec_u8(
            self.width,
            self.height,
            std::mem::take(&mut self.data),
            pixel_type,
        )
        .map_err(|e| e.to_string())?;
        let mut dst = FirImage::new(w, h, pixel_type);
        Resizer::new()
            .resize(
                &src,
                &mut dst,
                &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
            )
            .map_err(|e| e.to_string())?;
        self.width = w;
        self.height = h;
        self.data = dst.into_vec();
        Ok(())
    }

    fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>, String> {
        let side = |n: u32| u16::try_from(n).map_err(|_| "image too large for JPEG".to_owned());
        let mut out = Vec::new();
        let mut enc = Encoder::new(&mut out, quality);
        // Pillow's `optimize=True` and its default 4:2:0 subsampling.
        enc.set_optimized_huffman_tables(true);
        enc.set_sampling_factor(SamplingFactor::F_2_2);
        let color = if self.grey {
            ColorType::Luma
        } else {
            ColorType::Rgb
        };
        debug_assert_eq!(
            self.data.len(),
            (self.width * self.height) as usize * self.channels()
        );
        enc.encode(&self.data, side(self.width)?, side(self.height)?, color)
            .map_err(|e| e.to_string())?;
        Ok(out)
    }
}

/// What Pillow's mode would be for `path`, where it differs from what the decoder returns, read
/// from the file's own header: a bilevel PNG is mode `1` (converted to RGB, where the decoder
/// returns grey), and a GIF or BMP whose palette is the identity grey ramp is mode `L` (kept
/// grey, where the decoder returns RGB). Only the number of channels in the JPEG depends on it;
/// that decides whether the picture fits under the cap, so it is matched.
fn pillow_mode(path: &Path) -> PillowMode {
    use std::io::Read as _;
    let mut head = Vec::with_capacity(1100);
    if std::fs::File::open(path)
        .and_then(|f| f.take(1100).read_to_end(&mut head))
        .is_err()
    {
        return PillowMode::AsDecoded;
    }
    mode_from_header(&head)
}

fn mode_from_header(head: &[u8]) -> PillowMode {
    let le16 = |at: usize| {
        head.get(at..at + 2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let le32 = |at: usize| {
        head.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        // IHDR: width, height, bit depth (offset 24), colour type (offset 25).
        return match (head.get(24), head.get(25)) {
            (Some(1), Some(0)) => PillowMode::Rgb,
            _ => PillowMode::AsDecoded,
        };
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        // The global colour table follows the 13-byte header.
        let flags = head.get(10).copied().unwrap_or(0);
        if flags & 0x80 == 0 {
            return PillowMode::AsDecoded;
        }
        let entries = 1usize << ((flags & 7) + 1);
        let Some(table) = head.get(13..13 + 3 * entries) else {
            return PillowMode::AsDecoded;
        };
        let ramp = table
            .chunks_exact(3)
            .enumerate()
            .all(|(i, rgb)| rgb.iter().all(|&c| usize::from(c) == i));
        return if ramp {
            PillowMode::Grey
        } else {
            PillowMode::AsDecoded
        };
    }
    if head.starts_with(b"BM") {
        // BITMAPINFOHEADER and later: bits per pixel at 28, colours used at 46, the palette
        // (four bytes an entry, B G R and padding) right after the header. Pillow reads a
        // palette of the greys 0..n as `L`, and two entries (0, 255) as `1`.
        let (Some(dib), Some(bits)) = (le32(14), le16(28)) else {
            return PillowMode::AsDecoded;
        };
        if dib < 40 || bits > 8 || bits == 0 {
            return PillowMode::AsDecoded;
        }
        let used = le32(46).unwrap_or(0);
        let colors = if used == 0 { 1usize << bits } else { used };
        let Some(palette) = head.get(14 + dib..14 + dib + 4 * colors) else {
            return PillowMode::AsDecoded;
        };
        let wanted = |i: usize| if colors == 2 { [0u8, 255][i] } else { i as u8 };
        let grey = colors <= 256
            && (0..colors).all(|i| palette[4 * i..4 * i + 3].iter().all(|&c| c == wanted(i)));
        return match (grey, colors) {
            (false, _) => PillowMode::AsDecoded,
            (true, 2) => PillowMode::Rgb,
            (true, _) => PillowMode::Grey,
        };
    }
    PillowMode::AsDecoded
}

/// `Image.thumbnail`'s size arithmetic (Pillow 12): the size the image is resized to, or `None`
/// when the requested box is at least as large as the image in both directions.
///
/// `Err` where Python raises: a zero height in the box divides by zero.
fn thumbnail_size(width: u32, height: u32, size: (i64, i64)) -> Result<Option<(u32, u32)>, String> {
    let (mut x, mut y) = size;
    let (w, h) = (i64::from(width), i64::from(height));
    if x >= w && y >= h {
        return Ok(None);
    }
    let aspect = w as f64 / h as f64;
    if y == 0 {
        return Err("division by zero".to_owned());
    }
    // `max(min(floor(n), ceil(n), key=key), 1)`: the nearer of the two integers (the floor on a
    // tie), and never below 1.
    let round_aspect = |number: f64, key: &dyn Fn(f64) -> f64| -> i64 {
        let (lo, hi) = (number.floor(), number.ceil());
        let pick = if key(hi) < key(lo) { hi } else { lo };
        (pick as i64).max(1)
    };
    if x as f64 / y as f64 >= aspect {
        let yf = y as f64;
        x = round_aspect(yf * aspect, &|n| (aspect - n / yf).abs());
    } else {
        let xf = x as f64;
        y = round_aspect(xf / aspect, &|n| {
            if n == 0.0 {
                0.0
            } else {
                (aspect - xf / n).abs()
            }
        });
    }
    Ok(Some((
        u32::try_from(x).map_err(|_| "size out of range".to_owned())?,
        u32::try_from(y).map_err(|_| "size out of range".to_owned())?,
    )))
}

/// `_downscale_to_fit`: the JPEG bytes and the `.jpg` filename.
fn downscale_to_fit(
    path: &Path,
    filename: &str,
    max_bytes: u64,
    max_dimension: u32,
) -> Result<Downscaled, String> {
    let mode = pillow_mode(path);
    let img = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let mut im = Pixels::from_image(img, mode);

    let max_dimension = i64::from(max_dimension);
    if im.width.max(im.height) > max_dimension as u32 {
        im.thumbnail((max_dimension, max_dimension))?;
    }

    let fits = |buf: &[u8]| buf.len() as u64 <= max_bytes;
    let mut quality = FIRST_QUALITY;
    let mut buf = Vec::new();
    while quality >= LOWEST_QUALITY {
        buf = im.to_jpeg(quality)?;
        if fits(&buf) {
            break;
        }
        quality -= 10;
    }

    // Still too big even at low quality (very large dimensions): shrink further.
    while !fits(&buf) && im.width.max(im.height) > SHRINK_FLOOR {
        im.thumbnail((
            (f64::from(im.width) * 0.8) as i64,
            (f64::from(im.height) * 0.8) as i64,
        ))?;
        buf = im.to_jpeg(SHRINK_QUALITY)?;
    }

    Ok((buf, format!("{}.jpg", splitext_stem(filename))))
}

#[cfg(test)]
mod tests;
