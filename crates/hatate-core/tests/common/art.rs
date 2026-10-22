//! Synthetic pictures for the image-compare tests: the Python tests draw with `PIL.ImageDraw`
//! and these draw the same shapes with plain arithmetic. Nothing here is a photograph or a file
//! from anywhere; every picture is made in the test that uses it.

#![allow(dead_code)]

use image::{Rgb, RgbImage};

pub type Color = [u8; 3];

/// A drawing surface with the `ImageDraw` calls the Python tests use.
#[derive(Clone)]
pub struct Canvas {
    pub img: RgbImage,
}

impl Canvas {
    pub fn new(w: u32, h: u32, fill: Color) -> Canvas {
        Canvas {
            img: RgbImage::from_pixel(w, h, Rgb(fill)),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.img.dimensions()
    }

    fn put(&mut self, x: i64, y: i64, c: Color) {
        if x >= 0 && y >= 0 && (x as u32) < self.img.width() && (y as u32) < self.img.height() {
            self.img.put_pixel(x as u32, y as u32, Rgb(c));
        }
    }

    /// `draw.rectangle([x0, y0, x1, y1])`: both corners inside.
    pub fn rectangle(&mut self, b: [f64; 4], c: Color) {
        for y in b[1].round() as i64..=b[3].round() as i64 {
            for x in b[0].round() as i64..=b[2].round() as i64 {
                self.put(x, y, c);
            }
        }
    }

    /// `draw.ellipse([x0, y0, x1, y1])`.
    pub fn ellipse(&mut self, b: [f64; 4], c: Color) {
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let (rx, ry) = (
            ((b[2] - b[0]) / 2.0 + 0.5).max(0.5),
            ((b[3] - b[1]) / 2.0 + 0.5).max(0.5),
        );
        for y in b[1].floor() as i64..=b[3].ceil() as i64 {
            for x in b[0].floor() as i64..=b[2].ceil() as i64 {
                let (dx, dy) = ((x as f64 - cx) / rx, (y as f64 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// `draw.polygon(points)`: even-odd fill.
    pub fn polygon(&mut self, pts: &[(f64, f64)], c: Color) {
        let (ymin, ymax) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(p.1), hi.max(p.1))
        });
        for y in ymin.floor() as i64..=ymax.ceil() as i64 {
            let yf = y as f64 + 0.5;
            let mut xs: Vec<f64> = Vec::new();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 <= yf && b.1 > yf) || (b.1 <= yf && a.1 > yf) {
                    xs.push(a.0 + (yf - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    for x in x0.round() as i64..=x1.round() as i64 {
                        self.put(x, y, c);
                    }
                }
            }
        }
    }

    /// `draw.line([(x0, y0), (x1, y1)], width=w)`.
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64), c: Color, width: u32) {
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0) as i64;
        let half = i64::from(width) / 2;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let (x, y) = (
                (a.0 + (b.0 - a.0) * t).round() as i64,
                (a.1 + (b.1 - a.1) * t).round() as i64,
            );
            for dy in -half..=half {
                for dx in -half..=half {
                    self.put(x + dx, y + dy, c);
                }
            }
        }
    }

    /// `image.resize(size, Image.LANCZOS)`.
    pub fn resized(&self, w: u32, h: u32) -> Canvas {
        Canvas {
            img: image::imageops::resize(&self.img, w, h, image::imageops::FilterType::Lanczos3),
        }
    }

    pub fn mirrored(&self) -> Canvas {
        Canvas {
            img: image::imageops::flip_horizontal(&self.img),
        }
    }

    /// `image.crop((left, top, right, bottom))`.
    pub fn cropped(&self, l: u32, t: u32, r: u32, b: u32) -> Canvas {
        Canvas {
            img: image::imageops::crop_imm(&self.img, l, t, r - l, b - t).to_image(),
        }
    }

    /// `ImageOps.expand(image, border=(bx, by), fill=color)`.
    pub fn expanded(&self, bx: u32, by: u32, fill: Color) -> Canvas {
        let (w, h) = self.size();
        let mut out = Canvas::new(w + 2 * bx, h + 2 * by, fill);
        image::imageops::replace(&mut out.img, &self.img, i64::from(bx), i64::from(by));
        out
    }

    /// `image.thumbnail((n, n))` for a picture larger than `n`: the long edge becomes `n`.
    pub fn thumbnail(&self, longest: u32) -> Canvas {
        let (w, h) = self.size();
        let s = f64::from(longest) / f64::from(w.max(h));
        self.resized(
            ((f64::from(w) * s).round() as u32).max(1),
            ((f64::from(h) * s).round() as u32).max(1),
        )
    }

    pub fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.img
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    pub fn jpeg(&self, quality: u8) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
            .encode_image(&self.img)
            .unwrap();
        out
    }
}

/// `test_image_compare.artwork`: structured content rather than noise.
pub fn artwork(w: u32, h: u32) -> Canvas {
    let mut im = Canvas::new(w, h, [238, 220, 205]);
    let (wf, hf) = (f64::from(w), f64::from(h));
    im.ellipse([wf * 0.25, hf * 0.08, wf * 0.75, hf * 0.55], [30, 32, 48]);
    im.rectangle([wf * 0.15, hf * 0.55, wf * 0.85, hf * 0.95], [58, 92, 176]);
    for i in 0..12 {
        let y = hf * 0.6 + f64::from(i) * hf * 0.025;
        im.line(
            (wf * 0.2, y),
            (wf * 0.8, y),
            [230, 230, 240],
            (h / 300).max(1),
        );
    }
    im.ellipse(
        [wf * 0.38, hf * 0.22, wf * 0.44, hf * 0.28],
        [250, 250, 250],
    );
    im
}

/// `_picture(seed, size)`: bands, blocks and a diagonal; different seeds give different pictures.
pub fn picture(seed: u64, w: u32, h: u32) -> Canvas {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    let mut next = move |n: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % n
    };
    let mut im = Canvas::new(w, h, [next(256) as u8, next(256) as u8, next(256) as u8]);
    for _ in 0..14 {
        let (x0, y0) = (next(u64::from(w)) as f64, next(u64::from(h)) as f64);
        im.rectangle(
            [
                x0,
                y0,
                x0 + 60.0 + next(200) as f64,
                y0 + 60.0 + next(200) as f64,
            ],
            [next(256) as u8, next(256) as u8, next(256) as u8],
        );
    }
    im.line(
        (0.0, 0.0),
        (f64::from(w), f64::from(h)),
        [255, 255, 255],
        25,
    );
    im
}

/// `test_multipage.page_art`: pages that differ the way real multi-image sets do.
pub fn page_art(index: usize) -> Canvas {
    let mut im = Canvas::new(400, 600, [245, 240, 235]);
    match index % 5 {
        0 => im.rectangle([20.0, 20.0, 380.0, 300.0], [20, 40, 160]),
        1 => im.ellipse([40.0, 200.0, 360.0, 560.0], [200, 60, 60]),
        2 => {
            for i in 0..5 {
                let y = f64::from(i) * 90.0;
                im.rectangle([20.0, 40.0 + y, 380.0, 100.0 + y], [30, 150, 70]);
            }
        }
        3 => im.polygon(
            &[(200.0, 30.0), (380.0, 560.0), (20.0, 560.0)],
            [240, 190, 20],
        ),
        _ => {
            for i in 0..4u32 {
                let (cx, cy) = (f64::from(i % 2) * 180.0, f64::from(i / 2) * 180.0);
                im.ellipse(
                    [40.0 + cx, 40.0 + cy, 180.0 + cx, 180.0 + cy],
                    [120, 20, 140],
                );
            }
        }
    }
    im
}

/// `test_multipage.thumbnail_bytes(image, size)`: a Lanczos resize, saved as JPEG at 80.
pub fn thumbnail_bytes(image: &Canvas) -> Vec<u8> {
    image.resized(250, 375).jpeg(80)
}
