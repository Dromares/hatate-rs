# Hydrus and Pawchive in the window (Phase 6.g)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of [DAN-2220](/DAN/issues/DAN-2220).
It holds the two maps Phase 6 asks every ticket for (the method map and the Python test map; 6.j checks the union), the
differential, the on-purpose differences with the assertion that pins each, the measured minimum size and the shot set.
Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`. **Every Hydrus and Pawchive answer in a test, a differential or a
shot comes from a scripted local server; every access key is a synthetic placeholder; the board's Hydrus is never
contacted.**

## 1. The cut

**Ported here** (`hatate-gtk`; the Hydrus client, the import paths and the Pawchive crawler are the merged `hatate-core`
modules and are not changed):

- `hydrus_ops.rs`: the tag lookup after Add Files and its result, Re-check Queued Imports (and the quiet pass after a
  session restore), Send to Hydrus, Import URL to Hydrus with its confirmation poll, Download and Send, Show in Hydrus
  with its explanation of a refusal, and Delete from Hydrus. Every action is gated on `is_configured` as in Python, runs
  off the main thread over copies of the rows, and is wired to the Files/Hydrus menu, the Queue context menu, the Review
  page's three buttons and their shortcuts.
- `hydrus_query.rs`: the Query Hydrus window and `action_query_hydrus`, including the empty-state button 6.c left
  disabled.
- `pawchive_dialog.rs`: the Pawchive Index window and `action_pawchive_index` (non-modal, one window, closes with the main
  window).
- `thumbnails.rs`: Hydrus thumbnails through 6.d's scheduler (`filter_known_hashes` in batches of 256, `fetch_thumbnail`
  for held files, the disk for the rest and for a thumbnail that will not decode).

**Not in this PR:** Preferences (6.h) is the only `deferred()` action left in the menu; the Shortcuts list and rebinding a
key are 6.h; the full-window sweep is 6.j.

## 2. Differences from Python, on purpose

| Id | What differs | Why | Pinned by |
|---|---|---|---|
| **D-A** | Delete from Hydrus asks with **Delete / Cancel**, Cancel the default and Escape. Python's box is Yes / No with No the default. | The ticket text ("Delete is destructive, so Cancel is the default") and design gate 2.6. | `smoke::hydrus_delete_defaults_to_cancel_and_only_deletes_after_the_answer` |
| **D-B** | A failure box's headline is `{prefix}: {user_message()}` and the raw text is in a detail line; Python writes `{prefix}: {str(exc)}`. The prefix is equal in every case. `Entry.error_message` still holds `str(exc)`, byte-compatible. | DAN-2116 item 1: the window shows `user_message()`, `detail()` goes to a detail line. | `smoke::hydrus_send_reports_what_failed_in_one_box_and_leaves_the_rows_unsent`, `smoke::hydrus_show_opens_a_page_and_explains_a_refusal` |
| **D-C** | A second Hydrus request while one is running gets a box ("already under way") instead of queuing behind it on the GUI thread. | Python's send loops ran on the GUI thread; here nothing blocks the loop. | `smoke::hydrus_requests_leave_the_main_loop_running_while_the_server_stalls` |
| **D-D** | The Query Hydrus window's result rows wrap, and its stated minimum is 420 x 480 (was 420 x 400); the Pawchive Index window states a minimum, 720 x 680 (Python states none). | Gate 2.5: every state fits its stated minimum. | `smoke::pawchive_and_query_windows_keep_their_stated_minimum` |

Each pinning assertion's `file:line` and the line reverted to turn it red are in section 7 (written at the head the
verifier is given).

## 3. Minimum size

The two new windows are their own surfaces and state their own minimum:

| Window | Stated minimum | Content need on the host (GTK 4.24) | Held by |
|---|---|---|---|
| Query Hydrus (`hydrus_query::MIN_WIDTH/HEIGHT`) | 420 x 480 | 333 x 441 | `smoke::pawchive_and_query_windows_keep_their_stated_minimum` |
| Pawchive Index (`pawchive_dialog::MIN_WIDTH/HEIGHT`) | 720 x 680 | 671 x 627 | the same scenario |

The fit test measures the window's **child** at the stated width, so the window's own size request cannot hide an
overflow. It was run red by lowering each stated height (400 for Query: `the Query Hydrus window is 333x441; its stated
minimum is 420x400`; 560 for Pawchive: `671x627 ... 720x560`). The numbers that count are the PNG headers of the
`hydrus-query-results-min-*` and `pawchive-min-*` shots from the hosted shot set (GTK 4.14), in section 5.

## 4. The differential

The same scripted action sequence runs through Python's `MainWindow` at `76c85c4` (temp `HOME`,
`QT_QPA_PLATFORM=offscreen`) and the Rust window (Xvfb, driven through its `gio::Action`s and model calls from the
test-only driver `tests/differential.rs` + `tests/differential/hydrus_diff.rs`). Each side talks to its own scripted
Hydrus built from the same table. After every step the harness compares the rows (each column's text, order, selection,
status tier), the status text, every dialog (text, buttons, default button, result, detail line), the `session.db` rows,
`config.json` and the server's request log (method, path, decoded query, key header, body). The harness lives outside the
repo (`worktrees/DAN-2220-tools`: `gen_h.py` the generator, `py_driver.py`, `cmp.py`); the log is
`worktrees/DAN-2220-differential.log`.

| Seed | Cases | Steps | Differing steps |
|---|---|---|---|
| 1 | 40 | 563 | 0 |
| 2 | 40 | 578 | 0 |
| 3 | 40 | 531 | 0 |
| 4 | 40 | 514 | 0 |

Documented classes inside those zero-difference runs (counts per seed are in the log): **D-A** (Delete box; 11 dialogs in
seed 1, 7 in seed 3, 12 in seed 4), **D-B** (failure headline; 8, 2 and 6) and one comparer rule: the
`get_url_files` requests of one confirmation poll round are compared as a multiset, because Python's own
`HydrusImportPollWorker` checks the pending entries concurrently in a thread pool (3 steps in seed 2). Negative controls:
altering one URL in the Rust request log gives 13 differences, dropping one request gives 35; perturbing the failure
prefix or leaking the server's words into the tail each gives 1.

**Not covered by the differential (smoke only):** the Query Hydrus window, the Pawchive window, Download and Send (it
reaches the network path), the https case and the stalled-server case.

## 5. Shots and contrast

(Filled from the hosted shot set at the head the verifier is given.)

## 6. Method map

### `gui/main_window.py`

| Python | Ported as |
|---|---|
| `_lookup_existing_hydrus_tags` (:2122) | `hydrus_ops::lookup_existing_hydrus_tags` |
| `_on_hydrus_tags_looked_up` (:2134) | `hydrus_ops::on_hydrus_tags_looked_up` |
| `action_reconcile_with_hydrus` (:1533) | `hydrus_ops::action_reconcile_with_hydrus` |
| `_start_hydrus_reconcile` (:1541) | `hydrus_ops::start_hydrus_reconcile` |
| `_on_hydrus_reconciled` (:1560) | `hydrus_ops::on_hydrus_reconciled` |
| `action_send_to_hydrus` (:3731) | `hydrus_ops::action_send_to_hydrus` (+ `on_files_sent`) |
| `action_import_url_to_hydrus` (:3789) | `hydrus_ops::action_import_url_to_hydrus` (+ `on_urls_sent`, `start_import_poll`) |
| `_on_hydrus_import_resolved` (:3903) | `hydrus_ops::on_hydrus_import_resolved` |
| `_on_hydrus_import_poll_finished` (:3921) | `hydrus_ops::on_hydrus_import_poll_finished` |
| `action_download_and_send_to_hydrus` (:3961) | `hydrus_ops::action_download_and_send_to_hydrus` (+ `on_downloaded_and_sent`) |
| `_show_rows_in_hydrus` (:3367) | `hydrus_ops::show_rows_in_hydrus` (+ `on_shown_in_hydrus`) |
| `_explain_show_in_hydrus_failure` (:3431) | `hydrus_ops::show_failure_text` |
| `_delete_rows_from_hydrus` (:3463) | `hydrus_ops::delete_rows_from_hydrus` (+ `on_delete_states`, `delete_in_hydrus`, `on_deleted_from_hydrus`) |
| `action_query_hydrus` (:2148) | `hydrus_query::action_query_hydrus` (+ `on_query_hydrus_done`) |
| `action_pawchive_index` (:2204) | `pawchive_dialog::action_pawchive_index` (+ `close_pawchive_dialog`) |
| `_build_hydrus_menu` (:726) | `menu.rs` (6.c; the entries are live here) |

### `gui/hydrus_query_dialog.py`

| Python | Ported as |
|---|---|
| `HydrusQueryDialog` (:21), `__init__` (:26) | `hydrus_query::HydrusQueryDialog::build` |
| `_do_search` (:60) | `HydrusQueryDialog::search` (+ `on_searched`, `result_label`, `query_tags`) |
| `_do_import` (:89) | `HydrusQueryDialog::import_selected` (+ `on_imported`) |
| `_parse_tag` (:131) | `hydrus_query::parse_hydrus_tag` |

### `gui/pawchive_index_dialog.py`

| Python | Ported as |
|---|---|
| `_hairline` (:29) | the `.field` / `.rule` styling in `theme.rs` (6.c); no function |
| `PawchiveIndexDialog` (:38), `__init__` (:41) | `pawchive_dialog::PawchiveIndexDialog::build` |
| `_populate` (:144) | `populate` (+ `row_cells`, `window_title`) |
| `_selected_creators` (:165), `_select_creators` (:169) | `selected_creators`, `select_creators` |
| `_find_or_add` (:184), `_on_found` (:206) | `find_or_add`, `on_found` (+ `found_label`) |
| `_add_checked` (:228), `_add_from_matches` (:243) | `add_checked`, `add_from_matches` |
| `_remove_selected` (:260) | `remove_selected` (Cancel by default) |
| `_start_indexing` (:275), `_set_busy` (:292) | `start_indexing`, `set_busy` |
| `_on_progress` (:301), `_on_creator_done` (:310), `_on_finished` (:323) | `on_progress`, `on_creator_done`, `on_finished` (+ `summary_line`) |
| `_stop` (:327), `closeEvent` (:333) | `stop`, `on_close` |

### `workers/` (the shells; the work is the merged core path)

| Python | Ported as |
|---|---|
| `HydrusTagLookupWorker` (`hydrus_lookup_worker.py:15`) | the worker bridge job in `lookup_existing_hydrus_tags` over `HydrusClient` |
| `HydrusReconcileWorker` (`hydrus_reconcile_worker.py:16`) | the job in `start_hydrus_reconcile` over `hydrus_import::reconcile` |
| `HydrusImportPollWorker` (`hydrus_import_poll_worker.py:21`) | `start_import_poll` over the merged `hydrus_import_url` poll |
| `PawchiveIndexWorker` (`pawchive_index_worker.py:19`), `PawchiveFindWorker` (:54) | the jobs in `start_indexing` and `find_or_add` over `PawchiveCrawler` |
| `ThumbnailWorker._known_to_hydrus` (`thumbnail_worker.py:48`), `_thumbnail_source` (:45) | `thumbnails::known_to_hydrus` and the Hydrus branch of the decode job |

## 7. Python test map

(Written at the head the verifier is given: an `ast` walk of `tests/test_*.py` at `76c85c4`
(`worktrees/DAN-2220-tools/scan6g.py`), with each Rust assertion cited at `file:line` and the pin tests reverted red.)

## 8. Status claims

This PR adds no status claim. The README's work-in-progress banner is untouched. The claim grep at the head is in the PR
body.
