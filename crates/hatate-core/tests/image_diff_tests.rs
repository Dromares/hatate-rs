//! `core/image_diff.py` (DAN-2127, Phase 4.e): `TestImageDifferenceMap` from
//! `tests/test_core_utils.py` at `76c85c4`, ported; the additive fields (`delta`, `local_rect`,
//! `match_rect`) tested directly; the error shape; and the result pinned to Pillow's on pictures
//! from the repo's own fixtures. The pictures here are drawn (`common::art`), not Pillow's, so
//! each ported test checks the property its Python original checks. `docs/image-diff.md` maps
//! each Python test to its Rust name.

mod common;

use std::path::Path;

use common::art::{Canvas, Color};
use hatate_core::error::Kind;
use hatate_core::image_compare::Source;
use hatate_core::image_diff::{
    CHANGE_THRESHOLD, Diff, Rect, difference_map, difference_map_images, verdict,
};
use hatate_core::pyraster::{self, Image, Mode};
use sha2::{Digest, Sha256};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/image_compare/images");

fn image_of(c: &Canvas) -> Image {
    let (w, h) = c.size();
    Image { width: w as usize, height: h as usize, mode: Mode::Rgb, data: c.img.as_raw().clone() }
}

fn scene(size: (u32, u32)) -> Canvas {
    let mut im = Canvas::new(size.0, size.1, [35, 80, 130]);
    im.ellipse([80.0, 80.0, 380.0, 380.0], [225, 95, 70]);
    im.rectangle([500.0, 300.0, 760.0, 540.0], [40, 200, 140]);
    im
}

fn diff(a: &Canvas, b: &Canvas) -> Diff {
    difference_map_images(Some(&image_of(a)), Some(&image_of(b)))
}

fn mask_mean(d: &Diff, x0: usize, y0: usize, x1: usize, y1: usize) -> f64 {
    let mask = d.mask.as_ref().unwrap();
    let mut sum = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += f64::from(mask[y * d.width + x]);
        }
    }
    sum / ((x1 - x0) * (y1 - y0)) as f64
}

fn recolour(c: &Canvas, f: impl Fn([u8; 3]) -> [u8; 3]) -> Canvas {
    let mut out = c.clone();
    for px in out.img.pixels_mut() {
        px.0 = f(px.0);
    }
    out
}

// ---- the Python tests --------------------------------------------------------------------------

#[test]
fn test_a_picture_against_itself_shows_nothing() {
    let base = scene((800, 600));
    assert_eq!(diff(&base, &base.clone()).changed_fraction, 0.0);
}

#[test]
fn test_a_resize_and_re_encode_is_not_a_difference() {
    let base = scene((800, 600));
    let jpeg = base.resized(1600, 1200).jpeg(70);
    let reopened = pyraster::open(&jpeg, None).unwrap();
    let result = difference_map_images(Some(&image_of(&base)), Some(&reopened));
    assert!(
        result.changed_percent() < 1.0,
        "re-encoding alone read as {:.1}% changed",
        result.changed_percent()
    );
}

#[test]
fn test_a_watermark_is_found() {
    let base = scene((800, 600));
    let mut marked = base.clone();
    marked.rectangle([600.0, 520.0, 780.0, 580.0], [255, 255, 255]);
    let result = diff(&base, &marked);
    assert!(result.changed_percent() > 0.5);
    assert!(result.reliable);
    assert!(result.verdict.contains("watermark"), "{}", result.verdict);
}

#[test]
fn test_the_mask_marks_where_the_change_is() {
    let base = scene((800, 600));
    let mut marked = base.clone();
    marked.rectangle([600.0, 480.0, 780.0, 580.0], [255, 255, 255]);
    let result = diff(&base, &marked);
    let (w, h) = (result.width, result.height);
    let bottom_right = mask_mean(&result, (w as f64 * 0.6) as usize, (h as f64 * 0.7) as usize, w, h);
    let top_left = mask_mean(&result, 0, 0, (w as f64 * 0.4) as usize, (h as f64 * 0.4) as usize);
    assert!(
        bottom_right > top_left * 5.0 + 1.0,
        "the highlight is not where the change was made"
    );
}

#[test]
fn test_a_wholesale_difference_declines_to_explain_itself() {
    let a = scene((800, 600));
    let mut b = Canvas::new(800, 600, [200, 180, 120]);
    b.polygon(&[(400.0, 60.0), (700.0, 500.0), (120.0, 520.0)], [60, 60, 200]);
    let result = diff(&a, &b);
    assert!(!result.reliable);
    assert!(result.verdict.contains("perceptual match"), "{}", result.verdict);
}

#[test]
fn test_different_shapes_are_refused_rather_than_distorted() {
    let result = diff(&scene((800, 600)), &scene((600, 800)));
    assert!(!result.reliable);
    assert!(result.verdict.contains("different shapes"), "{}", result.verdict);
}

#[test]
fn test_a_global_brightness_shift_is_not_an_edit() {
    let base = scene((800, 600));
    let brighter = recolour(&base, |p| p.map(|c| (f64::from(c) * 1.08).round().min(255.0) as u8));
    assert!(diff(&base, &brighter).changed_percent() < 5.0);
}

#[test]
fn test_a_recolour_at_the_same_brightness_is_found() {
    let mut base = scene((800, 600));
    let mut recoloured = base.clone();
    base.rectangle([100.0, 420.0, 400.0, 560.0], [190, 90, 90]);
    recoloured.rectangle([100.0, 420.0, 400.0, 560.0], [90, 150, 90]);
    assert!(diff(&base, &recoloured).changed_percent() > 1.0, "a colour-only change went unnoticed");
}

#[test]
fn test_the_strongest_channel_decides_not_the_average() {
    let mut base = scene((800, 600));
    let mut red_only = base.clone();
    red_only.rectangle([120.0, 430.0, 380.0, 550.0], [240, 80, 130]);
    base.rectangle([120.0, 430.0, 380.0, 550.0], [80, 80, 130]);
    assert!(diff(&base, &red_only).changed_percent() > 1.0);
}

#[test]
fn test_a_saturation_shift_is_not_an_edit() {
    let base = scene((800, 600));
    let more = recolour(&base, |p| {
        let grey = 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
        p.map(|c| (grey + (f64::from(c) - grey) * 1.10).round().clamp(0.0, 255.0) as u8)
    });
    assert!(diff(&base, &more).changed_percent() < 5.0);
}

#[test]
fn test_a_missing_image_is_handled() {
    let base = image_of(&scene((800, 600)));
    for (a, b) in [(None, Some(&base)), (Some(&base), None), (None, None)] {
        let result = difference_map_images(a, b);
        assert!(result.mask.is_none());
        assert!(!result.reliable);
        assert_eq!(result.verdict, "Nothing to compare");
        assert!(result.delta.is_empty() && result.local_rect.is_none() && result.match_rect.is_none());
    }
}

#[test]
fn test_a_small_image_is_not_upscaled() {
    let small = scene((120, 90));
    let result = diff(&small, &small.clone());
    assert_eq!((result.width, result.height), (120, 90));
    assert_eq!(result.mask.unwrap().len(), 120 * 90);
}

// ---- the additive fields -----------------------------------------------------------------------

#[test]
fn identical_pictures_have_no_rectangle() {
    let base = scene((800, 600));
    let result = diff(&base, &base.clone());
    assert_eq!(result.local_rect, None);
    assert_eq!(result.match_rect, None);
}

#[test]
fn delta_is_the_mask_before_the_threshold() {
    let base = scene((800, 600));
    let mut marked = base.clone();
    marked.rectangle([600.0, 480.0, 780.0, 580.0], [255, 255, 255]);
    let result = diff(&base, &marked);
    let mask = result.mask.as_ref().unwrap();
    assert_eq!(result.delta.len(), mask.len());
    assert_eq!(result.delta.len(), result.width * result.height);
    let mut over = 0;
    for (d, m) in result.delta.iter().zip(mask) {
        assert_eq!(*d >= CHANGE_THRESHOLD, *m != 0, "delta {d} mask {m}");
        over += usize::from(*d >= CHANGE_THRESHOLD);
    }
    assert!(over > 0);
    assert_eq!(over as f64 / mask.len() as f64, result.changed_fraction);
    assert_eq!(*mask.iter().max().unwrap(), 255, "the strongest change reads as full intensity");
}

#[test]
fn the_rectangle_covers_a_known_caption() {
    let base = scene((800, 600));
    let mut captioned = base.clone();
    captioned.rectangle([600.0, 520.0, 780.0, 580.0], [255, 255, 255]);
    let result = diff(&base, &captioned);
    let r = result.match_rect.expect("a caption was added");
    // The blur spreads the box by a few pixels each way; the caption is inside the rectangle and
    // the rectangle is not much bigger than the caption.
    assert!(r.x <= 600 && r.y <= 520, "{r:?}");
    assert!(r.x + r.w >= 781 && r.y + r.h >= 581, "{r:?}");
    assert!(r.x >= 590 && r.y >= 510 && r.x + r.w <= 792 && r.y + r.h <= 592, "{r:?}");
    assert_eq!(result.local_rect, Some(r), "same size: the same box");
}

#[test]
fn the_rectangles_are_in_each_pictures_own_pixels() {
    // The local picture is twice the size of the match; the working size is 900 on the long edge
    // for the local one, so the two boxes differ by the ratio of the two sizes.
    let small = scene((800, 600));
    let mut marked_small = small.clone();
    marked_small.rectangle([600.0, 520.0, 780.0, 580.0], [255, 255, 255]);
    let big = small.resized(1600, 1200);
    let result = diff(&big, &marked_small);
    let (m, l) = (result.match_rect.unwrap(), result.local_rect.unwrap());
    assert!(m.x + m.w <= 800 && m.y + m.h <= 600, "{m:?}");
    assert!(l.x + l.w <= 1600 && l.y + l.h <= 1200, "{l:?}");
    let near = |a: u32, b: u32| (i64::from(a) - i64::from(b)).abs() <= 6;
    assert!(near(l.x / 2, m.x) && near(l.y / 2, m.y), "local {l:?} match {m:?}");
}

#[test]
fn a_rectangle_never_leaves_the_picture() {
    // A change in the last row and column: the box is clipped to the picture, never past it.
    let base = Canvas::new(50, 40, [10, 10, 10]);
    let mut edge = base.clone();
    edge.rectangle([30.0, 20.0, 49.0, 39.0], [250, 250, 250]);
    let r = diff(&base, &edge).local_rect.unwrap();
    assert!(r.x + r.w <= 50 && r.y + r.h <= 40, "{r:?}");
}

#[test]
fn output_is_deterministic() {
    let base = scene((800, 600));
    let mut marked = base.clone();
    marked.rectangle([100.0, 100.0, 300.0, 200.0], [0, 0, 0]);
    assert_eq!(diff(&base, &marked), diff(&base, &marked));
}

#[test]
fn the_fixture_pair_gives_a_mask_and_a_box() {
    let result = difference_map(
        Source::Path(&Path::new(FIXTURES).join("crop_src.png")),
        Source::Path(&Path::new(FIXTURES).join("crop_cut.png")),
    )
    .unwrap();
    assert_eq!((result.width, result.height), (120, 90));
    assert!(result.mask.is_some() && result.local_rect.is_some());
    let l = result.local_rect.unwrap();
    assert!(l.w >= 1 && l.h >= 1 && l.x + l.w <= 120 && l.y + l.h <= 90);
    let m = result.match_rect.unwrap();
    assert!(m.x + m.w <= 110 && m.y + m.h <= 82, "{m:?}");
    // 110x82 against 120x90 is almost the same shape but a different crop: not reliable.
    assert!(!result.reliable);
}

// ---- the verdict text --------------------------------------------------------------------------

#[test]
fn the_verdict_bands_and_their_boundaries() {
    let v = |f: f64| verdict(f, true);
    assert!(v(0.0).starts_with("No visible differences"));
    assert!(v(0.0049).starts_with("No visible differences"));
    assert!(v(0.005).starts_with("0.5% of the frame differs, in small patches"));
    assert!(v(0.0499).starts_with("5.0% of the frame differs, in small patches"));
    assert!(v(0.05).starts_with("5% of the frame differs - large enough"));
    assert!(v(0.4499).starts_with("45% of the frame differs - large enough"));
    assert!(v(0.45).starts_with("45% of the frame differs - too much"));
    assert!(v(1.0).starts_with("100% of the frame differs - too much"));
    assert!(verdict(0.0, false).starts_with("These are different shapes"));
}

#[test]
fn percent_formatting_rounds_half_to_even_like_python() {
    // Python: f"{12.5:.0f}" == "12", f"{13.5:.0f}" == "14", f"{1.25:.1f}" == "1.2".
    assert!(verdict(0.125, true).starts_with("12% of the frame"));
    assert!(verdict(0.135, true).starts_with("14% of the frame"));
    assert!(verdict(0.0125, true).starts_with("1.2% of the frame"));
    assert!(verdict(0.0075, true).starts_with("0.8% of the frame"));
}

// ---- the error shape ---------------------------------------------------------------------------

#[test]
fn a_missing_file_gives_a_message_with_no_path_and_no_os_text() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("secret-name-that-must-not-leak.png");
    let ok = Path::new(FIXTURES).join("crop_src.png");
    let err = difference_map(Source::Path(&gone), Source::Path(&ok)).unwrap_err();
    assert_eq!(err.kind(), Kind::ImageUnreadable);
    let message = err.user_message();
    assert!(!message.contains("secret-name") && !message.contains("os error"), "{message}");
    assert!(!message.contains(dir.path().to_str().unwrap()), "{message}");
    assert!(err.detail().contains("os error"), "{}", err.detail());
    assert!(message.contains("local"), "{message}");
    // The other side is named too.
    let err = difference_map(Source::Path(&ok), Source::Path(&gone)).unwrap_err();
    assert!(err.user_message().contains("matched"));
}

#[test]
fn a_file_that_is_not_a_picture_is_undecodable_not_unreadable() {
    let ok = Path::new(FIXTURES).join("crop_src.png");
    let text = Path::new(FIXTURES).join("not_an_image.txt");
    let err = difference_map(Source::Path(&ok), Source::Path(&text)).unwrap_err();
    assert_eq!(err.kind(), Kind::ImageUndecodable);
    assert!(!err.user_message().contains("not_an_image"));
}

// ---- pinned to Pillow --------------------------------------------------------------------------

/// `(local, match, mask sha256, width, height, changed_fraction bits, verdict, reliable)`, computed
/// by `core/image_diff.py` at `76c85c4` under Pillow 12.3.0 and numpy 2.5.3 on these fixtures.
#[allow(clippy::type_complexity)]
const PILLOW: [(&str, &str, &str, usize, usize, u64, &str, bool); 12] = [
    ("crop_src.png", "crop_cut.png", "808b25e9c002a341b3106918e672cb5d7ae38b29327edd870a7ed2d90c540f09", 120, 90, 4603778865081628705, "62% of the frame differs - too much for this view to say WHAT changed. Either a heavily reworked version, a different crop, or a different picture; the perceptual match above is the better guide.", false),
    ("mirror_src.png", "mirror_flip.png", "61b2cba55e1c058d3e8c0e3304253f9d62905d1f6f4e982fcc100c7486b31da9", 60, 40, 4603707141087563175, "61% of the frame differs - too much for this view to say WHAT changed. Either a heavily reworked version, a different crop, or a different picture; the perceptual match above is the better guide.", false),
    ("rgb.png", "rgb_b.png", "74ef49e914ead1d70099d965c8a5e653aa4efd7ddf24a676ad7fe41a09fccc3b", 48, 36, 4603950669067413580, "64% of the frame differs - too much for this view to say WHAT changed. Either a heavily reworked version, a different crop, or a different picture; the perceptual match above is the better guide.", false),
    ("big_1024.jpg", "jpeg420.jpg", "e54a287eb9bd40ca42bbf93da635ae9628afb5ff582e427545d0e285e74aba8e", 900, 900, 4606943650179773212, "These are different shapes, so they cannot be lined up - the highlights below are the mismatch, not an edit. Compare them under the wipe instead.", false),
    ("lossy.webp", "lossless.webp", "68ac1de9ef3d8b702ce5a97137a39dfe36a5656c54385c10f6363a58d7d0b8dd", 48, 36, 4597612269591855104, "23% of the frame differs - large enough to be a real edit: added text, a censoring bar, a logo, or a retouch.", true),
    ("flat.png", "letterbox.png", "20f4f761f7f011283e8d43c4f7d717ac02ad916e91a68b135d8ab8420b9d8bee", 64, 64, 4603395700753956864, "These are different shapes, so they cannot be lined up - the highlights below are the mismatch, not an edit. Compare them under the wipe instead.", false),
    ("p.png", "rgba.png", "7636c9f8da9a82f9a9fcd804a25795a9c900e1c7e19f7442a7b113c8ccd64eb2", 48, 36, 0, "No visible differences - the two look like the same file's content.", true),
    ("wide_1100.png", "big_3500.png", "10dbdcf38b0ed498c34eadd385daccbcf3208d9236fe7893ccffb4e58ce1404e", 900, 49, 4607167304678818976, "These are different shapes, so they cannot be lined up - the highlights below are the mismatch, not an edit. Compare them under the wipe instead.", false),
    ("tall_3x451.png", "tiny_1x1.png", "a666a80213fa5395af35a221dd4a02a1940326501cb3a519417b3d736721949e", 3, 451, 4604160047431464259, "These are different shapes, so they cannot be lined up - the highlights below are the mismatch, not an edit. Compare them under the wipe instead.", false),
    ("jpeg444.jpg", "jpeg_gray.jpg", "929b4cff6a1bb3bd7c0af5dae15d1f6da0656823c190ab2c59d1a61aa3183ca9", 48, 36, 4600124694383976145, "36% of the frame differs - large enough to be a real edit: added text, a censoring bar, a logo, or a retouch.", true),
    ("crop_src.png", "crop_src.png", "158b8db9da6b60d8cd31b06f22d3a911fd7e64ec70a281848089a8eb150d1609", 120, 90, 0, "No visible differences - the two look like the same file's content.", true),
    ("exif_rot.jpg", "big_1100_prog.jpg", "a775a780014d3e38c3226efef356fc64323d36e83fa72d9f83ee646c156c1db3", 48, 36, 4607182418800017408, "These are different shapes, so they cannot be lined up - the highlights below are the mismatch, not an edit. Compare them under the wipe instead.", false),
];

#[test]
fn the_result_matches_pillow_and_numpy_on_the_fixtures() {
    for (a, b, sha, w, h, fraction, text, reliable) in PILLOW {
        let result = difference_map(
            Source::Path(&Path::new(FIXTURES).join(a)),
            Source::Path(&Path::new(FIXTURES).join(b)),
        )
        .unwrap();
        let mask = result.mask.as_ref().unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(mask)), sha, "{a} vs {b}: mask");
        assert_eq!((result.width, result.height), (w, h), "{a} vs {b}: size");
        assert_eq!(result.changed_fraction.to_bits(), fraction, "{a} vs {b}: fraction");
        assert_eq!(result.verdict, text, "{a} vs {b}: verdict");
        assert_eq!(result.reliable, reliable, "{a} vs {b}: reliable");
    }
}

#[test]
fn rect_type_is_plain_data() {
    let r = Rect { x: 1, y: 2, w: 3, h: 4 };
    assert_eq!(r, r);
}

#[allow(dead_code)]
fn color_is_used(_: Color) {}
