# `hatate batch <dir>`: the headless search run (Phase 3.i, DAN-2065)

What this ticket ports, where the cut is, how it was checked against the real Python, what
differs on purpose, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net, cache, quota-pause, crash-marker and
session modules, the engine clients and runner (3.a to 3.f) and `search_image` (3.h,
`docs/search-engine.md`) are reused as they are. The only change to earlier code is one transient
field on `Entry` (section 5).

`hatate batch` is experimental: it is a work-in-progress command in a work-in-progress rewrite.

## 1. The cut

| Python | Rust |
|---|---|
| `core/file_intake.py`: `scan_folder`, `filter_duplicate_paths` | `batch::intake::{scan_folder, filter_duplicate_paths, abspath, has_image_extension}` |
| `workers/file_hash_worker.py` (no Qt) and `core/hydrus_tag_lookup.{hash_from_filename, verify_filename_hashes}` | `batch::hashing::{hash_from_filename, verify_filename_hashes, split_by_filename_hash, hash_files, estimate_remaining}` |
| `gui/main_window._import_files` / `_on_file_hash_finished` (hash, drop duplicates, add) and `_launch_search_worker` (forget the quota flag and the engines that stood down, clear the pause file) | `batch::run::run_batch` |
| `workers/search_worker.py`: `_run`, `_queue_retries`, `_skip_saucenao_now`, `_saucenao_quota_should_pause`, `_wait_with_countdown` | `batch::run::{search_pending, queue_retries, skip_saucenao_now, saucenao_quota_should_pause, wait_with_countdown}` |
| `workers/session_autosave_worker.py` / `gui/session_autosave.py` (the interval, the floor, the empty-list rule) | `batch::run::Autosave` |
| `core/logger.log_matched_url` | `batch::matched_urls::log_matched_url` |
| `main.py` signal handlers, crash markers | `hatate-cli`: `batch_cmd` (SIGINT/SIGTERM raise the stop flag, the loop saves the session and exits 130/143; a second signal quits at once), Phase 1 `crash_marker` |

Not here, and who owns it: Hydrus auto-import (`_maybe_auto_import`, the staging directory and its
`owner.pid`, `_switch_to_importable_match`) is Phase 4. A batch with `auto_import_enabled` on prints
one line saying so and carries on. The GUI's one-shot "Continue with other engines" override
(`force_continue_without_saucenao`) is a dialog button; the command line has the saved setting
`continue_without_saucenao_on_quota` and exit status 3.

Exit status: 0 finished, 1 error, 2 usage, 3 paused (SauceNAO's daily allowance ran out), 130
stopped by SIGINT, 143 stopped by SIGTERM.

The session goes to `session.db` at the XDG path through the Phase 1 `SessionWriter`, after the
existing session has been restored, so a profile with rows in it is never wiped: new files are
appended and everything not yet searched is searched.

## 2. Differences from Python, on purpose

1. **The scan sorts names within each directory** (files and sub-directories). Python walks in
   file-system order, so "the first of two identical files wins" is not reproducible there. The
   differential makes Python's `os.walk` sorted the same way.
2. **Autosave writes the whole list**, not per-row revisions. `SessionWriter::save` is the Phase 1
   writer; Python's revision map only exists to make GUI saves cheaper.
3. **Logging is `tracing` to stderr** (`-v` info, `-vv` debug). No `hatate-rs.log` is written by
   `batch`, and `app.log` is never touched (Phase 1 rule).
4. **Dependency:** `tracing-subscriber` in `hatate-cli` for the stderr log (already a
   `hatate-core` dev-dependency).
5. **`Entry.matched_url` is transient** (section 5).

## 3. How it was checked

**Differential against the real Python worker** (the oracle). The driver lives outside the repo
(the tree is Rust only); its log is `worktrees/DAN-2065-differential.log`. The Python side runs
the real `workers/search_worker.py::SearchWorker.run()`, the real `file_intake`, `FileHashWorker`
and `session_db` at `76c85c4`, with the HTTP transport and the four engines scripted, a fake clock
for `sleep`/`monotonic`/`time` and `random.uniform` pinned at its midpoint. The Rust side runs
`run_batch` through `NetClient::new` (rustls with the explicit provider) against a local https
server, with the same scripted engines and a manual clock. Per folder scenario it compares: files
found, duplicates, files added; the engine calls per image in order (a retry is a second group);
every request (method, URL with query, User-Agent, Referer, Cookie, Authorization); every
search-cache file; every sleep; the quota-pause file; the matched-URL log; the pause event; and the
session the Rust side wrote, **read back with the Python app's own `session_db.load_entries`**
(0 corrupt rows), entry by entry against what Python saved and re-read.

Result at `e89f23d`: 50 folder scenarios (5 seeds), 481 entries compared, 146 duplicates skipped;
folders with sub-directories, upper-case extensions, byte copies, a file named by its real hash and
a file whose name lies, non-image files, quota pauses (5), continue-without-SauceNAO (3), retries
(16 images), caches (385 files), 4,116 sleeps, 1,116 requests, 128 matched-URL lines. **0
differing.** The first pass at `05d0ced` found one (a cached result whose matches all died: Python
logs the stale `entry.matched_url`, section 5). Two further 1,000-image scenarios, compared the same
way, also differ nowhere.

Not compared, because Python has no oracle for it: the number of intermediate autosaves (the GUI
timer drives them; the Rust loop's interval rule is covered by `the_session_is_autosaved_on_its_interval_during_a_long_wait` and
`an_interval_under_ten_seconds_is_ten`), and the stderr text.

**Checkpoint numbers** are on the ticket (DAN-2065).

## 4. Python tests that exercise this scope

Python tests at `76c85c4` in `tests/` that call a function listed in section 1, and the Rust test
that covers each, or why none does. `core_utils` = `test_core_utils.py`, `gui_smoke` =
`test_gui_smoke.py`. Rust: `intake`/`hashing`/`matched_urls` are `#[cfg(test)]` modules of
`crates/hatate-core/src/batch/`, `batch_run` is `crates/hatate-core/tests/batch_run.rs`,
`batch_cli` is `crates/hatate-cli/tests/batch_cli.rs`, `startup_files` is
`crates/hatate-core/tests/startup_files.rs`.

| Python test | Rust |
|---|---|
| `core_utils::TestFolderScan` (2) | `intake::scan_finds_images_recursively_and_skips_the_rest`, `scan_of_a_missing_folder_is_empty`, `extension_set_is_the_python_one_case_insensitive` |
| `core_utils::TestFilenameHashes` (7) | `hashing::a_hydrus_style_name_yields_its_hash`, `other_names_yield_nothing` (ordinary and upper-case names), `verification_declines_when_there_is_nothing_to_verify`, `names_that_match_their_contents_are_taken_without_reading_the_rest`, `a_lying_name_is_caught_by_the_sample_and_everything_is_read`, `an_unreadable_sampled_file_is_not_trusted`, `local_hash_source_never_trusts_a_name`; `batch_run::hash_source_local_reads_every_file_and_a_lying_name_does_not_fool_it` |
| `file_hash_worker::TestFileHashWorkerStop` (2) | `hashing::a_raised_stop_flag_gives_no_result`, `parallel_and_sequential_agree`, `progress_always_reports_the_last_file`; `batch_cli::sigterm_during_hashing_stops_the_run_with_143` |
| `core_utils::TestQuotaPausePersistence` (4) | `startup_files::record_writes_exactly_the_four_keys_in_order`, `a_python_reader_would_accept_what_record_writes`, `clear_removes_the_file_and_is_fine_when_there_is_none`, `reset_is_the_next_utc_midnight`; `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it`, `a_new_run_forgets_the_old_pause_file` |
| `core_utils::TestRateLimitCredit` (6) | `net::pacing::tests::remaining_delay_credits_elapsed_time` (Phase 1); `batch_run::the_gap_between_images_is_waited_out_with_a_countdown_and_not_after_the_last`, `an_image_served_from_the_cache_costs_no_gap` |
| `session::TestMatchedUrlLogRotation` (1) | `matched_urls::appends_a_line_and_creates_the_folder`, `a_full_file_rotates_to_dot_one_and_the_old_dot_one_to_dot_two`, `a_log_that_cannot_be_written_is_not_an_error` |
| `gui_smoke::TestFailedSearchesAreRetriedOnce` (9) | `batch_run::a_network_fault_is_retried_once_after_the_first_pass` (a failed image is tried again; a good one is not; several), `a_persistent_failure_is_retried_exactly_once`, `retries_off_means_one_attempt`, `stopping_cancels_the_retry_pass`, `an_exhausted_quota_cancels_the_retry_pass`, `progress_accounts_for_the_extra_attempts`; "a not found is not a failure" is `engine_runner` `retry_could_help` (3.h) plus `queue_retries` selecting on `Error` only |
| `search_quality::TestRetryOnlyWhenItCouldHelp` (4), `yandex::TestStandingDown::test_a_resting_yandex_says_so_on_the_row` | `engine_runner/tests.rs` `retry_could_help` section (3.f/3.h); `batch_run::a_panic_in_one_engine_marks_that_image_and_the_batch_goes_on` for the unattributable message |
| `gui_smoke::TestBatchCarriesOnWithoutSauceNao` (5) | `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` (stops by default), `continue_without_saucenao_carries_on_and_says_so_once` (finishes, later images searched without SauceNAO, marked provisional and not cached, said once) |
| `gui_smoke::TestPauseIsPersistedWhenTheBatchStops` (1) | `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` |
| `gui_smoke::TestSessionAutosaverOnItsOwn` (8) | empty list never written: `batch_run::an_empty_folder_says_so_and_saves_nothing`; interval and floor: `the_session_is_autosaved_on_its_interval_during_a_long_wait`, `an_interval_under_ten_seconds_is_ten`; off: `autosave_off_saves_when_files_are_added_and_at_the_end_only`. The revision-map tests (failed save, successful save, non-dict map) and the timer tests (runs at the interval, off stops it, shutdown stops it) do not apply: there is no incremental revision map (difference 2) and no timer, the interval is read in the loop |
| `interrupted_banner::TestRowMarker::test_the_search_worker_spends_the_flag` | `batch_run::a_search_spends_the_interrupted_note` |
| `gui_smoke::TestFilteredRowsAreNeverActedOnByAccident::test_a_filter_does_not_limit_what_start_search_processes` | `batch_run::files_already_in_the_session_that_are_not_searched_are_searched_too` |
| `gui_smoke::TestAutoImportStagingIsCleanedUp` (3), `test_search_worker_staging.py`, `search_quality::TestSendUrlFallsBackToHatatesDownload`, `hydrus_import_poll`, `url_importer_duplicate_relationships::TestAutoImportWiring`, `hydrus_provenance_note::TestAutoImportWiring` | do not apply: Hydrus auto-import and its staging directory are Phase 4; `batch_run::auto_import_on_says_it_is_not_in_this_build_and_carries_on` and the `no staging directory is made` assertion in it |

Do not apply, with the reason (all of them test Qt widgets, signals or dialogs, which the
headless command has no counterpart for): `gui_smoke::TestEngineAlertsReachTheUser` (alert channel
to a window; engine errors go to the log and the row's `error_message`),
`gui_smoke::TestFilteredRowsAreNeverActedOnByAccident` (all but the one above: the table filter),
`gui_smoke::TestPersistedQuotaPauseShownOnStartup` (the banner; the file rules it reads are
Phase 1's `startup_files::a_pause_whose_reset_has_passed_is_deleted_at_startup` and
`a_missing_or_corrupt_file_means_no_pause_and_is_left_alone`),
`gui_smoke::TestPausedBannerContinueButton`, `TestResumeWithoutSaucenaoOverride` and
`quota_pause_banner::TestActions` (the dialog's one-shot override, section 1),
`interrupted_banner::TestStripNote` and the other `TestRowMarker` tests (the strip and the row
marker; the flag itself is Phase 1's), `gui_harness::TestNamedGaps` (which classes are widgets),
`core_utils::TestCrashDiagnostics` (Qt excepthook and message handler; a panic in one engine is
contained per image, `a_panic_in_one_engine_marks_that_image_and_the_batch_goes_on`, and the
crash marker is Phase 1's), `test_workers_background.py` (the availability, upscale and pawchive
workers are other tickets' scope).

## 5. `Entry.matched_url`

Python's `ImageEntry.matched_url` is a stored field that `select_candidate` copies, not a value
derived from the selected candidate. It outlives the candidate: when a cached result's matches
have all died since, `search_image` calls `select_candidate(-1)`, which changes nothing, and the
row is left `good`/`poor` with no candidates and the dead URL in `matched_url`. The worker's
matched-URL log writes `entry.matched_url`, so Python logs a dead URL for that row. 3.h read the
URL from the selected candidate instead, which logs nothing; the differential found the gap (1 of
294 entries in the first pass).

`Entry.matched_url` is now a `#[serde(skip)]` field like `last_searched`: set by `select_candidate`,
cleared on the "every match was dead" path of a fresh search (as Python does), never stored (a row
loaded from a session gets it from `select_candidate`, as in Python). The batch log reads it. No
session byte changes.

## 6. Cost note: cache writes are fsynced

`SearchEngine` stores the search-cache file with the Phase 1 atomic write, which fsyncs. Python's
`search_cache.save_cached_result` is a plain `write_text`. On a disk that costs about 13 ms per
searched image (1,000 images: 13.7 s of waiting, 0.7 s of CPU); on tmpfs it costs nothing
(0.27 s for the same 1,000 images, against 1.7 s for the Python worker). Against the 45 to 75 s
rate-limit gap between real searches it is under 0.03%, so nothing here changes it; it is noted so
the speed figures are not misread.
