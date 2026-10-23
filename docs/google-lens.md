# Google Lens, the engine half: image prep, pacing, stand-down, search, ranking (Phase 5.a, DAN-2163)

What this ticket ports, where the cut is, how it was checked against the real Python, which
differences could not be matched, and which Python tests map to which Rust ones. Python source of
truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`, `core/google_lens.py` (CPython 3.14.8, Pillow
12.3.0 for the differential). The 2.j Lens parsers (`engines/google_lens.rs`), the 3.f engine runner
(`docs/engine-runner.md`) and the Phase 1 to 4 modules are reused as they are; the changes to them
are listed in section 2.

## 1. The cut

| Python (`core/google_lens.py`) | Rust |
|---|---|
| `media_type`, `LENS_READS`, `prepare_for_lens` (with the format check), `_is_animated`, `_to_jpeg`, `upload_script` | `engines/lens_prep.rs` |
| `rule34_searches`, `_recover_rule34_searches`, `_resolve_subreddits`, `_is_taggable`, `taggable_first`, `keep_taggable_first`, `effective_timeout` | `engines/lens_rank.rs` |
| `search`, the pacing and stand-down state (`is_blocked`, `is_resting`, `resting_until`, `reset_blocked_flag`, `end_rest`, `wait_for_turn`, `_latch`, `_count_failure`, `MAX_CONSECUTIVE_FAILURES`, `CHALLENGE_REST_SECONDS`, `MIN_SECONDS_BETWEEN_SEARCHES`), `GoogleLensMatch` and the error types, `announce_unavailable` and `ALERT_KEYS` | `engines/lens_search.rs`: `LensFetch`, `LensClient`, `LensError`, `LensConnection` |
| `core/engine_alerts.py` | `engine_alerts.rs` |

Not here, and who owns it:

* `core/lens_browser.py`, the Chromium driver, is 5.b (CDP) and 5.c. The shipped `LensFetch` is
  `NoBrowser`, which answers `LensBrowserUnavailable` exactly as the stand-in this replaces did, so
  `hatate batch` prints the same text as before.
* `boorus.rule34.find_post_by_search` and `reddit.find_post` are network code and have no ticket yet.
  `search` reaches them through the `LensSites` seam; the shipped `NoSites` finds nothing, which is
  also what Python gets when its own request fails. The shipped build never reaches the seam
  (`NoBrowser` answers first). The differential scripts the seam on both sides.
* The parsers `parse_exact_matches`, `parse_exact_tiles`, `parse_lens_payload`, `owner_of_image` were
  ported in 2.j and are called, not copied.

## 2. Changes to merged modules

* `engines/google_lens.rs`: one line, the visibility of what `lens_search` calls.
* `engine_runner.rs`, `engine_backends.rs`: `LiveEngines.lens` is a `LensClient` over `NoBrowser`;
  the "build without Google Lens" stand-in is removed. The runner's Lens branch returns `Raised` for
  the new `LensError::Unexpected`, as it does for the other engines' unexpected errors.
* `error.rs`: `ErrorKind` gains `EngineUnavailable`, `Blocked`, `Failed`, `FormatRefused`,
  `Unexpected`. `hydrus_client.rs` adds the arms for them in its exhaustive match.
* `image_prep.rs`: `encode_rgb_jpeg`, the one encoder `_to_jpeg` and the over-budget re-encode use.
* `pymeta.rs`, `pyimage.rs`: what `_is_animated` needs of Pillow's `n_frames` (APNG, MPO and
  animated WebP; section 4). `pyimage::jpeg` and `pymeta::jpeg` now refuse a frame header whose layer
  table ends mid-entry, as Pillow's `IndexError` does.
* `tests/engine_runner_https.rs` wrote the bytes "a small picture" as `shot.png`. The format check
  rightly refuses that, so it now writes a real PNG.

## 3. What the ticket text asked for and the tree did not have

1. **"The runner's existing event type" does not exist on `main`.** The 3.i `RunState` channel has
   not landed. `LensConnection{NotReachable, WaitingForLens, Ready, Blocked}` goes to a standalone
   sink (`LensClient::with_connection_sink`) that 3.i's event type can wrap. `Ready` and `Blocked`
   are each reached by a test (`a_search_ranks_exact_matches_by_position_and_reports_ready`,
   `three_failed_images_in_a_row_stand_the_engine_down_until_a_search_starts_again`);
   `NotReachable` and `WaitingForLens` are 5.b's.
2. **`LensSites`** (section 1).
3. **Format check (DAN-2063 F2).** `prepare_for_lens` refuses a video, a JPEG XL or a non-image with
   Python's text; the refusal is an ordinary failed image: it does not latch and does not count
   toward `MAX_CONSECUTIVE_FAILURES` (`a_video_is_refused_with_a_reason_and_does_not_latch_or_count`).

## 4. How it was checked

The harness lives outside the tree (the tree is Rust only): `gen.py` (seeded cases) -> `oracle.py`
(Python `core/google_lens.py` at `76c85c4`) -> a Rust test file copied into `tests/` untracked ->
`cmp.py`. Log: `worktrees/DAN-2163-differential.log`. Seeds 101, 202, 303; `TZ=Europe/Berlin` on both
sides.

* **Pure functions, direct differential.** Per seed: 502 `media_type`, 91 `upload_script`, 800
  `rule34_searches`, 600 `taggable_first`, 2,000 `keep_taggable_first`, 11 `effective_timeout`, 214
  local-time formats, about 167 files through `prepare_for_lens` (every `corpus/` Lens image that is
  an image, the Python repo's fixtures, a video, a JXL, a non-image, animated GIF/WebP/APNG/TIFF/MPO,
  an image over `MAX_UPLOAD_BYTES`, cut and bit-flipped files) and about 166 through `_is_animated`
  and `_to_jpeg`. Compared: mime, filename, the pixels handed to the encoder, the refusal text.
* **`search`, scripted fetch.** Python's `lens_browser.fetch_results_payloads` is patched and the
  Rust `LensFetch` is fed the same scripted `(payloads, tiles)` or the same exception
  (`LensBrowserUnavailable`, `LensChallengeUnanswered`, `LensBrowserError`), with a fake clock, fake
  sleep and scripted site lookups on both sides: 300 / 2,000 / 2,000 sequences = 6,160 / 41,207 /
  40,891 steps, long enough to reach the failure counter's latch, the challenge rest and its expiry.
  Compared after every step: every match field, the error class and message, the alerts announced,
  every sleep from `wait_for_turn`, `is_blocked`, `is_resting`, `resting_until`, the fetch arguments
  (bytes hash, mime, name, timeout, flags) and the site-lookup arguments.
* **Damaged animated files.** Per seed, 9,000 files made by cutting `anim.webp`, `anim_lossy.webp`,
  `anim_alpha.webp`, `anim.png`, `anim_alpha.png` and `mpo.jpg` at every length and changing one byte
  at a time in their headers, run through `_is_animated`. The first run differed on 296 WebP and 25
  MPO files; the port now follows libwebp's demuxer (the frame's size is the bitstream's, reserved
  `VP8X` flags are refused, an unknown sub-chunk ends a frame and the rest of the `ANMF` payload is
  read as top-level chunks, the canvas limit is Pillow's) and Pillow's TIFF directory reader (it
  stops with the tags it has when it runs out of bytes). Result: 0 differing.

**Result: 0 unexplained differences on all three seeds.** What differs, by name:

| Class | What | Count per seed |
|---|---|---|
| **F1** | A picture only Pillow decodes (ICO, AVIF, JPEG 2000): Python converts it to JPEG, this build cannot decode it and refuses it (`FormatRefused`, no latch) | 10 of ~168 files |
| **F2** | An animated PNG cut off after its default image: Pillow converts the first frame, the port's decoder fails on the cut and returns the PNG as it was | 0 to 4 |
| **F3** | A JPEG with one flipped bit in its entropy data: the port's decoder and libjpeg give different pixels | 0 to 1 |
| **J1** | The re-encoded JPEG's bytes are `jpeg-encoder`'s, not libjpeg's (granted on DAN-2057). Dimensions, colour mode and the pixels handed to the encoder match. The dHash a Rule 34 lookup takes from those bytes differs with them. | 26 to 28 prep cases; 51, 454, 420 scenario steps |
| **A1** | `_is_animated` on a GIF or TIFF differs; unobservable, because `media_type` of those is not in `LENS_READS` and they are converted either way | 7 to 9 |

F1 to F3 are in decoders merged before this ticket; none is Lens code. They go to Cloud on a
`Ruling needed` child before the verify ticket opens.

## 5. Python tests that exercise this code

Python tests at `76c85c4`. `test_appimage_lens.py` (12 tests) maps to Phase 7. `test_lens_browser.py`
tests of the browser (relaunch, profile, drag) map to 5.b/5.c; the ones of pacing and rest map as
below.

| Python | Rust |
|---|---|
| `test_search_quality`: `test_an_unrecognised_format_is_not_called_a_jpeg`, `test_a_format_google_takes_is_sent_untouched`, `test_a_gif_is_converted_because_lens_cannot_read_one`, `test_an_animated_picture_is_sent_as_its_first_frame`, `test_a_still_webp_is_left_alone`, `test_a_format_google_refuses_is_converted_to_jpeg`, `test_transparency_is_flattened_onto_white_not_black`, `test_the_real_media_type_is_sniffed_from_the_bytes`, `test_the_filename_is_not_trusted_to_describe_the_content`, `test_it_is_never_uploaded_as_an_opaque_blob` | `lens_prep` unit tests (`the_media_type_is_read_from_the_bytes_and_unknown_bytes_are_none`, `a_transparent_pixel_flattens_to_white_and_an_opaque_one_is_left_alone`, `an_animated_webp_is_animated_...`, `an_animated_png_is_animated_...`, `libwebps_demuxer_...`, `an_mpo_is_animated_...`), `a_gif_is_converted_to_a_jpeg_under_a_jpg_name_before_it_goes_up`, and the differential's `prep` section |
| `test_a_video_is_refused_with_a_reason`, `test_a_refused_format_does_not_stand_the_engine_down` | `a_video_is_refused_with_a_reason_and_does_not_latch_or_count` |
| `test_the_upload_script_embeds_its_arguments_safely` | `the_upload_script_carries_its_arguments_as_ascii_json_and_nothing_else`; differential `upload_script` |
| `test_a_missing_browser_is_reported_and_stands_down`, `test_a_genuinely_missing_playwright_fails_loudly_not_silently`, `test_a_lens_that_stood_down_is_not_retried`, `test_a_new_search_tries_again` | `an_unavailable_browser_latches_and_tells_the_user_once`, `three_failed_images_in_a_row_stand_the_engine_down_until_a_search_starts_again` |
| `test_an_unanswered_challenge_stands_the_engine_down`, `test_a_challenge_is_recognised_and_latched` (Lens half), `test_an_unanswered_check_rests_lens_through_a_new_search`, `test_the_rest_ends`, `test_the_rest_ends_within_the_same_run`, `test_a_resting_lens_says_so_on_the_row`, `test_resting_until_survives_a_concurrent_clear` | `an_unanswered_robot_check_rests_for_thirty_minutes_and_survives_a_new_search`, `pacing_stops_when_asked_to_and_a_rest_ending_clears_the_turn` |
| `test_an_empty_payload_is_reported_never_silently_nothing`, `test_a_run_of_failures_stands_the_engine_down`, `test_one_good_image_clears_the_failure_run`, `test_a_javascript_only_page_is_reported_not_recorded_as_empty` | `a_page_with_nothing_in_it_counts_as_a_failure`, `a_good_page_clears_the_run_of_failures` |
| `test_the_best_parsing_response_wins_not_the_biggest`, `test_a_lens_landing_is_parsed_through_the_lens_reader`, `test_exact_matches_outrank_visual_ones`, `test_search_puts_exact_before_visual_and_taggable_first_in_each`, `test_scores_stay_ordered_after_the_two_tabs_are_merged` | `a_search_ranks_exact_matches_by_position_and_reports_ready`; the parsers are 2.j's (`docs/parsers.md`) |
| `test_a_lens_search_gets_longer_than_one_http_request` | `lens_rank` `the_budget_is_a_floor_not_a_ceiling`; differential `effective_timeout` |
| `test_searches_are_spaced_start_to_start`, `test_two_arriving_together_are_both_spaced`, `test_stopping_ends_the_wait` | `the_second_search_waits_out_the_45_seconds_in_one_second_steps`, `pacing_stops_when_asked_to_and_a_rest_ending_clears_the_turn` |
| `test_searches_are_read_from_tiles_precise_first`, `test_search_puts_the_recovered_post_first`, `test_lens_search_hands_back_the_post`, `test_a_booru_post_past_the_cut_survives` | `lens_rank` `precise_rule34_searches_come_first_and_repeats_are_dropped`, `the_first_rule34_search_that_finds_its_post_is_the_one_used`, `a_subreddit_listing_is_swapped_for_its_post_or_dropped_and_never_listed_twice`; differential `rule34`, `search` |
| `test_what_is_kept_stays_in_googles_order`, `test_tag_giving_sites_rank_first_within_each_group`, `test_a_short_list_is_left_alone`, `test_only_untaggable_results_still_fill_every_slot`; `test_parsers_and_tags`: `test_becoming_taggable_is_enough_when_fewer_than_8_already_rank_ahead`, `test_becoming_taggable_now_rescues_a_visual_match_behind_8_taggable_exacts`, `test_reservation_leaves_the_under_cap_case_unchanged` | `lens_rank` `taggable_sites_go_ahead_of_the_rest_and_each_half_keeps_its_order`, `a_list_under_the_limit_is_untouched`, `untaggable_sites_do_not_crowd_out_one_that_gives_tags`, `reserved_slots_keep_the_best_taggable_visual_matches_in_a_crowd_of_exact_ones` |
| `test_lens_dependency_alerts`: `test_a_missing_dependency_is_reported_with_which_one`, `test_a_missing_chromium_is_announced_with_its_fix`, `test_a_missing_playwright_says_the_package`, `test_a_whole_batch_raises_it_once`, `test_the_engine_runner_records_the_error_and_returns_false` | `the_alert_text_follows_what_is_missing`, `an_unavailable_browser_latches_and_tells_the_user_once`, `engine_alerts` `a_key_is_delivered_once_until_reset`, `text_puts_the_remedy_in_its_own_block` |
| `test_search_quality` tests of other engines that mention Lens only in a shared fixture (`test_a_rejected_key_stands_down_for_the_batch`, `test_an_exhausted_quota_stands_down_too`, `test_a_per_image_error_is_not_latched`, `test_a_consent_wall_is_recognised_and_latched`, `test_an_unusual_traffic_check_is_recognised`, `test_a_plain_server_error_is_not_called_a_block`, `test_a_plain_403_is_not_called_a_bot_check`, `test_a_skipped_engine_is_never_cached_as_a_confident_not_found`, `test_existing_enabled_sites_gain_the_new_names`, `test_a_network_fault_alongside_it_still_is`, `test_search_brings_a_deep_booru_post_to_the_top`, `test_registered_and_found_by_find_parser`) | not Lens: they exercise SauceNAO, Yandex, the boorus and the registry, ported in 2.x and 3.x |
| `test_lens_browser`: `test_a_relaunch_that_fails_answers_the_waiting_search` | 5.b: it is the browser's relaunch |

## 6. Error shape (DAN-2116 item 1)

`LensError` has `kind()`, `user_message()` and `detail()`.
`an_unreadable_file_is_a_failed_image_whose_user_message_names_no_path` and
`every_error_has_a_kind_and_a_message_with_no_url_key_or_path_in_it` assert that `user_message`
carries no path, no OS error text, no URL query and no key. `Entry.error_message` and `MatchStatus`
are unchanged on disk.
