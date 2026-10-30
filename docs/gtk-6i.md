# Settings > MCP, the Shortcuts tab and the server's lifecycle (Phase 6.i)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of [DAN-2222](/DAN/issues/DAN-2222).
It holds the two maps Phase 6 asks every ticket for (the method map and the Python test map; 6.j checks the union),
the differentials with their counts, the on-purpose differences and the shot set. Python is
`Dromares/Hatate-Linux-Redux` at `76c85c4`. Nothing here reaches a network except `127.0.0.1`: the tests start the
window's own server on a free local port over plain http with a synthetic token.

## 1. The cut

**Ported here** (`hatate-gtk`; `hatate-core` gains `QueueInbox::channel`, `QueueEvent::ReviewedChanged` and
`token_urlsafe`, all additive; `py_dumps` becomes `pub`; `launch_search_worker` returns whether a run began; no new dependency):

- `mcp_bridge.rs`: the Qt-side lifecycle of `gui/mcp_bridge.py` and of `MainWindow` around it. The server starts with
  the window when `mcp.enabled`, `closeEvent` stops it first, and a tool reaches the window's own `QueueModel` through a
  `QueueInbox` pumped on the GTK thread (the channel wakes the loop; nothing polls). `research` starts the window's own
  search and the tool returns when it ends. The controller is built lazily, so a disabled server costs nothing.
- `settings_mcp.rs`, `mcp_audit_view.rs`: the MCP page: six Connection states, the masked token with Copy and Generate,
  the Dry run sentence and weight, the four tier groups, the "Got it" enable summary, and the Recent tool calls table
  (Actions/All, a follow timer only while the tab is on show, a 200-row cap and its caption).
- `shortcuts_page.rs`: the Shortcuts tab over 6.b's registry: a one-key recorder, Clear, Reset, a conflict line, and OK
  refused on a duplicate with Python's warning. `config.json` keeps only what differs from the default.
- `settings_dialog.rs`, `settings_values.rs`, `settings_form.rs`, `settings_node.rs`: the two tabs plugged into 6.h's
  frame, `apply` for the eight `mcp` keys and `review_shortcuts`, the refresh after OK that rebinds the server only when
  `enabled`, `port` or `token` changed.
- `theme.rs`: `label.ink-100`, `label.ink-65` and `button.key-edit`, tokens only.
- `examples/shot.rs` and `ci.yml`: the states in section 6.

**Not in this PR** (named owners):

| What | Owner | State here |
|---|---|---|
| The MCP tools themselves and the server's wire | 5.d, 5.e (merged) | reused as they are; section 5 drives them through the window |
| The six Test buttons and Fetch from Hydrus | 6.h-2 | unchanged: built and insensitive |
| The queue table header keys | 6.d, 6.e | set aside in the comparer, as in 6.g |

## 2. Differences from Python

| Id | What differs | Why | Pinned by |
|---|---|---|---|
| **D-A** | A failed start shows `last_error_user_message()` where Python shows the raw `last_error`; the raw text only picks the state. | The ticket orders it (5.d F2: no token character in a message). | `crates/hatate-gtk/src/mcp_audit_view.rs:855` |
| **D-B** | Connection state 3 (the `mcp` package is not installed) cannot occur. | The server is part of the build, as in `docs/mcp-server.md`. | the card text is pinned, `crates/hatate-gtk/src/mcp_audit_view.rs:799` |
| **D-C** | The `QKeySequenceEdit` is a button that records the next key (a modifier alone does not end it; Delete is recorded as `Del`, Backspace as `Backspace`; clearing is the Clear button). | GTK has no such widget. The spelling is Qt's (`Shift+N`, `Ctrl+Shift+A`, `Del`). | `crates/hatate-gtk/tests/smoke.rs:7836`, `crates/hatate-gtk/tests/smoke.rs:7839`, `shortcuts_page.rs` `text_is_written_the_way_a_key_sequence_writes_it` |
| **D-D** (C1) | When Hydrus refuses the connection, the library words after `Could not reach Hydrus at <url>: ` differ (27 steps over the wire seed). | The accepted class C1 (DAN-2126, DAN-2133), `docs/hydrus-client.md`. | the comparer's `norm_c1` (`cmp.py`), which accepts only that tail |
| **D-E** | The `Server: uvicorn` header (1212 steps) and `serverInfo.version` (60 steps) differ. | Not compared and granted, `docs/mcp-server.md` (V1). | counted by the comparer, not hidden |
| **D-F** | After a tool's `toggle_reviewed` or `remove_row` the status line says what the window's own click says. Not a difference from Python: the wire differential found the Rust window leaving the line alone (80 and 230 steps) and it was fixed here (`mcp_bridge.rs` `render_queue_events`, `QueueEvent::ReviewedChanged`). | A defect of this ticket's own code. | `crates/hatate-gtk/tests/smoke.rs:7501` and `crates/hatate-gtk/tests/smoke.rs:7531`, each red with its line removed (`worktrees/DAN-2222-revert.log` R1, R2) |

## 3. Method map (`gui/` at `76c85c4`)

| Python | Rust |
|---|---|
| gui/settings_dialog.py:52 `_read_mcp_audit_entries` | `crates/hatate-gtk/src/mcp_audit_view.rs:108` |
| gui/settings_dialog.py:77 `_mcp_time_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:190` |
| gui/settings_dialog.py:91 `_mcp_target_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:204` |
| gui/settings_dialog.py:101 `_mcp_outcome_weight` | `crates/hatate-gtk/src/mcp_audit_view.rs:215` |
| gui/settings_dialog.py:125 `_mcp_outcome_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:246` |
| gui/settings_dialog.py:166 `_mcp_outcome_tooltip` | `crates/hatate-gtk/src/mcp_audit_view.rs:329` |
| gui/settings_dialog.py:2075 `_build_shortcuts_tab` | `crates/hatate-gtk/src/shortcuts_page.rs:300` |
| gui/settings_dialog.py:2175 `_current_shortcut_bindings` | `crates/hatate-gtk/src/shortcuts_page.rs:412` |
| gui/settings_dialog.py:2181 `_reset_shortcuts_to_defaults` | `crates/hatate-gtk/src/shortcuts_page.rs:426` |
| gui/settings_dialog.py:2188 `_refresh_shortcut_conflicts` | `crates/hatate-gtk/src/shortcuts_page.rs:436` |
| gui/settings_dialog.py:2212 `_build_mcp_tab` | `crates/hatate-gtk/src/settings_mcp.rs:53` (the page as data) and `crates/hatate-gtk/src/settings_dialog.rs:442` (its handlers) |
| gui/settings_dialog.py:2311 `_build_mcp_connection_card` | `crates/hatate-gtk/src/settings_mcp.rs:53` and `crates/hatate-gtk/src/mcp_audit_view.rs:450` (the six states) |
| gui/settings_dialog.py:2331 `_build_mcp_always_available_group`, :2346 `_build_mcp_tier_checkbox_group` | `crates/hatate-gtk/src/settings_mcp.rs:53` |
| gui/settings_dialog.py:2386 `_copy_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs:543` |
| gui/settings_dialog.py:2391 `_generate_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs:551` |
| gui/settings_dialog.py:2398 `_refresh_mcp_dry_run_hint` | `crates/hatate-gtk/src/settings_dialog.rs:567`, `crates/hatate-gtk/src/settings_mcp.rs:241` |
| gui/settings_dialog.py:2412 `_on_mcp_enabled_toggled`, :2417 `_show_mcp_enable_summary` | `crates/hatate-gtk/src/settings_dialog.rs:578`, `crates/hatate-gtk/src/settings_mcp.rs:250`, the toggle handler in `crates/hatate-gtk/src/settings_dialog.rs:442` |
| gui/settings_dialog.py:2454 `_mcp_server_handle` | `crates/hatate-gtk/src/mcp_bridge.rs:317` |
| gui/settings_dialog.py:2457 `_mcp_restart_server` | `crates/hatate-gtk/src/settings_dialog.rs:595` |
| gui/settings_dialog.py:2474 `_refresh_mcp_connection` | `crates/hatate-gtk/src/settings_dialog.rs:606` |
| gui/settings_dialog.py:2536 `_build_mcp_audit_card`, :2594 `_on_mcp_audit_filter_changed`, :2605 `_open_mcp_audit_folder` | `crates/hatate-gtk/src/settings_mcp.rs:314` |
| gui/settings_dialog.py:2597 `_on_mcp_audit_follow_toggled` | `crates/hatate-gtk/src/settings_mcp.rs:468` |
| gui/settings_dialog.py:2620 `_refresh_mcp_audit_table` | `crates/hatate-gtk/src/settings_mcp.rs:422` |
| gui/settings_dialog.py `apply_to_settings` and the set half of `__init__`, MCP fields and `review_shortcuts` | `crates/hatate-gtk/src/settings_values.rs:297` |
| `gui/main_window.py:421` `__init__`, lines 531-542 (build `McpToolHandlers` and `McpServerController`, start when enabled) | `crates/hatate-gtk/src/window.rs:535`, `crates/hatate-gtk/src/mcp_bridge.rs:273`, `crates/hatate-gtk/src/mcp_bridge.rs:228` |
| `gui/main_window.py:4047` `action_open_settings`, lines 4050-4077 (rebind only when enabled, port or token changed) | `crates/hatate-gtk/src/settings_dialog.rs:930`, `crates/hatate-gtk/src/mcp_bridge.rs:289` |
| `gui/main_window.py:4139` `closeEvent`, lines 4143-4148 (stop the server first) | `crates/hatate-gtk/src/session_ops.rs:846`, `crates/hatate-gtk/src/mcp_bridge.rs:309` |
| `gui/mcp_bridge.py:60` `MainThreadTimeout`, `:64` `MainThreadInvoker` (`__init__`, `_run`, `call`, `shutdown`) | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, `crates/hatate-gtk/src/mcp_bridge.rs:116` |
| `gui/mcp_bridge.py:137` `McpToolHandlers` (`__init__`, `_mcp_settings`, `_run_tool`, `_entry_at`) and the tools `list_queue` :209, `get_entry` :234, `get_candidates` :247, `get_images` :277, `get_diff` :311, `list_actions` :343, `select_candidate` :363, `toggle_reviewed` :383, `send_upload` :465, `send_url` :490, `download_send` :526, `remove_row` :558, `reset_result` :573; `_queue_row` :596, `_entry_detail` :608, `_matches_filter` :635, `_guess_mime` :664 | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and status line (the `remove_row` and `toggle_reviewed` status lines are pinned, see section 2) |
| `gui/mcp_bridge.py:400` `research`, `:409` `_perform_research`, `:449` `read_results` | `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) |

## 4. Python test map

An `ast` walk of `tests/test_*.py` at `76c85c4` (`worktrees/DAN-2222-tools/testmap.py`), following `setUp`, the test's own
class and base-class helpers and module helpers, for a reference to any MCP-page or Shortcuts-page member of
`SettingsDialog`, `MainThreadInvoker`, `McpToolHandlers` or the `_mcp_*` attributes of the window, found **70
tests in 20 classes** (list: `worktrees/DAN-2222-testmap.tsv`). The settings, Shortcuts and type-voice rows
below are generated from that walk and a rule table (`worktrees/DAN-2222-tools/mkdoc.py`; a walked test with no rule
aborts the generator, so none is unmapped). The audit and bridge classes are the 5.d and 5.e tests, already mapped
row by row in the docs named in each row. Where a row names an assertion it is the line that makes the row's claim.

| Python test (`tests/` at `76c85c4`) | Rust |
|---|---|
| `test_gui_smoke.py:5607` `TestShortcutsSettingsTab.test_it_offers_every_registered_action` | `crates/hatate-gtk/tests/smoke.rs:7824`, `crates/hatate-gtk/tests/smoke.rs:7878` (every registered id has an editor) |
| `test_gui_smoke.py:5611` `TestShortcutsSettingsTab.test_only_changed_bindings_are_persisted` | `crates/hatate-gtk/tests/smoke.rs:7891` (OK saves only `next_row` and `prev_row`, the two that differ), `crates/hatate-gtk/tests/smoke.rs:7896` |
| `test_gui_smoke.py:5619` `TestShortcutsSettingsTab.test_clearing_a_binding_is_remembered_as_cleared` | `crates/hatate-gtk/tests/smoke.rs:7897` (a cleared key is saved as `""`), `crates/hatate-gtk/tests/smoke.rs:7870` |
| `test_gui_smoke.py:5626` `TestShortcutsSettingsTab.test_a_duplicate_key_is_named_rather_than_resolved_silently` | `crates/hatate-gtk/tests/smoke.rs:7845` (both names in the line) |
| `test_gui_smoke.py:5635` `TestShortcutsSettingsTab.test_ok_is_refused_while_two_actions_share_a_key` | `crates/hatate-gtk/tests/smoke.rs:7851`, `crates/hatate-gtk/tests/smoke.rs:7855`, `crates/hatate-gtk/tests/smoke.rs:7866` |
| `test_gui_smoke.py:5646` `TestShortcutsSettingsTab.test_ok_goes_through_once_the_clash_is_resolved` | `crates/hatate-gtk/tests/smoke.rs:7885` (OK closes the dialog once the clash is cleared) |
| `test_gui_smoke.py:5654` `TestShortcutsSettingsTab.test_reset_puts_every_default_back` | `crates/hatate-gtk/tests/smoke.rs:7876` and the loop after it (`crates/hatate-gtk/tests/smoke.rs:7878`) |
| `test_mcp_settings_tab.py:65` `TestConstructionWithNoMainWindow.test_builds_without_a_parent` | `crates/hatate-gtk/tests/smoke.rs:7552` (a window with the server off opens on `Stopped.`). The no-parent half does not apply: the Rust dialog is always built over its main window (6.h) |
| `test_mcp_settings_tab.py:70` `TestConstructionWithNoMainWindow.test_enabled_defaults_off_and_hydrus_writes_defaults_on` | `crates/hatate-gtk/tests/smoke.rs:7628` and the two `!shown.text.contains` lines after it (opening from defaults and ticking the server lists Hydrus writes and neither of the other two: **indirect**, no test reads the three boxes' initial state directly); the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) opens the dialog from defaults in every case and compares every control's value |
| `test_mcp_settings_tab.py:91` `TestApplyToSettings.test_round_trips_every_mcp_field` | `crates/hatate-gtk/tests/smoke.rs:7716` to `crates/hatate-gtk/tests/smoke.rs:7720` (port, token, dry run, research, enabled, destructive off, Hydrus writes on, audit size at its default) and `crates/hatate-gtk/tests/smoke.rs:7723` (`config.json`). Hydrus writes switched **off** and a changed audit size are not asserted in a smoke test; the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) sets every one of the eight fields and compares `config.json` as bytes |
| `test_mcp_settings_tab.py:120` `TestEnableSummaryConstructionSafety.test_does_not_fire_when_restoring_a_saved_enabled_state` | `crates/hatate-gtk/tests/smoke.rs:7610` (`opening on a saved enabled state shows nothing`) |
| `test_mcp_settings_tab.py:132` `TestEnableSummaryConstructionSafety.test_fires_exactly_once_on_a_real_off_to_on_click` | `crates/hatate-gtk/tests/smoke.rs:7618` (one box after off then on) and `crates/hatate-gtk/tests/smoke.rs:7616` (none for on to off) |
| `test_mcp_settings_tab.py:156` `TestConnectionStates.test_state_1_stopped_when_not_enabled` | `crates/hatate-gtk/src/mcp_audit_view.rs:784`, `crates/hatate-gtk/src/mcp_audit_view.rs:786`; `crates/hatate-gtk/tests/smoke.rs:7552`, `crates/hatate-gtk/tests/smoke.rs:7553` |
| `test_mcp_settings_tab.py:162` `TestConnectionStates.test_state_2_listening_when_running` | `crates/hatate-gtk/src/mcp_audit_view.rs:789`; `crates/hatate-gtk/tests/smoke.rs:7587` (a real server on a free port) |
| `test_mcp_settings_tab.py:167` `TestConnectionStates.test_state_3_mcp_package_not_installed` | `crates/hatate-gtk/src/mcp_audit_view.rs:799` (the card text). The state is **unreachable** in this build (D-B) |
| `test_mcp_settings_tab.py:174` `TestConnectionStates.test_state_4_no_token` | `crates/hatate-gtk/src/mcp_audit_view.rs:809`; `crates/hatate-gtk/tests/smoke.rs:7481` (a blank token against a real controller) |
| `test_mcp_settings_tab.py:179` `TestConnectionStates.test_state_5_port_busy` | `crates/hatate-gtk/src/mcp_audit_view.rs:821`; `crates/hatate-gtk/tests/smoke.rs:7457` (a real held port; its remedy names Restart server) |
| `test_mcp_settings_tab.py:186` `TestConnectionStates.test_state_6_honest_fallback_shows_open_log_button` | `crates/hatate-gtk/src/mcp_audit_view.rs:832` and `crates/hatate-gtk/src/mcp_audit_view.rs:834` (the text and that the button shows). **Not asserted:** that a click opens the log viewer (`crates/hatate-gtk/src/settings_dialog.rs:514`, one line); no scripted server fails in an unnamed way |
| `test_mcp_settings_tab.py:197` `TestRestartServer.test_restart_uses_the_typed_port_and_token_not_the_saved_ones` | `crates/hatate-gtk/tests/smoke.rs:7694` then `crates/hatate-gtk/tests/smoke.rs:7705` (the server listens on the typed port, the saved one is unchanged); the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) (`m_press` Restart) |
| `test_mcp_settings_tab.py:226` `TestAuditOutcomeFormatting.test_refused_call` | `crates/hatate-gtk/src/mcp_audit_view.rs:531` |
| `test_mcp_settings_tab.py:232` `TestAuditOutcomeFormatting.test_dry_run` | `crates/hatate-gtk/src/mcp_audit_view.rs:544` |
| `test_mcp_settings_tab.py:239` `TestAuditOutcomeFormatting.test_successful_hydrus_send_gets_the_trailing_glyph` | `crates/hatate-gtk/src/mcp_audit_view.rs:558` |
| `test_mcp_settings_tab.py:247` `TestAuditOutcomeFormatting.test_refused_hydrus_send_has_no_glyph` | `crates/hatate-gtk/src/mcp_audit_view.rs:561` |
| `test_mcp_settings_tab.py:252` `TestAuditOutcomeFormatting.test_select_candidate` | `crates/hatate-gtk/src/mcp_audit_view.rs:586` |
| `test_mcp_settings_tab.py:258` `TestAuditOutcomeFormatting.test_toggle_reviewed` | `crates/hatate-gtk/src/mcp_audit_view.rs:589`, `crates/hatate-gtk/src/mcp_audit_view.rs:592` |
| `test_mcp_settings_tab.py:264` `TestAuditOutcomeFormatting.test_remove_row_and_reset_result` | `crates/hatate-gtk/src/mcp_audit_view.rs:599`, `crates/hatate-gtk/src/mcp_audit_view.rs:607` |
| `test_mcp_settings_tab.py:276` `TestTargetCell.test_row_and_candidate` | `crates/hatate-gtk/src/mcp_audit_view.rs:655` |
| `test_mcp_settings_tab.py:279` `TestTargetCell.test_row_only` | `crates/hatate-gtk/src/mcp_audit_view.rs:659` |
| `test_mcp_settings_tab.py:282` `TestTargetCell.test_no_row_is_an_em_dash` | `crates/hatate-gtk/src/mcp_audit_view.rs:667` |
| `test_mcp_settings_tab.py:287` `TestTimeCell.test_a_past_date_shows_the_month_and_day` | `crates/hatate-gtk/src/mcp_audit_view.rs:674`, `crates/hatate-gtk/src/mcp_audit_view.rs:676` |
| `test_mcp_settings_tab.py:290` `TestTimeCell.test_an_unparseable_timestamp_is_shown_verbatim_not_raised` | `crates/hatate-gtk/src/mcp_audit_view.rs:691` |
| `test_mcp_settings_tab.py:305` `TestReadAuditLog.test_missing_file_is_empty_not_an_error` | `crates/hatate-gtk/src/mcp_audit_view.rs:702` |
| `test_mcp_settings_tab.py:308` `TestReadAuditLog.test_a_truncated_final_line_is_dropped_not_fatal` | `crates/hatate-gtk/src/mcp_audit_view.rs:709`, `crates/hatate-gtk/tests/smoke.rs:7793` (a log mid-write through the live table) |
| `test_mcp_settings_tab.py:341` `TestAuditTableFiltering.test_actions_filter_hides_reads_by_default` | `crates/hatate-gtk/tests/smoke.rs:7780` (Actions hides the read); `crates/hatate-gtk/src/mcp_audit_view.rs:735` |
| `test_mcp_settings_tab.py:347` `TestAuditTableFiltering.test_all_calls_filter_shows_everything` | `crates/hatate-gtk/tests/smoke.rs:7786`; `crates/hatate-gtk/src/mcp_audit_view.rs:737` |
| `test_mcp_settings_tab.py:352` `TestAuditTableFiltering.test_newest_first` | `crates/hatate-gtk/tests/smoke.rs:7786`; `crates/hatate-gtk/src/mcp_audit_view.rs:730` |
| `test_type_voice.py:507` `TestBareWidgets.test_settings_dialog_helper_buttons_keep_sentence_case` | **not asserted.** GTK buttons keep the case they are given and the pill CSS (`theme.rs`) does not upper-case them; the shots `settings-mcp-running-*` show `Restart server` and `Generate new token` in sentence case |
| `test_mcp_audit.py` `TestAppending` (6 tests, lines 27, 32, 40, 44, 54, 61) | `docs/mcp-server.md` (Python test mapping, `TestAppending`): `audit::tests::creates_the_file_and_its_parent_and_writes_one_object_per_line`, `a_refusal_a_dry_run_and_a_candidate_index_are_logged`, `iso_format_matches_strftime`, `line_format_is_pythons_json_dumps_with_the_fixed_key_order`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_audit.py` `TestRedaction` (5 tests, lines 80, 89, 97, 106, 113) | `docs/mcp-server.md` (`TestRedaction`): `audit::tests::redaction_covers_every_key_nested_and_case_insensitive`, `secrets_never_reach_the_file`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_audit.py` `TestRotation` (2 tests, lines 129, 146) | `docs/mcp-server.md` (`TestRotation`): `audit::tests::rotates_once_the_cap_is_reached_and_keeps_one_generation`, `a_write_failure_is_swallowed`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestMainThreadInvoker` (2 tests, lines 133, 137) | `docs/mcp-handlers.md` section 6 (`TestMainThreadInvoker`): `queue::tests::a_call_runs_on_the_owners_thread`, `a_call_reaches_the_owner_and_a_failure_is_the_message`, `a_closure_that_runs_longer_than_the_timeout_times_out`; the same-thread test does not apply (D3). This PR adds `queue::tests::an_inbox_runs_marshaled_calls_against_a_model_kept_elsewhere_and_wakes_its_owner` for the window's own channel; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestReadTools` (7 tests, lines 207, 215, 221, 229, 235, 241, 250) | `docs/mcp-handlers.md` section 6 (`TestReadTools`): `list_queue_reports_every_row_by_default`, `list_queue_respects_offset_and_limit`, `list_queue_filter_keyword_unreviewed`, `get_entry_reports_the_requested_row`, `get_entry_out_of_range_reports_an_error_not_a_crash`, `list_actions_reports_every_tool_with_its_gate_state`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestLocalWrites` (3 tests, lines 263, 274, 281) | `docs/mcp-handlers.md` section 6 (`TestLocalWrites`): `select_candidate_switches_the_active_match`, `toggle_reviewed_sets_the_flag_and_local_writes_are_never_gated`; through the window: `crates/hatate-gtk/tests/smoke.rs:7496` and, for the status line, `crates/hatate-gtk/tests/smoke.rs:7501` (red with the line removed, `worktrees/DAN-2222-revert.log` R1); and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestGatingAndAudit` (4 tests, lines 289, 299, 310, 318) | `docs/mcp-handlers.md` section 6 (`TestGatingAndAudit`): `a_disabled_tier_is_refused_with_the_setting_named_and_the_entry_survives`, `a_hydrus_write_is_refused_without_calling_hydrus_at_all`, `a_refusal_and_an_allowed_call_are_both_audited`; through the window: `crates/hatate-gtk/tests/smoke.rs:7520`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestDryRun` (4 tests, lines 327, 340, 348, 360) | `docs/mcp-handlers.md` section 6 (`TestDryRun`): `dry_run_changes_nothing_says_so_and_is_audited`, `turning_dry_run_off_performs_the_real_action`, `read_tools_ignore_dry_run`; through the window, the wire differential's gate state 2 (all on with dry run) and 4 (dry run with all tiers off); and the wire differential (section 5) drives these tools through the window's own server |

## 5. The differentials

Harness (outside the repo, `worktrees/DAN-2222-tools`): `gen_m.py`/`gen_w.py` (the generators, seeded) -> `py_driver_m.py`
(the real `SettingsDialog` and `MainWindow.action_open_settings` at `76c85c4`, under the Hatate venv python: the system python
has no `mcp` package and the Python server cannot start without it; a network namespace with only loopback) -> the Rust
driver (`crates/hatate-gtk/tests/differential.rs`, test only, `kind:"mcp"`) -> `cmp.py`. Log: `worktrees/DAN-2222-differential.log`.

**Action differential.** After every step: `config.json` bytes, every control value of both pages, the Connection card
text, the token and dry-run hints, the conflict line, whether the server runs, the dialogs (text, buttons, default) and the
window digest of 6.g. Steps: the server box and its Got it box, the port (1024, 65535, a held port), token typing, Generate,
Copy and Restart, the four tiers, dry run, audit size, shortcut set, clear and reset, OK refused on a conflict, OK, Cancel and
a second open. Set aside, counted: the Queue header keys (6.d, 6.e), the log path (a temp root), and the random Generate
token (compared by its 43-character urlsafe shape).

| Seed | Cases | Steps | Differing steps | Differences | Errors |
|---|---|---|---|---|---|
| 1 | 40 | 662 | 0 | 0 | 0 |
| 2 | 40 | 640 | 0 | 0 | 0 |
| 3 | 40 | 675 | 0 | 0 | 0 |

Run at heads `007db71` (seed 1) and `d57204a` (seeds 2 and 3); the changes between them are the two window fixes of D-F
and rustfmt, and the wire run below is at `d57204a`. The first pass of seed 1 showed 133 differences, all faults of the
Python driver (it built a second dialog when OK had refused, and typed tokens without `textEdited`); fixed in the driver.
**Negative control:** `CONFLICT_TAIL` losing its final `.` turned a 6-case run red (1 difference, class
`.dialogs[].text`); reverted, 6 cases clean.

**Wire differential** (5.d's method against the server the window started; the Python side is the real
`McpServerController` plus `McpToolHandlers` inside the window the driver starts): raw HTTP/1.1 to `/mcp` while the main
loop turns. Each case: initialize, initialized, tools/list, all 14 tools in seeded order with seeded arguments (some bad),
19 refusals (no, wrong and wrong-scheme token, bad `Host`, bad `Origin`, `PUT`, bad content type, no `Accept`, unknown tool,
bad arguments, unknown method, bad JSON, unknown session, `DELETE` then a call), over five gate states. The audit file is
compared line by line, timestamps set aside.

| Seed | Cases | Steps | Differing steps | Differences |
|---|---|---|---|---|
| 1 @ `d57204a` | 30 | 1302 | 0 | 0 |

Before the fix of D-F the same seed gave 80 differences in 5 cases (`remove_row`) and 230 in 30 (`toggle_reviewed`); the
re-run of the 6 differing cases after the fix gave 0. Set aside and counted (D-D, D-E): the `Server` header, `serverInfo.version`,
and the library words of a refused Hydrus connection.
