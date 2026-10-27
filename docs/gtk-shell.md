# The GTK shell (Phase 6.c)

Work in progress; not yet a replacement for Hatate. This document belongs to
[DAN-2215](/DAN/issues/DAN-2215). It states what the window shell ports from Python at `76c85c4`, what it
leaves to the later Phase 6 tickets, how the worker bridge works, the minimum window size and how it was
measured, the differences from Python that are on purpose, the action differential's method and counts, and
the two maps Phase 6 asks every ticket for (the method map and the Python test map). 6.j checks the union
of the maps across tickets.

## 1. The cut

**Ported here:** the application and window frame; the full menubar (every menu Python has, as a data
table in `menu.rs`); the theme and fonts; the shared widgets this ticket's own screens use; the dialog seam;
the worker bridge; add files / add folder / drop, with the hash pass and its progress; restore at start-up,
Open / Save As / Clear Session; the autosave timer and the close teardown; the Crashed banner, "Nothing
survived.", the persisted SauceNAO quota pause; the empty state; the log viewer; the run strip's idle count
and the status bar's sent count; the minimum size; and the CI display smoke test.

**Left to later tickets** (each menu item whose action a later ticket owns is registered as a *disabled*
`gio::Action` and marked `DEFERRED(6.x)` in `menu.rs`, so 6.j can find any that are left):

| Ticket | Owns |
|---|---|
| 6.b | merged (PR #58) before this PR's verify ticket opens. The window uses its `missing_files` (`check_session_files`, `missing_status_text`, `missing_dialog_text`, `Entry.file_missing`) and `applog::LogRing`; nothing of 6.b is re-implemented here |
| 6.d | the Queue table, filters, sort, columns, thumbnails, context menu, Export, Write Tag Files, Clear Search Cache |
| 6.e | starting and stopping a search, the Sites menu, the live run strip (pace estimate, rate-limit wait), live banners, availability and upscale checks, Parser Health, engine alerts |
| 6.f | Review and compare, the tag rail, Edit Match Conditions, mark reviewed |
| 6.g | the Hydrus menu's actions, Query Hydrus, Send, URL import, Pawchive |
| 6.h | Preferences, settings search, the MCP tab |
| 6.i | the MCP bridge (`closeEvent` stops the server first: `DEFERRED(6.i)`) |

## 2. The worker bridge

Python runs each long job on a `QThread`, reports with Qt signals, and keeps a registry
(`gui/worker_lifecycle.py`) so a replaced worker is never destroyed while its thread still runs. The Rust
window owns **one** tokio runtime (`worker::Workers`). A job runs on it, and reports over a channel that a
future on the GTK main context drains, so every callback runs on the main thread, in order, and nothing a
job does can touch a widget.

* `Workers::run` is blocking work (hashing files, reading a session) on the runtime's blocking pool.
  `Workers::run_async` is a future (a network call) on its worker threads.
* A job gets a `Ctx`: `report(progress)` and `stopped()`. A job that never checks the flag still stops being
  *heard*: `JobHandle::retire` drops its callbacks, so a stale result never reaches a list that has moved on
  (this is what `retire_attrs(..., clear=True)` buys Python).
* `Slot` is one named worker attribute (`self.file_hash_worker`): `replace` retires the old job.
* `Workers::shutdown(grace)` is `closeEvent`'s teardown: every job is told to stop and silenced, and the
  runtime gets a grace period (2 s, as `grace_ms=2000`) to wind down. A job that outlives it is abandoned.
* Nothing is parked: a tokio task is not a `QThread`, so the registry's parking and pruning have no Rust
  counterpart (the Python tests for them are listed as not applicable in section 7).

Every later ticket uses this bridge and adds a stalled-server test that the window keeps handling actions
while a job waits (`worker::tests::an_async_job_runs_on_the_runtime_and_a_stalled_one_does_not_block_the_loop`
is the pattern; `a_second_add_waits_and_the_window_keeps_working` is the window-level one).

## 3. Minimum size

`MIN_WIDTH x MIN_HEIGHT = 1010 x 790`, set with `set_size_request`. Python's 712x515 is not carried over.

Measured on the CI toolkit (Ubuntu 24.04: GTK 4.14, libadwaita 1.5) by the `minimum_size_fits_every_state_in_both_schemes`
scenario in `tests/smoke.rs`, which renders every state of the shell in dark and light with the right rail
in, and fails if the window has to grow past the stated size or the content gets less than it needs. The
window reads 1000x780 on the outer rect; the other 10 px are the client-side-decoration margin, which is why
the stated size is 10 px larger in each direction. What each state needs (hosted `ci` run 40075892018 at
`b7c2bf8`):

| State | Dark | Light |
|---|---|---|
| Queue, empty | 777 x 618 | 777 x 618 |
| Queue, rows | 777 x 618 | 777 x 618 |
| Queue, Crashed banner | 777 x 737 | 777 x 737 |
| Queue, "Nothing survived." | 885 x 630 | 885 x 630 |
| Queue, quota banner | 830 x 737 | 830 x 737 |
| Review, 340 px rail in | **1000** x 618 | **1000** x 618 |

The width is set by Review with its rail (1000); the height by the two banner states (737), plus a margin:
the first cut stated 750, and the CI log showed the banner states given 777 px of content against a window
that reads 740, which is too close for a floor, so the stated height is 790.

## 4. Differences from Python, on purpose

| # | Difference | Why |
|---|---|---|
| D2 | A Yes/No question's default (and Escape) is **No**; Python leaves Qt's default, which is Yes. | Design gate 2.6 (a product call on DAN-2025): destructive confirmations default to Cancel. Counted in the differential. |
| D3 | Control borders take the border tiers `ink-46` (rest) and `ink-58` (hover), as Python does (DAN-224); cards, the run strip, the frame hairline and rules stay `ink-18`. The tiers are in the vendored `tokens.css` since DAN-2213, so this is no longer a difference from the sheet; it is kept here because the first cut of 6.c drew every border in `ink-18`. | DAN-2242 fix 4, the design read's call on [DAN-2229](/DAN/issues/DAN-2229): `ink-45` is not used (it is 2.68:1 on the light page). The `ink-18` set is the deliberate sub-3:1 set, listed in section 8. |
| D4 | Menu items carry no tooltips. | `GMenu` items have none; the text is the label. |
| D5 | Session dialogs show `user_message()` where Python would show raw exception text; they still show the path the user chose, which is their own input. | The data contract (DAN-2116 item 1): no paths or OS text in a message; `detail()` goes to the log. |
| D1 | **Python's incremental autosave can leave a stale `session.db` row**; Rust's autosave writes the whole list, so it saves what the window shows. | Ruled by Cloud on [DAN-2227](/DAN/issues/DAN-2227): accepted as a documented class, not ported (a Python data bug; matching it would add a `revision` field to `Entry` in `hatate-core`). One scenario of 300. See section 5. |

**The vendored `assets/tokens.css` is pinned to `Dromares/hatate-ryoku-mockup@0ef6e0c`** (`spec(DAN-2213)`, on
top of `fff6302`), byte for byte: `git show 0ef6e0c:tokens.css | cmp - crates/hatate-gtk/assets/tokens.css` prints
nothing. The file carries no header of its own, so that `cmp` stays exact; this paragraph and the `tokens.rs`
module comment are the cite. Until the PR #60 reads, the cite was `fff6302` plus uncommitted edits of the
mockup's working tree.

Matched on purpose even though it looks like a bug: with an **empty** list, Python's Clear Session discards
the saved session *before* it asks, so answering No still leaves the saved session discarded
(`action_clear_session`, `main_window.py:1921`). Likewise the final save at close writes an empty list
(`save_session(self.entries)`), which the first differential run caught: an emptied list leaves an empty
session behind, not the old rows.

## 5. The action differential

Method: the same scripted action sequence runs through Python's real `MainWindow` at `76c85c4` (temp `HOME`
and XDG dirs, `QT_QPA_PLATFORM=offscreen`, dialogs and choosers answered from a script, `time.time`,
`time.strftime` and `datetime.now` frozen) and through the Rust window (Xvfb, temp profile, scripted
`Prompter`, driven through its `gio::Action`s and model calls from `tests/differential.rs`, which is
test-only and reads its inputs from the environment; the shipped binary has none of that). After every step
both sides write one JSON digest and the comparer diffs them with no tolerance.

A digest holds: the list (path, hash, status, sent, confirmed, reviewed, interrupted), the page on show, the
ghost glyph, the mode, the status-bar text, the run strip (count, ETA, note, sent count, wait countdown,
SauceNAO label, gauge), the run banner (visibility, text, meta line), the quota banner (visibility, text,
button, meta line), the "Nothing survived." meta line, every dialog (kind, title, text, buttons, default,
result), every chooser (kind, title, filter, default name), every path opened with the desktop, and what is
on disk: `session.db` rows (raw `data` JSON), `config.json`, and the named session files.

Generator: `gen.py --seed SEED --count N`, thirteen seed profiles built with Python's own session writer
(empty, clean, clean with missing files, light and system themes, Crashed with and without a fault log or
clean-exit marker, Crashed with nothing saved, Crashed with an empty database, an active, a stale and an
unreadable quota pause, Crashed with a pause), then 3 to 9 steps per scenario drawn from add files, add
folder, drop (files and folders), save session as, open session (a saved one, a foreign one, a broken one,
an empty list, a missing path), clear session, set theme (including an invalid one), autosave, delete files
and open log; each step carries 0 to 2 scripted Yes/No answers. Harness, generator and comparer live outside
the repo (`worktrees/DAN-2215-tools`); the log is `worktrees/DAN-2215-differential.log`.

**Result** (seed `20261024`, built at `38ac99c`, re-run on the Rust half at the merged head `8e5764a` against the same Python digests): **300 scenarios, 2477 steps, 10 field differences in 5 steps, every one of them class D1 below**; no other difference. The 303 Yes/No boxes differ only in class D2. Log: `worktrees/DAN-2215-differential.log` (both runs).

| Class | Count | Disposition |
|---|---|---|
| D2: a Yes/No box defaults to No (design gate 2.6) | 303 of 303 Yes/No boxes (default field only) | documented, on purpose |
| D1: Python's incremental autosave leaves a stale `session.db` row after Open Session when the path is already in the saved list | 1 scenario (`s0181`), 5 steps, 10 field differences (`status`, `hydrus_hash` of one row) | **accepted** by Cloud on [DAN-2227](/DAN/issues/DAN-2227). Mechanism: Python's incremental autosave skips a row whose path is unchanged and whose revision is equal (0 == 0 after a load), so `session.db` keeps stale content. A future incremental save in `hatate-core` must bump `revision` on every mutation so that it never carries this quirk. This ticket keeps the full write per tick. **A difference outside D1 and D2 at the verified head is a FAIL, not a new class.** |

Normalised in the comparer, and why: `config.json`'s `log_file_path` (carries the side's temp root) and
`table_header_columns` / `table_header_state` (the Queue table's layout, written at close by a widget this
ticket does not have: 6.d). For an `Ok`-only box Python records `NoButton` and Qt then uses the one button,
which is the Rust default `ok`; that is not a difference.

**What the differential found and fixed** (each was a real gap, caught by the first runs): the run strip's
`N not searched` and the status bar's `N sent` were never refreshed from the list; the quota banner did not
put `ETA paused` on the strip; the ghost glyph followed the list instead of the drop-zone page (Python keeps
力 on the "Nothing survived." page); and the final save at close skipped an empty list.

## 6. Method map

Every `def` in the `gui/` files this ticket reads, at `76c85c4`. `PORT` rows name the Rust symbol. Other
rows name the ticket that owns the def. Files wholly owned by one later ticket are one line at the end.

### `gui/main_window.py`

| Line | Python | Disposition |
|---|---|---|
| 148 | `_format_duration` | **port**: `text::format_duration` |
| 176 | `_mark_interrupted_mid_search` | **port**: `hatate_core::model::mark_interrupted_mid_search (merged core; called from restore)` |
| 194 | `_plural` | **port**: `text::plural` |
| 198 | `_interrupted_prose` | **port**: `text::interrupted_prose` |
| 225 | `_is_waiting_review` | **port**: `text::is_waiting_review` |
| 231 | `_waiting_review_count` | **port**: `text::waiting_review_count` |
| 235 | `_quota_pause_prose` | **port**: `text::quota_pause_prose` |
| 257 | `_format_finish_time` | 6.e: the live ETA's finish time |
| 282 | `_host_from_url` | 6.d: the Site column |
| 317 | `_load_preview_pixmap` | 6.d: thumbnails / preview decode |
| 342 | `_muted_text_color` | 6.d: table delegates |
| 353 | `_nothing_to_write_text` | 6.d: Write Tag Files |
| 370 | `_written_summary` | 6.d: Write Tag Files |
| 388 | `_engine_alert_forwarder` | 6.e: engine alerts |
| 421 | `__init__` | **port**: `window::MainWindow::new (the shell, queue page, banners, menu, restore, autosave); the table, filter bar, Review and Activity pages are 6.d/6.e/6.f` |
| 544 | `_show_persisted_quota_pause` | **port**: `banners::show_persisted_quota_pause` |
| 571 | `_build_menu` | **port**: `menu::menus + menu::model + window (the whole menubar as a data table; later tickets' items are registered disabled and marked DEFERRED(6.x))` |
| 726 | `_build_hydrus_menu` | **port**: `menu::menus (the Hydrus menu; its actions are 6.g's, registered disabled)` |
| 756 | `_build_sites_menu` | 6.e: Sites menu |
| 781 | `_on_site_toggled` | 6.e: Sites menu |
| 791 | `_set_all_sites` | 6.e: Sites menu |
| 803 | `_build_central_widget` | **port**: `window::MainWindow::new (shell stack and queue page); the table is 6.d` |
| 810 | `_build_queue_page` | **port**: `window::MainWindow::new (queue stack: rows page, drop zone, Nothing survived); the table and filter bar are 6.d` |
| 967 | `_build_drop_zone` | **port**: `widgets::empty_prose + window (the empty.html layout)` |
| 1017 | `_refresh_queue_empty_state` | **port**: `window::MainWindow::refresh_queue_empty_state` |
| 1035 | `ghost_glyph_for` | **port**: `window::MainWindow::ghost_glyph_for` |
| 1043 | `_apply_filter` | 6.d: filter |
| 1060 | `_refresh_filter_ui` | 6.d: filter |
| 1067 | `_selected_entries` | 6.d: selection |
| 1072 | `_reselect` | 6.d: selection |
| 1099 | `_update_preview` | 6.f: preview |
| 1144 | `_on_mode_changed` | **port**: `window::MainWindow::on_mode_changed (the hook; Review's body is 6.f)` |
| 1158 | `_populate_candidate_combo` | 6.f: candidate picker |
| 1186 | `_sync_candidate_combo_tooltip` | 6.f: candidate picker |
| 1194 | `_on_candidate_combo_changed` | 6.f: candidate picker |
| 1207 | `_ensure_candidate_loaded` | 6.f: candidate fetch |
| 1238 | `_run_dialog` | 6.f: match-conditions dialog helper |
| 1265 | `_on_candidate_fetched` | 6.f: candidate fetch |
| 1293 | `_refresh_tag_list` | 6.f: tag rail |
| 1314 | `_on_edit_tags_toggled` | 6.f: tag rail |
| 1319 | `_on_tag_item_edited` | 6.f: tag rail |
| 1347 | `_show_matched_preview_menu` | 6.f: preview menu |
| 1371 | `dragEnterEvent` | **port**: `window (the drop target accepts files)` |
| 1375 | `dropEvent` | **port**: `session_ops::drop_paths` |
| 1388 | `action_add_files` | **port**: `session_ops::action_add_files` |
| 1395 | `action_add_folder` | **port**: `session_ops::action_add_folder` |
| 1400 | `_restore_saved_session` | **port**: `session_ops::restore_saved_session` |
| 1462 | `_show_nothing_survived` | **port**: `banners::show_nothing_survived` |
| 1487 | `_dismiss_nothing_survived` | **port**: `banners::dismiss_nothing_survived` |
| 1500 | `_show_run_banner` | **port**: `banners::show_run_banner` |
| 1533 | `action_reconcile_with_hydrus` | 6.g: Hydrus reconcile |
| 1541 | `_start_hydrus_reconcile` | 6.g: Hydrus reconcile |
| 1560 | `_on_hydrus_reconciled` | 6.g: Hydrus reconcile |
| 1569 | `_start_missing_file_check` | **port**: `session_ops::start_missing_file_check` (runs 6.b's `missing_files::check_session_files` on the worker bridge) |
| 1589 | `_on_missing_files_checked` | **port**: `session_ops::on_missing_files_checked` |
| 1627 | `action_save_session_as` | **port**: `session_ops::action_save_session_as` |
| 1657 | `action_export_results` | 6.d: Export |
| 1712 | `action_write_tag_files` | 6.d: Write Tag Files |
| 1757 | `_ask_about_writing_tag_files` | 6.d: Write Tag Files |
| 1842 | `action_open_session` | **port**: `session_ops::action_open_session` |
| 1908 | `_stop_background_workers` | **port**: `session_ops::stop_background_workers` |
| 1921 | `action_clear_session` | **port**: `session_ops::action_clear_session` |
| 1944 | `_import_files` | **port**: `session_ops::import_files` |
| 1975 | `_on_file_hash_progress` | **port**: `session_ops::on_file_hash_progress` |
| 1984 | `_on_file_hash_finished` | **port**: `session_ops::on_file_hash_finished` |
| 2035 | `_start_thumbnail_generation` | 6.d: thumbnails |
| 2068 | `_on_thumbnail_ready` | 6.d: thumbnails |
| 2100 | `_on_thumbnail_progress` | 6.d: thumbnails |
| 2106 | `_on_thumbnails_finished` | 6.d: thumbnails |
| 2122 | `_lookup_existing_hydrus_tags` | 6.g: Hydrus tag lookup |
| 2134 | `_on_hydrus_tags_looked_up` | 6.g: Hydrus tag lookup |
| 2148 | `action_query_hydrus` | 6.g: Query Hydrus |
| 2170 | `_add_tags_to_current` | 6.f: tag rail |
| 2194 | `action_open_log` | **port**: `session_ops::action_open_log` |
| 2201 | `action_parser_health` | 6.e: Parser Health |
| 2204 | `action_pawchive_index` | 6.g: Pawchive index |
| 2217 | `_warn_about_broken_parsers` | 6.e: parser health warning |
| 2233 | `action_view_logs` | **port**: `session_ops::action_view_logs` |
| 2236 | `_open_crash_log` | **port**: `session_ops::open_crash_log` |
| 2242 | `_update_clear_cache_action_label` | 6.d: Clear Search Cache |
| 2247 | `action_clear_search_cache` | 6.d: Clear Search Cache |
| 2264 | `action_toggle_search` | 6.e: start/stop |
| 2270 | `action_start_search` | 6.e: start/stop |
| 2282 | `action_stop_search` | 6.e: start/stop |
| 2289 | `_on_worker_progress` | 6.e: live run strip |
| 2295 | `_refresh_run_progress_label` | **port**: `session_ops::refresh_run_progress_label (the idle and counted branches; the live pace estimate is 6.e)` |
| 2327 | `_idle_run_text` | **port**: `session_ops::idle_run_text` |
| 2336 | `_on_worker_waiting` | 6.e: rate-limit status |
| 2339 | `_on_wait_countdown` | 6.e: rate-limit countdown |
| 2347 | `_on_auto_imported` | 6.g: auto-import |
| 2373 | `_on_worker_image_updated` | 6.e: live row update |
| 2400 | `_refresh_saucenao_quota_label` | 6.e: SauceNAO quota readout |
| 2411 | `_render_saucenao_quota_label` | 6.e: SauceNAO quota readout |
| 2417 | `_on_paused_out_of_quota` | 6.e: live quota pause |
| 2441 | `_show_quota_banner` | **port**: `banners::show_quota_banner` |
| 2472 | `_hide_quota_banner` | **port**: `banners::hide_quota_banner` |
| 2481 | `_review_the_waiting` | 6.f: Review the N waiting (the button is built here) |
| 2491 | `_resume_without_saucenao` | 6.e: Continue with other engines |
| 2507 | `_on_continuing_without_saucenao` | 6.e: Continue with other engines |
| 2521 | `_on_engine_alert` | 6.e: engine alerts |
| 2548 | `_on_worker_finished` | 6.e: run end |
| 2559 | `_register_new_entries` | 6.d: table model registration |
| 2566 | `_on_header_clicked` | 6.d: sort |
| 2580 | `_apply_sort` | 6.d: sort |
| 2606 | `_restore_table_header_state` | 6.d: column layout |
| 2633 | `_repair_header_after_restore` | 6.d: column layout |
| 2656 | `_save_table_header_state` | 6.d: column layout (closeEvent calls it: DEFERRED(6.d)) |
| 2664 | `_refresh_sent_count_label` | **port**: `session_ops::refresh_sent_count_label` |
| 2688 | `_refresh_entry_row` | 6.d: row repaint (calls _refresh_sent_count_label) |
| 2701 | `_refresh_table` | **port**: `session_ops::list_changed (empty state, ghost glyph, sent count); the view's refresh is 6.d` |
| 2749 | `_get_local_pixmap` | 6.d: thumbnails |
| 2766 | `_get_row_thumbnail` | 6.d: thumbnails |
| 2795 | `_schedule_viewport_thumbnails` | 6.d: thumbnails |
| 2804 | `_visible_row_range` | 6.d: thumbnails |
| 2830 | `_update_visible_thumbnails` | 6.d: thumbnails |
| 2921 | `_queue_thumbnail_regeneration` | 6.d: thumbnails |
| 2930 | `_run_thumbnail_regeneration` | 6.d: thumbnails |
| 2943 | `_show_table_context_menu` | 6.d: context menu |
| 2946 | `_select_rows_by_status` | 6.d: selection |
| 2952 | `_select_rows_by_predicate` | 6.d: selection |
| 2971 | `_live_entries` | 6.d: selection helpers |
| 2987 | `_reset_rows` | 6.d: row actions |
| 3037 | `_set_rows_reviewed` | 6.f: mark reviewed |
| 3061 | `_research_rows` | 6.e: re-search |
| 3076 | `_research_rows_with_engine` | 6.e: re-search |
| 3100 | `_research_rows_with_opposite_engine` | 6.e: re-search |
| 3146 | `_check_match_availability` | 6.e: availability check |
| 3168 | `_on_availability_progress` | 6.e: availability check |
| 3172 | `_on_availability_finished` | 6.e: availability check |
| 3209 | `_remove_dead_candidates` | 6.e: availability check |
| 3224 | `_open_compare_dialog` | 6.f: compare dialog |
| 3240 | `_check_rows_for_upscaling` | 6.e: upscale check |
| 3260 | `_on_upscale_entry_checked` | 6.e: upscale check |
| 3281 | `_on_upscale_check_finished` | 6.e: upscale check |
| 3304 | `_launch_search_worker` | 6.e: search launch |
| 3367 | `_show_rows_in_hydrus` | 6.g: Show in Hydrus |
| 3431 | `_explain_show_in_hydrus_failure` | 6.g: Show in Hydrus |
| 3463 | `_delete_rows_from_hydrus` | 6.g: Delete from Hydrus |
| 3566 | `_remove_rows` | 6.d: remove rows |
| 3598 | `_remove_entries` | 6.d: remove rows |
| 3633 | `_successor_surviving` | 6.d: remove rows |
| 3651 | `_sync_preview_to_selection` | 6.f: preview |
| 3664 | `select_table_row` | 6.d: selection |
| 3689 | `action_open_matched_url` | 6.f: Open matched URL |
| 3693 | `action_show_in_queue` | 6.f: Show in Queue |
| 3704 | `_current_entry` | 6.f: current entry |
| 3710 | `_on_selection_changed` | 6.d: selection |
| 3719 | `_remove_selected_tags` | 6.f: tag rail |
| 3731 | `action_send_to_hydrus` | 6.g: Send to Hydrus |
| 3789 | `action_import_url_to_hydrus` | 6.g: URL import |
| 3903 | `_on_hydrus_import_resolved` | 6.g: URL import |
| 3921 | `_on_hydrus_import_poll_finished` | 6.g: URL import |
| 3961 | `action_download_and_send_to_hydrus` | 6.g: Download and send |
| 4047 | `action_open_settings` | 6.h: Preferences |
| 4079 | `action_set_theme` | **port**: `window::MainWindow::set_theme` |
| 4099 | `action_edit_conditions` | 6.f: Edit Match Conditions |
| 4105 | `eventFilter` | 6.d: default column widths |
| 4125 | `showEvent` | 6.d: initial thumbnail pass |
| 4134 | `resizeEvent` | 6.d: thumbnail pass on resize |
| 4139 | `closeEvent` | **port**: `session_ops::close_teardown (the DEFERRED(6.x) steps are named in its body)` |

### `gui/shell.py`

| Line | Python | Disposition |
|---|---|---|
| 65 | `split_figures` | **port**: `shell::split_figures` |
| 79 | `Readout` | **port**: `shell::Readout` |
| 92 | `Readout.__init__` | method of `Readout`; follows it |
| 102 | `Readout.text` | method of `Readout`; follows it |
| 105 | `Readout.setText` | method of `Readout`; follows it |
| 131 | `_ModeStack` | **port**: `shell::Shell.mode_stack (a gtk::Stack)` |
| 146 | `_ModeStack.minimumSizeHint` | method of `_ModeStack`; follows it |
| 152 | `_ModeStack.sizeHint` | method of `_ModeStack`; follows it |
| 158 | `_ModeStack.setCurrentWidget` | method of `_ModeStack`; follows it |
| 182 | `ShellMixin` | **port**: `shell::Shell` |
| 193 | `_build_shell` | **port**: `shell::Shell::new` |
| 216 | `_build_top_bar` | **port**: `shell::Shell::new` |
| 244 | `_build_app_mark` | **port**: `shell::Shell::new` |
| 268 | `_build_run_strip` | **port**: `shell::Shell::new` |
| 360 | `_placeholder_page` | **port**: `shell::placeholder_page` |
| 371 | `_build_status_bar` | **port**: `shell::Shell::new` |
| 397 | `set_mode` | **port**: `window::MainWindow::set_mode` |
| 416 | `ghost_glyph_for` | **port**: `shell::ghost_glyph` |
| 421 | `set_ghost` | **port**: `widgets::GhostKanji::set_glyph` |
| 427 | `current_mode` | **port**: `shell::Shell::current_mode` |
| 430 | `_on_mode_changed` | **port**: `window::MainWindow::on_mode_changed` |

### `gui/theme.py`

| Line | Python | Disposition |
|---|---|---|
| 236 | `chip_label` | 6.d (table chips) |
| 240 | `status_glyph` | 6.d (Status column) |
| 252 | `status_weight` | 6.d (Status column) |
| 259 | `status_glyph_font` | **port**: `fonts (Ryoku Status face) + 6.d` |
| 267 | `mono_font` | **port**: `theme::css (mono family)` |
| 282 | `ink_color` | **port**: `tokens (named colours)` |
| 298 | `palette` | **port**: `tokens::Palette` |
| 303 | `resolve_mode` | **port**: `theme::resolve_scheme` |
| 328 | `stylesheet` | **port**: `theme::css (generated from tokens.css; GTK CSS)` |

### `gui/fonts.py`

| Line | Python | Disposition |
|---|---|---|
| 31 | `register_fonts` | **port**: `fonts::load / fonts::load_default` |

### `gui/message.py`

| Line | Python | Disposition |
|---|---|---|
| 31 | `build` | **port**: `dialogs::AdwPrompter (no native-dialog workaround is needed)` |
| 41 | `_show` | **port**: `dialogs::Prompter::message` |
| 72 | `information` | **port**: `dialogs::information` |
| 76 | `warning` | **port**: `dialogs::warning` |
| 80 | `critical` | **port**: `dialogs::critical` |
| 84 | `question` | **port**: `dialogs::question` |

### `gui/worker_lifecycle.py`

| Line | Python | Disposition |
|---|---|---|
| 34 | `WorkerRegistry` | **port**: `worker::Workers` |
| 38 | `__init__` | **port**: `worker::Workers::new` |
| 41 | `__len__` | n/a: no parked-thread list (tokio tasks are not QThreads) |
| 44 | `__contains__` | n/a: same |
| 48 | `retiring` | n/a: same |
| 52 | `prune` | n/a: same |
| 64 | `retire` | **port**: `worker::Slot::retire` |
| 104 | `_prune_soon` | n/a: same |
| 107 | `retire_attrs` | **port**: `worker::Slot::retire (one slot per attribute) + session_ops::close_teardown` |
| 129 | `wait_for` | **port**: `session_ops::close_teardown (the autosave wait) + worker::Workers::shutdown` |

### `gui/session_autosave.py`

| Line | Python | Disposition |
|---|---|---|
| 33 | `SessionAutosaver` | **port**: `session_ops (autosave_timer, autosave_worker)` |
| 37 | `__init__` | **port**: `window::State` |
| 54 | `mark_saved` | n/a: no revision bookkeeping (D1, DAN-2227) |
| 60 | `apply_settings` | **port**: `session_ops::apply_autosave_settings` |
| 72 | `save_now` | **port**: `session_ops::autosave_now` |
| 95 | `shutdown` | **port**: `session_ops::close_teardown` |
| 106 | `_on_done` | **port**: `session_ops::autosave_now (completion callback)` |

### `gui/log_viewer_dialog.py`

| Line | Python | Disposition |
|---|---|---|
| 15 | `LogViewerDialog` | **port**: `logview::LogViewer` |
| 24 | `__init__` | **port**: `logview::LogViewer::new` |
| 76 | `refresh` | **port**: `logview::LogViewer::refresh` |
| 90 | `_toggle_crash_log` | **port**: `logview::LogViewer (toggle)` |
| 95 | `_copy_all` | **port**: `logview::LogViewer (copy all)` |
| 98 | `_open_log_folder` | **port**: `logview::LogViewer (open folder)` |

### `gui/widgets.py`

| Line | Python | Disposition |
|---|---|---|
| 70 | `card` | **port**: `widgets::card` |
| 98 | `section_label` | **port**: `widgets::section_label` |
| 116 | `section_header` | **port**: `widgets::section_header` |
| 132 | `FlowLayout` | 6.d/6.f (filter and tag chips wrap) |
| 139 | `__init__` | method of the class above; follows it |
| 145 | `addItem` | method of the class above; follows it |
| 149 | `count` | method of the class above; follows it |
| 152 | `itemAt` | method of the class above; follows it |
| 155 | `takeAt` | method of the class above; follows it |
| 161 | `hasHeightForWidth` | method of the class above; follows it |
| 164 | `heightForWidth` | method of the class above; follows it |
| 167 | `setGeometry` | method of the class above; follows it |
| 171 | `sizeHint` | method of the class above; follows it |
| 174 | `minimumSize` | method of the class above; follows it |
| 182 | `_arrange` | method of the class above; follows it |
| 201 | `WrappingLabel` | GTK labels wrap natively (`wrap(true)`); no port |
| 211 | `__init__` | method of the class above; follows it |
| 215 | `resizeEvent` | method of the class above; follows it |
| 221 | `setText` | method of the class above; follows it |
| 228 | `LeadedLabel` | GTK label line-height is CSS; no port |
| 238 | `__init__` | method of the class above; follows it |
| 242 | `_document` | method of the class above; follows it |
| 257 | `heightForWidth` | method of the class above; follows it |
| 262 | `paintEvent` | method of the class above; follows it |
| 276 | `_banner_shell` | **port**: `widgets::banner_shell` |
| 331 | `run_banner` | **port**: `widgets::run_banner` |
| 365 | `quota_pause_banner` | **port**: `widgets::quota_pause_banner` |
| 388 | `meta_line` | **port**: `widgets::meta_line` |
| 427 | `empty_prose` | **port**: `widgets::empty_prose` |
| 446 | `nothing_survived` | **port**: `widgets::nothing_survived` |
| 517 | `restyle` | **port**: `theme::install (the provider swap restyles everything)` |
| 534 | `heading` | **port**: `widgets::heading` |
| 540 | `screen_title` | **port**: `widgets::screen_title` |
| 549 | `display_title` | **port**: `widgets::display_title` |
| 558 | `muted` | **port**: `widgets::muted` |
| 564 | `hint` | **port**: `widgets::hint` |
| 571 | `uppercase_voice` | **port**: `theme::css (caps / mono classes)` |
| 585 | `pill_button` | **port**: `widgets::pill_button` |
| 605 | `KeyCap` | **port**: `widgets::keycap` |
| 615 | `__init__` | method of the class above; follows it |
| 631 | `KeyedButton` | **port**: `widgets::keyed_button` |
| 642 | `__init__` | method of the class above; follows it |
| 654 | `keycap` | method of the class above; follows it |
| 657 | `key` | method of the class above; follows it |
| 660 | `set_key` | method of the class above; follows it |
| 666 | `refit` | method of the class above; follows it |
| 680 | `_place` | method of the class above; follows it |
| 685 | `resizeEvent` | method of the class above; follows it |
| 689 | `showEvent` | method of the class above; follows it |
| 694 | `keyed_button` | **port**: `widgets::keyed_button` |
| 710 | `KeyHintBar` | **port**: `widgets::KeyHintBar` |
| 721 | `__init__` | method of the class above; follows it |
| 732 | `set_groups` | method of the class above; follows it |
| 768 | `_text_label` | method of the class above; follows it |
| 775 | `plain_text` | method of the class above; follows it |
| 780 | `visible_group_count` | method of the class above; follows it |
| 783 | `minimumSizeHint` | method of the class above; follows it |
| 786 | `resizeEvent` | method of the class above; follows it |
| 790 | `showEvent` | method of the class above; follows it |
| 794 | `_fit` | method of the class above; follows it |
| 818 | `link_button` | **port**: `widgets::link_button` |
| 838 | `icon_button` | **port**: `widgets::icon_button` |
| 866 | `zoom_button` | 6.f (Review zoom) |
| 879 | `apply_tracking` | theme::css (letter-spacing) |
| 892 | `segmented` | **port**: `widgets::segmented` |
| 957 | `disclosure` | **port**: `widgets::disclosure` |
| 969 | `_retext` | method of the class above; follows it |
| 980 | `fold` | method of the class above; follows it |
| 987 | `chip` | **port**: `widgets::chip` |
| 1000 | `roomy` | 6.d (chip padding in the table) |
| 1023 | `GhostKanji` | **port**: `widgets::GhostKanji` |
| 1049 | `__init__` | method of the class above; follows it |
| 1057 | `glyph` | method of the class above; follows it |
| 1060 | `set_glyph` | method of the class above; follows it |
| 1065 | `_font` | method of the class above; follows it |
| 1071 | `ghost_rect` | widgets::GhostKanji |
| 1081 | `_pixmap` | method of the class above; follows it |
| 1105 | `paintEvent` | method of the class above; follows it |
| 1113 | `see_through` | widgets::GhostKanji (the ghost is drawn behind the page) |
| 1138 | `Brackets` | **port**: `widgets::brackets` |
| 1155 | `__init__` | method of the class above; follows it |
| 1165 | `corner_rects` | widgets::brackets |
| 1177 | `_fit` | method of the class above; follows it |
| 1186 | `eventFilter` | method of the class above; follows it |
| 1192 | `paintEvent` | method of the class above; follows it |
| 1199 | `image_frame` | **port**: `widgets::image_frame` |
| 1212 | `_ElidedLabel` | **port**: `widgets::FrameCaption (ellipsized gtk::Label)` |
| 1217 | `__init__` | method of the class above; follows it |
| 1221 | `minimumSizeHint` | method of the class above; follows it |
| 1224 | `paintEvent` | method of the class above; follows it |
| 1232 | `FrameCaption` | **port**: `widgets::FrameCaption` |
| 1238 | `__init__` | method of the class above; follows it |
| 1254 | `apply_theme` | **port**: `theme::install` |
| 1263 | `LinkLabel` | 6.f |
| 1270 | `__init__` | method of the class above; follows it |
| 1274 | `mousePressEvent` | method of the class above; follows it |
| 1279 | `WideComboBox` | 6.f (candidate picker) |
| 1285 | `showPopup` | method of the class above; follows it |
| 1292 | `CandidatePicker` | 6.f |
| 1304 | `__init__` | method of the class above; follows it |
| 1309 | `elided_text` | method of the class above; follows it |
| 1318 | `_option` | method of the class above; follows it |
| 1323 | `paintEvent` | method of the class above; follows it |
| 1339 | `ScaledImageLabel` | 6.f (Review image) |
| 1353 | `__init__` | method of the class above; follows it |
| 1357 | `setPixmap` | method of the class above; follows it |
| 1361 | `source_pixmap` | method of the class above; follows it |
| 1364 | `resizeEvent` | method of the class above; follows it |
| 1371 | `sizeHint` | method of the class above; follows it |
| 1374 | `minimumSizeHint` | method of the class above; follows it |
| 1377 | `_render` | method of the class above; follows it |

### Files owned by one later ticket

| File | Defs | Owner |
|---|---|---|
| `gui/image_table_model.py`, `gui/table_delegates.py`, `gui/table_context_menu.py`, `gui/filter_bar.py` | 34 + 8 + 12 + 17 | 6.d |
| `gui/activity_view.py` | 10 | 6.e |
| `gui/parser_health_dialog.py` | 8 | 6.e |
| `gui/review_view.py`, `gui/review_shortcuts.py`, `gui/preview_text.py`, `gui/compare_dialog.py`, `gui/match_conditions_dialog.py`, `gui/add_tags_dialog.py` | 29 + 11 + 14 + 44 + 2 + 2 | 6.f |
| `gui/hydrus_query_dialog.py`, `gui/pawchive_index_dialog.py` | 4 + 17 | 6.g |
| `gui/settings_dialog.py`, `gui/settings_search.py` | 55 + 5 | 6.h |
| `gui/mcp_bridge.py` | 56 | 6.i |

## 7. Python test map

844 tests in the 20 files the ticket lists, at `76c85c4`. By owner: 6.b: 7, 6.c: 118, 6.d: 240, 6.d/6.e: 2, 6.d/6.f: 5, 6.e: 53, 6.f: 166, 6.g: 73, 6.h: 66, core: 75, DAN-2242: 5, n/a: 34.

`6.c` rows name the Rust test that covers them (`smoke::<scenario>` is a scenario of `tests/smoke.rs`, run under
Xvfb; `lib::<path>` is a unit test). `core` rows are ported and differentialed in earlier phases; the window's
use of them is covered by the session scenarios. `6.b` rows are 6.b's. A later ticket's rows are that ticket's to
map; they are listed so 6.j can check the union. `n/a` rows give the reason. A Python assertion about pixel
geometry (a 20 px leading, a 37% weighting) is not asserted in Rust: the design gate judges those from the shot
set (section 8), and the row says so.
A row that says `not asserted` names a Python test that no Rust test in the suite checks. Where the action
differential (section 5) compares the same field with Python's after every step, the row says so; that is
evidence from a run, not a test that stays in the tree, and CI does not run it.

### `tests/test_empty_state.py` (13)

| Line | Test | Disposition |
|---|---|---|
| 56 | `TestHeading.test_the_heading_is_serif_36px_with_the_full_stop` | **6.c**: `smoke::empty_state_and_idle_strip` asserts the copy: the `Queue.` heading, the `01 // Queue` section label, `Drop images here.` and the three action labels; the serif face, the 36 px size and the weighting are judged in the shot set, not in a test |
| 68 | `TestHeading.test_it_is_not_a_second_screen_title` | **6.c**: `smoke::empty_state_and_idle_strip` asserts the copy: the `Queue.` heading, the `01 // Queue` section label, `Drop images here.` and the three action labels; that it is a different style from the screen title is judged in the shot set, not in a test |
| 78 | `TestFrame.test_the_label_is_a_numbered_section_label` | **6.c**: `smoke::empty_state_and_idle_strip` asserts the copy: the `Queue.` heading, the `01 // Queue` section label, `Drop images here.` and the three action labels (the numbered label is the `01 // Queue` text) |
| 87 | `TestFrame.test_the_frame_fills_the_page_height` | **6.c**: judged in the shot set, not in a test (frame geometry) |
| 105 | `TestFrame.test_the_frame_is_a_hairline_with_corner_brackets` | **6.c**: judged in the shot set, not in a test (hairline and corner brackets) |
| 136 | `TestFrame.test_the_brackets_do_not_swallow_the_actions` | **6.c**: judged in the shot set, not in a test (the brackets are drawn over the frame, not as an input layer) |
| 148 | `TestTopWeighted.test_the_headline_is_about_37_percent_down` | **6.c**: judged in the shot set, not in a test (37% weighting) |
| 159 | `TestTopWeighted.test_the_block_is_above_the_centre_of_its_frame` | **6.c**: judged in the shot set, not in a test (block position) |
| 170 | `TestActions.test_the_actions_are_uppercase_mono` | **6.c**: `smoke::empty_state_and_idle_strip` asserts the copy: the `Queue.` heading, the `01 // Queue` section label, `Drop images here.` and the three action labels; the uppercase mono paint is the `caps` style, judged in the shot set |
| 180 | `TestActions.test_the_actions_are_12px_apart` | **6.c**: judged in the shot set, not in a test (12 px gaps) |
| 192 | `TestEmptyGhost.test_the_empty_queue_wears_the_empty_glyph` | **6.c**: `lib::shell::tests::modes_and_ghosts_are_pythons` asserts `EMPTY_GHOST_GLYPH == "空"`; `smoke::empty_state_and_idle_strip` compares the empty page's glyph with that constant; the action differential compares `ghost` |
| 196 | `TestEmptyGhost.test_a_list_brings_the_queue_glyph_back_and_clearing_it_goes_again` | **6.c**: `smoke::add_files_hashes_and_dedupes` asserts 力 once a list exists; the return to 空 after the list is emptied is checked only by the action differential (`ghost`), not by a Rust test |
| 208 | `TestEmptyGhost.test_the_other_modes_keep_their_own_glyph_while_the_list_is_empty` | **6.c**: `lib::shell::tests::modes_and_ghosts_are_pythons` asserts the mapping `shell::ghost_glyph` returns (queue 力, review 鏡, activity 動; the empty page's 空 is `EMPTY_GHOST_GLYPH`); the window-level behaviour (`MainWindow::ghost_glyph_for`: 空 only while the Queue shows its drop zone, every other mode keeping its own glyph while the list is empty) is checked only by the action differential (`ghost`), not by a test |

### `tests/test_fonts.py` (4)

| Line | Test | Disposition |
|---|---|---|
| 27 | `TestFontRegistration.test_the_real_bundle_registers_every_file` | **6.c**: `lib::fonts::tests::nine_faces_are_bundled_and_none_is_woff2` |
| 33 | `TestFontRegistration.test_a_missing_directory_returns_minus_one_rather_than_raising` | **6.c**: `lib::fonts::tests::files_are_written_once_and_rewritten_when_damaged`, `lib::error::tests::a_font_error_names_no_path` (a font that fails to register leaves fallback faces and a typed error) |
| 40 | `TestFontRegistration.test_a_corrupt_font_file_registers_as_minus_one` | **6.c**: `lib::fonts::tests::files_are_written_once_and_rewritten_when_damaged`, `lib::error::tests::a_font_error_names_no_path` (a font that fails to register leaves fallback faces and a typed error) |
| 59 | `TestRyokuKanjiSubset.test_cmap_has_the_japanese_mirror_glyph_not_the_simplified_one` | **6.c**: `smoke::fonts_cover_the_status_glyphs_and_kanji` (U+93E1 is in the bundled subset) |

### `tests/test_gui_harness.py` (13)

| Line | Test | Disposition |
|---|---|---|
| 395 | `TestGuiConstructionAndLifecycle.test_every_row_constructs_and_survives` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 402 | `TestGuiConstructionAndLifecycle.test_table_covers_every_scoped_module` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 435 | `TestNamedGaps.test_mixins_have_no_init` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 447 | `TestNamedGaps.test_preview_text_has_no_widget_class` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 458 | `TestNamedGaps.test_function_only_modules_define_no_widget` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 477 | `TestNamedGaps.test_main_window_is_a_widget_composing_the_mixin_gaps` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 494 | `TestNamedGaps.test_helper_classes_are_plain_not_widgets` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 516 | `TestNoNetworkGuardIsLive.test_guard_raises_on_a_socket_connect` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 522 | `TestNoNetworkGuardIsLive.test_guard_raises_on_an_http_request` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 528 | `TestNoNetworkGuardIsLive.test_guard_is_clean_after_exit` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 561 | `TestThemedWindowKeepsTheApplicationAlive.test_application_survives_the_test_that_built_the_window` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 588 | `TestEnsureAppPinsTheApplication.test_application_survives_the_caller_dropping_its_reference` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |
| 597 | `TestEnsureAppPinsTheApplication.test_no_test_module_builds_its_own_application` | n/a: Qt test-harness hygiene (QApplication lifetime, socket guard, mixin shapes); the Rust window is built by tests/common::Fx on an app the test owns, and the network guard is 'no live network in tests' enforced by the injected hosts |

### `tests/test_gui_smoke.py` (456)

| Line | Test | Disposition |
|---|---|---|
| 124 | `TestSettingsDialogConstruction.test_dialog_and_every_tab_build` | 6.h: Preferences |
| 141 | `TestSettingsDialogConstruction.test_settings_round_trip_through_the_dialog` | 6.h: Preferences |
| 155 | `TestSettingsDialogConstruction.test_deviantart_cookie_field_explains_the_rotation_fix_and_its_limit` | 6.h: Preferences |
| 191 | `TestDeviantArtCookieReconciliation.test_an_untouched_box_does_not_clobber_a_same_session_rotation` | 6.h: Preferences |
| 208 | `TestDeviantArtCookieReconciliation.test_an_actual_edit_still_wins_over_a_same_session_rotation` | 6.h: Preferences |
| 228 | `TestDeviantArtCookieReconciliation.test_clearing_the_box_is_an_explicit_logout_not_a_no_op` | 6.h: Preferences |
| 259 | `TestMainWindowSignalHandlers.test_thumbnail_ready_handler` | 6.d |
| 265 | `TestMainWindowSignalHandlers.test_thumbnail_ready_for_removed_entry_is_safe` | 6.d |
| 272 | `TestMainWindowSignalHandlers.test_search_progress_handlers` | 6.e |
| 282 | `TestMainWindowSignalHandlers.test_progress_label_shows_the_count_then_the_estimate` | 6.e: live run strip (the idle branch is S run_strip_and_sent_count_follow_the_list) |
| 303 | `TestMainWindowSignalHandlers.test_a_finished_run_keeps_the_count_and_drops_the_estimate` | 6.e: live run strip (the idle branch is S run_strip_and_sent_count_follow_the_list) |
| 322 | `TestMainWindowSignalHandlers.test_hash_and_thumbnail_progress_handlers` | **6.c**: not asserted for the hash half: `smoke::add_files_hashes_and_dedupes` fills the hashes and reads the status strings, but not the gauge or `~Nm left` (the duration text is `lib::text::tests::durations_round_to_even_and_pad_on_request`); the thumbnail half is 6.d's |
| 328 | `TestMainWindowSignalHandlers.test_auto_import_handler` | 6.g |
| 336 | `TestMainWindowSignalHandlers.test_row_refresh_paths` | 6.d |
| 343 | `TestMainWindowSignalHandlers.test_selection_and_preview_handlers` | 6.d/6.f |
| 350 | `TestMainWindowSignalHandlers.test_saucenao_quota_label_survives_odd_data` | 6.e |
| 360 | `TestMainWindowSignalHandlers.test_sorting_every_column` | 6.d |
| 403 | `TestEngineAlertsReachTheUser.test_the_channel_is_open_only_while_a_search_is_running` | 6.e: search run, pacing, engine alerts, parser health |
| 427 | `TestEngineAlertsReachTheUser.test_closing_mid_run_takes_the_listener_off_too` | 6.e: engine-alert listener |
| 438 | `TestEngineAlertsReachTheUser.test_a_forwarder_whose_window_is_gone_is_harmless` | 6.e: search run, pacing, engine alerts, parser health |
| 449 | `TestEngineAlertsReachTheUser.test_an_alert_becomes_a_box_and_a_status_line` | 6.e: search run, pacing, engine alerts, parser health |
| 467 | `TestEngineAlertsReachTheUser.test_enabling_lens_without_its_dependencies_warns_at_once` | 6.e: search run, pacing, engine alerts, parser health |
| 488 | `TestEngineAlertsReachTheUser.test_a_working_install_says_nothing` | 6.e: search run, pacing, engine alerts, parser health |
| 520 | `TestMessageBoxesAreNeverNative.test_built_boxes_opt_out_of_native_dialogs` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 527 | `TestMessageBoxesAreNeverNative.test_wrappers_match_qt_default_buttons` | **6.c**: `lib::dialogs::tests::a_destructive_question_defaults_to_no`, `lib::dialogs::tests::a_one_button_box_defaults_to_ok` (Qt's defaults (Ok; Yes) become Ok and No on purpose: design gate 2.6, class D2) |
| 536 | `TestMessageBoxesAreNeverNative.test_wrapper_returns_the_clicked_button_and_does_not_leak` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 561 | `TestMessageBoxesAreNeverNative.test_no_static_qmessagebox_helpers_remain_in_the_gui` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 574 | `TestMessageBoxesAreNeverNative.test_no_bare_qmessagebox_construction_in_the_gui` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 613 | `TestDialogLifetime.test_message_boxes_do_not_accumulate` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 624 | `TestDialogLifetime.test_compare_dialogs_do_not_accumulate` | 6.f |
| 636 | `TestDialogLifetime.test_result_and_fields_survive_until_the_next_event_loop_turn` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 674 | `TestCompareViewZoom.test_new_images_start_fitted` | 6.f: Review, compare and its shortcuts |
| 680 | `TestCompareViewZoom.test_actual_size_is_one_image_pixel_per_screen_pixel` | 6.f: Review, compare and its shortcuts |
| 685 | `TestCompareViewZoom.test_zoom_keeps_the_point_under_the_cursor` | 6.f: Review, compare and its shortcuts |
| 701 | `TestCompareViewZoom.test_panning_cannot_lose_the_image_offscreen` | 6.f: Review, compare and its shortcuts |
| 711 | `TestCompareViewZoom.test_an_axis_that_fits_stays_centred` | 6.f: Review, compare and its shortcuts |
| 718 | `TestCompareViewZoom.test_zoom_is_bounded` | 6.f: Review, compare and its shortcuts |
| 725 | `TestCompareViewZoom.test_painting_at_every_zoom_and_split` | 6.f: Review, compare and its shortcuts |
| 735 | `TestCompareViewZoom.test_wipe_line_colour_is_mode_invariant` | 6.f: Review, compare and its shortcuts |
| 755 | `TestCompareViewZoom.test_empty_view_is_safe` | 6.f: Review, compare and its shortcuts |
| 766 | `TestCompareViewZoom.test_only_one_image_still_zooms` | 6.f: Review, compare and its shortcuts |
| 810 | `TestWorkerRetirement.test_running_worker_is_kept_referenced` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 816 | `TestWorkerRetirement.test_pruning_never_releases_a_running_worker` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 823 | `TestWorkerRetirement.test_finished_worker_is_released` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 831 | `TestWorkerRetirement.test_retiring_none_is_safe` | **6.c**: `lib::worker::tests::a_slot_holds_one_job_and_replacing_silences_the_old_one` runs `Slot::replace` on a fresh empty `Slot`, which calls `Slot::retire` on the empty slot first (`worker.rs`), and every smoke scenario's `close_teardown` retires empty slots (`session_ops.rs`); no test names the empty-slot case or asserts its result beyond "does not panic" |
| 834 | `TestWorkerRetirement.test_replacing_a_running_candidate_worker_does_not_orphan_it` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 848 | `TestContextMenuLabels.test_one_row_reads_in_the_singular` | 6.d: Queue table, filters, context menu, export, tag files |
| 852 | `TestContextMenuLabels.test_several_rows_carry_the_count` | 6.d: Queue table, filters, context menu, export, tag files |
| 858 | `TestContextMenuLabels.test_zero_is_not_treated_as_one` | 6.d: Queue table, filters, context menu, export, tag files |
| 914 | `TestTableContextMenu.test_every_actionable_entry_has_a_handler` | 6.d: Queue table, filters, context menu, export, tag files |
| 920 | `TestTableContextMenu.test_the_same_holds_with_one_row_and_with_none` | 6.d: Queue table, filters, context menu, export, tag files |
| 927 | `TestTableContextMenu.test_nothing_selected_offers_only_the_select_by_menus` | 6.d: Queue table, filters, context menu, export, tag files |
| 938 | `TestTableContextMenu.test_labels_follow_the_number_of_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 946 | `TestTableContextMenu.test_one_image_only_actions_are_disabled_for_several` | 6.d: Queue table, filters, context menu, export, tag files |
| 954 | `TestTableContextMenu.test_those_same_actions_are_enabled_for_one` | 6.d: Queue table, filters, context menu, export, tag files |
| 960 | `TestTableContextMenu.test_reset_is_disabled_when_no_row_has_a_result` | 6.d: Queue table, filters, context menu, export, tag files |
| 971 | `TestTableContextMenu.test_picking_an_entry_runs_its_own_action` | 6.d: Queue table, filters, context menu, export, tag files |
| 982 | `TestTableContextMenu.test_each_engine_entry_searches_that_engine` | 6.d: Queue table, filters, context menu, export, tag files |
| 991 | `TestTableContextMenu.test_the_engine_submenu_offers_every_engine` | 6.d: Queue table, filters, context menu, export, tag files |
| 1001 | `TestTableContextMenu.test_a_removal_while_the_menu_is_open_does_not_shift_the_action` | 6.d: Queue table, filters, context menu, export, tag files |
| 1050 | `TestSessionAutosaverOnItsOwn.test_an_empty_list_is_never_written` | **6.c**: `smoke::autosave_writes_the_list_off_the_main_thread` (an empty list is skipped) |
| 1056 | `TestSessionAutosaverOnItsOwn.test_the_timer_runs_at_the_configured_interval` | **6.c**: `lib::session_ops::tests::autosave_interval_is_floored_and_off_stops_it` (the interval rule is a pure function) |
| 1065 | `TestSessionAutosaverOnItsOwn.test_turning_it_off_stops_the_timer` | **6.c**: `lib::session_ops::tests::autosave_interval_is_floored_and_off_stops_it` (the interval rule is a pure function) |
| 1074 | `TestSessionAutosaverOnItsOwn.test_a_self_defeating_interval_is_floored` | **6.c**: `lib::session_ops::tests::autosave_interval_is_floored_and_off_stops_it` (the interval rule is a pure function) |
| 1085 | `TestSessionAutosaverOnItsOwn.test_a_failed_save_does_not_advance_the_revision_map` | n/a: revision bookkeeping of the incremental save is not ported: class D1, DAN-2227 |
| 1093 | `TestSessionAutosaverOnItsOwn.test_a_successful_save_records_what_was_written` | n/a: revision bookkeeping of the incremental save is not ported: class D1, DAN-2227 |
| 1097 | `TestSessionAutosaverOnItsOwn.test_a_non_dict_revision_map_is_refused` | n/a: revision bookkeeping of the incremental save is not ported: class D1, DAN-2227 |
| 1103 | `TestSessionAutosaverOnItsOwn.test_shutdown_stops_the_timer_with_no_worker_running` | **6.c**: `smoke::close_saves_and_marks_a_clean_exit` asserts the closing flag, the final save and the markers; that the timer is removed is not asserted |
| 1140 | `TestE621LoginTest.test_half_filled_credentials_are_refused_before_any_request` | 6.h: Preferences |
| 1152 | `TestE621LoginTest.test_it_sends_the_typed_credentials_not_the_saved_ones` | 6.h: Preferences |
| 1163 | `TestE621LoginTest.test_it_uses_the_apps_own_header_builder` | 6.h: Preferences |
| 1174 | `TestE621LoginTest.test_accepted_credentials_are_reported_as_such` | 6.h: Preferences |
| 1179 | `TestE621LoginTest.test_a_401_is_reported_as_a_rejection_with_what_to_do` | 6.h: Preferences |
| 1186 | `TestE621LoginTest.test_a_network_error_is_not_reported_as_a_rejection` | 6.h: Preferences |
| 1196 | `TestE621LoginTest.test_an_unexpected_status_is_neither_accept_nor_reject` | 6.h: Preferences |
| 1212 | `TestFilterLabelText.test_nothing_chosen_reads_as_all` | 6.d: Queue table, filters, context menu, export, tag files |
| 1216 | `TestFilterLabelText.test_everything_ticked_also_reads_as_all` | 6.d: Queue table, filters, context menu, export, tag files |
| 1222 | `TestFilterLabelText.test_one_choice_is_named` | 6.d: Queue table, filters, context menu, export, tag files |
| 1227 | `TestFilterLabelText.test_several_choices_are_counted` | 6.d: Queue table, filters, context menu, export, tag files |
| 1232 | `TestFilterLabelText.test_nothing_ticked_is_not_the_same_as_all` | 6.d: Queue table, filters, context menu, export, tag files |
| 1238 | `TestFilterLabelText.test_the_count_says_hidden_not_removed` | 6.d: Queue table, filters, context menu, export, tag files |
| 1301 | `TestWorkerRegistryOnItsOwn.test_a_running_worker_is_parked` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 1307 | `TestWorkerRegistryOnItsOwn.test_a_finished_worker_is_released_on_prune` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 1314 | `TestWorkerRegistryOnItsOwn.test_retiring_none_is_safe` | **6.c**: `lib::worker::tests::a_slot_holds_one_job_and_replacing_silences_the_old_one` runs `Slot::replace` on a fresh empty `Slot`, which calls `Slot::retire` on the empty slot first (`worker.rs`), and every smoke scenario's `close_teardown` retires empty slots (`session_ops.rs`); no test names the empty-slot case or asserts its result beyond "does not panic" |
| 1318 | `TestWorkerRegistryOnItsOwn.test_a_worker_that_finishes_within_the_grace_is_never_parked` | **6.c**: `lib::worker::tests::a_stop_is_visible_to_the_job_and_its_result_is_still_delivered`, `lib::worker::tests::shutdown_retires_every_job` (stop reaches the job; the grace is Workers::shutdown) |
| 1324 | `TestWorkerRegistryOnItsOwn.test_stop_is_called_when_the_worker_offers_one` | **6.c**: `lib::worker::tests::a_stop_is_visible_to_the_job_and_its_result_is_still_delivered`, `lib::worker::tests::shutdown_retires_every_job` (stop reaches the job; the grace is Workers::shutdown) |
| 1337 | `TestWorkerRegistryOnItsOwn.test_retire_attrs_walks_named_attributes` | **6.c**: `lib::worker::tests::a_retired_job_is_never_heard_again`, `lib::worker::tests::shutdown_retires_every_job` (one Slot per attribute; close retires them all) |
| 1347 | `TestWorkerRegistryOnItsOwn.test_retire_attrs_can_clear_the_attribute` | **6.c**: `lib::worker::tests::a_retired_job_is_never_heard_again`, `lib::worker::tests::shutdown_retires_every_job` (one Slot per attribute; close retires them all) |
| 1357 | `TestWorkerRegistryOnItsOwn.test_wait_for_ignores_a_worker_that_is_not_running` | n/a: Qt test hygiene / the parked-thread registry; every Rust fixture builds its window on its own temp profile and there is no parked-thread list |
| 1360 | `TestWorkerRegistryOnItsOwn.test_a_queued_prune_survives_the_registry_being_collected` | n/a: a QThread must not be destroyed while running, so Python parks it; a tokio task is not a QThread and nothing is parked |
| 1443 | `TestSessionStoreIsIsolatedPerTest.test_a_closing_a_window_autosaves_its_entries` | **6.c**: `smoke::close_saves_and_marks_a_clean_exit` (the final save wrote the list), `smoke::close_writes_an_empty_list_over_the_old_rows` |
| 1453 | `TestSessionStoreIsIsolatedPerTest.test_b_a_later_test_never_sees_it` | n/a: Qt test hygiene / the parked-thread registry; every Rust fixture builds its window on its own temp profile and there is no parked-thread list |
| 1485 | `TestWorkerCallbacksDuringClose.test_missing_files_checked_is_a_no_op_while_closing` | **6.c**: `lib::worker::tests::a_retired_job_is_never_heard_again` (close retires the check's job); `smoke::close_saves_and_marks_a_clean_exit` sets the closing flag |
| 1491 | `TestWorkerCallbacksDuringClose.test_availability_finished_is_a_no_op_while_closing` | 6.e: availability worker |
| 1499 | `TestWorkerCallbacksDuringClose.test_file_hash_finished_is_a_no_op_while_closing` | **6.c**: `lib::worker::tests::a_retired_job_is_never_heard_again` (close retires the hash job); `smoke::close_saves_and_marks_a_clean_exit` sets the closing flag |
| 1508 | `TestWorkerCallbacksDuringClose.test_hydrus_import_poll_finished_is_a_no_op_while_closing` | 6.g: import poll |
| 1538 | `TestMessageBoxSurvivesDeletionDuringItsOwnExec.test_deleting_the_parent_mid_exec_does_not_abort` | n/a: Qt's native-dialog and nested-exec() hazards; a GTK dialog never blocks and has no native-dialog path (the Prompter takes a continuation) |
| 1563 | `TestDisplayHelpers.test_duration_formatting` | **6.c**: `lib::text::tests::durations_round_to_even_and_pad_on_request` (_format_duration) |
| 1569 | `TestDisplayHelpers.test_duration_formatting_reaches_days` | **6.c**: `lib::text::tests::durations_round_to_even_and_pad_on_request` (_format_duration) |
| 1578 | `TestDisplayHelpers.test_finish_time_phrasing_follows_the_distance` | 6.d: Queue table, filters, context menu, export, tag files |
| 1589 | `TestDisplayHelpers.test_an_absurd_horizon_yields_no_finish_time` | 6.d: Queue table, filters, context menu, export, tag files |
| 1595 | `TestDisplayHelpers.test_entry_labels_handle_unsearched_entries` | 6.d: Queue table, filters, context menu, export, tag files |
| 1674 | `TestAutosaveIsOffTheGuiThread.test_the_call_returns_immediately_and_writes_in_the_background` | **6.c**: `smoke::autosave_writes_the_list_off_the_main_thread` asserts the write lands while the loop is pumped; that the call returns before the write is not asserted |
| 1684 | `TestAutosaveIsOffTheGuiThread.test_a_second_tick_does_not_stack_another_write` | **6.c**: not asserted: nothing tests that a tick during a running write is skipped (`autosave_worker.is_running()` in `session_ops::autosave_now`) |
| 1692 | `TestAutosaveIsOffTheGuiThread.test_an_empty_list_is_never_written` | **6.c**: `smoke::autosave_writes_the_list_off_the_main_thread` (an empty list is skipped) |
| 1734 | `TestDeleteFromHydrus.test_confirmed_delete_removes_only_what_hydrus_confirmed` | 6.g |
| 1747 | `TestDeleteFromHydrus.test_saying_no_deletes_nothing` | 6.g |
| 1757 | `TestDeleteFromHydrus.test_a_file_hydrus_still_holds_afterwards_stays_in_the_list` | 6.g |
| 1768 | `TestDeleteFromHydrus.test_nothing_held_means_nothing_is_asked` | 6.g |
| 1779 | `TestDeleteFromHydrus.test_without_an_access_key_it_refuses_early` | 6.g |
| 1816 | `TestTagsAppearWhenAMatchLands.test_tags_show_without_reselecting_the_row` | 6.d/6.e |
| 1832 | `TestTagsAppearWhenAMatchLands.test_a_re_search_clears_the_previous_tags` | 6.e |
| 1846 | `TestTagsAppearWhenAMatchLands.test_an_update_for_another_row_still_follows_the_search` | 6.d/6.e |
| 1898 | `TestCompareUsesTheSelectedCandidate.test_an_unvisited_candidate_has_its_details_fetched` | 6.f: Review, compare and its shortcuts |
| 1905 | `TestCompareUsesTheSelectedCandidate.test_a_candidate_that_already_has_a_url_is_not_refetched` | 6.f: Review, compare and its shortcuts |
| 1910 | `TestCompareUsesTheSelectedCandidate.test_each_candidate_gets_its_own_picture` | 6.f: Review, compare and its shortcuts |
| 1941 | `TestCompareUsesTheSelectedCandidate.test_a_match_that_will_not_load_says_so` | 6.f: Review, compare and its shortcuts |
| 1950 | `TestCompareUsesTheSelectedCandidate.test_a_failing_fetch_does_not_take_the_window_down` | 6.f: Review, compare and its shortcuts |
| 1983 | `TestParserHealthDialog.test_it_lists_every_site_even_before_anything_is_known` | 6.e: search run, pacing, engine alerts, parser health |
| 1989 | `TestParserHealthDialog.test_a_broken_parser_shows_in_the_session_column` | 6.e: search run, pacing, engine alerts, parser health |
| 1996 | `TestParserHealthDialog.test_live_results_fill_the_check_column` | 6.e: search run, pacing, engine alerts, parser health |
| 2008 | `TestParserHealthDialog.test_an_all_clear_says_so` | 6.e: search run, pacing, engine alerts, parser health |
| 2015 | `TestParserHealthDialog.test_a_check_that_could_not_run_is_not_reported_as_all_clear` | 6.e: search run, pacing, engine alerts, parser health |
| 2020 | `TestParserHealthDialog.test_the_check_runs_off_the_gui_thread` | 6.e: search run, pacing, engine alerts, parser health |
| 2093 | `TestRateLimitIsMeasuredEngineToEngine.test_engine_to_engine_gap_is_never_shortened` | 6.e: search run, pacing, engine alerts, parser health |
| 2111 | `TestRateLimitIsMeasuredEngineToEngine.test_non_engine_work_does_not_add_to_the_batch` | 6.e: search run, pacing, engine alerts, parser health |
| 2156 | `TestImageTableModel.test_columns_render_their_entry_fields` | 6.d: Queue table, filters, context menu, export, tag files |
| 2170 | `TestImageTableModel.test_similarity_of_none_is_a_dash_not_zero` | 6.d: Queue table, filters, context menu, export, tag files |
| 2177 | `TestImageTableModel.test_missing_file_is_marked_in_the_file_column` | 6.d: Queue table, filters, context menu, export, tag files |
| 2195 | `TestImageTableModel.test_present_file_gets_no_colour_or_tooltip` | 6.d: Queue table, filters, context menu, export, tag files |
| 2205 | `TestImageTableModel.test_sent_column_distinguishes_queued_from_confirmed` | 6.d: Queue table, filters, context menu, export, tag files |
| 2236 | `TestImageTableModel.test_status_column_carries_a_glyph` | 6.d: Queue table, filters, context menu, export, tag files |
| 2255 | `TestImageTableModel.test_chip_keys_in_the_same_column_are_pairwise_distinct` | 6.d: Queue table, filters, context menu, export, tag files |
| 2323 | `TestImageTableModel.test_ink_tiers_resolve_to_distinct_colours_per_theme` | 6.d: Queue table, filters, context menu, export, tag files |
| 2358 | `TestImageTableModel.test_thumb_column_has_no_text` | 6.d: Queue table, filters, context menu, export, tag files |
| 2364 | `TestImageTableModel.test_headers_match_the_column_list` | 6.d: Queue table, filters, context menu, export, tag files |
| 2372 | `TestImageTableModel.test_a_valid_parent_has_no_children` | 6.d: Queue table, filters, context menu, export, tag files |
| 2380 | `TestImageTableModel.test_out_of_range_rows_return_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 2385 | `TestImageTableModel.test_refreshing_a_removed_entry_is_a_no_op` | 6.d: Queue table, filters, context menu, export, tag files |
| 2391 | `TestImageTableModel.test_row_cost_does_not_grow_with_the_list` | 6.d: Queue table, filters, context menu, export, tag files |
| 2450 | `TestTableSelectionHelpers.test_selects_every_matching_row` | 6.d |
| 2454 | `TestTableSelectionHelpers.test_selecting_replaces_the_previous_selection` | 6.d |
| 2459 | `TestTableSelectionHelpers.test_no_match_clears_the_selection_and_says_so` | 6.d |
| 2465 | `TestTableSelectionHelpers.test_whole_rows_are_selected_not_single_cells` | 6.d |
| 2491 | `TestRowNumbersAreShown.test_rows_are_numbered_from_one` | 6.d: Queue table, filters, context menu, export, tag files |
| 2496 | `TestRowNumbersAreShown.test_numbering_follows_rows_not_entries` | 6.d: Queue table, filters, context menu, export, tag files |
| 2503 | `TestRowNumbersAreShown.test_out_of_range_sections_are_blank` | 6.d: Queue table, filters, context menu, export, tag files |
| 2506 | `TestRowNumbersAreShown.test_column_titles_still_work` | 6.d: Queue table, filters, context menu, export, tag files |
| 2588 | `TestPerHostPacing.test_with_no_overrides_every_host_keeps_the_global_gap` | 6.e: search run, pacing, engine alerts, parser health |
| 2598 | `TestPerHostPacing.test_a_skipped_host_imposes_no_wait` | 6.e: search run, pacing, engine alerts, parser health |
| 2613 | `TestPerHostPacing.test_a_longer_override_extends_the_wait` | 6.e: search run, pacing, engine alerts, parser health |
| 2621 | `TestPerHostPacing.test_a_stricter_host_paces_the_whole_wave` | 6.e: search run, pacing, engine alerts, parser health |
| 2630 | `TestPerHostPacing.test_a_broken_engine_report_falls_back_instead_of_hammering` | 6.e: search run, pacing, engine alerts, parser health |
| 2665 | `TestPerEngineDelaySettingsRoundTrip.test_untouched_boxes_store_no_override` | 6.h: Preferences |
| 2670 | `TestPerEngineDelaySettingsRoundTrip.test_an_override_survives_a_round_trip` | 6.h: Preferences |
| 2674 | `TestPerEngineDelaySettingsRoundTrip.test_other_engines_are_not_given_zero_overrides` | 6.h: Preferences |
| 2682 | `TestPerEngineDelaySettingsRoundTrip.test_a_zero_override_never_reaches_the_settings` | 6.h: Preferences |
| 2687 | `TestPerEngineDelaySettingsRoundTrip.test_the_stored_pair_is_always_ordered` | 6.h: Preferences |
| 2691 | `TestPerEngineDelaySettingsRoundTrip.test_a_half_filled_pair_becomes_a_usable_interval` | 6.h: Preferences |
| 2697 | `TestPerEngineDelaySettingsRoundTrip.test_what_is_saved_is_what_the_limiter_reads` | 6.h: Preferences |
| 2743 | `TestTableFollowsEntryListReplacement.test_removing_imported_entries_updates_the_table` | 6.d: Queue table, filters, context menu, export, tag files |
| 2750 | `TestTableFollowsEntryListReplacement.test_thumbnails_follow_their_entries_after_a_removal` | 6.d: Queue table, filters, context menu, export, tag files |
| 2775 | `TestTableFollowsEntryListReplacement.test_removing_every_entry_empties_the_table` | 6.d: Queue table, filters, context menu, export, tag files |
| 2780 | `TestTableFollowsEntryListReplacement.test_the_model_tracks_the_current_list_object` | 6.d: Queue table, filters, context menu, export, tag files |
| 2786 | `TestTableFollowsEntryListReplacement.test_replacing_the_list_wholesale_is_picked_up` | **6.c**: `smoke::autosave_writes_the_list_off_the_main_thread` (the queue is pushed to and then replaced; the autosave reads the current list) |
| 2796 | `TestTableFollowsEntryListReplacement.test_clearing_the_list_wholesale_is_picked_up` | 6.d: Queue table, filters, context menu, export, tag files |
| 2802 | `TestTableFollowsEntryListReplacement.test_in_place_removal_still_works` | 6.d: Queue table, filters, context menu, export, tag files |
| 2888 | `TestSendFilesToHydrus.test_nothing_selected_asks_for_a_selection` | 6.g |
| 2895 | `TestSendFilesToHydrus.test_a_missing_access_key_stops_before_sending` | 6.g |
| 2906 | `TestSendFilesToHydrus.test_the_windows_settings_reach_the_send` | 6.g |
| 2918 | `TestSendFilesToHydrus.test_each_selected_row_is_sent_once` | 6.g |
| 2926 | `TestSendFilesToHydrus.test_an_unselected_row_is_never_sent` | 6.g |
| 2935 | `TestSendFilesToHydrus.test_a_failure_is_reported_and_does_not_stop_the_rest` | 6.g |
| 2947 | `TestSendFilesToHydrus.test_a_warning_counts_as_sent_but_is_surfaced` | 6.g |
| 2956 | `TestSendFilesToHydrus.test_rows_stay_when_remove_after_import_is_off` | 6.g |
| 2963 | `TestSendFilesToHydrus.test_only_successful_rows_are_removed_when_that_is_on` | 6.g |
| 2976 | `TestSendFilesToHydrus.test_the_table_matches_the_entries_after_removal` | 6.g |
| 2999 | `TestSendUrlToHydrusImporter.test_a_file_hatate_downloaded_is_done_and_not_polled` | 6.g |
| 3016 | `TestSendUrlToHydrusImporter.test_nothing_selected_asks_for_a_selection` | 6.g |
| 3022 | `TestSendUrlToHydrusImporter.test_a_missing_access_key_stops_before_queueing` | 6.g |
| 3030 | `TestSendUrlToHydrusImporter.test_each_selected_url_is_queued` | 6.g |
| 3038 | `TestSendUrlToHydrusImporter.test_an_entry_with_no_url_is_skipped_not_failed` | 6.g |
| 3047 | `TestSendUrlToHydrusImporter.test_a_failure_is_surfaced` | 6.g |
| 3053 | `TestSendUrlToHydrusImporter.test_queued_is_not_treated_as_imported` | 6.g |
| 3063 | `TestSendUrlToHydrusImporter.test_removal_waits_for_confirmation_rather_than_removing_at_once` | 6.g |
| 3077 | `TestSendUrlToHydrusImporter.test_confirmed_entries_are_removed_unconfirmed_ones_kept` | 6.g |
| 3087 | `TestSendUrlToHydrusImporter.test_a_resolved_entry_records_its_hash` | 6.g |
| 3094 | `TestSendUrlToHydrusImporter.test_an_unresolved_entry_is_marked_not_confirmed` | 6.g |
| 3129 | `TestDownloadAndSendToHydrus.test_nothing_selected_asks_for_a_selection` | 6.g |
| 3135 | `TestDownloadAndSendToHydrus.test_a_missing_access_key_stops_before_downloading` | 6.g |
| 3146 | `TestDownloadAndSendToHydrus.test_each_selected_row_is_downloaded_and_sent` | 6.g |
| 3153 | `TestDownloadAndSendToHydrus.test_a_row_with_no_direct_url_is_skipped_not_failed` | 6.g |
| 3161 | `TestDownloadAndSendToHydrus.test_a_failure_is_surfaced` | 6.g |
| 3167 | `TestDownloadAndSendToHydrus.test_only_successful_rows_are_removed` | 6.g |
| 3178 | `TestDownloadAndSendToHydrus.test_the_downloaded_files_are_cleaned_up` | 6.g |
| 3200 | `TestDownloadAndSendToHydrus.test_cleanup_happens_even_when_every_download_fails` | 6.g |
| 3278 | `TestAutoImportStagingIsCleanedUp.test_the_staging_directory_is_removed_after_a_batch` | 6.g: Hydrus / Pawchive / auto-import actions |
| 3285 | `TestAutoImportStagingIsCleanedUp.test_it_is_removed_even_when_the_batch_is_stopped` | 6.g: Hydrus / Pawchive / auto-import actions |
| 3305 | `TestAutoImportStagingIsCleanedUp.test_it_is_removed_even_when_the_run_raises` | 6.g: Hydrus / Pawchive / auto-import actions |
| 3373 | `TestResetRowsAction.test_only_the_named_rows_are_reset` | 6.d: Queue table, filters, context menu, export, tag files |
| 3381 | `TestResetRowsAction.test_declining_the_confirmation_changes_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 3392 | `TestResetRowsAction.test_the_cached_result_goes_too` | 6.d: Queue table, filters, context menu, export, tag files |
| 3399 | `TestResetRowsAction.test_rows_are_never_removed_from_the_list` | 6.d: Queue table, filters, context menu, export, tag files |
| 3405 | `TestResetRowsAction.test_the_table_shows_the_reset_state` | 6.d: Queue table, filters, context menu, export, tag files |
| 3416 | `TestResetRowsAction.test_user_tags_survive_but_booru_tags_do_not` | 6.d: Queue table, filters, context menu, export, tag files |
| 3420 | `TestResetRowsAction.test_resetting_unsearched_rows_says_so_and_does_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 3429 | `TestResetRowsAction.test_the_status_bar_reports_what_happened` | 6.d: Queue table, filters, context menu, export, tag files |
| 3435 | `TestResetRowsAction.test_a_missing_cache_entry_is_not_counted` | 6.d: Queue table, filters, context menu, export, tag files |
| 3442 | `TestResetRowsAction.test_an_entry_with_no_hash_is_still_reset` | 6.d: Queue table, filters, context menu, export, tag files |
| 3502 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_filter_hides_rows_without_removing_them` | 6.d: Queue table, filters, context menu, export, tag files |
| 3507 | `TestFilteredRowsAreNeverActedOnByAccident.test_row_zero_means_the_first_VISIBLE_entry` | 6.d: Queue table, filters, context menu, export, tag files |
| 3512 | `TestFilteredRowsAreNeverActedOnByAccident.test_a_finished_thumbnail_repaints_the_row_it_belongs_to` | 6.d: Queue table, filters, context menu, export, tag files |
| 3528 | `TestFilteredRowsAreNeverActedOnByAccident.test_a_thumbnail_for_a_hidden_row_repaints_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 3540 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_searched_row_selected_is_the_one_on_screen` | 6.d: Queue table, filters, context menu, export, tag files |
| 3554 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_thumbnail_range_is_clamped_to_the_visible_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 3561 | `TestFilteredRowsAreNeverActedOnByAccident.test_removing_the_selection_removes_only_visible_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 3570 | `TestFilteredRowsAreNeverActedOnByAccident.test_sending_to_hydrus_only_sends_visible_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 3593 | `TestFilteredRowsAreNeverActedOnByAccident.test_resetting_only_resets_visible_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 3608 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_current_entry_follows_the_visible_row` | 6.d: Queue table, filters, context menu, export, tag files |
| 3613 | `TestFilteredRowsAreNeverActedOnByAccident.test_clearing_the_filter_shows_everything_again` | 6.d: Queue table, filters, context menu, export, tag files |
| 3619 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_selection_survives_a_filter_change` | 6.d: Queue table, filters, context menu, export, tag files |
| 3626 | `TestFilteredRowsAreNeverActedOnByAccident.test_a_selected_row_the_filter_hides_is_dropped_from_the_selection` | 6.d: Queue table, filters, context menu, export, tag files |
| 3633 | `TestFilteredRowsAreNeverActedOnByAccident.test_sorting_while_filtered_reselects_the_right_row` | 6.d: Queue table, filters, context menu, export, tag files |
| 3642 | `TestFilteredRowsAreNeverActedOnByAccident.test_row_numbers_run_one_to_n_over_visible_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 3650 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_count_says_rows_are_hidden_not_removed` | 6.d: Queue table, filters, context menu, export, tag files |
| 3662 | `TestFilteredRowsAreNeverActedOnByAccident.test_no_count_is_shown_when_nothing_is_filtered` | 6.d: Queue table, filters, context menu, export, tag files |
| 3665 | `TestFilteredRowsAreNeverActedOnByAccident.test_a_filter_matching_nothing_shows_an_empty_list_not_everything` | 6.d: Queue table, filters, context menu, export, tag files |
| 3672 | `TestFilteredRowsAreNeverActedOnByAccident.test_a_filter_does_not_limit_what_start_search_processes` | 6.d: Queue table, filters, context menu, export, tag files |
| 3694 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_filter_menus_offer_all_and_none` | 6.d: Queue table, filters, context menu, export, tag files |
| 3703 | `TestFilteredRowsAreNeverActedOnByAccident.test_none_hides_every_row` | 6.d: Queue table, filters, context menu, export, tag files |
| 3708 | `TestFilteredRowsAreNeverActedOnByAccident.test_all_shows_every_row_again` | 6.d: Queue table, filters, context menu, export, tag files |
| 3713 | `TestFilteredRowsAreNeverActedOnByAccident.test_all_counts_as_not_filtering` | 6.d: Queue table, filters, context menu, export, tag files |
| 3720 | `TestFilteredRowsAreNeverActedOnByAccident.test_none_then_ticking_one_isolates_it` | 6.d: Queue table, filters, context menu, export, tag files |
| 3729 | `TestFilteredRowsAreNeverActedOnByAccident.test_all_and_none_apply_to_one_menu_only` | 6.d: Queue table, filters, context menu, export, tag files |
| 3737 | `TestFilteredRowsAreNeverActedOnByAccident.test_the_tickboxes_follow_an_all_or_none_press` | 6.d: Queue table, filters, context menu, export, tag files |
| 3745 | `TestFilteredRowsAreNeverActedOnByAccident.test_all_none_are_absent_when_there_is_nothing_to_pick` | 6.d: Queue table, filters, context menu, export, tag files |
| 3798 | `TestPreviewClearsWhenRowsAreRemoved.test_a_picture_is_shown_while_a_row_is_selected` | 6.f: Review, compare and its shortcuts |
| 3804 | `TestPreviewClearsWhenRowsAreRemoved.test_removing_another_row_keeps_your_place` | 6.f: Review, compare and its shortcuts |
| 3820 | `TestPreviewClearsWhenRowsAreRemoved.test_removing_the_selected_row_moves_to_the_next` | 6.f: Review, compare and its shortcuts |
| 3829 | `TestPreviewClearsWhenRowsAreRemoved.test_an_auto_import_removal_leaves_the_watched_row_alone` | 6.f: Review, compare and its shortcuts |
| 3837 | `TestPreviewClearsWhenRowsAreRemoved.test_an_auto_import_removing_the_watched_row_moves_on` | 6.f: Review, compare and its shortcuts |
| 3846 | `TestPreviewClearsWhenRowsAreRemoved.test_the_matched_side_follows_as_well` | 6.f: Review, compare and its shortcuts |
| 3857 | `TestPreviewClearsWhenRowsAreRemoved.test_the_tag_list_and_readout_follow_as_well` | 6.f: Review, compare and its shortcuts |
| 3869 | `TestPreviewClearsWhenRowsAreRemoved.test_removing_everything_clears_it` | 6.f: Review, compare and its shortcuts |
| 3875 | `TestPreviewClearsWhenRowsAreRemoved.test_selecting_again_afterwards_still_works` | 6.f: Review, compare and its shortcuts |
| 3895 | `TestPreviewText.test_the_caption_names_the_site_and_the_score` | 6.f: Review, compare and its shortcuts |
| 3900 | `TestPreviewText.test_the_caption_omits_what_it_does_not_have` | 6.f: Review, compare and its shortcuts |
| 3905 | `TestPreviewText.test_a_site_with_no_score_still_reads` | 6.f: Review, compare and its shortcuts |
| 3910 | `TestPreviewText.test_not_searched_and_not_found_are_not_confused` | 6.f: Review, compare and its shortcuts |
| 3920 | `TestPreviewText.test_searching_says_so` | 6.f: Review, compare and its shortcuts |
| 3926 | `TestPreviewText.test_an_error_carries_its_message` | 6.f: Review, compare and its shortcuts |
| 3933 | `TestPreviewText.test_an_error_with_no_message_does_not_say_none` | 6.f: Review, compare and its shortcuts |
| 3939 | `TestPreviewText.test_match_info_omits_pieces_that_are_unknown` | 6.f: Review, compare and its shortcuts |
| 3947 | `TestPreviewText.test_match_info_with_nothing_at_all_says_so` | 6.f: Review, compare and its shortcuts |
| 3953 | `TestPreviewText.test_no_candidate_is_blank_not_a_placeholder` | 6.f: Review, compare and its shortcuts |
| 3957 | `TestPreviewText.test_human_size_scales_units` | 6.f: Review, compare and its shortcuts |
| 3964 | `TestPreviewText.test_the_banner_is_empty_without_a_candidate` | 6.f: Review, compare and its shortcuts |
| 3971 | `TestPreviewText.test_a_bigger_match_reads_as_good` | 6.f: Review, compare and its shortcuts |
| 3978 | `TestPreviewText.test_a_smaller_match_is_not_green` | 6.f: Review, compare and its shortcuts |
| 3987 | `TestPreviewText.test_the_banner_states_the_similarity` | 6.f: Review, compare and its shortcuts |
| 3994 | `TestPreviewText.test_local_info_reports_unreadable_files_plainly` | 6.f: Review, compare and its shortcuts |
| 4019 | `TestColumnSortKeys.test_every_column_has_a_key` | 6.d: Queue table, filters, context menu, export, tag files |
| 4028 | `TestColumnSortKeys.test_an_unknown_column_sorts_everything_equal` | 6.d: Queue table, filters, context menu, export, tag files |
| 4033 | `TestColumnSortKeys.test_files_sort_case_insensitively` | 6.d: Queue table, filters, context menu, export, tag files |
| 4040 | `TestColumnSortKeys.test_unsearched_rows_sort_below_any_similarity` | 6.d: Queue table, filters, context menu, export, tag files |
| 4046 | `TestColumnSortKeys.test_tags_sort_by_how_many` | 6.d: Queue table, filters, context menu, export, tag files |
| 4053 | `TestColumnSortKeys.test_sent_state_orders_unsent_then_queued_then_confirmed` | 6.d: Queue table, filters, context menu, export, tag files |
| 4062 | `TestColumnSortKeys.test_a_missing_booru_name_does_not_raise` | 6.d: Queue table, filters, context menu, export, tag files |
| 4066 | `TestColumnSortKeys.test_size_difference_sorts_by_ratio_not_label` | 6.d: Queue table, filters, context menu, export, tag files |
| 4103 | `TestHydrusReconcileAction.test_nothing_waiting_never_contacts_hydrus` | 6.g |
| 4113 | `TestHydrusReconcileAction.test_a_queued_entry_is_confirmed_when_hydrus_has_it` | 6.g |
| 4124 | `TestHydrusReconcileAction.test_an_unknown_file_stays_queued` | 6.g |
| 4135 | `TestHydrusReconcileAction.test_hydrus_being_unreachable_is_not_a_crash` | 6.g |
| 4147 | `TestHydrusReconcileAction.test_the_action_says_so_when_there_is_nothing_to_do` | 6.g |
| 4152 | `TestHydrusReconcileAction.test_the_quiet_pass_stays_silent_when_nothing_changed` | 6.g |
| 4165 | `TestHydrusReconcileAction.test_the_quiet_pass_speaks_up_when_it_changed_something` | 6.g |
| 4185 | `TestNoTagsReasonIsShown.test_the_reason_appears_under_the_match_info` | 6.f: Review, compare and its shortcuts |
| 4192 | `TestNoTagsReasonIsShown.test_it_is_on_its_own_line` | 6.f: Review, compare and its shortcuts |
| 4200 | `TestNoTagsReasonIsShown.test_a_match_with_tags_shows_no_reason` | 6.f: Review, compare and its shortcuts |
| 4205 | `TestNoTagsReasonIsShown.test_a_reason_still_shows_when_nothing_else_is_known` | 6.f: Review, compare and its shortcuts |
| 4213 | `TestNoTagsReasonIsShown.test_the_reason_survives_the_search_cache` | 6.f: Review, compare and its shortcuts |
| 4273 | `TestSelectionSurvivesARefresh.test_a_plain_refresh_keeps_the_selection` | 6.d: Queue table, filters, context menu, export, tag files |
| 4278 | `TestSelectionSurvivesARefresh.test_dropping_dead_matches_keeps_your_place` | 6.d: Queue table, filters, context menu, export, tag files |
| 4290 | `TestSelectionSurvivesARefresh.test_editing_a_tag_keeps_your_place` | 6.d: Queue table, filters, context menu, export, tag files |
| 4302 | `TestSelectionSurvivesARefresh.test_a_sort_keeps_your_place` | 6.d: Queue table, filters, context menu, export, tag files |
| 4309 | `TestSelectionSurvivesARefresh.test_a_refresh_that_removes_the_selected_entry_selects_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 4318 | `TestSelectionSurvivesARefresh.test_a_multi_row_selection_survives_intact` | 6.d: Queue table, filters, context menu, export, tag files |
| 4369 | `TestFailedSearchesAreRetriedOnce.test_a_failed_image_is_tried_again` | 6.e: search run, pacing, engine alerts, parser health |
| 4378 | `TestFailedSearchesAreRetriedOnce.test_a_successful_image_is_not_tried_again` | 6.e: search run, pacing, engine alerts, parser health |
| 4383 | `TestFailedSearchesAreRetriedOnce.test_an_image_that_keeps_failing_is_not_retried_forever` | 6.e: search run, pacing, engine alerts, parser health |
| 4393 | `TestFailedSearchesAreRetriedOnce.test_the_setting_turns_it_off` | 6.e: search run, pacing, engine alerts, parser health |
| 4400 | `TestFailedSearchesAreRetriedOnce.test_several_failures_are_all_retried` | 6.e: search run, pacing, engine alerts, parser health |
| 4411 | `TestFailedSearchesAreRetriedOnce.test_a_not_found_is_not_a_failure` | 6.e: search run, pacing, engine alerts, parser health |
| 4419 | `TestFailedSearchesAreRetriedOnce.test_stopping_cancels_the_retry_pass` | 6.e: search run, pacing, engine alerts, parser health |
| 4439 | `TestFailedSearchesAreRetriedOnce.test_an_exhausted_daily_quota_cancels_the_retry_pass` | 6.e: search run, pacing, engine alerts, parser health |
| 4457 | `TestFailedSearchesAreRetriedOnce.test_progress_accounts_for_the_extra_attempts` | 6.e: search run, pacing, engine alerts, parser health |
| 4482 | `TestBorrowedTagsAreLabelled.test_the_preview_names_the_match_the_tags_came_from` | 6.f: Review, compare and its shortcuts |
| 4492 | `TestBorrowedTagsAreLabelled.test_a_match_with_its_own_tags_says_nothing_extra` | 6.f: Review, compare and its shortcuts |
| 4499 | `TestBorrowedTagsAreLabelled.test_the_provenance_survives_the_search_cache` | 6.f: Review, compare and its shortcuts |
| 4555 | `TestCompareDifferencesMode.test_a_pixmap_survives_the_round_trip_intact` | 6.f: Review, compare and its shortcuts |
| 4565 | `TestCompareDifferencesMode.test_the_overlay_is_the_local_image_s_shape` | 6.f: Review, compare and its shortcuts |
| 4574 | `TestCompareDifferencesMode.test_it_reports_what_it_found` | 6.f: Review, compare and its shortcuts |
| 4582 | `TestCompareDifferencesMode.test_both_pictures_are_shown_with_the_overlay_over_them` | 6.f: Review, compare and its shortcuts |
| 4593 | `TestCompareDifferencesMode.test_the_view_frames_the_pair_not_one_picture` | 6.f: Review, compare and its shortcuts |
| 4602 | `TestCompareDifferencesMode.test_the_overlay_keeps_its_transparency` | 6.f: Review, compare and its shortcuts |
| 4609 | `TestCompareDifferencesMode.test_the_wipe_moves_the_split_in_the_differences_view` | 6.f: Review, compare and its shortcuts |
| 4617 | `TestCompareDifferencesMode.test_the_two_sides_are_the_same_size_so_the_split_registers` | 6.f: Review, compare and its shortcuts |
| 4626 | `TestCompareDifferencesMode.test_the_wipe_sweeps_the_overlay_across_both_panes` | 6.f: Review, compare and its shortcuts |
| 4662 | `TestCompareDifferencesMode.test_the_slider_is_labelled_for_the_mode` | 6.f: Review, compare and its shortcuts |
| 4671 | `TestCompareDifferencesMode.test_leaving_the_mode_goes_back_to_the_ordinary_wipe` | 6.f: Review, compare and its shortcuts |
| 4680 | `TestCompareDifferencesMode.test_the_overlay_is_built_once_and_reused` | 6.f: Review, compare and its shortcuts |
| 4690 | `TestCompareDifferencesMode.test_the_mode_is_unavailable_without_both_images` | 6.f: Review, compare and its shortcuts |
| 4700 | `TestCompareDifferencesMode.test_the_mode_is_available_once_both_are_loaded` | 6.f: Review, compare and its shortcuts |
| 4751 | `TestBatchCarriesOnWithoutSauceNao.test_it_stops_by_default` | 6.e: search run, pacing, engine alerts, parser health |
| 4755 | `TestBatchCarriesOnWithoutSauceNao.test_with_the_toggle_it_finishes_the_batch` | 6.e: search run, pacing, engine alerts, parser health |
| 4759 | `TestBatchCarriesOnWithoutSauceNao.test_the_later_images_are_searched_without_saucenao` | 6.e: search run, pacing, engine alerts, parser health |
| 4765 | `TestBatchCarriesOnWithoutSauceNao.test_those_results_are_marked_provisional` | 6.e: search run, pacing, engine alerts, parser health |
| 4770 | `TestBatchCarriesOnWithoutSauceNao.test_it_says_so_once_rather_than_per_image` | 6.e: search run, pacing, engine alerts, parser health |
| 4799 | `TestPersistedQuotaPauseShownOnStartup.test_a_fresh_pause_is_shown` | **6.c**: `smoke::persisted_quota_pause` (a fresh pause: banner visible, no dialog, status line) |
| 4816 | `TestPersistedQuotaPauseShownOnStartup.test_a_stale_pause_past_its_reset_time_is_cleared_and_silent` | **6.c**: `smoke::persisted_quota_pause` (a stale pause: banner hidden, file deleted) |
| 4836 | `TestPersistedQuotaPauseShownOnStartup.test_no_persisted_pause_is_the_ordinary_silent_startup` | **6.c**: `smoke::empty_state_and_idle_strip` (no banner and `Ready` on an ordinary start) |
| 4847 | `TestPersistedQuotaPauseShownOnStartup.test_a_persisted_pause_does_not_clobber_the_run_banner` | **6.c**: not asserted with both set: no 6.c scenario starts with a crash and a pause together (the two banners are separate widgets, `run_banner` and `quota_banner`, each shown by its own scenario) |
| 4899 | `TestPauseIsPersistedWhenTheBatchStops.test_pausing_records_the_counts` | 6.e: search run, pacing, engine alerts, parser health |
| 4948 | `TestPausedBannerContinueButton.test_continue_actually_launches_a_search_without_saucenao` | 6.e |
| 5002 | `TestResumeWithoutSaucenaoOverride.test_without_the_override_it_still_pauses` | 6.e: search run, pacing, engine alerts, parser health |
| 5008 | `TestResumeWithoutSaucenaoOverride.test_the_override_finishes_the_batch_despite_the_saved_setting_being_off` | 6.e: search run, pacing, engine alerts, parser health |
| 5012 | `TestResumeWithoutSaucenaoOverride.test_the_saved_setting_is_unchanged_by_the_override` | 6.e: search run, pacing, engine alerts, parser health |
| 5083 | `TestReviewDecisionCardSizing.test_declared_minimum_width_fits_the_button_grid` | 6.f: Review, compare and its shortcuts |
| 5097 | `TestReviewDecisionCardSizing.test_buttons_dont_clip_when_squeezed_to_the_declared_minimum` | 6.f: Review, compare and its shortcuts |
| 5138 | `TestModeStackSizeFloor.test_launches_at_the_requested_1100x650` | **6.c**: `smoke::minimum_size_fits_every_state_in_both_schemes` (Python's 712x515 floor is not carried over: the stated minimum is 1010x790, measured with the rail in (docs/gtk-shell.md)) |
| 5144 | `TestModeStackSizeFloor.test_queue_floor_ignores_activitys_width_and_reviews_height` | **6.c**: `smoke::minimum_size_fits_every_state_in_both_schemes` (the window floor; the Activity page's own growth is 6.e) |
| 5165 | `TestModeStackSizeFloor.test_switching_to_activity_grows_without_clipping` | **6.c**: `smoke::minimum_size_fits_every_state_in_both_schemes` (the window floor; the Activity page's own growth is 6.e) |
| 5213 | `TestReviewShortcuts.test_every_registered_action_has_a_handler` | 6.f: Review, compare and its shortcuts |
| 5222 | `TestReviewShortcuts.test_they_are_scoped_to_the_table_not_the_window` | 6.f: Review, compare and its shortcuts |
| 5233 | `TestReviewShortcuts.test_j_and_k_work_on_the_review_page` | 6.f: Review, compare and its shortcuts |
| 5249 | `TestReviewShortcuts.test_sending_the_current_image_away_keeps_your_place` | 6.f: Review, compare and its shortcuts |
| 5268 | `TestReviewShortcuts.test_sending_the_last_row_away_lands_on_the_new_last_row` | 6.f: Review, compare and its shortcuts |
| 5276 | `TestReviewShortcuts.test_a_hydrus_confirmation_removing_your_row_moves_you_on` | 6.f: Review, compare and its shortcuts |
| 5293 | `TestReviewShortcuts.test_a_hydrus_confirmation_elsewhere_leaves_you_where_you_are` | 6.f: Review, compare and its shortcuts |
| 5304 | `TestReviewShortcuts.test_review_page_buttons_move_between_images` | 6.f: Review, compare and its shortcuts |
| 5319 | `TestReviewShortcuts.test_typing_in_the_filter_box_is_not_swallowed` | 6.f: Review, compare and its shortcuts |
| 5334 | `TestReviewShortcuts.test_j_and_k_step_through_the_list` | 6.f: Review, compare and its shortcuts |
| 5344 | `TestReviewShortcuts.test_stepping_stops_at_the_ends_rather_than_wrapping` | 6.f: Review, compare and its shortcuts |
| 5355 | `TestReviewShortcuts.test_n_skips_to_images_still_needing_a_decision` | 6.f: Review, compare and its shortcuts |
| 5363 | `TestReviewShortcuts.test_n_says_so_rather_than_doing_nothing_at_the_end` | 6.f: Review, compare and its shortcuts |
| 5370 | `TestReviewShortcuts.test_shift_n_searches_backwards` | 6.f: Review, compare and its shortcuts |
| 5376 | `TestReviewShortcuts.test_an_action_needing_one_row_says_so_on_a_multi_selection` | 6.f: Review, compare and its shortcuts |
| 5385 | `TestReviewShortcuts.test_an_action_needing_a_selection_says_so_when_there_is_none` | 6.f: Review, compare and its shortcuts |
| 5393 | `TestReviewShortcuts.test_navigation_works_with_nothing_selected` | 6.f: Review, compare and its shortcuts |
| 5403 | `TestReviewShortcuts.test_rebinding_takes_effect_without_a_restart` | 6.f: Review, compare and its shortcuts |
| 5413 | `TestReviewShortcuts.test_reinstalling_does_not_leave_the_old_binding_behind` | 6.f: Review, compare and its shortcuts |
| 5426 | `TestReviewShortcuts.test_a_cleared_binding_installs_no_shortcut` | 6.f: Review, compare and its shortcuts |
| 5436 | `TestReviewShortcuts.test_an_unparseable_binding_is_skipped_rather_than_fatal` | 6.f: Review, compare and its shortcuts |
| 5467 | `TestDynamicShortcutLabels.test_button_labels_show_effective_bindings` | 6.f: Review, compare and its shortcuts |
| 5475 | `TestDynamicShortcutLabels.test_position_strip_hint_uses_dynamic_bindings` | 6.f: Review, compare and its shortcuts |
| 5489 | `TestDynamicShortcutLabels.test_rebinding_updates_button_labels_and_strip` | 6.f: Review, compare and its shortcuts |
| 5511 | `TestDynamicShortcutLabels.test_shortcuts_dialog_shows_all_actions_with_bindings` | 6.f: Review, compare and its shortcuts |
| 5545 | `TestShowInQueue.test_show_in_queue_from_review_selects_current_row` | 6.f: Review, compare and its shortcuts |
| 5573 | `TestShowInQueue.test_show_in_queue_from_activity_switches_to_queue` | 6.f: Review, compare and its shortcuts |
| 5607 | `TestShortcutsSettingsTab.test_it_offers_every_registered_action` | 6.h: Preferences |
| 5611 | `TestShortcutsSettingsTab.test_only_changed_bindings_are_persisted` | 6.h: Preferences |
| 5619 | `TestShortcutsSettingsTab.test_clearing_a_binding_is_remembered_as_cleared` | 6.h: Preferences |
| 5626 | `TestShortcutsSettingsTab.test_a_duplicate_key_is_named_rather_than_resolved_silently` | 6.h: Preferences |
| 5635 | `TestShortcutsSettingsTab.test_ok_is_refused_while_two_actions_share_a_key` | 6.h: Preferences |
| 5646 | `TestShortcutsSettingsTab.test_ok_goes_through_once_the_clash_is_resolved` | 6.h: Preferences |
| 5654 | `TestShortcutsSettingsTab.test_reset_puts_every_default_back` | 6.h: Preferences |
| 5679 | `TestReviewedColumn.test_the_column_exists_and_is_last` | 6.d: Queue table, filters, context menu, export, tag files |
| 5683 | `TestReviewedColumn.test_only_reviewed_rows_are_marked` | 6.d: Queue table, filters, context menu, export, tag files |
| 5688 | `TestReviewedColumn.test_a_ticked_cell_explains_itself` | 6.d: Queue table, filters, context menu, export, tag files |
| 5693 | `TestReviewedColumn.test_rows_wanting_a_decision_sort_first` | 6.d: Queue table, filters, context menu, export, tag files |
| 5701 | `TestReviewedColumn.test_adding_this_column_invalidates_an_older_saved_layout` | 6.d: Queue table, filters, context menu, export, tag files |
| 5741 | `TestReviewedInTheContextMenu.test_the_entry_marks_an_unreviewed_row` | 6.d: Queue table, filters, context menu, export, tag files |
| 5745 | `TestReviewedInTheContextMenu.test_the_entry_flips_to_unmark_once_marked` | 6.d: Queue table, filters, context menu, export, tag files |
| 5750 | `TestReviewedInTheContextMenu.test_a_mixed_selection_offers_to_mark` | 6.d: Queue table, filters, context menu, export, tag files |
| 5757 | `TestReviewedInTheContextMenu.test_every_menu_entry_still_has_a_handler` | 6.d: Queue table, filters, context menu, export, tag files |
| 5765 | `TestReviewedInTheContextMenu.test_select_by_review_state_counts_each_group` | 6.d: Queue table, filters, context menu, export, tag files |
| 5774 | `TestReviewedInTheContextMenu.test_marking_updates_the_rows_and_the_count` | **6.c**: `smoke::run_strip_and_sent_count_follow_the_list` (the `to review` figure of the sent count (marking itself is 6.f)) |
| 5780 | `TestReviewedInTheContextMenu.test_unmarking_puts_them_back` | 6.d: Queue table, filters, context menu, export, tag files |
| 5785 | `TestReviewedInTheContextMenu.test_the_count_stays_quiet_until_something_is_reviewed` | **6.c**: `smoke::run_strip_and_sent_count_follow_the_list` (the `to review` figure of the sent count (marking itself is 6.f)) |
| 5821 | `TestReviewedFromTheKeyboard.test_space_marks_and_unmarks` | 6.f: Review, compare and its shortcuts |
| 5829 | `TestReviewedFromTheKeyboard.test_n_skips_rows_already_reviewed` | 6.f: Review, compare and its shortcuts |
| 5839 | `TestReviewedFromTheKeyboard.test_marking_then_pressing_n_advances_past_it` | 6.f: Review, compare and its shortcuts |
| 5847 | `TestReviewedFromTheKeyboard.test_n_says_so_when_the_list_is_finished` | 6.f: Review, compare and its shortcuts |
| 5855 | `TestReviewedFromTheKeyboard.test_space_on_a_mixed_selection_marks_rather_than_unmarks` | 6.f: Review, compare and its shortcuts |
| 5863 | `TestReviewedFromTheKeyboard.test_a_pass_resumes_where_it_left_off_after_a_restart` | 6.f: Review's resume point (restore itself is S restore_clean_session) |
| 5897 | `TestReviewedFromTheKeyboard.test_space_needs_a_selection_and_says_so` | 6.f: Review, compare and its shortcuts |
| 5926 | `TestHydrusMenu.test_the_hydrus_menu_exists` | **6.c**: `lib::menu::tests::the_other_menus_are_pythons`, `smoke::window_menus_and_deferred_actions` (the menu is built here; its actions are 6.g's and registered disabled) |
| 5929 | `TestHydrusMenu.test_it_offers_the_three_documented_actions` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5938 | `TestHydrusMenu.test_the_files_menu_still_has_its_recheck_entry` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5945 | `TestHydrusMenu.test_both_menus_share_one_recheck_action` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5955 | `TestHydrusMenu.test_the_recheck_tooltip_comes_from_the_one_constant` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5962 | `TestHydrusMenu.test_hydrus_is_not_buried_behind_help` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5972 | `TestHydrusMenu.test_send_upload_triggers_the_real_handler` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5980 | `TestHydrusMenu.test_send_url_triggers_the_real_handler` | 6.g: Hydrus / Pawchive / auto-import actions |
| 5989 | `TestHydrusMenu.test_recheck_triggers_the_real_handler` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6028 | `TestExportResults.test_csv_lands_on_disk_with_a_row_per_image` | 6.d: Queue table, filters, context menu, export, tag files |
| 6036 | `TestExportResults.test_json_lands_on_disk` | 6.d: Queue table, filters, context menu, export, tag files |
| 6042 | `TestExportResults.test_the_chosen_filter_decides_the_format_not_the_offered_name` | 6.d: Queue table, filters, context menu, export, tag files |
| 6053 | `TestExportResults.test_a_missing_extension_is_added` | 6.d: Queue table, filters, context menu, export, tag files |
| 6058 | `TestExportResults.test_under_all_files_the_typed_extension_decides` | 6.d: Queue table, filters, context menu, export, tag files |
| 6064 | `TestExportResults.test_cancelling_the_dialog_writes_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 6072 | `TestExportResults.test_an_empty_list_says_so_rather_than_writing_a_header_only_file` | 6.d: Queue table, filters, context menu, export, tag files |
| 6082 | `TestExportResults.test_an_unwritable_path_is_reported_not_raised` | 6.d: Queue table, filters, context menu, export, tag files |
| 6090 | `TestExportResults.test_exporting_a_selection_writes_only_those_rows` | 6.d: Queue table, filters, context menu, export, tag files |
| 6098 | `TestExportResults.test_the_status_bar_says_what_was_written` | 6.d: Queue table, filters, context menu, export, tag files |
| 6103 | `TestExportResults.test_the_files_menu_offers_it` | 6.d: Queue table, filters, context menu, export, tag files |
| 6111 | `TestExportResults.test_the_right_click_menu_offers_the_selection` | 6.d: Queue table, filters, context menu, export, tag files |
| 6186 | `TestWriteTagFiles.test_it_writes_one_file_per_image_beside_the_image` | 6.d: Queue table, filters, context menu, export, tag files |
| 6193 | `TestWriteTagFiles.test_it_asks_before_writing_anything` | 6.d: Queue table, filters, context menu, export, tag files |
| 6199 | `TestWriteTagFiles.test_answering_no_writes_nothing` | 6.d: Queue table, filters, context menu, export, tag files |
| 6203 | `TestWriteTagFiles.test_the_prompt_says_how_many_where_and_under_what_name` | 6.d: Queue table, filters, context menu, export, tag files |
| 6210 | `TestWriteTagFiles.test_an_existing_file_is_kept_when_that_is_the_answer` | 6.d: Queue table, filters, context menu, export, tag files |
| 6225 | `TestWriteTagFiles.test_an_existing_file_is_replaced_when_that_is_the_answer` | 6.d: Queue table, filters, context menu, export, tag files |
| 6235 | `TestWriteTagFiles.test_cancelling_the_existing_files_prompt_writes_nothing_at_all` | 6.d: Queue table, filters, context menu, export, tag files |
| 6247 | `TestWriteTagFiles.test_the_skip_setting_asks_once_and_keeps_existing_files` | 6.d: Queue table, filters, context menu, export, tag files |
| 6261 | `TestWriteTagFiles.test_the_overwrite_setting_still_says_so_before_replacing` | 6.d: Queue table, filters, context menu, export, tag files |
| 6273 | `TestWriteTagFiles.test_the_naming_style_setting_is_honoured` | 6.d: Queue table, filters, context menu, export, tag files |
| 6279 | `TestWriteTagFiles.test_images_with_no_tags_are_reported_rather_than_written_empty` | 6.d: Queue table, filters, context menu, export, tag files |
| 6291 | `TestWriteTagFiles.test_a_missing_file_gets_no_sidecar_and_is_counted` | 6.d: Queue table, filters, context menu, export, tag files |
| 6298 | `TestWriteTagFiles.test_an_empty_list_says_so_without_prompting` | 6.d: Queue table, filters, context menu, export, tag files |
| 6308 | `TestWriteTagFiles.test_an_unwritable_target_is_reported_not_raised` | 6.d: Queue table, filters, context menu, export, tag files |
| 6316 | `TestWriteTagFiles.test_writing_a_selection_touches_only_those_images` | 6.d: Queue table, filters, context menu, export, tag files |
| 6320 | `TestWriteTagFiles.test_the_status_bar_says_what_was_written` | 6.d: Queue table, filters, context menu, export, tag files |
| 6324 | `TestWriteTagFiles.test_the_files_menu_offers_it` | 6.d: Queue table, filters, context menu, export, tag files |
| 6332 | `TestWriteTagFiles.test_the_right_click_menu_offers_the_selection` | 6.d: Queue table, filters, context menu, export, tag files |
| 6378 | `TestShowInHydrus.test_it_shows_the_selected_files` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6388 | `TestShowInHydrus.test_one_file_gets_its_own_name_on_the_page` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6394 | `TestShowInHydrus.test_files_hydrus_does_not_have_are_left_out_and_counted` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6404 | `TestShowInHydrus.test_nothing_in_hydrus_says_so_instead_of_opening_an_empty_page` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6413 | `TestShowInHydrus.test_rows_with_no_hash_yet_are_explained` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6423 | `TestShowInHydrus.test_no_access_key_is_reported_before_any_request` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6432 | `TestShowInHydrus.test_a_403_names_the_permission_that_is_missing` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6445 | `TestShowInHydrus.test_a_404_says_the_client_is_too_old` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6454 | `TestShowInHydrus.test_any_other_failure_is_reported_not_raised` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6462 | `TestShowInHydrus.test_an_unreachable_hydrus_is_reported` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6472 | `TestShowInHydrus.test_the_menu_offers_it_and_wires_it` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6489 | `TestShowInHydrus.test_h_shows_the_selection` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6504 | `TestShowInHydrus.test_h_with_nothing_selected_says_so_rather_than_acting` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6518 | `TestShowInHydrus.test_the_menu_greys_it_out_when_no_row_has_a_hash` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6542 | `TestSideBySideIsSharp.test_it_fills_the_box_at_device_pixels` | 6.f: Review, compare and its shortcuts |
| 6552 | `TestSideBySideIsSharp.test_it_redraws_from_the_source_when_the_box_grows` | 6.f: Review, compare and its shortcuts |
| 6564 | `TestSideBySideIsSharp.test_a_large_picture_does_not_hold_the_box_open` | 6.f: Review, compare and its shortcuts |
| 6571 | `TestSideBySideIsSharp.test_the_local_picture_is_decoded_past_the_old_700px` | 6.f: Review, compare and its shortcuts |
| 6608 | `TestSideBySideReusesTheFullMatch.test_a_redraw_keeps_the_full_resolution_match` | 6.f |
| 6613 | `TestSideBySideReusesTheFullMatch.test_another_candidate_does_not_borrow_it` | 6.f |
| 6652 | `TestPawchiveIndexWindow.test_a_pasted_link_adds_its_artist` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6658 | `TestPawchiveIndexWindow.test_artists_with_pawchive_matches_can_be_added_in_one_go` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6663 | `TestPawchiveIndexWindow.test_removing_asks_first_and_defaults_to_no` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6677 | `TestPawchiveIndexWindow.test_name_search_results_are_ticked_then_added` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6686 | `TestPawchiveIndexWindow.test_the_menu_opens_one_window_and_quitting_closes_it` | 6.g: Hydrus / Pawchive / auto-import actions |
| 6711 | `TestRemovalKeepsYourPlace.test_an_auto_import_removing_the_selected_row_lands_on_the_next` | 6.d/6.f |
| 6718 | `TestRemovalKeepsYourPlace.test_removing_another_row_leaves_the_selection_alone` | 6.d/6.f |
| 6723 | `TestRemovalKeepsYourPlace.test_the_remove_key_lands_on_the_next_row_too` | 6.d/6.f |
| 6728 | `TestRemovalKeepsYourPlace.test_removing_the_last_row_lands_on_the_one_above` | 6.d/6.f |
| 6748 | `TestFinishedTestsReleaseTheirWidgets.test_a_gui_test_case_does_not_leave_a_queued_delete_pending` | n/a: Qt test hygiene / the parked-thread registry; every Rust fixture builds its window on its own temp profile and there is no parked-thread list |
| 6757 | `TestFinishedTestsReleaseTheirWidgets.test_a_themed_window_does_not_outlive_its_test` | n/a: Qt test hygiene / the parked-thread registry; every Rust fixture builds its window on its own temp profile and there is no parked-thread list |

### `tests/test_interrupted_banner.py` (41)

| Line | Test | Disposition |
|---|---|---|
| 55 | `TestPreviousRunStoppedAt.test_it_is_the_last_line_before_this_runs_start` | **6.c**: `lib::startup::tests::the_stop_time_is_the_last_stamped_line_before_this_runs_start` (its fixture has a continuation line) |
| 64 | `TestPreviousRunStoppedAt.test_continuation_lines_do_not_count` | **6.c**: `lib::startup::tests::the_stop_time_is_the_last_stamped_line_before_this_runs_start` (its fixture has a continuation line) |
| 73 | `TestPreviousRunStoppedAt.test_a_first_ever_run_has_no_previous_run` | **6.c**: `lib::startup::tests::no_run_start_or_no_earlier_stamp_is_unknown` (empty log, no start line, only this run's start line) |
| 76 | `TestPreviousRunStoppedAt.test_a_log_with_no_start_line_or_no_file_is_unknown` | **6.c**: `lib::startup::tests::no_run_start_or_no_earlier_stamp_is_unknown` (empty log, no start line, only this run's start line) |
| 93 | `TestLastCleanExit.test_a_clean_shutdown_records_when` | **6.c**: `smoke::close_saves_and_marks_a_clean_exit` asserts the clean-exit file is written at close (the marker itself is `hatate_core::crash_marker`, merged earlier) |
| 98 | `TestLastCleanExit.test_a_crash_does_not_overwrite_it` | core: `hatate_core::crash_marker` (merged earlier); 6.c adds no test that a crash leaves the clean-exit stamp alone |
| 103 | `TestLastCleanExit.test_none_ever_recorded_is_unknown_not_guessed` | core: `hatate_core::crash_marker` (merged earlier); the window leaves the `Last clean exit` item out when none is recorded, which only the action differential compares (`run_banner.meta`) |
| 153 | `TestFrameAndKicker.test_the_banner_wears_the_four_corner_brackets` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 162 | `TestFrameAndKicker.test_the_kicker_reads_run_interrupted_partial_results_survived` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 172 | `TestFrameAndKicker.test_the_kicker_is_the_loudest_ink_and_clears_contrast` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 185 | `TestProse.test_it_reassures_and_carries_the_counts` | **6.c**: `smoke::restore_after_a_crash_shows_the_banner` asserts the whole body text literally (the reassurance and the counts), and `lib::text::tests::the_interrupted_prose_leaves_out_a_zero_queued_count` |
| 196 | `TestProse.test_the_wrapped_prose_is_not_clipped` | **6.c**: `smoke::minimum_size_fits_every_state_in_both_schemes` (the `queue-crashed-banner` state fits at the stated minimum in both schemes) |
| 214 | `TestProse.test_counts_by_category` | **6.c**: `lib::text::tests::the_interrupted_prose_leaves_out_a_zero_queued_count` (sent / still queued / waiting / never reached) and the literal body in `smoke::restore_after_a_crash_shows_the_banner` |
| 233 | `TestProse.test_nothing_queued_is_not_announced` | **6.c**: `lib::text::tests::the_interrupted_prose_leaves_out_a_zero_queued_count` |
| 245 | `TestMetaLine.test_it_reads_autosave_clean_exit_and_crash_log` | **6.c**: `lib::banners::tests::the_crash_log_figure_is_a_count_or_none` (the crash-log figure); the assembled meta line (`Last autosave`, `Last clean exit`, `Crash log`) is compared with Python's by the action differential (`run_banner.meta`), and no Rust test in the suite asserts it |
| 254 | `TestMetaLine.test_it_paints_as_the_mockups_uppercase_mono_line` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 260 | `TestMetaLine.test_it_sits_flush_left_under_the_prose` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 268 | `TestMetaLine.test_an_unknown_clean_exit_or_autosave_is_left_out_not_invented` | **6.c**: the action differential (section 5) compares `run_banner.meta` with Python's after every step; no Rust test in the suite asserts it (an unknown item is left out) |
| 272 | `TestMetaLine.test_two_faults_pluralise` | **6.c**: `lib::banners::tests::the_crash_log_figure_is_a_count_or_none` (`no new entry` / `1 new entry` / `3 new entries`) and `lib::startup::tests::faults_are_counted_in_the_section_before_the_last` |
| 281 | `TestActionsUnchanged.test_resume_is_the_one_primary_and_view_crash_log_is_not` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 287 | `TestActionsUnchanged.test_discard_is_still_a_link_not_a_button` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 294 | `TestActionsUnchanged.test_discard_is_uppercase_mono_tracked_and_still_underlined` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 306 | `TestActionsUnchanged.test_the_glyph_has_its_own_column_and_the_prose_starts_66px_in` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 319 | `TestActionsUnchanged.test_the_actions_sit_below_the_prose_in_one_row_discard_last` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 338 | `TestActionsUnchanged.test_discard_is_not_louder_than_it_was` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 362 | `TestRowMarker.test_the_restore_flags_the_row_that_was_in_flight_and_resets_it` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 370 | `TestRowMarker.test_a_clean_restore_leaves_a_stale_row_alone` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 374 | `TestRowMarker.test_only_that_rows_engine_cell_says_so_in_italic` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 389 | `TestRowMarker.test_the_marker_is_dim_but_clears_the_text_floor` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 399 | `TestRowMarker.test_the_marker_breaks_at_the_space_and_fits_the_row_in_the_default_column` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 424 | `TestRowMarker.test_it_goes_once_the_row_has_a_result_or_is_searched_again` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 431 | `TestRowMarker.test_the_flag_is_not_saved` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 436 | `TestRowMarker.test_flagging_does_not_make_an_entry_look_changed` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 442 | `TestRowMarker.test_the_search_worker_spends_the_flag` | 6.d: the per-row `interrupted` marker in the Engine column (the flag itself is core: mark_interrupted_mid_search, merged) |
| 451 | `TestStripNote.test_the_strip_says_how_long_ago_it_stopped` | **6.c**: `smoke::restore_after_a_crash_shows_the_banner` asserts only that the strip note starts `stopped `; the figure comes from `lib::startup::tests::the_stop_time_is_the_last_stamped_line_before_this_runs_start` and `lib::text::tests::durations_round_to_even_and_pad_on_request`, and the action differential compares `run_note` |
| 457 | `TestStripNote.test_the_figure_is_the_bold_part` | **6.c**: judged in the shot set, not in a test (the `crashed` shots, dark and light; contrast in section 8) |
| 462 | `TestStripNote.test_with_no_log_to_read_the_autosave_is_the_fallback` | **6.c**: the action differential (section 5) compares `run_note` with Python's after every step; no Rust test in the suite asserts it (the autosave fallback) |
| 466 | `TestStripNote.test_nothing_known_says_nothing` | **6.c**: the action differential (section 5) compares `run_note` with Python's after every step; no Rust test in the suite asserts it |
| 470 | `TestStripNote.test_a_clean_restore_has_no_note` | **6.c**: the action differential (section 5) compares `run_note` with Python's after every step; no Rust test in the suite asserts it (`smoke::restore_clean_session` asserts only that the run banner is hidden) |
| 474 | `TestStripNote.test_discarding_the_session_clears_it` | **6.c**: `smoke::nothing_survived_variant` asserts the note clears when its New-search button is pressed; the Discard link's own clear is compared only by the action differential (`run_note`) |
| 483 | `TestStripNote.test_starting_a_search_clears_it` | 6.e: starting a search is 6.e's |

### `tests/test_log_viewer_dialog.py` (5)

| Line | Test | Disposition |
|---|---|---|
| 47 | `TestCrashLogToggle.test_starts_on_the_app_log` | **6.c**: `smoke::log_viewer_and_open_log` asserts the title, the `Log file: ` path and the empty-log text on open, and `lib::logview::tests::an_empty_log_says_so_in_pythons_words` |
| 51 | `TestCrashLogToggle.test_missing_crash_log_says_so_instead_of_showing_app_log_content` | **6.c**: `smoke::log_viewer_and_open_log` asserts the crash view reads `(no crash log yet - nothing has crashed)` |
| 55 | `TestCrashLogToggle.test_toggle_shows_crash_log_contents` | **6.c**: `lib::logview::tests::a_log_shows_its_text` (a log's contents); `smoke::log_viewer_and_open_log` toggles to the crash view with an absent crash log only |
| 62 | `TestCrashLogToggle.test_toggle_back_returns_to_the_app_log` | **6.c**: not asserted: `smoke::log_viewer_and_open_log` toggles to the crash log and reads the button as `View App Log`, but never toggles back |
| 68 | `TestCrashLogToggle.test_show_crash_log_opens_preswitched` | **6.c**: `smoke::log_viewer_and_open_log` (`open_crash_log` then `showing_crash_log()`) |

### `tests/test_mcp_settings_tab.py` (29)

| Line | Test | Disposition |
|---|---|---|
| 65 | `TestConstructionWithNoMainWindow.test_builds_without_a_parent` | 6.h: Settings > MCP tab |
| 70 | `TestConstructionWithNoMainWindow.test_enabled_defaults_off_and_hydrus_writes_defaults_on` | 6.h: Settings > MCP tab |
| 91 | `TestApplyToSettings.test_round_trips_every_mcp_field` | 6.h: Settings > MCP tab |
| 120 | `TestEnableSummaryConstructionSafety.test_does_not_fire_when_restoring_a_saved_enabled_state` | 6.h: Settings > MCP tab |
| 132 | `TestEnableSummaryConstructionSafety.test_fires_exactly_once_on_a_real_off_to_on_click` | 6.h: Settings > MCP tab |
| 156 | `TestConnectionStates.test_state_1_stopped_when_not_enabled` | 6.h: Settings > MCP tab |
| 162 | `TestConnectionStates.test_state_2_listening_when_running` | 6.h: Settings > MCP tab |
| 167 | `TestConnectionStates.test_state_3_mcp_package_not_installed` | 6.h: Settings > MCP tab |
| 174 | `TestConnectionStates.test_state_4_no_token` | 6.h: Settings > MCP tab |
| 179 | `TestConnectionStates.test_state_5_port_busy` | 6.h: Settings > MCP tab |
| 186 | `TestConnectionStates.test_state_6_honest_fallback_shows_open_log_button` | 6.h: Settings > MCP tab |
| 197 | `TestRestartServer.test_restart_uses_the_typed_port_and_token_not_the_saved_ones` | 6.h: Settings > MCP tab |
| 226 | `TestAuditOutcomeFormatting.test_refused_call` | 6.h: Settings > MCP tab |
| 232 | `TestAuditOutcomeFormatting.test_dry_run` | 6.h: Settings > MCP tab |
| 239 | `TestAuditOutcomeFormatting.test_successful_hydrus_send_gets_the_trailing_glyph` | 6.h: Settings > MCP tab |
| 247 | `TestAuditOutcomeFormatting.test_refused_hydrus_send_has_no_glyph` | 6.h: Settings > MCP tab |
| 252 | `TestAuditOutcomeFormatting.test_select_candidate` | 6.h: Settings > MCP tab |
| 258 | `TestAuditOutcomeFormatting.test_toggle_reviewed` | 6.h: Settings > MCP tab |
| 264 | `TestAuditOutcomeFormatting.test_remove_row_and_reset_result` | 6.h: Settings > MCP tab |
| 276 | `TestTargetCell.test_row_and_candidate` | 6.h: Settings > MCP tab |
| 279 | `TestTargetCell.test_row_only` | 6.h: Settings > MCP tab |
| 282 | `TestTargetCell.test_no_row_is_an_em_dash` | 6.h: Settings > MCP tab |
| 287 | `TestTimeCell.test_a_past_date_shows_the_month_and_day` | 6.h: Settings > MCP tab |
| 290 | `TestTimeCell.test_an_unparseable_timestamp_is_shown_verbatim_not_raised` | 6.h: Settings > MCP tab |
| 305 | `TestReadAuditLog.test_missing_file_is_empty_not_an_error` | 6.h: Settings > MCP tab |
| 308 | `TestReadAuditLog.test_a_truncated_final_line_is_dropped_not_fatal` | 6.h: Settings > MCP tab |
| 341 | `TestAuditTableFiltering.test_actions_filter_hides_reads_by_default` | 6.h: Settings > MCP tab |
| 347 | `TestAuditTableFiltering.test_all_calls_filter_shows_everything` | 6.h: Settings > MCP tab |
| 352 | `TestAuditTableFiltering.test_newest_first` | 6.h: Settings > MCP tab |

### `tests/test_quota_pause_banner.py` (25)

| Line | Test | Disposition |
|---|---|---|
| 96 | `TestNoModal.test_a_quota_pause_raises_no_modal` | **6.c**: `smoke::persisted_quota_pause` asserts the restore path only (a pause on disk at start: banner visible, `count_shown() == 0`); the live `_on_paused_out_of_quota` handler Python's test drives is 6.e's |
| 116 | `TestNoModal.test_a_pause_with_nothing_recorded_still_raises_no_modal` | 6.e: the live `_on_paused_out_of_quota` handler with nothing recorded, which expects the banner visible; `smoke::persisted_quota_pause` covers the restore path only (a stale pause: no banner, file deleted) and does not assert it |
| 124 | `TestBannerVariant.test_the_kicker_reads_run_paused_quota_exhausted` | **6.c**: `smoke::persisted_quota_pause` asserts the banner is visible and its body text; the kicker wording is `widgets::quota_pause_banner` and is read in the shot set (`quota` shot), not asserted |
| 132 | `TestBannerVariant.test_the_kicker_clears_text_contrast` | **6.c**: judged in the shot set (contrast table, section 8), not in a test |
| 143 | `TestBannerVariant.test_it_wears_the_same_brackets_as_the_interrupted_banner` | **6.c**: judged in the shot set, not in a test (brackets) |
| 150 | `TestBannerVariant.test_the_prose_carries_the_counts` | **6.c**: `lib::text::tests::the_quota_prose_counts_or_falls_back_to_the_workers_figure`, `lib::text::tests::a_pause_is_described_with_its_reset_time_and_distance`; `smoke::persisted_quota_pause` asserts the body contains `Nothing was lost.` and the status line carries the described pause |
| 160 | `TestBannerVariant.test_the_prose_is_not_clipped_at_any_width` | **6.c**: `smoke::minimum_size_fits_every_state_in_both_schemes` (the quota banner fits at the stated minimum in both schemes) |
| 175 | `TestBannerVariant.test_the_prose_is_drawn_at_the_mockups_20px_leading` | **6.c**: judged in the shot set, not in a test (leading) |
| 187 | `TestBannerVariant.test_the_banner_keeps_the_mockups_vertical_rhythm` | **6.c**: judged in the shot set, not in a test (vertical rhythm) |
| 196 | `TestBannerVariant.test_the_run_strip_puts_the_pause_note_after_the_counts` | **6.c**: `smoke::persisted_quota_pause` asserts the strip's note is literally `paused — quota exhausted`, its estimate `ETA paused` and its count `nothing queued` |
| 206 | `TestBannerVariant.test_the_meta_line_gives_the_time_the_count_and_the_reset` | **6.c**: the meta line is built by `banners::show_persisted_quota_pause` (`Quota exhausted at`, `Images waiting`, `Resets`); the text pieces are `lib::text` unit tests, the line itself is read in the shot set, not asserted |
| 220 | `TestBannerVariant.test_without_a_list_the_prose_falls_back_to_the_workers_count` | **6.c**: `lib::text::tests::the_quota_prose_counts_or_falls_back_to_the_workers_figure` |
| 228 | `TestActions.test_review_is_the_primary_and_continue_is_the_secondary` | 6.e: Review / Continue buttons' handlers and their visibility; the action differential compares `quota_banner.review` and `review_visible` on the restore path |
| 241 | `TestActions.test_review_opens_review_on_the_first_waiting_row` | 6.e: Review / Continue buttons' handlers and their visibility; the action differential compares `quota_banner.review` and `review_visible` on the restore path |
| 248 | `TestActions.test_review_is_hidden_when_nothing_is_waiting` | 6.e: Review / Continue buttons' handlers and their visibility; the action differential compares `quota_banner.review` and `review_visible` on the restore path |
| 254 | `TestActions.test_continue_launches_without_saucenao_and_clears_the_banner` | 6.e: Review / Continue buttons' handlers and their visibility; the action differential compares `quota_banner.review` and `review_visible` on the restore path |
| 270 | `TestActions.test_clearing_the_list_retires_the_banner` | 6.e: Review / Continue buttons' handlers and their visibility; the action differential compares `quota_banner.review` and `review_visible` on the restore path |
| 281 | `TestPersistedAcrossRestart.test_a_pause_on_disk_shows_the_banner_not_just_a_status_line` | **6.c**: `smoke::persisted_quota_pause` (banner visible, and the status line joined with a middle dot) |
| 291 | `TestPersistedAcrossRestart.test_a_stale_pause_shows_no_banner` | **6.c**: `smoke::persisted_quota_pause` (banner hidden, the stale file deleted); the strip note after a stale pause is not asserted |
| 297 | `TestPersistedAcrossRestart.test_no_pause_shows_no_banner` | **6.c**: `smoke::empty_state_and_idle_strip` (no quota banner on an ordinary start) |
| 301 | `TestPersistedAcrossRestart.test_the_banner_sits_below_a_crash_recovery_banner_when_both_are_true` | **6.c**: not asserted with both set: `smoke::restore_after_a_crash_shows_the_banner` and `smoke::persisted_quota_pause` each show their own banner, no scenario starts with both, and the order is not asserted |
| 319 | `TestStrip.test_the_strip_reads_paused_the_count_and_eta_paused` | **6.c**: `smoke::persisted_quota_pause` asserts, on the restore path, the note `paused — quota exhausted`, `ETA paused` and the count `nothing queued`, as literal text; the live path through `_refresh_saucenao_quota_label` is 6.e's |
| 327 | `TestStrip.test_the_run_ending_does_not_wipe_the_pause_from_the_strip` | 6.e: the run ending is 6.e's; `smoke::persisted_quota_pause` asserts the other half, that dismissing the pause clears the note and the estimate |
| 335 | `TestStrip.test_after_a_relaunch_the_strip_still_says_paused` | **6.c**: `smoke::persisted_quota_pause` (the strip is read after a start with the pause on disk) |
| 340 | `TestStrip.test_an_ordinary_idle_strip_is_unchanged` | **6.c**: `smoke::empty_state_and_idle_strip` compares the strip with `IDLE_PROGRESS` (`nothing queued`) and `IDLE_ETA` (`ETA —`) as constants; the note label is not asserted there, and the action differential compares `run_note` and `run_progress` |

### `tests/test_review_commit_caption.py` (4)

| Line | Test | Disposition |
|---|---|---|
| 51 | `TestSendCaption.test_caption_text_and_position_beneath_send` | 6.f: Review view |
| 62 | `TestSendCaption.test_send_is_unchanged` | 6.f: Review view |
| 87 | `TestSendCaption.test_contrast_dark` | 6.f: Review view |
| 90 | `TestSendCaption.test_contrast_light` | 6.f: Review view |

### `tests/test_review_frames.py` (22)

| Line | Test | Disposition |
|---|---|---|
| 56 | `TestCaptionText.test_local_meta_is_the_mockups_line` | 6.f: Review view |
| 65 | `TestCaptionText.test_local_meta_is_empty_for_an_unreadable_file` | 6.f: Review view |
| 68 | `TestCaptionText.test_a_measured_match_is_named_by_its_site_alone` | 6.f: Review view |
| 72 | `TestCaptionText.test_an_unmeasured_score_keeps_its_qualifier` | 6.f: Review view |
| 77 | `TestCaptionText.test_a_match_with_no_site_and_no_score_is_just_match` | 6.f: Review view |
| 80 | `TestCaptionText.test_match_meta_uses_the_caption_separators` | 6.f: Review view |
| 95 | `TestFramesAndCaptions.test_the_two_frames_are_equal_and_carry_four_bracket_corners` | 6.f: Review view |
| 114 | `TestFramesAndCaptions.test_the_caption_sits_below_its_frame_and_nothing_sits_above` | 6.f: Review view |
| 127 | `TestFramesAndCaptions.test_captions_are_mono_caps_and_say_what_the_mockup_says` | 6.f: Review view |
| 147 | `TestFramesAndCaptions.test_an_unmeasured_match_keeps_its_ranking_qualifier_in_the_caption` | 6.f: Review view |
| 157 | `TestFramesAndCaptions.test_the_fixture_window_starts_no_network_fetch` | 6.f: Review view |
| 165 | `TestFramesAndCaptions.test_no_match_says_so_in_the_caption` | 6.f: Review view |
| 170 | `TestFramesAndCaptions.test_a_long_caption_elides_instead_of_widening_the_window` | 6.f: Review view |
| 178 | `TestFramesAndCaptions.test_a_parsers_reason_gets_its_own_line_and_only_when_there_is_one` | 6.f: Review view |
| 191 | `TestFramesAndCaptions.test_the_wipe_view_is_framed_and_its_zoom_floats_inside_the_frame` | 6.f: Review view |
| 218 | `TestWipeChrome.test_the_slider_handle_is_a_24px_target_with_a_hairline_track` | 6.f: Review view |
| 239 | `TestWipeChrome.test_local_and_match_name_the_ends_of_the_slider` | 6.f: Review view |
| 252 | `TestWipeChrome.test_the_zoom_row_is_boxed_mono_buttons` | 6.f: Review view |
| 265 | `TestWipeChrome.test_wipe_carries_the_one_line_hint_and_a_warning_replaces_it` | 6.f: Review view |
| 278 | `TestWipeChrome.test_the_wipe_chrome_stays_out_of_side_by_side` | 6.f: Review view |
| 289 | `TestViewportBudget.test_the_picture_gets_at_least_55_percent_of_the_window_height` | 6.f: Review view |
| 301 | `TestViewportBudget.test_the_footer_still_fits_and_sits_where_the_page_ends` | 6.f: Review view |

### `tests/test_review_keycaps.py` (18)

| Line | Test | Disposition |
|---|---|---|
| 109 | `TestKeyCapWidget.test_text_is_the_key_as_given_and_the_capitals_are_the_fonts` | 6.f: Review view |
| 116 | `TestKeyCapWidget.test_a_cap_keeps_its_own_height_inside_a_taller_row` | 6.f: Review view |
| 128 | `TestKeyCapWidget.test_keyed_button_keeps_its_label_as_text_and_exposes_the_cap` | 6.f: Review view |
| 134 | `TestKeyCapWidget.test_an_empty_key_hides_the_cap_and_gives_the_room_back` | 6.f: Review view |
| 146 | `TestKeysInsideTheButtons.test_every_button_that_has_a_key_shows_it_as_a_cap` | 6.f: Review view |
| 156 | `TestKeysInsideTheButtons.test_each_cap_sits_inside_its_button_clear_of_the_label` | 6.f: Review view |
| 174 | `TestKeysInsideTheButtons.test_key_names_read_against_the_surface_they_sit_on` | 6.f: Review view |
| 195 | `TestKeysInsideTheButtons.test_the_mark_label_flip_still_fits_at_the_cards_declared_minimum` | 6.f: Review view |
| 214 | `TestKeysInsideTheButtons.test_rebinding_a_key_changes_the_cap_and_unbinding_hides_it` | 6.f: Review view |
| 227 | `TestMatchStepperKeys.test_bracket_keys_follow_their_steppers_and_dim_with_them` | 6.f: Review view |
| 240 | `TestMatchStepperKeys.test_a_cap_sits_immediately_after_its_stepper` | 6.f: Review view |
| 250 | `TestHintBar.test_default_bar_reads_like_the_mockups` | 6.f: Review view |
| 260 | `TestHintBar.test_the_figures_are_strong_and_the_words_are_not` | 6.f: Review view |
| 266 | `TestHintBar.test_the_bar_carries_the_mockups_1px_rule_across_the_page` | 6.f: Review view |
| 281 | `TestHintBar.test_the_bar_is_centred_between_the_previous_and_next_buttons` | 6.f: Review view |
| 291 | `TestHintBar.test_a_narrow_bar_drops_its_tail_and_keeps_the_position` | 6.f: Review view |
| 313 | `TestHintBar.test_a_rebound_key_is_what_the_bar_shows` | 6.f: Review view |
| 322 | `TestHintBar.test_the_bar_updates_as_the_selection_moves` | 6.f: Review view |

### `tests/test_review_readout.py` (10)

| Line | Test | Disposition |
|---|---|---|
| 46 | `TestReadoutParts.test_a_measured_score_reads_similarity_with_a_good_glyph` | 6.f: Review view |
| 53 | `TestReadoutParts.test_a_ranking_never_says_similarity` | 6.f: Review view |
| 60 | `TestReadoutParts.test_a_smaller_match_draws_no_up_arrow` | 6.f: Review view |
| 65 | `TestReadoutParts.test_no_candidate_says_nothing` | 6.f: Review view |
| 70 | `TestReadoutParts.test_the_tooltip_keeps_what_the_banner_said` | 6.f: Review view |
| 113 | `TestReadoutInTheHeader.test_the_readout_is_28px_and_shows_every_part` | 6.f: Review view |
| 124 | `TestReadoutInTheHeader.test_a_ranking_row_is_marked_in_the_window` | 6.f: Review view |
| 129 | `TestReadoutInTheHeader.test_the_banner_row_is_gone` | 6.f: Review view |
| 133 | `TestReadoutInTheHeader.test_the_readout_fits_the_compare_column` | 6.f: Review view |
| 153 | `TestReadoutInTheHeader.test_zoom_floats_inside_the_frame_not_in_the_header` | 6.f: Review view |

### `tests/test_review_tag_rail.py` (7)

| Line | Test | Disposition |
|---|---|---|
| 80 | `TestReviewTagRail.test_match_label_sits_in_a_hairline_box_with_the_steppers` | 6.f: Review view |
| 92 | `TestReviewTagRail.test_steppers_are_36px_boxed_triangles_with_the_bracket_keys` | 6.f: Review view |
| 103 | `TestReviewTagRail.test_steppers_walk_the_candidates_and_disable_at_the_ends` | 6.f: Review view |
| 116 | `TestReviewTagRail.test_no_candidates_means_no_steppers_and_the_picker_stays_below` | 6.f: Review view |
| 127 | `TestReviewTagRail.test_rows_are_32px_and_at_least_five_full_rows_show_at_1440x900` | 6.f: Review view |
| 141 | `TestReviewTagRail.test_the_marker_is_dimmed_the_tag_is_bright_and_each_row_is_ruled` | 6.f: Review view |
| 165 | `TestReviewTagRail.test_a_tag_without_a_marker_draws_as_one_bright_run` | 6.f: Review view |

### `tests/test_run_banner.py` (8)

| Line | Test | Disposition |
|---|---|---|
| 44 | `TestRunBanner.test_hidden_on_clean_restore` | **6.c**: `smoke::restore_clean_session` (the run banner stays hidden) |
| 56 | `TestRunBanner.test_hidden_when_nothing_survived` | **6.c**: `smoke::nothing_survived_variant` (the run banner stays hidden) |
| 62 | `TestRunBanner.test_shown_on_unclean_restore_with_entries` | **6.c**: `smoke::restore_after_a_crash_shows_the_banner` |
| 69 | `TestRunBanner.test_body_counts_per_category` | **6.c**: `smoke::restore_after_a_crash_shows_the_banner` (the body text, literally) |
| 89 | `TestRunBanner.test_resume_button_routes_to_action_start_search` | 6.e: Resume starts a search |
| 105 | `TestRunBanner.test_crash_log_button_routes_to_open_crash_log` | **6.c**: not asserted: `smoke::log_viewer_and_open_log` calls `open_crash_log` directly; clicking the banner's button is not exercised |
| 116 | `TestRunBanner.test_crash_log_action_opens_dialog_preswitched` | **6.c**: `smoke::log_viewer_and_open_log` (`open_crash_log` opens the viewer on the crash log) |
| 124 | `TestRunBanner.test_discard_label_routes_to_action_clear_session` | **6.c**: not asserted: `smoke::clear_session_matches_the_empty_list_quirk` drives the `clear-session` action; clicking the Discard link is not exercised |

### `tests/test_sent_tag_filter.py` (23)

| Line | Test | Disposition |
|---|---|---|
| 70 | `TestTheHelper.test_no_settings_at_all_means_no_filtering` | 6.d: sent-tag filter |
| 75 | `TestTheHelper.test_off_sends_everything_even_when_sources_are_unticked` | 6.d: sent-tag filter |
| 79 | `TestTheHelper.test_on_honours_the_ticked_sources` | 6.d: sent-tag filter |
| 83 | `TestTheHelper.test_on_with_nothing_ticked_sends_nothing` | 6.d: sent-tag filter |
| 89 | `TestTheHelper.test_a_stale_source_name_is_ignored_not_fatal` | 6.d: sent-tag filter |
| 95 | `TestTheHelper.test_the_returned_list_is_not_the_entrys_own` | 6.d: sent-tag filter |
| 156 | `TestTheThreeSends.test_off_by_default_every_send_still_carries_every_tag` | 6.d: sent-tag filter |
| 162 | `TestTheThreeSends.test_off_ignores_unticked_sources` | 6.d: sent-tag filter |
| 171 | `TestTheThreeSends.test_on_every_send_drops_the_unticked_sources` | 6.d: sent-tag filter |
| 179 | `TestTheThreeSends.test_no_settings_argument_still_sends_everything` | 6.d: sent-tag filter |
| 197 | `TestTheExport.test_off_by_default_the_export_carries_every_tag` | 6.d: sent-tag filter |
| 204 | `TestTheExport.test_on_the_export_drops_the_unticked_sources` | 6.d: sent-tag filter |
| 214 | `TestTheExport.test_tag_count_matches_the_tags_actually_written` | 6.d: sent-tag filter |
| 229 | `TestTheExport.test_render_passes_the_settings_through` | 6.d: sent-tag filter |
| 237 | `TestTheExport.test_the_column_order_is_unchanged` | 6.d: sent-tag filter |
| 260 | `TestTheMigration.test_a_v12_config_with_sources_unticked_does_not_start_filtering` | 6.d: sent-tag filter |
| 270 | `TestTheMigration.test_a_pre_schema_config_is_treated_the_same` | 6.d: sent-tag filter |
| 275 | `TestTheMigration.test_an_explicit_opt_in_survives_a_reload` | 6.d: sent-tag filter |
| 285 | `TestTheMigration.test_the_schema_version_was_bumped_for_this` | 6.d: sent-tag filter |
| 289 | `TestTheMigration.test_a_fresh_install_defaults_to_off` | 6.d: sent-tag filter |
| 306 | `TestTheDialogCopy.test_the_two_meanings_are_worded_separately` | 6.d: sent-tag filter |
| 323 | `TestTheDialogCopy.test_the_switch_round_trips_through_apply_to_settings` | 6.d: sent-tag filter |
| 339 | `TestTheDialogCopy.test_an_unchecked_switch_shows_as_unchecked` | 6.d: sent-tag filter |

### `tests/test_session.py` (80)

| Line | Test | Disposition |
|---|---|---|
| 48 | `TestSessionRoundTrip.test_no_saved_session_is_empty` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 51 | `TestSessionRoundTrip.test_the_reviewed_mark_survives_a_real_save_and_load` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 62 | `TestSessionRoundTrip.test_an_unreviewed_entry_stays_unreviewed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 66 | `TestSessionRoundTrip.test_the_upscale_verdict_survives_a_real_save_and_load` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 77 | `TestSessionRoundTrip.test_a_session_written_before_the_upscale_verdict_existed_loads_as_not_checked` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 85 | `TestSessionRoundTrip.test_hash_survives` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 91 | `TestSessionRoundTrip.test_selected_candidate_survives` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 100 | `TestSessionRoundTrip.test_all_tag_sources_survive` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 111 | `TestSessionRoundTrip.test_candidate_details_survive` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 117 | `TestSessionRoundTrip.test_status_and_dimensions_survive` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 124 | `TestSessionRoundTrip.test_corrupt_file_degrades_gracefully` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 130 | `TestSessionRoundTrip.test_unknown_format_version_ignored` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 136 | `TestSessionRoundTrip.test_clear_empties_the_saved_session` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 143 | `TestSessionRoundTrip.test_clear_also_removes_a_legacy_json_session` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 163 | `TestSessionRoundTrip.test_an_old_migrated_backup_is_removed_once_the_db_loads` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 169 | `TestSessionRoundTrip.test_a_recent_migrated_backup_is_kept` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 175 | `TestSessionRoundTrip.test_the_backup_is_kept_while_the_db_is_empty` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 184 | `TestSessionRoundTrip.test_revisions_of_a_loaded_list_make_the_next_save_incremental` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 199 | `TestSessionRoundTrip.test_entry_without_path_is_skipped` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 209 | `TestSessionRoundTrip.test_has_saved_session_is_false_on_a_fresh_install` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 214 | `TestSessionRoundTrip.test_has_saved_session_is_true_once_something_is_saved` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 218 | `TestSessionRoundTrip.test_has_saved_session_is_true_for_a_corrupt_file_too` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 230 | `TestMatchedUrlLogRotation.test_it_rotates_past_the_cap_and_keeps_a_bounded_number` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 264 | `TestSentToHydrusResetOnFreshSearch.test_fresh_search_clears_a_stale_sent_flag` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 299 | `TestSentToHydrusResetOnFreshSearch.test_fresh_search_also_clears_the_confirmed_flag` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 365 | `TestNamedSessionFiles.test_named_session_round_trips` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 374 | `TestNamedSessionFiles.test_saving_a_named_session_does_not_touch_the_automatic_one` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 379 | `TestNamedSessionFiles.test_automatic_save_does_not_touch_a_named_session` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 388 | `TestNamedSessionFiles.test_entry_count_without_loading` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 393 | `TestNamedSessionFiles.test_missing_file_loads_as_empty` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 396 | `TestNamedSessionFiles.test_malformed_file_is_rejected_not_raised` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 403 | `TestNamedSessionFiles.test_valid_json_that_is_not_a_session_is_rejected` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 412 | `TestNamedSessionFiles.test_temp_file_is_cleaned_up_by_the_atomic_replace` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 459 | `TestRestoredMatchNeedsRefetch.test_restored_candidate_has_no_image_bytes` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 465 | `TestRestoredMatchNeedsRefetch.test_restored_candidate_is_flagged_for_refetch` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 473 | `TestRestoredMatchNeedsRefetch.test_no_refetch_once_loaded` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 478 | `TestRestoredMatchNeedsRefetch.test_match_url_and_candidates_survive` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 509 | `TestMissingFileCheck.test_reports_only_the_deleted_files` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 516 | `TestMissingFileCheck.test_present_files_are_never_reported` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 521 | `TestMissingFileCheck.test_unreadable_directory_means_all_its_files` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 535 | `TestMissingFileCheck.test_groups_by_directory_to_cut_round_trips` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 545 | `TestMissingFileCheck.test_empty_input` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 549 | `TestMissingFileCheck.test_stop_check_abandons_the_scan` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 561 | `TestMissingFileCheck.test_flag_is_not_persisted` | 6.b: core/missing_files.py (6.b); the window's use of it is S missing_files_dialog |
| 591 | `TestAutosaveSettings.test_defaults` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 597 | `TestAutosaveSettings.test_round_trips` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 628 | `TestHydrusSentState.test_upload_path_is_confirmed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 639 | `TestHydrusSentState.test_url_importer_is_sent_but_not_confirmed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 651 | `TestHydrusSentState.test_having_a_hash_does_not_imply_sent` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 660 | `TestHydrusSentState.test_sent_state_survives_a_restart` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 692 | `TestSessionWriteDurability.test_write_is_atomic_and_leaves_no_scratch_files` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 698 | `TestSessionWriteDurability.test_concurrent_writers_do_not_share_a_scratch_path` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 722 | `TestSessionWriteDurability.test_a_failed_write_cleans_up_after_itself` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 728 | `TestSessionWriteDurability.test_serializing_while_another_thread_mutates_does_not_raise` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 762 | `TestSessionWriteDurability.test_a_short_candidate_list_is_clamped_on_load` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 811 | `TestSessionDatabase.test_round_trip_preserves_entries_and_order` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 820 | `TestSessionDatabase.test_only_changed_rows_are_rewritten` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 840 | `TestSessionDatabase.test_the_rotating_reconcile_repairs_what_the_counter_missed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 855 | `TestSessionDatabase.test_removed_entries_are_deleted` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 866 | `TestSessionDatabase.test_reordering_is_persisted` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 875 | `TestSessionDatabase.test_a_failed_save_does_not_advance_the_revision_map` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 887 | `TestSessionDatabase.test_an_empty_database_loads_as_an_empty_list` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 892 | `TestSessionDatabase.test_a_corrupt_row_is_skipped_not_fatal` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 923 | `TestLegacySessionMigration.test_a_legacy_file_is_migrated_on_first_load` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 929 | `TestLegacySessionMigration.test_the_old_file_is_kept_as_a_backup_but_not_reread` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 937 | `TestLegacySessionMigration.test_the_database_wins_when_both_exist` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 944 | `TestLegacySessionMigration.test_named_sessions_are_still_json` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 964 | `TestEntryRevisionTracking.test_a_field_write_marks_the_entry_changed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 971 | `TestEntryRevisionTracking.test_thumbnail_bytes_do_not_mark_it_changed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 980 | `TestEntryRevisionTracking.test_the_missing_flag_does_not_mark_it_changed` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 987 | `TestEntryRevisionTracking.test_touch_covers_changes_inside_the_entry` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 998 | `TestEntryRevisionTracking.test_revision_is_not_persisted` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1048 | `TestRemovedEntriesStayRemoved.test_a_full_save_deletes_what_is_no_longer_in_the_list` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1058 | `TestRemovedEntriesStayRemoved.test_the_restored_order_has_no_gaps_left_by_removals` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1068 | `TestRemovedEntriesStayRemoved.test_an_incremental_save_still_deletes` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1077 | `TestRemovedEntriesStayRemoved.test_a_save_that_removes_nothing_deletes_nothing` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1088 | `TestRemovedEntriesStayRemoved.test_removals_survive_a_save_that_knows_nothing_of_them` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1098 | `TestRemovedEntriesStayRemoved.test_saving_an_empty_list_empties_the_stored_session` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1104 | `TestRemovedEntriesStayRemoved.test_the_saved_state_of_a_kept_entry_is_preserved` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |
| 1129 | `TestRemovedEntriesStayRemoved.test_a_removed_entry_that_returns_later_is_stored_once` | core: ported and differentialed in Phases 1 and 5 (hatate-core session / config); the window's use of them is covered by the smoke scenarios on session ops |

### `tests/test_settings_search.py` (10)

| Line | Test | Disposition |
|---|---|---|
| 31 | `TestMatching.test_words_match_in_any_order` | 6.h: settings search |
| 38 | `TestMatching.test_the_tab_name_counts_as_part_of_the_name` | 6.h: settings search |
| 45 | `TestMatching.test_an_empty_query_matches_nothing` | 6.h: settings search |
| 52 | `TestMatching.test_shorter_names_come_first` | 6.h: settings search |
| 56 | `TestMatching.test_matching_ignores_case` | 6.h: settings search |
| 74 | `TestHighlightReentrancy.test_a_second_highlight_inside_the_revert_window_still_restores` | 6.h: settings search |
| 110 | `TestIndexingTheRealDialog.test_it_finds_a_useful_number_of_settings` | 6.h: settings search |
| 114 | `TestIndexingTheRealDialog.test_every_tab_is_reachable_by_its_own_name` | 6.h: settings search |
| 126 | `TestIndexingTheRealDialog.test_explanations_are_not_indexed_as_names` | 6.h: settings search |
| 133 | `TestIndexingTheRealDialog.test_a_known_setting_is_findable` | 6.h: settings search |

### `tests/test_tag_sidecars.py` (29)

| Line | Test | Disposition |
|---|---|---|
| 40 | `TestSidecarPath.test_append_style_keeps_the_image_extension` | 6.d: Write Tag Files |
| 47 | `TestSidecarPath.test_replace_style_drops_it` | 6.d: Write Tag Files |
| 53 | `TestSidecarPath.test_two_images_differing_only_in_extension_collide_under_replace` | 6.d: Write Tag Files |
| 66 | `TestSidecarPath.test_a_name_with_dots_in_it_is_not_truncated` | 6.d: Write Tag Files |
| 73 | `TestSidecarPath.test_an_unrecognised_style_setting_falls_back_to_the_default` | 6.d: Write Tag Files |
| 82 | `TestSidecarPath.test_an_unrecognised_overwrite_setting_falls_back_to_asking` | 6.d: Write Tag Files |
| 88 | `TestSidecarPath.test_the_shipped_defaults_are_the_safe_ones` | 6.d: Write Tag Files |
| 103 | `TestRender.test_one_tag_per_line_newline_terminated` | 6.d: Write Tag Files |
| 107 | `TestRender.test_namespaces_are_written_the_way_they_are_displayed` | 6.d: Write Tag Files |
| 115 | `TestRender.test_duplicates_are_dropped_keeping_the_first` | 6.d: Write Tag Files |
| 125 | `TestRender.test_order_is_the_entrys_own` | 6.d: Write Tag Files |
| 132 | `TestRender.test_no_tags_renders_nothing_at_all` | 6.d: Write Tag Files |
| 138 | `TestRender.test_the_written_list_is_exactly_what_a_hydrus_send_would_carry` | 6.d: Write Tag Files |
| 157 | `TestRender.test_no_settings_means_every_tag` | 6.d: Write Tag Files |
| 179 | `TestPlan.test_planning_writes_nothing` | 6.d: Write Tag Files |
| 186 | `TestPlan.test_an_entry_with_no_tags_is_reported_not_written_empty` | 6.d: Write Tag Files |
| 192 | `TestPlan.test_a_missing_file_gets_no_sidecar` | 6.d: Write Tag Files |
| 201 | `TestPlan.test_an_existing_target_is_flagged_but_still_planned` | 6.d: Write Tag Files |
| 211 | `TestPlan.test_directories_lists_each_folder_once` | 6.d: Write Tag Files |
| 224 | `TestPlan.test_the_plan_honours_the_naming_style_setting` | 6.d: Write Tag Files |
| 243 | `TestWrite.test_writes_the_file_beside_the_image` | 6.d: Write Tag Files |
| 250 | `TestWrite.test_line_endings_are_lf_on_every_platform` | 6.d: Write Tag Files |
| 258 | `TestWrite.test_an_existing_file_is_left_exactly_alone_by_default` | 6.d: Write Tag Files |
| 272 | `TestWrite.test_overwrite_replaces_it` | 6.d: Write Tag Files |
| 282 | `TestWrite.test_an_existing_file_appearing_between_plan_and_write_is_not_clobbered` | 6.d: Write Tag Files |
| 298 | `TestWrite.test_overwriting_leaves_no_temporary_files_behind` | 6.d: Write Tag Files |
| 309 | `TestWrite.test_an_overwrite_that_fails_part_way_leaves_the_old_file_intact` | 6.d: Write Tag Files |
| 332 | `TestWrite.test_the_written_file_is_not_world_readable_by_accident` | 6.d: Write Tag Files |
| 347 | `TestWrite.test_one_unwritable_target_does_not_abandon_the_rest` | 6.d: Write Tag Files |

### `tests/test_theme.py` (21)

| Line | Test | Disposition |
|---|---|---|
| 316 | `TestPalettes.test_both_modes_define_the_same_tokens` | **6.c**: `lib::tokens::tests::light_tokens_override_only_what_the_light_block_sets`, `lib::tokens::tests::dark_tokens_are_the_sheets_values` (the palette is the sheet's) |
| 321 | `TestPalettes.test_every_token_is_substituted` | **6.c**: `lib::theme::tests::no_placeholder_is_left_in_the_sheet` |
| 329 | `TestPalettes.test_every_token_is_a_colour` | **6.c**: `lib::theme::tests::every_named_colour_used_is_defined`, `lib::tokens::tests::var_references_resolve` (every named colour the CSS uses is defined in both schemes) |
| 336 | `TestPalettes.test_resolve_mode_pins_what_it_is_given` | **6.c**: `lib::theme::tests::resolve_scheme_matches_resolve_mode`, `lib::theme::tests::modes_are_the_three_python_has` |
| 340 | `TestPalettes.test_resolve_mode_always_answers_something_drawable` | **6.c**: `lib::theme::tests::resolve_scheme_matches_resolve_mode`, `lib::theme::tests::modes_are_the_three_python_has` |
| 380 | `TestPaletteSubscriptReachability.test_every_subscript_reachable_key_exists_in_both_palettes` | **6.c**: `lib::theme::tests::every_named_colour_used_is_defined`, `lib::tokens::tests::var_references_resolve` (every named colour the CSS uses is defined in both schemes) |
| 427 | `TestInkColorTierReachability.test_every_ink_color_tier_exists_in_both_palettes` | **6.c**: `lib::theme::tests::every_named_colour_used_is_defined`, `lib::tokens::tests::var_references_resolve` (every named colour the CSS uses is defined in both schemes) |
| 475 | `TestTableItemsAreNeverStyled.test_the_sheet_has_no_table_item_rule` | 6.d: Queue table item styling and engine chips |
| 485 | `TestTableItemsAreNeverStyled.test_the_model_background_survives_the_sheet` | 6.d: Queue table item styling and engine chips |
| 524 | `TestEngineChipsCarryNoHue.test_the_hue_keys_are_gone_from_both_palettes` | 6.d: Queue table item styling and engine chips |
| 532 | `TestEngineChipsCarryNoHue.test_the_dead_chip_and_danger_styles_are_gone_from_the_sheet` | 6.d: Queue table item styling and engine chips |
| 544 | `TestEngineChipsCarryNoHue.test_engine_chips_render_with_no_background_fill` | 6.d: Queue table item styling and engine chips |
| 619 | `TestFontWeightsMatchTheRegisteredFace.test_every_declared_weight_is_a_weight_the_family_ships` | **6.c**: `smoke::fonts_cover_the_status_glyphs_and_kanji`, `lib::fonts::tests::nine_faces_are_bundled_and_none_is_woff2` (the Ryoku Status face and the weights shipped) |
| 667 | `TestStatusGlyphFontCoverage.test_every_status_glyph_is_covered_by_its_routed_font` | **6.c**: `smoke::fonts_cover_the_status_glyphs_and_kanji`, `lib::fonts::tests::nine_faces_are_bundled_and_none_is_woff2` (the Ryoku Status face and the weights shipped) |
| 757 | `TestStatusWeightContrast.test_every_status_weight_clears_aa_against_card` | 6.d: the Status column's weights |
| 790 | `TestBorderTierContrast.test_border_tiers_clear_non_text_floor` | **6.c**: `lib::tokens::tests::contrast_matches_the_published_ratios`, `lib::theme::tests::the_ring_clears_three_to_one_on_every_surface_it_sits_on` (the border tiers are carried, D3; `lib::theme::tests::the_border_tier_clears_three_to_one_where_a_control_sits`, and the measured boundaries in section 8) |
| 807 | `TestBorderTierContrast.test_hover_focus_tier_is_louder_than_resting_tier` | **6.c**: `lib::tokens::tests::contrast_matches_the_published_ratios`, `lib::theme::tests::the_ring_clears_three_to_one_on_every_surface_it_sits_on` (the border tiers are carried, D3; `lib::theme::tests::the_border_tier_clears_three_to_one_where_a_control_sits`, and the measured boundaries in section 8) |
| 816 | `TestBorderTierContrast.test_ink_18_itself_is_unchanged` | **6.c**: `lib::tokens::tests::contrast_matches_the_published_ratios`, `lib::theme::tests::the_ring_clears_three_to_one_on_every_surface_it_sits_on` (the border tiers are carried, D3; `lib::theme::tests::the_border_tier_clears_three_to_one_where_a_control_sits`, and the measured boundaries in section 8) |
| 835 | `TestBorderTierContrast.test_header_section_border_clears_non_text_floor` | **6.c**: `lib::tokens::tests::contrast_matches_the_published_ratios`, `lib::theme::tests::the_ring_clears_three_to_one_on_every_surface_it_sits_on` (the border tiers are carried, D3; `lib::theme::tests::the_border_tier_clears_three_to_one_where_a_control_sits`, and the measured boundaries in section 8) |
| 857 | `TestBorderTierContrast.test_status_bar_border_is_documented_quiet_exception` | **6.c**: `lib::tokens::tests::contrast_matches_the_published_ratios`, `lib::theme::tests::the_ring_clears_three_to_one_on_every_surface_it_sits_on` (the border tiers are carried, D3; `lib::theme::tests::the_border_tier_clears_three_to_one_where_a_control_sits`, and the measured boundaries in section 8) |
| 1294 | `TestGenericContrastSweep.test_every_rendered_pair_clears_its_floor_or_is_exempt` | **6.c**: `lib::theme::tests::body_text_tiers_stay_above_four_and_a_half`, `lib::tokens::tests::contrast_matches_the_published_ratios` (text tiers; the rendered-pair sweep is the verifier's shot set) |

### `tests/test_thumbnail_worker.py` (26)

| Line | Test | Disposition |
|---|---|---|
| 106 | `TestThumbnailSource.test_source_defaults_to_local_when_there_are_no_settings` | 6.d: thumbnail worker |
| 110 | `TestThumbnailSource.test_source_is_read_from_settings` | 6.d: thumbnail worker |
| 114 | `TestThumbnailSource.test_off_reads_no_files_at_all_but_still_finishes` | 6.d: thumbnail worker |
| 128 | `TestThumbnailSource.test_hydrus_source_without_an_access_key_falls_back_to_local` | 6.d: thumbnail worker |
| 144 | `TestLocalDecode.test_a_large_image_is_decoded_down_to_the_requested_size` | 6.d: thumbnail worker |
| 155 | `TestLocalDecode.test_an_image_already_smaller_than_the_target_is_left_alone` | 6.d: thumbnail worker |
| 163 | `TestLocalDecode.test_an_undecodable_file_yields_None_rather_than_a_null_image` | 6.d: thumbnail worker |
| 173 | `TestLocalDecode.test_a_missing_file_yields_None` | 6.d: thumbnail worker |
| 177 | `TestLocalDecode.test_a_raising_decoder_yields_None_rather_than_killing_the_thread` | 6.d: thumbnail worker |
| 189 | `TestLocalDecode.test_every_decoded_row_is_emitted_with_its_own_entry` | 6.d: thumbnail worker |
| 200 | `TestLocalDecode.test_a_row_that_cannot_be_decoded_still_reports_progress` | 6.d: thumbnail worker |
| 217 | `TestDecodeFromBytes.test_bytes_are_scaled_down_to_the_requested_size` | 6.d: thumbnail worker |
| 225 | `TestDecodeFromBytes.test_bytes_already_small_enough_are_not_rescaled` | 6.d: thumbnail worker |
| 232 | `TestDecodeFromBytes.test_unrecognisable_bytes_yield_None` | 6.d: thumbnail worker |
| 240 | `TestKnownToHydrus.test_no_hashes_means_no_call_is_made_at_all` | 6.d: thumbnail worker |
| 247 | `TestKnownToHydrus.test_hashes_are_asked_for_in_batches` | 6.d: thumbnail worker |
| 265 | `TestKnownToHydrus.test_files_with_no_hash_are_not_asked_about` | 6.d: thumbnail worker |
| 275 | `TestKnownToHydrus.test_a_hydrus_error_stops_asking_and_keeps_what_was_answered` | 6.d: thumbnail worker |
| 299 | `TestKnownToHydrus.test_a_stop_request_abandons_the_remaining_batches` | 6.d: thumbnail worker |
| 334 | `TestHydrusThumbnailPath.test_a_known_hash_is_served_from_hydrus_without_reading_the_file` | 6.d: thumbnail worker |
| 350 | `TestHydrusThumbnailPath.test_a_hash_hydrus_does_not_know_is_decoded_from_disk` | 6.d: thumbnail worker |
| 361 | `TestHydrusThumbnailPath.test_an_empty_hydrus_thumbnail_falls_back_to_the_file` | 6.d: thumbnail worker |
| 372 | `TestHydrusThumbnailPath.test_an_undecodable_hydrus_thumbnail_falls_back_to_the_file` | 6.d: thumbnail worker |
| 388 | `TestStopRequest.test_stop_sets_the_flag` | 6.d: thumbnail worker |
| 394 | `TestStopRequest.test_a_worker_stopped_before_it_starts_emits_nothing_but_finishes` | 6.d: thumbnail worker |
| 406 | `TestStopRequest.test_stopping_part_way_through_leaves_the_earlier_rows_emitted` | 6.d: thumbnail worker |

## 8. Shots and contrast

**Where the shots come from.** The hosted `ci` run renders them on the CI toolkit (GTK 4.14 / libadwaita 1.5,
Xvfb, cairo renderer, temp profile, bundled fonts) with `examples/shot.rs` (test-only) and uploads them as the
artifact `shots` (run 40087927106 at `5ed1c1b`; `gh run download 40087927106 -n shots`). Every shot is
synthetic: no real session, file or image. Each shot has a `.rects` sidecar (label rectangles in window
coordinates) that the measuring scripts read. Window size is 1090x790 in every shot except `empty-min`, which
is the stated minimum (1010x790), and `logs` (the log viewer's own window).

**The shots are 1:1 since DAN-2242.** The first shot harness asked `WidgetPaintable` for the window's allocated
size (1090x790) while its intrinsic size is 1100x800 (the 5 px client-side margin), so every shot was scaled by
0.991 x 0.988 and each 1 px border was smeared over two pixels; a capture of the real display (`import -window
root`) showed them crisp. `texture_of` in `shot.rs` now snapshots at the intrinsic size and cuts the allocation
out. The tables below are from the fixed harness; numbers quoted from earlier reads (DAN-2228, DAN-2229) are
from scaled shots.

Shots, dark and light each: `empty` (the empty state), `empty-min` (the empty state at the stated minimum),
`rows` (a list with rows), `crashed` (the Crashed banner), `survived` ("Nothing survived."), `quota` (the
SauceNAO pause banner), `logs` (the log viewer window), `logs-selected` (its path line selected and focused) and `logs-selected-body` (its log text selected), `dialog` (a Yes/No question, default No, ring showing),
`menu` and `menu-*-popover` (the Files menu open), `menu-hover` (the same with `Save Session As...` under the
pointer), `focus` (the first control, the Queue mode switch, with the focus ring), `focus-files` (the
stamp-filled Add files) and `focus-folder` (the hollow Add folder) with the ring showing, and `hover-folder`
(Add folder under the pointer). A headless session has no pointer, so the hover states are set on the widget.

**Text contrast.** `measure.py` (outside the repo: `worktrees/DAN-2215-tools/measure.py SHOTDIR`) takes each
label's rectangle, the most common colour inside it as the background and the pixel farthest from it as the
foreground (the extreme-pixel method), and reports the WCAG 2.x ratio. **690 labels (the DAN-2242 set; the DAN-2245 set below is 732), dark and light: 0 enabled
labels under 4.5:1.** The full per-label table is `worktrees/DAN-2215/shots/contrast-ci2242.md`; the table below
groups it by (scheme, fg, bg). Disabled controls are exempt (`Start Search`, the gear, `Query Hydrus`,
`Resume Queue`, ...; each is `DEFERRED(6.e/6.f/6.g/6.h)`). **Not measured:** the text inside the `dialog` shot
(the scrim dims the window and the dialog's own labels are not in the widget tree the renderer records).

| scheme | fg | bg | ratio | verdict | labels | example |
|---|---|---|---|---|---|---|
| dark | #635f5b | #0d0d0d | 3.07 | disabled (exempt) | 12 | ⚙ |
| dark | #67635f | #141414 | 3.09 | disabled (exempt) | 9 | Query Hydrus…, Continue with other engines |
| dark | #0d0d0d | #6d6863 | 3.53 | disabled (exempt) | 12 | ▶ Start Search |
| dark | #111111 | #716c67 | 3.64 | disabled (exempt) | 2 | Resume Queue, Review the 4 waiting → |
| dark | #8a857f | #141414 | 5.04 | pass | 4 | Crash log, 410, Resets |
| dark | #87817b | #0d0d0d | 5.05 | pass | 13 | stopped, ETA —, 力 |
| dark | #8b8680 | #141414 | 5.11 | pass | 2 | Last autosave, 00:00 UTC |
| dark | #8c8680 | #141414 | 5.12 | pass | 24 | Hatate did not exit cleanly during this run. Eve, Discard &  |
| dark | #88837d | #0d0d0d | 5.17 | pass | 10 | Ready, ETA paused |
| dark | #89837d | #0d0d0d | 5.19 | pass | 3 | ago, paused — quota exhausted, last run failed — see crash l |
| dark | #89847e | #0d0d0d | 5.24 | pass | 1 | Restored 12 image(s) from your last session |
| dark | #8a847e | #0d0d0d | 5.26 | pass | 114 | Files, Hydrus, Sites, Settings, Help, Review, Activity, 00 / |
| dark | #97918a | #0d0d0d | 6.23 | pass | 1 | 力 |
| dark | #cdc4ba | #262524 | 8.89 | pass | 1 | Add folder… |
| dark | #cdc4ba | #141414 | 10.70 | pass | 29 | ◐, Run interrupted — partial results survived, 0s ago, no ne |
| dark | #cdc4ba | #0d0d0d | 11.29 | pass | 58 | 力, 鏡, Hatate, 3, 1, 0s, Queue., 空, Refresh, Copy all, Open l |
| dark | #0d0d0d | #cdc4ba | 11.29 | pass | 23 | Queue, Add files…, Files, Start new search |
| light | #3e3c3a | #141414 | 1.68 | disabled (exempt) | 10 | Query Hydrus…, Re-check Queued Imports, Pawchive Index…, Exp |
| light | #bdb9af | #f2ede1 | 1.68 | disabled (exempt) | 10 | Query Hydrus…, Re-check Queued Imports, Pawchive Index…, Exp |
| light | #847e74 | #e8e1d2 | 3.09 | disabled (exempt) | 12 | ⚙ |
| light | #89847b | #f2ede1 | 3.18 | disabled (exempt) | 9 | Query Hydrus…, Continue with other engines |
| light | #f4efe3 | #8a857c | 3.20 | disabled (exempt) | 2 | Resume Queue, Review the 4 waiting → |
| light | #efe9dc | #857f75 | 3.28 | disabled (exempt) | 12 | ▶ Start Search |
| light | #8c8680 | #141414 | 5.12 | pass | 3 | Ctrl+S, Ctrl+O |
| light | #5f5b52 | #e8e1d2 | 5.20 | pass | 13 | stopped, ETA —, 力 |
| light | #5e5950 | #e8e1d2 | 5.34 | pass | 10 | Ready, ETA paused |
| light | #5d584f | #e8e1d2 | 5.42 | pass | 4 | ago, paused — quota exhausted, Restored 12 image(s) from you |
| light | #5c574f | #e8e1d2 | 5.50 | pass | 114 | Files, Hydrus, Sites, Settings, Help, Review, Activity, 00 / |
| light | #615c55 | #f2ede1 | 5.67 | pass | 4 | Crash log, 410, Resets |
| light | #605c54 | #f2ede1 | 5.70 | pass | 2 | Last autosave, 00:00 UTC |
| light | #5f5b53 | #f2ede1 | 5.78 | pass | 27 | Hatate did not exit cleanly during this run. Eve, Discard &  |
| light | #4b4740 | #e8e1d2 | 7.09 | pass | 1 | 力 |
| light | #cdc4ba | #262524 | 8.89 | pass | 4 | Add Files…, Save Session As…, Ctrl+S |
| light | #cdc4ba | #141414 | 10.70 | pass | 9 | Add Folder…, Save Session As…, Open Session…, Open Matched U |
| light | #211d17 | #d9d4c9 | 11.34 | pass | 3 | Add folder…, Save Session As…, Ctrl+S |
| light | #211d17 | #ddd8cd | 11.80 | pass | 2 | Add Files… |
| light | #211d17 | #e8e1d2 | 12.88 | pass | 58 | 力, 鏡, Hatate, 3, 1, 0s, Queue., 空, Refresh, Copy all, Open l |
| light | #211d17 | #f2ede1 | 14.35 | pass | 38 | ◐, Run interrupted — partial results survived, 0s ago, no ne |
| light | #f6f1e6 | #211d17 | 14.88 | pass | 23 | Queue, Add files…, Files, Start new search |

**The `_ (_)` fragment under the dialog (DAN-2245 item 4, closed by DAN-2256).** In the GTK 4.14 / cairo shots
(`dialog-dark.png`, `dialog-light.png`), 35 pixels at x 413 to 448, y 497 to 499 sit just under the dialog's bottom
border (y 496), the lower rows of what look like glyph bottoms. They do **not** reproduce on GTK 4.24 / libadwaita
1.10 (the layout differs, so the comparison is the band under the border). The widget tree of the 4.14 dialog
(`dialog-*.tree`) has no widget with a box there. **Cause: the Queue page's placeholder label** (`12 image(s) in the
list. The list view arrives in 6.d.`, x 367 to 722, y 481 to 501) seen through the scrim: the sheet covers y <= 496,
so the label's descender rows 497 to 499 (the `g` of "image", the `(` `)` of "(s)") show below it. **Control:** the
`dialog-nolabel` state (the same shot with that label hidden; the two trees differ only in that label's line) on the
CI toolkit, run 40485308569 at `1135a27`: with the label 35 pixels in that band, without it 0, dark and light; the
two shots differ by exactly those 35 pixels in y >= 497. In the dark pair the other 3,161 differing pixels
(3,196 in all) lie in y 293 to 496, 3,157 of them at 7/255 or less; in the light pair 2,133 pixels differ in
all, so 2,098 besides those 35. Not explained. The label's glyphs also show, dimmed the same way, beside the
sheet.
**Decision: no change to the scrim.** A modal shows the window's content around the sheet, dimmed; that is what the
scrim is for, and the position comes from layout, not from the renderer, so the cairo-under-Xvfb question does not
move it. It is not a stray widget or a paint bug. No real-display capture was taken; the claim is about this layout
and was not measured on `ngl`. The placeholder goes in 6.d, when the list view replaces it.

**Selected text, and the log viewer (DAN-2245).** The first sweep did not read a selected-text state, and its two
`Log file:` rows above (`#6f849c` on `#0d0d0d` 5.05, `#50657b` on `#e8e1d2` 4.62) were the path line measured with
libadwaita's accent-blue selection under it, against a box wider than the selection: 3.48:1 dark and 3.40:1 light
by the design read. Selected text is now `ink` on an `ink-18` wash (`selection` rule in `theme.rs`, pinned by
`theme::tests::selected_text_is_ink_on_the_ink_18_wash` and, where the toolkit paints one headlessly,
`smoke::the_log_viewer_opens_on_close_and_selects_in_ink`), and the log viewer opens with focus on `Close`.
`measure.py` now takes `logs`, `logs-selected` and `logs-selected-body` in both schemes as required input (exit 2
if one is missing, exit 1 if any enabled row is under 4.5:1), and the renderer records a `selected` row for a
selected label and for each line of the log text view. On the CI shots (GTK 4.14, run 40091650109 at `5d54419`):
**732 labels, 0 enabled under 4.5:1; 8 selected-text rows, 0 under 4.5:1.** With the selection rule removed the
same script reads 3.49:1 dark and 3.44:1 light on the path line (exit 1).

| Shot | Element | scheme | fg | wash | ratio |
|---|---|---|---|---|---|
| logs-selected | path line | dark | #cdc4ba | #302e2c | 7.86 |
| logs-selected | path line | light | #211d17 | #a8a396 | 6.66 |
| logs-selected-body | log lines | dark | #cdc4ba | #2c2a28 | 8.31 |
| logs-selected-body | log lines | light | #211d17 | #a19888 | 5.88 |

**Non-text contrast (3:1) and the other DAN-2242 measures.** `worktrees/DAN-2242-tools/measure-2242.py SHOTDIR`
(output `worktrees/DAN-2242-measure-ci.log`), extreme pixel on a short scan across each edge, against the
surface it sits on.

| Boundary | scheme | pixel | surface | ratio | 3:1 |
|---|---|---|---|---|---|
| gear button border (`ink-46`) | dark | #65615c | page #0d0d0d | 3.16 | pass |
| gear button border (`ink-46`) | light | #827c72 | page #e8e1d2 | 3.18 | pass |
| mode-switch outer edge (`ink-46`) | dark | #65615c | page #0d0d0d | 3.16 | pass |
| mode-switch outer edge (`ink-46`) | light | #827c72 | page #e8e1d2 | 3.18 | pass |
| Add folder border (`ink-46`) | dark | #696560 | raised #141414 (it sits in the frame) | 3.19 | pass |
| Add folder border (`ink-46`) | light | #878279 | raised #f2ede1 | 3.27 | pass |
| Add folder border, pointer over it (`ink-58`) | dark / light | #87827b / #67635a | page | 5.10 / 4.60 | pass |
| focus ring on `No` (2 px `ink`) | dark / light | #cdc4ba / #211d17 | dialog #141414 / #f2ede1 | 10.70 / 14.35 | pass |
| focus ring on Add folder (hollow) | dark / light | #cdc4ba / #211d17 | raised | 10.70 / 14.35 | pass |
| focus ring on Add files (stamp-filled, `stamp_fg`) | dark / light | #0d0d0d / #f6f1e6 | stamp #cdc4ba / #211d17 | 11.29 / 14.88 | pass |
| **the deliberate sub-3:1 set (`ink-18`)**: run strip border | dark / light | #302e2c / #a8a396 | page | 1.44 / 1.93 | by design |
| frame hairline | dark / light | #353331 / #afaba1 | page | 1.54 / 1.76 | by design |
| menubar rule | dark / light | #302e2c / #a8a396 | page | 1.44 / 1.93 | by design |

Add folder measures 3.36 dark and 2.93 light against the *page* colour, but it never touches the page: the
button sits in the raised frame, so the surface it has to clear is the frame's.

- **The border tiers.** A control whose border is its shape (`button.pill`, `button.icon-boxed`, the
  mode-switch group, the banner action buttons, the disabled hollow variants) uses `ink-46` at rest and
  `ink-58` on hover, as `gui/theme.py` does (D3). `ink-45` is not used: it is 2.68:1 on the light page by the
  design read. Cards, the run strip, the frame hairline and rules stay `ink-18`: they group content that
  carries its own 4.5:1 text. A disabled hollow control keeps its `ink-46` border and `ink-45` text
  (libadwaita's 50% fade would take the border to 1.6:1); a disabled stamp keeps the fade.
- **Ghost kanji (fix 1).** The label box is 420 px tall (`630,-11 420x420`, was 420x604); its upper strokes
  bleed off under the menubar (ink rows 34 to 236 above the frame) as in the AppImage shot, and nothing of the
  glyph shows inside the frame: 20,700 sampled interior pixels, 0 off the raised fill, both schemes.
- **Frame fill (fix 2).** Interior pixel `#141414` dark / `#f2ede1` light in the empty, Crashed, quota and
  Nothing-survived states.
- **Card outline (fix 3).** No pixel outside the frame differs from the page colour (amplified diff
  `amp-empty-{dark,light}.png`: 0 stray pixels in the heading-to-frame strip and the left margin).
- **Hint on a hovered row (fix 5).** `Ctrl+S` on the hovered `Save Session As...` row: 8.89:1 dark, 11.34:1
  light (was 4.25 dark); at rest 5.12 / 5.78.
- **Dialog default (fix 6).** `No` shows a 2 px `ink` ring when the dialog opens (`set_focus_visible(true)` on
  the window before `choose`); `Yes` shows only its 1 px border.
- **The focus ring** passes in both schemes on every control shot: one ring token, 2 px `ink`, or `stamp_fg`
  on a stamp-filled control.
- **Not judgeable from shots:** window chrome, the GPU renderer, the real compositor backdrop (inactive
  selection), HiDPI and hover wash on a real pointer. Owner: Cloud, on real hardware in Phase 7 (DAN-1999 F4).

**Tab order of the shell** (state `tab` of `shot.rs`, walking `child_focus(TabForward)` from the top): on the
empty Queue, `Queue`, `Review`, `Activity`, `Add files`, `Add folder`. `Start Search`, the gear and
`Query Hydrus` are disabled and are skipped. The order is printed by the shot renderer in a live Xvfb
session (hosted `ci` log, step "Shot set (under Xvfb)").

**The menubar's keyboard path** (DAN-2246, smoke scenario `menubar_keyboard_path`, `menubar_keys.rs`). The
scenario sends real X key events from Rust: `tests/common/xtest.rs` is a minimal X11 client (wire protocol
over the display socket, XTEST `FakeInput`; no crate, no `xdotool`, test-only), so GTK receives them as it
would from a keyboard. Measured on the real window: GTK's own `PopoverMenuBar` already opens Files on Alt+F,
moves with Left and Right, and closes on Escape. It did **not** (a) open anything on F10, which only moved
focus onto the first bar item, or (b) give focus back on Escape, which left it on the bar. `menubar_keys.rs`
adds F10 (opens the first menu through the route an Alt mnemonic takes, so Left and Right keep working; F10
again closes; Shift+F10 and Ctrl+F10 are left alone) and returns focus to the widget that had it before the
bar once no menu is open. The earlier "keys never reach GTK" result (DAN-2242) was the log viewer windows of
earlier scenarios, which stay mapped and hold the keyboard; the scenario destroys other toplevels first.
Not covered: Alt alone (Qt's underline reveal), mnemonics for menus other than Files, and the keyboard path
on Wayland (the injector is X11 only; with `CI` set a missing XTEST fails the scenario, without it the
scenario skips).
