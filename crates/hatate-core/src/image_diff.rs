//! `core/image_diff.py`: where two copies of the same picture actually differ.
//!
//! Both pictures are scaled to one working size, levelled against each other per channel,
//! differenced (the strongest channel), blurred to throw away re-encoding noise, and thresholded.
//! What survives is the regions that changed. The result is approximate by design: it says "this
//! area differs", never "these exact pixels differ".
//!
//! Every field of Python's `DifferenceMap` (`mask`, `changed_fraction`, `verdict`, `reliable`) is
//! reproduced bit for bit: the resampling is `pyraster`'s port of Pillow's, the blur is Pillow's
//! three box-blur passes, and the float32 medians and the mask scaling are numpy's own arithmetic
//! (`pynumpy`). On top of that this module additively returns what the compare dialog needs to
//! draw a box: the blurred difference before the threshold (`delta`) and the bounding box of the
//! thresholded mask in each picture's own pixel coordinates (`local_rect`, `match_rect`). Those
//! have no Python counterpart.

use std::fmt;
use std::path::Path;

use crate::error::Kind;
use crate::image_compare::Source;
use crate::pynumpy::{median_f32, py_round};
use crate::pyraster::{self, Filter, Image, Mode, OpenError};

/// Both images are resampled to this long edge before comparing.
pub const WORK_LONG_EDGE: usize = 900;
/// Radius of the blur applied to the difference.
pub const NOISE_BLUR_RADIUS: f32 = 2.0;
/// Blurred difference, 0 to 255, below which a pixel counts as unchanged.
pub const CHANGE_THRESHOLD: u8 = 26;
/// Above this fraction the differences are no longer localised.
pub const WHOLESALE_FRACTION: f64 = 0.45;
/// Aspect ratios further apart than this cannot be scaled into the same box.
pub const ASPECT_TOLERANCE: f64 = 0.06;

/// A box in a picture's own pixel coordinates: `x`, `y` is the top-left pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// `DifferenceMap`, plus the additive fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    /// `DifferenceMap.mask`: mode `L`, `width * height`, row-major, 0 unchanged and brighter
    /// meaning more different (the strongest change reads as 255). `None` is Python's
    /// `mask=None` ("Nothing to compare").
    pub mask: Option<Vec<u8>>,
    /// The working size the mask and `delta` are at (Python's mask size).
    pub width: usize,
    pub height: usize,
    /// 0 to 1 of the frame that changed.
    pub changed_fraction: f64,
    /// What to tell the user.
    pub verdict: String,
    /// `false` when the two do not line up well enough to trust.
    pub reliable: bool,
    /// The blurred difference before the threshold: one byte per pixel, row-major, at the
    /// working size. This is the strongest-channel difference Python thresholds, not a
    /// luminance difference. `mask` is zero exactly where `delta < CHANGE_THRESHOLD`.
    pub delta: Vec<u8>,
    /// The bounding box of the thresholded mask in the matched picture's pixels; `None` when
    /// nothing changed.
    pub match_rect: Option<Rect>,
    /// The same box in the local picture's pixels.
    pub local_rect: Option<Rect>,
}

impl Diff {
    /// `DifferenceMap.changed_percent`.
    pub fn changed_percent(&self) -> f64 {
        self.changed_fraction * 100.0
    }

    fn nothing_to_compare() -> Diff {
        Diff {
            mask: None,
            width: 0,
            height: 0,
            changed_fraction: 0.0,
            verdict: "Nothing to compare".to_string(),
            reliable: false,
            delta: Vec::new(),
            match_rect: None,
            local_rect: None,
        }
    }
}

/// Which of the two pictures could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Local,
    Match,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Local => "local",
            Side::Match => "matched",
        }
    }
}

/// Why a difference map could not be made: one of the pictures did not open.
#[derive(Debug, Clone)]
pub struct DiffError {
    side: Side,
    kind: Kind,
    detail: String,
}

impl DiffError {
    fn new(side: Side, error: &OpenError, from_disk: Option<&std::io::Error>) -> DiffError {
        let (kind, detail) = match (error, from_disk) {
            (_, Some(io)) => (Kind::ImageUnreadable, io.to_string()),
            (OpenError::Bomb(b), None) => (Kind::ImageTooLarge, b.to_string()),
            (OpenError::Os(m), None) => (Kind::ImageUndecodable, m.clone()),
        };
        DiffError { side, kind, detail }
    }

    /// Which picture it was.
    pub fn side(&self) -> Side {
        self.side
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// What failed and what to do; no path and no operating-system text.
    pub fn user_message(&self) -> String {
        let which = self.side.word();
        match self.kind {
            Kind::ImageUnreadable => format!(
                "The {which} image could not be read. Check that the file still exists and \
                 that Hatate may open it."
            ),
            Kind::ImageTooLarge => {
                format!("The {which} image is too large to compare, so it was skipped.")
            }
            _ => format!(
                "The {which} image could not be decoded. It may be damaged or in a format \
                 Hatate cannot read."
            ),
        }
    }

    /// The raw text: the operating-system error or the decoder's message.
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for DiffError {}

fn load(source: Source<'_>, side: Side) -> Result<Image, DiffError> {
    match source {
        Source::Image(img) => Ok(img.clone()),
        Source::Bytes(b) => pyraster::open(b, None).map_err(|e| DiffError::new(side, &e, None)),
        Source::Path(p) => {
            let data = read(p)
                .map_err(|e| DiffError::new(side, &OpenError::Os(String::new()), Some(&e)))?;
            pyraster::open(&data, None).map_err(|e| DiffError::new(side, &e, None))
        }
    }
}

fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// `difference_map(local, match)`: where `remote` differs from `local`, as a mask over `local`'s
/// frame.
pub fn difference_map(local: Source<'_>, remote: Source<'_>) -> Result<Diff, DiffError> {
    let local = load(local, Side::Local)?;
    let remote = load(remote, Side::Match)?;
    Ok(difference_map_images(Some(&local), Some(&remote)))
}

/// `difference_map` on pictures already open. `None` is Python's `None`.
pub fn difference_map_images(local: Option<&Image>, remote: Option<&Image>) -> Diff {
    let (Some(local), Some(remote)) = (local, remote) else {
        return Diff::nothing_to_compare();
    };
    let size = working_size(local);
    if size == (0, 0) {
        return Diff::nothing_to_compare();
    }
    let (width, height) = size;
    let aspect_gap = (aspect(local) - aspect(remote)).abs();
    let aspects_match = aspect_gap <= ASPECT_TOLERANCE * 1.0f64.max(aspect(local));

    let a = prepare(local, size);
    let mut b = prepare(remote, size);
    let pixels = width * height;

    // Level-matched per channel, on the median.
    for channel in 0..3 {
        let mut plane_a: Vec<f32> = (0..pixels).map(|i| a[i * 3 + channel]).collect();
        let mut plane_b: Vec<f32> = (0..pixels).map(|i| b[i * 3 + channel]).collect();
        let shift = (median_f32(&mut plane_a) - median_f32(&mut plane_b)) as f32;
        for i in 0..pixels {
            b[i * 3 + channel] += shift;
        }
    }

    // The strongest channel disagreement, blurred.
    let diff: Vec<u8> = (0..pixels)
        .map(|i| {
            let m = (0..3)
                .map(|c| (a[i * 3 + c] - b[i * 3 + c]).abs())
                .fold(f32::MIN, f32::max);
            m.clamp(0.0, 255.0) as u8
        })
        .collect();
    let blurred = Image {
        width,
        height,
        mode: Mode::L,
        data: diff,
    }
    .gaussian_blur(NOISE_BLUR_RADIUS);
    let delta = blurred.data;

    let changed = delta.iter().filter(|&&v| v >= CHANGE_THRESHOLD).count();
    let fraction = changed as f64 / pixels as f64;

    // Only what cleared the threshold is shown, scaled so the strongest difference is 255.
    let peak = delta
        .iter()
        .map(|&v| if v >= CHANGE_THRESHOLD { v } else { 0 })
        .max()
        .unwrap_or(0);
    let scale = (255.0 / f64::from(peak)) as f32;
    let mask: Vec<u8> = delta
        .iter()
        .map(|&v| {
            if v < CHANGE_THRESHOLD {
                0
            } else if peak > 0 {
                (f32::from(v) * scale).clamp(0.0, 255.0) as u8
            } else {
                v
            }
        })
        .collect();

    let bounds = bounding_box(&mask, width, height);
    let reliable = aspects_match && fraction < WHOLESALE_FRACTION;
    Diff {
        local_rect: bounds.map(|b| to_picture(b, size, (local.width, local.height))),
        match_rect: bounds.map(|b| to_picture(b, size, (remote.width, remote.height))),
        mask: Some(mask),
        width,
        height,
        changed_fraction: fraction,
        verdict: verdict(fraction, aspects_match),
        reliable,
        delta,
    }
}

fn aspect(image: &Image) -> f64 {
    if image.height == 0 {
        0.0
    } else {
        image.width as f64 / image.height as f64
    }
}

/// `_working_size`: the long edge brought down to `WORK_LONG_EDGE`, never scaled up.
fn working_size(local: &Image) -> (usize, usize) {
    let (w, h) = (local.width, local.height);
    if w == 0 || h == 0 {
        return (0, 0);
    }
    let scale = WORK_LONG_EDGE as f64 / w.max(h) as f64;
    if scale >= 1.0 {
        return (w, h);
    }
    (
        py_round(w as f64 * scale).max(1) as usize,
        py_round(h as f64 * scale).max(1) as usize,
    )
}

/// `_prepare`: one picture as a float RGB array at the working size.
fn prepare(image: &Image, size: (usize, usize)) -> Vec<f32> {
    image
        .to_rgb()
        .resize_filtered(size.0, size.1, Filter::Lanczos)
        .data
        .iter()
        .map(|&v| f32::from(v))
        .collect()
}

/// The box around every non-zero mask pixel, `(x0, y0, x1, y1)` with the far edge exclusive.
fn bounding_box(mask: &[u8], width: usize, height: usize) -> Option<(usize, usize, usize, usize)> {
    let mut found: Option<(usize, usize, usize, usize)> = None;
    for y in 0..height {
        for x in 0..width {
            if mask[y * width + x] != 0 {
                found = Some(match found {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    found
}

/// A box at the working size, in a picture of `size` pixels: the pixels the box covers, rounded
/// outwards.
fn to_picture(
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    work: (usize, usize),
    size: (usize, usize),
) -> Rect {
    let lo = |v: usize, from: usize, to: usize| v * to / from;
    let hi = |v: usize, from: usize, to: usize| (v * to).div_ceil(from).min(to);
    let (rx0, ry0) = (lo(x0, work.0, size.0), lo(y0, work.1, size.1));
    let (rx1, ry1) = (hi(x1, work.0, size.0), hi(y1, work.1, size.1));
    Rect {
        x: rx0 as u32,
        y: ry0 as u32,
        w: (rx1.max(rx0 + 1).min(size.0) - rx0) as u32,
        h: (ry1.max(ry0 + 1).min(size.1) - ry0) as u32,
    }
}

/// `_verdict`.
pub fn verdict(fraction: f64, aspects_match: bool) -> String {
    if !aspects_match {
        return "These are different shapes, so they cannot be lined up - the highlights \
                below are the mismatch, not an edit. Compare them under the wipe instead."
            .to_string();
    }
    let percent = fraction * 100.0;
    if fraction >= WHOLESALE_FRACTION {
        return format!(
            "{percent:.0}% of the frame differs - too much for this view to say WHAT \
             changed. Either a heavily reworked version, a different crop, or a \
             different picture; the perceptual match above is the better guide."
        );
    }
    if fraction < 0.005 {
        return "No visible differences - the two look like the same file's content.".to_string();
    }
    if fraction < 0.05 {
        return format!(
            "{percent:.1}% of the frame differs, in small patches - typically a \
             signature, a watermark or a small caption."
        );
    }
    format!(
        "{percent:.0}% of the frame differs - large enough to be a real edit: added \
         text, a censoring bar, a logo, or a retouch."
    )
}
