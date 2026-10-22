//! `core/image_compare.py` (DAN-2062, Phase 3.g): the Python tests in `tests/test_image_compare.py`,
//! `TestPerceptualSimilarityScale` in `tests/test_core_utils.py` and `TestDistanceFixtures` /
//! `TestVerdictDecisionTable` (the comparison half) in `tests/test_hydrus_duplicate_relationships.py`
//! at `76c85c4`, ported. The pictures are drawn here (`common::art`) and are not Pillow's, so each
//! test checks the property its Python original checks, not a pixel. The hashes themselves are
//! pinned to Pillow's by `image_compare_golden.rs`. `docs/image-compare.md` maps each Python test
//! to its Rust name.

mod common;

use common::art::{Canvas, artwork, picture};
use hatate_core::image_compare::{
    self as ic, Comparison, DHASH_SAME_MAX, DHASH_SIMILAR_MAX, FINE_CONFIRM_MAX,
    MIRRORED_MAX_SIMILARITY, Source, UNCONFIRMED_MAX_SIMILARITY, aligned_score, aligned_similarity,
    compact_size_delta, compare_file_sizes, compare_sizes, describe_perceptual_match, dhash,
    hamming_distance, local_prints, perceptual_similarity, size_delta_sort_key, trim_border,
};
use hatate_core::pyraster::{Image, Mode};

fn image_of(c: &Canvas) -> Image {
    let (w, h) = c.size();
    Image {
        width: w as usize,
        height: h as usize,
        mode: Mode::Rgb,
        data: c.img.as_raw().clone(),
    }
}

fn hash_of(c: &Canvas) -> ic::Hash {
    dhash(Source::Image(&image_of(c)), 8).unwrap().unwrap()
}

fn hash_bytes(b: &[u8]) -> Option<ic::Hash> {
    dhash(Source::Bytes(b), 8).unwrap()
}

fn s(v: i64) -> Option<i64> {
    Some(v)
}

// -- TestSizeComparison

#[test]
fn test_larger_match() {
    let c = compare_sizes(s(1000), s(1500), s(2000), s(3000));
    assert!(c.verdict.contains("larger"));
    assert!(c.remote_is_bigger());
    assert_eq!(
        c.megapixel_ratio.unwrap().round() as i64,
        4,
        "twice the long edge is four times the pixels"
    );
}

#[test]
fn test_smaller_match_is_phrased_as_smaller() {
    let c = compare_sizes(s(2000), s(3000), s(1000), s(1500));
    assert!(c.verdict.contains("smaller"));
    assert!(!c.remote_is_bigger());
}

#[test]
fn test_identical() {
    assert_eq!(
        compare_sizes(s(1200), s(1600), s(1200), s(1600)).verdict,
        "same size"
    );
}

#[test]
fn test_a_pixel_or_two_is_not_an_upgrade() {
    let c = compare_sizes(s(1200), s(1600), s(1201), s(1601));
    assert_eq!(c.verdict, "same size");
    assert!(!c.remote_is_bigger());
}

#[test]
fn test_different_aspect_is_called_out() {
    let c = compare_sizes(s(1000), s(1500), s(2000), s(2000));
    assert!(c.aspect_differs);
    assert!(c.verdict.contains("different shape"));
}

#[test]
fn test_invalid_dimensions_yield_no_verdict() {
    for (a, b, c, d) in [
        (s(0), s(0), s(100), s(100)),
        (s(100), s(100), s(0), s(0)),
        (s(-5), s(10), s(10), s(10)),
        (None, s(1), s(1), s(1)),
    ] {
        assert_eq!(
            compare_sizes(a, b, c, d).verdict,
            "",
            "{a:?} {b:?} {c:?} {d:?}"
        );
    }
}

#[test]
fn test_file_size_comparison() {
    assert!(compare_file_sizes(s(1_000_000), s(2_000_000)).contains("bigger"));
    assert!(compare_file_sizes(s(2_000_000), s(1_000_000)).contains("smaller"));
    assert!(compare_file_sizes(s(1_000_000), s(1_020_000)).contains("about the same"));
    assert_eq!(compare_file_sizes(s(1_000_000), None), "");
    assert_eq!(compare_file_sizes(s(0), s(100)), "");
}

#[test]
fn test_a_large_ratio_is_never_in_exponent_form() {
    assert_eq!(
        compare_file_sizes(s(1_000), s(1_500_000)),
        "1,500\u{d7} bigger file"
    );
    assert_eq!(
        compare_file_sizes(s(1_000), s(150_000)),
        "150\u{d7} bigger file"
    );
    assert_eq!(
        compare_file_sizes(s(150_000), s(1_000)),
        "150\u{d7} smaller file"
    );
    assert_eq!(
        compare_file_sizes(s(1_000), s(2_500)),
        "2.5\u{d7} bigger file"
    );
    assert_eq!(
        compare_file_sizes(s(1_000), s(12_400)),
        "12\u{d7} bigger file"
    );
}

#[test]
fn test_the_size_column_is_never_in_exponent_form_either() {
    assert_eq!(
        compact_size_delta(s(10), s(10), s(12_000), s(12_000)),
        "+ 1,200x"
    );
    assert_eq!(compact_size_delta(s(100), s(100), s(250), s(250)), "+ 2.5x");
    assert_eq!(
        compact_size_delta(s(12_000), s(12_000), s(10), s(10)),
        "- 1,200x"
    );
}

// -- TestCompactSizeDelta

#[test]
fn test_bigger_match() {
    assert_eq!(
        compact_size_delta(s(1000), s(1500), s(4000), s(6000)),
        "+ 4x"
    );
}

#[test]
fn test_smaller_match_counts_how_many_times_smaller() {
    assert_eq!(
        compact_size_delta(s(4000), s(6000), s(1000), s(1500)),
        "- 4x"
    );
}

#[test]
fn test_equal() {
    assert_eq!(compact_size_delta(s(1200), s(1600), s(1200), s(1600)), "=");
}

#[test]
fn test_a_pixel_or_two_still_counts_as_equal() {
    assert_eq!(compact_size_delta(s(1200), s(1600), s(1201), s(1601)), "=");
}

#[test]
fn test_fractional_ratio() {
    assert_eq!(
        compact_size_delta(s(1000), s(1500), s(1500), s(2250)),
        "+ 1.5x"
    );
}

#[test]
fn test_unknown_dimensions_give_no_label() {
    for (a, b, c, d) in [
        (s(1000), s(1500), s(0), s(0)),
        (s(0), s(0), s(1000), s(1500)),
        (None, s(1), s(1), s(1)),
    ] {
        assert_eq!(compact_size_delta(a, b, c, d), "");
    }
}

#[test]
fn test_agrees_with_the_preview_panel() {
    let delta = compact_size_delta(s(1000), s(1500), s(2000), s(3000));
    let comparison = compare_sizes(s(1000), s(1500), s(2000), s(3000));
    assert!(delta.starts_with('+'));
    assert!(comparison.remote_is_bigger());
}

#[test]
fn test_sorts_by_ratio_not_by_text() {
    let pairs = [
        ((1000, 1500), (4000, 6000)), // + 4x
        ((4000, 6000), (1000, 1500)), // - 4x
        ((1200, 1600), (1200, 1600)), // =
        ((1000, 1500), (1500, 2250)), // + 1.5x
    ];
    let key =
        |p: &((i64, i64), (i64, i64))| size_delta_sort_key(s(p.0.0), s(p.0.1), s(p.1.0), s(p.1.1));
    let mut ordered = pairs.to_vec();
    ordered.sort_by(|a, b| key(a).total_cmp(&key(b)));
    let labels: Vec<String> = ordered
        .iter()
        .map(|p| compact_size_delta(s(p.0.0), s(p.0.1), s(p.1.0), s(p.1.1)))
        .collect();
    assert_eq!(labels, ["- 4x", "=", "+ 1.5x", "+ 4x"]);
}

#[test]
fn test_unknown_sorts_to_one_end_not_into_parity() {
    assert!(
        size_delta_sort_key(s(1000), s(1500), s(0), s(0))
            < size_delta_sort_key(s(1200), s(1600), s(1200), s(1600))
    );
}

// -- TestPerceptualMatching

fn original() -> Canvas {
    artwork(1000, 1400)
}

#[test]
fn test_same_image_at_double_resolution() {
    let o = original();
    let bigger = o.resized(2000, 2800);
    let d = hamming_distance(Some(&hash_of(&o)), Some(&hash_of(&bigger))).unwrap();
    assert!(d <= 5, "distance {d}");
}

#[test]
fn test_survives_heavy_jpeg_re_encoding() {
    let o = original();
    let d = hamming_distance(Some(&hash_of(&o)), hash_bytes(&o.jpeg(40)).as_ref()).unwrap();
    assert!(d <= 5, "distance {d}");
}

#[test]
fn test_survives_downscale_and_re_encode_together() {
    let o = original();
    let sample = o.resized(850, 1190).jpeg(70);
    let d = hamming_distance(Some(&hash_of(&o)), hash_bytes(&sample).as_ref()).unwrap();
    assert!(d <= 5, "distance {d}");
}

#[test]
fn test_different_images_are_reported_as_different() {
    let o = original();
    let mut other = artwork(1000, 1400);
    other.rectangle([0.0, 0.0, 1000.0, 700.0], [200, 40, 40]);
    other.ellipse([100.0, 800.0, 900.0, 1300.0], [20, 200, 90]);
    let d = hamming_distance(Some(&hash_of(&o)), Some(&hash_of(&other))).unwrap();
    assert!(d > 12, "distance {d}");
}

#[test]
fn test_accepts_bytes_and_paths_and_images() {
    let o = original();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.png");
    std::fs::write(&path, o.png()).unwrap();
    let by_path = dhash(Source::Path(&path), 8).unwrap();
    assert_eq!(by_path, dhash(Source::Bytes(&o.png()), 8).unwrap());
    assert_eq!(by_path, Some(hash_of(&o)));
}

#[test]
fn test_unreadable_source_returns_none_rather_than_raising() {
    assert_eq!(dhash(Source::Bytes(b"not an image"), 8).unwrap(), None);
    assert_eq!(
        dhash(
            Source::Path(std::path::Path::new("/nonexistent/path.jpg")),
            8
        )
        .unwrap(),
        None
    );
    assert_eq!(
        hamming_distance(None, Some(&ic::Hash([0, 0, 0, 123]))),
        None
    );
}

#[test]
fn test_descriptions_are_hedged_not_absolute() {
    assert!(describe_perceptual_match(Some(0)).contains("Looks like"));
    assert!(describe_perceptual_match(Some(8)).contains("Similar"));
    assert!(describe_perceptual_match(Some(30)).contains("different"));
    assert!(describe_perceptual_match(None).contains("Couldn't"));
}

// -- TestAlignedComparison

fn local() -> Canvas {
    picture(1, 600, 800)
}

fn jpeg_thumb(c: &Canvas) -> Vec<u8> {
    c.thumbnail(250).jpeg(70)
}

fn score(prints: &ic::LocalPrints, c: &Canvas) -> (Option<f64>, bool) {
    aligned_similarity(Some(prints), Source::Bytes(&jpeg_thumb(c)))
}

fn prints_of(c: &Canvas) -> ic::LocalPrints {
    local_prints(Source::Image(&image_of(c))).expect("the picture can be fingerprinted")
}

#[test]
fn test_a_plain_thumbnail_is_the_same_picture() {
    let l = local();
    assert!(score(&prints_of(&l), &l).0.unwrap() >= 90.0);
}

#[test]
fn test_a_cropped_copy_is_the_same_picture() {
    let l = local();
    let p = prints_of(&l);
    let (w, h) = l.size();
    for (a, b, c, d) in [
        (24, 32, w - 24, h - 32),
        (0, 0, w, (f64::from(h) * 0.9) as u32),
    ] {
        let cut = l.cropped(a, b, c, d);
        assert!(score(&p, &cut).0.unwrap() >= 90.0, "box {a} {b} {c} {d}");
    }
}

#[test]
fn test_a_letterboxed_copy_is_the_same_picture() {
    let l = local();
    let padded = l.expanded(0, 64, [0, 0, 0]);
    assert!(score(&prints_of(&l), &padded).0.unwrap() >= 90.0);
}

#[test]
fn test_a_mirrored_copy_is_recognised_but_kept_below_auto_import() {
    let l = local();
    let (sc, mirrored) = score(&prints_of(&l), &l.mirrored());
    let sc = sc.unwrap();
    assert!(mirrored);
    assert!(sc <= MIRRORED_MAX_SIMILARITY);
    assert!(sc >= 75.0, "score {sc}");
}

#[test]
fn test_a_different_picture_is_not() {
    let l = local();
    let p = prints_of(&l);
    for seed in [2, 3, 4, 5] {
        let sc = score(&p, &picture(seed, 600, 800)).0.unwrap();
        assert!(sc < 75.0, "seed {seed}: {sc}");
    }
}

#[test]
fn test_a_border_is_trimmed_and_a_picture_without_one_is_left_alone() {
    let l = local();
    let padded = l.expanded(40, 0, [255, 255, 255]);
    let trimmed = trim_border(&image_of(&padded)).expect("the bars are trimmed");
    assert_eq!((trimmed.width, trimmed.height), (600, 800));
    assert!(trim_border(&image_of(&l)).is_none());
}

#[test]
fn test_nothing_to_compare_gives_no_score() {
    let l = local();
    let p = prints_of(&l);
    assert_eq!(
        aligned_similarity(None, Source::Bytes(&jpeg_thumb(&l))),
        (None, false)
    );
    assert!(local_prints(Source::Bytes(b"not an image")).is_none());
    assert_eq!(
        aligned_similarity(Some(&p), Source::Bytes(b"not an image")),
        (None, false)
    );
}

#[test]
fn test_a_90_the_fine_hash_does_not_confirm_is_held_for_review() {
    let cmp = |distance, fine| Comparison {
        distance: Some(distance),
        mirrored: false,
        fine_distance: fine,
    };
    for (c, expected) in [
        (
            cmp(1, Some(FINE_CONFIRM_MAX + 10)),
            Some(UNCONFIRMED_MAX_SIMILARITY),
        ),
        (cmp(1, None), Some(UNCONFIRMED_MAX_SIMILARITY)),
        (cmp(1, Some(FINE_CONFIRM_MAX)), Some(98.0)),
        (cmp(10, Some(60)), None), // below 90 anyway: untouched
    ] {
        let (sc, _) = aligned_score(&c);
        let sc = sc.unwrap();
        match expected {
            None => assert!(sc < 90.0),
            Some(e) => assert_eq!(sc, e),
        }
    }
}

/// A PNG that states a size and nothing else: `Image.open` reads the header and stops, which is
/// all the decompression-bomb check needs.
fn png_claiming(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend_from_slice(&13u32.to_be_bytes());
    let mut body = b"IHDR".to_vec();
    body.extend_from_slice(&ihdr);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    // Pillow reads chunks up to the first IDAT before it checks the size.
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(b"IDAT");
    out.extend_from_slice(&crc32(b"IDAT").to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn test_an_oversized_image_fails_quietly() {
    // 400 million pixels: past twice Pillow's `MAX_IMAGE_PIXELS`, so `Image.open` raises
    // `DecompressionBombError`, which `local_prints` catches and `dhash` does not.
    let huge = png_claiming(20_000, 20_000);
    assert!(local_prints(Source::Bytes(&huge)).is_none());
    assert!(dhash(Source::Bytes(&huge), 8).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.png");
    std::fs::write(&path, &huge).unwrap();
    assert!(local_prints(Source::Path(&path)).is_none());
}

// -- test_core_utils.TestPerceptualSimilarityScale

#[test]
fn test_the_anchors_land_where_the_bands_say() {
    assert_eq!(perceptual_similarity(Some(0)), Some(100.0));
    assert_eq!(
        perceptual_similarity(Some(i64::from(DHASH_SAME_MAX))),
        Some(90.0)
    );
    assert_eq!(
        perceptual_similarity(Some(i64::from(DHASH_SIMILAR_MAX))),
        Some(70.0)
    );
    assert_eq!(perceptual_similarity(Some(32)), Some(0.0));
}

#[test]
fn test_it_decreases_all_the_way_down() {
    let scores: Vec<f64> = (0..=32)
        .map(|d| perceptual_similarity(Some(d)).unwrap())
        .collect();
    let mut sorted = scores.clone();
    sorted.sort_by(|a, b| b.total_cmp(a));
    assert_eq!(scores, sorted);
}

#[test]
fn test_a_distance_beyond_the_scale_is_zero_not_negative() {
    assert_eq!(perceptual_similarity(Some(64)), Some(0.0));
}

#[test]
fn test_no_distance_means_no_number() {
    assert_eq!(perceptual_similarity(None), None);
}

/// `Image.effect_mandelbrot((256, 256), (-2, -1.5, 1, 1.5), 40).convert("RGB")`: escape time
/// as grey. A picture with a lot of structure, from a formula.
fn mandelbrot() -> Canvas {
    let mut c = Canvas::new(256, 256, [0, 0, 0]);
    for py in 0..256u32 {
        for px in 0..256u32 {
            let (cr, ci) = (
                -2.0 + 3.0 * f64::from(px) / 255.0,
                -1.5 + 3.0 * f64::from(py) / 255.0,
            );
            let (mut zr, mut zi, mut n) = (0.0f64, 0.0f64, 0u32);
            while n < 40 && zr * zr + zi * zi <= 4.0 {
                (zr, zi) = (zr * zr - zi * zi + cr, 2.0 * zr * zi + ci);
                n += 1;
            }
            let v = (n * 255 / 40) as u8;
            c.img.put_pixel(px, py, image::Rgb([v, v, v]));
        }
    }
    c
}

#[test]
fn test_a_resize_and_a_re_encode_still_read_as_the_same_picture() {
    let source = mandelbrot();
    let original = hash_bytes(&source.png());
    let smaller = source.resized(128, 128).jpeg(40);
    let distance = hamming_distance(original.as_ref(), hash_bytes(&smaller).as_ref());
    assert!(perceptual_similarity(distance.map(i64::from)).unwrap() >= 90.0);
}

#[test]
fn test_a_different_picture_scores_low() {
    let one = hash_bytes(&mandelbrot().png());
    let two = hash_bytes(&Canvas::new(256, 256, [128, 128, 128]).png());
    let distance = hamming_distance(one.as_ref(), two.as_ref());
    assert!(perceptual_similarity(distance.map(i64::from)).unwrap() < 50.0);
}

// -- test_hydrus_duplicate_relationships: the comparison half (TestDistanceFixtures and
//    TestVerdictDecisionTable's thresholds). `duplicate_verdict` itself is Hydrus code.

fn pattern(size: u32) -> Canvas {
    let mut c = Canvas::new(size, size, [0, 0, 0]);
    for x in 0..size {
        for y in 0..size {
            c.img.put_pixel(
                x,
                y,
                image::Rgb([
                    ((x * 3 + y) % 256) as u8,
                    ((x * x + y * 7) % 256) as u8,
                    ((x + y * 5) % 256) as u8,
                ]),
            );
        }
    }
    c
}

/// Pillow's own LANCZOS resize, from the port the hashes use.
fn pillow_resize(c: &Canvas, w: usize, h: usize) -> Canvas {
    let r = image_of(c).resize(w, h, None, None);
    Canvas {
        img: image::RgbImage::from_raw(w as u32, h as u32, r.data).unwrap(),
    }
}

fn compare_fixture(base: &Canvas, other: &Canvas) -> Comparison {
    let local = image_of(base);
    let prints = local_prints(Source::Image(&local));
    ic::compare_to_local(prints.as_ref(), Source::Bytes(&other.png()))
}

#[test]
fn test_the_bigger_copy_is_a_confirmed_same_picture() {
    let base = pattern(240);
    let c = compare_fixture(&base, &pillow_resize(&base, 480, 480));
    assert!(c.distance.unwrap() <= DHASH_SAME_MAX);
    assert!(c.fine_distance.unwrap() <= FINE_CONFIRM_MAX);
    assert!(!c.mirrored);
    // The Python test's comment: "64-bit 0, 256-bit 1".
    assert_eq!((c.distance, c.fine_distance), (Some(0), Some(1)));
}

#[test]
fn test_the_edited_variant_lands_in_the_edit_or_crop_band() {
    let base = pattern(240);
    let mut edited = base.clone();
    edited.rectangle([0.0, 168.0, 240.0, 240.0], [250, 250, 250]);
    let c = compare_fixture(&base, &edited);
    assert!(c.distance.unwrap() > DHASH_SAME_MAX);
    assert!(c.distance.unwrap() <= DHASH_SIMILAR_MAX);
    // "64-bit 7, 256-bit 21".
    assert_eq!((c.distance, c.fine_distance), (Some(7), Some(21)));
}

#[test]
fn test_the_mirrored_copy_is_recognised_as_mirrored() {
    let base = pattern(240);
    assert!(compare_fixture(&base, &base.mirrored()).mirrored);
}

#[test]
fn test_the_different_picture_is_past_the_band() {
    let base = pattern(240);
    let rotated = Canvas {
        img: image::imageops::rotate90(&pattern(200).img),
    };
    let different = pillow_resize(&rotated, 240, 240);
    assert!(compare_fixture(&base, &different).distance.unwrap() > DHASH_SIMILAR_MAX);
}

#[test]
fn test_the_boundaries_are_inclusive() {
    // `duplicate_verdict`'s three bands, as the comparison numbers it reads: the boundary
    // distances still score as the bands they close.
    assert_eq!(
        perceptual_similarity(Some(i64::from(DHASH_SAME_MAX))),
        Some(90.0)
    );
    let same = Comparison {
        distance: Some(DHASH_SAME_MAX),
        mirrored: false,
        fine_distance: Some(FINE_CONFIRM_MAX),
    };
    assert_eq!(
        aligned_score(&same).0,
        Some(90.0),
        "confirmed at the boundary: not held back"
    );
    let unconfirmed = Comparison {
        fine_distance: Some(FINE_CONFIRM_MAX + 1),
        ..same.clone()
    };
    assert_eq!(
        aligned_score(&unconfirmed).0,
        Some(UNCONFIRMED_MAX_SIMILARITY.min(90.0))
    );
}
