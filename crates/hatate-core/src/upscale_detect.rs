//! `core/upscale_detect.py` and the logic of `workers/upscale_check_worker.py` (without Qt):
//! heuristics for whether an image looks artificially upscaled from a smaller original.
//!
//! Two independent signals, each a heuristic and presented that way:
//!
//! 1. [`compare_to_source`]: the local file is meaningfully bigger than the matched source.
//! 2. [`detect_naive_upscale`]: downscale then re-upscale with common filters and compare, only
//!    where the picture has fine detail ([`detail_mask`]). Simple (bicubic, bilinear, nearest)
//!    upscaling is nearly reversible this way. AI upscalers are not caught.
//!
//! plus [`check_upscaler_metadata`], a literal AI-upscaler name in the file's own metadata.
//!
//! Every float is numpy's `float32` arithmetic reproduced (`pynumpy`), and every resize is
//! `pyraster`'s port of Pillow's, so a result equals Python's field for field.

use std::path::Path;

use serde_json::Value;

use crate::model::Entry;
use crate::pyimage::DecompressionBomb;
use crate::pynumpy::{mean_f32, percentile_f32, py_round};
use crate::pyraster::{self, Filter, Image, OpenError};
use crate::pyrepr::py_repr;

/// Common upscale factors tested in the self-consistency check.
pub const CANDIDATE_FACTORS: [f64; 4] = [1.5, 2.0, 3.0, 4.0];
/// The resize filters tested, in Python's dict order (ties keep the first).
pub const CANDIDATE_FILTERS: [(&str, Filter); 4] = [
    ("Bicubic", Filter::Bicubic),
    ("Bilinear", Filter::Bilinear),
    ("Nearest-neighbor", Filter::Nearest),
    ("Lanczos", Filter::Lanczos),
];
/// Average round-trip difference (0 to 255), over detail pixels, at or below which the verdict is
/// "likely".
pub const STRONG_MATCH_THRESHOLD: f64 = 4.0;
/// ... and "possible".
pub const POSSIBLE_MATCH_THRESHOLD: f64 = 10.0;
/// Only pixels at or above this percentile of the gradient magnitude count as detail.
pub const DETAIL_PERCENTILE: f64 = 70.0;
/// A pixel needs at least this gradient magnitude to be a detail candidate at all.
pub const MIN_GRADIENT: f64 = 2.0;
/// Below this fraction of detail pixels there is not enough signal.
pub const MIN_DETAIL_FRACTION: f64 = 0.02;
/// Cap for the self-consistency test's working resolution.
pub const MAX_ANALYSIS_DIMENSION: usize = 2000;
/// Ceiling on the original image's pixel count before it is decoded at all.
pub const HARD_PIXEL_LIMIT: u64 = 40_000_000;
/// The local file must be at least this much bigger (percent) than the matched source to be flagged.
pub const SOURCE_COMPARISON_THRESHOLD_PERCENT: f64 = 15.0;

/// Known AI upscaling tool or model names that can appear in EXIF or PNG text.
pub const KNOWN_AI_UPSCALER_SIGNATURES: [&str; 16] = [
    "waifu2x",
    "real-esrgan",
    "realesrgan",
    "esrgan",
    "srmd",
    "realsr",
    "anime4k",
    "topaz gigapixel",
    "gigapixel ai",
    "letsenhance",
    "upscayl",
    "cupscale",
    "chainner",
    "swinir",
    "stable diffusion upscale",
    "ultimate sd upscale",
];

/// `SourceComparisonResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceComparisonResult {
    pub flagged: bool,
    pub local_size: (i64, i64),
    pub source_size: (i64, i64),
    pub message: String,
}

/// `SelfConsistencyResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelfConsistencyResult {
    pub best_factor: Option<f64>,
    pub best_filter: Option<&'static str>,
    pub best_diff: Option<f64>,
    pub detail_fraction: Option<f64>,
    /// `"none"`, `"possible"` or `"likely"`.
    pub confidence: &'static str,
    pub message: String,
}

impl Default for SelfConsistencyResult {
    fn default() -> Self {
        SelfConsistencyResult {
            best_factor: None,
            best_filter: None,
            best_diff: None,
            detail_fraction: None,
            confidence: "none",
            message: String::new(),
        }
    }
}

/// `UpscaleCheckResult`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UpscaleCheckResult {
    pub source_comparison: Option<SourceComparisonResult>,
    pub self_consistency: Option<SelfConsistencyResult>,
    /// A known AI upscaler name found in the metadata, as the message to show.
    pub metadata_signature: Option<String>,
}

/// `compare_to_source`: flags when the local image is meaningfully larger than what appears to be
/// its true source.
pub fn compare_to_source(
    local_width: i64,
    local_height: i64,
    source_width: i64,
    source_height: i64,
    threshold_percent: f64,
) -> SourceComparisonResult {
    if source_width == 0 || source_height == 0 {
        return SourceComparisonResult {
            flagged: false,
            local_size: (local_width, local_height),
            source_size: (0, 0),
            message: "No source dimensions available to compare against.".to_string(),
        };
    }
    let width_ratio = local_width as f64 / source_width as f64;
    let height_ratio = local_height as f64 / source_height as f64;
    let gain_percent = (width_ratio.max(height_ratio) - 1.0) * 100.0;
    let flagged = gain_percent >= threshold_percent
        && local_width >= source_width
        && local_height >= source_height;
    let message = if flagged {
        format!(
            "Local file ({local_width}\u{d7}{local_height}) is {gain_percent:.0}% larger than \
             the matched source ({source_width}\u{d7}{source_height}) - likely upscaled from it."
        )
    } else {
        format!(
            "Local file ({local_width}\u{d7}{local_height}) is not meaningfully larger than \
             the matched source ({source_width}\u{d7}{source_height})."
        )
    };
    SourceComparisonResult {
        flagged,
        local_size: (local_width, local_height),
        source_size: (source_width, source_height),
        message,
    }
}

/// `np.gradient` of a 2-D `float32` array, central differences inside and one-sided at the
/// edges: `(d/d row, d/d column)`. `None` when an axis has fewer than two items (numpy raises).
fn gradient(gray: &[f32], width: usize, height: usize) -> Option<(Vec<f32>, Vec<f32>)> {
    if width < 2 || height < 2 {
        return None;
    }
    let mut gy = vec![0f32; gray.len()];
    let mut gx = vec![0f32; gray.len()];
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            gy[i] = if y == 0 {
                gray[i + width] - gray[i]
            } else if y == height - 1 {
                gray[i] - gray[i - width]
            } else {
                (gray[i + width] - gray[i - width]) / 2.0
            };
            gx[i] = if x == 0 {
                gray[i + 1] - gray[i]
            } else if x == width - 1 {
                gray[i] - gray[i - 1]
            } else {
                (gray[i + 1] - gray[i - 1]) / 2.0
            };
        }
    }
    Some((gy, gx))
}

/// The message numpy raises when a picture is a single pixel wide or tall.
const GRADIENT_TOO_SMALL: &str = "Shape of array too small to calculate a numerical gradient, \
at least (edge_order + 1) elements are required.";

/// `_detail_mask`: pixels with genuine local detail, from grayscale intensity gradients. `Err`
/// is the `ValueError` numpy raises for a one-pixel-wide or one-pixel-tall picture.
pub fn detail_mask(image: &Image, percentile: f64) -> Result<Vec<bool>, &'static str> {
    let gray: Vec<f32> = image.to_l().data.iter().map(|&v| f32::from(v)).collect();
    let (gy, gx) = gradient(&gray, image.width, image.height).ok_or(GRADIENT_TOO_SMALL)?;
    let magnitude: Vec<f32> = gx
        .iter()
        .zip(&gy)
        .map(|(&x, &y)| (x * x + y * y).sqrt())
        .collect();
    let mut nonzero: Vec<f32> = magnitude
        .iter()
        .copied()
        .filter(|&m| f64::from(m) > MIN_GRADIENT)
        .collect();
    if nonzero.is_empty() {
        return Ok(vec![false; magnitude.len()]);
    }
    let threshold = percentile_f32(&mut nonzero, percentile).max(MIN_GRADIENT) as f32;
    Ok(magnitude.iter().map(|&m| m >= threshold).collect())
}

/// `_masked_mean_abs_diff`: mean absolute per-pixel difference (averaged over the channels),
/// restricted to the pixels in `mask`.
pub fn masked_mean_abs_diff(a: &Image, b: &Image, mask: &[bool]) -> f64 {
    let per_pixel = |i: usize| -> f32 {
        let sum: f32 = (0..3)
            .map(|c| (f32::from(a.data[i * 3 + c]) - f32::from(b.data[i * 3 + c])).abs())
            .fold(0.0, |acc, v| acc + v);
        (f64::from(sum) / 3.0) as f32
    };
    let masked: Vec<f32> = (0..mask.len())
        .filter(|&i| mask[i])
        .map(per_pixel)
        .collect();
    if masked.is_empty() {
        let all: Vec<f32> = (0..mask.len()).map(per_pixel).collect();
        return mean_f32(&all);
    }
    mean_f32(&masked)
}

/// `Image.open(path)`'s `OSError` text for a file that cannot be read.
fn os_error_text(error: &std::io::Error, path: &Path) -> String {
    let shown = error.to_string();
    let strerror = match shown.rfind(" (os error ") {
        Some(at) => &shown[..at],
        None => &shown,
    };
    let repr = py_repr(&Value::String(path.to_string_lossy().into_owned()));
    match error.raw_os_error() {
        Some(code) => format!("[Errno {code}] {strerror}: {repr}"),
        None => format!("{strerror}: {repr}"),
    }
}

/// `detect_naive_upscale`: tests whether downscaling and re-upscaling the picture reconstructs it
/// closely, measured only where it has detail. `Err` is the `DecompressionBombError` Python lets
/// through (it is neither an `OSError` nor a `ValueError`).
pub fn detect_naive_upscale(path: &Path) -> Result<SelfConsistencyResult, DecompressionBomb> {
    let could_not = |why: String| SelfConsistencyResult {
        message: format!("Could not analyze image: {why}"),
        ..SelfConsistencyResult::default()
    };
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Could not analyze {} for upscaling", path.display());
            return Ok(could_not(os_error_text(&e, path)));
        }
    };
    let (true_w, true_h) = match pyraster::pillow_size(&data)? {
        Some((w, h)) => (w as usize, h as usize),
        None => {
            let repr = py_repr(&Value::String(path.to_string_lossy().into_owned()));
            return Ok(could_not(format!("cannot identify image file {repr}")));
        }
    };

    if true_w < 64 || true_h < 64 {
        return Ok(SelfConsistencyResult {
            message: "Image too small to meaningfully test.".to_string(),
            ..SelfConsistencyResult::default()
        });
    }
    if (true_w as u64) * (true_h as u64) > HARD_PIXEL_LIMIT {
        tracing::warn!(
            "{} is {true_w}x{true_h}, over the safety limit - skipping the upscale self-consistency check entirely",
            path.display()
        );
        return Ok(SelfConsistencyResult {
            message: format!(
                "Image is too large ({true_w}\u{d7}{true_h}) to safely analyze - \
                 skipped to avoid a potential crash."
            ),
            ..SelfConsistencyResult::default()
        });
    }

    let draft = (true_w.max(true_h) > MAX_ANALYSIS_DIMENSION)
        .then_some((MAX_ANALYSIS_DIMENSION, MAX_ANALYSIS_DIMENSION));
    let opened = match pyraster::open(&data, draft) {
        Ok(i) => i,
        Err(OpenError::Bomb(b)) => return Err(b),
        Err(OpenError::Os(m)) => {
            tracing::warn!("Could not analyze {} for upscaling: {m}", path.display());
            return Ok(could_not(m));
        }
    };
    let mut im = opened.to_rgb();
    if im.width.max(im.height) > MAX_ANALYSIS_DIMENSION {
        let scale = MAX_ANALYSIS_DIMENSION as f64 / im.width.max(im.height) as f64;
        im = im.resize_filtered(
            py_round(im.width as f64 * scale).max(1) as usize,
            py_round(im.height as f64 * scale).max(1) as usize,
            Filter::Lanczos,
        );
    }
    let (orig_w, orig_h) = (im.width, im.height);

    let mask = match detail_mask(&im, DETAIL_PERCENTILE) {
        Ok(m) => m,
        Err(why) => return Ok(could_not(why.to_string())),
    };
    let detail_fraction = mask.iter().filter(|&&m| m).count() as f64 / mask.len() as f64;
    if detail_fraction < MIN_DETAIL_FRACTION {
        return Ok(SelfConsistencyResult {
            detail_fraction: Some(detail_fraction),
            message: "This image is almost entirely flat/simple content (little fine \
                      detail to test against) - not enough signal to reliably check for upscaling."
                .to_string(),
            ..SelfConsistencyResult::default()
        });
    }

    let mut best = SelfConsistencyResult {
        detail_fraction: Some(detail_fraction),
        ..SelfConsistencyResult::default()
    };
    for factor in CANDIDATE_FACTORS {
        let small_w = py_round(orig_w as f64 / factor).max(1) as usize;
        let small_h = py_round(orig_h as f64 / factor).max(1) as usize;
        if small_w < 32 || small_h < 32 {
            continue;
        }
        let downscaled = im.resize_filtered(small_w, small_h, Filter::Lanczos);
        for (name, filter) in CANDIDATE_FILTERS {
            let reconstructed = downscaled.resize_filtered(orig_w, orig_h, filter);
            let diff = masked_mean_abs_diff(&im, &reconstructed, &mask);
            if best.best_diff.is_none_or(|b| diff < b) {
                best.best_diff = Some(diff);
                best.best_factor = Some(factor);
                best.best_filter = Some(name);
            }
        }
    }

    let (Some(diff), Some(factor), Some(filter)) =
        (best.best_diff, best.best_factor, best.best_filter)
    else {
        return Ok(SelfConsistencyResult {
            message: "Image too small to meaningfully test.".to_string(),
            ..SelfConsistencyResult::default()
        });
    };
    if diff <= STRONG_MATCH_THRESHOLD {
        best.confidence = "likely";
        best.message = format!(
            "Likely upscaled ~{factor:.1}\u{d7} using {filter} \
             interpolation (in detail regions, round-trip reconstruction very close, \
             avg diff {diff:.1}/255)."
        );
    } else if diff <= POSSIBLE_MATCH_THRESHOLD {
        best.confidence = "possible";
        best.message = format!(
            "Possibly upscaled ~{factor:.1}\u{d7} using {filter} \
             interpolation (in detail regions, avg diff {diff:.1}/255) - \
             not a strong signal, could be coincidental."
        );
    } else {
        best.confidence = "none";
        best.message = format!(
            "No strong signal of simple upscaling (best round-trip diff in detail \
             regions: {diff:.1}/255). Note this test can't detect AI-based \
             upscaling (ESRGAN, waifu2x, etc.), which doesn't leave the same signature."
        );
    }
    Ok(best)
}

/// `check_upscaler_metadata`: a known AI upscaler name in the picture's own metadata (PNG text
/// chunks and every other string or bytes value of `Image.info`, plus the EXIF `ImageDescription`,
/// `Software` and `Artist`), as the message to show; `None` when there is none (which proves
/// nothing). A file Pillow cannot open gives `None`; only the `DecompressionBombError` passes
/// through.
pub fn check_upscaler_metadata(path: &Path) -> Result<Option<String>, DecompressionBomb> {
    let Ok(data) = std::fs::read(path) else {
        tracing::warn!("Could not read metadata from {}", path.display());
        return Ok(None);
    };
    if pyraster::pillow_size(&data)?.is_none() {
        tracing::warn!("Could not read metadata from {}", path.display());
        return Ok(None);
    }
    let Some(texts) = crate::pymeta::metadata_texts(&data) else {
        tracing::warn!("Could not read metadata from {}", path.display());
        return Ok(None);
    };
    let combined = texts.join(" ").to_lowercase();
    for signature in KNOWN_AI_UPSCALER_SIGNATURES {
        if combined.contains(signature) {
            return Ok(Some(format!(
                "Image metadata mentions \"{signature}\" - a known AI upscaling \
                 tool/model name."
            )));
        }
    }
    Ok(None)
}

/// What `UpscaleCheckWorker.run` does for one entry: fill in the local size if it is missing,
/// compare to the selected candidate's size, and run the metadata and self-consistency checks.
/// A `DecompressionBombError` (not an `OSError`) ends the batch in Python; here it is the `Err`.
pub fn check_entry(entry: &mut Entry) -> Result<UpscaleCheckResult, DecompressionBomb> {
    if entry.local_width.is_none() || entry.local_height.is_none() {
        let read = std::fs::read(&entry.path)
            .ok()
            .map(|d| pyraster::pillow_size(&d))
            .transpose()?
            .flatten();
        if let Some((w, h)) = read {
            entry.local_width = Some(i64::from(w));
            entry.local_height = Some(i64::from(h));
        } else {
            tracing::warn!("Could not read dimensions for {}", entry.filename());
        }
    }

    let mut source_comparison = None;
    if let Some(candidate) = entry.selected_candidate() {
        if let (Some(cw), Some(ch), Some(lw), Some(lh)) = (
            candidate.width.filter(|&v| v != 0),
            candidate.height.filter(|&v| v != 0),
            entry.local_width.filter(|&v| v != 0),
            entry.local_height.filter(|&v| v != 0),
        ) {
            source_comparison = Some(compare_to_source(
                lw,
                lh,
                cw,
                ch,
                SOURCE_COMPARISON_THRESHOLD_PERCENT,
            ));
        }
    }

    let path = Path::new(&entry.path);
    let metadata_signature = check_upscaler_metadata(path)?;
    let self_consistency = detect_naive_upscale(path)?;
    tracing::info!(
        "{}: source_flagged={:?} self_consistency={} metadata_signature={:?}",
        entry.filename(),
        source_comparison.as_ref().map(|s| s.flagged),
        self_consistency.confidence,
        metadata_signature
    );
    Ok(UpscaleCheckResult {
        source_comparison,
        self_consistency: Some(self_consistency),
        metadata_signature,
    })
}
