# Phase 6.b: the pure core modules of the window (DAN-2214)

Work in progress Rust rewrite of Hatate. This is the first Phase 6 ticket, lane A: core only, no widgets. Every module here lands in `hatate-core` so the window tickets call it rather than port it. Python source of truth: `Hatate-Linux-Redux` at `76c85c4`.

## What is ported and where the cut is

| Python (at `76c85c4`) | Rust (`hatate-core`) | Notes |
|---|---|---|
| `core/entry_filter.py` | `entry_filter.rs` | The filter bar's predicate. Not the MCP `_matches_filter` (ported in #50). |
| `core/eta.py`, and the status-strip helpers of `gui/main_window.py` | `eta.rs` | `RunEstimate`, `format_duration`, `format_finish_time`, `run_strip`, `wait_countdown_text`, `worker_waiting_text`. Every `round()` and `{:.0f}` sends an exact half to the even neighbour. |
| `core/export.py` | `export.rs` | CSV (`\r\n`, QUOTE_MINIMAL) and JSON (`indent=2`, `ensure_ascii=False`, no trailing newline). The JSON record's keys are in Python's dict order: the tag fields come last. |
| `core/sidecar.py` | `sidecar.rs` | Planning only: `sidecar_path`, `render`, `plan`. `write` is 6.d's. Names follow Python 3.14 `pathlib`. |
| `core/missing_files.py`, `workers/missing_file_worker.py` (logic) | `missing_files.rs` | `find_missing_paths`, `group_by_directory`, `check_session_files`, plus the loop and the two messages of the window's handler. |
| `core/viewport.py` | `viewport.rs` | Rows are `i64` because `-1` is a value (`rowAt` past the end). |
| `core/tag_colors.py` | `tag_colors.rs` |  |
| `core/shortcuts.py` | `shortcuts.rs` | Reads and writes `Settings.review_shortcuts`; the ids are held to `mcp::tools::REVIEW_ACTION_IDS` by a test. |
| `core/applog.py` (ring only) | `applog.rs` | `LogRing`, a cloneable handle holding the last 4000 formatted lines. The rotating file is `rotating_log.rs` (#8). |
| `gui/preview_text.py` | `preview_text.rs` | `image_opener` is the `ImageOpener` trait. `comparison_banner_colour` is the theme's; `comparison_readout` takes the glyph lookup as an argument. |
| `gui/settings_search.py` (`matches` only) | `settings_search.rs` | Generic over the widget the window indexes. The index over the dialog is 6.h's. |

Also changed in existing files, none of it on disk: `model.rs` gains `MatchStatus::label()` and `Hash`, `Entry::file_missing`, `Entry::stale_match_width/height` and `Entry::match_width()/match_height()` (the export writes them; the stale pair follows the existing `stale_*` fields that keep a cached result's dead match, set in `search_engine.rs` and cleared in `queue.rs::reset_entry`); `localtime.rs` gains `WallClock` and `local_wall_clock`; `lib.rs` registers the modules.

**Error shape (DAN-2116).** This ticket adds no error type: every function here is infallible (a path with no name, which makes Python raise `ValueError`, is reported with the missing files, and a non-finite duration never panics). There is nothing to attach `kind()`, `user_message()` and `detail()` to, and no test for the missing-file open message because nothing here opens a file for the user.

## Method map

Every `def` of the Python modules above, by `file:line` at `76c85c4`, with where it went. Defs of `gui/` and `gui/main_window.py` that are not listed belong to the tickets that build those surfaces; 6.j checks the union.

| Python def | Line | Rust, or the ticket that owns it |
|---|---|---|
| `core/entry_filter.py` `EntryFilter` | 37 | `entry_filter::EntryFilter` |
| `core/entry_filter.py` `EntryFilter.is_active` | 51 | `EntryFilter::is_active` |
| `core/entry_filter.py` `EntryFilter.matches` | 59 | `EntryFilter::matches` |
| `core/entry_filter.py` `EntryFilter.apply` | 72 | `EntryFilter::apply` (and `apply_indices`, the list positions of the same result) |
| `core/entry_filter.py` `site_of` | 80 | `entry_filter::site_of` |
| `core/entry_filter.py` `sites_present` | 93 | `entry_filter::sites_present` |
| `core/entry_filter.py` `statuses_present` | 103 | `entry_filter::statuses_present` |
| `core/entry_filter.py` `upscale_verdict_label` | 110 | `entry_filter::upscale_verdict_label` |
| `core/entry_filter.py` `upscale_verdicts_present` | 119 | `entry_filter::upscale_verdicts_present` |
| `core/eta.py` `RunEstimate` | 42 | `eta::RunEstimate` |
| `core/eta.py` `RunEstimate.__init__` | 49 | `RunEstimate::new` |
| `core/eta.py` `RunEstimate.reset` | 52 | `RunEstimate::reset` |
| `core/eta.py` `RunEstimate.record` | 63 | `RunEstimate::record` |
| `core/eta.py` `RunEstimate.intervals` | 82 | `RunEstimate::intervals` |
| `core/eta.py` `RunEstimate.seconds_per_image` | 86 | `RunEstimate::seconds_per_image` |
| `core/eta.py` `RunEstimate.seconds_remaining` | 96 | `RunEstimate::seconds_remaining` |
| `core/export.py` `_sent_state` | 50 | `export::sent_state` (private) |
| `core/export.py` `_cache_state` | 57 | `export::cache_state` (private) |
| `core/export.py` `_tag_strings` | 73 | folded into `export::row_values` (the `tags` field) |
| `core/export.py` `Field` | 77 | no struct: the field list is `export::HEADERS` and the match in `row_values` |
| `core/export.py` `_csv_value` | 117 | `export::csv_value` (private) |
| `core/export.py` `_row_values` | 130 | `export::row_values` |
| `core/export.py` `_exported_tags` | 141 | `model::tags_to_send` (called directly) |
| `core/export.py` `to_csv` | 147 | `export::to_csv` |
| `core/export.py` `_json_record` | 168 | `export::json_record` (private) |
| `core/export.py` `to_json` | 180 | `export::to_json` |
| `core/export.py` `format_for_path` | 200 | `export::format_for_path` |
| `core/export.py` `render` | 210 | `export::render` |
| `core/sidecar.py` `style_of` | 62 | `sidecar::style_of` |
| `core/sidecar.py` `overwrite_policy_of` | 74 | `sidecar::overwrite_policy_of` |
| `core/sidecar.py` `sidecar_path` | 80 | `sidecar::sidecar_path` |
| `core/sidecar.py` `render` | 89 | `sidecar::render` |
| `core/sidecar.py` `Planned` | 109 | `sidecar::Planned` |
| `core/sidecar.py` `Plan` | 118 | `sidecar::Plan` |
| `core/sidecar.py` `Plan.existing` | 130 | `Plan::existing` |
| `core/sidecar.py` `Plan.directories` | 135 | `Plan::directories` |
| `core/sidecar.py` `plan` | 141 | `sidecar::plan` |
| `core/sidecar.py` `Result` | 172 | left to 6.d (the write) |
| `core/sidecar.py` `_write_new` | 179 | left to 6.d (the write) |
| `core/sidecar.py` `_write_over` | 197 | left to 6.d (the write) |
| `core/sidecar.py` `write` | 223 | left to 6.d (the write) |
| `core/missing_files.py` `group_by_directory` | 28 | `missing_files::group_by_directory` |
| `core/missing_files.py` `find_missing_paths` | 36 | `missing_files::find_missing_paths` |
| `workers/missing_file_worker.py` `MissingFileWorker` | 14 | thread shell left to the window ticket that restores a session; the logic is `missing_files::check_session_files` |
| `workers/missing_file_worker.py` `MissingFileWorker.__init__` | 26 | left to the window ticket (thread shell) |
| `workers/missing_file_worker.py` `MissingFileWorker.stop` | 31 | left to the window ticket (thread shell: sets the flag `check_session_files` polls) |
| `workers/missing_file_worker.py` `MissingFileWorker.run` | 34 | `missing_files::check_session_files` |
| `core/viewport.py` `visible_range_with_buffer` | 12 | `viewport::visible_range_with_buffer` |
| `core/viewport.py` `ThumbnailWork` | 47 | `viewport::ThumbnailWork` |
| `core/viewport.py` `ThumbnailWork.examined` | 54 | `ThumbnailWork::examined` |
| `core/viewport.py` `entries_needing_thumbnails` | 58 | `viewport::entries_needing_thumbnails` |
| `core/tag_colors.py` `get_tag_color` | 44 | `tag_colors::get_tag_color` |
| `core/shortcuts.py` `ReviewAction` | 33 | `shortcuts::ReviewAction` |
| `core/shortcuts.py` `default_bindings` | 140 | `shortcuts::default_bindings` |
| `core/shortcuts.py` `resolve` | 145 | `shortcuts::resolve` |
| `core/shortcuts.py` `prune` | 166 | `shortcuts::prune` |
| `core/shortcuts.py` `normalize` | 181 | `shortcuts::normalize` |
| `core/shortcuts.py` `conflicts` | 191 | `shortcuts::conflicts` |
| `core/applog.py` `_RingHandler` | 22 | `applog::LogRing` (a cloneable handle instead of a process global) |
| `core/applog.py` `_RingHandler.emit` | 23 | `LogRing::push` / `LogRing::record` |
| `core/applog.py` `setup_logging` | 30 | not here: the rotating file is `rotating_log.rs` (#8); the ring is attached by the window (6.c) |
| `core/applog.py` `get_logger` | 60 | not here: Rust logs through `tracing` targets; the window (6.c) attaches the ring and the file |
| `core/applog.py` `get_recent_logs` | 69 | `LogRing::recent` |
| `core/applog.py` `get_log_file_path` | 73 | not here: `Paths::rust_log_file` (#8) |
| `gui/preview_text.py` `human_size` | 43 | `preview_text::human_size` |
| `gui/preview_text.py` `matched_caption` | 54 | `preview_text::matched_caption` |
| `gui/preview_text.py` `no_candidate_text` | 74 | `preview_text::no_candidate_text` |
| `gui/preview_text.py` `_dimensions` | 89 | `preview_text::dimensions` (private) |
| `gui/preview_text.py` `local_image_info_text` | 93 | `preview_text::local_image_info_text` |
| `gui/preview_text.py` `local_frame_meta` | 104 | `preview_text::local_frame_meta` |
| `gui/preview_text.py` `_local_info_parts` | 111 | `preview_text::local_info_parts` (private) |
| `gui/preview_text.py` `matched_frame_label` | 129 | `preview_text::matched_frame_label` |
| `gui/preview_text.py` `matched_frame_meta` | 143 | `preview_text::matched_frame_meta` |
| `gui/preview_text.py` `matched_image_info_text` | 160 | `preview_text::matched_image_info_text` |
| `gui/preview_text.py` `comparison_banner_text` | 193 | `preview_text::comparison_banner_text` |
| `gui/preview_text.py` `comparison_banner_tier` | 233 | `preview_text::comparison_banner_tier` |
| `gui/preview_text.py` `comparison_banner_colour` | 249 | left to the theme ticket (6.c): it is `theme.palette(mode)[tier]`, which needs the palette |
| `gui/preview_text.py` `ComparisonReadout` | 258 | `preview_text::ComparisonReadout` |
| `gui/preview_text.py` `comparison_readout` | 271 | `preview_text::comparison_readout` (the glyph lookup is an argument) |
| `gui/settings_search.py` `index_tabs` | 38 | left to 6.h (the index over the built widgets) |
| `gui/settings_search.py` `_name_of` | 60 | left to 6.h |
| `gui/settings_search.py` `matches` | 75 | `settings_search::matches` |
| `gui/settings_search.py` `highlight` | 98 | left to 6.h |
| `gui/main_window.py` `_format_duration` | 148 | `eta::format_duration` |
| `gui/main_window.py` `_format_finish_time` | 257 | `eta::format_finish_time` |
| `gui/main_window.py` `MainWindow._start_missing_file_check` | 1569 | left to the window ticket (thread shell) |
| `gui/main_window.py` `MainWindow._on_missing_files_checked` | 1589 | the loop and the two messages are `missing_files::flag_missing`, `missing_status_text`, `missing_dialog_text`; the handler (closing guard, refresh, modal box) is the window's |
| `gui/main_window.py` `MainWindow._on_worker_progress` | 2289 | left to the window ticket (calls `RunEstimate::record` and `run_strip`) |
| `gui/main_window.py` `MainWindow._refresh_run_progress_label` | 2295 | `eta::run_strip` (the text; painting the labels is the window's) |
| `gui/main_window.py` `MainWindow._idle_run_text` | 2327 | `eta::run_strip` (idle branch) |
| `gui/main_window.py` `MainWindow._on_worker_waiting` | 2336 | `eta::worker_waiting_text` |
| `gui/main_window.py` `MainWindow._on_wait_countdown` | 2339 | `eta::wait_countdown_text` |

## Oracle

**Direct differential** against Python at `76c85c4` (system Python 3.14.8, PyQt6 6.11): for each function above, generated inputs with the seed recorded, both sides run in this harness, outputs compared as parsed JSON, so strings, key order and floats are compared exactly.

- Harness (outside the repo, the tree is Rust only): `gen_py.py` builds the cases and runs the Python oracle, `zz_p6b_diff.rs` is the Rust runner copied into `tests/` for the run, `cmp.py` compares, `run_all.py SEED N` drives all three, `cfg_run.py SEED N` is the `config.json` byte comparison. Log: `worktrees/DAN-2214-differential.log`.
- Kinds: `entry_filter` (statuses, sites, verdicts, text, the menus), `eta` (run sequences with resets, durations incl. exact halves, finish times across the calendar incl. the datetime limit, the countdown and waiting lines, the run strip), `export` (CSV and JSON bytes in 7 time zones, settings variants, `format_for_path`), `sidecar` (path names for 31 odd paths in both styles, plans over a real temp tree with pre-existing targets), `missing_files` (a real tree with a broken symlink, an unreadable directory, a file used as a directory, stop after k polls, the worker), `viewport`, `tag_colors`, `shortcuts` (override maps with unknown ids, blanks, case-fold edge letters), `applog` (ring over 4000, formatted records), `preview_text` (files from 0 B to 3 GiB, an unreadable opener, readouts), `settings_search`.
- Seeds 2 and 7 at N=1500: **30,540 cases each, 0 differing.** Plus the `config.json` round trip: Python and Rust each load a fresh profile, set `review_shortcuts` to `prune(resolve(overrides))`, save, and the bytes and the mode are compared: 150 cases, 0 differing.
- The harness is sensitive: two deliberate Rust mutations (the ETA interval threshold, the export's `file_missing`) turned 126 of 2,130 cases red and were reverted.
- Differences: none. No tolerance and no skip list.

**Assumptions the oracle fixes, stated so a verifier can check them.**

- `pathlib`: the sidecar file name follows Python 3.14 (`Path('cat.').suffix` is `'.'`; earlier versions give `''`). `mcp/tool_handlers.rs::path_suffix` (#50) follows the older rule; the two are separate functions over separate Python calls and are not changed here.
- `strftime`: weekday and month names are the C locale's (`Mon`, `Mar`). A Qt app that has called `setlocale` would localise them in Python.
- A similarity that is an `int` in Python (only from a hand-edited session file) would print `95`, not `95.0`; the model stores `f64`, and every engine builds floats.
- `format_duration` on NaN or an infinity: Python raises; Rust gives `0s` or saturates. `format_finish_time` of an absurd horizon is `""` on both sides.

## Python test map

238 tests of `tests/` at `76c85c4` reference this surface, found by reading each test and the helpers it calls for the names of the functions above. The other 416 tests of `test_gui_smoke.py` and the rest of the suite drive window behaviour and are mapped by the window tickets (6.j checks the union).

| Python test | Rust test, differential, or why not |
|---|---|
| `test_core_utils.py::TestViewportRange.test_expands_by_the_buffer` | `viewport::tests::the_range_is_buffered_clamped_and_never_empty_for_a_swapped_pair`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_clamps_at_the_top` | `viewport::tests::the_range_is_buffered_clamped_and_never_empty_for_a_swapped_pair`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_clamps_at_the_bottom` | `viewport::tests::the_range_is_buffered_clamped_and_never_empty_for_a_swapped_pair`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_negative_last_means_past_the_final_row` | `viewport::tests::a_negative_end_is_the_last_row_and_a_negative_start_the_first`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_negative_first_means_top_of_list` | `viewport::tests::a_negative_end_is_the_last_row_and_a_negative_start_the_first`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_both_negative_covers_the_whole_short_list` | `viewport::tests::a_negative_end_is_the_last_row_and_a_negative_start_the_first`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_list_shorter_than_the_viewport` | `viewport::tests::a_single_row_and_a_short_list_are_covered_whole`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_single_row` | `viewport::tests::a_single_row_and_a_short_list_are_covered_whole`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_empty_list_returns_none` | `viewport::tests::the_range_is_buffered_clamped_and_never_empty_for_a_swapped_pair`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_swapped_pair_does_not_produce_an_empty_slice` | `viewport::tests::the_range_is_buffered_clamped_and_never_empty_for_a_swapped_pair`; differential `viewport` |
| `test_core_utils.py::TestViewportRange.test_range_stays_tiny_against_a_huge_list` | `viewport::tests::a_single_row_and_a_short_list_are_covered_whole`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_everything_is_needed_when_nothing_is_known` | `viewport::tests::a_row_being_made_is_not_work_to_do`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_a_cached_row_is_not_regenerated` | `viewport::tests::a_row_being_made_is_not_work_to_do`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_a_row_being_generated_is_not_queued_again` | `viewport::tests::a_row_being_made_is_not_work_to_do`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_cached_wins_over_in_flight` | `viewport::tests::cached_wins_over_in_flight`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_every_row_is_accounted_for_exactly_once` | `viewport::tests::every_row_is_accounted_for_once_and_identity_not_path_decides`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_two_rows_sharing_a_path_are_still_separate_work` | `viewport::tests::every_row_is_accounted_for_once_and_identity_not_path_decides`; differential `viewport` |
| `test_core_utils.py::TestThumbnailWorkSelection.test_no_rows_in_view_is_no_work` | `viewport::tests::every_row_is_accounted_for_once_and_identity_not_path_decides`; differential `viewport` |
| `test_core_utils.py::TestEntryFilter.test_an_empty_filter_matches_everything` | `entry_filter::tests::an_empty_filter_matches_everything_and_is_inactive`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_filtering_by_status` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_several_statuses_at_once` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_filtering_by_site` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_entries_with_no_match_have_their_own_site` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_filtering_by_filename_text` | `entry_filter::tests::text_is_a_trimmed_case_insensitive_filename_substring`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_text_matching_ignores_case_and_padding` | `entry_filter::tests::text_is_a_trimmed_case_insensitive_filename_substring`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_blank_text_is_not_a_filter` | `entry_filter::tests::an_empty_filter_matches_everything_and_is_inactive`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_filters_combine` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_an_empty_set_is_not_the_same_as_no_filter` | `entry_filter::tests::an_empty_set_is_not_the_same_as_no_filter`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_only_the_selected_candidates_site_counts` | `entry_filter::tests::only_the_selected_candidates_site_counts`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_filtering_by_upscale_verdict` | `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestEntryFilter.test_not_checked_is_its_own_upscale_verdict` | `entry_filter::tests::an_unknown_verdict_reads_as_not_checked`, `entry_filter::tests::status_site_and_verdict_each_narrow_the_list`; differential `entry_filter` |
| `test_core_utils.py::TestFilterOptionsOffered.test_only_present_sites_are_offered` | `entry_filter::tests::the_menus_offer_only_what_is_present_in_a_fixed_order`; differential `entry_filter` |
| `test_core_utils.py::TestFilterOptionsOffered.test_no_match_sorts_last` | `entry_filter::tests::the_menus_offer_only_what_is_present_in_a_fixed_order`; differential `entry_filter` |
| `test_core_utils.py::TestFilterOptionsOffered.test_statuses_are_offered_in_the_enums_order` | `entry_filter::tests::the_menus_offer_only_what_is_present_in_a_fixed_order`; differential `entry_filter` |
| `test_core_utils.py::TestFilterOptionsOffered.test_an_empty_list_offers_nothing` | `entry_filter::tests::the_menus_offer_only_what_is_present_in_a_fixed_order`; differential `entry_filter` |
| `test_core_utils.py::TestFilterOptionsOffered.test_upscale_verdicts_are_offered_worst_news_first` | `entry_filter::tests::the_menus_offer_only_what_is_present_in_a_fixed_order`; differential `entry_filter` |
| `test_core_utils.py::TestRunEstimate.test_says_nothing_until_it_has_two_intervals` | `eta::tests::nothing_is_said_until_two_intervals_have_passed`; differential `eta` |
| `test_core_utils.py::TestRunEstimate.test_the_leading_search_does_not_drag_the_pace_down` | `eta::tests::the_baseline_is_the_first_signal_and_the_total_is_reread`; differential `eta` |
| `test_core_utils.py::TestRunEstimate.test_it_follows_total_growing_when_retries_are_queued` | `eta::tests::the_baseline_is_the_first_signal_and_the_total_is_reread`; differential `eta` |
| `test_core_utils.py::TestRunEstimate.test_reset_forgets_the_previous_run` | `eta::tests::no_elapsed_time_is_unknown_and_reset_forgets`; differential `eta` |
| `test_core_utils.py::TestRunEstimate.test_a_finished_run_has_nothing_left` | `eta::tests::a_finished_run_has_nothing_left`; differential `eta` |
| `test_core_utils.py::TestReviewShortcutRegistry.test_the_shipped_defaults_do_not_collide` | `shortcuts::tests::conflicts_compare_case_and_space_insensitively_and_ignore_unbound`; differential `shortcuts` |
| `test_core_utils.py::TestReviewShortcutRegistry.test_every_action_has_a_default_and_a_label` | `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against`; differential `shortcuts` |
| `test_core_utils.py::TestReviewShortcutRegistry.test_an_override_wins_and_the_rest_stay_default` | `shortcuts::tests::resolve_drops_unknown_ids_strips_values_and_keeps_registry_order`; differential `shortcuts` |
| `test_core_utils.py::TestReviewShortcutRegistry.test_an_override_for_an_action_this_build_lacks_is_dropped` | `shortcuts::tests::resolve_drops_unknown_ids_strips_values_and_keeps_registry_order`; differential `shortcuts` |
| `test_core_utils.py::TestReviewShortcutRegistry.test_a_hand_edited_config_cannot_smuggle_a_non_string_through` | not applicable: `review_shortcuts` is an `OrderedMap<String>`; a config object holding a non-string value keeps the default for the whole key (Phase 1, `config_compat`) |
| `test_core_utils.py::TestReviewShortcutRegistry.test_only_real_changes_are_written_to_the_config` | `shortcuts::tests::prune_keeps_only_what_differs_from_the_default`, `shortcuts::tests::an_override_round_trips_through_config_json`; differential `shortcuts + config.json bytes` |
| `test_core_utils.py::TestResultsExport.test_csv_starts_with_a_header_naming_every_field` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_one_row_per_image` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_the_row_carries_the_result` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_a_comma_in_a_field_does_not_shift_the_columns` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_a_quote_in_a_field_survives` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_tags_carry_their_namespace` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_csv_line_endings_are_fixed_not_platform_dependent` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_booleans_render_as_a_spreadsheet_understands_them` | `export::tests::the_csv_is_quoted_crlf_and_in_header_order`, `export::tests::an_entry_with_no_match_leaves_the_match_columns_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_the_sent_column_uses_the_same_three_states_as_the_table` | `export::tests::the_sent_upscale_and_similarity_columns_say_what_the_table_says`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_the_upscale_verdict_column` | `export::tests::the_sent_upscale_and_similarity_columns_say_what_the_table_says`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_a_provisional_result_says_so_over_where_it_came_from` | `export::tests::provisional_wins_in_the_cache_column_and_zero_sizes_are_blank`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_an_empty_list_still_writes_the_header` | `export::tests::an_empty_list_is_an_empty_images_array`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_json_wraps_the_rows_with_a_count_and_a_timestamp` | `export::tests::the_json_record_puts_the_tag_fields_last_and_tags_as_objects`, `export::tests::json_escapes_only_what_it_must`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_json_keeps_tags_as_structured_values` | `export::tests::the_json_record_puts_the_tag_fields_last_and_tags_as_objects`, `export::tests::json_escapes_only_what_it_must`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_a_comma_in_a_tag_survives_json_but_not_csv` | `export::tests::the_json_record_puts_the_tag_fields_last_and_tags_as_objects`, `export::tests::json_escapes_only_what_it_must`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_both_formats_describe_the_same_fields` | `export::tests::the_json_record_puts_the_tag_fields_last_and_tags_as_objects`, `export::tests::the_csv_is_quoted_crlf_and_in_header_order`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_json_is_valid_for_an_empty_list` | `export::tests::an_empty_list_is_an_empty_images_array`; differential `export` |
| `test_core_utils.py::TestResultsExport.test_the_extension_picks_the_format_and_csv_is_the_fallback` | `export::tests::the_format_follows_the_extension`; differential `export` |
| `test_flat_surfaces.py::TestRegionsAreFlatHairlines.test_table_keeps_its_own_fill_inside_a_hairline` | not this surface (the name scan matched the widget's own `viewport()`/helper): table fill |
| `test_gui_smoke.py::TestMainWindowSignalHandlers.test_search_progress_handlers` | `eta::tests::the_run_strip_follows_the_state_of_the_run`, `eta::tests::nothing_is_said_until_two_intervals_have_passed`; the signal handler is the window's |
| `test_gui_smoke.py::TestMainWindowSignalHandlers.test_progress_label_shows_the_count_then_the_estimate` | `eta::tests::the_run_strip_follows_the_state_of_the_run`, `eta::tests::nothing_is_said_until_two_intervals_have_passed`; the signal handler is the window's |
| `test_gui_smoke.py::TestMainWindowSignalHandlers.test_a_finished_run_keeps_the_count_and_drops_the_estimate` | `eta::tests::the_run_strip_follows_the_state_of_the_run`, `eta::tests::nothing_is_said_until_two_intervals_have_passed`; the signal handler is the window's |
| `test_gui_smoke.py::TestWorkerCallbacksDuringClose.test_missing_files_checked_is_a_no_op_while_closing` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the closing guard of `_on_missing_files_checked` |
| `test_gui_smoke.py::TestDisplayHelpers.test_duration_formatting` | `eta::tests::durations_round_halves_to_even_like_python`; differential `eta duration` |
| `test_gui_smoke.py::TestDisplayHelpers.test_duration_formatting_reaches_days` | `eta::tests::durations_round_halves_to_even_like_python`; differential `eta duration` |
| `test_gui_smoke.py::TestDisplayHelpers.test_finish_time_phrasing_follows_the_distance` | `eta::tests::the_finish_time_is_phrased_by_distance`; differential `eta finish` |
| `test_gui_smoke.py::TestDisplayHelpers.test_an_absurd_horizon_yields_no_finish_time` | `eta::tests::the_finish_time_is_phrased_by_distance`; differential `eta finish` |
| `test_gui_smoke.py::TestPreviewText.test_the_caption_names_the_site_and_the_score` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_the_caption_omits_what_it_does_not_have` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_a_site_with_no_score_still_reads` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_not_searched_and_not_found_are_not_confused` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_searching_says_so` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_an_error_carries_its_message` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_an_error_with_no_message_does_not_say_none` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_match_info_omits_pieces_that_are_unknown` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_match_info_with_nothing_at_all_says_so` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_no_candidate_is_blank_not_a_placeholder` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_human_size_scales_units` | `preview_text::tests::sizes_are_human_sized_and_never_past_gb`; differential `preview_text human` |
| `test_gui_smoke.py::TestPreviewText.test_the_banner_is_empty_without_a_candidate` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_a_bigger_match_reads_as_good` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_a_smaller_match_is_not_green` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_the_banner_states_the_similarity` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`; differential `preview_text` |
| `test_gui_smoke.py::TestPreviewText.test_local_info_reports_unreadable_files_plainly` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk`; differential `preview_text` |
| `test_gui_smoke.py::TestNoTagsReasonIsShown.test_the_reason_appears_under_the_match_info` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestNoTagsReasonIsShown.test_it_is_on_its_own_line` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestNoTagsReasonIsShown.test_a_match_with_tags_shows_no_reason` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestNoTagsReasonIsShown.test_a_reason_still_shows_when_nothing_else_is_known` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestBorrowedTagsAreLabelled.test_the_preview_names_the_match_the_tags_came_from` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestBorrowedTagsAreLabelled.test_a_match_with_its_own_tags_says_nothing_extra` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text`; the label widget is the window's |
| `test_gui_smoke.py::TestReviewShortcuts.test_every_registered_action_has_a_handler` | window behaviour (a Phase 6 window ticket; 6.j checks the union): installing `QShortcut`s on the table; the registry is `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against` |
| `test_gui_smoke.py::TestReviewShortcuts.test_they_are_scoped_to_the_table_not_the_window` | window behaviour (a Phase 6 window ticket; 6.j checks the union): installing `QShortcut`s on the table; the registry is `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against` |
| `test_gui_smoke.py::TestReviewShortcuts.test_reinstalling_does_not_leave_the_old_binding_behind` | window behaviour (a Phase 6 window ticket; 6.j checks the union): installing `QShortcut`s on the table; the registry is `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against` |
| `test_gui_smoke.py::TestReviewShortcuts.test_a_cleared_binding_installs_no_shortcut` | window behaviour (a Phase 6 window ticket; 6.j checks the union): installing `QShortcut`s on the table; the registry is `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against` |
| `test_gui_smoke.py::TestReviewShortcuts.test_an_unparseable_binding_is_skipped_rather_than_fatal` | window behaviour (a Phase 6 window ticket; 6.j checks the union): installing `QShortcut`s on the table; the registry is `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against` |
| `test_gui_smoke.py::TestDynamicShortcutLabels.test_shortcuts_dialog_shows_all_actions_with_bindings` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the dialog; its rows are `shortcuts::REVIEW_ACTIONS` |
| `test_gui_smoke.py::TestShortcutsSettingsTab.test_it_offers_every_registered_action` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the settings tab (6.h); its rows are `shortcuts::REVIEW_ACTIONS` |
| `test_gui_smoke.py::TestWriteTagFiles.test_the_skip_setting_asks_once_and_keeps_existing_files` | left to 6.d (the action that asks, then writes; the plan is `sidecar::plan`) |
| `test_gui_smoke.py::TestWriteTagFiles.test_the_overwrite_setting_still_says_so_before_replacing` | left to 6.d (the action that asks, then writes; the plan is `sidecar::plan`) |
| `test_gui_smoke.py::TestWriteTagFiles.test_the_naming_style_setting_is_honoured` | left to 6.d (the action that asks, then writes; the plan is `sidecar::plan`) |
| `test_mcp_tools.py::TestReviewActionParity.test_every_mapped_id_is_a_real_review_action` | `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against`; the parity against the tool registry is `mcp::tools` (#50) |
| `test_mcp_tools.py::TestReviewActionParity.test_every_excluded_id_is_a_real_review_action` | `shortcuts::tests::the_registry_is_the_one_the_tool_surface_checks_against`; the parity against the tool registry is `mcp::tools` (#50) |
| `test_nothing_survived.py::TestRunStrip.test_it_sits_beside_the_idle_count` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; placement beside the idle count is the window's |
| `test_outcome_tags.py::TestSettingsDialogSection.test_the_section_is_findable_by_the_settings_search` | left to 6.h (the index over the built dialog) |
| `test_queue_view.py::TestQueueTableChrome.test_the_columns_fill_the_viewport_with_no_dead_strip` | not this surface (the name scan matched the widget's own `viewport()`/helper): table geometry |
| `test_queue_view.py::TestQueueTableChrome.test_the_default_widths_follow_the_window_until_the_user_resizes` | not this surface (the name scan matched the widget's own `viewport()`/helper): table geometry |
| `test_queue_view.py::TestQueueTableChrome.test_every_cell_draws_a_hairline_rule_under_it` | not this surface (the name scan matched the widget's own `viewport()`/helper): table geometry |
| `test_quota_pause_banner.py::TestBannerVariant.test_the_run_strip_puts_the_pause_note_after_the_counts` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the pause banner; the strip text it sits beside is `eta::run_strip` (quota-paused branch, `the_run_strip_follows_the_state_of_the_run`) |
| `test_quota_pause_banner.py::TestActions.test_continue_launches_without_saucenao_and_clears_the_banner` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the banner's actions |
| `test_quota_pause_banner.py::TestStrip.test_the_strip_reads_paused_the_count_and_eta_paused` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; the quota-paused branch; wiring to the pause state is the window's |
| `test_quota_pause_banner.py::TestStrip.test_the_run_ending_does_not_wipe_the_pause_from_the_strip` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; the quota-paused branch; wiring to the pause state is the window's |
| `test_quota_pause_banner.py::TestStrip.test_after_a_relaunch_the_strip_still_says_paused` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; the quota-paused branch; wiring to the pause state is the window's |
| `test_quota_pause_banner.py::TestStrip.test_an_ordinary_idle_strip_is_unchanged` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; the quota-paused branch; wiring to the pause state is the window's |
| `test_review_frames.py::TestCaptionText.test_local_meta_is_the_mockups_line` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk`; differential `preview_text` |
| `test_review_frames.py::TestCaptionText.test_local_meta_is_empty_for_an_unreadable_file` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk`; differential `preview_text` |
| `test_review_frames.py::TestCaptionText.test_a_measured_match_is_named_by_its_site_alone` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; differential `preview_text` |
| `test_review_frames.py::TestCaptionText.test_an_unmeasured_score_keeps_its_qualifier` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; differential `preview_text` |
| `test_review_frames.py::TestCaptionText.test_a_match_with_no_site_and_no_score_is_just_match` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; differential `preview_text` |
| `test_review_frames.py::TestCaptionText.test_match_meta_uses_the_caption_separators` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; differential `preview_text` |
| `test_review_frames.py::TestFramesAndCaptions.test_an_unmeasured_match_keeps_its_ranking_qualifier_in_the_caption` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; differential `preview_text`; the frame widget is the window's |
| `test_review_frames.py::TestFramesAndCaptions.test_no_match_says_so_in_the_caption` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; differential `preview_text`; the frame widget is the window's |
| `test_review_frames.py::TestFramesAndCaptions.test_a_long_caption_elides_instead_of_widening_the_window` | window behaviour (a Phase 6 window ticket; 6.j checks the union): eliding a caption in a frame is layout |
| `test_review_frames.py::TestViewportBudget.test_the_picture_gets_at_least_55_percent_of_the_window_height` | not this surface (the name scan matched the widget's own `viewport()`/helper): picture height budget of the review layout |
| `test_review_readout.py::TestReadoutParts.test_a_measured_score_reads_similarity_with_a_good_glyph` | `preview_text::tests::the_readout_splits_the_banner_into_parts`; differential `preview_text` |
| `test_review_readout.py::TestReadoutParts.test_a_ranking_never_says_similarity` | `preview_text::tests::the_readout_splits_the_banner_into_parts`; differential `preview_text` |
| `test_review_readout.py::TestReadoutParts.test_a_smaller_match_draws_no_up_arrow` | `preview_text::tests::the_readout_splits_the_banner_into_parts`; differential `preview_text` |
| `test_review_readout.py::TestReadoutParts.test_no_candidate_says_nothing` | `preview_text::tests::the_readout_splits_the_banner_into_parts`; differential `preview_text` |
| `test_review_readout.py::TestReadoutParts.test_the_tooltip_keeps_what_the_banner_said` | `preview_text::tests::the_readout_splits_the_banner_into_parts`; differential `preview_text` |
| `test_review_readout.py::TestReadoutInTheHeader.test_the_readout_is_28px_and_shows_every_part` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the readout's size and layout in the compare header |
| `test_review_readout.py::TestReadoutInTheHeader.test_the_readout_fits_the_compare_column` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the readout's size and layout in the compare header |
| `test_review_readout.py::TestReadoutInTheHeader.test_zoom_floats_inside_the_frame_not_in_the_header` | window behaviour (a Phase 6 window ticket; 6.j checks the union): the readout's size and layout in the compare header |
| `test_review_tag_rail.py::TestReviewTagRail.test_rows_are_32px_and_at_least_five_full_rows_show_at_1440x900` | not this surface (the name scan matched the widget's own `viewport()`/helper): tag rail geometry |
| `test_review_tag_rail.py::TestReviewTagRail.test_the_marker_is_dimmed_the_tag_is_bright_and_each_row_is_ruled` | not this surface (the name scan matched the widget's own `viewport()`/helper): tag rail geometry |
| `test_run_strip.py::TestStripGeometry.test_eta_is_right_aligned_and_last` | window behaviour (a Phase 6 window ticket; 6.j checks the union): label alignment |
| `test_run_strip.py::TestStripVoice.test_mockup_vocabulary_while_running` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestStripVoice.test_gauge_means_searched_and_sent_is_a_separate_count` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestStripVoice.test_eta_reads_in_its_own_readout` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestStripVoice.test_figures_are_bold_and_labels_are_mono` | window behaviour (a Phase 6 window ticket; 6.j checks the union): fonts |
| `test_run_strip.py::TestStripVoice.test_a_new_reading_leaves_no_old_text_under_it` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestIdleStrip.test_empty_window_reads_nothing_queued_eta_dash` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestIdleStrip.test_a_fresh_window_is_not_blank_before_anything_refreshes` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestIdleStrip.test_listed_but_unsearched_images_are_not_called_nothing_queued` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_run_strip.py::TestIdleStrip.test_a_finished_run_keeps_its_count_and_drops_the_eta` | `eta::tests::the_run_strip_follows_the_state_of_the_run`; differential `eta strip`; painting the labels is the window's |
| `test_search_quality.py::TestAPartialSearchIsNotAnError.test_the_preview_says_why` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`; differential `preview_text` |
| `test_selected_row_contrast.py::TestSelectedRowStaysLegible.test_dark` | not this surface (the name scan matched the widget's own `viewport()`/helper): selected-row contrast |
| `test_selected_row_contrast.py::TestSelectedRowStaysLegible.test_light` | not this surface (the name scan matched the widget's own `viewport()`/helper): selected-row contrast |
| `test_sent_tag_filter.py::TestTheExport.test_off_by_default_the_export_carries_every_tag` | `export::tests::the_tags_are_the_ones_a_send_would_carry`; differential `export (settings variants)` |
| `test_sent_tag_filter.py::TestTheExport.test_on_the_export_drops_the_unticked_sources` | `export::tests::the_tags_are_the_ones_a_send_would_carry`; differential `export (settings variants)` |
| `test_sent_tag_filter.py::TestTheExport.test_tag_count_matches_the_tags_actually_written` | `export::tests::the_tags_are_the_ones_a_send_would_carry`; differential `export (settings variants)` |
| `test_sent_tag_filter.py::TestTheExport.test_the_column_order_is_unchanged` | `export::tests::the_tags_are_the_ones_a_send_would_carry`; differential `export (settings variants)` |
| `test_session.py::TestMissingFileCheck.test_reports_only_the_deleted_files` | `missing_files::tests::a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing`; differential `missing_files` |
| `test_session.py::TestMissingFileCheck.test_present_files_are_never_reported` | `missing_files::tests::a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing`; differential `missing_files` |
| `test_session.py::TestMissingFileCheck.test_unreadable_directory_means_all_its_files` | `missing_files::tests::an_unreadable_directory_counts_all_its_files_and_nothing_is_nothing`; differential `missing_files` |
| `test_session.py::TestMissingFileCheck.test_groups_by_directory_to_cut_round_trips` | `missing_files::tests::paths_are_grouped_by_directory_in_first_seen_order`; differential `missing_files` |
| `test_session.py::TestMissingFileCheck.test_empty_input` | `missing_files::tests::an_unreadable_directory_counts_all_its_files_and_nothing_is_nothing`; differential `missing_files` |
| `test_session.py::TestMissingFileCheck.test_stop_check_abandons_the_scan` | `missing_files::tests::a_stop_returns_what_was_found_so_far_and_the_worker_reports_nothing`; differential `missing_files` |
| `test_settings_search.py::TestMatching.test_words_match_in_any_order` | `settings_search::tests::every_word_must_match_in_any_order`; differential `settings_search` |
| `test_settings_search.py::TestMatching.test_the_tab_name_counts_as_part_of_the_name` | `settings_search::tests::the_tab_name_is_part_of_what_an_entry_is_called`; differential `settings_search` |
| `test_settings_search.py::TestMatching.test_an_empty_query_matches_nothing` | `settings_search::tests::an_empty_query_matches_nothing_and_case_is_folded`; differential `settings_search` |
| `test_settings_search.py::TestMatching.test_shorter_names_come_first` | `settings_search::tests::the_tab_name_is_part_of_what_an_entry_is_called`, `settings_search::tests::equal_lengths_sort_by_name`; differential `settings_search` |
| `test_settings_search.py::TestMatching.test_matching_ignores_case` | `settings_search::tests::an_empty_query_matches_nothing_and_case_is_folded`; differential `settings_search` |
| `test_settings_search.py::TestHighlightReentrancy.test_a_second_highlight_inside_the_revert_window_still_restores` | left to 6.h (the highlight) |
| `test_settings_search.py::TestIndexingTheRealDialog.test_every_tab_is_reachable_by_its_own_name` | left to 6.h (the index over the built dialog) |
| `test_settings_search.py::TestIndexingTheRealDialog.test_explanations_are_not_indexed_as_names` | left to 6.h (the index over the built dialog) |
| `test_settings_search.py::TestIndexingTheRealDialog.test_a_known_setting_is_findable` | left to 6.h (the index over the built dialog) |
| `test_similarity_marking.py::TestTheExportSaysWhichNumbersAreReal.test_the_companion_field_sits_with_the_number` | `export::tests::the_sent_upscale_and_similarity_columns_say_what_the_table_says`; differential `export` |
| `test_similarity_marking.py::TestTheExportSaysWhichNumbersAreReal.test_a_measured_and_an_ordinal_row_export_differently` | `export::tests::the_sent_upscale_and_similarity_columns_say_what_the_table_says`; differential `export` |
| `test_similarity_marking.py::TestTheExportSaysWhichNumbersAreReal.test_a_row_with_no_match_exports_blank_rather_than_false` | `export::tests::the_sent_upscale_and_similarity_columns_say_what_the_table_says`; differential `export` |
| `test_similarity_marking.py::TestThePreviewPanelSaysIt.test_a_measured_caption_is_unchanged` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`; differential `preview_text` |
| `test_similarity_marking.py::TestThePreviewPanelSaysIt.test_an_ordinal_caption_says_so_in_words` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`; differential `preview_text` |
| `test_similarity_marking.py::TestThePreviewPanelSaysIt.test_the_banner_marks_a_ranking` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`; differential `preview_text` |
| `test_tag_sidecars.py::TestSidecarPath.test_append_style_keeps_the_image_extension` | `sidecar::tests::append_adds_the_extension_and_replace_swaps_the_suffix`, `sidecar::tests::the_path_is_normalised_the_way_pathlib_does`; differential `sidecar paths` |
| `test_tag_sidecars.py::TestSidecarPath.test_replace_style_drops_it` | `sidecar::tests::append_adds_the_extension_and_replace_swaps_the_suffix`, `sidecar::tests::the_path_is_normalised_the_way_pathlib_does`; differential `sidecar paths` |
| `test_tag_sidecars.py::TestSidecarPath.test_two_images_differing_only_in_extension_collide_under_replace` | `sidecar::tests::append_adds_the_extension_and_replace_swaps_the_suffix`, `sidecar::tests::the_path_is_normalised_the_way_pathlib_does`; differential `sidecar paths` |
| `test_tag_sidecars.py::TestSidecarPath.test_a_name_with_dots_in_it_is_not_truncated` | `sidecar::tests::append_adds_the_extension_and_replace_swaps_the_suffix`, `sidecar::tests::the_path_is_normalised_the_way_pathlib_does`; differential `sidecar paths` |
| `test_tag_sidecars.py::TestSidecarPath.test_an_unrecognised_style_setting_falls_back_to_the_default` | `sidecar::tests::an_unrecognised_setting_falls_back_to_the_default`; differential `sidecar` |
| `test_tag_sidecars.py::TestSidecarPath.test_an_unrecognised_overwrite_setting_falls_back_to_asking` | `sidecar::tests::an_unrecognised_setting_falls_back_to_the_default`; differential `sidecar` |
| `test_tag_sidecars.py::TestSidecarPath.test_the_shipped_defaults_are_the_safe_ones` | `sidecar::tests::planning_writes_nothing_and_the_shipped_defaults_are_the_safe_ones` |
| `test_tag_sidecars.py::TestRender.test_one_tag_per_line_newline_terminated` | `sidecar::tests::the_body_is_one_deduplicated_tag_per_line_in_the_entrys_order`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_namespaces_are_written_the_way_they_are_displayed` | `sidecar::tests::the_body_uses_display_names_and_the_same_filter_as_a_send`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_duplicates_are_dropped_keeping_the_first` | `sidecar::tests::the_body_is_one_deduplicated_tag_per_line_in_the_entrys_order`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_order_is_the_entrys_own` | `sidecar::tests::the_body_is_one_deduplicated_tag_per_line_in_the_entrys_order`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_no_tags_renders_nothing_at_all` | `sidecar::tests::the_body_is_one_deduplicated_tag_per_line_in_the_entrys_order`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_the_written_list_is_exactly_what_a_hydrus_send_would_carry` | `sidecar::tests::the_body_uses_display_names_and_the_same_filter_as_a_send`; differential `sidecar` |
| `test_tag_sidecars.py::TestRender.test_no_settings_means_every_tag` | `sidecar::tests::the_body_uses_display_names_and_the_same_filter_as_a_send`; differential `sidecar` |
| `test_tag_sidecars.py::TestPlan.test_planning_writes_nothing` | `sidecar::tests::planning_writes_nothing_and_the_shipped_defaults_are_the_safe_ones` |
| `test_tag_sidecars.py::TestPlan.test_an_entry_with_no_tags_is_reported_not_written_empty` | `sidecar::tests::the_plan_separates_writes_from_the_three_reasons_not_to`; differential `sidecar` |
| `test_tag_sidecars.py::TestPlan.test_a_missing_file_gets_no_sidecar` | `sidecar::tests::the_plan_separates_writes_from_the_three_reasons_not_to`; differential `sidecar` |
| `test_tag_sidecars.py::TestPlan.test_an_existing_target_is_flagged_but_still_planned` | `sidecar::tests::the_plan_separates_writes_from_the_three_reasons_not_to`; differential `sidecar` |
| `test_tag_sidecars.py::TestPlan.test_directories_lists_each_folder_once` | `sidecar::tests::the_plan_separates_writes_from_the_three_reasons_not_to`; differential `sidecar` |
| `test_tag_sidecars.py::TestPlan.test_the_plan_honours_the_naming_style_setting` | `sidecar::tests::the_plan_separates_writes_from_the_three_reasons_not_to`; differential `sidecar` |
| `test_tag_sidecars.py::TestWrite.test_writes_the_file_beside_the_image` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_line_endings_are_lf_on_every_platform` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_an_existing_file_is_left_exactly_alone_by_default` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_overwrite_replaces_it` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_an_existing_file_appearing_between_plan_and_write_is_not_clobbered` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_overwriting_leaves_no_temporary_files_behind` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_an_overwrite_that_fails_part_way_leaves_the_old_file_intact` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_the_written_file_is_not_world_readable_by_accident` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_tag_sidecars.py::TestWrite.test_one_unwritable_target_does_not_abandon_the_rest` | left to 6.d: `write`, `_write_new` and `_write_over` are not ported here (planning only) |
| `test_thin_modules.py::TestTagColors.test_nothing_is_colored_while_the_feature_is_off` | `tag_colors::tests::nothing_is_coloured_unless_the_setting_is_on`, `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_a_known_namespace_gets_its_documented_default` | `tag_colors::tests::nothing_is_coloured_unless_the_setting_is_on`, `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_the_booru_spelling_of_a_namespace_gets_the_same_color` | `tag_colors::tests::nothing_is_coloured_unless_the_setting_is_on`, `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_an_unknown_namespace_gets_no_color_rather_than_the_unnamespaced_one` | `tag_colors::tests::nothing_is_coloured_unless_the_setting_is_on`, `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_an_override_wins_over_the_built_in_default` | `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_an_override_can_name_a_namespace_that_has_no_default` | `tag_colors::tests::a_namespace_takes_its_override_then_its_default`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_an_unnamespaced_tag_gets_the_unnamespaced_color` | `tag_colors::tests::an_unnamespaced_tag_takes_the_blank_override_or_the_fallback`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_the_unnamespaced_color_is_overridden_under_the_empty_key` | `tag_colors::tests::an_unnamespaced_tag_takes_the_blank_override_or_the_fallback`; differential `tag_colors` |
| `test_thin_modules.py::TestTagColors.test_every_default_is_a_six_digit_hex_string` | `tag_colors::tests::every_default_is_a_six_digit_hex_string` |
| `test_thin_modules.py::TestApplog.test_setup_attaches_a_file_handler_and_the_ring_buffer` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_setup_is_idempotent` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_the_startup_line_names_the_file_it_writes_to` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_the_ring_buffer_holds_formatted_records` | `applog::tests::a_record_is_stored_as_the_line_the_file_carries`; differential `applog record` |
| `test_thin_modules.py::TestApplog.test_the_ring_buffer_is_bounded` | `applog::tests::the_ring_keeps_the_newest_lines_and_joins_them_with_newlines`, `applog::tests::a_clone_shares_the_ring_and_the_default_size_is_the_pythons`; differential `applog ring` |
| `test_thin_modules.py::TestApplog.test_a_record_that_cannot_be_formatted_does_not_raise` | not applicable: `LogRing::push` takes a line already formatted and cannot fail; formatting is `RotatingLog::format_line` |
| `test_thin_modules.py::TestApplog.test_get_logger_returns_the_root_app_logger_unprefixed` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_get_logger_prefixes_a_bare_name` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_get_logger_leaves_an_already_prefixed_name_alone` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_modules.py::TestApplog.test_the_log_file_lives_under_the_config_directory` | not applicable here: handler setup, `get_logger` naming and the log file location are Python `logging` machinery; the Rust file is `rotating_log.rs` (#8) and the window attaches the ring (6.c) |
| `test_thin_workers.py::TestMissingFileWorker.test_the_missing_paths_are_reported_and_the_present_ones_are_not` | `missing_files::tests::a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing`; differential `missing_files worker` |
| `test_thin_workers.py::TestMissingFileWorker.test_an_empty_session_still_emits_once` | not applicable: the signal emitted from a `QThread` is the window's thread shell; `check_session_files` returns the set |
| `test_thin_workers.py::TestMissingFileWorker.test_nothing_missing_emits_an_empty_set_rather_than_None` | not applicable: the signal emitted from a `QThread` is the window's thread shell; `check_session_files` returns the set |
| `test_thin_workers.py::TestMissingFileWorker.test_stop_sets_the_flag` | not applicable: the signal emitted from a `QThread` is the window's thread shell; `check_session_files` returns the set |
| `test_thin_workers.py::TestMissingFileWorker.test_a_cancelled_check_reports_nothing_missing_rather_than_a_partial_answer` | `missing_files::tests::a_stop_returns_what_was_found_so_far_and_the_worker_reports_nothing`; differential `missing_files worker` |
| `test_thin_workers.py::TestMissingFileWorker.test_the_stop_flag_is_handed_to_the_scan_itself` | `missing_files::tests::a_stop_returns_what_was_found_so_far_and_the_worker_reports_nothing`; differential `missing_files worker` |
| `test_thin_workers.py::TestMissingFileWorker.test_paths_are_passed_through_in_order` | `missing_files::tests::a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing`; differential `missing_files worker` |
| `test_thin_workers.py::TestMissingFileWorker.test_an_unreadable_directory_counts_all_of_its_files_as_missing` | `missing_files::tests::a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing`; differential `missing_files worker` |
| `test_thin_workers.py::TestHydrusTagLookupWorker.test_the_count_of_newly_tagged_entries_is_emitted` | not this surface (the name scan matched the widget's own `viewport()`/helper): that worker is `hydrus_tag_lookup`, ported in #33/#36; its thread shell is the window's |
| `test_thin_workers.py::TestHydrusTagLookupWorker.test_the_entries_and_settings_are_passed_straight_through` | not this surface (the name scan matched the widget's own `viewport()`/helper): that worker is `hydrus_tag_lookup`, ported in #33/#36; its thread shell is the window's |
| `test_thin_workers.py::TestHydrusTagLookupWorker.test_an_empty_batch_still_emits_zero` | not this surface (the name scan matched the widget's own `viewport()`/helper): that worker is `hydrus_tag_lookup`, ported in #33/#36; its thread shell is the window's |
| `test_thin_workers.py::TestHydrusTagLookupWorker.test_a_failure_emits_zero_rather_than_leaving_the_gui_waiting` | not this surface (the name scan matched the widget's own `viewport()`/helper): that worker is `hydrus_tag_lookup`, ported in #33/#36; its thread shell is the window's |
| `test_thin_workers.py::TestHydrusTagLookupWorker.test_even_a_bare_exception_is_caught` | not this surface (the name scan matched the widget's own `viewport()`/helper): that worker is `hydrus_tag_lookup`, ported in #33/#36; its thread shell is the window's |
