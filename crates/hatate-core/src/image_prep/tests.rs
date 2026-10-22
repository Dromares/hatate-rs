//! The Python tests of `core/image_prep.py` (`tests/test_image_prep.py` at `76c85c4`), through the
//! Rust port. The ladder is driven with real generated images: noise does not compress, so a PNG
//! of it is reliably over any cap a test sets and each rung of the quality ladder lands at a
//! different size. Caps are picked from the sizes this encoder measures for the same image, not
//! from Pillow's (the JPEG bytes are not matched, see the module docs).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::{ColorType as PngColor, ImageEncoder as _};

use super::*;

/// Seeded noise (xorshift): sizes must be reproducible.
fn noise(len: usize) -> Vec<u8> {
    let mut state: u64 = 20740;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Mode {
    Rgb,
    Grey,
    Rgba,
}

fn noise_png(width: u32, height: u32, mode: Mode) -> Vec<u8> {
    let (channels, color) = match mode {
        Mode::Rgb => (3, PngColor::Rgb8),
        Mode::Grey => (1, PngColor::L8),
        Mode::Rgba => (4, PngColor::Rgba8),
    };
    let data = noise((width * height) as usize * channels);
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&data, width, height, color.into())
        .unwrap();
    out
}

fn pixels(width: u32, height: u32) -> Pixels {
    Pixels {
        width,
        height,
        grey: false,
        data: noise((width * height * 3) as usize),
    }
}

struct Case {
    dir: tempfile::TempDir,
    prep: UploadPrep,
}

impl Case {
    fn new() -> Case {
        Case {
            dir: tempfile::Builder::new()
                .prefix("hatate-dan2057-")
                .tempdir()
                .unwrap(),
            prep: UploadPrep::new(),
        }
    }

    fn write(&self, name: &str, data: &[u8]) -> std::path::PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, data).unwrap();
        path
    }

    fn write_png(&self, name: &str, size: (u32, u32), mode: Mode) -> std::path::PathBuf {
        self.write(name, &noise_png(size.0, size.1, mode))
    }

    fn prepare(&self, path: &Path, max_bytes: u64, max_dimension: u32) -> (Bytes, String) {
        self.prep.prepare(path, max_bytes, max_dimension).unwrap()
    }
}

fn decoded(data: &[u8]) -> (image::ImageFormat, DynamicImage) {
    let reader = image::ImageReader::new(io::Cursor::new(data.to_vec()))
        .with_guessed_format()
        .unwrap();
    (reader.format().unwrap(), reader.decode().unwrap())
}

/// The size of the 1200x1200 noise picture at each quality of the ladder, 90 down to 40.
fn ladder_sizes() -> Vec<usize> {
    let im = pixels(1200, 1200);
    (4..=9)
        .rev()
        .map(|q| im.to_jpeg(q * 10).unwrap().len())
        .collect()
}

// --- TestSmallEnoughFile -------------------------------------------------------------------

#[test]
fn raw_bytes_and_original_filename_are_used_unchanged() {
    let case = Case::new();
    let raw = b"not really an image, and it does not need to be";
    let path = case.write("small.png", raw);
    let (data, filename) = case.prepare(&path, 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(&data[..], &raw[..]);
    assert_eq!(filename, "small.png");
}

#[test]
fn no_decode_happens_for_a_file_already_under_the_cap() {
    let case = Case::new();
    let path = case.write("small.png", &[b'x'; 10]);
    let got = case.prep.prepare_with(
        &path,
        1024,
        DEFAULT_MAX_DIMENSION,
        |p| std::fs::metadata(p).map(|m| m.len()),
        |_, _, _, _| panic!("downscale must not run"),
    );
    assert!(got.is_ok());
}

#[test]
fn a_file_exactly_at_the_cap_is_not_downscaled() {
    // The comparison is `<=`, so the boundary belongs to the cheap path.
    let case = Case::new();
    let raw = [b'y'; 512];
    let path = case.write("exact.png", &raw);
    let got = case
        .prep
        .prepare_with(
            &path,
            512,
            DEFAULT_MAX_DIMENSION,
            |p| std::fs::metadata(p).map(|m| m.len()),
            |_, _, _, _| panic!("downscale must not run"),
        )
        .unwrap();
    assert_eq!(&got.0[..], &raw[..]);
    assert_eq!(got.1, "exact.png");
}

// --- TestStatFailure -----------------------------------------------------------------------

#[test]
fn unreadable_size_falls_back_to_the_raw_bytes() {
    let case = Case::new();
    let raw = b"contents that still read fine";
    let path = case.write("odd.png", raw);
    let (data, filename) = case
        .prep
        .prepare_with(
            &path,
            1,
            DEFAULT_MAX_DIMENSION,
            |_| Err(io::Error::from_raw_os_error(2)),
            |_, _, _, _| panic!("downscale must not run"),
        )
        .unwrap();
    assert_eq!(&data[..], &raw[..]);
    assert_eq!(filename, "odd.png");
}

#[test]
fn stat_failure_does_not_poison_the_cache() {
    // The fallback returns before the cache is written.
    let case = Case::new();
    let path = case.write("odd.png", b"abc");
    case.prep
        .prepare_with(
            &path,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_DIMENSION,
            |_| Err(io::Error::from_raw_os_error(2)),
            downscale_to_fit,
        )
        .unwrap();
    assert!(case.prep.last_prepared().is_none());
}

#[test]
fn a_file_that_cannot_be_read_at_all_is_the_oserror() {
    let case = Case::new();
    let path = case.dir.path().join("missing.png");
    let err = case
        .prep
        .prepare(&path, 1024, DEFAULT_MAX_DIMENSION)
        .unwrap_err();
    assert_eq!(
        os_error_text(&err, &path),
        format!("[Errno 2] No such file or directory: '{}'", path.display())
    );
}

// --- TestOverCapDownscale ------------------------------------------------------------------

#[test]
fn result_fits_under_the_cap_and_is_a_real_jpeg() {
    // The one cap that quality 90 alone satisfies: nothing shrinks.
    let case = Case::new();
    let path = case.write_png("big.png", (1200, 1200), Mode::Rgb);
    let sizes = ladder_sizes();
    let cap = sizes[0] as u64 + 1000;
    assert!(std::fs::metadata(&path).unwrap().len() > cap);

    let (data, filename) = case.prepare(&path, cap, DEFAULT_MAX_DIMENSION);

    assert!(data.len() as u64 <= cap);
    assert_eq!(filename, "big.jpg");
    let (format, img) = decoded(&data);
    assert_eq!(format, image::ImageFormat::Jpeg);
    assert_eq!(
        (img.width(), img.height()),
        (1200, 1200),
        "cap met on quality alone"
    );
}

#[test]
fn max_dimension_is_applied_before_the_quality_ladder() {
    let case = Case::new();
    let path = case.write_png("big.png", (1200, 1200), Mode::Rgb);
    let (data, _) = case.prepare(&path, 120 * 1024, 300);
    let (_, img) = decoded(&data);
    assert!(img.width().max(img.height()) <= 300);
}

#[test]
fn a_small_but_heavy_file_is_never_shrunk_below_the_floor() {
    // 400x400 is inside max_dimension, so the thumbnail step is skipped, and it is already under
    // the shrink loop's 500 px floor: the dimensions come out untouched though the cap is
    // unreachable.
    let case = Case::new();
    let path = case.write_png("big.png", (400, 400), Mode::Rgb);
    let (data, _) = case.prepare(&path, 8 * 1024, 3000);
    let (_, img) = decoded(&data);
    assert_eq!((img.width(), img.height()), (400, 400));
}

#[test]
fn quality_ladder_walks_down_when_quality_90_is_not_enough() {
    // A cap quality 90 cannot meet but a lower rung can. The output is exactly the encoder's
    // output at the rung the walk stops on, and the dimensions are untouched.
    let case = Case::new();
    let path = case.write_png("big.png", (1200, 1200), Mode::Rgb);
    let sizes = ladder_sizes(); // 90, 80, 70, 60, 50, 40
    assert!(sizes[0] > sizes[1] && sizes[1] > sizes[2] && sizes[2] > sizes[3]);
    let cap = ((sizes[2] + sizes[3]) / 2) as u64; // between q70 and q60

    let (data, _) = case.prepare(&path, cap, DEFAULT_MAX_DIMENSION);

    assert!(data.len() as u64 <= cap);
    assert_eq!(
        &data[..],
        &pixels(1200, 1200).to_jpeg(60).unwrap()[..],
        "stopped on q60"
    );
    let (_, img) = decoded(&data);
    assert_eq!((img.width(), img.height()), (1200, 1200));
}

#[test]
fn shrink_loop_runs_when_even_quality_40_is_too_big() {
    let case = Case::new();
    let path = case.write_png("big.png", (1200, 1200), Mode::Rgb);
    let cap = (ladder_sizes()[5] * 2 / 3) as u64; // under the q40 size
    let (data, _) = case.prepare(&path, cap, DEFAULT_MAX_DIMENSION);
    assert!(data.len() as u64 <= cap);
    let (_, img) = decoded(&data);
    assert!(img.width().max(img.height()) < 1200);
}

#[test]
fn shrink_loop_stops_at_the_500px_floor_rather_than_forever() {
    // An impossible cap: the loop ends with an image around the floor and hands back an
    // over-cap result rather than spinning or failing.
    let case = Case::new();
    let path = case.write_png("big.png", (1200, 1200), Mode::Rgb);
    let (data, filename) = case.prepare(&path, 1, DEFAULT_MAX_DIMENSION);
    assert_eq!(filename, "big.jpg");
    assert!(data.len() > 1);
    let (_, img) = decoded(&data);
    assert!(img.width().max(img.height()) <= 500);
}

#[test]
fn rgba_is_converted_since_jpeg_has_no_alpha_channel() {
    let case = Case::new();
    let path = case.write_png("alpha.png", (900, 900), Mode::Rgba);
    let (data, filename) = case.prepare(&path, 60 * 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(filename, "alpha.jpg");
    let (format, img) = decoded(&data);
    assert_eq!(format, image::ImageFormat::Jpeg);
    assert!(
        matches!(img, DynamicImage::ImageRgb8(_)),
        "{:?}",
        img.color()
    );
}

#[test]
fn greyscale_is_kept_as_l_rather_than_widened_to_rgb() {
    let case = Case::new();
    let path = case.write_png("grey.png", (900, 900), Mode::Grey);
    let (data, _) = case.prepare(&path, 60 * 1024, DEFAULT_MAX_DIMENSION);
    let (_, img) = decoded(&data);
    assert!(
        matches!(img, DynamicImage::ImageLuma8(_)),
        "{:?}",
        img.color()
    );
}

#[test]
fn extensionless_name_still_gets_a_jpg_suffix() {
    let case = Case::new();
    let path = case.write("noextension", &noise_png(900, 900, Mode::Rgb));
    let (_, filename) = case.prepare(&path, 60 * 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(filename, "noextension.jpg");
}

// --- TestDownscaleFailureDegrades ----------------------------------------------------------

#[test]
fn raw_bytes_are_sent_when_downscaling_raises() {
    let case = Case::new();
    let raw = [b'z'; 4096];
    let path = case.write("broken.png", &raw);
    let (data, filename) = case
        .prep
        .prepare_with(
            &path,
            16,
            DEFAULT_MAX_DIMENSION,
            |p| std::fs::metadata(p).map(|m| m.len()),
            |_, _, _, _| Err("pretend the decoder gave up".to_owned()),
        )
        .unwrap();
    assert_eq!(&data[..], &raw[..]);
    assert_eq!(filename, "broken.png", "original name, original format");
}

#[test]
fn a_decoder_that_panics_is_not_fatal_either() {
    let case = Case::new();
    let raw = [b'z'; 4096];
    let path = case.write("broken.png", &raw);
    let (data, filename) = case
        .prep
        .prepare_with(
            &path,
            16,
            DEFAULT_MAX_DIMENSION,
            |p| std::fs::metadata(p).map(|m| m.len()),
            |_, _, _, _| panic!("decoder bug"),
        )
        .unwrap();
    assert_eq!((&data[..], filename.as_str()), (&raw[..], "broken.png"));
    assert!(case.prep.last_prepared().is_none());
}

#[test]
fn a_truncated_file_is_not_fatal() {
    // Not a stand-in: real bytes the decoder genuinely cannot open.
    let case = Case::new();
    let mut raw = b"\x89PNG\r\n\x1a\n".to_vec();
    raw.extend_from_slice(&[0u8; 4096]);
    let path = case.write("truncated.png", &raw);
    let (data, filename) = case.prepare(&path, 64, DEFAULT_MAX_DIMENSION);
    assert_eq!(&data[..], &raw[..]);
    assert_eq!(filename, "truncated.png");
}

#[test]
fn a_failed_downscale_does_not_cache_the_raw_bytes() {
    // One transient failure must not stick for every later engine asking about the same file.
    let case = Case::new();
    let path = case.write("broken.png", &[b'z'; 4096]);
    case.prep
        .prepare_with(
            &path,
            16,
            DEFAULT_MAX_DIMENSION,
            |p| std::fs::metadata(p).map(|m| m.len()),
            |_, _, _, _| Err("nope".to_owned()),
        )
        .unwrap();
    assert!(case.prep.last_prepared().is_none());
}

// --- TestPrepCache -------------------------------------------------------------------------

fn counting(
    calls: &Arc<AtomicUsize>,
    result: &'static str,
) -> impl FnOnce(&Path, &str, u64, u32) -> Result<Downscaled, String> {
    let calls = calls.clone();
    move |_, _, _, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok((result.as_bytes().to_vec(), "x.jpg".to_owned()))
    }
}

fn stat(p: &Path) -> io::Result<u64> {
    std::fs::metadata(p).map(|m| m.len())
}

#[test]
fn the_second_caller_gets_the_first_callers_work() {
    let case = Case::new();
    let path = case.write_png("big.png", (900, 900), Mode::Rgb);
    let cap = 60 * 1024;
    let (first, first_name) = case.prepare(&path, cap, DEFAULT_MAX_DIMENSION);
    let calls = Arc::new(AtomicUsize::new(0));
    let (second, second_name) = case
        .prep
        .prepare_with(
            &path,
            cap,
            DEFAULT_MAX_DIMENSION,
            stat,
            counting(&calls, "never"),
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        std::ptr::eq(first.as_ptr(), second.as_ptr()),
        "the same buffer, not a copy"
    );
    assert_eq!(second_name, first_name);
}

#[test]
fn a_different_cap_is_a_different_key() {
    // Reusing the memo would hand SauceNAO bytes prepared for IQDB's (different) cap.
    let case = Case::new();
    let path = case.write_png("big.png", (900, 900), Mode::Rgb);
    case.prepare(&path, 60 * 1024, DEFAULT_MAX_DIMENSION);
    let calls = Arc::new(AtomicUsize::new(0));
    let (data, _) = case
        .prep
        .prepare_with(
            &path,
            30 * 1024,
            DEFAULT_MAX_DIMENSION,
            stat,
            counting(&calls, "fresh"),
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(&data[..], b"fresh");
}

#[test]
fn a_different_max_dimension_is_a_different_key() {
    let case = Case::new();
    let path = case.write_png("big.png", (900, 900), Mode::Rgb);
    case.prepare(&path, 60 * 1024, 3000);
    let calls = Arc::new(AtomicUsize::new(0));
    case.prep
        .prepare_with(&path, 60 * 1024, 400, stat, counting(&calls, "fresh"))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_file_that_changed_size_is_re_prepared() {
    // Without the size in the key, a file edited between two engines' calls would upload the old
    // picture under the new one's name.
    let case = Case::new();
    let path = case.write("small.png", &[b'a'; 100]);
    let (first, _) = case.prepare(&path, 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(&first[..], &[b'a'; 100][..]);
    std::fs::write(&path, [b'b'; 200]).unwrap();
    let (second, _) = case.prepare(&path, 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(&second[..], &[b'b'; 200][..]);
}

#[test]
fn a_different_path_is_a_different_key() {
    let case = Case::new();
    let one = case.write("one.png", &[b'1'; 50]);
    let two = case.write("two.png", &[b'2'; 50]);
    case.prepare(&one, 1024, DEFAULT_MAX_DIMENSION);
    let (data, filename) = case.prepare(&two, 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(&data[..], &[b'2'; 50][..]);
    assert_eq!(filename, "two.png");
}

#[test]
fn only_the_most_recent_result_is_kept() {
    // Deliberately one entry, not an LRU: the access pattern is two calls for the same file,
    // seconds apart.
    let case = Case::new();
    let one = case.write("one.png", &[b'1'; 50]);
    let two = case.write("two.png", &[b'2'; 50]);
    case.prepare(&one, 1024, DEFAULT_MAX_DIMENSION);
    case.prepare(&two, 1024, DEFAULT_MAX_DIMENSION);
    assert_eq!(
        case.prep.last_prepared().unwrap().path,
        two.to_string_lossy()
    );
}

#[test]
fn concurrent_callers_decode_once() {
    // The engines run in parallel, which is what the lock is for: the first thread through fills
    // the memo and the rest hit it.
    let case = Arc::new(Case::new());
    let path = case.write_png("big.png", (900, 900), Mode::Rgb);
    let calls = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let (case, path, calls) = (case.clone(), path.clone(), calls.clone());
            std::thread::spawn(move || {
                case.prep
                    .prepare_with(
                        &path,
                        60 * 1024,
                        DEFAULT_MAX_DIMENSION,
                        stat,
                        move |p, n, b, d| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            downscale_to_fit(p, n, b, d)
                        },
                    )
                    .unwrap()
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the same file was decoded more than once"
    );
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|(data, _)| data == &results[0].0));
}

#[test]
fn the_cache_can_be_reset() {
    // Python: `test_the_module_global_is_reset_between_tests` proves its setUp clears the module
    // global. Here every test owns its cache; this is the reset the global one offers.
    let case = Case::new();
    let path = case.write("small.png", &[b'a'; 10]);
    case.prepare(&path, 1024, DEFAULT_MAX_DIMENSION);
    assert!(case.prep.last_prepared().is_some());
    case.prep.reset();
    assert!(case.prep.last_prepared().is_none());
}

// --- TestDefaults --------------------------------------------------------------------------

#[test]
fn the_default_cap_leaves_headroom_under_the_cited_8_mib_limit() {
    const { assert!(DEFAULT_MAX_BYTES < 8 * 1024 * 1024) };
    assert_eq!(DEFAULT_MAX_DIMENSION, 3000);
}

// --- the arithmetic ------------------------------------------------------------------------

#[test]
fn thumbnail_sizes_are_pillows() {
    // Reference values from Pillow 12.3 `Image.thumbnail((m, m), LANCZOS)`.
    for ((w, h, m), want) in [
        ((6000, 4000, 3000), (3000, 2000)),
        ((4000, 6000, 3000), (2000, 3000)),
        ((3001, 3001, 3000), (3000, 3000)),
        ((1200, 1200, 300), (300, 300)),
        ((1, 5000, 3000), (1, 3000)),
        ((5000, 1, 3000), (3000, 1)),
        ((7, 3001, 3000), (7, 3000)),
        ((3001, 7, 3000), (3000, 7)),
        ((2999, 3001, 3000), (2998, 3000)),
        ((4001, 4000, 3000), (3000, 2999)),
        ((1000, 3, 300), (300, 1)),
        ((333, 7777, 3000), (128, 3000)),
        ((100000, 3, 3000), (3000, 1)),
    ] {
        assert_eq!(
            thumbnail_size(w, h, (m, m)).unwrap(),
            Some(want),
            "{w}x{h} into {m}"
        );
    }
    // A box at least as large as the image leaves it alone.
    assert_eq!(thumbnail_size(300, 200, (300, 300)).unwrap(), None);
}

#[test]
fn a_zero_height_box_is_the_zero_division_python_raises() {
    assert!(thumbnail_size(10, 10, (5, 0)).is_err());
    // And a result with a zero side is Pillow's resize error: the file goes out unchanged.
    let mut px = pixels(1, 2000);
    assert!(px.thumbnail((0, 1600)).is_err());
}

#[test]
fn names() {
    assert_eq!(basename(Path::new("/a/b/c.png")), "c.png");
    assert_eq!(basename(Path::new("c.png")), "c.png");
    assert_eq!(basename(Path::new("/a/b/")), "");
    for (name, stem) in [
        ("big.png", "big"),
        ("a.b.c", "a.b"),
        ("noextension", "noextension"),
        (".hidden", ".hidden"),
        ("..png", "..png"),
        ("a.", "a"),
        ("", ""),
    ] {
        assert_eq!(splitext_stem(name), stem, "{name:?}");
    }
}

// --- Pillow's mode, read from the header ---------------------------------------------------

fn gif_head(flags: u8, table: &[u8]) -> Vec<u8> {
    let mut head = b"GIF89a".to_vec();
    head.extend_from_slice(&[4, 0, 4, 0, flags, 0, 0]);
    head.extend_from_slice(table);
    head
}

fn bmp_head(bits: u16, colors_used: u32, palette: &[[u8; 3]]) -> Vec<u8> {
    let mut head = b"BM".to_vec();
    head.resize(14, 0);
    head.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    head.resize(28, 0);
    head.extend_from_slice(&bits.to_le_bytes());
    head.resize(46, 0);
    head.extend_from_slice(&colors_used.to_le_bytes());
    head.resize(54, 0);
    for rgb in palette {
        head.extend_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
    }
    head
}

#[test]
fn a_grey_ramp_palette_is_pillows_mode_l_and_a_bilevel_png_is_rgb() {
    let ramp: Vec<u8> = (0..=255u8).flat_map(|i| [i, i, i]).collect();
    let mut off_by_one = ramp.clone();
    off_by_one[3 * 7] = 8;
    assert_eq!(mode_from_header(&gif_head(0xF7, &ramp)), PillowMode::Grey);
    assert_eq!(
        mode_from_header(&gif_head(0xF7, &off_by_one)),
        PillowMode::AsDecoded
    );
    assert_eq!(
        mode_from_header(&gif_head(0x00, &[])),
        PillowMode::AsDecoded,
        "no global table"
    );

    let greys: Vec<[u8; 3]> = (0..=255u8).map(|i| [i, i, i]).collect();
    assert_eq!(mode_from_header(&bmp_head(8, 0, &greys)), PillowMode::Grey);
    assert_eq!(
        mode_from_header(&bmp_head(8, 16, &greys[..16])),
        PillowMode::Grey
    );
    let mut tinted = greys.clone();
    tinted[3] = [3, 3, 4];
    assert_eq!(
        mode_from_header(&bmp_head(8, 0, &tinted)),
        PillowMode::AsDecoded
    );
    // Two entries, black and white: mode `1`, converted to RGB. A 24-bit BMP has no palette.
    assert_eq!(
        mode_from_header(&bmp_head(1, 2, &[[0, 0, 0], [255, 255, 255]])),
        PillowMode::Rgb
    );
    assert_eq!(
        mode_from_header(&bmp_head(24, 0, &[])),
        PillowMode::AsDecoded
    );

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(24, 0);
    png.extend_from_slice(&[1, 0]); // 1-bit grey
    assert_eq!(mode_from_header(&png), PillowMode::Rgb);
    png[24..26].copy_from_slice(&[8, 0]); // 8-bit grey stays L
    assert_eq!(mode_from_header(&png), PillowMode::AsDecoded);
    assert_eq!(mode_from_header(b"not an image"), PillowMode::AsDecoded);
    assert_eq!(mode_from_header(b""), PillowMode::AsDecoded);
}
