# Search cache and startup files (`hatate_core`, DAN-1982)

Phase 1.d of the Rust rewrite. Contract: `docs/file-formats.md` sections 4, 5.2, 5.3, 5.5
and rules 6.3 and 6.4. Python source of truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`
(`core/search_cache.py`, `core/saucenao.py:244-335`, `core/crashlog.py`, `core/applog.py`).
Out of scope here: `matched_urls.log`, `mcp_audit.jsonl`, sidecars, exports, Hydrus writes,
`pawchive_index.db` (Phases 2, 4, 5).

| Module | File | What it does |
|---|---|---|
| `cache` | `search_cache/<sha256>.json` | typed entry, lookup (hit or classified miss), atomic store, drop, clear, size |
| `quota_pause` | `saucenao_quota_pause.json` | read, honour at startup, write the four exact keys, clear |
| `crash_marker` | `crash.log.running`, `last_clean_exit` | take part in the protocol: marker at start, removed on clean exit |
| `rotating_log` | `hatate-rs.log` (+ `.1` `.2` `.3`) | the Rust app's own log, rotated like `app.log` |
| `fsutil` (private) | | temp file in the same directory, fsync, rename |

## Decisions

- **The Rust app takes part in the crash-marker protocol** (board decision on DAN-1982). It
  creates `crash.log.running` after looking for the previous one, and removes it, then
  writes `last_clean_exit`, at the end of a clean exit (`RunSession::finish_clean`). There is
  no `Drop` that does this: a panic unwinds through `Drop`, and a crash must not be called
  clean. `crash.log` (Python's faulthandler output) is not written.
- **The Rust log is `hatate-rs.log`, never `app.log`** (board decision on DAN-1982). The name
  collides with nothing in spec 1.2 or 1.3, nor with a rotated copy of one, nor with
  `hatate-rs.json` (DAN-1979); `tests/paths_sandbox.rs` checks that against the spec text.
  Differences from `app.log`, on purpose: stamps are **UTC** (there is no time-zone database
  in the tree; the first line of each run says so), and nothing reads this file back.
- **Atomic writes.** Python writes the cache and the pause file in place, so a crash can leave
  a truncated file. Rust writes `.<name>.<pid>.<n>.tmp` next to the target, fsyncs, renames.
  The temp name starts with a dot and ends in `.tmp`, so Python's `*.json` globs (size,
  clear) never see it; `SearchCache::clear` also removes leftovers of a killed run.
- **Keys are validated.** `CacheKey` is exactly 64 lowercase hex digits, so a hash can never
  be a path and both apps always name the same file. Python does not validate.
- **Reads copy Python's leniency, writes copy `json.dumps`.** Strings holding numbers
  (`"12"`) are coerced like `int()`/`float()`; writing a sample back gives Python's bytes
  (one line, `", "` and `": "`, `\uXXXX` for non-ASCII, `repr` floats). Where Python would
  crash on a hand-edited file (a non-number `selected_candidate_index`, an offset-less
  `reset_at` compared with an aware time), Rust reads it as a miss, 0, or UTC.
- **`tag_version`** is `1` for a MangaDex chapter URL (`mangadex.matches`, including its
  quirk that the host test uses the whole `netloc`, port included) and `0` otherwise.

## Where each rule is tested

All tests use temp profiles (`tests/common::Sandbox`, the DAN-1979 guard) or `tempfile`
directories; none touches a real profile.

| Rule | Test |
|---|---|
| 6.3 key is the lowercase SHA-256 | `tests/cache_search.rs::key_is_lowercase_sha256_hex`, `::key_refuses_anything_that_is_not_64_lowercase_hex` |
| 6.3 `format_version: 2`, the six keys, the twenty candidate keys | `::a_stored_entry_has_the_six_keys_and_format_version_2` |
| 6.3 `tag_version` 1 for MangaDex, else 0 | `::tag_version_is_1_for_mangadex_and_0_otherwise`; `src/cache/model.rs::mangadex_tag_version_matches_the_python_parser` |
| 6.3 never cache `error` | `::error_results_are_never_cached`, `::a_skipped_store_leaves_an_existing_entry_alone` |
| 6.3 never cache provisional | `::provisional_results_are_never_cached` |
| 6.3 unreadable or truncated is a miss | `::every_truncation_of_a_real_entry_is_a_miss`, `::unreadable_and_garbage_files_are_misses`, `::valid_json_that_is_not_an_entry_is_a_miss` |
| 6.3 atomic write, simulated kill at three stages | `src/cache/mod.rs::kill_tests`, `src/fsutil.rs` tests |
| 6.3 atomic write, real `SIGKILL` of a writer process | `tests/cache_kill.rs` (fails against an in-place writer: see the PR) |
| 4 every corpus sample is a hit; writing one back is byte-identical | `::every_corpus_sample_reads_as_a_hit`, `::the_good_sample_reads_field_for_field`, `::writing_a_sample_back_is_byte_identical_to_python` |
| 4 only `not_found` expires; the read deletes it; TTL 0 off; no `cached_at` | `::only_not_found_expires_and_the_read_deletes_it`, `::ttl_zero_or_negative_never_expires`, `::a_not_found_without_a_usable_timestamp_counts_as_expired` |
| 4 old format keeps candidates, marks them unfetched; tag version change | `::an_old_format_keeps_the_candidates_but_marks_them_unfetched`, `::a_changed_tag_version_rereads_tags_but_not_remote_info` |
| 3.4.1 availability verdict: real bool and a date within 30 days | `::an_availability_verdict_needs_a_bool_and_a_date_within_30_days` |
| 4 drop, clear, size | `::drop_clear_and_size`, `::temp_files_do_not_look_like_cache_entries` |
| 5.5 / 6.4 exact keys and bytes; reset is the next 00:00 UTC | `tests/startup_files.rs::the_sample_parses_and_writes_back_byte_identical`, `::record_writes_exactly_the_four_keys_in_order`, `::reset_is_the_next_utc_midnight` |
| 5.5 / 6.4 honour an existing file; delete a stale or unparsable one; leave a corrupt one | `::an_existing_python_file_is_honoured_while_in_force`, `::a_pause_whose_reset_has_passed_is_deleted_at_startup`, `::an_unparsable_reset_is_deleted_at_startup`, `::a_missing_or_corrupt_file_means_no_pause_and_is_left_alone` |
| 5.3 / 6.4 marker removed on clean exit, no banner for the next launch | `::a_clean_run_leaves_no_marker_and_no_banner_for_the_next_launch` |
| 5.3 a crash or panic leaves the marker | `::a_run_that_never_finishes_cleanly_is_seen_by_the_next_launch`, `::a_panic_is_not_a_clean_exit`, `::a_marker_left_by_the_python_app_is_seen_and_replaced` |
| 5.2 / 6.4 `hatate-rs.log`, rotation at `maxBytes`, three backups, `app.log` untouched | `::the_rust_log_is_hatate_rs_log_and_never_app_log`, `::rotation_happens_exactly_at_max_bytes`, `::rotation_keeps_three_backups_and_drops_the_oldest`, `::the_rust_log_never_touches_app_log_files_even_when_rotating` |
| the log name collides with nothing in 1.2 / 1.3 | `tests/paths_sandbox.rs::rust_log_file_collides_with_nothing_in_sections_1_2_and_1_3` |

## Black-box check against the Python app

`tests/blackbox_emit.rs` (ignored by default) writes a profile with the Rust code into a
directory under `$TMPDIR`. A verifier points a `76c85c4` checkout of the Python app at it
(`XDG_CONFIG_HOME=<dir>/config`, temp `HOME`) and calls the app's own `load_cached_result`
and `apply_cached_result`, `get_quota_pause_state`, `check_unclean_shutdown` and
`last_clean_exit_age_seconds`. The Python side is a throw-away script outside this
Rust-only tree; the PR records its output.
