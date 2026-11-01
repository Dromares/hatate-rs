# Settings > MCP, the Shortcuts tab and the server's lifecycle (Phase 6.i)

Written while the rewrite was work in progress (0.0.x); the current status is in `README.md`. This is the doc of [DAN-2222](/DAN/issues/DAN-2222).
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
| The six Test buttons and Fetch from Hydrus | 6.h-2 | ported by 6.h-2 (`docs/gtk-6h2.md`) |
| The queue table header keys | 6.d, 6.e | set aside in the comparer, as in 6.g |

## 2. Differences from Python

| Id | What differs | Why | Pinned by |
|---|---|---|---|
| **D-A** | A failed start shows `last_error_user_message()` where Python shows the raw `last_error`; the raw text only picks the state. | The ticket orders it (5.d F2: no token character in a message). | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_failure_card_shows_the_user_message_never_the_raw_text` `assert!(!card.primary.contains("latin-1") && !card.primary.contains("position"));` |
| **D-B** | Connection state 3 (the `mcp` package is not installed) cannot occur. | The server is part of the build, as in `docs/mcp-server.md`. | the card text is pinned, `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(missing.state, Connection::NotInstalled);` |
| **D-C** | The `QKeySequenceEdit` is a button that records the next key (a modifier alone does not end it; Delete is recorded as `Del`, Backspace as `Backspace`; clearing is the Clear button). | GTK has no such widget. The spelling is Qt's (`Shift+N`, `Ctrl+Shift+A`, `Del`). | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(edit.value(), "Shift+Q");`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(edit.value(), "Del");`, `shortcuts_page.rs` `text_is_written_the_way_a_key_sequence_writes_it` |
| **D-D** (C1) | When Hydrus refuses the connection, the library words after `Could not reach Hydrus at <url>: ` differ (27 steps over the wire seed). | The accepted class C1 (DAN-2126, DAN-2133), `docs/hydrus-client.md`. | the comparer's `norm_c1` (`cmp.py`), which accepts only that tail |
| **D-E** | The `Server: uvicorn` header (1212 steps) and `serverInfo.version` (60 steps) differ. | Not compared and granted, `docs/mcp-server.md` (V1). | counted by the comparer, not hidden |
| **D-F** | After a tool's `toggle_reviewed` or `remove_row` the status line says what the window's own click says. Not a difference from Python: the wire differential found the Rust window leaving the line alone (80 and 230 steps) and it was fixed here (`mcp_bridge.rs` `render_queue_events`, `QueueEvent::ReviewedChanged`). | A defect of this ticket's own code. | `crates/hatate-gtk/tests/smoke.rs` `fn mcp_tool_calls_change_the_list_and_the_table_follows` `"Marked 1 image(s) as reviewed \u{2014} 1 still to review"` and `crates/hatate-gtk/tests/smoke.rs` `fn mcp_tool_calls_change_the_list_and_the_table_follows` `assert_eq!(f.say(), "Removed 1 file(s) from the list");`, each red with its line removed (`worktrees/DAN-2222-revert.log` R1, R2) |

## 3. Method map (`gui/` at `76c85c4`)

| Python | Rust |
|---|---|
| gui/settings_dialog.py:52 `_read_mcp_audit_entries` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn read_entries` `pub fn read_entries(path: &Path) -> Vec<Value> {` |
| gui/settings_dialog.py:77 `_mcp_time_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn time_cell` `pub fn time_cell(timestamp: &str, now: i64) -> String {` |
| gui/settings_dialog.py:91 `_mcp_target_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn target_cell` `pub fn target_cell(entry: &Value) -> String {` |
| gui/settings_dialog.py:101 `_mcp_outcome_weight` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn outcome_weight` `pub fn outcome_weight(entry: &Value) -> Weight {` |
| gui/settings_dialog.py:125 `_mcp_outcome_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn outcome_cell` `pub fn outcome_cell(entry: &Value) -> (String, Weight) {` |
| gui/settings_dialog.py:166 `_mcp_outcome_tooltip` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn outcome_tooltip` `pub fn outcome_tooltip(entry: &Value) -> String {` |
| gui/settings_dialog.py:2075 `_build_shortcuts_tab` | `crates/hatate-gtk/src/shortcuts_page.rs` `fn build` `pub fn build(overrides: &OrderedMap<String>) -> Rc<ShortcutsPage> {` |
| gui/settings_dialog.py:2175 `_current_shortcut_bindings` | `crates/hatate-gtk/src/shortcuts_page.rs` `fn bindings` `pub fn bindings(&self) -> OrderedMap<String> {` |
| gui/settings_dialog.py:2181 `_reset_shortcuts_to_defaults` | `crates/hatate-gtk/src/shortcuts_page.rs` `fn reset_to_defaults` `pub fn reset_to_defaults(&self) {` |
| gui/settings_dialog.py:2188 `_refresh_shortcut_conflicts` | `crates/hatate-gtk/src/shortcuts_page.rs` `fn refresh_conflicts` `pub fn refresh_conflicts(&self) -> bool {` |
| gui/settings_dialog.py:2212 `_build_mcp_tab` | `crates/hatate-gtk/src/settings_mcp.rs` `const PAGE` `pub const PAGE: Node = Node::Column(&[` (the page as data) and `crates/hatate-gtk/src/settings_dialog.rs` `fn wire_mcp` `fn wire_mcp(self: &Rc<Self>) {` (its handlers) |
| gui/settings_dialog.py:2311 `_build_mcp_connection_card` | `crates/hatate-gtk/src/settings_mcp.rs` `const PAGE` `pub const PAGE: Node = Node::Column(&[` and `crates/hatate-gtk/src/mcp_audit_view.rs` `fn connection` `pub fn connection(enabled: bool, port: i64, server: Option<&ServerReport>) -> ConnectionText {` (the six states) |
| gui/settings_dialog.py:2331 `_build_mcp_always_available_group`, :2346 `_build_mcp_tier_checkbox_group` | `crates/hatate-gtk/src/settings_mcp.rs` `const PAGE` `pub const PAGE: Node = Node::Column(&[` |
| gui/settings_dialog.py:2386 `_copy_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs` `fn copy_mcp_token` `pub fn copy_mcp_token(&self) {` |
| gui/settings_dialog.py:2391 `_generate_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs` `fn generate_mcp_token` `pub fn generate_mcp_token(&self) {` |
| gui/settings_dialog.py:2398 `_refresh_mcp_dry_run_hint` | `crates/hatate-gtk/src/settings_dialog.rs` `fn refresh_mcp_dry_run_hint` `fn refresh_mcp_dry_run_hint(&self) {`, `crates/hatate-gtk/src/settings_mcp.rs` `fn dry_run_text` `pub fn dry_run_text(on: bool) -> (&'static str, Weight) {` |
| gui/settings_dialog.py:2412 `_on_mcp_enabled_toggled`, :2417 `_show_mcp_enable_summary` | `crates/hatate-gtk/src/settings_dialog.rs` `fn show_mcp_enable_summary` `fn show_mcp_enable_summary(&self) {`, `crates/hatate-gtk/src/settings_mcp.rs` `fn enable_summary` `pub fn enable_summary(hydrus_writes: bool, research: bool, destructive: bool) -> String {`, the toggle handler in `crates/hatate-gtk/src/settings_dialog.rs` `fn wire_mcp` `fn wire_mcp(self: &Rc<Self>) {` |
| gui/settings_dialog.py:2454 `_mcp_server_handle` | `crates/hatate-gtk/src/mcp_bridge.rs` `fn mcp_report` `pub fn mcp_report(&self) -> ServerReport {` |
| gui/settings_dialog.py:2457 `_mcp_restart_server` | `crates/hatate-gtk/src/settings_dialog.rs` `fn restart_mcp_server` `pub fn restart_mcp_server(&self) {` |
| gui/settings_dialog.py:2474 `_refresh_mcp_connection` | `crates/hatate-gtk/src/settings_dialog.rs` `fn refresh_mcp_connection` `pub fn refresh_mcp_connection(&self) {` |
| gui/settings_dialog.py:2536 `_build_mcp_audit_card`, :2594 `_on_mcp_audit_filter_changed`, :2605 `_open_mcp_audit_folder` | `crates/hatate-gtk/src/settings_mcp.rs` `fn build` `pub fn build(` |
| gui/settings_dialog.py:2597 `_on_mcp_audit_follow_toggled` | `crates/hatate-gtk/src/settings_mcp.rs` `fn sync_timer` `fn sync_timer(&self) {` |
| gui/settings_dialog.py:2620 `_refresh_mcp_audit_table` | `crates/hatate-gtk/src/settings_mcp.rs` `fn refresh` `pub fn refresh(&self) {` |
| gui/settings_dialog.py `apply_to_settings` and the set half of `__init__`, MCP fields and `review_shortcuts` | `crates/hatate-gtk/src/settings_values.rs` `fn initial` `v.set("mcp_allow_research", Value::Bool(s.mcp.allow_research));` |
| `gui/main_window.py:421` `__init__`, lines 531-542 (build `McpToolHandlers` and `McpServerController`, start when enabled) | `crates/hatate-gtk/src/window.rs` `fn new` `window.start_mcp_at_launch();`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn start_mcp_at_launch` `pub fn start_mcp_at_launch(&self) {`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn mcp_handlers` `fn mcp_handlers(&self) -> Result<Arc<dyn McpHandlers>, String> {` |
| `gui/main_window.py:4047` `action_open_settings`, lines 4050-4077 (rebind only when enabled, port or token changed) | `crates/hatate-gtk/src/settings_dialog.rs` `fn apply_settings_dialog` `pub fn apply_settings_dialog(&self, values: &Values, da_snapshot: &str) {`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn restart_mcp` `pub fn restart_mcp(&self, settings: &McpSettings) -> bool {` |
| `gui/main_window.py:4139` `closeEvent`, lines 4143-4148 (stop the server first) | `crates/hatate-gtk/src/session_ops.rs` `fn close_teardown` `self.stop_mcp();`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn stop_mcp` `pub fn stop_mcp(&self) {` |
| `gui/mcp_bridge.py:60` `MainThreadTimeout`, `:64` `MainThreadInvoker` (`__init__`, `_run`, `call`, `shutdown`) | `crates/hatate-core/src/queue.rs` `struct QueueInbox` `pub struct QueueInbox {` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs` `fn channel` `pub fn channel(wake: impl Fn() + Send + Sync + 'static) -> (QueueInbox, QueueChannel) {`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs` `fn pump_mcp` `pub fn pump_mcp(&self) {`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn attach_mcp` `pub fn attach_mcp(&self, wake: async_channel::Receiver<()>) {` |
| `gui/mcp_bridge.py:137` `McpToolHandlers` (`__init__`, `_mcp_settings`, `_run_tool`, `_entry_at`) and the tools `list_queue` :209, `get_entry` :234, `get_candidates` :247, `get_images` :277, `get_diff` :311, `list_actions` :343, `select_candidate` :363, `toggle_reviewed` :383, `send_upload` :465, `send_url` :490, `download_send` :526, `remove_row` :558, `reset_result` :573; `_queue_row` :596, `_entry_detail` :608, `_matches_filter` :635, `_guess_mime` :664 | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs` `fn render_queue_events` `fn render_queue_events(&self, events: &[QueueEvent]) {` turns the queue's events into the table, selection and status line (the `remove_row` and `toggle_reviewed` status lines are pinned, see section 2) |
| `gui/mcp_bridge.py:400` `research`, `:409` `_perform_research`, `:449` `read_results` | `crates/hatate-gtk/src/mcp_bridge.rs` `fn start_pending_researches` `fn start_pending_researches(&self) {`, `crates/hatate-gtk/src/mcp_bridge.rs` `fn release_mcp_research` `pub(crate) fn release_mcp_research(&self) {` (the window starts its own search and the tool goes on when it ends) |

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
| `test_gui_smoke.py:5607` `TestShortcutsSettingsTab.test_it_offers_every_registered_action` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(page.edits.len(), REVIEW_ACTIONS.len());`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(page.edit(a.id).unwrap().value(), a.default, "{}", a.id);` (every registered id has an editor) |
| `test_gui_smoke.py:5611` `TestShortcutsSettingsTab.test_only_changed_bindings_are_persisted` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `("prev_row".to_owned(), "".to_owned())` (OK saves only `next_row` and `prev_row`, the two that differ), `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(json["review_shortcuts"]["next_row"], "F9");` |
| `test_gui_smoke.py:5619` `TestShortcutsSettingsTab.test_clearing_a_binding_is_remembered_as_cleared` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(json["review_shortcuts"]["prev_row"], "");` (a cleared key is saved as `""`), `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `page.clears[1].emit_clicked();` |
| `test_gui_smoke.py:5626` `TestShortcutsSettingsTab.test_a_duplicate_key_is_named_rather_than_resolved_silently` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `"Same key on more than one action - Qt fires neither, so these would both stop working:\nX is bound to Next image and Previous image"` (both names in the line) |
| `test_gui_smoke.py:5635` `TestShortcutsSettingsTab.test_ok_is_refused_while_two_actions_share_a_key` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `"the dialog stays open on a conflict"`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(shown.title, "Shortcut conflict");`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `"nothing was saved"` |
| `test_gui_smoke.py:5646` `TestShortcutsSettingsTab.test_ok_goes_through_once_the_clash_is_resolved` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert!(f.w.s.settings_dialog.borrow().is_none());` (OK closes the dialog once the clash is cleared) |
| `test_gui_smoke.py:5654` `TestShortcutsSettingsTab.test_reset_puts_every_default_back` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `page.reset.emit_clicked();` and the loop after it (`crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(page.edit(a.id).unwrap().value(), a.default, "{}", a.id);`) |
| `test_mcp_settings_tab.py:65` `TestConstructionWithNoMainWindow.test_builds_without_a_parent` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_page_states_the_server_and_follows_the_boxes` `assert_eq!(d.live_text("mcp_connection_primary"), "Stopped.");` (a window with the server off opens on `Stopped.`). The no-parent half does not apply: the Rust dialog is always built over its main window (6.h) |
| `test_mcp_settings_tab.py:70` `TestConstructionWithNoMainWindow.test_enabled_defaults_off_and_hydrus_writes_defaults_on` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_enable_summary_fires_on_a_real_change_only` `.contains("Send files and tags to your live Hydrus library")` and the two `!shown.text.contains` lines after it (opening from defaults and ticking the server lists Hydrus writes and neither of the other two: **indirect**, no test reads the three boxes' initial state directly); the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) opens the dialog from defaults in every case and compares every control's value |
| `test_mcp_settings_tab.py:91` `TestApplyToSettings.test_round_trips_every_mcp_field` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server` `assert_eq!(saved.port, i64::from(other));` to `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server` `assert_eq!(saved.audit_log_max_bytes, 5_000_000);` (port, token, dry run, research, enabled, destructive off, Hydrus writes on, audit size at its default) and `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server` `assert_eq!(json["mcp"]["port"], i64::from(other));` (`config.json`). Hydrus writes switched **off** and a changed audit size are not asserted in a smoke test; the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) sets every one of the eight fields and compares `config.json` as bytes |
| `test_mcp_settings_tab.py:120` `TestEnableSummaryConstructionSafety.test_does_not_fire_when_restoring_a_saved_enabled_state` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_enable_summary_fires_on_a_real_change_only` `f.script.count_shown(),` (`opening on a saved enabled state shows nothing`) |
| `test_mcp_settings_tab.py:132` `TestEnableSummaryConstructionSafety.test_fires_exactly_once_on_a_real_off_to_on_click` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_enable_summary_fires_on_a_real_change_only` `assert_eq!(f.script.count_shown(), 1);` (one box after off then on) and `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_enable_summary_fires_on_a_real_change_only` `assert_eq!(f.script.count_shown(), 0);` (none for on to off) |
| `test_mcp_settings_tab.py:156` `TestConnectionStates.test_state_1_stopped_when_not_enabled` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(stopped.state, Connection::Stopped);`, `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert!(!stopped.show_restart);`; `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_page_states_the_server_and_follows_the_boxes` `assert_eq!(d.live_text("mcp_connection_primary"), "Stopped.");`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_page_states_the_server_and_follows_the_boxes` `assert!(!d.control_visible("mcp_restart"));` |
| `test_mcp_settings_tab.py:162` `TestConnectionStates.test_state_2_listening_when_running` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(live.state, Connection::Listening);`; `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_page_states_the_server_and_follows_the_boxes` `format!("Listening on 127.0.0.1:{port}.")` (a real server on a free port) |
| `test_mcp_settings_tab.py:167` `TestConnectionStates.test_state_3_mcp_package_not_installed` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(missing.state, Connection::NotInstalled);` (the card text). The state is **unreachable** in this build (D-B) |
| `test_mcp_settings_tab.py:174` `TestConnectionStates.test_state_4_no_token` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(blank.state, Connection::NoToken);`; `crates/hatate-gtk/tests/smoke.rs` `fn mcp_server_stays_off_when_disabled_and_reports_a_held_port_and_a_blank_token` `assert_eq!(card.state, Connection::NoToken);` (a blank token against a real controller) |
| `test_mcp_settings_tab.py:179` `TestConnectionStates.test_state_5_port_busy` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(busy.state, Connection::PortBusy);`; `crates/hatate-gtk/tests/smoke.rs` `fn mcp_server_stays_off_when_disabled_and_reports_a_held_port_and_a_blank_token` `assert_eq!(card.state, Connection::PortBusy);` (a real held port; its remedy names Restart server) |
| `test_mcp_settings_tab.py:186` `TestConnectionStates.test_state_6_honest_fallback_shows_open_log_button` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert_eq!(other.state, Connection::Failed);` and `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_six_states_read_as_the_spec_says` `assert!(other.remedy.is_empty() && other.show_open_log);` (the text and that the button shows). **Not asserted:** that a click opens the log viewer (`crates/hatate-gtk/src/settings_dialog.rs` `fn wire_mcp` `d.main.action_view_logs();`, one line); no scripted server fails in an unnamed way |
| `test_mcp_settings_tab.py:197` `TestRestartServer.test_restart_uses_the_typed_port_and_token_not_the_saved_ones` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server` `d.press("mcp_restart");` then `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server` `assert_eq!(f.w.s.settings.lock().unwrap().mcp.port, i64::from(port));` (the server listens on the typed port, the saved one is unchanged); the action differential (`crates/hatate-gtk/tests/differential.rs`, `kind:"mcp"`, ops `m_press m_clear m_reset hold_port release_port mcp_digest`) (`m_press` Restart) |
| `test_mcp_settings_tab.py:226` `TestAuditOutcomeFormatting.test_refused_call` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_refused_call_names_its_tier_and_is_drawn_at_ink_100` `assert_eq!(` |
| `test_mcp_settings_tab.py:232` `TestAuditOutcomeFormatting.test_dry_run` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_dry_run_is_recorded_not_run_at_ink_100` `assert_eq!(` |
| `test_mcp_settings_tab.py:239` `TestAuditOutcomeFormatting.test_successful_hydrus_send_gets_the_trailing_glyph` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_successful_hydrus_send_ends_in_the_sent_glyph_and_a_refused_one_does_not` `assert_eq!(text, "Allowed \u{2014} sent  \u{25cf}");` |
| `test_mcp_settings_tab.py:247` `TestAuditOutcomeFormatting.test_refused_hydrus_send_has_no_glyph` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_successful_hydrus_send_ends_in_the_sent_glyph_and_a_refused_one_does_not` `assert!(!cell(refused).0.contains('\u{25cf}'));` |
| `test_mcp_settings_tab.py:252` `TestAuditOutcomeFormatting.test_select_candidate` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_local_decisions_read_as_the_spec_table_says` `assert_eq!(cell(pick).0, "Allowed \u{2014} picked candidate 2");` |
| `test_mcp_settings_tab.py:258` `TestAuditOutcomeFormatting.test_toggle_reviewed` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_local_decisions_read_as_the_spec_table_says` `assert_eq!(cell(reviewed).0, "Allowed \u{2014} marked reviewed");`, `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_local_decisions_read_as_the_spec_table_says` `assert_eq!(cell(unreviewed).0, "Allowed \u{2014} marked unreviewed");` |
| `test_mcp_settings_tab.py:264` `TestAuditOutcomeFormatting.test_remove_row_and_reset_result` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_local_decisions_read_as_the_spec_table_says` `"Allowed \u{2014} row removed"`, `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_local_decisions_read_as_the_spec_table_says` `"Allowed \u{2014} result reset"` |
| `test_mcp_settings_tab.py:276` `TestTargetCell.test_row_and_candidate` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_target_is_the_row_and_candidate_the_row_or_a_dash` `assert_eq!(` |
| `test_mcp_settings_tab.py:279` `TestTargetCell.test_row_only` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_target_is_the_row_and_candidate_the_row_or_a_dash` `assert_eq!(` |
| `test_mcp_settings_tab.py:282` `TestTargetCell.test_no_row_is_an_em_dash` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_target_is_the_row_and_candidate_the_row_or_a_dash` `assert_eq!(target_cell(&json!({})), "\u{2014}");` |
| `test_mcp_settings_tab.py:287` `TestTimeCell.test_a_past_date_shows_the_month_and_day` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_past_date_shows_the_month_and_day_and_today_only_the_time` `assert_eq!(time_cell("2020-01-02T03:04:05Z", now), "Jan 2, 03:04:05");`, `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_past_date_shows_the_month_and_day_and_today_only_the_time` `assert_eq!(time_cell("2023-11-13T23:59:59Z", now), "Nov 13, 23:59:59");` |
| `test_mcp_settings_tab.py:290` `TestTimeCell.test_an_unparseable_timestamp_is_shown_verbatim_not_raised` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_timestamp_strptime_would_refuse_is_shown_as_it_came` `assert_eq!(time_cell(bad, 0), bad);` |
| `test_mcp_settings_tab.py:305` `TestReadAuditLog.test_missing_file_is_empty_not_an_error` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_missing_log_is_empty_and_a_truncated_last_line_is_dropped` `assert!(read_entries(&path).is_empty());` |
| `test_mcp_settings_tab.py:308` `TestReadAuditLog.test_a_truncated_final_line_is_dropped_not_fatal` | `crates/hatate-gtk/src/mcp_audit_view.rs` `fn a_missing_log_is_empty_and_a_truncated_last_line_is_dropped` `assert_eq!(entries.len(), 1);`, `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_audit_table_follows_the_log_newest_first` `assert_eq!(d.audit.rows().len(), 3);` (a log mid-write through the live table) |
| `test_mcp_settings_tab.py:341` `TestAuditTableFiltering.test_actions_filter_hides_reads_by_default` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_audit_table_follows_the_log_newest_first` `assert_eq!(tools, ["select_candidate", "toggle_reviewed"], "{rows:?}");` (Actions hides the read); `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_table_is_newest_first_filtered_to_actions_and_capped_with_a_caption` `actions.caption,` |
| `test_mcp_settings_tab.py:347` `TestAuditTableFiltering.test_all_calls_filter_shows_everything` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_audit_table_follows_the_log_newest_first` `assert_eq!(tools, ["select_candidate", "toggle_reviewed", "list_queue"]);`; `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_table_is_newest_first_filtered_to_actions_and_capped_with_a_caption` `"Showing the most recent {DISPLAY_LIMIT} of {} calls.",` |
| `test_mcp_settings_tab.py:352` `TestAuditTableFiltering.test_newest_first` | `crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_audit_table_follows_the_log_newest_first` `assert_eq!(tools, ["select_candidate", "toggle_reviewed", "list_queue"]);`; `crates/hatate-gtk/src/mcp_audit_view.rs` `fn the_table_is_newest_first_filtered_to_actions_and_capped_with_a_caption` `actions.rows[0].target,` |
| `test_type_voice.py:507` `TestBareWidgets.test_settings_dialog_helper_buttons_keep_sentence_case` | **not asserted.** GTK buttons keep the case they are given and the pill CSS (`theme.rs`) does not upper-case them; the shots `settings-mcp-running-*` show `Restart server` and `Generate new token` in sentence case |
| `test_mcp_audit.py` `TestAppending` (6 tests, lines 27, 32, 40, 44, 54, 61) | `docs/mcp-server.md` (Python test mapping, `TestAppending`): `audit::tests::creates_the_file_and_its_parent_and_writes_one_object_per_line`, `a_refusal_a_dry_run_and_a_candidate_index_are_logged`, `iso_format_matches_strftime`, `line_format_is_pythons_json_dumps_with_the_fixed_key_order`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_audit.py` `TestRedaction` (5 tests, lines 80, 89, 97, 106, 113) | `docs/mcp-server.md` (`TestRedaction`): `audit::tests::redaction_covers_every_key_nested_and_case_insensitive`, `secrets_never_reach_the_file`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_audit.py` `TestRotation` (2 tests, lines 129, 146) | `docs/mcp-server.md` (`TestRotation`): `audit::tests::rotates_once_the_cap_is_reached_and_keeps_one_generation`, `a_write_failure_is_swallowed`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestMainThreadInvoker` (5 tests, lines 111, 118, 124, 133, 137) | `docs/mcp-handlers.md` section 6 (`TestMainThreadInvoker`): `queue::tests::a_call_runs_on_the_owners_thread`, `a_call_reaches_the_owner_and_a_failure_is_the_message`, `a_closure_that_runs_longer_than_the_timeout_times_out`; the same-thread test does not apply (D3). This PR adds `queue::tests::an_inbox_runs_marshaled_calls_against_a_model_kept_elsewhere_and_wakes_its_owner` for the window's own channel; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestReadTools` (7 tests, lines 207, 215, 221, 229, 235, 241, 250) | `docs/mcp-handlers.md` section 6 (`TestReadTools`): `list_queue_reports_every_row_by_default`, `list_queue_respects_offset_and_limit`, `list_queue_filter_keyword_unreviewed`, `get_entry_reports_the_requested_row`, `get_entry_out_of_range_reports_an_error_not_a_crash`, `list_actions_reports_every_tool_with_its_gate_state`; and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestLocalWrites` (3 tests, lines 263, 274, 281) | `docs/mcp-handlers.md` section 6 (`TestLocalWrites`): `select_candidate_switches_the_active_match`, `toggle_reviewed_sets_the_flag_and_local_writes_are_never_gated`; through the window: `crates/hatate-gtk/tests/smoke.rs` `fn mcp_tool_calls_change_the_list_and_the_table_follows` `assert_eq!(answered["reviewed"], true, "{answered}");` and, for the status line, `crates/hatate-gtk/tests/smoke.rs` `fn mcp_tool_calls_change_the_list_and_the_table_follows` `"Marked 1 image(s) as reviewed \u{2014} 1 still to review"` (red with the line removed, `worktrees/DAN-2222-revert.log` R1); and the wire differential (section 5) drives these tools through the window's own server |
| `test_mcp_bridge.py` `TestGatingAndAudit` (4 tests, lines 289, 299, 310, 318) | `docs/mcp-handlers.md` section 6 (`TestGatingAndAudit`): `a_disabled_tier_is_refused_with_the_setting_named_and_the_entry_survives`, `a_hydrus_write_is_refused_without_calling_hydrus_at_all`, `a_refusal_and_an_allowed_call_are_both_audited`; through the window: `crates/hatate-gtk/tests/smoke.rs` `fn mcp_tool_calls_change_the_list_and_the_table_follows` `assert_eq!(refused["allowed"], false, "{refused}");`; and the wire differential (section 5) drives these tools through the window's own server |
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

## 6. Minimum size, shots and contrast

Hosted `ci` run 41039450112 at `169f87c587bd0baa519ec5eda332294e87691fe3` (GTK 4.14 / libadwaita 1.5), artifact `shots`: every number below is read
from that artifact, not from the author's host (GTK 4.24). The shot set is unchanged by the later commits of this PR (they touch this doc and
`examples/shot.rs`'s `settings-tab-<n>` walk, which writes no PNG that is measured here); the final head's run is cited in the PR comment.

**Minimum size** (DAN-2356): the stated minimum is 560 x 440 (`settings_dialog::MIN_WIDTH/MIN_HEIGHT`). The fit test
`smoke::settings_window_keeps_its_stated_minimum` (`crates/hatate-gtk/tests/smoke.rs` `fn settings_window_keeps_its_stated_minimum` `fn settings_window_keeps_its_stated_minimum(app: &adw::Application) {`) walks every tab the window has, including 7 (Shortcuts) and 8 (MCP),
with and without a Find query. No fix to the pages' size was needed in this PR, so there is no red run to show for it: the PNG header of each
shot below is the test. Read from the header of every file (`worktrees/DAN-2222-tools/mk6.py`):

| State | Default 940 x 660 (dark, light) | At the stated minimum 560 x 440 (dark, light), PNG header |
|---|---|---|
| Shortcuts tab, defaults | `settings-7-dark.png` `settings-7-light.png`: 940 x 660, 940 x 660 | `settings-7-min-dark.png` `settings-7-min-light.png`: 560 x 440, 560 x 440 |
| Shortcuts, recorder waiting for its key | `settings-shortcuts-record-dark.png` `settings-shortcuts-record-light.png`: 940 x 660, 940 x 660 | `settings-shortcuts-record-min-dark.png` `settings-shortcuts-record-min-light.png`: 560 x 440, 560 x 440 |
| Shortcuts, two actions on one key (conflict line) | `settings-shortcuts-conflict-dark.png` `settings-shortcuts-conflict-light.png`: 940 x 660, 940 x 660 | `settings-shortcuts-conflict-min-dark.png` `settings-shortcuts-conflict-min-light.png`: 560 x 440, 560 x 440 |
| MCP tab, server off (Stopped) | `settings-8-dark.png` `settings-8-light.png`: 940 x 660, 940 x 660 | `settings-8-min-dark.png` `settings-8-min-light.png`: 560 x 440, 560 x 440 |
| MCP, server listening | `settings-mcp-running-dark.png` `settings-mcp-running-light.png`: 940 x 660, 940 x 660 | `settings-mcp-running-min-dark.png` `settings-mcp-running-min-light.png`: 560 x 440, 560 x 440 |
| MCP, port held (Can't start) | `settings-mcp-held-dark.png` `settings-mcp-held-light.png`: 940 x 660, 940 x 660 | `settings-mcp-held-min-dark.png` `settings-mcp-held-min-light.png`: 560 x 440, 560 x 440 |
| MCP, Recent tool calls with synthetic rows (All) | `settings-mcp-audit-dark.png` `settings-mcp-audit-light.png`: 940 x 660, 940 x 660 | `settings-mcp-audit-min-dark.png` `settings-mcp-audit-min-light.png`: 560 x 440, 560 x 440 |
| Focus ring on the server box | `settings-focus-mcp-dark.png` `settings-focus-mcp-light.png`: 940 x 660, 940 x 660 | (no minimum-size shot: the ring is shown at the default size) |
| Focus ring on the first key button | `settings-focus-shortcut-dark.png` `settings-focus-shortcut-light.png`: 940 x 660, 940 x 660 | (no minimum-size shot: the ring is shown at the default size) |

0 of 7 minimum-size rows differ from 560 x 440 (14 headers, dark and light).

**Focus rings** (non-text 3:1 floor; `worktrees/DAN-2222-tools/ring.py`: the pixels that changed against the same page's unfocused shot,
inside the control's box; baseline `settings-7` for the first key button and `settings-mcp-running` for the server box):

| Control | scheme | ring px | bg px | ratio (3:1 floor) | region in the shot (x0,y0,x1,y1) |
|---|---|---|---|---|---|
| first key button | dark | #cdc4ba | #0d0d0d | 11.29 pass | (556, 244, 726, 316) |
| first key button | light | #211d17 | #e8e1d2 | 12.88 pass | (556, 244, 726, 316) |
| server box | dark | #cdc4ba | #0d0d0d | 11.29 pass | (40, 150, 260, 190) |
| server box | light | #211d17 | #e8e1d2 | 12.88 pass | (40, 150, 260, 190) |

The recorder's own state (`settings-shortcuts-record`) is a separate shot: the button under recording shows the "Press a key..." word, measured in
the contrast table below.

**Contrast** (extreme-pixel method, `worktrees/DAN-2222-tools/measure.py`, over the 32 Settings shots of this PR's states (7 states and 2 focus states, both schemes, the 7 at both sizes), the label boxes listed in each `.rects`; the full table is `worktrees/DAN-2222-measure.md`, not committed): 764 rows,
764 pass the 4.5:1 text floor, 0 disabled rows, **0 BELOW**. Lowest passing rows: dark 5.24 (`a setting's name - "quota", ...`, the Find
placeholder, `#88827c` on `#090909`), light 5.09 (the same placeholder, `#595348` on `#ddd2bd`); on the 6.i pages themselves the lowest is the
tab strip's unselected tab names: dark 5.26 `#8a847e` on `#0d0d0d`, light 5.50 `#5c574f` on `#e8e1d2`.

**Tab order** (`examples/shot.rs` state `settings-tab-7` and `settings-tab-8`, Tab walked from the top of the window; the walk is
toolkit-ordering only, taken on the author's host, log `worktrees/DAN-2222-tabwalk.log`; no hosted run prints it). From the notebook, which
holds the first stop (its tab strip), Tab goes:

- **Shortcuts** (tab 7): each action's key button then its "Leave ... with no key at all" clear button, 16 actions in the table's order
  (J, K, N, Shift+N, ], [, C, O, H, Return, U, D, F5, R, Space, Del); then Reset all to defaults, Cancel, OK; then the Find box; then the
  notebook again. Nothing is skipped and nothing takes a stop twice.
- **MCP** (tab 8): Enable the MCP server, two entries (the port and the token), Copy token, Generate new token, Dry run, the three tier boxes
  (Hydrus, re-search, remove and reset), the Actions and All calls toggles, the live-refresh toggle, the audit folder button, the audit
  filter, the table, Cancel, OK, then Find. The status lines and labels are not stops.

**Not judgeable from a shot** (pinned by text, not by pixels): the "Got it" enable summary and the "Shortcut conflict" warning are libadwaita
windows of their own, which the Settings window's snapshot does not hold (as for the Lens warning in 6.h). They are pinned by
`crates/hatate-gtk/tests/smoke.rs` `fn settings_mcp_enable_summary_fires_on_a_real_change_only` `fn settings_mcp_enable_summary_fires_on_a_real_change_only(app: &adw::Application) {` (`settings_mcp_enable_summary_fires_on_a_real_change_only`: the summary fires on a real change only and lists the tiers ticked in the dialog)
and `crates/hatate-gtk/tests/smoke.rs` `fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict` `assert_eq!(edit.value(), "Shift+Q");` (title "Shortcut conflict", kind Warning, both action names in the text, the dialog stays open). A shot of those
toplevels is not part of this PR; the design read is asked to judge the box's words from those pins and `crates/hatate-gtk/src/dialogs.rs` `fn message_detailed` `dialog.add_responses(&[("ok", "Got it")]);`.

### 6.1 Round 1 fixup (design read of `866766f`, DAN-2433 comment `dae79788`)

Shots from hosted ci run 41045477236 at `dfaec09` (green, one gate `ci`, ~23 min). Measuring script `worktrees/DAN-2222-tools/measure2.py`
(`measure.py` with the three new state names added to its pattern; extreme-pixel method), output `worktrees/DAN-2222-measure-fix.md`.

| Finding | What changed | Evidence |
|---|---|---|
| **F1** middle of the MCP page in no shot | States `settings-mcp-tiers` (scrolled to the Dry run box), `settings-mcp-tiers-dry` (the same with Dry run ticked, the sentence flips), `settings-mcp-tiers-low` (scrolled to the Hydrus box, through the destructive box), `settings-focus-mcp-tier` (the research tier's check box focused, inside its group box). Each in dark and light, default and minimum size; wired into both `ci.yml` shot loops. | PNG headers of all 10 minimum-size shots read 560 x 440 (0 differ). 1018 labels measured over the new and the old 6.i states, 0 below 4.5:1, lowest 5.09 (the Find placeholder, light). |
| **F2** Outcome clipped at 560 | The page itself was 575 px wide at the minimum: the token box (20 characters) pushed the whole page past the window, and the audit table with it. `mcp_token` is 8 characters wide, the path caption ellipsizes in the middle, and the table's Tool, Tier and Target columns ellipsize (full text in the tooltip) with Outcome kept at 11 characters. The inner scroller no longer scrolls sideways. | `settings-mcp-audit-min-{dark,light}.png`: the right edge of the card is in view, Outcome reads "Recorded, not ...", "Allowed - pick...", "Allowed". Tool and Tier are shortened at 560 ("tog...", "Rev..."), which is the price of the column that matters. Pin: `smoke.rs` `settings_mcp_audit_table_follows_the_log_newest_first` asserts the page needs <= 520 px; it went red at 575 px with the token box back at 20 characters (`worktrees/DAN-2222-fixup-red-R1.log`). |
| **F3** "Keep up to ... MB" off-token | Both labels (and the table's Time, Tool, Tier and Target cells) take the page's `ink-100` label class. | `settings-mcp-audit-{dark,light}`: `Keep up to`, `MB before trimming the oldest entries.` #cdc4ba on #0d0d0d 11.29 (dark), #211d17 on #e8e1d2 12.88 (light); before the fixup #ffffff and #2e2d2a. |

**Contrast of the new states** (rows from `worktrees/DAN-2222-measure-fix.md`; floor 4.5:1 for text, 3:1 for a non-text mark):

| Element | State | fg on bg (dark) | ratio | fg on bg (light) | ratio |
|---|---|---|---|---|---|
| Dry run sentence, Dry run off ("Dry run is off. Allowed actions happen for real ...") | `mcp-tiers` | #cdc4ba on #0d0d0d | 11.29 | #211d17 on #e8e1d2 | 12.88 |
| Dry run sentence, Dry run on ("Dry run is on. The AI can look at everything ...") | `mcp-tiers-dry` | #8a847e on #0d0d0d | 5.26 | #5c574f on #e8e1d2 | 5.50 |
| Group titles ("Always available ...", "On by default ...", "Off by default ...") | `mcp-tiers`, `mcp-tiers-low` | #cdc4ba on #0d0d0d | 11.29 | #211d17 on #e8e1d2 | 12.88 |
| Hydrus-writes live marker "●" (text floor applied, stricter than 3:1) | `mcp-tiers-low` | #8a847e on #0d0d0d | 5.26 | #5c574f on #e8e1d2 | 5.50 |
| Tier hints ("Throws away work ...") | `mcp-tiers-low` | #cdc4ba on #0d0d0d | 11.29 | #211d17 on #e8e1d2 | 12.88 |
| Tier check-box labels ("Let it re-search a stuck entry", the Hydrus and destructive ones) and the "Always" prose | `mcp-tiers`, `mcp-tiers-low` | #ffffff on #0d0d0d | 19.44 | #2e2d2a on #e8e1d2 | 10.58 |
| Focus ring on the research tier's check box (left and right edges of its box, `focus-mcp-tier-min`) | `focus-mcp-tier` | #cdc4ba on #0d0d0d | 11.29 | #211d17 on #e8e1d2 | 12.88 |

The tier check-box labels and the "Always" prose are drawn in the stock label colour (#ffffff dark, #2e2d2a light), as every Settings check box
label is since 6.h; they pass the floor but are not ink tokens. That is the F3 class on a page this ticket did not write, so it is left for
DAN-2319 and named there, not changed here (it would move every Settings page).

**`None` in the audit row:** `mcp_audit_view.rs` prints `None` for `select_candidate` only when the audit entry has no `selected_candidate_index`
key (Python's `outcome.get(...)` prints `None` the same way). The real handler always writes that key (`crates/hatate-core/src/mcp/tool_handlers.rs` `fn select_candidate` `.push("selected_candidate_index", json!(selected)))`), so a real row
cannot print `None`; the synthetic seed in `examples/shot.rs` omitted it and now carries `"selected_candidate_index": 1`.

**Not judgeable from a shot:** unchanged from section 6 (the Got it and Shortcut conflict boxes). The Tab order of the MCP page is the walk of
section 6 (`settings-tab-8`, taken at `866766f`, before this fixup, which adds no widget and no Tab stop); this fixup shows one tier check
box's ring (the research one, `settings-focus-mcp-tier-{dark,light}.png`). The Hydrus and destructive tier boxes' rings were not shot.

### 6.2 Low notes from the DAN-2222 approval (DAN-2436)

Mutations the DAN-2222 verifier found surviving, and the Tool-column note. Each pin below was run red against its mutation
(`worktrees/DAN-2436-mutants.log`); the F2 pins sit in `settings_mcp_low_notes_pins`, which the audit-table scenario calls, so the
`smoke.rs` lines cited above did not move except for the 16 after that call.

| Mutation | Pin | Red with |
|---|---|---|
| F2b `set_hscrollbar_policy(Never)` to `Automatic` (`settings_mcp.rs`) | `audit_card_keeps_a_long_path_and_a_wide_row_inside_the_page`: the scroller's minimum width is at least the grid's | the scroller asks for 24px of the grid's 367px |
| F2c path caption `set_ellipsize(Middle)` removed | same function: the caption's natural width exceeds the room (the scenario cannot pass vacuously) and its minimum is below it; the card fits the minimum window | the path caption needs 1078px (a real fixture path, `TMPDIR=/tmp`) |
| F2e column 3 `set_max_width_chars(14)` to `40` | same function: a 30 character Target cell asks for under 95% of the same text's width in an uncapped label | the Target cell asks for 247px of the text's 247px |
| `DISPLAY_LIMIT` 200 to 201 | `mcp_audit_view::tests::the_display_limit_is_pythons_two_hundred` (the existing cap test is written in terms of the constant) | left: 201, right: 200 |
| port-only rebind (the port dropped from the tuple `settings_dialog.rs` compares on OK) | `settings_mcp_low_notes_pins`: an OK that changes only the port releases the old port and answers on the new | `pump_until(... new port listening)` fails |
| lazy-build guard of `restart_mcp` removed | same function: `restart_mcp` with the server off and nothing built leaves `McpBridge::built()` false | restart_mcp built a controller for a server that is off |

The F2c fixture's own path is the fixture's temp directory, so the old scenario catches F2c or not by how long `TMPDIR` is. The new
function builds its own path (two folder names of 40 and 54 characters), so the pin does not depend on the environment.

**Tool column (design note, Phase 7).** At the minimum window (560 x 440) the Tool, Tier and Target columns give way before Outcome
does, and Tool shows about three characters (`tog`, `Rev...`); the full text is in the tooltip. This is the same trade as section 6:
the Outcome is the column the card exists for. It is a copy and layout item for Phase 7 (a shorter tool name, or a two line
cell), carried on DAN-2319's ledger. `wontfix` in 6.i.
