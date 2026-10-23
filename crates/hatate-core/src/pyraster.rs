//! The slice of Pillow `Image` that `core/image_compare.py` uses, so a hash computed here equals
//! the one Pillow computes: `Image.open`, `convert("L")` and `convert("RGB")`, `crop`,
//! `ImageOps.mirror`, `resize` and `thumbnail` with `Image.LANCZOS`, `Image.draft` for JPEG.
//!
//! JPEG goes through [`crate::pyjpeg`] (libjpeg-turbo's pixels). Everything else is decoded by the
//! `image` crate and mapped onto the mode Pillow would give it (`Mode`), because converting to
//! grey is mode-dependent in Pillow. Pillow's resampling is ported rather than called:
//! `precompute_coeffs` (with the fractional `box` that `reducing_gap` produces), the 22-bit
//! fixed-point 8-bit passes of `Resample.c`, and `ImagingReduce`'s box averages, with the
//! multiplier it derives in `float`.

use std::fmt;

use crate::pyimage;
pub use crate::pyimage::DecompressionBomb;
use crate::pyjpeg;

/// Pillow's mode, for the modes a decoded picture can have here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    L,
    La,
    Rgb,
    Rgba,
    Cmyk,
}

impl Mode {
    pub fn channels(self) -> usize {
        match self {
            Mode::L => 1,
            Mode::La => 2,
            Mode::Rgb => 3,
            Mode::Rgba | Mode::Cmyk => 4,
        }
    }
}

/// A Pillow image: `mode`, size, and `width * height * channels` interleaved bytes.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub mode: Mode,
    pub data: Vec<u8>,
}

/// Why `Image.open(...)` and the load that follows raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// `OSError` or `UnidentifiedImageError`: Python's callers catch it (`OSError`).
    Os(String),
    /// `Image.DecompressionBombError`: not an `OSError`; the callers differ in catching it.
    Bomb(pyimage::DecompressionBomb),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Os(m) => f.write_str(m),
            OpenError::Bomb(b) => write!(f, "{b}"),
        }
    }
}

impl std::error::Error for OpenError {}

fn os(msg: impl Into<String>) -> OpenError {
    OpenError::Os(msg.into())
}

/// `Image.open(io.BytesIO(data))` followed by `load()`, with `Image.draft("RGB", draft)` called in
/// between when `draft` is given (it does nothing for any format but JPEG).
pub fn open(data: &[u8], draft: Option<(usize, usize)>) -> Result<Image, OpenError> {
    let size = pyimage::image_size_checked(data).map_err(OpenError::Bomb)?;
    if size.is_none() && header_formats(data) {
        return Err(os("cannot identify image file"));
    }
    if let (true, Some((w, h))) = (data.starts_with(b"\xff\xd8\xff"), size) {
        let scale = match draft {
            Some((dw, dh)) if dw > 0 && dh > 0 => {
                let scale = (w as usize / dw).min(h as usize / dh);
                [8usize, 4, 2, 1]
                    .into_iter()
                    .find(|&s| scale >= s)
                    .unwrap_or(1)
            }
            _ => 1,
        };
        let d = pyjpeg::decode(data, scale).map_err(|e| os(e.to_string()))?;
        let mode = match d.kind {
            pyjpeg::Kind::Gray => Mode::L,
            pyjpeg::Kind::Rgb => Mode::Rgb,
            pyjpeg::Kind::Cmyk => Mode::Cmyk,
        };
        return Ok(Image {
            width: d.width,
            height: d.height,
            mode,
            data: d.data,
        });
    }
    open_other(data)
}

/// Whether `data` starts like a format `pyimage` reads the header of (Pillow's first five
/// plugins); for those, `pyimage` saying "not an image" is final.
fn header_formats(data: &[u8]) -> bool {
    data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(b"\xff\xd8\xff")
        || (data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"))
        || data.starts_with(b"BM")
}

/// `Image.open(...).size`: header only. Formats `pyimage` does not read (TIFF, ICO, PNM...) are
/// measured by the `image` crate.
pub fn pillow_size(data: &[u8]) -> Result<Option<(u32, u32)>, pyimage::DecompressionBomb> {
    if header_formats(data) {
        return pyimage::image_size_checked(data);
    }
    Ok(image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok()))
}

/// Everything that is not a JPEG: the `image` crate's decoders, mapped to Pillow's modes.
fn open_other(data: &[u8]) -> Result<Image, OpenError> {
    use image::DynamicImage as D;
    // A PNG is decoded from the pixel stream as Pillow reads it, not as the file stands.
    let data = &*crate::pypng::canonical(data).map_err(os)?;
    let reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| os(e.to_string()))?;
    let img = reader.decode().map_err(|e| os(e.to_string()))?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let hi = |v: u16| (v >> 8) as u8;
    let (mode, data) = match img {
        D::ImageLuma8(g) => (Mode::L, g.into_raw()),
        D::ImageLumaA8(g) => (Mode::La, g.into_raw()),
        D::ImageRgb8(g) => (Mode::Rgb, g.into_raw()),
        D::ImageRgba8(g) => (Mode::Rgba, g.into_raw()),
        // Pillow reads 16-bit grey as `I;16`, which converts by clipping at 255.
        D::ImageLuma16(g) => (
            Mode::L,
            g.into_raw()
                .into_iter()
                .map(|v| u8::try_from(v).unwrap_or(255))
                .collect(),
        ),
        D::ImageLumaA16(g) => (
            Mode::Rgba,
            g.as_raw()
                .chunks_exact(2)
                .flat_map(|p| [hi(p[0]), hi(p[0]), hi(p[0]), hi(p[1])])
                .collect(),
        ),
        D::ImageRgb16(g) => (Mode::Rgb, g.into_raw().into_iter().map(hi).collect()),
        D::ImageRgba16(g) => (Mode::Rgba, g.into_raw().into_iter().map(hi).collect()),
        other => (Mode::Rgb, other.to_rgb8().into_raw()),
    };
    Ok(Image {
        width: w,
        height: h,
        mode,
        data,
    })
}

/// `ITU-R 601-2` luma as Pillow's `L24`: `(R * 19595 + G * 38470 + B * 7471 + 0x8000) >> 16`.
#[inline]
fn l24(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 19595 + u32::from(g) * 38470 + u32::from(b) * 7471 + 0x8000) >> 16) as u8
}

/// Pillow's `MULDIV255(a, b)`.
#[inline]
fn muldiv255(a: i32, b: i32) -> i32 {
    let tmp = a * b + 128;
    ((tmp >> 8) + tmp) >> 8
}

/// Pillow's `cmyk2rgb` for one pixel.
#[inline]
fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> (u8, u8, u8) {
    let nk = 255 - i32::from(k);
    let f = |v: u8| (nk - muldiv255(i32::from(v), nk)).clamp(0, 255) as u8;
    (f(c), f(m), f(y))
}

impl Image {
    /// `image.convert("L")`.
    pub fn to_l(&self) -> Image {
        let n = self.width * self.height;
        let mut out = Vec::with_capacity(n);
        match self.mode {
            Mode::L => out.extend_from_slice(&self.data),
            Mode::La => out.extend(self.data.chunks_exact(2).map(|p| p[0])),
            Mode::Rgb => out.extend(self.data.chunks_exact(3).map(|p| l24(p[0], p[1], p[2]))),
            Mode::Rgba => out.extend(self.data.chunks_exact(4).map(|p| l24(p[0], p[1], p[2]))),
            Mode::Cmyk => out.extend(self.data.chunks_exact(4).map(|p| {
                let (r, g, b) = cmyk_to_rgb(p[0], p[1], p[2], p[3]);
                l24(r, g, b)
            })),
        }
        Image {
            width: self.width,
            height: self.height,
            mode: Mode::L,
            data: out,
        }
    }

    /// `image.convert("RGB")`.
    pub fn to_rgb(&self) -> Image {
        let n = self.width * self.height;
        let mut out = Vec::with_capacity(n * 3);
        match self.mode {
            Mode::L => out.extend(self.data.iter().flat_map(|&v| [v, v, v])),
            Mode::La => out.extend(self.data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0]])),
            Mode::Rgb => out.extend_from_slice(&self.data),
            Mode::Rgba => out.extend(self.data.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]])),
            Mode::Cmyk => out.extend(self.data.chunks_exact(4).flat_map(|p| {
                let (r, g, b) = cmyk_to_rgb(p[0], p[1], p[2], p[3]);
                [r, g, b]
            })),
        }
        Image {
            width: self.width,
            height: self.height,
            mode: Mode::Rgb,
            data: out,
        }
    }

    /// `ImageOps.mirror`: left to right.
    pub fn mirror(&self) -> Image {
        let ch = self.mode.channels();
        let mut out = Vec::with_capacity(self.data.len());
        for row in self.data.chunks_exact(self.width * ch) {
            for px in row.chunks_exact(ch).rev() {
                out.extend_from_slice(px);
            }
        }
        Image {
            data: out,
            ..self.clone()
        }
    }

    /// `image.crop((left, top, right, bottom))`, for a box inside the image.
    pub fn crop(&self, left: usize, top: usize, right: usize, bottom: usize) -> Image {
        let ch = self.mode.channels();
        let (w, h) = (right - left, bottom - top);
        let mut out = Vec::with_capacity(w * h * ch);
        for y in top..bottom {
            let row = y * self.width * ch;
            out.extend_from_slice(&self.data[row + left * ch..row + right * ch]);
        }
        Image {
            width: w,
            height: h,
            mode: self.mode,
            data: out,
        }
    }

    /// `Image.resize(size, Image.LANCZOS, box, reducing_gap)` for modes `L` and `RGB`.
    pub fn resize(
        &self,
        width: usize,
        height: usize,
        box_: Option<[f64; 4]>,
        reducing_gap: Option<f64>,
    ) -> Image {
        assert!(matches!(self.mode, Mode::L | Mode::Rgb));
        let full = [0.0, 0.0, self.width as f64, self.height as f64];
        let mut box_ = box_.unwrap_or(full);
        if self.width == width && self.height == height && box_ == full {
            return self.clone();
        }
        let mut src: std::borrow::Cow<'_, Image> = std::borrow::Cow::Borrowed(self);
        if let Some(gap) = reducing_gap {
            let factor_x = (((box_[2] - box_[0]) / width as f64 / gap) as usize).max(1);
            let factor_y = (((box_[3] - box_[1]) / height as f64 / gap) as usize).max(1);
            if factor_x > 1 || factor_y > 1 {
                // `_get_safe_box`
                let support = 3.0 - 0.5;
                let sx = (box_[2] - box_[0]) / width as f64 * support;
                let sy = (box_[3] - box_[1]) / height as f64 * support;
                let safe = [
                    ((box_[0] - sx) as i64).max(0) as usize,
                    ((box_[1] - sy) as i64).max(0) as usize,
                    (((box_[2] + sx).ceil()) as usize).min(self.width),
                    (((box_[3] + sy).ceil()) as usize).min(self.height),
                ];
                src = std::borrow::Cow::Owned(self.reduce(factor_x, factor_y, safe));
                box_ = [
                    (box_[0] - safe[0] as f64) / factor_x as f64,
                    (box_[1] - safe[1] as f64) / factor_y as f64,
                    (box_[2] - safe[0] as f64) / factor_x as f64,
                    (box_[3] - safe[1] as f64) / factor_y as f64,
                ];
            }
        }
        src.resample(width, height, box_, Filter::Lanczos)
    }

    /// `Image.resize((width, height), filter)` with no box and no `reducing_gap`, for modes `L`
    /// and `RGB`: what `core/image_diff.py` and `core/upscale_detect.py` call.
    pub fn resize_filtered(&self, width: usize, height: usize, filter: Filter) -> Image {
        assert!(matches!(self.mode, Mode::L | Mode::Rgb));
        if self.width == width && self.height == height {
            return self.clone();
        }
        match filter {
            Filter::Nearest => self.resize_nearest(width, height),
            Filter::Bilinear | Filter::Bicubic | Filter::Lanczos => self.resample(
                width,
                height,
                [0.0, 0.0, self.width as f64, self.height as f64],
                filter,
            ),
        }
    }

    /// `Image.resize(.., NEAREST)`: `ImagingScaleAffine`, whose source column is accumulated in
    /// `double` (`xo += a[0]`) rather than computed per pixel.
    fn resize_nearest(&self, width: usize, height: usize) -> Image {
        let ch = self.mode.channels();
        let (ax, ay) = (
            self.width as f64 / width as f64,
            self.height as f64 / height as f64,
        );
        let coord = |v: f64| if v < 0.0 { -1 } else { v as i64 };
        let mut xs = Vec::with_capacity(width);
        let mut xo = ax * 0.5;
        for _ in 0..width {
            xs.push(coord(xo));
            xo += ax;
        }
        let mut out = vec![0u8; width * height * ch];
        let mut yo = ay * 0.5;
        for y in 0..height {
            let yi = coord(yo);
            if yi >= 0 && (yi as usize) < self.height {
                let row = &self.data[yi as usize * self.width * ch..][..self.width * ch];
                for (x, &xi) in xs.iter().enumerate() {
                    if xi >= 0 && (xi as usize) < self.width {
                        let (src, dst) = (xi as usize * ch, (y * width + x) * ch);
                        out[dst..dst + ch].copy_from_slice(&row[src..src + ch]);
                    }
                }
            }
            yo += ay;
        }
        Image {
            width,
            height,
            mode: self.mode,
            data: out,
        }
    }

    /// `image.filter(ImageFilter.GaussianBlur(radius))` for mode `L`: Pillow's three box-blur
    /// passes (`BoxBlur.c`), each rounding to 8 bits, along x and then along y.
    pub fn gaussian_blur(&self, radius: f32) -> Image {
        assert_eq!(self.mode, Mode::L);
        let (w, h) = (self.width, self.height);
        if w == 0 || h == 0 || radius == 0.0 {
            return self.clone();
        }
        let box_radius = gaussian_box_radius(radius, 3);
        let mut data = self.data.clone();
        for _ in 0..3 {
            for row in data.chunks_exact_mut(w) {
                let blurred = box_blur_line(row, box_radius);
                row.copy_from_slice(&blurred);
            }
        }
        let mut column = vec![0u8; h];
        for _ in 0..3 {
            for x in 0..w {
                for (y, c) in column.iter_mut().enumerate() {
                    *c = data[y * w + x];
                }
                let blurred = box_blur_line(&column, box_radius);
                for (y, &v) in blurred.iter().enumerate() {
                    data[y * w + x] = v;
                }
            }
        }
        Image {
            data,
            ..self.clone()
        }
    }

    /// `Image.thumbnail((x, y), Image.LANCZOS)` with the default `reducing_gap` of 2.0, on an
    /// image that is not a JPEG file (so `draft` does nothing). Replaces `self`.
    pub fn thumbnail(&mut self, x: i64, y: i64) {
        let Some((w, h)) = thumbnail_size(self.width, self.height, (x, y)) else {
            return;
        };
        if (self.width, self.height) != (w, h) {
            *self = self.resize(w, h, None, Some(2.0));
        }
    }

    /// `Image.reduce((fx, fy), box)`: the mean of each `fx` x `fy` block (the last, partial, block
    /// of a row or column over the pixels it has), in `ImagingReduce`'s rounding.
    pub fn reduce(&self, fx: usize, fy: usize, box_: [usize; 4]) -> Image {
        let ch = self.mode.channels();
        let (bw, bh) = (box_[2] - box_[0], box_[3] - box_[1]);
        let (ow, oh) = (bw.div_ceil(fx), bh.div_ceil(fy));
        let mut out = vec![0u8; ow * oh * ch];
        for oy in 0..oh {
            let y0 = box_[1] + oy * fy;
            let y1 = (y0 + fy).min(box_[3]);
            for ox in 0..ow {
                let x0 = box_[0] + ox * fx;
                let x1 = (x0 + fx).min(box_[2]);
                let count = (x1 - x0) * (y1 - y0);
                let multiplier = reduce_multiplier(count);
                let amend = (count / 2) as u32;
                for c in 0..ch {
                    let mut sum = amend;
                    for y in y0..y1 {
                        for x in x0..x1 {
                            sum += u32::from(self.data[(y * self.width + x) * ch + c]);
                        }
                    }
                    out[(oy * ow + ox) * ch + c] = (sum.wrapping_mul(multiplier) >> 24) as u8;
                }
            }
        }
        Image {
            width: ow,
            height: oh,
            mode: self.mode,
            data: out,
        }
    }

    /// `ImagingResample` with a (fractional) source box.
    fn resample(&self, width: usize, height: usize, box_: [f64; 4], filter: Filter) -> Image {
        let ch = self.mode.channels();
        let need_h = width != self.width || box_[0] != 0.0 || box_[2] != width as f64;
        let need_v = height != self.height || box_[1] != 0.0 || box_[3] != height as f64;
        let mut cur = Image {
            data: Vec::new(),
            ..self.clone()
        };
        let mut data: std::borrow::Cow<'_, [u8]> = std::borrow::Cow::Borrowed(&self.data);
        let (mut w, mut h) = (self.width, self.height);
        // Pillow 12.3 does the horizontal pass first, except on a picture more than 100 times
        // taller than it is wide, where it does the vertical one first. Each pass rounds to 8
        // bits, so the order is visible in the result. (Measured against Pillow over 4,000 sizes;
        // the threshold does not depend on the output size.)
        let vertical_first = self.height > 100 * self.width;
        let order = if vertical_first {
            [false, true]
        } else {
            [true, false]
        };
        for horizontal in order {
            if horizontal && need_h {
                data = std::borrow::Cow::Owned(resample_axis(
                    &data,
                    w,
                    h,
                    ch,
                    width,
                    (box_[0], box_[2]),
                    true,
                    filter,
                ));
                w = width;
            } else if !horizontal && need_v {
                data = std::borrow::Cow::Owned(resample_axis(
                    &data,
                    w,
                    h,
                    ch,
                    height,
                    (box_[1], box_[3]),
                    false,
                    filter,
                ));
                h = height;
            }
        }
        cur.data = data.into_owned();
        cur.width = w;
        cur.height = h;
        cur
    }
}

/// `division_UINT32(count, 8)` of `Reduce.c`: `2^32 / (256 * count)` in `float`.
fn reduce_multiplier(count: usize) -> u32 {
    let max_dividend = (256 * count) as u32;
    let max_int = (1u64 << 32) as f32;
    (max_int / max_dividend as f32) as u32
}

/// `Image.thumbnail`'s size arithmetic: the size the image becomes, `None` when the box is at least
/// as large as the image in both directions.
fn thumbnail_size(width: usize, height: usize, size: (i64, i64)) -> Option<(usize, usize)> {
    let (mut x, mut y) = size;
    let (w, h) = (width as i64, height as i64);
    if x >= w && y >= h {
        return None;
    }
    let aspect = w as f64 / h as f64;
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
    Some((x as usize, y as usize))
}

/// The resampling filters `Image.resize` is called with in this port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos,
}

impl Filter {
    /// `struct filter.support` of `Resample.c`.
    fn support(self) -> f64 {
        match self {
            Filter::Nearest => 0.0,
            Filter::Bilinear => 1.0,
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
        }
    }

    /// `bilinear_filter`, `bicubic_filter` (a = -0.5) and `lanczos_filter` of `Resample.c`.
    fn eval(self, x: f64) -> f64 {
        match self {
            Filter::Nearest => unreachable!("nearest does not go through the convolution"),
            Filter::Bilinear => {
                let x = x.abs();
                if x < 1.0 { 1.0 - x } else { 0.0 }
            }
            Filter::Bicubic => {
                const A: f64 = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
            Filter::Lanczos => lanczos(x),
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

/// One pass of `ImagingResample` for 8-bit pixels: `precompute_coeffs` over the input range
/// `(in0, in1)`, `normalize_coeffs_8bpc` and the 22-bit fixed-point sums.
#[allow(clippy::too_many_arguments)]
fn resample_axis(
    src: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    out_size: usize,
    (in0, in1): (f64, f64),
    horizontal: bool,
    filter: Filter,
) -> Vec<u8> {
    const PRECISION_BITS: u32 = 32 - 8 - 2;
    let in_size = if horizontal { width } else { height };
    // The box reaches C as `float[4]`: single precision, and the width is a `float` difference.
    let (in0, in1) = (in0 as f32, in1 as f32);
    let scale = f64::from(in1 - in0) / out_size as f64;
    let in0 = f64::from(in0);
    let filter_scale = scale.max(1.0);
    let support = filter.support() * filter_scale;
    let ss = 1.0 / filter_scale;
    let mut taps: Vec<(usize, Vec<i32>)> = Vec::with_capacity(out_size);
    for xx in 0..out_size {
        let center = in0 + (xx as f64 + 0.5) * scale;
        let xmin = ((center - support + 0.5) as i64).max(0) as usize;
        let xmax = (((center + support + 0.5) as i64).max(0) as usize).min(in_size);
        let mut k: Vec<f64> = (0..xmax.saturating_sub(xmin))
            .map(|x| filter.eval(((x + xmin) as f64 - center + 0.5) * ss))
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

/// `_gaussian_blur_radius` of `BoxBlur.c`, in the C's own mix of `float` and `double`.
fn gaussian_box_radius(radius: f32, passes: i32) -> f32 {
    let sigma2: f32 = radius * radius / passes as f32;
    let big_l = (12.0f64 * f64::from(sigma2) + 1.0).sqrt() as f32;
    let l = ((f64::from(big_l) - 1.0) / 2.0).floor() as f32;
    let mut a: f32 = (2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * sigma2);
    a /= 6.0 * (sigma2 - (l + 1.0) * (l + 1.0));
    l + a
}

/// One `ImagingLineBoxBlur8` over a line, with the weights `ImagingHorizontalBoxBlur` derives
/// from the fractional radius (all `UINT32` arithmetic, wrapping like C).
fn box_blur_line(line: &[u8], float_radius: f32) -> Vec<u8> {
    let xsize = line.len() as i32;
    let radius = float_radius as i32;
    let ww = (16_777_216f32 / (float_radius * 2.0 + 1.0)) as u32;
    let fw = (16_777_216u32.wrapping_sub((radius * 2 + 1) as u32 * ww)) / 2;
    let edge_a = (radius + 1).min(xsize);
    let edge_b = (xsize - radius - 1).max(0);
    let lastx = xsize - 1;
    let at = |i: i32| i32::from(line[i as usize]);
    let mut out = vec![0u8; line.len()];
    let mut acc: u32 = (at(0) * (radius + 1)) as u32;
    for x in 0..edge_a - 1 {
        acc = acc.wrapping_add(at(x) as u32);
    }
    acc = acc.wrapping_add((at(lastx) * (radius - edge_a + 1)) as u32);
    let mut step = |x: i32, subtract: i32, add: i32, left: i32, right: i32| {
        acc = acc.wrapping_add((at(add) - at(subtract)) as u32);
        let bulk = acc
            .wrapping_mul(ww)
            .wrapping_add(((at(left) + at(right)) as u32).wrapping_mul(fw));
        out[x as usize] = (bulk.wrapping_add(1 << 23) >> 24) as u8;
    };
    if edge_a <= edge_b {
        for x in 0..edge_a {
            step(x, 0, x + radius, 0, x + radius + 1);
        }
        for x in edge_a..edge_b {
            step(
                x,
                x - radius - 1,
                x + radius,
                x - radius - 1,
                x + radius + 1,
            );
        }
        for x in edge_b..=lastx {
            step(x, x - radius - 1, lastx, x - radius - 1, lastx);
        }
    } else {
        for x in 0..edge_b {
            step(x, 0, x + radius, 0, x + radius + 1);
        }
        for x in edge_b..edge_a {
            step(x, 0, lastx, 0, lastx);
        }
        for x in edge_a..=lastx {
            step(x, x - radius - 1, lastx, x - radius - 1, lastx);
        }
    }
    out
}
