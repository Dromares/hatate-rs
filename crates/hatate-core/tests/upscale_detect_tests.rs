//! `core/upscale_detect.py` and `workers/upscale_check_worker.py` (DAN-2127, Phase 4.e): the
//! Python tests in `tests/test_upscale_detect.py` and `TestUpscaleCheckWorker` in
//! `tests/test_workers_background.py` at `76c85c4`, ported; the metadata scan over each place
//! Pillow keeps text; the cut-offs; and results pinned to Pillow's and numpy's on formula-built
//! pictures. `docs/upscale-detect.md` maps each Python test to its Rust name.

mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use common::art::Canvas;
use hatate_core::model::{Candidate, Entry};
use hatate_core::pyraster::{DecompressionBomb, Filter, Image, Mode};
use hatate_core::upscale_detect::{
    Checks, HARD_PIXEL_LIMIT, Real, SelfConsistencyResult, SourceComparisonResult, check_entries,
    check_entry, check_upscaler_metadata, compare_to_source, detect_naive_upscale,
};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/image_compare/images"
);

/// `_detailed`: fine per-pixel detail, `((x*7+y*3)%256, (x*3+y*11)%256, (x*13+y)%256)`.
fn detailed(w: usize, h: usize) -> Image {
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&[
                ((x * 7 + y * 3) % 256) as u8,
                ((x * 3 + y * 11) % 256) as u8,
                ((x * 13 + y) % 256) as u8,
            ]);
        }
    }
    Image {
        width: w,
        height: h,
        mode: Mode::Rgb,
        data,
    }
}

fn png_of(im: &Image) -> Vec<u8> {
    let rgb =
        image::RgbImage::from_raw(im.width as u32, im.height as u32, im.data.clone()).unwrap();
    let mut out = Vec::new();
    rgb.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn saved(dir: &tempfile::TempDir, name: &str, im: &Image) -> PathBuf {
    write(dir, name, &png_of(im))
}

fn analyse(p: &Path) -> SelfConsistencyResult {
    detect_naive_upscale(p).unwrap()
}

/// `_anime_style`: large flat fills plus some linework.
fn anime_style(size: u32) -> Canvas {
    let s = f64::from(size);
    let mut im = Canvas::new(size, size, [240, 220, 200]);
    im.rectangle([s * 0.1, s * 0.1, s * 0.5, s * 0.6], [60, 90, 180]);
    im.ellipse([s * 0.3, s * 0.05, s * 0.7, s * 0.45], [20, 20, 30]);
    for i in (0..size).step_by(4) {
        im.line(
            (f64::from(i), s * 0.7),
            (f64::from(i) + 8.0, s * 0.85),
            [0, 0, 0],
            1,
        );
    }
    im
}

// ---- tests/test_upscale_detect.py --------------------------------------------------------------

#[test]
fn test_flags_clearly_larger_local_file() {
    assert!(compare_to_source(2000, 2000, 1000, 1000, 15.0).flagged);
}

#[test]
fn test_ignores_minor_size_difference() {
    assert!(!compare_to_source(1005, 1005, 1000, 1000, 15.0).flagged);
}

#[test]
fn test_smaller_local_is_never_flagged() {
    assert!(!compare_to_source(500, 500, 1000, 1000, 15.0).flagged);
}

#[test]
fn test_missing_source_dimensions_is_safe() {
    let r = compare_to_source(1000, 1000, 0, 0, 15.0);
    assert!(!r.flagged);
    assert_eq!(r.source_size, (0, 0));
    assert_eq!(
        r.message,
        "No source dimensions available to compare against."
    );
}

#[test]
fn test_detects_a_genuinely_upscaled_image() {
    let dir = tempfile::tempdir().unwrap();
    let up = detailed(400, 400)
        .resize_filtered(100, 100, Filter::Lanczos)
        .resize_filtered(400, 400, Filter::Bicubic);
    let r = analyse(&saved(&dir, "up.png", &up));
    assert!(["likely", "possible"].contains(&r.confidence), "{r:?}");
}

#[test]
fn test_native_detail_is_not_flagged_as_likely() {
    let dir = tempfile::tempdir().unwrap();
    assert_ne!(
        analyse(&saved(&dir, "nat.png", &detailed(400, 400))).confidence,
        "likely"
    );
}

#[test]
fn test_flat_heavy_art_is_not_a_false_positive() {
    // REGRESSION (Python): measuring across the whole image let large flat regions dominate.
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, "anime.png", &anime_style(400).png());
    assert_eq!(analyse(&p).confidence, "none");
}

#[test]
fn test_difference_is_within_valid_range() {
    // REGRESSION (Python): the histogram was summed across concatenated RGB channels.
    let dir = tempfile::tempdir().unwrap();
    let diff = analyse(&saved(&dir, "range.png", &detailed(400, 400)))
        .best_diff
        .unwrap();
    assert!((0.0..=255.0).contains(&diff));
}

#[test]
fn test_oversized_image_is_skipped_not_attempted() {
    const { assert!(HARD_PIXEL_LIMIT > 0) };
    let dir = tempfile::tempdir().unwrap();
    let r = analyse(&write(&dir, "big.png", &header_only_png(7000, 7000)));
    assert_eq!(
        r.message,
        "Image is too large (7000\u{d7}7000) to safely analyze - skipped to avoid a potential crash."
    );
    assert!(r.best_diff.is_none() && r.detail_fraction.is_none());
    // Exactly at the limit is still attempted (6324 x 6324 is just under 40 million pixels).
    let at = analyse(&write(&dir, "edge.png", &header_only_png(6324, 6324)));
    assert!(!at.message.contains("too large"), "{}", at.message);
}

#[test]
fn test_detects_known_tool_in_png_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(
        &dir,
        "m.png",
        &with_chunk(
            &solid_png(60, 60),
            b"tEXt",
            b"Software\0Real-ESRGAN v0.3.0",
            true,
        ),
    );
    assert_eq!(
        check_upscaler_metadata(&p).unwrap().as_deref(),
        Some("Image metadata mentions \"real-esrgan\" - a known AI upscaling tool/model name.")
    );
}

#[test]
fn test_unrelated_software_is_not_flagged() {
    let dir = tempfile::tempdir().unwrap();
    let p = write(
        &dir,
        "g.png",
        &with_chunk(&solid_png(60, 60), b"tEXt", b"Software\0GIMP 2.10", true),
    );
    assert_eq!(check_upscaler_metadata(&p).unwrap(), None);
}

#[test]
fn test_clean_image_is_not_flagged() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        check_upscaler_metadata(&write(&dir, "c.png", &solid_png(60, 60))).unwrap(),
        None
    );
}

// ---- tests/test_workers_background.py::TestUpscaleCheckWorker ----------------------------------

/// The four calls the worker makes, recorded; what they answer is set per test.
struct Mock {
    dimensions: Option<(i64, i64)>,
    comparison: Option<SourceComparisonResult>,
    consistency: SelfConsistencyResult,
    metadata: Option<String>,
    opened: RefCell<Vec<String>>,
    compared: RefCell<Vec<(i64, i64, i64, i64)>>,
    naive: RefCell<Vec<String>>,
    meta: RefCell<Vec<String>>,
}

impl Mock {
    fn new() -> Mock {
        Mock {
            dimensions: Some((800, 600)),
            comparison: None,
            consistency: SelfConsistencyResult::default(),
            metadata: None,
            opened: RefCell::default(),
            compared: RefCell::default(),
            naive: RefCell::default(),
            meta: RefCell::default(),
        }
    }
}

impl Checks for Mock {
    fn dimensions(&self, path: &Path) -> Result<Option<(i64, i64)>, DecompressionBomb> {
        self.opened.borrow_mut().push(path.display().to_string());
        Ok(self.dimensions)
    }

    fn compare_to_source(&self, lw: i64, lh: i64, sw: i64, sh: i64) -> SourceComparisonResult {
        self.compared.borrow_mut().push((lw, lh, sw, sh));
        self.comparison
            .clone()
            .expect("a comparison was not expected")
    }

    fn check_upscaler_metadata(&self, path: &Path) -> Result<Option<String>, DecompressionBomb> {
        self.meta.borrow_mut().push(path.display().to_string());
        Ok(self.metadata.clone())
    }

    fn detect_naive_upscale(
        &self,
        path: &Path,
    ) -> Result<SelfConsistencyResult, DecompressionBomb> {
        self.naive.borrow_mut().push(path.display().to_string());
        Ok(self.consistency.clone())
    }
}

fn entry(path: &str) -> Entry {
    Entry::new(path)
}

fn with_candidate(e: &mut Entry, width: Option<i64>, height: Option<i64>) {
    let mut c = Candidate::new("http://example.com/1");
    c.width = width;
    c.height = height;
    e.candidates = vec![c];
    e.select_candidate(0);
}

fn run(
    entries: &mut [Entry],
    mock: &Mock,
) -> Vec<(String, hatate_core::upscale_detect::UpscaleCheckResult)> {
    let mut checked = Vec::new();
    check_entries(entries, mock, |e, r| {
        checked.push((e.path.clone(), r.clone()))
    })
    .unwrap();
    checked
}

#[test]
fn test_one_result_is_emitted_per_entry_and_then_finished_all() {
    let mut entries: Vec<Entry> = (0..3).map(|i| entry(&format!("/tmp/a{i}.png"))).collect();
    let out = run(&mut entries, &Mock::new());
    let paths: Vec<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["/tmp/a0.png", "/tmp/a1.png", "/tmp/a2.png"]);
}

#[test]
fn test_an_empty_batch_still_says_it_finished() {
    let out = run(&mut [], &Mock::new());
    assert!(out.is_empty());
}

#[test]
fn test_missing_local_dimensions_are_read_off_the_file_and_kept() {
    let mut e = [entry("/tmp/a.png")];
    assert_eq!(e[0].local_width, None);
    let mut mock = Mock::new();
    mock.dimensions = Some((1234, 567));
    run(&mut e, &mock);
    assert_eq!(
        (e[0].local_width, e[0].local_height),
        (Some(1234), Some(567))
    );
}

#[test]
fn test_already_known_dimensions_are_not_re_read_from_disk() {
    let mut e = entry("/tmp/a.png");
    e.local_width = Some(100);
    e.local_height = Some(50);
    let mock = Mock::new();
    run(std::slice::from_mut(&mut e), &mock);
    assert!(mock.opened.borrow().is_empty());
    assert_eq!((e.local_width, e.local_height), (Some(100), Some(50)));
}

#[test]
fn test_an_unreadable_file_is_logged_and_the_batch_carries_on() {
    let mut entries = [entry("/tmp/gone.png"), entry("/tmp/also-gone.png")];
    let mut mock = Mock::new();
    mock.dimensions = None;
    let out = run(&mut entries, &mock);
    assert_eq!(out.len(), 2);
    assert_eq!(entries[0].local_width, None);
}

#[test]
fn test_the_source_comparison_runs_only_when_both_sizes_are_known() {
    let comparison = SourceComparisonResult {
        flagged: true,
        local_size: (1600, 1200),
        source_size: (800, 600),
        message: "2x".into(),
    };
    let mut e = entry("/tmp/a.png");
    e.local_width = Some(1600);
    e.local_height = Some(1200);
    with_candidate(&mut e, Some(800), Some(600));
    let mut mock = Mock::new();
    mock.comparison = Some(comparison.clone());
    let out = run(std::slice::from_mut(&mut e), &mock);
    assert_eq!(*mock.compared.borrow(), [(1600, 1200, 800, 600)]);
    assert_eq!(out[0].1.source_comparison, Some(comparison));
}

#[test]
fn test_a_candidate_that_reports_no_dimensions_skips_the_comparison() {
    // REGRESSION shape (Python): comparing against 0 or None would flag every such match.
    for (w, h) in [
        (None, None),
        (Some(0), Some(0)),
        (Some(800), None),
        (None, Some(600)),
    ] {
        let mut e = entry("/tmp/a.png");
        e.local_width = Some(1600);
        e.local_height = Some(1200);
        with_candidate(&mut e, w, h);
        let mock = Mock::new();
        let out = run(std::slice::from_mut(&mut e), &mock);
        assert!(mock.compared.borrow().is_empty(), "{w:?} {h:?}");
        assert_eq!(out[0].1.source_comparison, None);
    }
}

#[test]
fn test_no_selected_candidate_skips_the_comparison_but_not_the_rest() {
    let mut e = entry("/tmp/a.png");
    e.local_width = Some(1600);
    e.local_height = Some(1200);
    let mut mock = Mock::new();
    mock.metadata = Some("Topaz Gigapixel AI".into());
    let out = run(std::slice::from_mut(&mut e), &mock);
    assert!(mock.compared.borrow().is_empty());
    assert_eq!(out[0].1.source_comparison, None);
    assert_eq!(
        out[0].1.metadata_signature.as_deref(),
        Some("Topaz Gigapixel AI")
    );
}

#[test]
fn test_unreadable_dimensions_do_not_stop_the_other_two_heuristics() {
    let mut e = entry("/tmp/gone.png");
    let mut mock = Mock::new();
    mock.dimensions = None;
    mock.metadata = Some("waifu2x".into());
    let out = run(std::slice::from_mut(&mut e), &mock);
    assert_eq!(*mock.naive.borrow(), ["/tmp/gone.png"]);
    assert_eq!(*mock.meta.borrow(), ["/tmp/gone.png"]);
    assert_eq!(out[0].1.metadata_signature.as_deref(), Some("waifu2x"));
}

#[test]
fn test_all_three_heuristics_land_in_the_one_emitted_result() {
    let mut e = entry("/tmp/a.png");
    e.local_width = Some(10);
    e.local_height = Some(10);
    let mut mock = Mock::new();
    mock.consistency = SelfConsistencyResult {
        confidence: "likely",
        message: "clean 2x round trip".into(),
        ..SelfConsistencyResult::default()
    };
    mock.metadata = Some("Real-ESRGAN".into());
    let out = run(std::slice::from_mut(&mut e), &mock);
    let r = &out[0].1;
    assert_eq!(r.self_consistency.as_ref(), Some(&mock.consistency));
    assert_eq!(r.metadata_signature.as_deref(), Some("Real-ESRGAN"));
    assert_eq!(r.source_comparison, None);
}

#[test]
fn the_real_checks_run_end_to_end_on_one_entry() {
    let dir = tempfile::tempdir().unwrap();
    let up = detailed(100, 100).resize_filtered(300, 300, Filter::Bilinear);
    let p = saved(&dir, "real.png", &up);
    let mut e = entry(p.to_str().unwrap());
    with_candidate(&mut e, Some(100), Some(100));
    let r = check_entry(&mut e, &Real).unwrap();
    assert_eq!((e.local_width, e.local_height), (Some(300), Some(300)));
    assert!(r.source_comparison.unwrap().flagged);
    assert_eq!(r.self_consistency.unwrap().confidence, "likely");
    assert_eq!(r.metadata_signature, None);
}

// ---- messages and cut-offs ---------------------------------------------------------------------

#[test]
fn the_source_comparison_messages_match_python() {
    let m = |a, b, c, d| compare_to_source(a, b, c, d, 15.0).message;
    assert_eq!(
        m(1155, 100, 1000, 100),
        "Local file (1155\u{d7}100) is 16% larger than the matched source (1000\u{d7}100) - likely upscaled from it."
    );
    assert_eq!(
        m(2050, 1, 1000, 1),
        "Local file (2050\u{d7}1) is 105% larger than the matched source (1000\u{d7}1) - likely upscaled from it."
    );
    assert_eq!(
        m(1125, 1125, 1000, 1000),
        "Local file (1125\u{d7}1125) is not meaningfully larger than the matched source (1000\u{d7}1000)."
    );
    // Wider but shorter than the source: not flagged, whatever the gain.
    assert!(!compare_to_source(2000, 500, 1000, 1000, 15.0).flagged);
}

#[test]
fn the_shipped_threshold_is_fifteen_percent_gain() {
    // `Real` is what the worker uses: 15.5% larger is flagged, 14.9% is not, at either edge.
    let flagged = |lw, lh, sw, sh| Real.compare_to_source(lw, lh, sw, sh).flagged;
    assert!(flagged(1155, 1155, 1000, 1000));
    // 1.15 - 1.0 is 0.1499999..., so exactly 15% on paper is not flagged, in Python or here.
    assert!(!flagged(1150, 800, 1000, 800));
    assert!(flagged(1151, 800, 1000, 800));
    assert!(!flagged(1149, 1149, 1000, 1000));
    assert!(flagged(800, 1155, 800, 1000), "the taller edge counts too");
}

#[test]
fn a_file_that_cannot_be_read_says_so_in_pythons_words() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone.png");
    let r = analyse(&gone);
    assert_eq!(
        r.message,
        format!(
            "Could not analyze image: [Errno 2] No such file or directory: '{}'",
            gone.display()
        )
    );
    let r = analyse(dir.path());
    assert_eq!(
        r.message,
        format!(
            "Could not analyze image: [Errno 21] Is a directory: '{}'",
            dir.path().display()
        )
    );
    let text = write(&dir, "x.png", b"not a picture\n");
    assert_eq!(
        analyse(&text).message,
        format!(
            "Could not analyze image: cannot identify image file '{}'",
            text.display()
        )
    );
    assert_eq!(check_upscaler_metadata(&text).unwrap(), None);
    assert_eq!(check_upscaler_metadata(&gone).unwrap(), None);
}

#[test]
fn small_pictures_are_too_small_to_test() {
    let dir = tempfile::tempdir().unwrap();
    for (w, h) in [(63, 200), (200, 63), (10, 10)] {
        let r = analyse(&saved(&dir, "s.png", &detailed(w, h)));
        assert_eq!(r.message, "Image too small to meaningfully test.");
        assert_eq!(r.confidence, "none");
    }
}

#[test]
fn a_picture_decoded_past_the_bomb_limit_is_raised_not_reported() {
    // Python's `DecompressionBombError` is neither OSError nor ValueError: it passes through.
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, "bomb.png", &header_only_png(20000, 10000));
    let err = detect_naive_upscale(&p).unwrap_err();
    assert!(err.to_string().contains("200000000 pixels"), "{err}");
    assert!(check_upscaler_metadata(&p).is_err());
}

#[test]
fn a_one_pixel_wide_result_reports_numpys_error() {
    // 64 x 90000 shrinks to 1 x 2000 for the analysis, and np.gradient refuses one pixel.
    let dir = tempfile::tempdir().unwrap();
    let thin = Image {
        width: 64,
        height: 90000,
        mode: Mode::Rgb,
        data: [200, 10, 10].repeat(64 * 90000),
    };
    let r = analyse(&saved(&dir, "thin.png", &thin));
    assert_eq!(
        r.message,
        "Could not analyze image: Shape of array too small to calculate a numerical gradient, \
         at least (edge_order + 1) elements are required."
    );
    assert_eq!(r.detail_fraction, None);
}

#[test]
fn a_flat_picture_is_not_enough_signal() {
    let dir = tempfile::tempdir().unwrap();
    let flat = Image {
        width: 128,
        height: 128,
        mode: Mode::Rgb,
        data: vec![240; 128 * 128 * 3],
    };
    let r = analyse(&saved(&dir, "flat.png", &flat));
    assert_eq!(r.detail_fraction, Some(0.0));
    assert!(
        r.message
            .starts_with("This image is almost entirely flat/simple content")
    );
    assert_eq!(r.best_diff, None);
}

// ---- pinned to Pillow and numpy ----------------------------------------------------------------

type Golden = (
    &'static str,
    f64,
    &'static str,
    u64,
    u64,
    &'static str,
    &'static str,
);

/// `(case, best_factor, best_filter, best_diff bits, detail_fraction bits, confidence, message)`
/// from `detect_naive_upscale` at `76c85c4` under Pillow 12.3.0 and numpy 2.5.3.
const PILLOW: [Golden; 7] = [
    (
        "native400",
        1.5,
        "Bicubic",
        4620730195203063808,
        4604860982077093944,
        "possible",
        "Possibly upscaled ~1.5\u{d7} using Bicubic interpolation (in detail regions, avg diff 8.1/255) - not a strong signal, could be coincidental.",
    ),
    (
        "chain100to400",
        1.5,
        "Lanczos",
        4602719715314368512,
        4599019644475408384,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Lanczos interpolation (in detail regions, round-trip reconstruction very close, avg diff 0.5/255).",
    ),
    (
        "bilinear150to450",
        1.5,
        "Lanczos",
        4607552027042512896,
        4603527808862401190,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Lanczos interpolation (in detail regions, round-trip reconstruction very close, avg diff 1.1/255).",
    ),
    (
        "nearest120x90to360x270",
        1.5,
        "Nearest-neighbor",
        4610987726001405952,
        4598455999522044577,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Nearest-neighbor interpolation (in detail regions, round-trip reconstruction very close, avg diff 1.8/255).",
    ),
    (
        "lanczos200to300",
        1.5,
        "Lanczos",
        4615028850529665024,
        4599430422801420433,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Lanczos interpolation (in detail regions, round-trip reconstruction very close, avg diff 3.5/255).",
    ),
    (
        "bicubic100to250",
        1.5,
        "Lanczos",
        4608843682085339136,
        4599180855327669738,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Lanczos interpolation (in detail regions, round-trip reconstruction very close, avg diff 1.4/255).",
    ),
    (
        "bicubic90x70to360x280",
        1.5,
        "Lanczos",
        4603387104376913920,
        4598718510928895714,
        "likely",
        "Likely upscaled ~1.5\u{d7} using Lanczos interpolation (in detail regions, round-trip reconstruction very close, avg diff 0.6/255).",
    ),
];

#[test]
fn the_self_consistency_result_matches_pillow_and_numpy() {
    let dir = tempfile::tempdir().unwrap();
    for (case, factor, filter, diff, fraction, confidence, message) in PILLOW {
        let im = match case {
            "native400" => detailed(400, 400),
            "chain100to400" => detailed(400, 400)
                .resize_filtered(100, 100, Filter::Lanczos)
                .resize_filtered(400, 400, Filter::Bicubic),
            "bilinear150to450" => detailed(150, 150).resize_filtered(450, 450, Filter::Bilinear),
            "nearest120x90to360x270" => {
                detailed(120, 90).resize_filtered(360, 270, Filter::Nearest)
            }
            "lanczos200to300" => detailed(200, 200).resize_filtered(300, 300, Filter::Lanczos),
            "bicubic100to250" => detailed(100, 100).resize_filtered(250, 250, Filter::Bicubic),
            "bicubic90x70to360x280" => detailed(90, 70).resize_filtered(360, 280, Filter::Bicubic),
            other => panic!("{other}"),
        };
        let r = analyse(&saved(&dir, &format!("{case}.png"), &im));
        assert_eq!(r.best_factor, Some(factor), "{case}");
        assert_eq!(r.best_filter, Some(filter), "{case}");
        assert_eq!(r.best_diff.map(f64::to_bits), Some(diff), "{case}");
        assert_eq!(
            r.detail_fraction.map(f64::to_bits),
            Some(fraction),
            "{case}"
        );
        assert_eq!(r.confidence, confidence, "{case}");
        assert_eq!(r.message, message, "{case}");
    }
}

// ---- the metadata scan: every place Pillow keeps text ------------------------------------------

fn crc32(cid: &[u8], data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(cid);
    h.update(data);
    h.finalize()
}

fn raw_chunk(cid: &[u8], data: &[u8]) -> Vec<u8> {
    let mut v = (data.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(cid);
    v.extend_from_slice(data);
    v.extend_from_slice(&crc32(cid, data).to_be_bytes());
    v
}

/// Splice a chunk into a PNG before the first IDAT (`before`) or before IEND.
fn with_chunk(png: &[u8], cid: &[u8], data: &[u8], before: bool) -> Vec<u8> {
    let mut at = 8;
    loop {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let name = &png[at + 4..at + 8];
        if (before && name == b"IDAT") || (!before && name == b"IEND") {
            break;
        }
        at += 12 + len;
    }
    let mut out = png[..at].to_vec();
    out.extend_from_slice(&raw_chunk(cid, data));
    out.extend_from_slice(&png[at..]);
    out
}

fn solid_png(w: u32, h: u32) -> Vec<u8> {
    Canvas::new(w, h, [10, 120, 30]).png()
}

/// A PNG that is only a header: a signature, IHDR and the start of an IDAT.
fn header_only_png(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    v.extend_from_slice(&raw_chunk(b"IHDR", &ihdr));
    v.extend_from_slice(&[0, 0, 0, 0]);
    v.extend_from_slice(b"IDAT");
    v
}

fn zlib(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// The metadata message for `bytes` saved as `name`, or `None`.
fn found(name: &str, bytes: &[u8]) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let p = write(&dir, name, bytes);
    check_upscaler_metadata(&p).unwrap()
}

fn says(tool: &str) -> Option<String> {
    Some(format!(
        "Image metadata mentions \"{tool}\" - a known AI upscaling tool/model name."
    ))
}

/// A little-endian EXIF block with one ASCII tag.
fn exif_ascii(tag: u16, text: &str) -> Vec<u8> {
    let mut v = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&tag.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    let value = [text.as_bytes(), &[0]].concat();
    v.extend_from_slice(&(value.len() as u32).to_le_bytes());
    v.extend_from_slice(&(8u32 + 2 + 12 + 4).to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&value);
    v
}

#[test]
fn png_ztxt_and_itxt_and_a_late_eximf_are_all_read() {
    let base = solid_png(40, 40);
    let mut z = b"Comment\0\0".to_vec();
    z.extend_from_slice(&zlib(b"made with Waifu2X"));
    assert_eq!(
        found("z.png", &with_chunk(&base, b"zTXt", &z, true)),
        says("waifu2x")
    );

    let mut plain = b"Description\0\0\0en\0Description\0".to_vec();
    plain.extend_from_slice("topaz gigapixel".as_bytes());
    assert_eq!(
        found("i.png", &with_chunk(&base, b"iTXt", &plain, true)),
        says("topaz gigapixel")
    );
    let mut packed = b"Description\0\x01\0en\0Description\0".to_vec();
    packed.extend_from_slice(&zlib("\u{212a}x anime4\u{212a}".as_bytes()));
    assert_eq!(
        found("j.png", &with_chunk(&base, b"iTXt", &packed, true)),
        says("anime4k")
    );

    // eXIf after the pixel data: Pillow reads it only because getexif() loads the picture.
    let late = with_chunk(&base, b"eXIf", &exif_ascii(305, "Real-ESRGAN"), false);
    assert_eq!(found("late.png", &late), says("real-esrgan"));
    let early = with_chunk(&base, b"eXIf", &exif_ascii(270, "Upscayl 2.0"), true);
    assert_eq!(found("early.png", &early), says("upscayl"));
    // The whole eXIf payload is a bytes value of `info`, so a tool name is found in any tag (Make,
    // 271) as well, as the text of the bytes.
    assert_eq!(
        found(
            "make.png",
            &with_chunk(&base, b"eXIf", &exif_ascii(271, "waifu2x"), true)
        ),
        says("waifu2x")
    );
}

#[test]
fn a_tool_name_split_over_two_text_values_is_joined_with_a_space() {
    let base = with_chunk(&solid_png(40, 40), b"tEXt", b"A\0topaz", true);
    let both = with_chunk(&base, b"tEXt", b"B\0gigapixel ai", true);
    assert_eq!(found("two.png", &both), says("topaz gigapixel"));
    // Upper case and a trailing space do not matter; a near miss does.
    assert_eq!(
        found(
            "miss.png",
            &with_chunk(&solid_png(40, 40), b"tEXt", b"A\0topaz  gigapixel", true)
        ),
        None
    );
}

#[test]
fn a_chunk_with_a_wrong_checksum_makes_the_file_unreadable_to_pillow_so_nothing_is_found() {
    let mut bad = with_chunk(&solid_png(40, 40), b"tEXt", b"Software\0waifu2x", true);
    let at = bad.windows(8).position(|w| w == b"Software").unwrap();
    bad[at] ^= 0x20;
    assert_eq!(found("crc.png", &bad), None);
}

#[test]
fn text_chunks_after_the_pixel_data_are_not_in_info_when_the_scan_reads_it() {
    let late = with_chunk(&solid_png(40, 40), b"tEXt", b"Software\0waifu2x", false);
    assert_eq!(found("late-text.png", &late), None);
}

#[test]
fn raw_profile_type_exif_feeds_the_exif_tags() {
    let exif = exif_ascii(305, "ChaiNNer");
    let hex: String = exif.iter().map(|b| format!("{b:02x}")).collect();
    let text = format!("Raw profile type exif\0\nexif\n    {}\n{hex}\n", exif.len());
    let png = with_chunk(&solid_png(40, 40), b"tEXt", text.as_bytes(), true);
    assert_eq!(found("raw.png", &png), says("chainner"));
}

fn jpeg_of(w: u32, h: u32) -> Vec<u8> {
    Canvas::new(w, h, [200, 10, 10]).jpeg(80)
}

fn jpeg_with(segment_marker: u8, payload: &[u8], jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, segment_marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

#[test]
fn jpeg_comment_exif_xmp_and_icc_are_read() {
    let base = jpeg_of(40, 30);
    assert_eq!(
        found("c.jpg", &jpeg_with(0xfe, b"x SwinIR x", &base)),
        says("swinir")
    );
    assert_eq!(
        found(
            "e.jpg",
            &jpeg_with(
                0xe1,
                &[b"Exif\0\0".as_slice(), &exif_ascii(315, "Cupscale")].concat(),
                &base
            )
        ),
        says("cupscale")
    );
    let xmp = [
        b"http://ns.adobe.com/xap/1.0/\0".as_slice(),
        b"<x>LetsEnhance</x>",
    ]
    .concat();
    assert_eq!(
        found("x.jpg", &jpeg_with(0xe1, &xmp, &base)),
        says("letsenhance")
    );
    // ICC profile fragments before the frame header, one fragment of one.
    let icc = [
        b"ICC_PROFILE\0".as_slice(),
        &[1, 1],
        b"..stable diffusion upscale..",
    ]
    .concat();
    assert_eq!(
        found("i.jpg", &jpeg_with(0xe2, &icc, &base)),
        says("stable diffusion upscale")
    );
    // Two fragments declared but one present: Pillow drops the profile.
    let lone = [b"ICC_PROFILE\0".as_slice(), &[1, 2], b"realsr"].concat();
    assert_eq!(found("l.jpg", &jpeg_with(0xe2, &lone, &base)), None);
    assert_eq!(found("clean.jpg", &base), None);
}

#[test]
fn a_gif_comment_in_the_first_frame_is_read() {
    // A 1 x 1 GIF with a comment extension in front of the image descriptor.
    let mut g = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff".to_vec();
    g.extend_from_slice(b"\x21\xfe\x0fmade by Anime4K\x00");
    g.extend_from_slice(b",\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00;");
    assert_eq!(found("c.gif", &g), says("anime4k"));
    let mut clean = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff".to_vec();
    clean.extend_from_slice(b",\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00;");
    assert_eq!(found("clean.gif", &clean), None);
}

#[test]
fn webp_exif_is_read_only_when_the_vp8x_flag_is_set() {
    let lossless = std::fs::read(Path::new(FIXTURES).join("lossless.webp")).unwrap();
    let vp8l = &lossless[12..];
    let build = |flags: u8| {
        let exif = exif_ascii(305, "Real-ESRGAN");
        let mut vp8x = vec![flags, 0, 0, 0];
        vp8x.extend_from_slice(&[47, 0, 0, 35, 0, 0]);
        let mut body = b"WEBP".to_vec();
        body.extend_from_slice(b"VP8X");
        body.extend_from_slice(&10u32.to_le_bytes());
        body.extend_from_slice(&vp8x);
        body.extend_from_slice(vp8l);
        body.extend_from_slice(b"EXIF");
        body.extend_from_slice(&(exif.len() as u32).to_le_bytes());
        body.extend_from_slice(&exif);
        if exif.len() % 2 == 1 {
            body.push(0);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    };
    assert_eq!(found("flag.webp", &build(0x08)), says("real-esrgan"));
    assert_eq!(found("noflag.webp", &build(0x00)), None);
}

#[test]
fn a_tiff_file_is_scanned_through_its_own_ifd0() {
    // The fixture TIFF carries no text: nothing found, and no panic on the IFD walk.
    let real = std::fs::read(Path::new(FIXTURES).join("tiff.tif")).unwrap();
    assert_eq!(found("plain.tif", &real), None);
}

#[test]
fn undecodable_names_in_the_metadata_do_not_raise() {
    // Bytes that are not UTF-8 are dropped, not an error: a comment with a stray byte.
    let base = jpeg_of(40, 30);
    let comment = [b"swin".as_slice(), &[0xff, 0xfe], b"ir"].concat();
    assert_eq!(
        found("bad.jpg", &jpeg_with(0xfe, &comment, &base)),
        says("swinir")
    );
}
