# image_compare, multipage page picking and the similarity check (Phase 3.g, DAN-2062)

What this ticket ports, where the cut is, how it was checked against the real Python, the classes
that are not matched, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4` (Pillow 12.3.0, libjpeg-turbo 3.1.4.1 in the black-box
venv). The Phase 1 net layer (`docs/net.md`), the 3.b fetch half (`docs/network-halves.md`) and the
Phase 2 pure halves (`docs/parsers.md`) are reused as they are.

## 1. The cut

| Python | Rust |
|---|---|
| `core/image_compare.py`, all of it: `dhash`, `hamming_distance`, `image_dimensions`, `perceptual_similarity`, `describe_perceptual_match`, `trim_border`, `local_prints`, `compare_to_local`, `aligned_similarity`, `compare_sizes`, `compact_size_delta`, `size_delta_sort_key`, `compare_file_sizes` | `image_compare` |
| `core/multipage.py` (page picking by thumbnail hash, `check_page`, `MAX_PAGES_TO_COMPARE`) | `multipage` |
| `core/multipage_resolve.py` (the network half: the page list, the thumbnails, the pick) | `multipage_resolve` (built on `remote::fetch_text` / `download_bytes` and the parsers' `pages_api_url` / `parse_pages`) |
| `core/similarity_check.py` (`measure_ordinal_similarities`, the borrowed-measurement recheck) | `similarity_check` |
| Pillow, as far as these modules use it: `Image.open` + `load`, `draft`, `convert("L")`, `crop`, `ImageOps.mirror`, `resize` / `thumbnail` / `reduce` with `LANCZOS` | `pyraster` (raster), `pyjpeg` (the JPEG decoder), and `pypng` / `pyinflate` (the pixel stream of a PNG as Pillow reads it) |

Why a port of Pillow and not the `image` crate: a perceptual hash compares neighbouring pixels, and
one bit decides a page choice. Measured on generated JPEGs, the `image` crate's decoder differs
from Pillow's in 1% to 30% of samples by 1 to 37 grey levels, so `pyjpeg` is a port of the libjpeg-turbo
decoder Pillow ships (baseline and progressive Huffman, restart intervals, the accurate integer
IDCT with the SIMD saturation, the 4x4 / 2x2 / 1x1 reduced IDCTs behind `Image.draft`, fancy
upsampling, YCbCr / YCCK / CMYK), and `pyraster` ports Pillow's resampling (Lanczos with the box and
`reducing_gap` arithmetic: the `f32` box, `reduce`'s per-block rounding, the horizontal-first
rule with its `in_h > 100 * in_w` exception). No new dependency; `fast_image_resize` and
`image_hasher` are not used for these.

Not here, and who owns it: the engine clients' `find_post_by_search` for rule34 (listing request,
thumbnail downloads, a 16x16 dhash comparison) is the engine client's, as `boorus/rule34.rs` says;
the verdict table of the Hydrus duplicate relationships is `hydrus_import` (Phase 4); the search
worker that calls the three modules is 3.h.

No `mocked_transport` corpus item exercises a function in this ticket's scope (checked in
`corpus/MANIFEST.json`), so there is no corpus replay to add. The Phase 2 corpus runner is
unchanged and has no skip list.

## 2. Changes to earlier code

| Where | What | Why |
|---|---|---|
| `model::Candidate` | two session-only fields, `thumb_bytes: Option<Vec<u8>>` and `page_index: i64` (both `#[serde(skip)]`, defaults `None` and `0`) | `MatchCandidate.thumb_bytes` / `.page_index`, which `multipage_resolve` and `similarity_check` read and write; `_candidate_to_dict` does not store them, so no session file changes |
| `availability::run_bounded`, `boorus::host::hostname`, `boorus::sizes::digit_value` | `pub(crate)` instead of private | reused, not copied |
| `.gitattributes` | `-text` for `crates/hatate-core/tests/data/image_compare/**` | the fixtures are binary |

## 3. How it was checked against the real Python

The real Python at `76c85c4` (its `core/` copied out byte for byte) and Pillow 12.3.0 ran in the
black-box venv; the harness lives outside the repo (Rust only tree), its log is
`worktrees/DAN-2062-differential.log`. Every comparison is exact: no tolerance on any hash, size
or score.

| Family | Cases | Compared | Differing |
|---|---|---|---|
| `image_compare` over generated pictures (sizes 1 to 4000 px; modes L, RGB, RGBA, P; PNG, GIF, BMP, WebP, TIFF, PPM, ICO, JPEG; EXIF-rotated JPEGs; the corpus thumbnails) | 12 247 (3068 + 3091 + 3020 + 3068 over four seeds) | `dhash` at sizes 8 (path) and 16 (bytes), `image_dimensions`, `local_prints` (which runs `trim_border`), `compare_to_local`, `aligned_similarity`, and the exception class where Python raises | **0** |
| the pure functions of the three modules (`perceptual_similarity`, `describe_perceptual_match`, `compare_file_sizes`, `page_index_from_thumbnail`, `dimensions_identify_page`, `dimensions_contradict_page`, `page_url_for_index`) over three seeds | 19 677 (6565 + 6550 + 6562) | return value or error class and message | **0** |
| `multipage_resolve` and `similarity_check` over a scripted transport: Python's `requests`/`core.net` scripted, the Rust side served by a local https server | 1 320 (resolve 869, measure 301, recheck 150) | return value or error; every request (method, URL with query, explicit headers, cookies); every sleep; the candidate fields and the module state afterwards | **0** |
| the JPEG decoder alone (`Image.open` + `load` at every `draft` scale) over valid and scaled files | 1 429 checks | decoded bytes, size, mode | **26** (all arithmetic coded: class C1) |
| the JPEG decoder over damaged files (flipped bytes, inserted bytes, dropped bytes, truncations, flipped header bytes) | 968 checks (242 files at 4 draft scales) | decoded bytes, or the error class | **43**: classes C1, C2, C3 below |

Every difference the first runs showed was matched; those that are left are the classes in
section 4. Each match was a hash difference on a real file: the standard Huffman tables for a Motion
JPEG frame, an EOI that is not there, the SIMD IDCT saturating where C code wraps, `reduce` rounding
with `floor` in `f32`, the f32 box in `resize`.

### 3.1 PNG pixel streams (DAN-2160)

`pyraster::open` used to hand a PNG to the `png` crate, which differs from Pillow's `PngImageFile`
wherever a file is damaged: it checks the CRC of every `IDAT`, ignores the adler32, and gives a
different answer for a stream that ends early. Found by the Phase 4.c differential
(`hydrus-import.md` section 4, class C8: Pillow hashes a PNG whose pixel chunk has a bad CRC,
`pyraster` refused it, so no duplicate verdict was written). Pillow does this, and `pypng` /
`pyinflate` now do the same before the `image` crate sees the file:

| Pillow | Where |
|---|---|
| the CRC of a pixel chunk is skipped unread | `pypng::pixel_data` |
| the payloads of the consecutive `IDAT` (and `fdAT`) chunks are one zlib stream, fed 65 536 bytes at a time | `pypng::pixel_data` |
| `ZipDecode` calls zlib's `inflate` once per row, and only while input is left: a stream whose input is used up before the last row is `OSError: image file is truncated (0 bytes not processed)` even if zlib could still produce the rest | `pyinflate` (an exact port of zlib's `inflate`: input taken one byte at a time, the next symbol decoded before the room is looked at, a pending match carried to the next row; `miniz_oxide` and `fdeflate` read ahead and cannot tell) |
| the adler32 is checked only when it is in the block that completes the last row | `pyinflate::St::Check` |
| a stream that ends exactly on a row boundary is a picture whose remaining rows are zero; one that ends inside a row is `image file is truncated` | `pypng::inflate_rows` |
| a filter type past 4 is `unrecognized data stream contents when reading image file`; any zlib error is `broken data stream when reading image file` | `pypng::inflate_rows` |
| `load_end` walks the chunks after the pixels, reads each body to its end and raises `Truncated File Read` for one the file cuts short, before the decoder's own error is raised; checksums there are not looked at | `pypng::trailing_chunks_complete` |
| what `load_read` says when it is asked for more and there is none: `image file is truncated` (no header left), `(0 bytes not processed)` (a non-pixel chunk follows), `broken PNG file (chunk b'...')` (no chunk id) | `pypng::pixel_data` |

`pyraster::open_other` re-states what Pillow got (the chunks before the first `IDAT` verbatim, one
`IDAT` with exactly the rows, `IEND`) and the `image` crate decodes that, so every colour type, bit
depth and Adam7 mapping is unchanged. `hydrus_import::validate_image_decodes` also runs
`Image.verify()` for a PNG, between the open and the load as Python does (`pypng::verify`: every
chunk from the first `IDAT` to `IEND`, checksums included), because Python's validator does.

Checked against Pillow 12.3.0 on files damaged in the ways above (a bad CRC on one or every
`IDAT`, bit flips in the pixel data with the CRC stale or recomputed, a damaged zlib header or
adler32, a stream cut at 20 / 50 / 80 / 95 %, a stream re-encoded to end early on or inside a row,
a pixel stream re-chunked into 1 to 40 pieces with empty ones, bytes after `IEND`, a missing
`IEND`, a text chunk with a bad CRC or cut short after the pixels, Adam7 files, every colour type
and bit depth): **20 020 files** (1 020 from the eleven `tests/data` PNGs, and 19 000 random over
five seeds), `worktrees/DAN-2160-pillow-probe.log`. Pillow decodes or raises the same way for every
one, and every decoded file has the same size and the same grey and RGB bytes. Before: of the 1 020,
174 decoded in Pillow and were refused here, 102 raised in Pillow and decoded here. The words of the
exception agree except where the damage is before the pixel data (`cannot identify image file`,
`Truncated IHDR chunk`, a PNG with no `IDAT`: 448 files, the text of the `image` crate or of the
header reader, never a different decision). Tests: `pyraster_png_damage.rs` (3 tests; 25 damaged
files and 11 validator files, each expected value checked against Pillow, red before the fix).

Not matched, because Python itself does not raise an `OSError` there: an exception that is not
an `OSError` (`SyntaxError: broken PNG file (chunk ...)` when a truncated stream is followed by
bytes that are not a chunk id, a `struct.error` from a short `pHYs` after the pixels) escapes
Python's `except (OSError, ValueError)`; here the file is refused with an `OpenError::Os`. The
chunk handlers after the pixel data (text, ICC, EXIF) are not run for their errors; `pymeta`
reads them.

## 4. Differences that are not matched

Each is counted over the runs above and is brought to Cloud on DAN-2062 before the verify opens.

| Class | What | Python | Rust | Count |
|---|---|---|---|---|
| **C1** a JPEG that is not Huffman coded | arithmetic coded (SOF9 to SOF11, SOF13 to SOF15), lossless (SOF3, SOF5 to SOF7) or hierarchical | Pillow decodes it, so the file hashes | `pyjpeg::Error::Unsupported`, which `pyraster::open` reports as an `OSError`, so the file reads as "could not hash" (`local_prints` and `dhash` give `None`, exactly as for a file that is not a picture) | valid files: 26 of 1429 checks (all arithmetic); damaged files whose marker byte became a non-Huffman SOF: 28 of 968 checks (7 files at 4 scales); one lossless file probed by hand (`cjpeg -lossless 1`) decodes in Pillow and is `Unsupported` here. Hierarchical is not measured (no encoder at hand) |
| **C2** damaged entropy data that overflows a 16-bit lane | a damaged file whose coefficients dequantise past 16 bits: libjpeg-turbo's SIMD IDCT works in 16-bit lanes, and the port reproduces its saturation and wrap-around for ordinary overflow but not for every order of overflow | a particular garbage picture | a different garbage picture, equal in size and mode, 4 to 956 bytes of the decoded data differ | 14 of 968 checks (9 files) |
| **C3** a JPEG with no EOI after a single scan where the lookahead hits the end | one file of the damaged set | `OSError('image file is truncated (1 bytes not processed)')` | decodes | 1 of 968 checks |
| **C4** a progressive file that stops short of full precision in a band | libjpeg's block smoothing would run | smoothed output | `Unsupported` | 0 in either set (no file reached it); listed because the code path refuses rather than guesses |
| **C5** a page number past 38 digits | `page_index_from_thumbnail` | Python's `int` has no limit | stops at `i128::MAX`, which every caller treats as "past the page count", as the huge number is | 0 differing in the 19 677 pure cases (they reach 20 digits, which is exact); the 40-digit saturation is pinned by `multipage_tests::test_a_page_number_past_the_integer_width_saturates` |

What each means for the product: C1 is a rare real-world file (a booru does not serve arithmetic
or lossless JPEG); where one turns up, the candidate keeps its ordinal score instead of a measured
one (`similarity_check`'s existing "could not hash" path: an INFO line naming the file and the reason, e.g. `arith.jpg: could not hash the local file (unsupported JPEG: arithmetic, lossless or hierarchical JPEG) - leaving ordinal scores alone ...`; pinned by `test_a_jpeg_the_port_cannot_decode_says_why_at_info`) and a multi-page post
stays on the page the site named. C2 and C3 only occur in damaged files and a damaged picture
already hashes as noise. None of the three changes a hash for a well-formed Huffman JPEG, PNG,
GIF, BMP, WebP, TIFF, PPM or ICO: 0 differing of 12 247.

Found in this ticket, fixed in DAN-2088: `image_prep`'s thumbnail passes were horizontal-first
always, whereas Pillow goes vertical-first when `in_h > 100 * in_w` (`pyraster` already had the
rule; `image_prep` is the upload-size downscale, not a hash path). For a picture more than 100
times taller than wide the output differed from Pillow's by a grey level or so (a 5 x 520 grey
picture thumbnailed to 3 x 312 differed in the sum of its bytes, 119,385 against Pillow's 119,374).
`image_prep` now takes the same rule; the tests
`a_picture_over_a_hundred_times_taller_than_wide_is_resampled_down_first` (three sizes, expected
values from Pillow 12.3) and `the_across_pass_stays_first_at_exactly_a_hundred_to_one` pin it.

## 5. Mutation check

Run in the author tree and reverted each time (`worktrees/DAN-2062-mutation.log`):

| Mutation | Red |
|---|---|
| the Lanczos window `-3.0..3.0` to `-2.0..2.0` (`pyraster::lanczos`) | `image_compare_golden::dhash_matches_pillow_on_every_format`, `::local_prints_match_pillow`, `::comparisons_match_pillow` |
| `DHASH_SAME_MAX` 5 to 6 | `image_compare_golden::comparisons_match_pillow` |

## 6. Python test mapping

Every test in `tests/` at `76c85c4` that names a function in scope (an AST scan for the names
above in each test's body, 98 tests): 93 map to a Rust test of the same name, 5 have a reason below.
Rust test files: `image_compare_tests.rs` (45), `image_compare_golden.rs` (5, Pillow's own output
pinned as fixtures under `tests/data/image_compare/`), `multipage_tests.rs` (78),
`similarity_check_tests.rs` (24), and `pyraster_png_damage.rs` (3, section 3.1; no Python test). The `multipage_resolve` tests are in `multipage_tests.rs` and run over a local https server (the net layer's own constructor).

| Python test | Rust test, or why not |
|---|---|
| `test_core_utils.py::TestPerceptualSimilarityScale::test_the_anchors_land_where_the_bands_say` | `image_compare_tests.rs::test_the_anchors_land_where_the_bands_say` |
| `test_core_utils.py::TestPerceptualSimilarityScale::test_it_decreases_all_the_way_down` | `image_compare_tests.rs::test_it_decreases_all_the_way_down` |
| `test_core_utils.py::TestPerceptualSimilarityScale::test_a_distance_beyond_the_scale_is_zero_not_negative` | `image_compare_tests.rs::test_a_distance_beyond_the_scale_is_zero_not_negative` |
| `test_core_utils.py::TestPerceptualSimilarityScale::test_no_distance_means_no_number` | `image_compare_tests.rs::test_no_distance_means_no_number` |
| `test_core_utils.py::TestPerceptualSimilarityScale::test_a_resize_and_a_re_encode_still_read_as_the_same_picture` | `image_compare_tests.rs::test_a_resize_and_a_re_encode_still_read_as_the_same_picture` |
| `test_core_utils.py::TestPerceptualSimilarityScale::test_a_different_picture_scores_low` | `image_compare_tests.rs::test_a_different_picture_scores_low` |
| `test_hydrus_duplicate_relationships.py::TestDistanceFixtures::test_the_bigger_copy_is_a_confirmed_same_picture` | `image_compare_tests.rs::test_the_bigger_copy_is_a_confirmed_same_picture` |
| `test_hydrus_duplicate_relationships.py::TestDistanceFixtures::test_the_edited_variant_lands_in_the_edit_or_crop_band` | `image_compare_tests.rs::test_the_edited_variant_lands_in_the_edit_or_crop_band` |
| `test_hydrus_duplicate_relationships.py::TestDistanceFixtures::test_the_different_picture_is_past_the_band` | `image_compare_tests.rs::test_the_different_picture_is_past_the_band` |
| `test_hydrus_duplicate_relationships.py::TestVerdictDecisionTable::test_a_same_picture_the_fine_hash_contradicts_says_nothing` | `image_compare_tests.rs::…` asserts `FINE_CONFIRM_MAX` bounds in the `compare_to_local` table (`cmp(1, Some(FINE_CONFIRM_MAX + 10))`); the verdict function itself is `hydrus_import`, Phase 4 |
| `test_hydrus_duplicate_relationships.py::TestVerdictDecisionTable::test_the_boundaries_are_inclusive` | `image_compare_tests.rs::test_the_boundaries_are_inclusive` |
| `test_image_compare.py::TestSizeComparison::test_larger_match` | `image_compare_tests.rs::test_larger_match` |
| `test_image_compare.py::TestSizeComparison::test_smaller_match_is_phrased_as_smaller` | `image_compare_tests.rs::test_smaller_match_is_phrased_as_smaller` |
| `test_image_compare.py::TestSizeComparison::test_identical` | `image_compare_tests.rs::test_identical` |
| `test_image_compare.py::TestSizeComparison::test_a_pixel_or_two_is_not_an_upgrade` | `image_compare_tests.rs::test_a_pixel_or_two_is_not_an_upgrade` |
| `test_image_compare.py::TestSizeComparison::test_different_aspect_is_called_out` | `image_compare_tests.rs::test_different_aspect_is_called_out` |
| `test_image_compare.py::TestSizeComparison::test_invalid_dimensions_yield_no_verdict` | `image_compare_tests.rs::test_invalid_dimensions_yield_no_verdict` |
| `test_image_compare.py::TestSizeComparison::test_file_size_comparison` | `image_compare_tests.rs::test_file_size_comparison` |
| `test_image_compare.py::TestSizeComparison::test_a_large_ratio_is_never_in_exponent_form` | `image_compare_tests.rs::test_a_large_ratio_is_never_in_exponent_form` |
| `test_image_compare.py::TestSizeComparison::test_the_size_column_is_never_in_exponent_form_either` | `image_compare_tests.rs::test_the_size_column_is_never_in_exponent_form_either` |
| `test_image_compare.py::TestCompactSizeDelta::test_bigger_match` | `image_compare_tests.rs::test_bigger_match` |
| `test_image_compare.py::TestCompactSizeDelta::test_smaller_match_counts_how_many_times_smaller` | `image_compare_tests.rs::test_smaller_match_counts_how_many_times_smaller` |
| `test_image_compare.py::TestCompactSizeDelta::test_equal` | `image_compare_tests.rs::test_equal` |
| `test_image_compare.py::TestCompactSizeDelta::test_a_pixel_or_two_still_counts_as_equal` | `image_compare_tests.rs::test_a_pixel_or_two_still_counts_as_equal` |
| `test_image_compare.py::TestCompactSizeDelta::test_fractional_ratio` | `image_compare_tests.rs::test_fractional_ratio` |
| `test_image_compare.py::TestCompactSizeDelta::test_unknown_dimensions_give_no_label` | `image_compare_tests.rs::test_unknown_dimensions_give_no_label` |
| `test_image_compare.py::TestCompactSizeDelta::test_agrees_with_the_preview_panel` | `image_compare_tests.rs::test_agrees_with_the_preview_panel` |
| `test_image_compare.py::TestCompactSizeDelta::test_sorts_by_ratio_not_by_text` | `image_compare_tests.rs::test_sorts_by_ratio_not_by_text` |
| `test_image_compare.py::TestCompactSizeDelta::test_unknown_sorts_to_one_end_not_into_parity` | `image_compare_tests.rs::test_unknown_sorts_to_one_end_not_into_parity` |
| `test_image_compare.py::TestPerceptualMatching::test_same_image_at_double_resolution` | `image_compare_tests.rs::test_same_image_at_double_resolution` |
| `test_image_compare.py::TestPerceptualMatching::test_survives_heavy_jpeg_re_encoding` | `image_compare_tests.rs::test_survives_heavy_jpeg_re_encoding` |
| `test_image_compare.py::TestPerceptualMatching::test_survives_downscale_and_re_encode_together` | `image_compare_tests.rs::test_survives_downscale_and_re_encode_together` |
| `test_image_compare.py::TestPerceptualMatching::test_different_images_are_reported_as_different` | `image_compare_tests.rs::test_different_images_are_reported_as_different` |
| `test_image_compare.py::TestPerceptualMatching::test_accepts_bytes_and_paths_and_images` | `image_compare_tests.rs::test_accepts_bytes_and_paths_and_images` |
| `test_image_compare.py::TestPerceptualMatching::test_unreadable_source_returns_none_rather_than_raising` | `image_compare_tests.rs::test_unreadable_source_returns_none_rather_than_raising` |
| `test_image_compare.py::TestPerceptualMatching::test_descriptions_are_hedged_not_absolute` | `image_compare_tests.rs::test_descriptions_are_hedged_not_absolute` |
| `test_image_compare.py::TestAlignedComparison::test_a_mirrored_copy_is_recognised_but_kept_below_auto_import` | `image_compare_tests.rs::test_a_mirrored_copy_is_recognised_but_kept_below_auto_import` |
| `test_image_compare.py::TestAlignedComparison::test_a_border_is_trimmed_and_a_picture_without_one_is_left_alone` | `image_compare_tests.rs::test_a_border_is_trimmed_and_a_picture_without_one_is_left_alone` |
| `test_image_compare.py::TestAlignedComparison::test_nothing_to_compare_gives_no_score` | `image_compare_tests.rs::test_nothing_to_compare_gives_no_score` |
| `test_image_compare.py::TestAlignedComparison::test_a_90_the_fine_hash_does_not_confirm_is_held_for_review` | `image_compare_tests.rs::test_a_90_the_fine_hash_does_not_confirm_is_held_for_review` |
| `test_image_compare.py::TestAlignedComparison::test_an_oversized_image_fails_quietly` | `image_compare_tests.rs::test_an_oversized_image_fails_quietly` |
| `test_multipage.py::TestChoosingThePage::test_identifies_each_page_in_turn` | `multipage_tests.rs::test_identifies_each_page_in_turn` |
| `test_multipage.py::TestChoosingThePage::test_a_later_page_is_found_not_just_the_first` | `multipage_tests.rs::test_a_later_page_is_found_not_just_the_first` |
| `test_multipage.py::TestChoosingThePage::test_single_page_post_is_not_considered` | `multipage_tests.rs::test_single_page_post_is_not_considered` |
| `test_multipage.py::TestChoosingThePage::test_empty_input` | `multipage_tests.rs::test_empty_input` |
| `test_multipage.py::TestRefusingToGuess::test_indistinguishable_pages_are_not_chosen_between` | `multipage_tests.rs::test_indistinguishable_pages_are_not_chosen_between` |
| `test_multipage.py::TestRefusingToGuess::test_local_image_absent_from_the_post` | `multipage_tests.rs::test_local_image_absent_from_the_post` |
| `test_multipage.py::TestRefusingToGuess::test_a_featureless_local_image_is_refused` | `multipage_tests.rs::test_a_featureless_local_image_is_refused` |
| `test_multipage.py::TestRefusingToGuess::test_unreadable_local_image` | `multipage_tests.rs::test_unreadable_local_image` |
| `test_multipage.py::TestRefusingToGuess::test_unreadable_thumbnails` | `multipage_tests.rs::test_unreadable_thumbnails` |
| `test_multipage.py::TestPageUrlRewriting::test_rewrites_to_another_page` | `multipage_tests.rs::test_rewrites_to_another_page` |
| `test_multipage.py::TestPageUrlRewriting::test_page_zero_is_unchanged` | `multipage_tests.rs::test_page_zero_is_unchanged` |
| `test_multipage.py::TestPageUrlRewriting::test_only_the_last_marker_is_replaced` | `multipage_tests.rs::test_only_the_last_marker_is_replaced` |
| `test_multipage.py::TestPageUrlRewriting::test_a_url_without_the_marker_is_refused` | `multipage_tests.rs::test_a_url_without_the_marker_is_refused` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_reads_the_page_off_a_manga_thumbnail` | `multipage_tests.rs::test_reads_the_page_off_a_manga_thumbnail` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_double_digit_pages` | `multipage_tests.rs::test_double_digit_pages` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_single_page_thumbnail_form` | `multipage_tests.rs::test_single_page_thumbnail_form` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_historical_index_is_read_too` | `multipage_tests.rs::test_historical_index_is_read_too` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_a_different_illust_id_is_refused` | `multipage_tests.rs::test_a_different_illust_id_is_refused` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_old_thumbnails_carry_no_page` | `multipage_tests.rs::test_old_thumbnails_carry_no_page` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_only_saucenao_pixiv_thumbnails` | `multipage_tests.rs::test_only_saucenao_pixiv_thumbnails` |
| `test_multipage.py::TestPageIndexFromSauceNaoThumbnail::test_missing_inputs` | `multipage_tests.rs::test_missing_inputs` |
| `test_multipage.py::TestDimensionsAsConfirmation::test_a_unique_exact_size_identifies_the_page` | `multipage_tests.rs::test_a_unique_exact_size_identifies_the_page` |
| `test_multipage.py::TestDimensionsAsConfirmation::test_a_size_shared_by_two_pages_identifies_nothing` | `multipage_tests.rs::test_a_size_shared_by_two_pages_identifies_nothing` |
| `test_multipage.py::TestDimensionsAsConfirmation::test_a_resized_local_file_identifies_nothing` | `multipage_tests.rs::test_a_resized_local_file_identifies_nothing` |
| `test_multipage.py::TestDimensionsAsConfirmation::test_unknown_local_size` | `multipage_tests.rs::test_unknown_local_size` |
| `test_multipage.py::TestDimensionsAsRefutation::test_a_resize_of_the_named_page_is_not_contradicted` | `multipage_tests.rs::test_a_resize_of_the_named_page_is_not_contradicted` |
| `test_multipage.py::TestDimensionsAsRefutation::test_a_portrait_local_file_contradicts_a_landscape_page` | `multipage_tests.rs::test_a_portrait_local_file_contradicts_a_landscape_page` |
| `test_multipage.py::TestDimensionsAsRefutation::test_the_widest_confirmed_real_case_still_passes` | `multipage_tests.rs::test_the_widest_confirmed_real_case_still_passes` |
| `test_multipage.py::TestDimensionsAsRefutation::test_unknown_sizes_never_contradict` | `multipage_tests.rs::test_unknown_sizes_never_contradict` |
| `test_multipage.py::TestTiesAreMarked::test_indistinguishable_pages_are_flagged` | `multipage_tests.rs::test_indistinguishable_pages_are_flagged` |
| `test_multipage.py::TestTiesAreMarked::test_a_post_without_the_image_is_not_flagged` | `multipage_tests.rs::test_a_post_without_the_image_is_not_flagged` |
| `test_multipage.py::TestTiesAreMarked::test_a_confident_match_is_not_flagged` | `multipage_tests.rs::test_a_confident_match_is_not_flagged` |
| `test_multipage.py::TestCheckingOnePage::test_the_right_page_is_confirmed` | `multipage_tests.rs::test_the_right_page_is_confirmed` |
| `test_multipage.py::TestCheckingOnePage::test_a_different_page_is_refused` | `multipage_tests.rs::test_a_different_page_is_refused` |
| `test_multipage.py::TestCheckingOnePage::test_plausible_is_not_good_enough` | `multipage_tests.rs::test_plausible_is_not_good_enough` |
| `test_multipage.py::TestCheckingOnePage::test_a_featureless_local_image_confirms_nothing` | `multipage_tests.rs::test_a_featureless_local_image_confirms_nothing` |
| `test_multipage.py::TestCheckingOnePage::test_unreadable_images` | `multipage_tests.rs::test_unreadable_images` |
| `test_parsers_and_tags.py::TestMultipageResolverIsNotPixivOnly::test_the_resolver_no_longer_hardcodes_pixiv_for_the_page_list` | does not apply: it greps the Python source of `_resolve_multipage_candidate`; the behaviour it guards (a non-Pixiv site's page list is used) is `multipage_tests.rs::test_twitter_offers_the_page_list_hooks` and the other-sites resolver tests in the same file |
| `test_parsers_and_tags.py::TestMultipageResolverIsNotPixivOnly::test_saucenaos_page_hint_stays_pixiv_only` | `multipage_tests.rs::test_saucenaos_page_hint_stays_pixiv_only` |
| `test_parsers_and_tags.py::TestParsedOnce::test_the_page_list_reuses_the_record_just_fetched` | `multipage_tests.rs::test_the_page_list_reuses_the_record_just_fetched` |
| `test_pawchive_index.py::TestTheIndexAsASearchEngine::test_its_thumbnail_names_the_image_in_a_multi_image_post` | `multipage_tests.rs::test_its_thumbnail_names_the_image_in_a_multi_image_post` |
| `test_search_quality.py::TestMeasuredSimilarityReplacesOrdinalScores::test_a_blurred_placeholder_is_never_measured_from` | `similarity_check_tests.rs::test_a_blurred_placeholder_is_never_measured_from` |
| `test_search_quality.py::TestMeasuredSimilarityReplacesOrdinalScores::test_a_tiny_image_is_not_measured_from_either` | `similarity_check_tests.rs::test_a_tiny_image_is_not_measured_from_either` |
| `test_search_quality.py::TestMeasuredSimilarityReplacesOrdinalScores::test_junk_bytes_are_not_measured_from` | `similarity_check_tests.rs::test_junk_bytes_are_not_measured_from` |
| `test_search_quality.py::TestSkippedMeasurementIsLogged::test_a_thumbnail_too_poor_to_hash_says_so` | `similarity_check_tests.rs::test_a_thumbnail_too_poor_to_hash_says_so` |
| `test_search_quality.py::TestSkippedMeasurementIsLogged::test_a_local_file_that_could_not_be_hashed_says_so_at_INFO` | `similarity_check_tests.rs::test_a_local_file_that_could_not_be_hashed_says_so_at_info` (the name's `INFO` is lower-cased; asserts the one INFO line) |
| `test_search_quality.py::TestTwitterParser::test_the_photo_list_is_offered_for_the_multipage_resolver` | `twitter.rs::the_photo_list_is_offered_for_the_multipage_resolver` |
| `test_search_quality.py::TestRule34SearchTiles::test_the_closest_post_is_chosen_over_a_variant` | later ticket: these two exercise `rule34.find_post_by_search` (listing request, thumbnail downloads, dhash comparison), which `boorus/rule34.rs` leaves to the engine client (Google Lens, 3.e); `dhash` and `hamming_distance` are here and matched |
| `test_search_quality.py::TestRule34SearchTiles::test_nothing_close_enough_finds_nothing` | later ticket, same as the row above |
| `test_search_quality.py::TestBorrowedThumbnails::test_a_wrong_shape_is_remeasured_against_the_posts_own_image` | `similarity_check_tests.rs::test_a_wrong_shape_is_remeasured_against_the_posts_own_image` |
| `test_search_quality.py::TestBorrowedThumbnails::test_the_right_shape_is_left_alone` | `similarity_check_tests.rs::test_the_right_shape_is_left_alone` |
| `test_search_quality.py::TestBorrowedThumbnails::test_no_image_of_its_own_means_unmeasured` | `similarity_check_tests.rs::test_no_image_of_its_own_means_unmeasured` |
| `test_search_quality.py::TestNeverMeasuredOrdinalGetsARealScore::test_a_lens_exact_match_gets_measured_instead_of_the_ordinal_80` | `similarity_check_tests.rs::test_a_lens_exact_match_gets_measured_instead_of_the_ordinal_80` |
| `test_search_quality.py::TestNeverMeasuredOrdinalGetsARealScore::test_a_real_similarity_engine_is_left_alone` | `similarity_check_tests.rs::test_a_real_similarity_engine_is_left_alone` |
| `test_search_quality.py::TestNeverMeasuredOrdinalGetsARealScore::test_no_page_image_leaves_the_ordinal_score_but_marked_unmeasured` | `similarity_check_tests.rs::test_no_page_image_leaves_the_ordinal_score_but_marked_unmeasured` |
| `test_search_quality.py::TestPawchivePagesByHash::test_the_local_files_hash_picks_its_page_without_downloading_anything` | `multipage_tests.rs::test_the_local_files_hash_picks_its_page_without_downloading_anything` |
| `test_similarity_marking.py::TestTheFlagSurvivesARestart::test_a_restored_measured_candidate_is_not_queued_for_re_measuring` | `similarity_check_tests.rs::test_a_restored_measured_candidate_is_not_queued_for_re_measuring` |
