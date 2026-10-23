# image_diff (Phase 4.e, DAN-2127)

`core/image_diff.py` at `76c85c4` (Pillow 12.3.0, numpy 2.5.3 in the black-box venv), ported to
`crates/hatate-core/src/image_diff.rs`. It answers "where do these two copies of a picture differ":
both pictures are scaled to one working size (long edge at most 900, never scaled up), levelled per
channel on the median, differenced on the strongest channel, blurred, thresholded. The compare
dialog (Phase 5) draws the result.

## 1. The cut

| Python | Rust |
|---|---|
| `DifferenceMap` (`mask`, `changed_fraction`, `verdict`, `reliable`, `changed_percent`), `difference_map`, `_prepare`, `_working_size`, `_verdict`, `WORK_LONG_EDGE`, `NOISE_BLUR_RADIUS`, `CHANGE_THRESHOLD`, `WHOLESALE_FRACTION`, `ASPECT_TOLERANCE` | `image_diff::Diff`, `difference_map`, `difference_map_images`, `verdict` and the same constants |
| Pillow as `image_diff` uses it: `convert("RGB")`, `resize(.., LANCZOS)`, `ImageFilter.GaussianBlur(2.0)` | `pyraster::Image::{to_rgb, resize_filtered, gaussian_blur}` (Pillow's three box-blur passes of `BoxBlur.c`, ported) |
| numpy as `image_diff` uses it: `np.median` of `float32`, `float32` arithmetic, `astype(uint8)` | `pynumpy::median_f32` and plain `f32` operators |

Not here: the compare dialog and its wipe view (Phase 5), and where the dialog is wired to a worker.

## 2. What is added to Python's output

`difference_map(local, remote) -> Result<Diff, DiffError>` keeps every field of `DifferenceMap` equal to
Python's and returns, additively:

* `delta: Vec<u8>`: one byte per pixel, row-major, at the working size (`width`, `height`): the
  levelled, blurred strongest-channel difference before the threshold. `mask` is non-zero exactly
  where `delta >= CHANGE_THRESHOLD`.
  **The ticket's data-contract line said "luminance". Python's difference is not luminance:
  `_prepare` keeps colour (a recolour at equal brightness must show) and the difference is the
  strongest channel's. `delta` is that, so the compare dialog sees what Python thresholds. Cloud
  confirmed this reading on DAN-2137 and it replaces the contract wording for the Phase 6
  consumers; the invariant `mask == (delta >= CHANGE_THRESHOLD)` is tested
  (`delta_is_the_mask_before_the_threshold`).**
* `local_rect`, `match_rect: Option<Rect {x, y, w, h}>`: the bounding box of the non-zero mask pixels,
  rounded outwards into each picture's own pixel coordinates (they differ when the pictures differ
  in size); `None` when nothing changed.

`difference_map_images(None, _)` and a zero-sized local picture give Python's
`DifferenceMap(None, 0.0, "Nothing to compare", False)`.

## 3. Error shape (DAN-2116 item 1)

`DiffError` has `kind() -> error::ErrorKind`, `user_message()` and `detail()`. `crates/hatate-core/src/error.rs`
is created here (`ErrorKind` with `ErrorKind::Net(net::ErrorKind)` and the image kinds `ImageUnreadable`,
`ImageUndecodable`, `ImageTooLarge`; Phase 4.a adds its own variants to the same enum). `user_message` names which picture
("local" or "matched") and what to do, and never a path or operating-system text; `detail` has the
raw text. Pinned by `image_diff_tests::a_missing_file_gives_a_message_with_no_path_and_no_os_text`.
Nothing on disk changes (`Entry.error_message` and `MatchStatus` are untouched).

## 4. How it was checked against the real Python

Direct differential, the same image files through `core/image_diff.py` (its `core/` copied out of
`76c85c4`) and through `difference_map`. Harness outside the repo (`worktrees/DAN-2127-tools`), log
`worktrees/DAN-2127-differential.log`.

| Family | Cases | Compared | Differing |
|---|---|---|---|
| generated pairs (seeds 1, 2, 3; `gen_pairs.py OUT SEED 240`): JPEG re-encodes at quality 60 to 95, resized copies (Lanczos, bilinear, nearest, 0.33x to 2x), crops, added caption or watermark, different scenes, mismatched aspects, tiny images (1x1 to 12x12), recolours, flat art, noisy pictures, palette GIF, RGBA to WebP, greyscale; sizes 1x1 to 4000x3000 | 720 (3 x 240) | `mask` (sha256 of the whole `L` plane), mask size and mode, `changed_fraction` and `changed_percent` (bit-equal `f64`), `verdict`, `reliable`, or the exception class and message | **0** |

The additive fields have no Python counterpart and are tested directly (`image_diff_tests.rs`: the
box of a known caption, `None` on identical pictures, `delta` equal to `mask` under the threshold, the
two boxes in each picture's own pixels, a box never past the picture).

The two things that took work to match, found by the first runs and fixed before the figures above:
Pillow's blur (`ImagingGaussianBlur` is three 8-bit box-blur passes with a fractional radius, not a
Gaussian; the `float`/`double` mix of `_gaussian_blur_radius` matters) and numpy's `float32`
`median` (mean of the two middle values in `float32`).

## 5. Mutation check

`worktrees/DAN-2127-mutation.log`; each mutation applied, the named binary run, reverted.

| Mutation | Red |
|---|---|
| `CHANGE_THRESHOLD` 26 to 27 | `the_result_matches_pillow_and_numpy_on_the_fixtures` |
| `WHOLESALE_FRACTION` 0.45 to 0.5 | `the_verdict_bands_and_their_boundaries`, `test_a_wholesale_difference_declines_to_explain_itself` |
| level matching switched off | `the_result_matches_pillow_and_numpy_on_the_fixtures` |
| `reliable` ignores the aspect check (a result field) | `test_different_shapes_are_refused_rather_than_distorted` |
| box blur 3 passes to 2 | `gaussian_blur_radius_two_matches_pillow_including_one_pixel_lines`, `the_result_matches_pillow_and_numpy_on_the_fixtures` |
| median of an even count takes the lower middle | `pynumpy::tests::median_even_is_the_mean_of_the_two_middle_values`, `the_result_matches_pillow_and_numpy_on_the_fixtures` |

## 6. Python test mapping

Every test in `TestImageDifferenceMap` (`tests/test_core_utils.py` at `76c85c4`, 12 tests) maps to the
Rust test of the same name in `image_diff_tests.rs`. The pictures are drawn with `common::art`, not
Pillow's `ImageDraw`, so each test checks the property its Python original checks.

| Python test | Rust test |
|---|---|
| `test_a_picture_against_itself_shows_nothing` | same name |
| `test_a_resize_and_re_encode_is_not_a_difference` | same name (JPEG q70 at 2x, reopened through `pyraster::open`) |
| `test_a_watermark_is_found` | same name |
| `test_the_mask_marks_where_the_change_is` | same name |
| `test_a_wholesale_difference_declines_to_explain_itself` | same name |
| `test_different_shapes_are_refused_rather_than_distorted` | same name |
| `test_a_global_brightness_shift_is_not_an_edit` | same name |
| `test_a_recolour_at_the_same_brightness_is_found` | same name |
| `test_the_strongest_channel_decides_not_the_average` | same name |
| `test_a_saturation_shift_is_not_an_edit` | same name |
| `test_a_missing_image_is_handled` | same name |
| `test_a_small_image_is_not_upscaled` | same name |

Pillow's own output is pinned separately: `the_result_matches_pillow_and_numpy_on_the_fixtures` (12
pairs from the repo's fixtures), `pyraster_filters.rs` (96 resizes with the four filters on six
shapes including 1-pixel-wide and 300:5, and 10 blurs including 1x1 and 1x30; digests computed by
Pillow), `pynumpy::tests::matches_numpy_bit_for_bit_on_arrays_of_every_length_class`.

## 7. Changes to earlier code

| Where | What | Why |
|---|---|---|
| `pyraster` | additive: `Filter`, `Image::resize_filtered`, `Image::gaussian_blur`, `pub use DecompressionBomb`; `resample_axis` takes the filter (Lanczos unchanged) | Bicubic, Bilinear and Nearest resize, and the blur |
| `image_compare.rs` | **not touched** | differential-tested against Pillow (`docs/image-compare.md`) |
