# `hatate batch <dir>`: the headless search run (Phase 3.i, DAN-2065; auto-import Phase 4.g, DAN-2132)

What this ticket ports, where the cut is, how it was checked against the real Python, what
differs on purpose, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net, cache, quota-pause, crash-marker and
session modules, the engine clients and runner (3.a to 3.f) and `search_image` (3.h,
`docs/search-engine.md`) are reused as they are. The only change to earlier code in 3.i is one
transient field on `Entry` and the line that sets it (section 5). Hydrus auto-import (4.g) is
section 7.

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
| `workers/search_worker.py`: `_maybe_auto_import`, `_switch_to_importable_match`, `url_import_refusal` in it (4.g) | `batch::auto_import::{maybe_auto_import, switch_to_importable_match}`; the refusal is `hydrus_import::url_import_refusal` (4.d) |
| `workers/search_worker.py`: `_write_pid_file`, `_read_pid_file`, `_pid_is_alive`, `_cleanup_stale_staging_dirs`, the `mkdtemp` + pid step of `_run`, `_cleanup_auto_import_tmp`, `register_cleanup_handlers`' `cleanup()` (4.g) | `batch::staging::{write_pid_file, read_pid_file, pid_is_alive, cleanup_stale, StagingDir, cleanup_current}` |
| `gui/main_window._on_auto_imported` (what it does to the list: drop the row when `remove_after_import`) (4.g) | `batch::auto_import::removes_row` and the removal at the end of `run_batch` |
| `main.py` signal handlers, crash markers | `hatate-cli`: `batch_cmd` (SIGINT/SIGTERM raise the stop flag, the loop saves the session and exits 130/143; a second signal quits at once), Phase 1 `crash_marker` |

Not here, and who owns it: the GUI's one-shot "Continue with other engines" override
(`force_continue_without_saucenao`) is a dialog button; the command line has the saved setting
`continue_without_saucenao_on_quota` and exit status 3. Hydrus auto-import is section 7.

The Pawchive index engine (`enable_pawchive_index`) searches the profile's `pawchive_index.db` in a
batch (Phase 4.h, `docs/pawchive-index.md` section 7). The batch only reads that file.

Exit status: 0 finished, 1 error (including a run that ended on an exception the Python worker does
not catch, section 7), 2 usage, 3 paused (SauceNAO's daily allowance ran out), 130 stopped by
SIGINT, 143 stopped by SIGTERM.

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
5. **`Entry.stale_matched_url`**, a transient field for one Python quirk (section 5).

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
| `gui_smoke::TestRateLimitIsMeasuredEngineToEngine` (3) | `batch_run::booru_time_counts_toward_the_gap_and_the_engine_to_engine_interval_stays_whole` (non-engine time is credited, 80 s waited not 120, and the engine-to-engine interval is the configured 60 s), `the_gap_between_images_is_waited_out_with_a_countdown_and_not_after_the_last` (with nothing else spending time the gap is waited whole) |
| `gui_smoke::TestPerHostPacing` (4, plus the fallback) | `batch_run::a_hosts_own_interval_replaces_the_global_gap_for_that_host_only` (a laxer override is not held to the global delay and SauceNAO, not queried, imposes nothing; a stricter one stretches the gap), `a_stricter_host_paces_the_whole_wave`, `no_engine_report_falls_back_to_the_global_delay`; the default is `the_gap_between_images_is_waited_out_with_a_countdown_and_not_after_the_last`; the arithmetic is Phase 1's `net::pacing::tests` |
| `core_utils::TestDuplicateFiltering` (2) | `intake::an_existing_path_is_a_duplicate_even_spelled_differently`, `the_same_content_under_another_name_is_a_duplicate`, `duplicates_inside_the_batch_keep_the_first` |
| `core_utils::TestHashSourceSplit` (3) | `hashing::names_that_match_their_contents_are_taken_without_reading_the_rest`, `a_lying_name_is_caught_by_the_sample_and_everything_is_read`, `local_hash_source_never_trusts_a_name`; `batch_run::hash_source_local_reads_every_file_and_a_lying_name_does_not_fool_it` |
| `core_utils::TestQuotaPauseGating` (5) | `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` (pause), `with_saucenao_out_of_the_order_its_quota_is_irrelevant` (not in the order), `continue_without_saucenao_carries_on_and_says_so_once` (carry on without it: no pause, SauceNAO skipped) |
| `session::TestMatchedUrlLogRotation` (1) | `matched_urls::appends_a_line_and_creates_the_folder`, `a_full_file_rotates_to_dot_one_and_the_old_dot_one_to_dot_two`, `a_log_that_cannot_be_written_is_not_an_error` |
| `gui_smoke::TestFailedSearchesAreRetriedOnce` (9) | `batch_run::a_network_fault_is_retried_once_after_the_first_pass` (a failed image is tried again; a good one is not; several), `a_persistent_failure_is_retried_exactly_once`, `retries_off_means_one_attempt`, `stopping_cancels_the_retry_pass`, `an_exhausted_quota_cancels_the_retry_pass`, `progress_accounts_for_the_extra_attempts`; "a not found is not a failure" is `engine_runner` `retry_could_help` (3.h) plus `queue_retries` selecting on `Error` only |
| `search_quality::TestRetryOnlyWhenItCouldHelp` (4), `yandex::TestStandingDown::test_a_resting_yandex_says_so_on_the_row` | `engine_runner/tests.rs` `retry_could_help` section (3.f/3.h); `batch_run::a_panic_in_one_engine_marks_that_image_and_the_batch_goes_on` for the unattributable message |
| `gui_smoke::TestBatchCarriesOnWithoutSauceNao` (5) | `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` (stops by default), `continue_without_saucenao_carries_on_and_says_so_once` (finishes, later images searched without SauceNAO, marked provisional and not cached, said once) |
| `gui_smoke::TestPauseIsPersistedWhenTheBatchStops` (1) | `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` |
| `gui_smoke::TestSessionAutosaverOnItsOwn` (8) | empty list never written: `batch_run::an_empty_folder_says_so_and_saves_nothing`; interval and floor: `the_session_is_autosaved_on_its_interval_during_a_long_wait`, `an_interval_under_ten_seconds_is_ten`; off: `autosave_off_saves_when_files_are_added_and_at_the_end_only`. The revision-map tests (failed save, successful save, non-dict map) and the timer tests (runs at the interval, off stops it, shutdown stops it) do not apply: there is no incremental revision map (difference 2) and no timer, the interval is read in the loop |
| `interrupted_banner::TestRowMarker::test_the_search_worker_spends_the_flag` | `batch_run::a_search_spends_the_interrupted_note` |
| `gui_smoke::TestFilteredRowsAreNeverActedOnByAccident::test_a_filter_does_not_limit_what_start_search_processes` | `batch_run::files_already_in_the_session_that_are_not_searched_are_searched_too` |
| `gui_smoke::TestAutoImportStagingIsCleanedUp` (3) | `batch_auto_import::the_staging_directory_exists_during_the_run_with_this_pid_and_is_gone_after` (removed after a batch), `the_staging_directory_is_removed_when_the_run_is_stopped` (stopped), `the_staging_directory_is_removed_when_the_run_ends_on_an_error` (a worker that dies midway) |
| `test_search_worker_staging.py::TestSearchWorkerStagingPaths` (3) | `paths::tests` (Phase 1: `auto_import_staging_dir` is `CACHE_DIR/auto_import_staging`); `batch_auto_import::the_staging_directory_exists_during_the_run_with_this_pid_and_is_gone_after` (a directory in the cache folder, not in `/tmp`) |
| `test_search_worker_staging.py::TestSearchWorkerStagingCleanup` (9, plus the pid-file ones) | `staging::tests::create_makes_a_private_directory_with_this_pid_and_drop_removes_it` (`test_staging_dir_created_in_cache_not_tmp`, `test_write_and_read_pid_file`), `batch_auto_import::a_run_sweeps_the_directories_earlier_runs_left_and_spares_a_live_owner` (`test_cleanup_stale_dirs_on_startup`, `test_cleanup_ignores_non_hatate_dirs`, `test_cleanup_skips_live_pid`, `test_cleanup_removes_dead_pid`, `test_cleanup_removes_dir_without_pid`), `staging::tests::the_pid_file_is_trimmed_and_anything_else_is_unknown`, `zero_is_not_alive_this_process_is_and_a_huge_number_is_an_error`, `a_dead_pid_is_not_alive`; `batch_staging_signal::the_signal_path_cleanup_removes_the_directory_of_a_run_in_progress` (the atexit/signal `cleanup()`); `test_current_staging_dir_tracked` is the same test. `test_register_cleanup_handlers_idempotent` and `test_sigint_handler_exits_via_systemexit` do not apply: they test Python's `signal.signal` registration from the GUI's main thread; the command's own handlers are `batch_cmd::spawn_signal_handler` and `batch_cli` (3.i) |
| `search_quality::TestSendUrlFallsBackToHatatesDownload::test_auto_import_downloads_when_no_match_hydrus_can_take`; the rest of the class is 4.d (`docs/hydrus-import-url.md`) | `batch_auto_import::auto_import_downloads_when_no_match_hydrus_can_take` |
| `search_quality::TestHydrusUrlImporterGuard::test_auto_import_falls_back_to_a_match_hydrus_can_take`, `test_the_fallback_never_goes_below_the_threshold` (the only two tests of the class that build a `SearchWorker`) | `batch_auto_import::auto_import_falls_back_to_a_match_hydrus_can_take`, `the_fallback_never_goes_below_the_threshold`, `only_matches_that_clear_the_bar_on_a_measured_score_and_are_web_links_are_tried`, `at_most_five_alternatives_are_asked_about` |
| `search_quality::TestHydrusUrlImporterGuard`, the other five tests | `hydrus_import_url::hydrus_s_own_answers_decide_which_urls_the_importer_is_not_handed`, `hydrus_not_answering_does_not_block_imports`, `a_refused_url_is_not_sent_or_marked_sent` (4.d). They call `url_import_refusal` and `send_url_to_importer` directly and build no `SearchWorker`, so the 3.i note that said the class "builds a `SearchWorker` only to call `url_import_refusal`" was wrong |
| `test_hydrus_import_poll.py` (`TestConfirmation`, `TestTimeout`, `TestProgress`, `TestStopAndErrors`) | `hydrus_import_poll` (4.d). The batch's use of it: `batch_auto_import::a_raised_stop_flag_ends_the_poll_without_a_confirmation`, `with_every_setting_that_needs_a_hash_off_the_import_is_not_confirmed` |
| `url_importer_duplicate_relationships::TestAutoImportWiring` (6) | `batch_auto_import::each_of_the_three_settings_alone_is_enough_to_confirm_the_import` (`test_the_relationship_setting_alone_is_enough_to_confirm_and_pair`, `test_removal_still_confirms_on_its_own_with_relationships_off`, and the paired hashes: the confirmed hash replaces the local one), `with_every_setting_that_needs_a_hash_off_the_import_is_not_confirmed` (`test_nothing_is_confirmed_or_related_when_both_settings_are_off`), `an_unconfirmed_import_relates_nothing_and_writes_no_note` (`test_an_unconfirmed_import_relates_nothing`); `test_the_pairing_uses_the_hash_the_local_copy_had` and `test_a_read_back_failure_becomes_a_warning_not_a_failed_import` are the `finish_url_import` tests of 4.d (`docs/hydrus-import-url.md`) |
| `hydrus_provenance_note::TestAutoImportWiring` (4) | `batch_auto_import::each_of_the_three_settings_alone_is_enough_to_confirm_the_import` (the note setting alone), `with_every_setting_that_needs_a_hash_off_the_import_is_not_confirmed`, `an_unconfirmed_import_relates_nothing_and_writes_no_note` (`test_an_unconfirmed_import_gets_no_note`) |

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
`gui_smoke::TestWorkerCallbacksDuringClose` (a completion slot of a window that is closing must not open a
modal; there is no window or slot), `search_quality::TestHydrusUrlImporterGuard` (it builds a
`SearchWorker` only to call `url_import_refusal`, the Hydrus URL import check, which is Phase 4),
`core_utils::TestCrashDiagnostics` (Qt excepthook and message handler; a panic in one engine is
contained per image, `a_panic_in_one_engine_marks_that_image_and_the_batch_goes_on`, and the
crash marker is Phase 1's), `test_workers_background.py` (the availability, upscale and pawchive
workers are other tickets' scope).

## 5. `Entry.stale_matched_url`

Python's `ImageEntry.matched_url` is a stored field that `select_candidate` copies, not a value
derived from the selected candidate. It outlives the candidate in one place: when a cached
result's matches have all died since, `search_image` calls `select_candidate(-1)`, which changes
nothing, and the row is left `good`/`poor` with no candidates and the dead URL in `matched_url`.
The worker's matched-URL log writes `entry.matched_url`, so Python logs a dead URL for that row.
3.h read the URL from the selected candidate instead, which logs nothing; the differential found
the gap (1 of 294 entries in the first pass).

`Entry.stale_matched_url` is a `#[serde(skip)]` field like `last_searched`: set only on that cache
path (to the URL of the match that was selected before the drop), cleared when a fresh search
starts, never stored. The batch log writes the selected candidate's URL, or this when there is no
candidate. Nothing else reads it and no session byte changes. (A first version made `matched_url`
a transient field set by `select_candidate`; it made a loaded entry differ from the one that was
saved in `session_compat`'s hostile round-trip tests, which is the wrong place to pay for a log
line, so only the one stale case is carried.)

## 6. Cost note: cache writes are fsynced

`SearchEngine` stores the search-cache file with the Phase 1 atomic write, which fsyncs. Python's
`search_cache.save_cached_result` is a plain `write_text`. On a disk that costs about 13 ms per
searched image (1,000 images: 13.7 s of waiting, 0.7 s of CPU); on tmpfs it costs nothing
(0.27 s for the same 1,000 images, against 1.7 s for the Python worker). Against the 45 to 75 s
rate-limit gap between real searches it is under 0.03%, so nothing here changes it; it is noted so
the speed figures are not misread.

## 7. Hydrus auto-import (Phase 4.g, DAN-2132)

**What is ported.** `SearchWorker._maybe_auto_import` and `_switch_to_importable_match`
(`batch::auto_import`), the staging directory, its `owner.pid`, the sweep of directories an earlier
run left behind and the cleanup however the run ends (`batch::staging`), and the loop wiring in
`batch::run::search_queue` where `SearchWorker._run` calls them. The refusal check
(`url_import_refusal`) and the import functions are the 4.d ones, called as they are. The window's
`_on_auto_imported` has one effect on the list that a headless run can reproduce: with
`remove_after_import` on, an imported row is dropped. The batch drops it when the run ends (the
window drops it as each import finishes); the session the run saves does not hold it.

The Hydrus client is built once per run from the settings the run starts with (`HydrusClient::new`:
rustls with the explicit provider, `verify_ssl` from the settings), as `SearchWorker` builds one.
With `auto_import_enabled` on and an empty access key the run says so and imports nothing.

**Differences from Python, on purpose.**

1. **An exception the Python worker does not catch ends the run as `Ending::Failed`**: exit status
   1, the rest of the queue unsearched, the list and the session kept as they stand. In Python such
   an exception ends the worker thread silently (nothing reaches the user; the window sits on
   "Searching"). Three things can escape: the staging directory cannot be made (`OSError`), an
   `owner.pid` holds a number larger than a C `int` (`OverflowError` out of `os.kill`, which the
   sweep's `except OSError` does not catch), and a Hydrus answer of the wrong shape during
   auto-import (`AttributeError`/`TypeError`, `docs/hydrus-import.md`). The differential compares
   the class of each and what was done up to that point.
2. **Dependency:** `libc` in `hatate-core` for `kill(pid, 0)`. Already in `Cargo.lock` (tokio).
3. **`Entry.stale_similarity_measured` and `Entry.stale_source_name`**, two more transient fields
   in the style of section 5. A cached result whose matches have all died leaves `ImageEntry`
   with the dead match's `similarity`, `similarity_measured` and `booru_name` (`select_candidate(-1)`
   changes nothing); 3.i carried the URL, and the provenance note writes the other two. The
   first differential run found 16 scenarios (of 1,500) whose notes lacked the `Site:` line or the
   measured wording for such a row. `provenance_note` reads them through `Entry::booru_name` and
   `Entry::similarity_measured`, which fall back to the stale fields; no stored byte changes.
4. **The countdown rounds halves to even** as Python's `round()` does (found by the differential;
   `the_countdown_reports_when_the_displayed_second_changes_with_halves_to_even`).

**How it was checked.** The 3.i differential, rebuilt (the driver lives outside the repo; the
log is `worktrees/DAN-2132-differential-run3.log`), with auto-import on against a scripted
Hydrus. The Python side runs the real `SearchWorker.run()` of `76c85c4` with the HTTP transport and
the engines scripted and a fake clock; the Rust side runs `run_batch` through the binary's own
constructors against a local https server. Per scenario it compares: files found, duplicates and
added; every Hydrus request (method, URL with query, headers Python sets, JSON body, form field
names and non-file values) and, for each, whether a staging directory sat beside it; every site
request; every sleep; the search-cache files; the countdown and import events; the staging
directory's contents before and after, with `owner.pid` set up stale (dead pid, no pid file,
garbage in it) and live (a running process); the exception class when the worker dies; and the
session read back with the Python app's own `session_db.load_entries`, entry by entry.

Result at `205893b`: **1,500 scenarios** (3 seeds × 500), 4,222 entries,
5,702 Hydrus requests, 4,354 site requests, 60,125 sleeps, 1,862 auto-imports (1,101 sent), 2,763
stale staging directories set up, the three methods (`url_importer` 854, `download_send` 498,
`upload` 218) and a bogus one (292), remove-after-import (236 rows removed), refusals (243),
stops mid-run (279), and 192 runs that ended on an exception (`OSError` 47, `OverflowError` 41,
`AttributeError` 104; the class and the state at that point match in each). **0 differing.** The one class the harness cuts is the already-documented **C1/C5**
(the transport library's words after `Could not reach Hydrus at <url>: `, the image decoder's
words), 57 messages; they change a word, never a decision (`docs/hydrus-client.md`,
`docs/hydrus-import-url.md`).

Not compared: the stderr text and the exact order of two log lines.
