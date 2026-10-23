# Hydrus file import (Phase 4.c, DAN-2130)

What this ticket ports, where the cut is, how it was checked against the real Python, what is not
matched, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4` (Pillow 12.3.0 in the black-box venv). It uses 4.a's
`HydrusClient` (`docs/hydrus-client.md`) and 4.b's write calls (`docs/hydrus-client-write.md`) as they
are, and the Phase 3 `image_compare` (`docs/image-compare.md`) for the duplicate verdict.

## 1. The cut

| Python | Rust (`crates/hatate-core/src`) |
|---|---|
| `core/hydrus_import.py`: `ImportResult`, `send_file_upload`, `_repoint`, `_is_exact_copy`, `_looks_like_a_web_page`, `_validate_magic_bytes`, `_validate_image_decodes`, `duplicate_verdict`, `record_duplicate_relationship`, `write_provenance_note`, `_joined`, `_imported_bytes` | `hydrus_import`: `ImportResult`, `send_file_upload`, `repoint`, `is_exact_copy`, `looks_like_a_web_page`, `validate_magic_bytes`, `validate_image_decodes`, `duplicate_verdict` (+ `verdict_of`, its decision table), `record_duplicate_relationship`, `write_provenance_note`, `joined`, `imported_bytes` |
| `core/provenance_note.py` (`note_name`, `provenance_note`, the note's wording) | `provenance_note` |
| `core/similarity_display.py`: `similarity_label`, `similarity_tooltip` and the two notes (the chip key is the GUI's) | `similarity_display` |
| `core/models.py`: `enabled_tag_sources`, `tags_to_send`, `ImageEntry.matched_url` | `model::{enabled_tag_sources, tags_to_send, Entry::matched_url}` |

These three were not ported before and the file-upload half reads them, so they come with it.

Not here, and who owns it: the URL-import half of `hydrus_import.py` (`url_import_refusal`,
`_forwarded_url`, `relate_confirmed_url_import`, `finish_url_import`, `send_url_or_download`,
`download_instead`, `send_url_to_importer`, `download_and_send`) is 4.d. `normalize_url_for_hydrus` is
`hydrus_url` (Phase 3) and the file upload does not call it. The workers and auto-import wiring are
Phase 5; every GUI caller is Phase 6.

No new dependency, no new error type (the errors are 4.a's `HydrusError` with its `kind()`,
`user_message()` and `detail()`), no change to `Entry.error_message` or `MatchStatus` on disk.

## 2. How it is built

- **Errors.** Python's functions catch `HydrusError` and turn it into an `ImportResult` or a warning
  string; anything else (an `AttributeError` from an answer of the wrong shape) propagates. A function
  here returns `Err` only for what Python lets propagate (`ErrorKind::UnexpectedResponse`). A caught
  error is also kept on `ImportResult::fault`, the typed error behind `error`, so a caller can show
  `user_message()` (no path, URL query, key or OS text) while `error` stays Python's text,
  byte for byte, as `Entry.error_message` carries it. `fault` is not part of `ImportResult`'s equality.
- **`ImportResult`** has all seven Python fields (`refused`, `note` and `confirmed` are 4.d's, but the
  type is one).
- **The note's date** is `clock.local_stamp(clock.wall())` cut to the minute (`%Y-%m-%d %H:%M`), so a
  test injects the clock; the binaries pass `SystemClock`.
- **Entry state.** Python's `ImageEntry` stores `matched_url`, `booru_name`, `similarity` and
  `similarity_measured` beside the candidates; `Entry` here derives them from the selected candidate
  (`Entry::matched_url` also reads the one a cached result's dead matches left behind). `repoint`
  changes the selected candidate's URL (and its `tag_version`, which is a function of the URL) and, with
  no selected candidate, the one left behind.
- **`duplicate_verdict`** measures the two real files with `image_compare` (sync CPU, like its other
  callers). `verdict_of` is its decision table on a `Comparison`, the way Python's tests reach it by
  patching `compare_to_local`.
- **`imported_bytes`** uses a private temp file (mode 0600, removed on drop) like `mkstemp` + `unlink`.
- **Decode check.** `validate_image_decodes` runs `pyraster::open`, the Phase 3 port of the part of
  Pillow the hashes use. Pillow's "cannot identify image file" is told from a damaged file by
  `image::guess_format`. A file the build cannot decode at all (AVIF; arithmetic, lossless or
  hierarchical JPEG; a TIFF feature the `image` crate refuses) is **passed**, not rejected (class C4):
  rejecting what Pillow decodes would turn a good file into a failed import.

## 3. How it was checked against the real Python

The real Python at `76c85c4` (its `core/` copied out byte for byte, `requests` real, only
`HTTPAdapter.send` scripted, the wall clock the note reads fixed) and Pillow 12.3.0 ran in the black-box
venv; the Rust side ran offline (`unshare -rn`) against a scripted local server. The harness lives
outside the repo (the tree is Rust only); its log is `worktrees/DAN-2130-differential.log`. Every
comparison is exact.

| Family | Cases (ops) | Compared |
|---|---|---|
| `send_file_upload`: 1 or 2 calls on one entry, random answers for add_file / associate_url / add_tags / metadata / permission / set_notes (status codes, aborts, timeouts, bad JSON, wrong shapes, hash absent / empty / upper case), source filter on and off, note on and off, entries with 0 to 3 candidates, any selection, stale URL, settings `None` | 5 471 (7 088) | every `ImportResult` field, the entry afterwards (hash, error message, sent, confirmed, matched URL, candidate URLs, tags), every request (method, path and query, key header, content type, body bytes, timeout, verify) |
| `record_duplicate_relationship` (generated pictures: same, resized, cropped, brightened, patched, blurred, mirrored, rotated, other; PNG, JPEG, WebP, GIF, BMP, TIFF; damaged files; hashes equal, upper case, empty) and the real fixtures | 3 043 (3 043) | the returned warning and every request |
| `duplicate_verdict`, the same pictures and the real fixtures (each fixture against itself and against the next one) | 1 885 (1 885) | the verdict |
| `write_provenance_note` | 1 745 (1 745) | the warning, every request |
| `_imported_bytes` | 519 (519) | the bytes or `None`, every request, the temp files left behind |
| `_repoint`, `_is_exact_copy`, `_joined`, `similarity_label` / `_tooltip` (rounding halves, nan, inf, -0), `tags_to_send`, `provenance_note` text and date | 2 106 (4 357) | return values and entry state |
| `_validate_magic_bytes` (generated pictures in 7 formats, container signatures, every fixture with 5 claimed formats) | 1 459 (3 929) | the message or `None` |
| `_validate_image_decodes` (the same, damaged in 6 ways; every fixture whole and cut at 25, 50, 90 and 99 %) | 1 509 (3 417) | the message or `None` |
| `_looks_like_a_web_page` | 568 (1 467) | the boolean |
| Time zones for the note's date | 6 (New York, Kolkata, UTC, London, Auckland, Los Angeles), dates from 2000 to 2038 | the text of the note |

**Total: 18 305 cases, 28 350 ops generated and 28 345 compared (the 5 later ops of a case that stopped at a C2 are not), 27 255 requests, 0 unexplained differences.** (Seeds 11 to 16, N 3000
each, and the fixture family; `gen.py SEED N OUT [fixtures]`.) The real fixtures are the PNG, JPEG,
WebP, GIF, BMP, TIFF, ICO and PPM files under `tests/data/image_compare` (the Python repo's own, via
3.g); the generated pictures come from `gen.py` with the seeds above. Hydrus is a scripted local server
and every access key is 64 zeros.

## 4. Differences that are not matched

Each is counted over the runs above and is brought to Cloud on a `Ruling needed` child of DAN-2130
before the verify opens.

| Class | What | Python | Rust | Count |
|---|---|---|---|---|
| **C1** transport words | the library's own words after `Could not reach Hydrus at <url>: ` in an error message or warning | `requests`' exception text | the net layer's text | 379 ops. The same class Cloud accepted as C1 on DAN-2126 (DAN-2133); the part before and after the words is compared |
| **C2** a `hash` that is not a string | the add_file answer carries a bool, number, list or object as `hash` | carries it into the entry, the URL call and the tags, or fails later with a `TypeError` | `Err(UnexpectedResponse)` before the entry changes | 19 cases (the later ops of such a case and 36 later requests are not compared) |
| **C3** the address in Pillow's text | `cannot identify image file <_io.BytesIO object at 0x7f...>` | the real address of the object, which differs from run to run | the same text with a fixed address (`0x7f0000000000`) | 436 ops, equal otherwise |
| **C4** a file the build cannot decode is passed | AVIF; arithmetic, lossless or hierarchical JPEG; a TIFF feature the `image` crate refuses; any of them damaged | Pillow decodes it, or finds it damaged | no error | 13 ops where Python rejects a damaged one (the file Pillow accepts is passed by both) |
| **C5** the text for a damaged image | a file that is identified but damaged | Pillow's exception text, which has decoder-internal numbers (`image file is truncated (309 bytes not processed)`, `Truncated File Read`, `broken PNG file (bad header checksum in b'IDAT')`, `could not create decoder object`, ...) | the decoder's own text (`image file is truncated`, `unexpected end of file`, ...) | 652 of the 1 088 ops where both reject. **Reject / accept is equal in every one**; only the words after `Downloaded data does not decode as a valid image: ` differ |
| **C6** a damaged ICO | an ICO cut 1 % short | Pillow decodes it | the `image` crate refuses it | 1 op |
| **C7** a JPEG the build cannot hash | an arithmetic-coded JPEG is the local or the downloaded file | `duplicate_verdict` measures it and answers | `None` (no relationship is written): the inherited `image-compare` C1 | 3 verdicts and 12 requests not made, all on the one fixture `cj_arith.jpg` |
| **C8** a PNG with a damaged IDAT | a PNG whose IDAT checksum or zlib stream is damaged is the local or the downloaded file | Pillow hashes it (it reads IDAT without checking the CRC) | `pyraster` refuses it: `None` | 2 verdicts. Phase 3 code, not changed here; a follow-up would make `pyraster::open` as lenient as Pillow |

What each means for the product: C1 to C3 and C5 change a word in a message and never a decision.
C4 and C6 are the decode check being lenient where it cannot be strict, and C6 the other way round for
one damaged container. C7 and C8 mean no duplicate relationship is written for a file Hatate cannot
hash, which is the safe side ("never fire on an unconfirmed verdict"). C2 cannot happen with a real
Hydrus (its hash is a 64-character string).

## 5. The tests

`tests/hydrus_import.rs` (32), `provenance_note::tests` (6), `similarity_display::tests` (3), all
against a scripted local Hydrus and a synthetic key. They include the two schemes through the binaries'
own constructor (`HydrusClient::new`, the 1.c net layer): plain http, and https against a self-signed
local server with `verify_ssl` true (the connection is refused, nothing is sent) and false (it
succeeds); one timeout; one network error; and the error shape (a missing local file gives a
`user_message` with neither the path nor `os error`; a network failure's `user_message` has no URL
query, no key and no OS text).

## 6. Python tests that exercise a function in scope

`tests/test_hydrus_provenance_note.py`

| Python | Rust |
|---|---|
| `TestNoteText`: `records_the_five_facts`, `says_which_program_wrote_it` | `provenance_note::tests::it_records_the_five_facts_and_says_which_program_wrote_it` |
| `a_measured_similarity_is_written_as_a_plain_number`, `an_unmeasured_similarity_is_marked_and_explained` | `a_measured_similarity_is_plain_and_an_unmeasured_one_is_marked_and_explained` |
| `a_missing_fact_is_left_out_rather_than_written_as_unknown` | `a_missing_fact_is_left_out_rather_than_written_as_unknown` (the Python test clears `entry.similarity` beside a selected candidate; `Entry` derives it from the candidate, so that state cannot exist: the line is absent only for an entry with no candidate, which `no_match_means_no_note_at_all` and the differential cover) |
| `either_half_of_a_match_on_its_own_is_still_worth_a_note` | `either_half_of_a_match_on_its_own_is_still_worth_a_note` (an engine with an empty URL stands for "matched URL cleared") |
| `no_match_means_no_note_at_all` | `no_match_means_no_note_at_all` |
| `the_date_defaults_to_now` | does not apply: the date comes from the injected `Clock` (`SystemClock` in the binaries); the text and date in 6 zones are in the differential |
| `the_note_name_falls_back_when_the_setting_is_blank`, `the_default_name_says_which_program_owns_it` | `the_note_name_falls_back_when_blank_and_the_default_says_whose_it_is` |
| `TestFeatureOff`: `it_is_off_by_default`, `the_note_name_is_blank_by_default` | `no_hash_writes_no_note_and_the_note_settings_default_to_off_and_blank`; the absent-setting reading is the Phase 1 config tests |
| `nothing_is_asked_of_hydrus_and_no_text_is_built`, `settings_of_none_is_treated_as_off` | `the_note_is_off_by_default_and_then_costs_no_request` |
| `a_whole_import_is_unchanged`, `the_url_importer_path_is_unchanged` | 4.d (`download_and_send`, URL importer) |
| `TestFeatureOn`: `the_note_is_written_onto_the_imported_file`, `a_configured_name_is_used` | `the_note_text_name_and_date_are_written_after_the_hash_is_confirmed_held` |
| `nothing_is_written_for_a_file_with_no_match`, `..._when_hydrus_is_not_holding_the_file` | `no_note_is_written_for_a_file_hydrus_is_not_holding_or_an_entry_with_no_match` |
| `no_hash_writes_nothing` | `no_hash_writes_no_note_and_the_note_settings_default_to_off_and_blank` |
| `end_to_end_through_send_file_upload` | `an_upload_writes_the_note_last_and_reports_a_failed_note_beside_its_own_warnings` |
| `end_to_end_through_download_and_send`, `..._the_url_importer_path` | 4.d |
| `TestDegradesGracefully`: `a_key_without_the_permission_names_it_and_says_where`, `any_other_hydrus_failure_still_leaves_the_import_standing` | `a_missing_notes_permission_and_a_failed_write_come_back_as_warnings` |
| `an_old_hydrus_says_so_rather_than_reporting_a_404` | the message is 4.b's `set_note` (`hydrus_client_write` tests); here the warning carries it (same test) |
| `a_failed_note_does_not_hide_a_failed_relationship` | `an_exact_copy_is_a_pawchive_match_and_joined_drops_the_empty_ones` (`joined`); the two together are 4.d |
| `the_upload_path_reports_it_alongside_its_own_warnings` | `an_upload_writes_the_note_last_and_reports_a_failed_note_beside_its_own_warnings` |
| `TestClientWrapper`, `TestPermissionTable` | 4.b (`set_note`, `missing_permission`) |
| `TestAutoImportWiring`, `TestBatchPollWorkerWiring` | do not apply here: the auto-import and poll workers are Phase 5 and 4.d |

`tests/test_hydrus_duplicate_relationships.py`

| Python | Rust |
|---|---|
| `TestDistanceFixtures` (four tests of `compare_to_local` on the fixtures) | `image_compare` is 3.g's; the distances of the edited variant (64-bit 7, 256-bit 21) are asserted in `an_edited_variant_is_an_alternate_and_its_measured_distances_...` |
| `TestVerdict`: `a_bigger_copy_..._is_the_same_picture`, `a_mirrored_copy_says_nothing`, `a_different_picture_says_nothing`, `unreadable_bytes_say_nothing`, `an_unreadable_local_file_says_nothing` | `the_verdict_reads_real_files_and_says_nothing_for_unreadable_ones` |
| `a_different_edit_is_an_alternate` | `an_edited_variant_is_an_alternate_and_its_measured_distances_are_the_ones_the_python_suite_states` |
| `TestVerdictDecisionTable` (five tests) | `the_verdict_table_says_nothing_without_a_distance_a_fine_hash_or_when_mirrored` |
| `TestFeatureOff`: `it_is_off_by_default`, `nothing_is_asked_of_hydrus_and_nothing_is_measured`, `settings_of_none_is_treated_as_off` | `the_same_picture_is_marked_better_and_made_king_but_only_when_the_user_asked`, `..._settings_default_to_off_and_blank` |
| `a_whole_import_is_unchanged`, `end_to_end_through_download_and_send` | 4.d |
| `TestFeatureOn`: `same_picture_sets_the_new_file_better_and_crowns_it` | `the_same_picture_is_marked_better_and_made_king_but_only_when_the_user_asked` |
| `a_different_edit_is_filed_as_an_alternate_and_crowns_nobody` | `an_alternate_is_filed_as_an_alternate_and_crowns_nobody` |
| `an_uncertain_verdict_writes_nothing`, `a_mirrored_copy_writes_nothing`, `nothing_is_written_when_hydrus_is_not_holding_the_local_copy`, `..._when_there_is_no_local_hash`, `..._when_hydrus_already_had_this_exact_file` | `a_different_picture_the_same_hash_and_a_file_hydrus_does_not_hold_write_nothing` (mirrored: the verdict table and `the_verdict_reads_real_files...`) |
| `TestDegradesGracefully`: `a_key_without_the_permission_names_it_and_says_where`, `any_other_hydrus_failure_still_leaves_the_import_standing` | `a_failed_relationship_write_is_a_warning_and_a_missing_permission_names_it` |
| `an_old_hydrus_says_so_rather_than_reporting_a_404` | 4.b's `set_file_relationship` message; here the warning carries it (same test) |
| `a_failure_crowning_the_new_file_is_reported_not_raised` | `a_failure_crowning_the_new_file_is_reported_not_raised` |
| `TestClientWrappers`, `TestPermissionProbe`, `TestErrorStatusCodes` | 4.a and 4.b |

`tests/test_hydrus_import_validation.py` (the validators; each Python test runs them through
`download_and_send`, which is 4.d)

| Python | Rust |
|---|---|
| `rejects_empty_file`, `rejects_implausibly_small_file`, `rejects_wrong_magic_bytes_jpeg`, `claimed_format_png_with_jpeg_bytes_rejected`, `accepts_valid_jpeg`, `accepts_valid_png`, `infers_format_from_extension_when_no_remote_format`, the five `extensionless_url_...` tests | `magic_bytes_follow_the_claimed_format_then_the_extension_then_a_sniff` |
| `rejects_html_page`, `..._html_rejected` | `the_web_page_check_reads_the_first_512_bytes_after_leading_whitespace_case_blind` |
| `rejects_corrupted_image` | `a_decode_check_passes_real_pictures_and_names_html_and_truncation` |
| `size_check_still_works_when_known`, `rejects_hydrus_hash_mismatch`, `accepts_matching_hydrus_hash` | 4.d (`download_and_send`) |

`tests/test_sent_tag_filter.py`: `TestTheHelper` -> `the_tag_filter_is_off_by_default_and_literal_when_on`
(`the_returned_list_is_not_the_entrys_own` does not apply: `tags_to_send` returns references);
`TestTheThreeSends` -> `the_source_filter_decides_which_tags_are_sent_only_when_it_is_on` for the upload,
the other two sends are 4.d; `TestTheExport`, `TestTheMigration` and `TestTheDialogCopy` are the
exports (Phase 5), the config migration (Phase 1) and the dialog (Phase 6).

`tests/test_similarity_marking.py` (`similarity_label`, `similarity_tooltip`) ->
`similarity_display::tests`; the rest of that file is the GUI. `tests/test_session.py`
`test_upload_path_is_confirmed` -> `an_upload_over_the_default_plain_http_scheme_sends_file_url_and_tags_in_order`
(`sent_to_hydrus` and `hydrus_import_confirmed` true); its URL-importer twin is 4.d.
`tests/test_url_importer_duplicate_relationships.py` calls `record_duplicate_relationship` through the
URL importer (4.d). The `send_file_upload` mentions in `tests/test_gui_smoke.py` patch the function out
and are Phase 6.
