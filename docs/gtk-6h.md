# The Settings window (Phase 6.h)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of [DAN-2221](/DAN/issues/DAN-2221).
It holds the two maps Phase 6 asks every ticket for (the method map and the Python test map; 6.j checks the union), the
differential with its documented classes, the on-purpose differences, the measured minimum size and the shot set.
Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`. Nothing here reaches a network: the Test buttons that would are
built and insensitive (section 2, D-C).

## 1. The cut

**Ported here** (`hatate-gtk`; `hatate-core` gains `settings_search::{name_of, index_tabs}`, the index half of
`gui/settings_search.py`):

- `settings_spec.rs`: every page of `SettingsDialog` as data (labels, tooltips, ranges, choices), **generated from the
  built Python dialog at `76c85c4`**, so the words cannot drift from a hand copy.
- `settings_values.rs`: `initial(&Settings)` (the set half of `__init__`) and `apply(values, snapshot, &mut Settings)`
  (`apply_to_settings`), with Qt's spin rounding and bounds, `findData`-or-first for combos, `splitlines` and trimming, and
  the DeviantArt snapshot rule (DAN-123).
- `settings_form.rs`, `settings_node.rs`, `settings_dialog.rs`: the form built from the spec, the tabs, the Find row over
  `settings_search`, the two-second mark, the live pipeline line, the Google Lens warning, Browse, OK and Cancel, and
  `action_open_settings` with the refreshes Python runs after OK. The menu's Preferences is wired.
- `theme.rs`: the Settings window's CSS, tokens only (`window.settings-window`, `.found`).

**Not in this PR** (named owners):

| What | Owner | State here |
|---|---|---|
| Shortcuts page and the key-binding editor | 6.i | a tab with a one-line note; its keys are untouched on OK |
| MCP page | 6.i | the same |
| The six Test buttons (DeviantArt, AnimePictures, Sankaku, e621, Pixiv, Hydrus) and Fetch from Hydrus | 6.h-2 | built, insensitive, with Python's tooltip: they call the network and need a transport differential over https |

## 2. Differences from Python

| Id | What differs | Why | Pinned by |
|---|---|---|---|
| **D-A** (M1) | The Find box does not list the Shortcuts and MCP pages' labels (12 rows over seeds 1 to 3); both tab entries are listed. | Those pages are 6.i's. | the comparer (`cmp_s.py` `drop_6i`) drops exactly the rows that start `MCP  ›  ` or `Shortcuts  ›  ` other than the tab entry, and counts them |
| **D-B** (L1) | The Lens warning says `no Chromium-based browser could be found.` where Python names Playwright; its detail text differs the same way. | This build has no Playwright (5.b). | `crates/hatate-gtk/tests/smoke.rs:7250-7260` (text start and end, detail); the comparer accepts only Python's two exact sentences against core's exact text, by regex and equality |
| **D-C** | The Test buttons and Fetch from Hydrus are insensitive. | 6.h-2 (above). | `settings_dialog.rs` `insensitive_network_buttons` |
| **D-D** (C1) | A JSON integer in a float field of a hand-edited `config.json` (`min_similarity: 101`, `engine_delays.<name>: [1, 2]`) is written back as `101.0` and `[1.0, 2.0]` on a close with no OK; Python keeps `101`. Numerically identical. | The cause is the config module (PR #5), not this window. **Ruled by Cloud on DAN-2406 (Option A, three conditions).** The P2 against the module is DAN-2407; it removes this class. | the comparer's `c1_map` (exact `N` to `N.0` at float-typed paths, then **byte-identical**); negative control below |
| **D-E** | The window has a stated minimum of 560 x 440 and its pages scroll sideways below their natural width. Python states only a minimum width of 480. | Gate 2.5 wants a stated minimum that every state fits. | `smoke.rs:7151` (`settings_window_keeps_its_stated_minimum`), red at 837 x 209 before the change (`worktrees/DAN-2221-revert.log` R2) |
| (ruled) | The saved `table_header_state` is `hatate-rs:1:` and the widths, not a Qt blob. | DAN-2293. | (not this ticket's) |

**C1, the three conditions** (DAN-2406): (1) exact mapping only, then byte-identical texts; (2) negative control; (3) the
untruncated evidence. For (2): `python3 -I cmp_s.py run/2/py.jsonl run/2/rs.jsonl --negctl` perturbs one value in the
hatate-rs text of the first C1 case (`N.0` to `N.5`) and reports `cases=40 differing=1`. For (3): the unified diff of the
five cases (s2-024, s2-030, s3-023, s3-025, s3-035) is in `worktrees/DAN-2221-differential.log`; every hunk is an
integer-to-float line, none is another difference.

## 3. Method map (`gui/` at `76c85c4`)

| Python | Rust |
|---|---|
| `settings_dialog.py:47` `_nonblank_lines` | `settings_values.rs` `nonblank_lines` |
| `:178` `SettingsDialog.__init__` | `settings_dialog.rs` `SettingsDialog::build`, `settings_values::initial` |
| `:236` `_scrollable` | `settings_dialog.rs` `scrollable` |
| `:249` `_build_search_row`, `:280` `_on_search`, `:297` `_jump_to_setting` | `build`, `on_search`, `jump_to_setting` |
| `:309` `_build_general_tab`, `:571` `_build_engine_tab`, `:978` `_build_import_tab`, `:1088` `_build_site_logins_tab`, `:1528` `_build_saucenao_tab`, `:1593` `_build_hydrus_tab`, `:1900` `_build_tag_namespaces_tab`, `:2003` `_build_outcome_tags_group` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` |
| `:565` `_browse_log_path` | `wire_controls`, button `button.browse` |
| `:939` `_warn_if_lens_cannot_run` | `warn_if_lens_cannot_run` |
| `:2650` `accept`, `:2666` `apply_to_settings` | `accept`, `settings_values::apply`, `MainWindow::apply_settings_dialog` |
| `:2827` `_update_engine_pipeline` | `update_pipeline`, `settings_values::pipeline_text` |
| `:1294` `_test_deviantart`, `:1384` `_test_animepictures`, `:1478` `_test_sankaku`, `:1685` `_test_e621`, `:1763` `_test_pixiv`, `:1882` `_test_hydrus`, `:1853` `_fetch_tag_services`, `:1845` `_on_service_selected` | **6.h-2** |
| `:2075` `_build_shortcuts_tab`, `:2175` `_current_shortcut_bindings`, `:2181` `_reset_shortcuts_to_defaults`, `:2188` `_refresh_shortcut_conflicts` | **6.i** |
| `:52` `_read_mcp_audit_entries` to `:166` `_mcp_outcome_tooltip` (5 helpers), `:2212` `_build_mcp_tab` to `:2650` (`_build_mcp_connection_card`, `_build_mcp_always_available_group`, `_build_mcp_tier_checkbox_group`, `_copy_mcp_token`, `_generate_mcp_token`, `_refresh_mcp_dry_run_hint`, `_on_mcp_enabled_toggled`, `_show_mcp_enable_summary`, `_mcp_server_handle`, `_mcp_restart_server`, `_refresh_mcp_connection`, `_build_mcp_audit_card`, `_on_mcp_audit_filter_changed`, `_on_mcp_audit_follow_toggled`, `_open_mcp_audit_folder`, `_on_tabs_current_changed`, `_refresh_mcp_audit_table`) | **6.i** |
| `settings_search.py:38` `index_tabs`, `:60` `_name_of`, `:75` `matches` | `hatate-core` `settings_search::{index_tabs, name_of, matches}` |
| `:98` `highlight` | `settings_dialog.rs` `SettingsDialog::highlight` |

## 4. Python test map

An `ast` walk of `tests/test_*.py` at `76c85c4` (`worktrees/DAN-2221-tools/testmap.py`), following setUp, the test's own
class helpers and module helpers, for a reference to `SettingsDialog`, `settings_search` or any of the dialog's methods,
found **65 tests in 18 classes**. The rows are generated from that walk and a rule table
(`worktrees/DAN-2221-tools/mkmap.py`); a row with no rule aborts the generator, so none is unmapped. Where a row names an assertion it
is the line that makes the row's claim.

| Python test (`tests/` at `76c85c4`) | Rust |
|---|---|
| `test_gui_smoke.py:124` `TestSettingsDialogConstruction.test_dialog_and_every_tab_build` | `crates/hatate-gtk/tests/smoke.rs:7066` (the nine tab names, in order), `crates/hatate-gtk/tests/smoke.rs:7082` (controls open on the stored values); the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`) opens the dialog in every case |
| `test_gui_smoke.py:141` `TestSettingsDialogConstruction.test_settings_round_trip_through_the_dialog` | `crates/hatate-gtk/tests/smoke.rs:7112` (OK then reload, equals what Python's `apply_to_settings` gives), `crates/hatate-gtk/tests/smoke.rs:7141` (untouched controls hand back what they were given) |
| `test_gui_smoke.py:155` `TestSettingsDialogConstruction.test_deviantart_cookie_field_explains_the_rotation_fix_and_its_limit` | **not asserted.** The tooltip text is in `settings_spec.rs`, generated from the built Python dialog at `76c85c4`; no Rust test reads it |
| `test_gui_smoke.py:191` `TestDeviantArtCookieReconciliation.test_an_untouched_box_does_not_clobber_a_same_session_rotation` | `crates/hatate-gtk/src/settings_values.rs:661` (`rotated` survives an unchanged box) |
| `test_gui_smoke.py:208` `TestDeviantArtCookieReconciliation.test_an_actual_edit_still_wins_over_a_same_session_rotation` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`) (an edited cookie box, then OK); the unit test pins only the clearing edit, `crates/hatate-gtk/src/settings_values.rs:665` |
| `test_gui_smoke.py:228` `TestDeviantArtCookieReconciliation.test_clearing_the_box_is_an_explicit_logout_not_a_no_op` | `crates/hatate-gtk/src/settings_values.rs:665` (a blank edit writes the empty cookie) |
| `test_gui_smoke.py:467` `TestEngineAlertsReachTheUser.test_enabling_lens_without_its_dependencies_warns_at_once` | `crates/hatate-gtk/tests/smoke.rs:7249` (title), `:7250-7260` (text start and end, detail non-empty), `:7261` (the box stays ticked); wording differs from Python's on purpose, class L1 |
| `test_gui_smoke.py:488` `TestEngineAlertsReachTheUser.test_a_working_install_says_nothing` | `crates/hatate-gtk/tests/smoke.rs:7280` (a browser on `PATH`: no box); red with the early return disabled (`worktrees/DAN-2221-revert.log` R3) |
| `test_gui_smoke.py:1140` `TestE621LoginTest.test_half_filled_credentials_are_refused_before_any_request` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1152` `TestE621LoginTest.test_it_sends_the_typed_credentials_not_the_saved_ones` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1163` `TestE621LoginTest.test_it_uses_the_apps_own_header_builder` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1174` `TestE621LoginTest.test_accepted_credentials_are_reported_as_such` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1179` `TestE621LoginTest.test_a_401_is_reported_as_a_rejection_with_what_to_do` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1186` `TestE621LoginTest.test_a_network_error_is_not_reported_as_a_rejection` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:1196` `TestE621LoginTest.test_an_unexpected_status_is_neither_accept_nor_reject` | 6.h-2 (the Test buttons are built and insensitive) |
| `test_gui_smoke.py:2665` `TestPerEngineDelaySettingsRoundTrip.test_untouched_boxes_store_no_override` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): `engine_delay_*` controls untouched, `config.json` compared as bytes |
| `test_gui_smoke.py:2670` `TestPerEngineDelaySettingsRoundTrip.test_an_override_survives_a_round_trip` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): `engine_delay_*` set, OK, reopen |
| `test_gui_smoke.py:2674` `TestPerEngineDelaySettingsRoundTrip.test_other_engines_are_not_given_zero_overrides` | `crates/hatate-gtk/src/settings_values.rs:643-648` (`iqdb`, both zero, is absent from the saved map) |
| `test_gui_smoke.py:2682` `TestPerEngineDelaySettingsRoundTrip.test_a_zero_override_never_reaches_the_settings` | `crates/hatate-gtk/src/settings_values.rs:643-648` (a zero pair is no override) |
| `test_gui_smoke.py:2687` `TestPerEngineDelaySettingsRoundTrip.test_the_stored_pair_is_always_ordered` | `crates/hatate-gtk/src/settings_values.rs:643-648` (`ascii2d` 30 and 20 is saved `[20, 30]`) |
| `test_gui_smoke.py:2691` `TestPerEngineDelaySettingsRoundTrip.test_a_half_filled_pair_becomes_a_usable_interval` | `crates/hatate-gtk/src/settings_values.rs:643-648` (`saucenao` 0 and 40 is saved `[40, 40]`) |
| `test_gui_smoke.py:2697` `TestPerEngineDelaySettingsRoundTrip.test_what_is_saved_is_what_the_limiter_reads` | does not apply here: the limiter is `hatate-core`'s (Phase 5); this ticket's dialog does not read it |
| `test_gui_smoke.py:5607` `TestShortcutsSettingsTab.test_it_offers_every_registered_action` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5611` `TestShortcutsSettingsTab.test_only_changed_bindings_are_persisted` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5619` `TestShortcutsSettingsTab.test_clearing_a_binding_is_remembered_as_cleared` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5626` `TestShortcutsSettingsTab.test_a_duplicate_key_is_named_rather_than_resolved_silently` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5635` `TestShortcutsSettingsTab.test_ok_is_refused_while_two_actions_share_a_key` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5646` `TestShortcutsSettingsTab.test_ok_goes_through_once_the_clash_is_resolved` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_gui_smoke.py:5654` `TestShortcutsSettingsTab.test_reset_puts_every_default_back` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:65` `TestConstructionWithNoMainWindow.test_builds_without_a_parent` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:70` `TestConstructionWithNoMainWindow.test_enabled_defaults_off_and_hydrus_writes_defaults_on` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:91` `TestApplyToSettings.test_round_trips_every_mcp_field` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:120` `TestEnableSummaryConstructionSafety.test_does_not_fire_when_restoring_a_saved_enabled_state` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:132` `TestEnableSummaryConstructionSafety.test_fires_exactly_once_on_a_real_off_to_on_click` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:156` `TestConnectionStates.test_state_1_stopped_when_not_enabled` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:162` `TestConnectionStates.test_state_2_listening_when_running` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:167` `TestConnectionStates.test_state_3_mcp_package_not_installed` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:174` `TestConnectionStates.test_state_4_no_token` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:179` `TestConnectionStates.test_state_5_port_busy` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:186` `TestConnectionStates.test_state_6_honest_fallback_shows_open_log_button` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:197` `TestRestartServer.test_restart_uses_the_typed_port_and_token_not_the_saved_ones` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:341` `TestAuditTableFiltering.test_actions_filter_hides_reads_by_default` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:347` `TestAuditTableFiltering.test_all_calls_filter_shows_everything` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_mcp_settings_tab.py:352` `TestAuditTableFiltering.test_newest_first` | 6.i (Shortcuts and MCP pages; the tabs are placeholders here) |
| `test_outcome_tags.py:621` `TestSettingsDialogSection.test_the_boxes_show_the_current_setting` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): every control's value is compared after every step, the outcome-tag boxes among the 97 ids; `crates/hatate-gtk/tests/smoke.rs:7141` |
| `test_outcome_tags.py:632` `TestSettingsDialogSection.test_the_boxes_are_empty_by_default` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`) (a default config is the first scenario of every seed) |
| `test_outcome_tags.py:639` `TestSettingsDialogSection.test_what_is_typed_is_saved` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`); unit `crates/hatate-gtk/src/settings_values.rs` `rule_tags_and_the_blacklist_keep_their_lines_trimmed_without_the_blank_ones` |
| `test_outcome_tags.py:651` `TestSettingsDialogSection.test_blank_lines_are_not_saved` | `crates/hatate-gtk/src/settings_values.rs:712-713` (blank lines dropped from the blacklist and the rule tags) |
| `test_outcome_tags.py:660` `TestSettingsDialogSection.test_clearing_a_box_switches_that_rule_off` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`) (a blank box, then OK, `config.json` bytes) |
| `test_outcome_tags.py:668` `TestSettingsDialogSection.test_a_full_round_trip_leaves_the_settings_unchanged` | `crates/hatate-gtk/tests/smoke.rs:7141`, `crates/hatate-gtk/src/settings_values.rs:613-621` |
| `test_outcome_tags.py:680` `TestSettingsDialogSection.test_the_section_is_findable_by_the_settings_search` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): `s_find` result lists compared with Python's after every query, class M1 aside |
| `test_sent_tag_filter.py:306` `TestTheDialogCopy.test_the_two_meanings_are_worded_separately` | **not asserted.** The two labels are in `settings_spec.rs`, generated from the Python dialog; no Rust test reads them |
| `test_sent_tag_filter.py:323` `TestTheDialogCopy.test_the_switch_round_trips_through_apply_to_settings` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): the switch set, OK, `config.json` bytes |
| `test_sent_tag_filter.py:339` `TestTheDialogCopy.test_an_unchecked_switch_shows_as_unchecked` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): the control value after open, from a config with the switch off |
| `test_settings_search.py:31` `TestMatching.test_words_match_in_any_order` | `crates/hatate-core/src/settings_search.rs:184` |
| `test_settings_search.py:38` `TestMatching.test_the_tab_name_counts_as_part_of_the_name` | `crates/hatate-core/src/settings_search.rs:191` |
| `test_settings_search.py:45` `TestMatching.test_an_empty_query_matches_nothing` | `crates/hatate-core/src/settings_search.rs:197` |
| `test_settings_search.py:52` `TestMatching.test_shorter_names_come_first` | `crates/hatate-core/src/settings_search.rs:204` |
| `test_settings_search.py:56` `TestMatching.test_matching_ignores_case` | `crates/hatate-core/src/settings_search.rs:197` (case folded) |
| `test_settings_search.py:74` `TestHighlightReentrancy.test_a_second_highlight_inside_the_revert_window_still_restores` | `crates/hatate-gtk/tests/smoke.rs:7190` and `:7193`; red with the token check dropped (`worktrees/DAN-2221-revert.log` R1) |
| `test_settings_search.py:110` `TestIndexingTheRealDialog.test_it_finds_a_useful_number_of_settings` | the action differential (`tests/differential.rs` `settings_step`, ops `s_open s_set s_find s_jump s_tab s_press s_ok s_cancel`): `s_find` result lists of the real dialog against Python's |
| `test_settings_search.py:114` `TestIndexingTheRealDialog.test_every_tab_is_reachable_by_its_own_name` | `crates/hatate-core/src/settings_search.rs:148` (each tab, then each name once) |
| `test_settings_search.py:126` `TestIndexingTheRealDialog.test_explanations_are_not_indexed_as_names` | `crates/hatate-core/src/settings_search.rs:127` (`name_of` drops long labels and fragments) |
| `test_settings_search.py:133` `TestIndexingTheRealDialog.test_a_known_setting_is_findable` | `crates/hatate-gtk/tests/smoke.rs:7204` (`quota pause`), `crates/hatate-gtk/tests/smoke.rs:7216` (`hydrus key`) |
| `test_type_voice.py:507` `TestBareWidgets.test_settings_dialog_helper_buttons_keep_sentence_case` | **not asserted.** Every button's text is in `settings_spec.rs`, generated from the Python dialog; no Rust test reads its case |

**Not asserted** (three rows): the DeviantArt tooltip text, the two sent-tag-filter labels and the helper buttons' case.
They are in `settings_spec.rs` as generated from the built Python dialog; no Rust test reads them. For 6.j.

## 5. The action differential

Seeds 1 to 5, 40 scenarios each (`worktrees/DAN-2221-tools/gen_s.py`, `run_s.sh SEED 40`; the harness lives outside the
repo; the log is `worktrees/DAN-2221-differential.log`). Python's `SettingsDialog` at `76c85c4` (temp `HOME`,
`QT_QPA_PLATFORM=offscreen`) and the Rust window run the same steps; after every step the comparer reads `config.json` as
bytes, the dialog's tabs and current tab, the Find text and result list, all 97 control values, the pipeline line, message
boxes and file choosers. The generator starts from awkward but loadable configs (`cfg_overrides`).

SEEDS_TABLE

## 6. Minimum size, shots and contrast

HOSTED_SECTION
