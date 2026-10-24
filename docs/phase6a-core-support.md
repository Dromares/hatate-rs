# Phase 6.a: core support for the window (DAN-2216)

Work in progress Rust rewrite of Hatate. Lane A, second Phase 6 ticket: core only, no widgets. Python source of truth: `Hatate-Linux-Redux` at `76c85c4`. Design authority: `phase6-calls` on DAN-2213 (calls 1, 2 and 4) and Cloud's rulings on DAN-2224.

## What is ported and where the cut is

| Python (at `76c85c4`) | Rust (`hatate-core`) | Notes |
|---|---|---|
| `core/models.py` `MatchStatus.sort_rank`; `gui/theme.py` glyph, weight, chip and glyph-font tables; `gui/table_delegates.py` `BOXED_CHIPS` | `status_tier.rs` `StatusTier` | Six tiers, one per `MatchStatus`. The tier depends on `status` alone. **There is no `Flagged` tier** and the rank is Python's (Good 0 ... Unsearched 5), not the order in the plan's table (`phase6-calls` premises 2 and 3). |
| `gui/image_table_model.py` Upscale, Sent and Reviewed columns | `status_tier.rs` `UpscaleTier`, `SentTier`, `reviewed_rank` | Independent of the Status tier. |
| `workers/search_worker.py` signals, `gui/main_window.py` handlers for them | `run_state.rs` `RunChannel` (a `batch::Reporter`), `RunState`, `RunEnd`, `RunEvent` | Four new default-empty `Reporter` hooks (`searching`, `progress`, `paused_for_quota`, `finished`), called by the loop. `hatate batch` does not use the channel; its output and exit codes are unchanged. |
| `gui/main_window.py` `_restore_saved_session`, `_interrupted_prose` | `stop_record.rs` `launch_banner`, `RestoreCounts`, `counts_clause`, `crashed_body` | The launch decision as a pure function over what the restore read. |
| (new, Rust-only: DAN-2224) | `stop_record.rs` `StopRecordFile` | The persisted stop record, in `hatate-rs.json`. |
| `gui/main_window.py` `THUMB_*`, `workers/thumbnail_worker.py` sizing | `thumb.rs` | `ThumbState`, `ThumbKey`, the size contract, `fit_within`, `ThumbBook` (the LRU the window's texture cache sits in). The decoding, the worker and the textures are 6.d's. |
| Phase 3 error types | `kind()`, `user_message()`, `detail()` on 22 more types | See "Error shape". Nothing is stored differently. |
| `gui/mcp_bridge.py` `_queue_row`, `_entry_detail` | a pinning test only | `mcp_tool_handlers.rs::list_queue_and_get_entry_rows_have_exactly_pythons_keys` (the 5.e verifier's P2 note). |

Not here, and who owns it: the banners' copy and widgets (6.d and 6.e); the four renamed strings (DAN-2224 decision 1: 6.d's string table, each with a comment pointing at its Python line); the run strip's text (6.b's `eta::run_strip`, fed by `RunState`); the worker bridge (6.c); the thumbnail decode and texture (6.d). Nothing in the window is claimed.

## Error shape (DAN-2116 item 1)

`Display` of every type below is unchanged: it is Python's `str(exc)`, which `Entry.error_message` keeps on disk byte for byte. The new methods are side channels for the window: `user_message()` says what failed and what to do with no path, URL, query, key or OS text, and `detail()` is the raw text for a log. Two new `ErrorKind`s, `Timeout` and `InvalidUrl`; no other kind moved.

| Type | `kind()` |
|---|---|
| `pyimage::DecompressionBomb`, `pyraster::OpenError::Bomb` | `ImageTooLarge` |
| `pyraster::OpenError::Os` | `ImageUndecodable` |
| `remote::Raised` | `Timeout` (hard timeout), `InvalidUrl` (value and unicode errors), `DownloadFailed` (body read) |
| `engine_runner::Raised` | `Unexpected` |
| `engine_runner::IndexError` | `Database` |
| `pyurl::UrlValueError` | `InvalidUrl` |
| `hard_timeout::HardTimeoutError` | `Timeout` |
| `multipage_resolve::ResolveError`, `similarity_check::SimilarityError`, `search_engine::SearchError` | the wrapped type's |
| `IqdbError`, `SauceNaoError`, `TraceMoeError`, `PawchiveLookupError` | `EngineFailed` |
| `PawchiveError` | the wrapped type's |
| `Ascii2dError`, `GoogleImagesError`, `YandexError` | `EngineFailed`, `EngineBlocked`, `Unexpected` |
| `engines::UnexpectedInput` | `Unexpected` |
| `boorus::BooruError` | `DownloadFailed` (fetch), `NotFound` (content gone) |

Already had the split before this ticket, and are in the same test: `HydrusError`, `LensError`, `LensFetchError`, `InvokeError`, `StagingError`, `RunFault`, `UrlImportError`, `DiffError`, `McpError`, `PawchiveIndexError`, `PrepareError`, `StepError`.

Tests: `error_shape_tests.rs` builds 46 variants whose text holds a path, a URL query with a key and an OS error, and asserts that no `user_message` shows any of them, that `detail()` keeps the raw text, that `Display` did not move, and that a missing-file open names neither the path nor `os error`. It includes the `InvokeError::Timeout` (`McpQueueTimeout`) variant, the 5.e verifier's second P2 note.

## Status tiers (`phase6-calls` call 1)

| Rank | `MatchStatus` | Tier | Chip | Glyph | Font | Ink | Bold | Italic | Boxed |
|---|---|---|---|---|---|---|---|---|---|
| 0 | Good | Good | Good | U+25CF | mono | ink_65 | no | no | no |
| 1 | Poor | Review | Review | U+25D0 | Ryoku Status | ink_100 | yes | no | no |
| 2 | NotFound | Not found | Not found | U+25CB | Ryoku Status | ink_100 | yes | no | no |
| 3 | Error | Error | Error | U+00D7 | mono | ink_100 | yes | no | yes |
| 4 | Searching | Searching | Searching | U+25D0 | Ryoku Status | ink_65 | no | yes | no |
| 5 | NotSearched | Unsearched | Unsearched | U+00B7 | mono | ink_65 | no | no | no |

`UpscaleTier` Flagged 0, Clear 1, NotChecked 2 (`None`, `""` and any other string). `SentTier` NotSent 0, Queued 1, Sent 2. Reviewed 0 or 1. Test: `status_tier::tests` (36 cells plus unknown verdicts, the 6.0 table, the sort ranks).

**One deliberate difference from Python, granted by the design call:** Review and Not found are bold (Python bolds only Error; `phase6-calls` 1d). It is a pixel property; the differential compares glyph, font, ink, text, italic, boxed and rank, and reports bold for those two tiers as the granted difference.

## RunState and the channel (`phase6-calls` call 2, DAN-2183, DAN-2224)

`RunState` is `Idle`, `Running(Progress)`, `Waiting(Wait)`, `PausedForQuota`, or `Ended(RunEnd)`. `RunEnd` is `Completed`, `Interrupted`, `Crashed`, `Failed(user_message)` or `FailedEmpty`. `LensConnection` travels on the same `RunSink` as `RunEvent::Lens`, through `RunChannel::lens_sink()` into `LensClient::with_connection_sink`.

| Python signal, in the worker's order | Reporter hook | State |
|---|---|---|
| `image_updated` (status Searching) | `searching(entry, done, total)` | `Running` with `current` |
| `progress(done, total)` | `progress(done, total)` | `Running` with `done` and the ETA from `RunEstimate` |
| `wait_countdown(label, remaining, total)` (and `waiting`) | `countdown(label, remaining, total)` | `Waiting`; `("", 0, 0)` goes back to `Running` |
| `paused_out_of_quota(searched, remaining)` | `paused_for_quota(searched, remaining)` | `PausedForQuota` |
| `finished_all` | `finished(&Ending)` | Finished: `Completed`. Stopped: `Interrupted`. Failed: `Failed(fault.user_message())`. PausedForQuota: no new state. A stop that arrives during the last image reads as Stopped, as `run_batch` reports it. |

`Crashed` and `FailedEmpty` are launch states (`stop_record::launch_banner`): the crash marker was found; Crashed with at least one restored row, FailedEmpty with a session file and none; with no session file nothing (B1). Core cannot tell a kill from a panic.

Each variant is reached from a test: `batch_run.rs` (Completed, Running, Waiting with the ETA, Interrupted, PausedForQuota, Failed; the Failed run kills the worker on its staging directory, and its message holds no `/` and no `os error`), `run_state_launch.rs` (Interrupted, Crashed, FailedEmpty, from the real crash marker and real session files), `run_state_lens.rs` (all four `LensConnection` states from a scripted browser, in order with the run's states).

## The persisted stop record (DAN-2224 decision 2)

`hatate-rs.json`, key `stop_record`: `{version: 1, ended: "stopped", at, unsearched}`. The constraints, and the test for each:

| # | Constraint | Test |
|---|---|---|
| 1 | Rust-only: Python's session and config files are byte-identical | `batch_run::a_stop_says_interrupted_and_leaves_a_record_in_hatate_rs_json_only`, `run_state_launch::no_launch_rule_touches_the_python_files` |
| 2 | Written on `Ending::Stopped` only, atomically; a failed write is non-fatal | `batch_run::a_stop_that_cannot_be_recorded_is_still_a_stop`; the other endings leave no record: `a_completed_run_says_...`, `an_exhausted_quota_is_a_paused_state_...`, `a_worker_that_dies_is_failed_...` |
| 3 | Cleared when a run starts, on Resume Queue, on Dismiss | `batch_run::starting_a_run_clears_the_stop_record`; `RunChannel::clear_stop_record` and `StopRecordFile::clear` for the two actions (6.d calls them) |
| 4 | The marker wins | `run_state_launch::the_marker_wins_over_the_record_and_the_record_is_cleared` |
| 5 | A trigger, not a source of counts; no unsearched row, no file or an unreadable one: no banner, record cleared | `run_state_launch::a_clean_launch_after_a_stop_...` (the record says 99, the banner counts the session), `a_record_with_nothing_left_to_search_or_no_session_...` |
| 6 | Malformed or unknown version: ignored and cleared | `stop_record::tests::malformed_and_unknown_version_records_are_ignored`, `run_state_launch::a_malformed_record_is_ignored_...` |
| 7 | In-session Stop shows no banner; `Failed` is not persisted | the channel only writes on Stopped; `a_worker_that_dies_...` |

A file that is not a JSON object is set aside as `hatate-rs.json.unreadable`, the way `config.json` is; other keys in the file are kept. This module owns only its key.

## Thumbnail contract (DAN-2116 item 3, call 4)

40 px cell (`THUMB_COLUMN_SIZE`), 49 px row, decode at 4x (160 px) whatever the density, texture at 40 x scale (scale 1 to 4). Key: the file's path and the decode size (Python's `id(entry)` is an in-memory identity; the path survives a re-sort, a filter and a session reload). Cache: 4000 entries, least recently used out. `ThumbState` is `Loading` (also "not requested yet"), `Failed` (tooltip: `Thumbnail unavailable — the file is missing or could not be read.`, no path) or `Loaded`. `fit_within` is `QSize.scaled(KeepAspectRatio)` in integer arithmetic.

## Method map

Every `def` or table of the Python surface in scope, by `file:line` at `76c85c4`.

| Python | Line | Rust, or the ticket that owns it |
|---|---|---|
| `core/models.py` `MatchStatus.label` | 36 | `MatchStatus::label` (6.b), `StatusTier::label` |
| `core/models.py` `MatchStatus.sort_rank` | 47 | `StatusTier::rank` |
| `core/models.py` `ImageEntry.needs_review` | 263 | `queue::needs_review` (5.e), used by `RestoreCounts::of` |
| `gui/theme.py` `STATUS_GLYPHS` | 155 | `StatusTier::glyph`, `SentTier::glyph` |
| `gui/theme.py` `STATUS_WEIGHTS` | 192 | `StatusTier::ink` |
| `gui/theme.py` `STATUS_GLYPH_FONT`, `_NEEDS_GLYPH_FONT` | 208, 209 | `StatusTier::glyph_font`, `SentTier::glyph_font` |
| `gui/theme.py` `CHIP_LABELS` | 217 | `StatusTier::chip_text`, `SentTier::cell_text` |
| `gui/theme.py` `chip_label`, `status_glyph`, `status_weight`, `status_glyph_font` | 236, 240, 252, 259 | the methods above (an unknown key has no Rust counterpart: the enums are closed) |
| `gui/table_delegates.py` `BOXED_CHIPS` | 52 | `StatusTier::boxed`, `bold`, `italic` |
| `gui/image_table_model.py` `EMPTY_CELL` | 52 | `status_tier::EMPTY_CELL` |
| `gui/image_table_model.py` `_upscale_glyph`, `_upscale_weight` | 111, 124 | `UpscaleTier::glyph`, `ink` |
| `gui/image_table_model.py` `_entry_sent_label` | 237 | `SentTier::cell_text` |
| `gui/image_table_model.py` sort keys for Sent, Reviewed, Upscale | 644, 649, 652 | `SentTier::rank`, `reviewed_rank`, `UpscaleTier::rank` |
| `core/entry_filter.py` `upscale_verdict_label` | 110 | `entry_filter::upscale_verdict_label` (6.b), `UpscaleTier::label` |
| `gui/main_window.py` `_plural` | 194 | `stop_record::images` (private) |
| `gui/main_window.py` `_interrupted_prose` | 198 | `stop_record::crashed_body`, `counts_clause` (the Interrupted body is new: DAN-2224) |
| `gui/main_window.py` `_is_waiting_review`, `_waiting_review_count` | 225, 231 | `RestoreCounts::of` |
| `gui/main_window.py` `_restore_saved_session` | 1400 | `stop_record::launch_banner`; the restore itself is 6.c/6.d's |
| `gui/main_window.py` `_show_nothing_survived`, `_show_run_banner` | 1462, 1500 | the widgets are 6.d's; the decision is `launch_banner` |
| `gui/main_window.py` `_on_worker_waiting`, `_on_wait_countdown` | 2336, 2339 | `RunChannel::countdown` (the text is 6.b's `eta::worker_waiting_text`, `wait_countdown_text`) |
| `gui/main_window.py` `_on_paused_out_of_quota` | 2417 | `RunChannel::paused_for_quota`; the banner is 6.e's |
| `gui/main_window.py` `_on_search_progress`, `_on_search_finished` | (see the Python test map) | `RunChannel::progress`, `finished`; the labels are 6.b's `eta::run_strip` |
| `gui/main_window.py` `THUMB_COLUMN_SIZE`, `QUEUE_ROW_HEIGHT`, `THUMB_DECODE_QUALITY_FACTOR`, `THUMB_ICON_CACHE_ENTRIES` | 113, 114, 121, 128 | `thumb::THUMB_COLUMN_SIZE`, `QUEUE_ROW_HEIGHT`, `THUMB_DECODE_QUALITY_FACTOR`, `THUMB_ICON_CACHE_ENTRIES` |
| `gui/main_window.py` `THUMB_VIEWPORT_BUFFER_ROWS`, `THUMB_VIEWPORT_DEBOUNCE_MS` | 140, 142 | 6.d (the viewport pass; `viewport.rs` is 6.b's) |
| `gui/main_window.py` `_start_thumbnail_generation`, `_on_thumbnail_ready`, `_on_thumbnail_progress`, `_on_thumbnails_finished` | 2035, 2068, 2100, 2106 | `ThumbBook`, `ThumbKey` here; the worker bridge and the texture are 6.d's |
| `workers/thumbnail_worker.py` `_decode`, `_decode_from_bytes` | 86, 71 | `thumb::fit_within` (the sizing); the decode is 6.d's |
| `core/lru_cache.py` `LRUCache` | 14 | `lru::LruCache` (Phase 1), `ThumbBook` |
| `gui/mcp_bridge.py` `_queue_row`, `_entry_detail` | 596, 608 | `mcp::tool_handlers::queue_row`, `entry_detail` (5.e); the pin is new |
| `workers/search_worker.py` signals `image_updated`, `progress`, `waiting`, `wait_countdown`, `paused_out_of_quota`, `finished_all` | 160-170 | the `Reporter` hooks in the table above |

## Python test map

The surface in scope is the tiers and ranks, the run state, the launch decision and counts, the thumbnail sizing, the error shape and the MCP row keys. A test that only exercises a widget maps to the ticket that builds it.

| Python test (`tests/`) | Rust test, or reason |
|---|---|
| `test_core_utils.py` `test_every_status_has_a_glyph`, `test_every_status_has_chip_wording` | `status_tier::tests::the_chip_of_each_tier_matches_the_table_of_call_1a` |
| `test_gui_smoke.py` `test_status_column_carries_a_glyph`, `test_chip_keys_in_the_same_column_are_pairwise_distinct`, `test_error_is_boxed_and_no_other_status_is` | same; the pairwise-distinct check on the six tiers is `each_tier_is_one_status_and_round_trips` plus the table. The painted check is 6.d's. |
| `test_theme.py` `TestStatusGlyphFontCoverage` | `StatusTier::glyph_font` is in the table test; that the bundled face covers the glyphs is 6.c's font test |
| `test_theme.py` `TestStatusWeightContrast` | 6.d (contrast is measured on the painted chip) |
| `test_gui_smoke.py` `test_sent_column_distinguishes_queued_from_confirmed`, `test_sent_state_orders_unsent_then_queued_then_confirmed` | `status_tier::tests::the_sent_and_reviewed_columns_rank_as_python_sorts_them` |
| `test_gui_smoke.py` `TestReviewedColumn` (`test_only_reviewed_rows_are_marked`, `test_rows_wanting_a_decision_sort_first`) | same test (the cell text and the rank); the column is 6.d's |
| `test_gui_smoke.py` `test_sorting_every_column`, `TestColumnSortKeys` | the three ranks here; the stable sort and the header cycle are 6.d (A3) |
| `test_interrupted_banner.py` `TestProse` (`test_it_reassures_and_carries_the_counts`, `test_counts_by_category`, `test_nothing_queued_is_not_announced`) | `stop_record::tests::the_counts_clause_is_pythons_for_one_and_for_two`, `counts_are_taken_from_the_rows_as_python_counts_them` |
| `test_interrupted_banner.py` `TestPreviousRunStoppedAt`, `TestLastCleanExit` | `crash_marker` (Phase 2) |
| `test_interrupted_banner.py` `TestFrameAndKicker`, `TestMetaLine`, `TestActionsUnchanged`, `TestRowMarker`, `TestStripNote` | widgets and copy: 6.d and 6.e (the row marker's flag is `mark_interrupted_mid_search`, Phase 2) |
| `test_nothing_survived.py` `TestVariantShown` | `run_state_launch::a_marker_and_a_session_that_will_not_load_is_failed_empty_...` (the decision); the rendering is 6.d's |
| `test_nothing_survived.py` `TestFaultsInPreviousRun`, `TestSavedSessionAge`, `TestEmptyRhythm`, `TestActionGap` | crash log: Phase 7 (DAN-2213 call 2b); age: `SessionStore::saved_session_age_seconds` (Phase 2); rhythm and gaps: 6.d |
| `test_quota_pause_banner.py`, `test_gui_smoke.py` `TestPersistedQuotaPauseShownOnStartup`, `TestPauseIsPersistedWhenTheBatchStops` | the pause file: `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` (Phase 3.i); the state: `batch_run::an_exhausted_quota_is_a_paused_state_not_an_ending`; the banner is 6.e's |
| `test_gui_smoke.py` `test_search_progress_handlers`, `test_progress_label_shows_the_count_then_the_estimate`, `test_a_finished_run_keeps_the_count_and_drops_the_estimate` | `eta::run_strip` (6.b) fed by `batch_run::the_gap_between_images_is_a_waiting_state_...` (the ETA) and `a_completed_run_says_running_...`; the label widgets are 6.d's |
| `test_gui_smoke.py` `test_progress_accounts_for_the_extra_attempts`, `test_stopping_cancels_the_retry_pass`, `test_an_exhausted_daily_quota_cancels_the_retry_pass` | `batch_run` (Phase 3.i), unchanged |
| `test_thumbnail_worker.py` `TestLocalDecode` (`test_a_large_image_is_decoded_down_to_the_requested_size`, `test_an_image_already_smaller_than_the_target_is_left_alone`), `TestDecodeFromBytes` | `thumb::tests::a_picture_is_fitted_the_way_qt_scales_it` (the sizing); the decode: 6.d |
| `test_thumbnail_worker.py` `TestThumbnailSource`, `TestKnownToHydrus`, `TestHydrusThumbnailPath`, `TestStopRequest`, and `test_an_undecodable_file_yields_None...` | the worker: 6.d. `ThumbState::Failed` is the state a `None` decode lands in (`thumb::tests::a_cell_is_loading_until_a_decode_lands_then_loaded_or_failed`) |
| `test_gui_smoke.py` `test_thumbnail_ready_handler`, `test_thumbnails_follow_their_entries_after_a_removal`, `test_a_finished_thumbnail_repaints_the_row_it_belongs_to`, `test_a_thumbnail_for_a_hidden_row_repaints_nothing`, `test_thumbnail_range_is_clamped_to_the_visible_rows` | 6.d (they are about repaint and the viewport). The key that survives a removal or a re-sort is `ThumbKey` (the path). |
| `test_gui_smoke.py` `test_thumb_column_has_no_text` | 6.d |
| `test_mcp_bridge.py` `test_list_queue_*`, `test_get_entry_*` | `mcp_tool_handlers.rs` (5.e) and the new key pin |
| `test_mcp_bridge.py` `test_times_out_if_the_callable_runs_longer_than_the_timeout` | `queue::tests` (5.e) and `error_shape_tests::a_queue_timeout_has_the_shape_too` |
| `test_review_*`, `test_settings_search`, `test_mcp_settings_tab`, `test_tag_sidecars`, `test_sent_tag_filter`, `test_gui_harness` | not this ticket's surface (review page, settings, sidecars: 6.b/6.f-6.i); `test_gui_harness` is the harness 6.c builds on |

Python has no test for the error shape (it has no such thing); the Rust tests are the DAN-2116 acceptance line.

## How it was checked

(Filled in below after the differential: see the PR body and `worktrees/DAN-2216-differential.log`.)
