# Hydrus URL import, import poll and reconcile (Phase 4.d, DAN-2131)

What this ticket ports, where the cut is, how it was checked against the real Python, what is not
matched, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. It builds on 4.a's `HydrusClient`
(`docs/hydrus-client.md`), 4.b's write calls (`docs/hydrus-client-write.md`) and 4.c's file import
(`docs/hydrus-import.md`) as they are, and on the Phase 3 `hydrus_url`, `image_compare` and
`pyraster` modules.

## 1. The cut

| Python | Rust (`crates/hatate-core/src`) |
|---|---|
| `core/hydrus_import.py`, the URL-import half: `url_import_refusal`, `_forwarded_url`, `relate_confirmed_url_import`, `finish_url_import`, `send_url_or_download`, `download_instead`, `send_url_to_importer`, `download_and_send` | `hydrus_import/url.rs` (a child module of 4.c's `hydrus_import`, re-exported from it): the same names without the underscore, plus `UrlImportError` |
| `core/hydrus_import_poll.py`: `extract_confirmed_hash`, `poll_single_url_import` | `hydrus_import_poll`: the same names, plus `PollOptions` |
| `workers/hydrus_import_poll_worker.py`'s `run`, without Qt | `hydrus_import_poll::run_import_poll` (`PollItem`, `PollWorkerOptions`, the `PollEvents` trait standing for the three signals) |
| `core/hydrus_reconcile.py`: `ReconcileResult` (+ `summary`), `entries_awaiting_confirmation`, `reconcile` | `hydrus_reconcile`: the same |
| `workers/hydrus_reconcile_worker.py`'s `run`, without Qt | `hydrus_reconcile::run_reconcile` / `run_reconcile_with` |

Small things that came with it: three `ErrorKind` variants (`DownloadFailed`, `DownloadInvalid`,
`HashMismatch`) and their `user_message` arms in `error.rs`; `pycompat::splitext`; `FromIterator` for
`DeletionStates`; and the visibility of four 4.a helpers (`unexpected`, `no_attribute`, `dict`,
`iterate`) widened to `pub(crate)`.

Not here, and who owns it: wiring auto-import into `hatate batch` is 4.g. The Qt classes themselves
(`QThread`, the signals' delivery) and every GUI caller (the table menu, the review shortcuts, the
reconcile action's dialogs) are Phase 6; what their `run` does is ported and tested here.

No new dependency. No change to `Entry.error_message` or `MatchStatus` on disk: the error text
Python writes is carried as Python's text, byte for byte, and `UrlImportError` /
`HydrusError` expose `kind()`, `user_message()` (what failed and what to do; no path, URL query,
key or OS text) and `detail()` as a side channel.

## 2. How it is built

- **Errors.** Python's functions catch `HydrusError` and turn it into an `ImportResult` or a
  warning; anything else propagates. A function here returns `Err` only for what Python lets
  propagate: a Hydrus answer of the wrong shape (`ErrorKind::UnexpectedResponse`, Python's
  `AttributeError`/`TypeError`), `ValueError` from `urlparse`, a hard deadline, an `OSError`
  writing the downloaded file. `UrlImportError::message()` is Python's `str(exc)`;
  `user_message()` is Hatate's own text.
- **The redirect probe** (`forwarded_url`, one HEAD, 15 s hard deadline) goes through the net layer,
  and reads `urlparse(url)` before it sends, like `core.net.request`: a bad-bracket address is a
  `ValueError` and sends nothing. (The differential found this in the first port; fixed.)
- **The poll** (`poll_single_url_import`) takes the clock as a parameter (`Clock`), so a test
  injects `ManualClock` and compares the sleeps. It retries on what Python catches as
  `HydrusError` and lets everything else out, and its last sleep never overshoots the timeout.
- **The poll worker** (`run_import_poll`) runs at most eight checks at once (`join_bounded`), as
  Python's thread pool does. A stop requested while checks are in flight drops their results and
  emits nothing; a confirmed entry's relationship is written *after* the stop check, because a
  relationship already in the user's library cannot be taken back.
- **The reconcile worker** (`run_reconcile`) is `reconcile` with the lookup and the stop check
  supplied; `run_reconcile_with` takes the lookup as a closure, which is how the tests drive it
  without a server and also how the shipped path is reached through `HydrusClient`.

## 3. How it was checked against the real Python

The real Python at `76c85c4` (`core.hydrus_import`, `core.hydrus_import_poll`,
`core.hydrus_reconcile`, `core.hydrus_client`, `core.net`, `core.remote` over `requests`, with only
the transport scripted, and the two workers' `run()` with their PyQt6 signals connected to
recorders) ran against a scripted Hydrus; the Rust side ran offline (`unshare -rn`) against a
scripted local Hydrus over http and a scripted https site server reached through
`NetConfig::with_host_override`. The harness lives outside the repo (the tree is Rust only); its log
is `worktrees/DAN-2131-differential.log`. TZ `America/New_York`.

Compared in every case: the return value, or the error class and message; every Hydrus request
(method, target with query, key header, content type, body bytes); every site request (method, host,
target, `Referer`, `Cookie`, `User-Agent`); the files left in the work folder; every sleep and
progress or countdown callback; and the entry afterwards.

| Family | Cases |
|---|---|
| `send_importer` (`send_url_to_importer`) | 3 164 |
| `download_send` (`download_and_send`) | 3 050 |
| `send_or_download` | 2 731 |
| `poll_single` (`poll_single_url_import`) | 1 758 |
| `relate` (`relate_confirmed_url_import` / `finish_url_import`) | 1 757 |
| `poll_worker` (`run_import_poll`) | 1 700 |
| `reconcile` (`reconcile` and its worker) | 1 687 |
| `refusal` (`url_import_refusal`, `_forwarded_url`) | 1 279 |
| `extract` (`extract_confirmed_hash`) | 874 |

**Total: 18 000 cases (seeds 11 to 16, 3 000 each), 20 674 ops, 46 004 Hydrus requests, 8 052 site
requests, 0 unexplained differences.** Every Hydrus access key is 64 zeros; no live service was
contacted.

## 4. Differences that are not matched

Counted over the six seeds. C1, C2 and C5 are the classes Cloud ruled on for 4.c (DAN-2159); D1 and D2
are new here and were ruled on [DAN-2169](/DAN/issues/DAN-2169) (Option A, accepted as documented
classes, not added to any standing grant).

| Class | What | Python | Rust | Count (ops, per seed 11 to 16) |
|---|---|---|---|---|
| **C1** transport words | the library's own words after `Could not reach Hydrus at <url>: ` | `requests`' exception text | the net layer's text | 122 |
| **C2** a non-string `hash` in an `add_file` answer | `download_and_send` uploads, Hydrus answers with a number, list or object as `hash` | carries it on or fails later | `Err(UnexpectedResponse)` before the entry changes | 7 (42 later requests of those cases are not compared) |
| **C5** the decoder's words for a damaged image | the text after `Downloaded data does not decode as a valid image: ` | Pillow's exception text | the decoder's own | 31 |
| **D1** a non-string `hash` in a `get_url_files` row | the row is resolved (status 2 or 3) and its `hash` is truthy but is `5`, a list, `true` or an object | returned as the "confirmed hash" (a latent Python bug) | `Err(UnexpectedResponse)` before anything is recorded: `extract_confirmed_hash`, `poll_single_url_import` and `run_import_poll` all fail closed | 249 = 47 + 30 + 42 + 45 + 44 + 41 |
| **D2** the text of `urlparse`'s `ValueError` | an address Python's `urlparse` refuses (`https://[::1/x`) | `ValueError: Invalid IPv6 URL` and similar | the class (`UrlImportError::Url`, `ErrorKind::InvalidInput`), the call site, the entry state and every request are equal; `message()` does not carry the library's word, `user_message()` is `The address of this match is not one Hatate can read, so it was not sent.` | 61 = 9 + 9 + 15 + 11 + 6 + 11 |

**Not compared, because of D1.** In a D1 case Python carries the odd value on and the rest of that case
is not comparable: **303 later ops and 471 requests** over the six seeds are **not compared**. They
are not "matched" and are not counted as such.

What each means for the product: C1, C5 and D2 change a word in a message and never a decision. C2 and
D1 cannot happen with a real Hydrus (its hash is a 64-character string); where Python would pass the
odd value on as a "confirmed" hash, Rust refuses before it writes a relationship, a note or a state
change. Tests: `extract_confirmed_hash::a_hash_that_is_not_a_string_is_refused_not_passed_on` and
`a_non_string_hash_in_the_worker_writes_nothing_and_reports_nothing` (D1);
`an_address_python_cannot_parse_is_told_to_the_person_without_library_words` (D2).

## 5. The tests

Integration tests, all against a scripted local Hydrus and a synthetic key:

- `tests/hydrus_import_url.rs` (24), `tests/hydrus_import_poll.rs` (17) and
  `tests/hydrus_reconcile.rs` (13), plus the unit tests in `hydrus_import_poll` and
  `hydrus_reconcile`.
- **Both schemes through the binaries' own constructor** (`HydrusClient::new`, the 1.c net layer with the
  explicit CryptoProvider): plain http, and https against a self-signed local server with
  `verify_ssl` true (the connection is refused, nothing is sent) and false (it succeeds).
  Named: `the_importer_over_https_with_certificate_checks_on_is_refused_and_off_it_succeeds`,
  `the_poll_over_https_with_certificate_checks_on_never_confirms_and_off_it_does`,
  `over_https_a_self_signed_hydrus_is_refused_with_checks_on_and_reached_with_them_off`,
  `the_worker_reconciles_over_plain_http_by_hash`. The site-side download goes over https to a local
  server with its own throw-away CA.
- **One timeout and one network error** per client:
  `a_timeout_and_a_dropped_connection_fail_the_send_with_a_user_message_that_leaks_nothing`,
  `the_redirect_probe_that_hits_its_deadline_leaves_the_function_like_hard_timeout_error`,
  `a_poll_that_times_out_or_loses_the_connection_is_retried_like_any_hydrus_error`,
  `a_hydrus_that_times_out_or_drops_the_connection_leaves_everything_as_it_was`.
  The first also asserts the error's `user_message()` has no URL query, no key and no OS text.

## 6. Python tests that exercise a function in scope

`tests/test_hydrus_import_poll.py`

| Python | Rust |
|---|---|
| `TestConfirmation`: `an_already_confirmed_import_returns_at_once_without_sleeping` | `an_already_confirmed_import_returns_at_once_without_sleeping` |
| `it_keeps_checking_until_hydrus_confirms`, `both_resolved_statuses_count_as_resolved` | `it_keeps_checking_until_hydrus_confirms_and_both_resolved_statuses_count`; `only_status_two_and_three_with_a_hash_confirm` (unit) |
| `status_zero_is_not_a_confirmation`, `a_resolved_status_with_no_hash_is_not_a_confirmation` | `the_confirmed_hash_is_read_from_the_first_resolved_status`, `only_status_two_and_three_with_a_hash_confirm` |
| `TestTimeout`: `it_gives_up_after_the_timeout...`, `the_final_sleep_never_overshoots_the_timeout`, `a_zero_timeout_still_checks_once...`, `a_confirmation_on_the_very_last_check_is_still_returned` | `it_gives_up_at_the_timeout_and_the_last_sleep_never_overshoots_it` (the four cases, sleeps compared) |
| `the_defaults_are_the_ones_shared_with_the_batch_worker` | `PollOptions::default` and `PollWorkerOptions::default` use the same `DEFAULT_TIMEOUT` / `DEFAULT_INTERVAL` (60 s, 2 s), asserted in `it_gives_up_at_the_timeout_and_the_last_sleep_never_overshoots_it` |
| `TestProgress` (four) | `progress_reports_the_remaining_seconds_and_never_goes_negative_or_runs_after_a_hit` (`on_progress: None` is every other poll test) |
| `TestStopAndErrors`: the three stop tests | `a_stop_ends_the_poll_without_a_request_or_part_way_and_is_not_a_failure` |
| `a_hydrus_error_is_retried...`, `errors_all_the_way_to_the_timeout_answer_none` | `a_hydrus_error_is_retried_and_errors_to_the_timeout_answer_none` |
| `a_non_hydrus_exception_is_not_swallowed` | `an_answer_of_the_wrong_shape_is_not_swallowed` |

`tests/test_urls_and_availability.py` `TestHydrusImportConfirmation` (four) ->
`the_confirmed_hash_is_read_from_the_first_resolved_status` (the four cases in order, as one test) and
`an_answer_of_the_wrong_shape_is_python_s_attribute_error` (the malformed one).

`tests/test_hydrus_import_poll_worker.py`

| Python | Rust |
|---|---|
| `test_stop_cancels_inflight_futures` | `a_stop_emits_nothing_more_and_still_clears_the_countdown_and_finishes` (a stop arrives while checks are in flight; no entry is resolved). The Python test also asserts a QThread stopped: does not apply, no Qt thread; `finished_all` is asserted instead |
| `test_stop_before_submit_emits_finished` | the same test, stop requested before the first round: no request, `finished` once |

`tests/test_url_importer_duplicate_relationships.py`

| Python | Rust |
|---|---|
| `TestFeatureOff` (four) | `with_the_setting_off_a_confirmed_url_import_costs_nothing` (off, `None`, no fetch-back); the default of the setting is the Phase 1 config tests |
| `TestFeatureOn`: `the_confirmed_file_is_read_back_and_paired_as_the_better_copy` | `the_confirmed_file_is_read_back_and_paired_as_the_better_copy` |
| `a_different_edit_is_filed_as_an_alternate...`, `an_unconfident_verdict_writes_nothing`, `the_deletion_state_check_still_guards_the_write`, `a_missing_permission_is_named_and_the_import_still_stands` | the verdict table, the alternate, the deletion-state guard and the permission warning are `record_duplicate_relationship`, 4.c's (`docs/hydrus-import.md`); this ticket's function calls it, and the calls are covered by the read-back test above and `a_read_back_failure_becomes_a_warning_not_a_failed_import` |
| `nothing_is_written_when_there_is_no_local_hash`, `..._when_hydrus_landed_on_the_file_the_user_had` | `nothing_is_related_without_a_local_hash_or_when_hydrus_landed_on_the_same_file` |
| `TestReadBackFailures` (four) | `a_read_back_failure_becomes_a_warning_not_a_failed_import` (a hash with no id); the failed download and the temp file are 4.c's `imported_bytes` (a private 0600 temp file removed on drop, so there is no cleanup to assert; 519 differential cases in 4.c) |
| `TestFileIdLookup` (six) | 4.a's `file_id_for_hash` (`docs/hydrus-client.md`) |
| `TestAutoImportWiring` | `finish_url_import` in `a_read_back_failure_becomes_a_warning...` and `with_the_setting_off_...`; the batch wiring itself is 4.g |
| `TestBatchPollWorkerWiring` (seven) | `a_confirmed_import_is_related_before_it_is_reported_and_off_costs_nothing` (paired before the signal, the warning rides on it, off and no settings cost nothing), `bookkeeping_that_goes_wrong_does_not_take_the_confirmation_down`, `a_stop_emits_nothing_more...` (an unconfirmed entry emits three values) |

`tests/test_search_quality.py`

| Python | Rust |
|---|---|
| `TestHydrusUrlImporterGuard`: `a_page_hydrus_cannot_read_is_refused`, `a_gallery_is_refused`, `posts_and_direct_files_go_through` | `hydrus_s_own_answers_decide_which_urls_the_importer_is_not_handed` |
| `hydrus_not_answering_does_not_block_imports` | `hydrus_not_answering_does_not_block_imports` |
| `a_refused_url_is_not_sent_or_marked_sent` | `a_refused_url_is_not_sent_or_marked_sent` |
| `auto_import_falls_back_to_a_match_hydrus_can_take`, `the_fallback_never_goes_below_the_threshold` | do not apply here: they run `SearchWorker._maybe_auto_import`, which is 4.g |
| `TestLegacyUrlsRewrittenForHydrus`: the rewrite tests | `normalize_url_for_hydrus` is `hydrus_url` (Phase 3); `the_rewritten_url_is_what_hydrus_is_asked_about_and_sent` for the call |
| `TestSendUrlFallsBackToHatatesDownload` | `hatate_downloads_what_hydrus_cannot`, `a_forwarding_legacy_link_is_followed_and_repointed`, `nothing_that_can_reach_the_original_says_so`, `a_web_page_in_place_of_the_image_is_not_uploaded`, `the_downloaded_size_must_match_the_size_the_head_reported`, `the_rewritten_url_is_what_hydrus_is_asked_about_and_sent` (the accepted URL is used as before), `a_failed_request_is_not_mistaken_for_a_refusal`; `auto_import_downloads_when_no_match_hydrus_can_take` is 4.g |
| `TestExactCopiesAreUploadedWhenHydrusCantFetchThem` | `an_exact_copy_is_uploaded_from_disk_when_hydrus_cannot_take_the_url` |

`tests/test_hydrus_import_validation.py` (the validators, reached through `download_and_send`)

| Python | Rust |
|---|---|
| `TestDownloadValidation`: the empty, tiny, wrong-magic, valid, extensionless, HTML, corrupted-image and claimed-format tests | `bytes_that_are_not_the_claimed_format_or_do_not_decode_are_not_uploaded` and `a_web_page_in_place_of_the_image_is_not_uploaded`; the validators themselves are 4.c's (`magic_bytes_follow_the_claimed_format_...`, `a_decode_check_passes_...`) |
| `size_check_still_works_when_known` | `the_downloaded_size_must_match_the_size_the_head_reported` |
| `rejects_hydrus_hash_mismatch`, `accepts_matching_hydrus_hash` | `a_hash_hydrus_returns_that_is_not_the_files_is_refused_before_the_url_and_tags`; the matching case is the `Fake` Hydrus in `hatate_downloads_what_hydrus_cannot` |

`tests/test_hydrus_duplicate_relationships.py` and `tests/test_hydrus_provenance_note.py`: the
`download_and_send` / `finish_url_import` end-to-end tests (`a_whole_import_is_unchanged`,
`end_to_end_through_download_and_send`, `the_url_importer_path_is_unchanged`,
`end_to_end_through_the_url_importer_path`, `settings_of_none_is_treated_as_off`,
`a_failed_note_does_not_hide_a_failed_relationship`) -> `with_the_setting_off_a_confirmed_url_import_costs_nothing`,
`hatate_downloads_what_hydrus_cannot` and `the_confirmed_file_is_read_back_and_paired_as_the_better_copy`;
the note is `write_provenance_note` (4.c). `TestAutoImportWiring` there is 4.g.
`TestPermissionProbe.inconclusive_is_not_treated_as_missing` (via `url_import_refusal`) -> 4.a's
`missing_permission` tests, and `hydrus_not_answering_does_not_block_imports` here.

`tests/test_session.py` `TestHydrusSentState.test_url_importer_is_sent_but_not_confirmed` ->
`the_rewritten_url_is_what_hydrus_is_asked_about_and_sent` (asserts `sent_to_hydrus` and not
`hydrus_import_confirmed`). `tests/test_hydrus_client_write_path.py`
`TestUrlInfoReaders.test_neither_reader_swallows_a_hydrus_error` -> 4.a's client tests, and
`a_hydrus_error_is_retried_and_errors_to_the_timeout_answer_none` for the retry it protects.

`tests/test_core_utils.py` `TestHydrusReconcile` (thirteen)

| Python | Rust (`tests/hydrus_reconcile.rs`) |
|---|---|
| `a_file_hydrus_holds_is_confirmed`, `a_file_hydrus_never_saw_stays_queued` | `a_file_hydrus_holds_is_confirmed_and_one_it_never_saw_stays_queued` |
| `a_file_deleted_in_hydrus_still_counts_as_imported` | `a_file_deleted_in_hydrus_still_counts_as_imported` |
| `entries_never_sent_are_not_touched`, `already_confirmed_entries_are_not_re_asked`, `an_entry_with_no_hash_is_left_alone` | `entries_not_sent_already_confirmed_or_without_a_hash_are_not_asked_about`; `only_sent_unconfirmed_entries_with_a_hash_are_waiting` (unit) |
| `confirmation_is_never_taken_away` | `confirmation_is_never_taken_away` |
| `two_rows_sharing_a_hash_both_get_the_answer`, `each_hash_is_asked_about_once` | `two_rows_sharing_a_hash_both_get_the_answer_from_one_question` |
| `hashes_are_asked_in_batches` | `hashes_are_asked_in_batches_and_progress_is_reported_per_batch` |
| `it_can_be_stopped_part_way` | `it_can_be_stopped_part_way` |
| `a_lookup_returning_nothing_is_survived` | `a_lookup_with_no_answer_is_survived_and_an_error_ends_the_call` |
| `nothing_waiting_asks_hydrus_nothing` | `nothing_waiting_asks_hydrus_nothing`, `the_worker_with_nothing_waiting_makes_no_request` |

`tests/test_gui_smoke.py`: `TestHydrusReconcileAction` (six) is the GUI action around the worker; what
the worker does is `the_worker_reconciles_over_plain_http_by_hash`,
`the_worker_with_nothing_waiting_makes_no_request`, `a_hydrus_that_times_out_or_drops_the_connection_...`
and `the_summary_reads_like_python_s` (the quiet-pass text); the dialogs are Phase 6.
`TestSendUrlToHydrusImporter`, `TestDownloadAndSendToHydrus`, `TestWorkerCallbacksDuringClose`,
`TestTableContextMenu` and `TestReviewShortcuts` are the table, menu and close handling around these
functions, Phase 6. The functions they patch in are the ones above.
`tests/test_workers_background.py`, `test_interrupted_banner.py`, `test_quota_pause_banner.py`,
`test_sent_tag_filter.py`: mention a worker name only for the GUI's own state and do not call a
function in scope.
