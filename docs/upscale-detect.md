# upscale_detect (Phase 4.e, DAN-2127)

`core/upscale_detect.py` and the logic of `workers/upscale_check_worker.py` at `76c85c4` (Pillow
12.3.0, numpy 2.5.3 in the black-box venv), ported to `crates/hatate-core/src/upscale_detect.rs`,
with `pymeta.rs` (what Pillow keeps in `Image.info`) and `pynumpy.rs` (numpy's `float32` reductions).

## 1. The cut

| Python | Rust |
|---|---|
| `SourceComparisonResult`, `SelfConsistencyResult`, `UpscaleCheckResult`, `compare_to_source`, `_detail_mask`, `_masked_mean_abs_diff`, `detect_naive_upscale`, `check_upscaler_metadata`, every constant and `KNOWN_AI_UPSCALER_SIGNATURES` | `upscale_detect::{SourceComparisonResult, SelfConsistencyResult, UpscaleCheckResult, compare_to_source, detail_mask, masked_mean_abs_diff, detect_naive_upscale, check_upscaler_metadata}` and the same constants |
| `UpscaleCheckWorker.run`, without Qt | `check_entry` and `check_entries(entries, &dyn Checks, on_checked)`; `Checks` stands in for the four calls the Python tests `patch`, `Real` is the shipped behaviour; `on_checked` is `entry_checked`, the return is `finished_all` |
| Pillow: `resize` with `NEAREST`, `BILINEAR`, `BICUBIC`, `LANCZOS`; `Image.open(..).size`; `draft`; `getexif` | `pyraster::Image::resize_filtered` and the existing `open`, `pillow_size`; `pymeta` |
| numpy: `gradient`, `percentile`, boolean masks, `mean` of `float32` (pairwise sum) | `pynumpy::{percentile_f32, mean_f32, sum_f32}` and `f32` operators |

Not here: the Qt thread, the signals and the right-click menu (Phase 5); the call site that stores
`upscale_verdict` and `upscale_check_detail` on an entry.

`check_entry` returns `Err(DecompressionBomb)` where Python lets `DecompressionBombError` end the
batch (it is neither `OSError` nor `ValueError`); every other failure is the `message` field, in
Python's wording where it can be (`[Errno 2] No such file or directory: '<path>'`,
`cannot identify image file '<path>'`, numpy's "Shape of array too small to calculate a numerical
gradient" for a picture that shrinks to one pixel wide).

`check_upscaler_metadata` finds what Pillow puts in `Image.info` as `str` or `bytes` (PNG `tEXt`,
`zTXt`, `iTXt`, `eXIf`, `iCCP`, JPEG `COM`, Exif, XMP, ICC, FlashPix, MPF, GIF first-frame comments,
WebP `ICCP`/`EXIF`/`XMP ` behind the `VP8X` flags, TIFF XMP and ICC), then EXIF tags 270, 305 and
315 from `getexif()` (PNG `eXIf` before or after the pixel data, "Raw profile type exif", JPEG, WebP,
TIFF). Pillow's checks that make `Image.open` fail (a wrong chunk checksum, a bad marker, a text
chunk past 64 MiB or 1 MiB decompressed) make the scan find nothing here too. BMP, ICO and PNM carry
no text in `info`. BigTIFF EXIF is not read by Pillow either (an 8-byte head cannot hold the offset).
New dependencies, already in the lockfile through `png`, so no new crate is built: `flate2` (zlib
inflate of text and ICC chunks) and `crc32fast` (PNG chunk checksums).

## 2. How it was checked against the real Python

Direct differential, the same files through the real Python functions and through the Rust ones;
harness outside the repo (`worktrees/DAN-2127-tools`), log `worktrees/DAN-2127-differential.log`.
Compared: `best_factor`, `best_diff`, `detail_fraction` (bit-equal `f64`), `best_filter`,
`confidence`, `message`, and the metadata result, or the exception.

| Set | Files | Differing |
|---|---|---|
| `gen_up.py` seeds 11, 12, 13 (240 each): genuine upscales with each of four filters at 1.5x to 4x, native noisy and flat art, gradients, 2000+ px JPEG and PNG (draft and Lanczos paths), under 64 px and extreme shapes, modes L, P, RGBA, LA, I;16, 1, CMYK JPEG, GIF, WebP, BMP, TIFF, missing files and directories, damaged files | 720 (51 damaged on purpose) | 9, all damaged files (class D below) |
| `gen_meta.py` seeds 5, 6, 7 (240 each): a tool name planted in 24 places (every text chunk type, eXIf before and after the pixel data, "Raw profile type exif", JPEG COM/Exif/XMP/ICC, WebP, GIF, TIFF, split over two values, upper case, Kelvin sign), negative controls, bad checksums, truncations | 720 (60 damaged on purpose) | **0** |
| `gen_dmg.py` seeds 3, 4 (60 per format): PNG, JPEG, GIF, WebP, BMP truncated, bit-flipped, with bytes inserted, deleted or zeroed | 600 | 198, class D below |
| **Total** | **2040** (1329 well-formed, 711 damaged on purpose) | **207, every one in a damaged file; 0 in the 1329 well-formed files** |

### Differences that are not matched (damaged files only)

The decoders are not Pillow's: `pyjpeg` is a libjpeg-turbo port (Phase 3.g), the others are the
`image` crate. A damaged file therefore fails or decodes differently, and the failure text is the
crate's. `pillow_decode_message` translates the cases whose Pillow wording does not depend on
decoder buffering (PNG "unexpected end of file" to "image file is truncated", an unknown filter
method to "unrecognized data stream contents", deflate and checksum errors to "broken data stream",
GIF "invalid code", WebP "Corrupt bitstream" to "failed to read next frame"); those match.

| Class | What | Count over the 711 damaged files |
|---|---|---|
| **D1** both fail, `message` text differs | 108 Pillow says `image file is truncated (N bytes not processed)`, where N is the number of bytes Pillow's decoder had not yet consumed (it depends on Pillow's read buffering, the libjpeg suspension point or a raw decoder's row stride); 51 WebP files whose RIFF header is intact and whose bitstream libwebp refuses ("could not create decoder object") against `cannot identify image file`; 29 files cut inside a PNG chunk or JPEG segment ("Truncated File Read") against `cannot identify image file`; 8 other wording (7 "broken data stream", 1 "unrecognized data stream contents" where the crate's error maps differently). Per format: JPEG 54, BMP 53, WebP 51, GIF 27, PNG 11 | 196 |
| **D2a** Pillow decodes, Rust refuses | PNG with a flipped byte in the pixel data (Pillow does not check the IDAT checksum, the crate does): 1; a JPEG: 1; BMP with bytes deleted, which Pillow reads as a shorter picture: 3. Rust reports "Could not analyze image", the safe direction | 5 |
| **D2c** both decode, numbers differ | damaged JPEG entropy data past libjpeg-turbo's 16-bit lanes (class C2 of `docs/image-compare.md`): 5; one lossy WebP with a flipped byte | 6 |

None of D1, D2a or D2c occurs in a well-formed file. What each means for the product: the text of a
"Could not analyze image: ..." line differs for a broken file (the verdict is the same: nothing
was analysed); for 5 of 711 broken files Python analyses a garbled picture where Rust declines; for
6 both analyse and the figures differ slightly. Cloud ruled on DAN-2137 (2026-10-23): D1, D2a and
D2c are accepted as documented classes, on the condition that a well-formed file never differs
(0 of 1329 here; 0 of 2049 counting the 720 image_diff pairs) and that the D2a and D2c cases are
listed one per row below.

### The 11 D2a and D2c cases, one per row

Generator `gen_dmg.py OUT SEED 60` (set `dmg1` is seed 3, set `dmg4` is seed 4); the file is
`OUT/<file>`, the damage is the generator's `kind` (`<format>-<damage>`). Python is Pillow 12.3.0
and numpy 2.5.3 at `76c85c4`. Floats are shown to the digits that matter; the log compares them
bit for bit. A field not listed is bit-equal. "Verdict" is `confidence` (and `best_filter`):
it changed in the 5 rows marked **yes**.

| # | Class | Set, file, damage | Differing fields (Python / Rust) | `confidence` / `best_filter` changed |
|---|---|---|---|---|
| 1 | D2a | `dmg1` seed 3, `d0021.png`, png-flip | Python decodes (IDAT checksum not checked), Rust refuses ("broken data stream when reading image file"). `best_factor` 1.5 / none, `best_diff` 2.6617 / none, `detail_fraction` 0.28862 / none, `best_filter` Bicubic / none, `confidence` likely / none | **yes**: likely to none |
| 2 | D2a | `dmg1` seed 3, `d0092.jpg`, jpg-flip | Python decodes a truncated entropy stream and reports "almost entirely flat/simple content"; Rust refuses ("image file is truncated"). `detail_fraction` 0.013619 / none | no: Python `confidence` is already none, so is Rust's |
| 3 | D2a | `dmg4` seed 4, `d0245.bmp`, bmp-delete | Python reads a shorter picture, Rust refuses ("failed to fill whole buffer"). `best_factor` 1.5 / none, `best_diff` 1.8721 / none, `detail_fraction` 0.29114 / none, `best_filter` Bicubic / none, `confidence` likely / none | **yes**: likely to none |
| 4 | D2a | `dmg4` seed 4, `d0251.bmp`, bmp-delete | same shape. `best_diff` 1.1891 / none, `detail_fraction` 0.28962 / none, `best_factor` 1.5 / none, `best_filter` Bicubic / none, `confidence` likely / none | **yes**: likely to none |
| 5 | D2a | `dmg4` seed 4, `d0273.bmp`, bmp-delete | same shape. `best_diff` 2.3426 / none, `detail_fraction` 0.29089 / none, `best_factor` 1.5 / none, `best_filter` Bicubic / none, `confidence` likely / none | **yes**: likely to none |
| 6 | D2c | `dmg1` seed 3, `d0085.jpg`, jpg-zero | `best_diff` 34.957 / 1.7790, `detail_fraction` 0.27124 / 0.25338, `confidence` none / likely, message "No strong signal" / "Likely upscaled ~1.5x using Lanczos". `best_factor` and `best_filter` equal (1.5, Lanczos) | **yes**: none to likely. A zeroed run in the entropy data decodes to different garbage; this is the one row where the verdict flips with both sides analysing |
| 7 | D2c | `dmg1` seed 3, `d0115.jpg`, jpg-flip | `best_diff` 30.338 / 30.277, `detail_fraction` 0.214067 / 0.213567 | no: both none, same factor and filter |
| 8 | D2c | `dmg1` seed 3, `d0209.webp`, webp-flip | `best_diff` 7.0578 / 7.0340, `detail_fraction` 0.241927 / 0.240104 (lossy WebP, one flipped byte) | no: both "Possibly upscaled ~1.5x using Lanczos" |
| 9 | D2c | `dmg4` seed 4, `d0072.jpg`, jpg-flip | `best_diff` 6.2326 / 6.3253, `detail_fraction` 0.2511 / 0.251267 | no: both "Possibly upscaled ~1.5x using Lanczos" |
| 10 | D2c | `dmg4` seed 4, `d0087.jpg`, jpg-insert | `best_diff` 8.8558 / 9.7462, `detail_fraction` 0.202367 / 0.201657 | no: both "Possibly upscaled ~1.5x using Lanczos" |
| 11 | D2c | `dmg4` seed 4, `d0108.jpg`, jpg-insert | `best_diff` 1.6546 / 1.4536, `detail_fraction` 0.288429 / 0.287952 | no: both "Likely upscaled ~1.5x using Lanczos" |

Rows 1 to 5 are the safe direction (Rust declines, nothing is claimed about the picture). Row 6 is
the only one where a verdict differs and both sides analyse; it is a garbled file in both cases.
`best_factor` and `best_filter` never change in rows 2 and 7 to 11, and change in rows 1, 3, 4, 5
only because Rust has no result at all.

## 3. Mutation check

`worktrees/DAN-2127-mutation.log`; each mutation applied, the named binary run, reverted.

| Mutation | Red |
|---|---|
| `STRONG_MATCH_THRESHOLD` 4.0 to 3.0 | `the_self_consistency_result_matches_pillow_and_numpy` |
| `DETAIL_PERCENTILE` 70 to 60 | `the_self_consistency_result_matches_pillow_and_numpy` |
| `SOURCE_COMPARISON_THRESHOLD_PERCENT` 15 to 16 | `the_shipped_threshold_is_fifteen_percent_gain` |
| the first two filters of `CANDIDATE_FILTERS` swapped (tie order) | `the_self_consistency_result_matches_pillow_and_numpy` |
| bicubic `a` -0.5 to -0.75 | `resize_matches_pillow_for_every_filter_down_up_and_skewed`, `the_self_consistency_result_matches_pillow_and_numpy` |
| nearest-neighbour pixel centre dropped | `resize_matches_pillow_for_every_filter_down_up_and_skewed`, `the_self_consistency_result_matches_pillow_and_numpy` |
| numpy pairwise-sum block 128 to 64 | `pynumpy::tests::matches_numpy_bit_for_bit_on_arrays_of_every_length_class`, `the_self_consistency_result_matches_pillow_and_numpy` |
| percentile index uses `n` instead of `n - 1` | `pynumpy::tests::percentile_interpolates_between_order_statistics` |
| a known tool name dropped from the list (`upscayl`) | `png_ztxt_and_itxt_and_a_late_eximf_are_all_read` |
| trailing `eXIf` not read | `png_ztxt_and_itxt_and_a_late_eximf_are_all_read` |
| PNG chunk checksum not checked in `pymeta` | **survives**: `pyimage::png`, which `pillow_size` runs first, rejects a wrong checksum before `pymeta` is reached; the second check is defence in depth |

## 4. Python test mapping

All 22 Python tests that exercise a function in scope map to a Rust test of the same name in
`upscale_detect_tests.rs`.

| Python test | Rust test |
|---|---|
| `test_upscale_detect.py::TestSourceComparison::test_flags_clearly_larger_local_file` | same name |
| `...::test_ignores_minor_size_difference` | same name |
| `...::test_smaller_local_is_never_flagged` | same name |
| `...::test_missing_source_dimensions_is_safe` | same name |
| `...::TestSelfConsistency::test_detects_a_genuinely_upscaled_image` | same name |
| `...::test_native_detail_is_not_flagged_as_likely` | same name |
| `...::test_flat_heavy_art_is_not_a_false_positive` | same name (the picture is drawn with `common::art`) |
| `...::test_difference_is_within_valid_range` | same name |
| `...::test_oversized_image_is_skipped_not_attempted` | same name, and it also runs a 7000x7000 header (the Python test only asserts the constant) |
| `...::TestUpscalerMetadata::test_detects_known_tool_in_png_metadata` | same name |
| `...::test_unrelated_software_is_not_flagged` | same name |
| `...::test_clean_image_is_not_flagged` | same name |
| `test_workers_background.py::TestUpscaleCheckWorker::test_one_result_is_emitted_per_entry_and_then_finished_all` | same name |
| `...::test_an_empty_batch_still_says_it_finished` | same name |
| `...::test_missing_local_dimensions_are_read_off_the_file_and_kept` | same name |
| `...::test_already_known_dimensions_are_not_re_read_from_disk` | same name |
| `...::test_an_unreadable_file_is_logged_and_the_batch_carries_on` | same name |
| `...::test_the_source_comparison_runs_only_when_both_sizes_are_known` | same name |
| `...::test_a_candidate_that_reports_no_dimensions_skips_the_comparison` | same name (four shapes of "no dimensions") |
| `...::test_no_selected_candidate_skips_the_comparison_but_not_the_rest` | same name |
| `...::test_unreadable_dimensions_do_not_stop_the_other_two_heuristics` | same name |
| `...::test_all_three_heuristics_land_in_the_one_emitted_result` | same name |

The worker tests `patch` `Image.open` and the three heuristics; the Rust ones substitute `Checks`.
Beyond the Python tests: metadata in every place Pillow keeps text, the messages and cut-offs, the
numbers pinned to Pillow and numpy (`the_self_consistency_result_matches_pillow_and_numpy`), and the
Pillow-pinned resize and blur digests in `pyraster_filters.rs`.

## 5. Changes to earlier code

| Where | What | Why |
|---|---|---|
| `pyraster` | `Filter`, `resize_filtered`, `gaussian_blur`; `pub use DecompressionBomb` | resize filters and the blur; the bomb type is in `Checks`'s signature |
| `Cargo.toml` | `crc32fast`, `flate2` | see section 1 |
| `error.rs` | new, shared by the Phase 4 tickets | DAN-2116 item 1 |
