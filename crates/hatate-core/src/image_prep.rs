//! `core/image_prep.py`: prepares an image for uploading to IQDB, SauceNAO and trace.moe.
//!
//! The services enforce their own upload caps and answer 413 above them. They only need enough
//! detail to match perceptually, so an oversized file is downscaled and re-encoded as JPEG here
//! instead of being sent as it is.
//!
//! **What is matched with Python.** The decision (send the file as it is, or re-encode), the
//! output dimensions, the output format (JPEG, with a `.jpg` name) and staying under the byte
//! cap whenever Python does. **What is not:** the re-encoded JPEG's bytes (granted by name on
//! DAN-2057). The resampler is Pillow's Lanczos ported (`resample_axis`) and the colour
//! conversion and chroma downsampling are libjpeg's (`LibjpegRgb`), so the pixels going into the
//! encoder are Pillow's; the DCT and the Huffman tables are the `jpeg-encoder` crate's, with
//! optimised tables and 4:2:0 like Pillow's `optimize=True`. The sizes it produces sit within
//! about 0.3% of Pillow's, so the quality ladder stops on the same rung for ordinary inputs;
//! `docs/net.md` lists the measured divergence.
//!
//! Never fails for image-processing reasons: it falls back to the raw file bytes if resizing
//! fails for any reason, so a bug here degrades to the old (occasionally 413) behaviour rather
//! than blocking search.

use std::io;
use std::path::Path;
use std::sync::Mutex;

use bytes::Bytes;
use image::DynamicImage;
use jpeg_encoder::{ColorType, Encoder, ImageBuffer, JpegColorType, SamplingFactor};
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
            (DynamicImage::ImageLuma16(g), _) => {
                // Pillow reads 16-bit grey as mode `I;16`, and converting that to RGB clips at 255
                // (it does not scale): a picture whose values run past 255 comes out white.
                let rgb: Vec<u8> = g
                    .as_raw()
                    .iter()
                    .flat_map(|&v| {
                        let v = u8::try_from(v).unwrap_or(255);
                        [v, v, v]
                    })
                    .collect();
                Pixels {
                    width: g.width(),
                    height: g.height(),
                    grey: false,
                    data: rgb,
                }
            }
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
        let channels = self.channels();
        let mut data = std::mem::take(&mut self.data);
        let (mut cur_w, mut cur_h) = (self.width as usize, self.height as usize);
        // Pillow resamples across first, then down, and skips an axis whose size is unchanged.
        if w != self.width {
            data = resample_axis(&data, cur_w, cur_h, channels, w as usize, true);
            cur_w = w as usize;
        }
        if h != self.height {
            data = resample_axis(&data, cur_w, cur_h, channels, h as usize, false);
            cur_h = h as usize;
        }
        debug_assert_eq!(data.len(), cur_w * cur_h * channels);
        self.width = w;
        self.height = h;
        self.data = data;
        Ok(())
    }

    fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>, String> {
        self.to_jpeg_with(quality, true)
    }

    /// `save(out, "JPEG", quality=quality[, optimize=True])`.
    fn to_jpeg_with(&self, quality: u8, optimize: bool) -> Result<Vec<u8>, String> {
        let side = |n: u32| u16::try_from(n).map_err(|_| "image too large for JPEG".to_owned());
        let mut out = Vec::new();
        let mut enc = Encoder::new(&mut out, quality);
        // Pillow's `optimize` and its default 4:2:0 subsampling.
        enc.set_optimized_huffman_tables(optimize);
        enc.set_sampling_factor(SamplingFactor::F_2_2);
        if self.grey {
            enc.encode(
                &self.data,
                side(self.width)?,
                side(self.height)?,
                ColorType::Luma,
            )
            .map_err(|e| e.to_string())?;
            return Ok(without_chroma_quantization_table(out));
        }
        enc.encode_image(LibjpegRgb {
            data: &self.data,
            width: side(self.width)?,
            height: side(self.height)?,
        })
        .map_err(|e| e.to_string())?;
        Ok(out)
    }
}

/// The encoder writes both quantization tables even for a one-component picture; libjpeg writes
/// only the one in use (table 0). Drops the unused table (`DQT`, id 1) from the headers, which
/// are 69 bytes in a stream whose whole body can be a couple of kilobytes.
fn without_chroma_quantization_table(jpeg: Vec<u8>) -> Vec<u8> {
    let mut at = 2;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF && jpeg[at + 1] != 0xDA {
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]])) + 2;
        let is_table_1 = jpeg[at + 1] == 0xDB && jpeg.get(at + 4).is_some_and(|&b| b & 0x0F == 1);
        if is_table_1 && at + len <= jpeg.len() {
            let mut out = jpeg;
            out.drain(at..at + len);
            return out;
        }
        at += len;
    }
    jpeg
}

/// RGB pixels as libjpeg hands them to the DCT: converted to YCbCr with its fixed-point
/// arithmetic, and each chroma sample the average of its 2x2 block (`h2v2_downsample`, with the
/// rounding bias alternating 1, 2 along a row; the last row and column repeat). The encoder's own
/// 4:2:0 path keeps every second chroma sample and drops the rest, which makes noisy pictures
/// about 5% larger than Pillow's at the same quality and moves the rung the quality ladder stops
/// on. It reads the chroma at the even rows and columns of what `fill_buffers` pushes, so each
/// position carries the average of the block it falls in.
struct LibjpegRgb<'a> {
    data: &'a [u8],
    width: u16,
    height: u16,
}

impl LibjpegRgb<'_> {
    /// `rgb_ycc_convert`: (Y, Cb, Cr) of the pixel at column `x`, row `y`.
    fn ycc(&self, x: usize, y: usize) -> (i32, i32, i32) {
        let at = (y * usize::from(self.width) + x) * 3;
        let (r, g, b) = (
            i32::from(self.data[at]),
            i32::from(self.data[at + 1]),
            i32::from(self.data[at + 2]),
        );
        const HALF: i32 = 1 << 15;
        const CENTRE: i32 = 128 << 16;
        (
            (19595 * r + 38470 * g + 7471 * b + HALF) >> 16,
            (-11059 * r - 21709 * g + 32768 * b + CENTRE + HALF - 1) >> 16,
            (32768 * r - 27439 * g - 5329 * b + CENTRE + HALF - 1) >> 16,
        )
    }
}

impl ImageBuffer for LibjpegRgb<'_> {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        JpegColorType::Ycbcr
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn fill_buffers(&self, y: u16, buffers: &mut [Vec<u8>; 4]) {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let y0 = usize::from(y) & !1;
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..w {
            buffers[0].push(self.ycc(x, usize::from(y)).0 as u8);
            let x0 = x & !1;
            let x1 = (x0 + 1).min(w - 1);
            let bias = if (x0 / 2) % 2 == 0 { 1 } else { 2 };
            let (mut cb, mut cr) = (bias, bias);
            for (px, py) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                let (_, b, r) = self.ycc(px, py);
                cb += b;
                cr += r;
            }
            buffers[1].push((cb >> 2) as u8);
            buffers[2].push((cr >> 2) as u8);
        }
    }
}

/// Pillow's Lanczos filter (`Resample.c`, support 3).
fn lanczos(x: f64) -> f64 {
    fn sinc(x: f64) -> f64 {
        if x == 0.0 {
            1.0
        } else {
            let x = x * std::f64::consts::PI;
            x.sin() / x
        }
    }
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// One pass of Pillow's `ImagingResample` for 8-bit pixels (`precompute_coeffs`,
/// `normalize_coeffs_8bpc` and the `8bpc` loops), across (`horizontal`) or down. The weights are
/// 22-bit fixed point and the sum is rounded and clamped like Pillow's, so the pixels equal
/// Pillow's `Image.resize(..., Image.LANCZOS)`. The size of a JPEG made from them then follows
/// Pillow's closely, so the quality ladder and the shrink loop stop where Python's do.
fn resample_axis(
    src: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    out_size: usize,
    horizontal: bool,
) -> Vec<u8> {
    const PRECISION_BITS: u32 = 32 - 8 - 2;
    let in_size = if horizontal { width } else { height };
    let scale = in_size as f64 / out_size as f64;
    let filter_scale = scale.max(1.0);
    let support = 3.0 * filter_scale;
    let ss = 1.0 / filter_scale;
    // Per output position: the first input index and its fixed-point weights.
    let mut taps: Vec<(usize, Vec<i32>)> = Vec::with_capacity(out_size);
    for xx in 0..out_size {
        let center = (xx as f64 + 0.5) * scale;
        let xmin = ((center - support + 0.5) as i64).max(0) as usize;
        let xmax = (((center + support + 0.5) as i64).max(0) as usize).min(in_size);
        let mut k: Vec<f64> = (0..xmax.saturating_sub(xmin))
            .map(|x| lanczos(((x + xmin) as f64 - center + 0.5) * ss))
            .collect();
        let ww: f64 = k.iter().sum();
        if ww != 0.0 {
            k.iter_mut().for_each(|w| *w /= ww);
        }
        let fixed = k
            .iter()
            .map(|&w| {
                let scaled = w * f64::from(1u32 << PRECISION_BITS);
                (if w < 0.0 { -0.5 + scaled } else { 0.5 + scaled }) as i32
            })
            .collect();
        taps.push((xmin, fixed));
    }
    let clip = |sum: i64| (sum >> PRECISION_BITS).clamp(0, 255) as u8;
    let round = 1i64 << (PRECISION_BITS - 1);
    if horizontal {
        let mut out = vec![0u8; out_size * height * channels];
        for y in 0..height {
            let row = &src[y * width * channels..(y + 1) * width * channels];
            for (xx, (xmin, k)) in taps.iter().enumerate() {
                for c in 0..channels {
                    let sum = k.iter().enumerate().fold(round, |acc, (i, &w)| {
                        acc + i64::from(row[(xmin + i) * channels + c]) * i64::from(w)
                    });
                    out[(y * out_size + xx) * channels + c] = clip(sum);
                }
            }
        }
        out
    } else {
        let mut out = vec![0u8; width * out_size * channels];
        for (yy, (ymin, k)) in taps.iter().enumerate() {
            for x in 0..width * channels {
                let sum = k.iter().enumerate().fold(round, |acc, (i, &w)| {
                    acc + i64::from(src[(ymin + i) * width * channels + x]) * i64::from(w)
                });
                out[yy * width * channels + x] = clip(sum);
            }
        }
        out
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

/// What `yandex.prepare_upload` does to the prepared bytes: opens them, takes the first frame,
/// converts to RGB, fits it in a `max_side` square (`Image.thumbnail`, Lanczos) and saves a JPEG
/// at `quality` (Pillow's defaults otherwise: standard Huffman tables, 4:2:0).
///
/// `Err` where Python's `except (OSError, ValueError, EOFError, DecompressionBombError)` catches:
/// bytes Pillow cannot open or decode, and a picture past its decompression-bomb limit. As with
/// [`prepare_upload_bytes`], the JPEG's bytes are not Pillow's (the DCT and the entropy coder are
/// the `jpeg-encoder` crate's); its dimensions and colour are.
pub fn reencode_jpeg(data: &[u8], max_side: u32, quality: u8) -> Result<Vec<u8>, String> {
    if let Err(bomb) = crate::pyimage::image_size_checked(data) {
        return Err(bomb.to_string());
    }
    let mode = mode_from_header(&data[..data.len().min(1100)]);
    let img = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let mut picture = Pixels::from_image(img, mode);
    if picture.grey {
        // `convert("RGB")`: the grey in all three channels.
        picture.data = picture.data.iter().flat_map(|&g| [g, g, g]).collect();
        picture.grey = false;
    }
    picture.thumbnail((i64::from(max_side), i64::from(max_side)))?;
    picture.to_jpeg_with(quality, false)
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
