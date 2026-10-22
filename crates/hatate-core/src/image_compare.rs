//! `core/image_compare.py`: comparing a local image against a matched one.
//!
//! Two separate questions, answered separately. "Is the match better than what I have?" is the
//! size arithmetic ([`compare_sizes`], [`compact_size_delta`], [`size_delta_sort_key`],
//! [`compare_file_sizes`]). "Is it the same image?" is a perceptual hash ([`dhash`],
//! [`local_prints`], [`compare_to_local`], [`aligned_similarity`]).
//!
//! **What is matched with Python.** Every hash: [`dhash`] and [`local_prints`] equal Pillow's on
//! the pictures the differential covers, because the decode, the grey conversion and the Lanczos
//! resize are Pillow's own arithmetic (`crate::pyjpeg`, `crate::pyraster`) and not an
//! approximation of it. What is not matched, and why, is in `docs/image-compare.md`.

use std::fmt;
use std::path::Path;

use crate::pyimage::DecompressionBomb;
use crate::pyraster::{self, Image, OpenError};

/// Long-edge ratios within this of 1.0 count as "the same size".
pub const SAME_SIZE_TOLERANCE: f64 = 0.02;
/// Aspect ratios differing by more than this suggest a crop or a different edit.
pub const ASPECT_TOLERANCE: f64 = 0.04;
/// dHash is 64 bits; these are the working values for "same image" and "related".
pub const DHASH_SAME_MAX: u32 = 5;
pub const DHASH_SIMILAR_MAX: u32 = 12;

/// A difference hash: Python's arbitrary-size `int`, of up to 256 bits here (`hash_size` 16), most
/// significant bit first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash(pub [u64; 4]);

impl Hash {
    /// Number of set bits (`bin(x).count("1")`).
    pub fn popcount(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    fn xor(&self, other: &Hash) -> Hash {
        Hash([
            self.0[0] ^ other.0[0],
            self.0[1] ^ other.0[1],
            self.0[2] ^ other.0[2],
            self.0[3] ^ other.0[3],
        ])
    }

    /// `hex(x)` without the `0x`, as Python writes the integer.
    pub fn to_hex(&self) -> String {
        let mut s = String::new();
        for w in self.0 {
            if s.is_empty() {
                if w != 0 {
                    s = format!("{w:x}");
                }
            } else {
                s.push_str(&format!("{w:016x}"));
            }
        }
        if s.is_empty() {
            s.push('0');
        }
        s
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// What a hash or a dimension is read from: Python's `Union[str, bytes, Image.Image]`.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Path(&'a Path),
    Bytes(&'a [u8]),
    Image(&'a Image),
}

/// `Image.open` on a source: the picture (with the JPEG `draft` when asked), or why not.
fn load(source: Source<'_>, draft: Option<(usize, usize)>) -> Result<Image, OpenError> {
    match source {
        Source::Image(img) => Ok(img.clone()),
        Source::Bytes(b) => pyraster::open(b, draft),
        Source::Path(p) => {
            let data = std::fs::read(p).map_err(|e| OpenError::Os(e.to_string()))?;
            pyraster::open(&data, draft)
        }
    }
}

/// `dhash`: a difference hash. Compares each pixel to its right-hand neighbour on a tiny greyscale
/// version, one bit per comparison. `Ok(None)` is Python's `None` (`OSError`, `ValueError` or
/// `IndexError` while reading); the `Err` is the `DecompressionBombError` Python lets through.
pub fn dhash(source: Source<'_>, hash_size: usize) -> Result<Option<Hash>, DecompressionBomb> {
    let image = match load(source, None) {
        Ok(i) => i,
        Err(OpenError::Os(_)) => return Ok(None),
        Err(OpenError::Bomb(b)) => return Err(b),
    };
    Ok(Some(hash_of(&image, hash_size)))
}

/// The hash of a picture already in memory.
fn hash_of(image: &Image, hash_size: usize) -> Hash {
    let small = image.to_l().resize(hash_size + 1, hash_size, None, None);
    let pixels = &small.data;
    let mut words = [0u64; 4];
    let mut index = 0usize;
    let total = hash_size * hash_size;
    for row in 0..hash_size {
        let offset = row * (hash_size + 1);
        for col in 0..hash_size {
            let bit = pixels[offset + col] > pixels[offset + col + 1];
            // Most significant bit first: bit `index` of `total` is at position `total - 1 - index`.
            if bit {
                let pos = total - 1 - index;
                words[3 - pos / 64] |= 1u64 << (pos % 64);
            }
            index += 1;
        }
    }
    Hash(words)
}

/// `image_dimensions`: `(width, height)`, or `None` if the image cannot be read. Header only.
pub fn image_dimensions(source: Source<'_>) -> Result<Option<(usize, usize)>, DecompressionBomb> {
    match source {
        Source::Image(img) => Ok(Some((img.width, img.height))),
        Source::Bytes(b) => dimensions_of(b),
        Source::Path(p) => match std::fs::read(p) {
            Ok(data) => dimensions_of(&data),
            Err(_) => Ok(None),
        },
    }
}

fn dimensions_of(data: &[u8]) -> Result<Option<(usize, usize)>, DecompressionBomb> {
    Ok(pyraster::pillow_size(data)?.map(|(w, h)| (w as usize, h as usize)))
}

/// `hamming_distance`: how many bits differ, `None` if either is missing.
pub fn hamming_distance(a: Option<&Hash>, b: Option<&Hash>) -> Option<u32> {
    Some(a?.xor(b?).popcount())
}

/// Anchors turning a hash distance into a percentage: the boundary of "looks like the same image"
/// is 90%, the boundary of "similar" 70%, and 32 (what two unrelated pictures average on 64 bits)
/// is 0. Linear between.
const SIMILARITY_ANCHORS: [(i64, f64); 4] = [
    (0, 100.0),
    (DHASH_SAME_MAX as i64, 90.0),
    (DHASH_SIMILAR_MAX as i64, 70.0),
    (32, 0.0),
];

/// Python's `round(x, 1)`: the float nearest the correctly rounded one-decimal value.
fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

/// `perceptual_similarity`: a percentage from a hash distance, `None` if there is no distance.
pub fn perceptual_similarity(distance: Option<i64>) -> Option<f64> {
    let distance = distance?;
    if distance <= 0 {
        return Some(100.0);
    }
    for pair in SIMILARITY_ANCHORS.windows(2) {
        let ((low_d, low_pct), (high_d, high_pct)) = (pair[0], pair[1]);
        if distance <= high_d {
            let span = high_d - low_d;
            if span <= 0 {
                return Some(high_pct);
            }
            let fraction = (distance - low_d) as f64 / span as f64;
            return Some(round1(low_pct + (high_pct - low_pct) * fraction));
        }
    }
    Some(0.0)
}

/// `describe_perceptual_match`: a hash distance as something meaningful to read.
pub fn describe_perceptual_match(distance: Option<i64>) -> String {
    match distance {
        None => "Couldn't compare the images".to_owned(),
        Some(d) if d <= i64::from(DHASH_SAME_MAX) => {
            format!("Looks like the same image (difference {d}/64)")
        }
        Some(d) if d <= i64::from(DHASH_SIMILAR_MAX) => {
            format!("Similar, but not identical - possibly a different edit or crop ({d}/64)")
        }
        Some(d) => format!("These look like different images (difference {d}/64)"),
    }
}

// -- comparing a thumbnail with the local file, tolerating how thumbnails differ

/// Every shape is cut from one copy this size, so a 5,000px original is resized once.
pub const LOCAL_WORKING_SIZE: usize = 512;
pub const ALIGN_EDGE_CROPS: [f64; 2] = [0.04, 0.08];
pub const ALIGN_SIDE_CROP: f64 = 0.10;
/// A plain bar: nearly every pixel close to the corner's shade of grey.
pub const BORDER_TOLERANCE: i32 = 24;
pub const BORDER_FLAT_SHARE: f64 = 0.97;
pub const BORDER_MAX_FRACTION: f64 = 0.25;
/// A mirrored copy is scored as a strong match to review, never high enough to auto-import.
pub const MIRRORED_MAX_SIMILARITY: f64 = 85.0;
/// A 90+ has to be confirmed at 256 bits before it stands.
pub const FINE_HASH_SIZE: usize = 16;
pub const FINE_CONFIRM_MAX: u32 = 20;
pub const UNCONFIRMED_MAX_SIMILARITY: f64 = 85.0;

/// The local image's hashes in every shape [`compare_to_local`] tries: 64-bit for scoring, 256-bit
/// (`fine_*`) for confirming a 90+.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPrints {
    pub straight: Vec<Hash>,
    pub mirrored: Option<Hash>,
    pub fine_straight: Vec<Hash>,
    pub fine_mirrored: Option<Hash>,
}

/// `trim_border`: the image without plain bars along its edges (letterboxing, any colour); `None`
/// where Python returns the very image it was given (nothing to trim, or more than a quarter).
pub fn trim_border(image: &Image) -> Option<Image> {
    let grey = image.to_l();
    let (width, height) = (grey.width, grey.height);
    let px = |x: usize, y: usize| i32::from(grey.data[y * width + x]);
    let flat = |values: &mut dyn Iterator<Item = i32>, reference: i32| -> bool {
        let (mut close, mut total) = (0usize, 0usize);
        for v in values {
            total += 1;
            if (v - reference).abs() <= BORDER_TOLERANCE {
                close += 1;
            }
        }
        total > 0 && close as f64 >= BORDER_FLAT_SHARE * total as f64
    };
    let flat_row =
        |y: usize, reference: i32| flat(&mut (0..width).step_by(2).map(|x| px(x, y)), reference);
    let flat_col = |x: usize, top: usize, bottom: usize, reference: i32| {
        flat(&mut (top..bottom).step_by(2).map(|y| px(x, y)), reference)
    };

    let (top_ref, bottom_ref) = (px(0, 0), px(width - 1, height - 1));
    let (wf, hf) = (width as f64, height as f64);
    let mut top = 0usize;
    while (top as f64) < hf * BORDER_MAX_FRACTION && flat_row(top, top_ref) {
        top += 1;
    }
    let mut bottom = height;
    while ((height - bottom) as f64) < hf * BORDER_MAX_FRACTION && flat_row(bottom - 1, bottom_ref)
    {
        bottom -= 1;
    }
    let mut left = 0usize;
    while (left as f64) < wf * BORDER_MAX_FRACTION && flat_col(left, top, bottom, top_ref) {
        left += 1;
    }
    let mut right = width;
    while ((width - right) as f64) < wf * BORDER_MAX_FRACTION
        && flat_col(right - 1, top, bottom, bottom_ref)
    {
        right -= 1;
    }
    // Signed: on a one-pixel-high picture the two scans can cross.
    if ((bottom as i64 - top as i64) as f64) < hf / 2.0
        || ((right as i64 - left as i64) as f64) < wf / 2.0
    {
        return None;
    }
    if (left, top, right, bottom) == (0, 0, width, height) {
        return None;
    }
    Some(image.crop(left, top, right, bottom))
}

/// `_shapes`: the picture, cropped slightly all round, and off each side.
fn shapes(image: &Image) -> Vec<Image> {
    let (width, height) = (image.width, image.height);
    let mut out = vec![image.clone()];
    for f in ALIGN_EDGE_CROPS {
        let (dx, dy) = ((width as f64 * f) as usize, (height as f64 * f) as usize);
        out.push(image.crop(dx, dy, width - dx, height - dy));
    }
    let (dx, dy) = (
        (width as f64 * ALIGN_SIDE_CROP) as usize,
        (height as f64 * ALIGN_SIDE_CROP) as usize,
    );
    out.push(image.crop(dx, 0, width, height));
    out.push(image.crop(0, dy, width, height));
    out.push(image.crop(0, 0, width - dx, height));
    out.push(image.crop(0, 0, width, height - dy));
    out
}

/// `local_prints`: hash the local image once, in every shape a thumbnail is compared against.
/// `None` if it cannot be read (Python also catches the decompression bomb here).
pub fn local_prints(source: Source<'_>) -> Option<LocalPrints> {
    local_prints_or_reason(source).ok()
}

/// [`local_prints`] with the reason a file could not be read (`Err`), so a caller can log why
/// instead of only that: an arithmetic-coded JPEG and a text file both give `None` otherwise.
pub fn local_prints_or_reason(source: Source<'_>) -> Result<LocalPrints, OpenError> {
    let image = load(source, Some((LOCAL_WORKING_SIZE, LOCAL_WORKING_SIZE)))?;
    let mut image = image.to_rgb();
    image.thumbnail(LOCAL_WORKING_SIZE as i64, LOCAL_WORKING_SIZE as i64);
    let trimmed = trim_border(&image);
    // Cropped both ways: a trim can take a plain background for a border, so crops of the
    // untrimmed picture are kept alongside.
    let mut all = shapes(&image);
    if let Some(t) = &trimmed {
        all.extend(shapes(t));
    }
    let mirror = trimmed.as_ref().unwrap_or(&image).mirror();
    let straight: Vec<Hash> = all.iter().map(|s| hash_of(s, 8)).collect();
    let fine: Vec<Hash> = all.iter().map(|s| hash_of(s, FINE_HASH_SIZE)).collect();
    Ok(LocalPrints {
        straight,
        mirrored: Some(hash_of(&mirror, 8)),
        fine_straight: fine,
        fine_mirrored: Some(hash_of(&mirror, FINE_HASH_SIZE)),
    })
}

/// How close a thumbnail is to the local image, in its closest shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// 64-bit, the closest shape.
    pub distance: Option<u32>,
    /// That closest shape was the mirrored one.
    pub mirrored: bool,
    /// 256-bit, the same way round.
    pub fine_distance: Option<u32>,
}

impl Comparison {
    fn none() -> Comparison {
        Comparison {
            distance: None,
            mirrored: false,
            fine_distance: None,
        }
    }
}

fn closest(mine: &[Hash], others: &[Hash]) -> Option<u32> {
    mine.iter()
        .flat_map(|m| others.iter().map(move |t| m.xor(t).popcount()))
        .min()
}

/// `compare_to_local`: how close a thumbnail is to the local image, in its closest shape.
pub fn compare_to_local(prints: Option<&LocalPrints>, thumbnail: Source<'_>) -> Comparison {
    let Some(prints) = prints else {
        return Comparison::none();
    };
    let Ok(image) = load(thumbnail, None) else {
        return Comparison::none();
    };
    let image = image.to_rgb();
    let trimmed = trim_border(&image);
    let views = [Some(&image), trimmed.as_ref()];
    let theirs: Vec<Hash> = views.iter().flatten().map(|v| hash_of(v, 8)).collect();
    if theirs.is_empty() {
        return Comparison::none();
    }
    let fine_theirs: Vec<Hash> = views
        .iter()
        .flatten()
        .map(|v| hash_of(v, FINE_HASH_SIZE))
        .collect();
    // Python's `views` holds the picture twice when nothing was trimmed; the minimum is the same.
    let straight = closest(&prints.straight, &theirs);
    let mirrored = prints
        .mirrored
        .as_ref()
        .and_then(|m| closest(std::slice::from_ref(m), &theirs));
    if let (Some(m), Some(s)) = (mirrored, straight)
        && m < s
    {
        let fine = prints
            .fine_mirrored
            .as_ref()
            .and_then(|m| closest(std::slice::from_ref(m), &fine_theirs));
        return Comparison {
            distance: Some(m),
            mirrored: true,
            fine_distance: fine,
        };
    }
    Comparison {
        distance: straight,
        mirrored: false,
        fine_distance: closest(&prints.fine_straight, &fine_theirs),
    }
}

/// `aligned_similarity`: `(percentage, mirrored)`: [`compare_to_local`] on the usual scale, a
/// mirrored copy and a 90+ the fine hash does not confirm held below auto-import.
pub fn aligned_similarity(
    prints: Option<&LocalPrints>,
    thumbnail: Source<'_>,
) -> (Option<f64>, bool) {
    aligned_score(&compare_to_local(prints, thumbnail))
}

/// The second half of [`aligned_similarity`]: a [`Comparison`] on the usual scale, a mirrored copy
/// and a 90+ the fine hash does not confirm held below auto-import. Python's tests reach it by
/// patching `compare_to_local`; here it is a function of its own.
pub fn aligned_score(comparison: &Comparison) -> (Option<f64>, bool) {
    let Some(mut score) = perceptual_similarity(comparison.distance.map(i64::from)) else {
        return (None, false);
    };
    if comparison.mirrored {
        score = score.min(MIRRORED_MAX_SIMILARITY);
    }
    if score > UNCONFIRMED_MAX_SIMILARITY
        && comparison
            .fine_distance
            .is_none_or(|d| d > FINE_CONFIRM_MAX)
    {
        score = UNCONFIRMED_MAX_SIMILARITY;
    }
    (Some(score), comparison.mirrored)
}

// -- the size comparison

/// How a matched image's dimensions relate to the local file's.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SizeComparison {
    pub local_size: Option<(i64, i64)>,
    pub remote_size: Option<(i64, i64)>,
    /// Long-edge ratio, remote / local.
    pub scale: Option<f64>,
    pub megapixel_ratio: Option<f64>,
    pub aspect_differs: bool,
    /// Short human phrase, e.g. "2.0x larger".
    pub verdict: String,
}

impl SizeComparison {
    pub fn remote_is_bigger(&self) -> bool {
        self.scale.is_some_and(|s| s > 1.0 + SAME_SIZE_TOLERANCE)
    }
}

/// Python's `a / b` on two ints: the correctly rounded quotient (ties to even), where a float
/// division of two converted operands would round twice once an operand passes 2^53.
fn int_div(a: i128, b: i128) -> f64 {
    const EXACT: i128 = 1 << 53;
    if a.abs() <= EXACT && b.abs() <= EXACT {
        return a as f64 / b as f64;
    }
    let negative = (a < 0) != (b < 0);
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
    let bits = |v: u128| 128 - v.leading_zeros();
    let mut mantissa = a / b;
    let mut rest = a % b;
    let mut exponent = 0i32;
    // Long division, a bit at a time, until the quotient has 55 significant bits (two more than
    // a double keeps, and the remainder says whether anything is left past them).
    while bits(mantissa) < 55 && rest != 0 {
        rest *= 2;
        let bit = rest >= b;
        if bit {
            rest -= b;
        }
        mantissa = (mantissa << 1) | u128::from(bit);
        exponent -= 1;
    }
    let width = bits(mantissa);
    if width > 53 {
        let drop = width - 53;
        let tail = mantissa & ((1u128 << drop) - 1);
        let half = 1u128 << (drop - 1);
        mantissa >>= drop;
        exponent += drop as i32;
        if tail > half || (tail == half && (rest != 0 || mantissa & 1 == 1)) {
            mantissa += 1;
        }
    }
    let value = mantissa as f64 * 2f64.powi(exponent);
    if negative { -value } else { value }
}

/// `compare_sizes`: relates a match's dimensions to the local file's, on the long edge.
pub fn compare_sizes(
    local_w: Option<i64>,
    local_h: Option<i64>,
    remote_w: Option<i64>,
    remote_h: Option<i64>,
) -> SizeComparison {
    let (Some(lw), Some(lh), Some(rw), Some(rh)) = (local_w, local_h, remote_w, remote_h) else {
        return SizeComparison::default();
    };
    if [lw, lh, rw, rh].iter().any(|&v| v <= 0) {
        return SizeComparison::default();
    }
    let (local_long, remote_long) = (lw.max(lh), rw.max(rh));
    let scale = int_div(i128::from(remote_long), i128::from(local_long));
    let megapixels = int_div(
        i128::from(rw) * i128::from(rh),
        i128::from(lw) * i128::from(lh),
    );
    let local_aspect = int_div(i128::from(lw), i128::from(lh));
    let remote_aspect = int_div(i128::from(rw), i128::from(rh));
    let aspect_differs = (local_aspect - remote_aspect).abs() / local_aspect > ASPECT_TOLERANCE;

    let mut verdict = if (scale - 1.0).abs() <= SAME_SIZE_TOLERANCE {
        "same size".to_owned()
    } else if scale > 1.0 {
        format!("{}\u{00d7} larger", py_format_g(scale, 2))
    } else {
        // How much smaller rather than as a fraction: "half the size" reads, "0.5x larger" does not.
        format!("{}\u{00d7} smaller", py_format_g(1.0 / scale, 2))
    };
    if aspect_differs {
        verdict.push_str(", different shape");
    }
    SizeComparison {
        local_size: Some((lw, lh)),
        remote_size: Some((rw, rh)),
        scale: Some(scale),
        megapixel_ratio: Some(megapixels),
        aspect_differs,
        verdict,
    }
}

/// `compact_size_delta`: "+ 4x" for four times the size, "- 4x" for a quarter, "=" for the same,
/// "" when it cannot be determined. Always relative to the local file.
pub fn compact_size_delta(
    local_w: Option<i64>,
    local_h: Option<i64>,
    remote_w: Option<i64>,
    remote_h: Option<i64>,
) -> String {
    let comparison = compare_sizes(local_w, local_h, remote_w, remote_h);
    let Some(scale) = comparison.scale else {
        return String::new();
    };
    if (scale - 1.0).abs() <= SAME_SIZE_TOLERANCE {
        return "=".to_owned();
    }
    if scale > 1.0 {
        return format!("+ {}x", ratio(scale, 3));
    }
    format!("- {}x", ratio(1.0 / scale, 3))
}

/// `_ratio`: a ratio to `digits` significant figures, never in exponent form.
fn ratio(value: f64, digits: i32) -> String {
    if value >= 10f64.powi(digits - 1) {
        return group_thousands(py_round_half_even(value));
    }
    py_format_g(value, digits as usize)
}

/// Python's `round(x)` for a float: to the nearest integer, ties to even.
fn py_round_half_even(x: f64) -> i128 {
    let floor = x.floor();
    let diff = x - floor;
    let f = floor as i128;
    if diff > 0.5 || (diff == 0.5 && f % 2 != 0) {
        f + 1
    } else {
        f
    }
}

/// `f"{n:,}"`.
fn group_thousands(n: i128) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// Python's `format(x, f".{precision}g")`.
pub fn py_format_g(x: f64, precision: usize) -> String {
    if x == 0.0 {
        return "0".to_owned();
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".to_owned()
        } else if x > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let p = precision.max(1);
    // The decimal exponent after rounding to `p` significant digits.
    let sci = format!("{:.*e}", p - 1, x);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if exp < -4 || exp >= p as i32 {
        let mantissa = strip_zeros(mantissa);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exp.abs())
    } else {
        let decimals = (p as i32 - 1 - exp).max(0) as usize;
        strip_zeros(&format!("{x:.decimals$}"))
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s.to_owned()
    }
}

/// `size_delta_sort_key`: orders the column by the real ratio rather than its text; unknown values
/// sort first.
pub fn size_delta_sort_key(
    local_w: Option<i64>,
    local_h: Option<i64>,
    remote_w: Option<i64>,
    remote_h: Option<i64>,
) -> f64 {
    compare_sizes(local_w, local_h, remote_w, remote_h)
        .scale
        .unwrap_or(-1.0)
}

/// `compare_file_sizes`: a short phrase relating two file sizes, "" when either is unknown.
pub fn compare_file_sizes(local_bytes: Option<i64>, remote_bytes: Option<i64>) -> String {
    let (Some(local), Some(remote)) = (local_bytes, remote_bytes) else {
        return String::new();
    };
    if local <= 0 || remote <= 0 {
        return String::new();
    }
    let ratio_v = int_div(i128::from(remote), i128::from(local));
    if (ratio_v - 1.0).abs() <= 0.05 {
        return "about the same size on disk".to_owned();
    }
    if ratio_v > 1.0 {
        return format!("{}\u{00d7} bigger file", ratio(ratio_v, 2));
    }
    format!("{}\u{00d7} smaller file", ratio(1.0 / ratio_v, 2))
}
