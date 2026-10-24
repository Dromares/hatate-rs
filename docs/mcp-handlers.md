# The MCP tool bodies and `mcp_images` (Phase 5.e, DAN-2167)

The fourteen tool bodies of `gui/mcp_bridge.py` (`McpToolHandlers`) without Qt, the channel that
replaces `MainThreadInvoker`, and `core/mcp_images.py`. The server that calls them, its tool
registry, gates, audit log, bearer check and the stdio bridge are 5.d (`docs/mcp-server.md`); this
ticket implements the `McpHandlers` trait 5.d fixed.

Where Python's handlers drive a `MainWindow`, these drive a queue model. Phase 6's GUI owns that
model and draws it; nothing here knows how.

## 1. The cut

Ported here:

| Python | Rust |
|---|---|
| `McpToolHandlers` (all 14 tools), `_run_tool`, `_entry_at`, `_queue_row`, `_entry_detail`, `_matches_filter`, `_guess_mime` | `mcp/tool_handlers.rs` |
| `MainThreadInvoker`, `MainThreadTimeout`, `DEFAULT_TIMEOUT_SECONDS` (20), `HYDRUS_SEND_TIMEOUT_SECONDS` (60), `RESEARCH_TIMEOUT_PER_IMAGE_SECONDS` (90), `RESEARCH_TIMEOUT_FLOOR_SECONDS` (60) | `queue.rs` (`QueueChannel`, `QueueOwner`, `MainThreadTimeout`, the constants) |
| the `window.entries` list and the few window methods the tools call (`_refresh_entry_row`, `_refresh_table`, `_on_selection_changed`, `_set_rows_reviewed`, `_remove_rows`, `_remove_entries`, `ImageEntry.reset_result`, `has_result`, `needs_review`) | `queue.rs` (`QueueModel`) |
| `window._research_rows` and `_launch_search_worker(..., bypass_cache=True)` | `queue::start_research` and `batch/research.rs` (`BatchSearchLauncher`) |
| `core/mcp_images.py` | `mcp_images.rs` |

Not here, and who owns it:

* The window itself, the table and preview it refreshes, the status line, and the GUI thread. Phase 6
  owns the `QueueModel` through a `QueueOwner`, drains `QueueModel::take_events()`, and starts the
  server (`McpServerController` plus `McpToolHandlers`) from Settings > MCP. No CLI subcommand
  starts the server in Phase 5.
* `SearchWorker`'s signals (`image_updated`, `progress`, `waiting`, ...). The re-search runs the
  merged `search_pending` over copies of the entries and hands the searched copies back; the live
  progress display is Phase 6's.
* `docs/phase5-coverage.md` lists every module of the Python app and its owner.

## 2. The queue model and its channel

`QueueModel` holds the entries in queue order (each with an `EntryId`, the stand-in for Python's
`id(entry)`), the settings object the window shares with the tools, the current row, and whether a
search is running. Rows are addressed by position in the full queue, never by what a filter shows.

`QueueChannel::call(closure, timeout)` is `invoker.call`: the closure runs on the owner's thread
against the model and the caller waits. A timeout is `MainThreadTimeout` with Python's text,
`Timed out after 20s waiting for the GUI thread`, and the closure keeps its place in the owner's
queue (Python's blocked emit does the same). An error from the closure is `RuntimeError(str(exc))`,
that is, its message. `shutdown()` makes later calls fail with `cannot schedule new futures after
shutdown`. The owner is a `QueueOwner` that the GUI pumps; `spawn_owner_thread` is a plain owner for
tests and a headless host.

What the window does after a change is a `QueueEvent` (`RowChanged`, `TableRefreshed`, `Removed`,
`SelectionChanged`, `SearchRunning`). `QueueModel::remove` moves the current row to the next
survivor down, else the nearest above, as `_successor_surviving` does.

## 3. The tools

`run_tool` is `_run_tool`: it reads `window.settings.mcp` on every call, gates the call
(`mcp::tools::gate`), applies `dry_run` to the write tiers, audits every call (refused, dry-run and
allowed alike) and returns `{"allowed": ..., **result}`. A body that raises is not audited, as in
Python.

* The read tools and the local writes run as one closure on the owner.
* `get_images` and `get_diff` look the entry and candidate up on the owner, then fetch on the
  tool's own thread (`mcp_images`), as Python does. A candidate that `fetch_candidate_details`
  filled in is written back to the model, because Python fills the candidate in place and the
  session keeps it.
* The Hydrus sends (`send_upload`, `send_url`, `download_send`) call the merged paths:
  `send_file_upload`, `send_url_or_download`, `download_and_send`. See section 5, D1.
* `research` starts the merged `search_pending` through a `SearchLauncher` and waits on a flag for
  `max(60, 90 * count)` seconds. The launcher copies the entries, runs with the search cache
  bypassed (`use_search_cache = false` in a copy of the settings, the effect of `bypass_cache=True`),
  writes no session (the owner saves its own list), and hands the copies back. A row
  `remove_after_import` dropped leaves the list.

## 4. How it was checked

**Wire and transport differential** (the harness lives outside the repo; the log is
`worktrees/DAN-2167-differential.log`). The same session file (written by Python's own
`write_session_payload`), the same scripted CDN and the same scripted Hydrus (plain http on fixed
local ports, a synthetic access key) go to Python `76c85c4` (FastMCP of `mcp 1.30.0`; the real
`McpToolHandlers`, with its real `MainThreadInvoker`, and the real `McpServerController`, over a stub
window on a Qt core event loop) and to the Rust server with these handlers. A seeded
generator (`gen.py SEED N`) makes a list of 10 to 14 entries with candidates (parsed and unparsed
sites, direct and preview URLs, local CDN URLs, unreadable and truncated and missing local files)
and N tool calls: every tool, valid and invalid rows, candidate indexes and filters, gate and
`dry_run` and `remove_after_import` flips between calls, and Hydrus modes that fail each step.
Compared: every HTTP status, content type and body of the JSON-RPC exchange (`serverInfo.version`
normalised, granted by name), every request each scripted server saw (method, target, referer,
content type, access-key header, body with the temporary directory name normalised), the audit-log
lines (timestamps normalised), the session state after every 40th call (every call in a debug run)
and after the last, and the search-cache directory.

Result at the head named in the log: seeds 1 to 8, 400 calls each, **3200 cases, 0 differences
except class P1, granted by name on DAN-2209** (section 5).

**Named tests** (`crates/hatate-core/tests/mcp_tool_handlers.rs`, `src/queue.rs`): the Python tests
mapped in section 6, plus the transport tests: `get_images` and `get_diff` over a self-signed local
**https** server through the 1.c net layer (the scheme the matches ship on), the Hydrus sends over
plain http through `HydrusClient::new` (the scheme `hydrus.api_url` ships on), and one timeout and
one failure for each client: a download bounded by `hydrus.timeout`
(`the_download_is_bounded_by_the_hydrus_timeout_setting`), a failed download
(`a_failed_download_is_reported_rather_than_crashing`), a send past its limit
(`a_send_that_outlives_its_limit_gives_the_timeout_text_and_still_finishes`), a Hydrus that is down
(`a_hydrus_that_is_down_is_a_failed_send_with_the_python_message`), a queue that never answers
(`a_handler_whose_queue_never_answers_gives_pythons_timeout_text`).

## 5. Differences from Python

**P1 (granted, [DAN-2209](/DAN/issues/DAN-2209)): the PNG container bytes of `get_diff`'s
`diff_image`.** Matched by decoded picture (pixels, mode, bit depth, colour type, size) in every case;
not by bytes. 37 `get_diff` responses in the 3200 cases (seeds 2 to 8: 7, 2, 2, 7, 8, 10, 1; seed 1
has none), decoded-equal 37 of 37, and the same 37 audit lines (Python's audit line prints the image
as `str(bytes)`, and so does the Rust one, so the line differs in the same substring only). Cause:
Pillow 12.3.0 in the venv is built on zlib-ng and chooses PNG row filters with its own heuristic; the
Rust encoder is the `png` crate's. A probe (not committed) fed Pillow's filtered rows one row at a
time to `libz-rs-sys` 0.5 (zlib-rs) at level 6, memLevel 9: 50 of 60 vectors reproduced Pillow's IDAT
stream, 10 did not. No crate available reproduces zlib-ng's deflate exactly, and the sizes differ by
5 to 15 percent (806 against 874, 737 against 808). Cloud ruled on DAN-2209 (2026-10-24): granted by
name, for this field only, under these conditions:

1. Decoded equality on every `get_diff` case: both PNGs decoded, pixels, mode (`L`), bit depth and
   size compared exactly; the count and one pixel hash per side are reported.
2. Python decodes the Rust PNG: Pillow's `Image.open(...).load()` on every Rust `diff_image`, warnings
   as errors; the `png` crate decodes every Python one.
3. Everything around the bytes stays byte-equal: content-block `type`, `mimeType`, block order and the
   text block. In the audit lines only the image-bytes substring is masked; the rest of each line is
   compared byte for byte.
4. No new dependency for this: no `libz-ng-sys` and no `libz-rs-sys` is added to `Cargo.lock` (the
   `zlib-rs` already in the lock is a transitive entry on `main`; this PR's `Cargo.lock` diff is empty).
   The probe stays out of the tree.
5. The differential names it: `P1 (granted, DAN-2209): N cases, decoded-equal N/N`, and 0 other
   differences.

The grant covers the PNG container bytes of that one block and the matching `str(bytes)` substring of
that tool's audit line, and nothing else. Python is not changed. The audit line's whole-image dump is
log growth, not a wrong result; no Python fix will be filed (a Phase 7 candidate).

**D1: a Hydrus send does not hold the model.** Python runs `send_*` ON the GUI thread: the window is
blocked until Hydrus answers or 60 s pass. Here the entry is copied out, sent on the tool's thread,
and written back in one step on the owner (with the removal or row refresh the window would do). The
call keeps the 60 s limit and its text; a send past it still finishes and still writes back, as
Python's does. Not wire-visible. The cost: an edit made to the same row while its send is in flight
is overwritten by the send's result.

**D2: a search-cache key that is not 64 lowercase hex digits** is refused by `CacheKey` (a hash can
never be a path); Python joins whatever the entry holds into a path. `reset_result` then reports
`dropped_from_cache: false`. Session hashes are always SHA-256 digests.

**D3: no same-thread shortcut.** Python's `call` runs the callable directly when the caller is the
GUI thread. No tool takes that path, and the Rust channel has no model to offer there; a call from
the owner's own thread would wait for itself.

**D4: a dead owner is a timeout at once.** If the owner is gone, the call fails with the timeout text
immediately instead of after the limit.

**D5: the audit log prints whole images**, in `get_images` and `get_diff` lines, because Python's
`json.dumps(default=str)` does. It is faithful, and it makes the log grow by the size of each image
(the rotation cap still applies). Reported, not changed (Python is fixes-only; a change is a
Python-side decision).

## 6. The Python tests

`tests/test_mcp_bridge.py`:

| Python test | Rust test |
|---|---|
| `TestMainThreadInvoker.test_runs_the_callable_on_the_thread_that_constructed_it` | `queue::tests::a_call_runs_on_the_owners_thread` |
| `test_returns_the_callables_result`, `test_an_exception_in_the_callable_is_raised_in_the_caller` | `queue::tests::a_call_reaches_the_owner_and_a_failure_is_the_message` |
| `test_a_same_thread_call_runs_directly` | not applicable: no tool takes the path (D3) |
| `test_times_out_if_the_callable_runs_longer_than_the_timeout` | `queue::tests::a_closure_that_runs_longer_than_the_timeout_times_out`, `an_owner_that_never_answers_gives_pythons_timeout_text` |
| `TestReadTools.test_list_queue_reports_every_row_by_default` | `list_queue_reports_every_row_by_default` |
| `test_list_queue_respects_offset_and_limit` | `list_queue_respects_offset_and_limit` |
| `test_list_queue_filter_keyword_unreviewed` | `list_queue_filter_keyword_unreviewed`, `tool_handlers::tests::the_filter_language_is_the_small_one` |
| `test_get_entry_reports_the_requested_row` | `get_entry_reports_the_requested_row` |
| `test_get_entry_out_of_range_reports_an_error_not_a_crash` | `get_entry_out_of_range_reports_an_error_not_a_crash` |
| `test_list_actions_reports_every_tool_with_its_gate_state`, `test_list_actions_reports_hydrus_writes_enabled_by_default` | `list_actions_reports_every_tool_with_its_gate_state` |
| `TestLocalWrites.test_select_candidate_switches_the_active_match` | `select_candidate_switches_the_active_match` |
| `test_toggle_reviewed_sets_the_flag`, `test_local_writes_are_never_gated` | `toggle_reviewed_sets_the_flag_and_local_writes_are_never_gated` |
| `TestGatingAndAudit.test_a_disabled_tier_is_refused_with_the_setting_named` | `a_disabled_tier_is_refused_with_the_setting_named_and_the_entry_survives` |
| `test_a_hydrus_write_is_refused_without_calling_hydrus_at_all` | `a_hydrus_write_is_refused_without_calling_hydrus_at_all` |
| `test_a_refusal_is_still_recorded_in_the_audit_log`, `test_an_allowed_call_is_recorded_too` | `a_refusal_and_an_allowed_call_are_both_audited` |
| `TestDryRun.test_dry_run_changes_nothing_and_says_so`, `test_dry_run_is_still_audited` | `dry_run_changes_nothing_says_so_and_is_audited` |
| `test_turning_dry_run_off_performs_the_real_action` | `turning_dry_run_off_performs_the_real_action` |
| `test_read_tools_ignore_dry_run` | `read_tools_ignore_dry_run` |

`tests/test_mcp_images.py`:

| Python test | Rust test |
|---|---|
| `TestLocalImageBytes.test_reads_the_real_file`, `test_a_missing_file_returns_none_rather_than_raising` | `local_image_bytes_reads_the_real_file_and_a_missing_one_is_none` |
| `TestFetchFullResolutionMatch.test_prefers_the_direct_file_url_over_the_preview` | `get_images_returns_both_pictures_at_full_resolution_over_https` (also checks the referer) |
| `test_falls_back_to_the_preview_when_no_direct_url_exists` | `a_preview_is_the_fallback_and_says_so_with_no_referer` |
| `test_tries_to_fetch_details_when_neither_url_is_known_yet` | `match_details_are_fetched_when_neither_url_is_known_and_kept_on_the_candidate` |
| `test_no_url_at_all_is_reported_rather_than_attempted` | `no_url_at_all_is_reported_rather_than_attempted` |
| `test_a_failed_download_is_reported_rather_than_crashing` | `a_failed_download_is_reported_rather_than_crashing` |
| `TestComputeDiff.test_identical_images_report_no_difference` | `get_diff_of_identical_pictures_reports_no_difference_with_a_png_mask`, `compute_diff_reports_missing_and_undecodable_input_rather_than_failing` |
| `test_missing_local_bytes_is_reported_rather_than_crashing`, `test_undecodable_bytes_are_reported_rather_than_crashing` | `compute_diff_reports_missing_and_undecodable_input_rather_than_failing` |

The other Python tests that name these classes: `tests/test_mcp_server.py` exercises the server with
a recording handler set (5.d, `docs/mcp-server.md`); `tests/test_mcp_settings_tab.py` fakes the
handlers for the settings tab (Phase 6); `tests/test_gui_harness.py` lists `MainThreadInvoker` and
`McpToolHandlers` as members of the GUI inventory (Phase 6).

## 7. Error shape

`InvokeError` (a timeout, or a failure of the closure) exposes `kind()`
(`ErrorKind::McpQueueTimeout`, `McpQueueFailed`), `user_message()` (what failed and what to do; no
path, URL, key or OS text) and `detail()` (Python's message). A tool's own failure reaches the
client as `HandlerError`, whose text is `str(exc)` as FastMCP prints it. `Entry.error_message` and
`MatchStatus` are unchanged on disk.
