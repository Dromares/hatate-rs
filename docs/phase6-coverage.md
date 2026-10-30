# Phase 6 exit check: every `gui/` file and `def`, and where it went (DAN-2223)

Work in progress Rust rewrite of Hatate, not yet a replacement for it. This is the Phase 6.j coverage table. It lists every `.py` file and every `def` in `gui/` of `Hatate-Linux-Redux` at `76c85c4`, the Phase 6 rows of `docs/phase5-coverage.md`, and for each its Rust home and the merged PR, or the reason it has no Rust function of its own.

## 1. How it is checked

`docs/phase6-gui-defs-76c85c4.txt` is the listing of the pinned tree: one line `gui/<file>.py:<line> <qualified name>` for each of the 641 functions and methods (nested functions included, written `outer.<locals>.inner`) of the 26 files, produced by an `ast` walk of `gui/*.py` at `76c85c4` by a script that lives outside the repo (the tree is Rust only; the walk is `ast.parse` and nothing else). The test `crates/hatate-gtk/tests/phase6_coverage.rs` reads the listing and this file and fails if a file or a `def` is **missing or extra**, if a row has no Rust home, if a row's line differs from the listing, or if a `crates/...` path cited in a row does not exist. It also checks that every Phase 6 row of `docs/phase5-coverage.md` points here. A Python tree that changes (it is fixes-only) is a new pin and a new listing; the test does not read the Python tree.

Each row's Rust home is the row of the owning ticket's method map (the last column names it), copied, not re-derived; the PR column is the merged PR of that ticket. A row whose source is `this table (6.j)` is one the owning docs did not name one by one, and was located in the tree for this table.

| Ticket | Doc | PR |
|---|---|---|
| 6.a core support | `docs/phase6a-core-support.md` | #59 |
| 6.b pure core | `docs/phase6b-pure-core.md` | #58 |
| 6.c shell | `docs/gtk-shell.md` | #57 (+#60, #61, #62 polish) |
| 6.d queue view | `docs/queue-view.md` | #67 |
| 6.e running searches | `docs/gtk-6e.md` | #63 |
| 6.f review view | `docs/gtk-6f.md` | #71 |
| 6.f-2 Wipe, Differences, compare | `docs/gtk-6f2.md` | #72 |
| 6.g Hydrus, Pawchive | `docs/gtk-6g.md` | #74 |
| 6.h Settings | `docs/gtk-6h.md` | #75 |
| 6.i Settings > MCP, Shortcuts | `docs/gtk-6i.md` | #76 |

## 2. The 26 files of `gui/`

| File | Defs | Rust modules named in its rows | PRs |
|---|---|---|---|
| `gui/activity_view.py` | 10 | `activity.rs` | #63 |
| `gui/add_tags_dialog.py` | 2 | `dialogs.rs`, `add_tags.rs` | #71 |
| `gui/compare_dialog.py` | 44 | `compare_view.rs`, `wipe_view.rs`, `compare_dialog.rs`, `review_compare.rs` | #72 |
| `gui/filter_bar.py` | 17 | `filter_bar.rs` | #67 |
| `gui/fonts.py` | 1 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/hydrus_query_dialog.py` | 4 | none: the file has no Rust module of its own (see its rows) | #74 |
| `gui/image_table_model.py` | 34 | `queue_view.rs` | #67 |
| `gui/log_viewer_dialog.py` | 5 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/main_window.py` | 161 | `search_run.rs`, `review_ops.rs`, `thumbnails.rs`, `queue_view.rs`, `checks.rs` | #57, #58, #59, #60, #61, #62, #63, #67, #71, #72, #74, #76 |
| `gui/match_conditions_dialog.py` | 2 | `dialogs.rs`, `review_ops.rs` | #71 |
| `gui/mcp_bridge.py` | 56 | `mcp_bridge.rs`, `tool_handlers.rs`, `queue.rs` | #76 |
| `gui/message.py` | 6 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/parser_health_dialog.py` | 8 | `parser_health.rs` | #63 |
| `gui/pawchive_index_dialog.py` | 17 | `pawchive_dialog.rs` | #74 |
| `gui/preview_text.py` | 14 | `preview_text.rs`, `eshuushuu.rs` | #71 |
| `gui/review_shortcuts.py` | 11 | `review_ops.rs`, `handlers.rs`, `fetch.rs`, `batch_cmd.rs`, `menubar_keys.rs` | #71 |
| `gui/review_view.py` | 29 | `review_ops.rs`, `review_view.rs`, `review_compare.rs`, `wipe_view.rs`, `compare_dialog.rs` | #71, #72 |
| `gui/session_autosave.py` | 6 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/settings_dialog.py` | 55 | `settings_dialog.rs`, `settings_mcp.rs`, `settings_spec.rs`, `settings_form.rs`, `mcp_audit_view.rs` | #75, #76 |
| `gui/settings_search.py` | 5 | `settings_dialog.rs` | #58, #75 |
| `gui/shell.py` | 18 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/table_context_menu.py` | 12 | none: the file has no Rust module of its own (see its rows) | #67 |
| `gui/table_delegates.py` | 8 | `queue_view.rs`, `theme.rs`, `review_ops.rs` | #67, #71 |
| `gui/theme.py` | 9 | none: the file has no Rust module of its own (see its rows) | #57, #59, #60, #61, #62 |
| `gui/widgets.py` | 98 | `theme.rs`, `review_view.rs`, `activity.rs`, `compare_dialog.rs` | #57, #60, #61, #62, #71 |
| `gui/worker_lifecycle.py` | 9 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |

## 3. The Phase 6 rows of `docs/phase5-coverage.md`

The twenty files that `docs/phase5-coverage.md` gave to Phase 6. `docs/phase5-coverage.md` now points each of them here. All twenty have a Rust home; none moved to Phase 7. `tools/render_states.py` is the only one whose home is an example, not a module.

| Python file | Rust home | PR |
|---|---|---|
| `core/applog.py` | `crates/hatate-core/src/applog.rs` (`LogRing`); the log viewer is `crates/hatate-gtk/src/logview.rs`, the tracing sink `logsink.rs`; the rotating file is `rotating_log.rs` | #58, #57, #8 |
| `core/entry_filter.py` | `crates/hatate-core/src/entry_filter.rs`, shown by `crates/hatate-gtk/src/filter_bar.rs` | #58, #67 |
| `core/eta.py` | `crates/hatate-core/src/eta.rs`, fed by `crates/hatate-gtk/src/search_run.rs` | #58, #63 |
| `core/export.py` | `crates/hatate-core/src/export.rs`, File > Export in `crates/hatate-gtk/src/table_actions.rs` | #58, #67 |
| `core/missing_files.py` | `crates/hatate-core/src/missing_files.rs`, called from `crates/hatate-gtk/src/window.rs` at restore | #58, #57 |
| `core/shortcuts.py` | `crates/hatate-core/src/shortcuts.rs`, keys in `crates/hatate-gtk/src/review_keys.rs`, the editor in `shortcuts_page.rs` | #58, #71, #76 |
| `core/sidecar.py` | `crates/hatate-core/src/sidecar.rs`, Write Tag Files in `crates/hatate-gtk/src/table_actions.rs` | #58, #67 |
| `core/tag_colors.py` | `crates/hatate-core/src/tag_colors.rs`, used by the tag panel in `crates/hatate-gtk/src/review_ops.rs` | #58, #71 |
| `core/viewport.py` | `crates/hatate-core/src/viewport.rs`, used by `crates/hatate-gtk/src/thumbnails.rs` | #58, #67 |
| `workers/availability_worker.py` | `crates/hatate-core/src/availability.rs` run as a worker-bridge job by `crates/hatate-gtk/src/checks.rs` | #26, #63 |
| `workers/candidate_worker.py` | `crates/hatate-core/src/search_engine.rs` `fetch_candidate_details`, run by `crates/hatate-gtk/src/review_ops.rs` | #33, #71 |
| `workers/hydrus_import_poll_worker.py` | `crates/hatate-core/src/hydrus_import_poll.rs`, driven by `crates/hatate-gtk/src/hydrus_ops.rs` | #45, #74 |
| `workers/hydrus_lookup_worker.py` | `crates/hatate-core/src/hydrus_tag_lookup.rs`, driven by `crates/hatate-gtk/src/hydrus_ops.rs` | #36, #74 |
| `workers/hydrus_reconcile_worker.py` | `crates/hatate-core/src/hydrus_reconcile.rs`, driven by `crates/hatate-gtk/src/hydrus_ops.rs` | #45, #74 |
| `workers/missing_file_worker.py` | `crates/hatate-core/src/missing_files.rs` `check_session_files`, run from `crates/hatate-gtk/src/window.rs` | #58, #57 |
| `workers/pawchive_index_worker.py` | `crates/hatate-core/src/pawchive_index/crawl.rs`, driven by `crates/hatate-gtk/src/pawchive_dialog.rs` | #38, #74 |
| `workers/session_autosave_worker.py` | `crates/hatate-core/src/batch/run.rs` `Autosave`, driven by `crates/hatate-gtk/src/session_ops.rs` | #35, #57 |
| `workers/thumbnail_worker.py` | `crates/hatate-core/src/thumb_decode.rs`, `thumb.rs`, run by `crates/hatate-gtk/src/thumbnails.rs` | #59, #67 |
| `workers/upscale_check_worker.py` | `crates/hatate-core/src/upscale_detect.rs`, run by `crates/hatate-gtk/src/checks.rs` | #37, #63 |
| `tools/render_states.py` | `crates/hatate-gtk/examples/shot.rs` (state renderer, dark and light, default and minimum size; section 5) | #57 and later, this PR |


## 4. Every `def` of `gui/`

Grouped by file, in line order. A row's Rust home is the owning ticket's wording; `Left to` text in it names a later ticket that took the rest. Where the home says a def has no function of its own, the reason is in the row.


### `gui/activity_view.py` (10 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 53 | `ActivityViewMixin._build_activity_page` | `hatate-gtk/src/activity.rs:142` `build` | #63 | `gtk-6e.md:257` |
| 79 | `ActivityViewMixin._build_engines_card` | `hatate-gtk/src/activity.rs:142` `build` (the chips) | #63 | `gtk-6e.md:258` |
| 128 | `ActivityViewMixin._engine_state` | `hatate-gtk/src/activity.rs:32` `engine_state` | #63 | `gtk-6e.md:259` |
| 158 | `ActivityViewMixin.refresh_engines` | `hatate-gtk/src/activity.rs:301` `refresh_engines` | #63 | `gtk-6e.md:260` |
| 199 | `ActivityViewMixin._build_log_card` | `hatate-gtk/src/activity.rs:142` `build` (the log card) | #63 | `gtk-6e.md:261` |
| 235 | `ActivityViewMixin._tail_log` | `hatate-gtk/src/activity.rs:327` `tail_log` | #63 | `gtk-6e.md:262` |
| 256 | `ActivityViewMixin._on_follow_toggled` | `hatate-gtk/src/activity.rs:375` `on_follow_toggled` | #63 | `gtk-6e.md:263` |
| 264 | `ActivityViewMixin._copy_log` | `hatate-gtk/src/activity.rs:388` `copy_log` | #63 | `gtk-6e.md:264` |
| 267 | `ActivityViewMixin._open_log_folder` | `hatate-gtk/src/activity.rs:394` `open_log_folder` | #63 | `gtk-6e.md:265` |
| 274 | `ActivityViewMixin.set_activity_live` | `hatate-gtk/src/activity.rs:353` `set_activity_live` | #63 | `gtk-6e.md:266` |

### `gui/add_tags_dialog.py` (2 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 16 | `AddTagsDialog.__init__` | `dialogs.rs` `Prompter::add_tags` (an `AlertDialog` with a text view) | #71 | `gtk-6f.md:313` |
| 36 | `AddTagsDialog.get_tags` | `add_tags.rs` `parse_added_tags` | #71 | `gtk-6f.md:314` |

### `gui/compare_dialog.py` (44 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 47 | `WipeView.__init__` | `crates/hatate-gtk/src/wipe_view.rs:81` `WipeView` over `crates/hatate-core/src/compare_view.rs` `WipeGeometry` | #72 | `gtk-6f2.md:183` |
| 72 | `WipeView._wipe_line_colour` | `crates/hatate-gtk/src/wipe_view.rs:410` (white in both schemes, `on_accent`) | #72 | `gtk-6f2.md:185` |
| 82 | `WipeView._pane_divider_colour` | `crates/hatate-gtk/src/wipe_view.rs:456` (the divider in `paint`) | #72 | `gtk-6f2.md:186` |
| 92 | `WipeView._reference` | `crates/hatate-core/src/compare_view.rs:116` `set_reference` | #72 | `gtk-6f2.md:187` |
| 101 | `WipeView.showing_differences` | `crates/hatate-core/src/compare_view.rs:127` `showing_differences`, `crates/hatate-gtk/src/wipe_view.rs:170` `showing_differences` | #72 | `gtk-6f2.md:188` |
| 104 | `WipeView._reference_size` | `crates/hatate-core/src/compare_view.rs:122` `set_differences` (the reference doubles in width) | #72 | `gtk-6f2.md:189` |
| 118 | `WipeView._fit_size` | `crates/hatate-core/src/compare_view.rs:146` `fit_size` | #72 | `gtk-6f2.md:190` |
| 126 | `WipeView._target_rect` | `crates/hatate-core/src/compare_view.rs:151` `target_rect` | #72 | `gtk-6f2.md:191` |
| 138 | `WipeView._clamp_pan` | `crates/hatate-core/src/compare_view.rs:163` `clamp_pan` | #72 | `gtk-6f2.md:192` |
| 153 | `WipeView.zoom_percent` | `crates/hatate-core/src/compare_view.rs:177` `zoom_percent`, `crates/hatate-gtk/src/wipe_view.rs:184` `zoom_percent` | #72 | `gtk-6f2.md:193` |
| 163 | `WipeView._apply_zoom` | `crates/hatate-core/src/compare_view.rs:163` `clamp_pan` | #72 | `gtk-6f2.md:192` |
| 186 | `WipeView._centre` | `crates/hatate-core/src/compare_view.rs:186` `apply_zoom` | #72 | `gtk-6f2.md:194` |
| 189 | `WipeView.zoom_in` | `crates/hatate-core/src/compare_view.rs:213` `zoom_in`, `crates/hatate-gtk/src/wipe_view.rs:222` `zoom_in` | #72 | `gtk-6f2.md:196` |
| 192 | `WipeView.zoom_out` | `crates/hatate-core/src/compare_view.rs:217` `zoom_out`, `crates/hatate-gtk/src/wipe_view.rs:229` `zoom_out` | #72 | `gtk-6f2.md:197` |
| 195 | `WipeView.zoom_to_fit` | `crates/hatate-core/src/compare_view.rs:221` `zoom_to_fit`, `crates/hatate-gtk/src/wipe_view.rs:236` `zoom_to_fit` | #72 | `gtk-6f2.md:198` |
| 201 | `WipeView.zoom_to_actual` | `crates/hatate-core/src/compare_view.rs:227` `zoom_to_actual`, `crates/hatate-gtk/src/wipe_view.rs:241` `zoom_to_actual` | #72 | `gtk-6f2.md:199` |
| 210 | `WipeView.set_images` | `crates/hatate-gtk/src/wipe_view.rs:133` `set_images` | #72 | `gtk-6f2.md:200` |
| 215 | `WipeView.set_difference_view` | `crates/hatate-gtk/src/wipe_view.rs:147` `set_difference_view` | #72 | `gtk-6f2.md:201` |
| 230 | `WipeView.clear_difference_view` | `crates/hatate-gtk/src/wipe_view.rs:159` `clear_difference_view` | #72 | `gtk-6f2.md:202` |
| 234 | `WipeView.set_position` | `crates/hatate-gtk/src/wipe_view.rs:175` `set_position` | #72 | `gtk-6f2.md:203` |
| 239 | `WipeView.wheelEvent` | `crates/hatate-core/src/compare_view.rs:239` `wheel`, scroll controller in `crates/hatate-gtk/src/wipe_view.rs:250` `wire` | #72 | `gtk-6f2.md:204` |
| 247 | `WipeView.mousePressEvent` | drag controller in `crates/hatate-gtk/src/wipe_view.rs:250` `wire` | #72 | `gtk-6f2.md:205` |
| 253 | `WipeView.mouseMoveEvent` | `crates/hatate-core/src/compare_view.rs:247` `drag_to`, drag controller in `wire` | #72 | `gtk-6f2.md:206` |
| 259 | `WipeView.mouseReleaseEvent` | drag controller in `wire` | #72 | `gtk-6f2.md:207` |
| 264 | `WipeView.mouseDoubleClickEvent` | click controller in `wire` (double-click fits) | #72 | `gtk-6f2.md:208` |
| 267 | `WipeView.keyPressEvent` | key controller in `wire` (`+`, `-`, `0`, `1`) | #72 | `gtk-6f2.md:209` |
| 282 | `WipeView.resizeEvent` | `set_widget_size` (compare_view.rs:132), called from the resize handler in `wire` | #72 | `gtk-6f2.md:210` |
| 290 | `WipeView._draw_side` | `crates/hatate-gtk/src/wipe_view.rs:338` `draw_side` | #72 | `gtk-6f2.md:211` |
| 316 | `WipeView._paint_differences` | `crates/hatate-gtk/src/wipe_view.rs:393` `paint` (the differences branch) | #72 | `gtk-6f2.md:212` |
| 359 | `WipeView.paintEvent` | `crates/hatate-gtk/src/wipe_view.rs:393` `paint` | #72 | `gtk-6f2.md:213` |
| 408 | `build_difference_overlay` | `crates/hatate-core/src/compare_view.rs:335` `build_difference_overlay`, `crates/hatate-core/src/compare_view.rs:294` `ramp_picture` (D-A) | #72 | `gtk-6f2.md:214` |
| 462 | `LoadedComparison.__init__` | `crates/hatate-gtk/src/review_compare.rs:68` `Loaded` | #72 | `gtk-6f2.md:215` |
| 470 | `load_comparison` | `crates/hatate-gtk/src/review_compare.rs:80` `load_comparison` | #72 | `gtk-6f2.md:217` |
| 546 | `CompareDialog.__init__` | `crates/hatate-gtk/src/compare_dialog.rs:47` `CompareDialog` | #72 | `gtk-6f2.md:218` |
| 664 | `CompareDialog._on_zoom_changed` | `crates/hatate-gtk/src/compare_dialog.rs` `new` (the `connect_zoom_changed` closure, D-C) | #72 | `gtk-6f2.md:220` |
| 667 | `CompareDialog._apply_mode` | `crates/hatate-gtk/src/compare_dialog.rs:286` `apply_mode` | #72 | `gtk-6f2.md:221` |
| 690 | `CompareDialog._show_differences` | `crates/hatate-gtk/src/compare_dialog.rs:312` `show_differences`, `crates/hatate-gtk/src/compare_dialog.rs:354` `present_differences` | #72 | `gtk-6f2.md:222` |
| 722 | `CompareDialog._build_difference_overlay` | `crates/hatate-gtk/src/review_compare.rs:56` `build_panes` | #72 | `gtk-6f2.md:223` |
| 725 | `CompareDialog._load` | `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` (the worker job), `crates/hatate-gtk/src/compare_dialog.rs:375` `loaded` | #72 | `gtk-6f2.md:224` |
| 737 | `CompareDialog._build_summary` | `crates/hatate-gtk/src/compare_dialog.rs:483` `summarize`, `crates/hatate-core/src/compare_view.rs:499` `build_summary` | #72 | `gtk-6f2.md:225` |
| 802 | `_pil_from_pixmap` | n/a: no `QPixmap`; the pictures are `Rgba` buffers (`rgb_of` drops the alpha) | #72 | `gtk-6f2.md:226` |
| 826 | `_pixmap_from_pil_rgba` | `crates/hatate-gtk/src/review_compare.rs:161` `texture_of` (`gdk::Texture` from the `Rgba`) | #72 | `gtk-6f2.md:227` |
| 837 | `_pixmap_from_pil` | `crates/hatate-gtk/src/review_compare.rs:161` `texture_of` | #72 | `gtk-6f2.md:228` |
| 849 | `_human` | `crates/hatate-core/src/compare_view.rs:480` `human` | #72 | `gtk-6f2.md:229` |

### `gui/filter_bar.py` (17 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 34 | `set_all_checked` | `filter_bar.rs` | #67 | `queue-view.md:159` |
| 43 | `summarise_selection` | `entry_filter::summarise_selection`, `describe_hidden`, `hidden_count_text` | #67 | `queue-view.md:160` |
| 61 | `describe_hidden` | `entry_filter::summarise_selection`, `describe_hidden`, `hidden_count_text` | #67 | `queue-view.md:160` |
| 66 | `hidden_count_text` | `entry_filter::summarise_selection`, `describe_hidden`, `hidden_count_text` | #67 | `queue-view.md:160` |
| 84 | `FilterBar.__init__` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 174 | `FilterBar._speak_field` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 182 | `FilterBar.current_filter` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 191 | `FilterBar.rebuild_menus` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 244 | `FilterBar.refresh` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 261 | `FilterBar.clear` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 278 | `FilterBar._add_all_none` | `hatate-gtk::filter_bar` | #67 | `queue-view.md:161` |
| 291 | `FilterBar._set_all_statuses` | `filter_bar.rs` `FilterBar::set_all` (`_set_all_*`, one function over the three menus) | #67 | `queue-view.md:161` |
| 300 | `FilterBar._set_all_sites` | `filter_bar.rs` `FilterBar::set_all` (`_set_all_*`, one function over the three menus) | #67 | `queue-view.md:161` |
| 305 | `FilterBar._set_all_upscale_verdicts` | `filter_bar.rs` `FilterBar::set_all` (`_set_all_*`, one function over the three menus) | #67 | `queue-view.md:161` |
| 310 | `FilterBar._on_status_toggled` | `filter_bar.rs` `FilterBar::toggled` (`_on_*_toggled`, one function over the three menus) | #67 | `queue-view.md:161` |
| 320 | `FilterBar._on_site_toggled` | `filter_bar.rs` `FilterBar::toggled` (`_on_*_toggled`, one function over the three menus) | #67 | `queue-view.md:161` |
| 327 | `FilterBar._on_upscale_verdict_toggled` | `filter_bar.rs` `FilterBar::toggled` (`_on_*_toggled`, one function over the three menus) | #67 | `queue-view.md:161` |

### `gui/fonts.py` (1 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 31 | `register_fonts` | **port**: `fonts::load / fonts::load_default` | #57 (+#60, #61, #62) | `gtk-shell.md:357` |

### `gui/hydrus_query_dialog.py` (4 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 26 | `HydrusQueryDialog.__init__` | `hydrus_query::HydrusQueryDialog::build` | #74 | `gtk-6g.md:253` |
| 60 | `HydrusQueryDialog._do_search` | `HydrusQueryDialog::search` (+ `on_searched`, `result_label`, `query_tags`) | #74 | `gtk-6g.md:254` |
| 89 | `HydrusQueryDialog._do_import` | `HydrusQueryDialog::import_selected` (+ `on_imported`) | #74 | `gtk-6g.md:255` |
| 131 | `_parse_tag` | `hydrus_query::parse_hydrus_tag` | #74 | `gtk-6g.md:256` |

### `gui/image_table_model.py` (34 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 77 | `default_column_widths` | `table_cells::default_column_widths` | #67 | `queue-view.md:147` |
| 111 | `_upscale_glyph` | `table_cells::cell_text` and `foreground` (Upscale column) | #67 | `queue-view.md:148` |
| 124 | `_upscale_weight` | `table_cells::cell_text` and `foreground` (Upscale column) | #67 | `queue-view.md:148` |
| 134 | `_entry_size_scale` | `table_cells::size_scale`, `scale_text`, `size_delta_label`, `size_delta_glyph`, `size_delta_ink` | #67 | `queue-view.md:149` |
| 146 | `_scale_text` | `table_cells::size_scale`, `scale_text`, `size_delta_label`, `size_delta_glyph`, `size_delta_ink` | #67 | `queue-view.md:149` |
| 156 | `_entry_size_delta_label` | `table_cells::size_scale`, `scale_text`, `size_delta_label`, `size_delta_glyph`, `size_delta_ink` | #67 | `queue-view.md:149` |
| 169 | `_size_delta_glyph` | `table_cells::size_scale`, `scale_text`, `size_delta_label`, `size_delta_glyph`, `size_delta_ink` | #67 | `queue-view.md:149` |
| 181 | `_size_delta_weight` | `table_cells::size_scale`, `scale_text`, `size_delta_label`, `size_delta_glyph`, `size_delta_ink` | #67 | `queue-view.md:149` |
| 197 | `_entry_engine_label` | `table_cells::engine_label`, `is_interrupted`, `cache_label`; the Sent label is a branch of `cell_text` | #67 | `queue-view.md:150` |
| 210 | `_is_interrupted` | `table_cells::engine_label`, `is_interrupted`, `cache_label`; the Sent label is a branch of `cell_text` | #67 | `queue-view.md:150` |
| 221 | `_entry_cache_label` | `table_cells::engine_label`, `is_interrupted`, `cache_label`; the Sent label is a branch of `cell_text` | #67 | `queue-view.md:150` |
| 237 | `_entry_sent_label` | `table_cells::engine_label`, `is_interrupted`, `cache_label`; the Sent label is a branch of `cell_text` | #67 | `queue-view.md:150` |
| 261 | `ImageTableModel.__init__` | `queue_view::QueueView` over the `ColumnView`; `Column::title` | #67 | `queue-view.md:151` |
| 289 | `ImageTableModel.rowCount` | `queue_view::QueueView` over the `ColumnView`; `Column::title` | #67 | `queue-view.md:151` |
| 297 | `ImageTableModel.columnCount` | `queue_view::QueueView` over the `ColumnView`; `Column::title` | #67 | `queue-view.md:151` |
| 302 | `ImageTableModel.headerData` | `queue_view::QueueView` over the `ColumnView`; `Column::title` | #67 | `queue-view.md:151` |
| 327 | `ImageTableModel.data` | `table_cells::cell_text`, `chip`, `foreground`, `italic`, `tooltip` | #67 | `queue-view.md:152` |
| 381 | `ImageTableModel._display` | `table_cells::cell_text`, `chip`, `foreground`, `italic`, `tooltip` | #67 | `queue-view.md:152` |
| 423 | `ImageTableModel._chip` | `table_cells::cell_text`, `chip`, `foreground`, `italic`, `tooltip` | #67 | `queue-view.md:152` |
| 446 | `ImageTableModel._foreground` | `table_cells::cell_text`, `chip`, `foreground`, `italic`, `tooltip` | #67 | `queue-view.md:152` |
| 478 | `ImageTableModel._tooltip` | `table_cells::cell_text`, `chip`, `foreground`, `italic`, `tooltip` | #67 | `queue-view.md:152` |
| 499 | `ImageTableModel.refresh_all` | `QueueView` rebuild and per-row refresh (`queue_view.rs`) | #67 | `queue-view.md:153` |
| 523 | `ImageTableModel.refresh_entry` | `QueueView` rebuild and per-row refresh (`queue_view.rs`) | #67 | `queue-view.md:153` |
| 534 | `ImageTableModel.entry_at` | `QueueView::cell`, `selected_ids`, `row_of` | #67 | `queue-view.md:154` |
| 545 | `ImageTableModel.entries_at` | `QueueView::cell`, `selected_ids`, `row_of` | #67 | `queue-view.md:154` |
| 555 | `ImageTableModel.visible_entries` | `QueueView::cell`, `selected_ids`, `row_of` | #67 | `queue-view.md:154` |
| 559 | `ImageTableModel.row_of` | `QueueView::cell`, `selected_ids`, `row_of` | #67 | `queue-view.md:154` |
| 566 | `ImageTableModel.set_filter` | `QueueView::set_filter`, `visible_count`, `total_count` | #67 | `queue-view.md:155` |
| 575 | `ImageTableModel.filter` | `QueueView::set_filter`, `visible_count`, `total_count` | #67 | `queue-view.md:155` |
| 578 | `ImageTableModel.visible_count` | `QueueView::set_filter`, `visible_count`, `total_count` | #67 | `queue-view.md:155` |
| 581 | `ImageTableModel.total_count` | `QueueView::set_filter`, `visible_count`, `total_count` | #67 | `queue-view.md:155` |
| 584 | `ImageTableModel.refresh_row` | `QueueView` rebuild and per-row refresh (`queue_view.rs`) | #67 | `queue-view.md:153` |
| 592 | `header_layout_is_usable` | `table_cells::header_layout_is_usable`, `sort_key` / `SortKey::compare` | #67 | `queue-view.md:156` |
| 613 | `sort_key_for_column` | `table_cells::header_layout_is_usable`, `sort_key` / `SortKey::compare` | #67 | `queue-view.md:156` |

### `gui/log_viewer_dialog.py` (5 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 24 | `LogViewerDialog.__init__` | **port**: `logview::LogViewer` | #57 (+#60, #61, #62) | `gtk-shell.md:401` |
| 76 | `LogViewerDialog.refresh` | **port**: `logview::LogViewer::refresh` | #57 (+#60, #61, #62) | `gtk-shell.md:403` |
| 90 | `LogViewerDialog._toggle_crash_log` | **port**: `logview::LogViewer (toggle)` | #57 (+#60, #61, #62) | `gtk-shell.md:404` |
| 95 | `LogViewerDialog._copy_all` | **port**: `logview::LogViewer (copy all)` | #57 (+#60, #61, #62) | `gtk-shell.md:405` |
| 98 | `LogViewerDialog._open_log_folder` | **port**: `logview::LogViewer (open folder)` | #57 (+#60, #61, #62) | `gtk-shell.md:406` |

### `gui/main_window.py` (161 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 148 | `_format_duration` | `eta::format_duration` | #58 | `phase6b-pure-core.md:114` |
| 176 | `_mark_interrupted_mid_search` | **port**: `hatate_core::model::mark_interrupted_mid_search (merged core; called from restore)` | #57 (+#60, #61, #62) | `gtk-shell.md:153` |
| 194 | `_plural` | `stop_record::images` (private) | #59 | `phase6a-core-support.md:115` |
| 198 | `_interrupted_prose` | The launch decision as a pure function over what the restore read. | #59 | `phase6a-core-support.md:12` |
| 225 | `_is_waiting_review` | `RestoreCounts::of` | #59 | `phase6a-core-support.md:117` |
| 231 | `_waiting_review_count` | `RestoreCounts::of` | #59 | `phase6a-core-support.md:117` |
| 235 | `_quota_pause_prose` | **port**: `text::quota_pause_prose` | #57 (+#60, #61, #62) | `gtk-shell.md:158` |
| 257 | `_format_finish_time` | `hatate_core::eta::format_finish_time` (6.a), called from `search_run.rs` `finish_time` | #63 | `gtk-6e.md:214` |
| 282 | `_host_from_url` | `crates/hatate-gtk/src/checks.rs:56` `host_from_url` (the detail lines of the availability check) and `review_ops.rs:99` `host_from_url` (the candidate combo label) | #63 | this table (6.j) |
| 317 | `_load_preview_pixmap` | `crates/hatate-gtk/src/review_ops.rs:34` `PREVIEW_DECODE` (2048) through `decode_thumbnail`, called from the preview job at `review_ops.rs:425` | #71 | this table (6.j) |
| 342 | `_muted_text_color` | no function: the secondary-text colour is the `ink_65`/`ink_46` token pair in `theme.rs` CSS (`tokens.rs` parses `tokens.css`), so no widget asks the theme for it | #57 (+#60, #61, #62) | this table (6.j) |
| 353 | `_nothing_to_write_text` | `crates/hatate-core/src/sidecar.rs:333` `nothing_to_write_text` | #58 | this table (6.j) |
| 370 | `_written_summary` | `crates/hatate-core/src/sidecar.rs:350` `written_summary` | #58 | this table (6.j) |
| 388 | `_engine_alert_forwarder` | `hatate-gtk/src/search_run.rs` `launch_search_worker` (the listener posts to the window's inbox; the window drains it while it is alive) | #63 | `gtk-6e.md:215` |
| 403 | `_engine_alert_forwarder.<locals>.forward` | nested in `_engine_alert_forwarder`: `hatate-gtk/src/search_run.rs` `launch_search_worker` (the listener posts to the window's inbox; the window drains it while it is alive) | #63 | `gtk-6e.md:215` |
| 421 | `MainWindow.__init__` | `crates/hatate-gtk/src/window.rs:535`, `crates/hatate-gtk/src/mcp_bridge.rs:273`, `crates/hatate-gtk/src/mcp_bridge.rs:228` | #76 | `gtk-6i.md:76` |
| 544 | `MainWindow._show_persisted_quota_pause` | **port**: `banners::show_persisted_quota_pause` | #57 (+#60, #61, #62) | `gtk-shell.md:167` |
| 571 | `MainWindow._build_menu` | **port**: `menu::menus + menu::model + window (the whole menubar as a data table; later tickets' items are registered disabled and marked DEFERRED(6.x))` | #57 (+#60, #61, #62) | `gtk-shell.md:168` |
| 726 | `MainWindow._build_hydrus_menu` | `menu.rs` (6.c; the entries are live here) | #74 | `gtk-6g.md:247` |
| 756 | `MainWindow._build_sites_menu` | `menu.rs` Sites menu table, built from `all_site_options()`; its check items are `install_site_actions` | #63 | `gtk-6e.md:216` |
| 781 | `MainWindow._on_site_toggled` | `hatate-gtk/src/search_run.rs:488` `on_site_toggled` | #63 | `gtk-6e.md:217` |
| 791 | `MainWindow._set_all_sites` | `hatate-gtk/src/search_run.rs:509` `set_all_sites` | #63 | `gtk-6e.md:218` |
| 803 | `MainWindow._build_central_widget` | **port**: `window::MainWindow::new (shell stack and queue page); the table is 6.d` | #57 (+#60, #61, #62) | `gtk-shell.md:173` |
| 810 | `MainWindow._build_queue_page` | **port**: `window::MainWindow::new (queue stack: rows page, drop zone, Nothing survived); the table and filter bar are 6.d` | #57 (+#60, #61, #62) | `gtk-shell.md:174` |
| 967 | `MainWindow._build_drop_zone` | **port**: `widgets::empty_prose + window (the empty.html layout)` | #57 (+#60, #61, #62) | `gtk-shell.md:175` |
| 1017 | `MainWindow._refresh_queue_empty_state` | **port**: `window::MainWindow::refresh_queue_empty_state` | #57 (+#60, #61, #62) | `gtk-shell.md:176` |
| 1035 | `MainWindow.ghost_glyph_for` | **port**: `window::MainWindow::ghost_glyph_for` | #57 (+#60, #61, #62) | `gtk-shell.md:177` |
| 1043 | `MainWindow._apply_filter` | `filter_bar.rs`, `queue_view::set_filter` | #67 | `queue-view.md:168` |
| 1060 | `MainWindow._refresh_filter_ui` | `filter_bar.rs`, `queue_view::set_filter` | #67 | `queue-view.md:168` |
| 1067 | `MainWindow._selected_entries` | `QueueView::selected_ids`, the reselect in `queue_view.rs` | #67 | `queue-view.md:169` |
| 1072 | `MainWindow._reselect` | `QueueView::selected_ids`, the reselect in `queue_view.rs` | #67 | `queue-view.md:169` |
| 1099 | `MainWindow._update_preview` | `crates/hatate-gtk/src/review_ops.rs:352` | #71 | `gtk-6f.md:237` |
| 1144 | `MainWindow._on_mode_changed` | **port**: `window::MainWindow::on_mode_changed (the hook; Review's body is 6.f)` | #57 (+#60, #61, #62) | `gtk-shell.md:183` |
| 1158 | `MainWindow._populate_candidate_combo` | `crates/hatate-gtk/src/review_ops.rs:497` | #71 | `gtk-6f.md:238` |
| 1186 | `MainWindow._sync_candidate_combo_tooltip` | `crates/hatate-gtk/src/review_ops.rs:523` | #71 | `gtk-6f.md:239` |
| 1194 | `MainWindow._on_candidate_combo_changed` | `crates/hatate-gtk/src/review_ops.rs:535` | #71 | `gtk-6f.md:240` |
| 1207 | `MainWindow._ensure_candidate_loaded` | `crates/hatate-gtk/src/review_ops.rs:566` | #71 | `gtk-6f.md:241` |
| 1238 | `MainWindow._run_dialog` | `dialogs.rs` `Prompter::match_conditions` (the dialog is an `AlertDialog`; no helper to port) | #71 | `gtk-6f.md:242` |
| 1265 | `MainWindow._on_candidate_fetched` | `crates/hatate-gtk/src/review_ops.rs:625` | #71 | `gtk-6f.md:243` |
| 1293 | `MainWindow._refresh_tag_list` | `crates/hatate-gtk/src/review_ops.rs:677` | #71 | `gtk-6f.md:244` |
| 1314 | `MainWindow._on_edit_tags_toggled` | `crates/hatate-gtk/src/review_ops.rs:734` | #71 | `gtk-6f.md:245` |
| 1319 | `MainWindow._on_tag_item_edited` | `crates/hatate-gtk/src/review_ops.rs:746` | #71 | `gtk-6f.md:246` |
| 1347 | `MainWindow._show_matched_preview_menu` | `crates/hatate-gtk/src/review_ops.rs:885` | #71 | `gtk-6f.md:247` |
| 1371 | `MainWindow.dragEnterEvent` | **port**: `window (the drop target accepts files)` | #57 (+#60, #61, #62) | `gtk-shell.md:194` |
| 1375 | `MainWindow.dropEvent` | **port**: `session_ops::drop_paths` | #57 (+#60, #61, #62) | `gtk-shell.md:195` |
| 1388 | `MainWindow.action_add_files` | **port**: `session_ops::action_add_files` | #57 (+#60, #61, #62) | `gtk-shell.md:196` |
| 1395 | `MainWindow.action_add_folder` | **port**: `session_ops::action_add_folder` | #57 (+#60, #61, #62) | `gtk-shell.md:197` |
| 1400 | `MainWindow._restore_saved_session` | The launch decision as a pure function over what the restore read. | #59 | `phase6a-core-support.md:12` |
| 1462 | `MainWindow._show_nothing_survived` | the widgets are 6.d's; the decision is `launch_banner` | #59 | `phase6a-core-support.md:119` |
| 1487 | `MainWindow._dismiss_nothing_survived` | **port**: `banners::dismiss_nothing_survived` | #57 (+#60, #61, #62) | `gtk-shell.md:200` |
| 1500 | `MainWindow._show_run_banner` | the widgets are 6.d's; the decision is `launch_banner` | #59 | `phase6a-core-support.md:119` |
| 1533 | `MainWindow.action_reconcile_with_hydrus` | `hydrus_ops::action_reconcile_with_hydrus` | #74 | `gtk-6g.md:234` |
| 1541 | `MainWindow._start_hydrus_reconcile` | `hydrus_ops::start_hydrus_reconcile` | #74 | `gtk-6g.md:235` |
| 1560 | `MainWindow._on_hydrus_reconciled` | `hydrus_ops::on_hydrus_reconciled` | #74 | `gtk-6g.md:236` |
| 1569 | `MainWindow._start_missing_file_check` | `crates/hatate-gtk/src/window.rs:55` the check over `hatate_core::missing_files::check_session_files`, run as a worker-bridge job at restore (the thread shell 6.b left to the window) | #57 (+#60, #61, #62) | this table (6.j) |
| 1589 | `MainWindow._on_missing_files_checked` | the loop and the two messages are `missing_files::flag_missing`, `missing_status_text`, `missing_dialog_text`; the handler (closing guard, refresh, modal box) is the window's | #58 | `phase6b-pure-core.md:117` |
| 1627 | `MainWindow.action_save_session_as` | **port**: `session_ops::action_save_session_as` | #57 (+#60, #61, #62) | `gtk-shell.md:207` |
| 1657 | `MainWindow.action_export_results` | `table_actions` (Export Results) over `export` (6.b) | #67 | `queue-view.md:170` |
| 1712 | `MainWindow.action_write_tag_files` | `table_actions`, `sidecar::{plan,write}` | #67 | `queue-view.md:171` |
| 1757 | `MainWindow._ask_about_writing_tag_files` | `table_actions`, `sidecar::{plan,write}` | #67 | `queue-view.md:171` |
| 1842 | `MainWindow.action_open_session` | **port**: `session_ops::action_open_session` | #57 (+#60, #61, #62) | `gtk-shell.md:211` |
| 1908 | `MainWindow._stop_background_workers` | **port**: `session_ops::stop_background_workers` | #57 (+#60, #61, #62) | `gtk-shell.md:212` |
| 1921 | `MainWindow.action_clear_session` | **port**: `session_ops::action_clear_session` | #57 (+#60, #61, #62) | `gtk-shell.md:213` |
| 1944 | `MainWindow._import_files` | **port**: `session_ops::import_files` | #57 (+#60, #61, #62) | `gtk-shell.md:214` |
| 1975 | `MainWindow._on_file_hash_progress` | **port**: `session_ops::on_file_hash_progress` | #57 (+#60, #61, #62) | `gtk-shell.md:215` |
| 1984 | `MainWindow._on_file_hash_finished` | **port**: `session_ops::on_file_hash_finished` | #57 (+#60, #61, #62) | `gtk-shell.md:216` |
| 2035 | `MainWindow._start_thumbnail_generation` | `thumbnails.rs` | #67 | `queue-view.md:172` |
| 2068 | `MainWindow._on_thumbnail_ready` | `thumbnails.rs` | #67 | `queue-view.md:172` |
| 2100 | `MainWindow._on_thumbnail_progress` | the progress strip of the non-lazy pass in `thumbnails.rs` | #67 | `queue-view.md:173` |
| 2106 | `MainWindow._on_thumbnails_finished` | `thumbnails.rs` | #67 | `queue-view.md:172` |
| 2122 | `MainWindow._lookup_existing_hydrus_tags` | `hydrus_ops::lookup_existing_hydrus_tags` | #74 | `gtk-6g.md:232` |
| 2134 | `MainWindow._on_hydrus_tags_looked_up` | `hydrus_ops::on_hydrus_tags_looked_up` | #74 | `gtk-6g.md:233` |
| 2148 | `MainWindow.action_query_hydrus` | `hydrus_query::action_query_hydrus` (+ `on_query_hydrus_done`) | #74 | `gtk-6g.md:245` |
| 2170 | `MainWindow._add_tags_to_current` | `crates/hatate-gtk/src/review_ops.rs:832` | #71 | `gtk-6f.md:248` |
| 2194 | `MainWindow.action_open_log` | **port**: `session_ops::action_open_log` | #57 (+#60, #61, #62) | `gtk-shell.md:225` |
| 2201 | `MainWindow.action_parser_health` | `hatate-gtk/src/parser_health.rs:459` `action_parser_health` | #63 | `gtk-6e.md:219` |
| 2204 | `MainWindow.action_pawchive_index` | `pawchive_dialog::action_pawchive_index` (+ `close_pawchive_dialog`) | #74 | `gtk-6g.md:246` |
| 2217 | `MainWindow._warn_about_broken_parsers` | `hatate-gtk/src/parser_health.rs:467` `warn_about_broken_parsers` | #63 | `gtk-6e.md:220` |
| 2233 | `MainWindow.action_view_logs` | **port**: `session_ops::action_view_logs` | #57 (+#60, #61, #62) | `gtk-shell.md:229` |
| 2236 | `MainWindow._open_crash_log` | **port**: `session_ops::open_crash_log` | #57 (+#60, #61, #62) | `gtk-shell.md:230` |
| 2242 | `MainWindow._update_clear_cache_action_label` | `table_menu::clear_cache_label`, `window.rs`, `menu.rs`, `table_actions` | #67 | `queue-view.md:174` |
| 2247 | `MainWindow.action_clear_search_cache` | `table_menu::clear_cache_label`, `window.rs`, `menu.rs`, `table_actions` | #67 | `queue-view.md:174` |
| 2264 | `MainWindow.action_toggle_search` | `hatate-gtk/src/search_run.rs:534` `action_toggle_search` | #63 | `gtk-6e.md:221` |
| 2270 | `MainWindow.action_start_search` | `hatate-gtk/src/search_run.rs:543` `action_start_search` | #63 | `gtk-6e.md:222` |
| 2282 | `MainWindow.action_stop_search` | `hatate-gtk/src/search_run.rs:574` `action_stop_search` | #63 | `gtk-6e.md:223` |
| 2289 | `MainWindow._on_worker_progress` | `hatate-gtk/src/search_run.rs:728` `on_worker_progress` | #63 | `gtk-6e.md:224` |
| 2295 | `MainWindow._refresh_run_progress_label` | `session_ops.rs` `refresh_run_progress_label` (6.c); its live half is `search_run.rs` `live_eta_text` | #63 | `gtk-6e.md:225` |
| 2327 | `MainWindow._idle_run_text` | `session_ops.rs` `idle_run_text` (6.c) | #63 | `gtk-6e.md:226` |
| 2336 | `MainWindow._on_worker_waiting` | `hatate-gtk/src/search_run.rs:739` `on_worker_waiting` | #63 | `gtk-6e.md:227` |
| 2339 | `MainWindow._on_wait_countdown` | `hatate-gtk/src/search_run.rs:747` `on_wait_countdown` | #63 | `gtk-6e.md:228` |
| 2347 | `MainWindow._on_auto_imported` | `hatate-gtk/src/search_run.rs:793` `on_auto_imported` (the row leaves the list when the run imported it; the import itself is `batch/run.rs`, Phase 5). The ticket lists it; `gtk-shell.md` had left it to 6.g | #63 | `gtk-6e.md:229` |
| 2373 | `MainWindow._on_worker_image_updated` | `hatate-gtk/src/search_run.rs:763` `on_worker_image_updated` (left to: the row model follows the worker's copy; selection and the preview refresh are `DEFERRED(6.d)` (`search_run.rs:779`)) | #63 | `gtk-6e.md:230` |
| 2400 | `MainWindow._refresh_saucenao_quota_label` | `hatate-gtk/src/search_run.rs:851` `refresh_saucenao_quota_label` | #63 | `gtk-6e.md:231` |
| 2411 | `MainWindow._render_saucenao_quota_label` | `hatate-gtk/src/search_run.rs:851` (the same function renders) | #63 | `gtk-6e.md:232` |
| 2417 | `MainWindow._on_paused_out_of_quota` | `hatate-gtk/src/search_run.rs:821` `on_paused_out_of_quota` | #63 | `gtk-6e.md:233` |
| 2441 | `MainWindow._show_quota_banner` | `banners.rs:192` `show_quota_banner` (6.c) | #63 | `gtk-6e.md:234` |
| 2472 | `MainWindow._hide_quota_banner` | `banners.rs:229` `hide_quota_banner` (6.c) | #63 | `gtk-6e.md:235` |
| 2481 | `MainWindow._review_the_waiting` | `crates/hatate-gtk/src/window.rs:591` | #71 | `gtk-6f.md:249` |
| 2491 | `MainWindow._resume_without_saucenao` | `hatate-gtk/src/search_run.rs:1064` `resume_without_saucenao` | #63 | `gtk-6e.md:237` |
| 2507 | `MainWindow._on_continuing_without_saucenao` | `hatate-gtk/src/search_run.rs:811` `on_continuing_without_saucenao` | #63 | `gtk-6e.md:238` |
| 2521 | `MainWindow._on_engine_alert` | `hatate-gtk/src/search_run.rs:1098` `on_engine_alert` (left to: the remedy is in the body (D3)) | #63 | `gtk-6e.md:239` |
| 2548 | `MainWindow._on_worker_finished` | `hatate-gtk/src/search_run.rs:861` `on_worker_finished`, `on_run_ended` | #63 | `gtk-6e.md:240` |
| 2559 | `MainWindow._register_new_entries` | `queue_view.rs` (new entries reach the view), `session_ops.rs` | #67 | `queue-view.md:175` |
| 2566 | `MainWindow._on_header_clicked` | `queue_view` header cycle, `QueueModel::reorder` | #67 | `queue-view.md:176` |
| 2580 | `MainWindow._apply_sort` | `queue_view` header cycle, `QueueModel::reorder` | #67 | `queue-view.md:176` |
| 2606 | `MainWindow._restore_table_header_state` | `table_cells::restore_header_state`, `repair_header_sections`, `encode_header_state`; `window.rs`, `session_ops.rs` | #67 | `queue-view.md:177` |
| 2633 | `MainWindow._repair_header_after_restore` | `table_cells::restore_header_state`, `repair_header_sections`, `encode_header_state`; `window.rs`, `session_ops.rs` | #67 | `queue-view.md:177` |
| 2656 | `MainWindow._save_table_header_state` | `table_cells::restore_header_state`, `repair_header_sections`, `encode_header_state`; `window.rs`, `session_ops.rs` | #67 | `queue-view.md:177` |
| 2664 | `MainWindow._refresh_sent_count_label` | `session_ops.rs` (the status bar's sent count, 6.c) | #67 | `queue-view.md:178` |
| 2688 | `MainWindow._refresh_entry_row` | `queue_view.rs` | #67 | `queue-view.md:179` |
| 2701 | `MainWindow._refresh_table` | `queue_view.rs` | #67 | `queue-view.md:179` |
| 2749 | `MainWindow._get_local_pixmap` | `thumb_decode` and `hatate-core::viewport` (`visible_range_with_buffer`, `entries_needing_thumbnails`, 6.b) | #67 | `queue-view.md:181` |
| 2766 | `MainWindow._get_row_thumbnail` | `thumbnails.rs`, `queue_view.rs` | #67 | `queue-view.md:180` |
| 2795 | `MainWindow._schedule_viewport_thumbnails` | `thumbnails.rs`, `queue_view.rs` | #67 | `queue-view.md:180` |
| 2804 | `MainWindow._visible_row_range` | `thumb_decode` and `hatate-core::viewport` (`visible_range_with_buffer`, `entries_needing_thumbnails`, 6.b) | #67 | `queue-view.md:181` |
| 2830 | `MainWindow._update_visible_thumbnails` | `thumb_decode` and `hatate-core::viewport` (`visible_range_with_buffer`, `entries_needing_thumbnails`, 6.b) | #67 | `queue-view.md:181` |
| 2921 | `MainWindow._queue_thumbnail_regeneration` | `crates/hatate-gtk/src/thumbnails.rs:155` `schedule_viewport_thumbnails`: the 250 ms debounced pass (`thumbnails.rs:132`) asks again for every visible row whose texture is gone, so an evicted thumbnail is regenerated without a ... | #67 | this table (6.j) |
| 2930 | `MainWindow._run_thumbnail_regeneration` | `crates/hatate-gtk/src/thumbnails.rs:221` `run_thumbnail_pass`, one pass for the burst (the pass skips what is decoded or decoding) | #67 | this table (6.j) |
| 2943 | `MainWindow._show_table_context_menu` | `queue_menu.rs`, `table_actions` | #67 | `queue-view.md:183` |
| 2946 | `MainWindow._select_rows_by_status` | `queue_menu.rs`, `table_actions` | #67 | `queue-view.md:183` |
| 2952 | `MainWindow._select_rows_by_predicate` | `queue_menu.rs`, `table_actions` | #67 | `queue-view.md:183` |
| 2971 | `MainWindow._live_entries` | `table_actions.rs` (the live selection resolved to ids at build time) | #67 | `queue-view.md:184` |
| 2987 | `MainWindow._reset_rows` | `table_actions` | #67 | `queue-view.md:185` |
| 3037 | `MainWindow._set_rows_reviewed` | `crates/hatate-gtk/src/table_actions.rs:323` | #71 | `gtk-6f.md:250` |
| 3061 | `MainWindow._research_rows` | `hatate-gtk/src/search_run.rs:957` `research_rows` (left to: 6.d's context menu calls it (`queue_menu.rs:56`, wired here after #67 merged)) | #63 | `gtk-6e.md:241` |
| 3076 | `MainWindow._research_rows_with_engine` | `hatate-gtk/src/search_run.rs:986` `research_rows_with_engine` (left to: 6.d's context menu calls it (`queue_menu.rs:57`)) | #63 | `gtk-6e.md:242` |
| 3100 | `MainWindow._research_rows_with_opposite_engine` | `hatate-gtk/src/search_run.rs:1012` `research_rows_with_opposite_engine` (left to: 6.d's context menu calls it (`queue_menu.rs:58`)) | #63 | `gtk-6e.md:243` |
| 3146 | `MainWindow._check_match_availability` | `hatate-gtk/src/checks.rs:64` `check_match_availability` | #63 | `gtk-6e.md:244` |
| 3168 | `MainWindow._on_availability_progress` | `hatate-gtk/src/checks.rs:136` `on_availability_progress` | #63 | `gtk-6e.md:245` |
| 3172 | `MainWindow._on_availability_finished` | `hatate-gtk/src/checks.rs:145` `on_availability_finished` | #63 | `gtk-6e.md:246` |
| 3209 | `MainWindow._remove_dead_candidates` | `hatate-gtk/src/checks.rs:230` `remove_dead_candidates` | #63 | `gtk-6e.md:247` |
| 3224 | `MainWindow._open_compare_dialog` | `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` | #72 | `gtk-6f2.md:161` |
| 3240 | `MainWindow._check_rows_for_upscaling` | `hatate-gtk/src/checks.rs:251` `check_rows_for_upscaling` | #63 | `gtk-6e.md:248` |
| 3260 | `MainWindow._on_upscale_entry_checked` | `hatate-gtk/src/checks.rs:307` `on_upscale_entry_checked` | #63 | `gtk-6e.md:249` |
| 3281 | `MainWindow._on_upscale_check_finished` | `hatate-gtk/src/checks.rs:320` `on_upscale_check_finished` | #63 | `gtk-6e.md:250` |
| 3304 | `MainWindow._launch_search_worker` | `hatate-gtk/src/search_run.rs:587` `launch_search_worker` over `hatate_core::batch::run::search_pending_with` | #63 | `gtk-6e.md:251` |
| 3367 | `MainWindow._show_rows_in_hydrus` | `hydrus_ops::show_rows_in_hydrus` (+ `on_shown_in_hydrus`) | #74 | `gtk-6g.md:242` |
| 3431 | `MainWindow._explain_show_in_hydrus_failure` | `hydrus_ops::show_failure_text` | #74 | `gtk-6g.md:243` |
| 3463 | `MainWindow._delete_rows_from_hydrus` | `hydrus_ops::delete_rows_from_hydrus` (+ `on_delete_states`, `delete_in_hydrus`, `on_deleted_from_hydrus`) | #74 | `gtk-6g.md:244` |
| 3566 | `MainWindow._remove_rows` | `table_actions` (Remove asks, Cancel default) | #67 | `queue-view.md:186` |
| 3598 | `MainWindow._remove_entries` | `table_actions` (Remove asks, Cancel default) | #67 | `queue-view.md:186` |
| 3633 | `MainWindow._successor_surviving` | `queue_view.rs` (the next surviving row) | #67 | `queue-view.md:187` |
| 3651 | `MainWindow._sync_preview_to_selection` | `crates/hatate-gtk/src/review_compare.rs:453` `sync_comparison_to_selection`, called from `review_ops.rs` `on_selection_changed` | #72 | `gtk-6f2.md:162` |
| 3664 | `MainWindow.select_table_row` | the queue half in `queue_view.rs`; the review half is 6.f | #67 | `queue-view.md:188` |
| 3689 | `MainWindow.action_open_matched_url` | `crates/hatate-gtk/src/review_ops.rs:939` | #71 | `gtk-6f.md:253` |
| 3693 | `MainWindow.action_show_in_queue` | `crates/hatate-gtk/src/review_ops.rs:963` | #71 | `gtk-6f.md:254` |
| 3704 | `MainWindow._current_entry` | `review_ops.rs` `current_id` | #71 | `gtk-6f.md:255` |
| 3710 | `MainWindow._on_selection_changed` | the queue half in `queue_view.rs`; the review half is 6.f | #67 | `queue-view.md:188` |
| 3719 | `MainWindow._remove_selected_tags` | `crates/hatate-gtk/src/review_ops.rs:784` | #71 | `gtk-6f.md:256` |
| 3731 | `MainWindow.action_send_to_hydrus` | `hydrus_ops::action_send_to_hydrus` (+ `on_files_sent`) | #74 | `gtk-6g.md:237` |
| 3789 | `MainWindow.action_import_url_to_hydrus` | `hydrus_ops::action_import_url_to_hydrus` (+ `on_urls_sent`, `start_import_poll`) | #74 | `gtk-6g.md:238` |
| 3903 | `MainWindow._on_hydrus_import_resolved` | `hydrus_ops::on_hydrus_import_resolved` | #74 | `gtk-6g.md:239` |
| 3921 | `MainWindow._on_hydrus_import_poll_finished` | `hydrus_ops::on_hydrus_import_poll_finished` | #74 | `gtk-6g.md:240` |
| 3961 | `MainWindow.action_download_and_send_to_hydrus` | `hydrus_ops::action_download_and_send_to_hydrus` (+ `on_downloaded_and_sent`) | #74 | `gtk-6g.md:241` |
| 4047 | `MainWindow.action_open_settings` | `crates/hatate-gtk/src/settings_dialog.rs:935`, `crates/hatate-gtk/src/mcp_bridge.rs:289` | #76 | `gtk-6i.md:77` |
| 4079 | `MainWindow.action_set_theme` | **port**: `window::MainWindow::set_theme` | #57 (+#60, #61, #62) | `gtk-shell.md:306` |
| 4099 | `MainWindow.action_edit_conditions` | `crates/hatate-gtk/src/review_ops.rs:813` | #71 | `gtk-6f.md:257` |
| 4105 | `MainWindow.eventFilter` | `hatate_core::table_cells::default_column_widths` (`table_cells.rs:457`) and `queue_view.rs:433` `FIRST_RUN_WIDTHS`; the `ColumnView` has no event filter, the first-run widths are fixed numbers, not a re-worked share of the ... | #67 | this table (6.j) |
| 4125 | `MainWindow.showEvent` | `crates/hatate-gtk/src/thumbnails.rs:155` `schedule_viewport_thumbnails`, first called when the window maps | #67 | this table (6.j) |
| 4134 | `MainWindow.resizeEvent` | `crates/hatate-gtk/src/thumbnails.rs:155` `schedule_viewport_thumbnails`, called again on the debounced pass (`thumbnails.rs:132`, `:147`) | #67 | this table (6.j) |
| 4139 | `MainWindow.closeEvent` | `crates/hatate-gtk/src/session_ops.rs:846`, `crates/hatate-gtk/src/mcp_bridge.rs:309` | #76 | `gtk-6i.md:78` |

### `gui/match_conditions_dialog.py` (2 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 17 | `MatchConditionsDialog.__init__` | `dialogs.rs` `Prompter::match_conditions` (four controls over OK and Cancel) | #71 | `gtk-6f.md:320` |
| 57 | `MatchConditionsDialog.apply_to` | `review_ops.rs` `action_edit_conditions` (the closure stores and saves on OK) | #71 | `gtk-6f.md:321` |

### `gui/mcp_bridge.py` (56 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 81 | `MainThreadInvoker.__init__` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 96 | `MainThreadInvoker._run` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 102 | `MainThreadInvoker.call` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 115 | `MainThreadInvoker.call.<locals>._emit` | nested in `call`: `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: ... | #76 | `gtk-6i.md:79` |
| 129 | `MainThreadInvoker.shutdown` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 148 | `McpToolHandlers.__init__` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 158 | `McpToolHandlers._mcp_settings` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 161 | `McpToolHandlers._run_tool` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 196 | `McpToolHandlers._entry_at` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 209 | `McpToolHandlers.list_queue` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 214 | `McpToolHandlers.list_queue.<locals>.perform` | nested in `list_queue`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 215 | `McpToolHandlers.list_queue.<locals>.perform.<locals>.work` | nested in `list_queue`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 234 | `McpToolHandlers.get_entry` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 237 | `McpToolHandlers.get_entry.<locals>.perform` | nested in `get_entry`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 238 | `McpToolHandlers.get_entry.<locals>.perform.<locals>.work` | nested in `get_entry`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 247 | `McpToolHandlers.get_candidates` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 250 | `McpToolHandlers.get_candidates.<locals>.perform` | nested in `get_candidates`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 251 | `McpToolHandlers.get_candidates.<locals>.perform.<locals>.work` | nested in `get_candidates`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 277 | `McpToolHandlers.get_images` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 280 | `McpToolHandlers.get_images.<locals>.perform` | nested in `get_images`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 281 | `McpToolHandlers.get_images.<locals>.perform.<locals>.lookup` | nested in `get_images`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 311 | `McpToolHandlers.get_diff` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 314 | `McpToolHandlers.get_diff.<locals>.perform` | nested in `get_diff`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 315 | `McpToolHandlers.get_diff.<locals>.perform.<locals>.lookup` | nested in `get_diff`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 343 | `McpToolHandlers.list_actions` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 344 | `McpToolHandlers.list_actions.<locals>.perform` | nested in `list_actions`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 363 | `McpToolHandlers.select_candidate` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 366 | `McpToolHandlers.select_candidate.<locals>.perform` | nested in `select_candidate`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events ... | #76 | `gtk-6i.md:80` |
| 367 | `McpToolHandlers.select_candidate.<locals>.perform.<locals>.work` | nested in `select_candidate`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events ... | #76 | `gtk-6i.md:80` |
| 383 | `McpToolHandlers.toggle_reviewed` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 386 | `McpToolHandlers.toggle_reviewed.<locals>.perform` | nested in `toggle_reviewed`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 387 | `McpToolHandlers.toggle_reviewed.<locals>.perform.<locals>.work` | nested in `toggle_reviewed`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 400 | `McpToolHandlers.research` | `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) | #76 | `gtk-6i.md:81` |
| 403 | `McpToolHandlers.research.<locals>.perform` | nested in `research`: `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) | #76 | `gtk-6i.md:81` |
| 409 | `McpToolHandlers._perform_research` | `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) | #76 | `gtk-6i.md:81` |
| 410 | `McpToolHandlers._perform_research.<locals>.start` | nested in `_perform_research`: `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) | #76 | `gtk-6i.md:81` |
| 449 | `McpToolHandlers._perform_research.<locals>.read_results` | `crates/hatate-gtk/src/mcp_bridge.rs:199`, `crates/hatate-gtk/src/mcp_bridge.rs:219` (the window starts its own search and the tool goes on when it ends) | #76 | `gtk-6i.md:81` |
| 465 | `McpToolHandlers.send_upload` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 468 | `McpToolHandlers.send_upload.<locals>.perform` | nested in `send_upload`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 469 | `McpToolHandlers.send_upload.<locals>.perform.<locals>.work` | nested in `send_upload`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 490 | `McpToolHandlers.send_url` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 493 | `McpToolHandlers.send_url.<locals>.perform` | nested in `send_url`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 494 | `McpToolHandlers.send_url.<locals>.perform.<locals>.work` | nested in `send_url`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 526 | `McpToolHandlers.download_send` | `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: `crates/hatate-gtk/src/mcp_bridge.rs:128`, ... | #76 | `gtk-6i.md:79` |
| 529 | `McpToolHandlers.download_send.<locals>.perform` | nested in `download_send`: `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: ... | #76 | `gtk-6i.md:79` |
| 530 | `McpToolHandlers.download_send.<locals>.perform.<locals>.work` | nested in `download_send`: `crates/hatate-core/src/queue.rs:517` and `QueueInbox::channel` (`crates/hatate-core/src/queue.rs:526`); the owner side is 5.e's. The window pumps the inbox from its own loop: ... | #76 | `gtk-6i.md:79` |
| 558 | `McpToolHandlers.remove_row` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 561 | `McpToolHandlers.remove_row.<locals>.perform` | nested in `remove_row`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 562 | `McpToolHandlers.remove_row.<locals>.perform.<locals>.work` | nested in `remove_row`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the ... | #76 | `gtk-6i.md:80` |
| 573 | `McpToolHandlers.reset_result` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 576 | `McpToolHandlers.reset_result.<locals>.perform` | nested in `reset_result`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 577 | `McpToolHandlers.reset_result.<locals>.perform.<locals>.work` | nested in `reset_result`: 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into ... | #76 | `gtk-6i.md:80` |
| 596 | `_queue_row` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 608 | `_entry_detail` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 635 | `_matches_filter` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |
| 664 | `_guess_mime` | 5.e, in `hatate-core` (`mcp/tool_handlers.rs`), unchanged here. What the window adds is the part Python does inside the Qt slot: `crates/hatate-gtk/src/mcp_bridge.rs:143` turns the queue's events into the table, selection and ... | #76 | `gtk-6i.md:80` |

### `gui/message.py` (6 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 31 | `build` | **port**: `dialogs::AdwPrompter (no native-dialog workaround is needed)` | #57 (+#60, #61, #62) | `gtk-shell.md:363` |
| 41 | `_show` | **port**: `dialogs::Prompter::message` | #57 (+#60, #61, #62) | `gtk-shell.md:364` |
| 72 | `information` | **port**: `dialogs::information` | #57 (+#60, #61, #62) | `gtk-shell.md:365` |
| 76 | `warning` | **port**: `dialogs::warning` | #57 (+#60, #61, #62) | `gtk-shell.md:366` |
| 80 | `critical` | **port**: `dialogs::critical` | #57 (+#60, #61, #62) | `gtk-shell.md:367` |
| 84 | `question` | **port**: `dialogs::question` | #57 (+#60, #61, #62) | `gtk-shell.md:368` |

### `gui/parser_health_dialog.py` (8 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 46 | `_ProbeWorker.__init__` | `ProbeRunner` (the seam the live check will use) (left to: **left to Phase 7 per DAN-2290**: no probe ships) | #63 | `gtk-6e.md:272` |
| 50 | `_ProbeWorker.run` | `ProbeRunner::run` (left to: **left to Phase 7 per DAN-2290**) | #63 | `gtk-6e.md:273` |
| 66 | `ParserHealthDialog.__init__` | `ProbeRunner` (the seam the live check will use) (left to: **left to Phase 7 per DAN-2290**: no probe ships) | #63 | `gtk-6e.md:272` |
| 120 | `ParserHealthDialog._populate` | `hatate-gtk/src/parser_health.rs:316` `populate` | #63 | `gtk-6e.md:275` |
| 155 | `ParserHealthDialog._start_probe` | `hatate-gtk/src/parser_health.rs:383` (the check button; disabled in this build, DAN-2290) | #63 | `gtk-6e.md:276` |
| 168 | `ParserHealthDialog._on_progress` | `hatate-gtk/src/parser_health.rs:417` `on_progress` | #63 | `gtk-6e.md:277` |
| 174 | `ParserHealthDialog._on_done` | `hatate-gtk/src/parser_health.rs:428` `on_done` | #63 | `gtk-6e.md:278` |
| 198 | `ParserHealthDialog.closeEvent` | `hatate-gtk/src/parser_health.rs:383` `start_probe`: the worker's callback holds a weak handle, so a result that arrives after the dialog is gone is dropped | #63 | `gtk-6e.md:279` |

### `gui/pawchive_index_dialog.py` (17 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 29 | `_hairline` | the `field` CSS class on the table and the results (`pawchive_dialog.rs:225`, `:226`); no function | #74 | `gtk-6g.md:262` |
| 41 | `PawchiveIndexDialog.__init__` | `pawchive_dialog::PawchiveIndexDialog::build` | #74 | `gtk-6g.md:263` |
| 144 | `PawchiveIndexDialog._populate` | `populate` (+ `row_cells`, `window_title`) | #74 | `gtk-6g.md:264` |
| 165 | `PawchiveIndexDialog._selected_creators` | `selected_creators`, `select_creators` | #74 | `gtk-6g.md:265` |
| 169 | `PawchiveIndexDialog._select_creators` | `selected_creators`, `select_creators` | #74 | `gtk-6g.md:265` |
| 184 | `PawchiveIndexDialog._find_or_add` | `find_or_add`, `on_found` (+ `found_label`) | #74 | `gtk-6g.md:266` |
| 206 | `PawchiveIndexDialog._on_found` | `find_or_add`, `on_found` (+ `found_label`) | #74 | `gtk-6g.md:266` |
| 228 | `PawchiveIndexDialog._add_checked` | `add_checked`, `add_from_matches` | #74 | `gtk-6g.md:267` |
| 243 | `PawchiveIndexDialog._add_from_matches` | `add_checked`, `add_from_matches` | #74 | `gtk-6g.md:267` |
| 260 | `PawchiveIndexDialog._remove_selected` | `remove_selected` (Cancel by default) | #74 | `gtk-6g.md:268` |
| 275 | `PawchiveIndexDialog._start_indexing` | `start_indexing`, `set_busy` | #74 | `gtk-6g.md:269` |
| 292 | `PawchiveIndexDialog._set_busy` | `start_indexing`, `set_busy` | #74 | `gtk-6g.md:269` |
| 301 | `PawchiveIndexDialog._on_progress` | `on_progress`, `on_creator_done`, `on_finished` (+ `summary_line`) | #74 | `gtk-6g.md:270` |
| 310 | `PawchiveIndexDialog._on_creator_done` | `on_progress`, `on_creator_done`, `on_finished` (+ `summary_line`) | #74 | `gtk-6g.md:270` |
| 323 | `PawchiveIndexDialog._on_finished` | `on_progress`, `on_creator_done`, `on_finished` (+ `summary_line`) | #74 | `gtk-6g.md:270` |
| 327 | `PawchiveIndexDialog._stop` | `stop`, `on_close` | #74 | `gtk-6g.md:271` |
| 333 | `PawchiveIndexDialog.closeEvent` | `stop`, `on_close` | #74 | `gtk-6g.md:271` |

### `gui/preview_text.py` (14 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 43 | `human_size` | `crates/hatate-core/src/preview_text.rs:49` | #71 | `gtk-6f.md:327` |
| 54 | `matched_caption` | `crates/hatate-core/src/preview_text.rs:70` | #71 | `gtk-6f.md:328` |
| 74 | `no_candidate_text` | `crates/hatate-core/src/preview_text.rs:90` | #71 | `gtk-6f.md:329` |
| 89 | `_dimensions` | `crates/hatate-core/src/boorus/eshuushuu.rs:557` | #71 | `gtk-6f.md:330` |
| 93 | `local_image_info_text` | `crates/hatate-core/src/preview_text.rs:129` | #71 | `gtk-6f.md:331` |
| 104 | `local_frame_meta` | `crates/hatate-core/src/preview_text.rs:140` | #71 | `gtk-6f.md:332` |
| 111 | `_local_info_parts` | `crates/hatate-core/src/preview_text.rs:111` | #71 | `gtk-6f.md:333` |
| 129 | `matched_frame_label` | `crates/hatate-core/src/preview_text.rs:129` | #71 | `gtk-6f.md:331` |
| 143 | `matched_frame_meta` | `crates/hatate-core/src/preview_text.rs:181` | #71 | `gtk-6f.md:335` |
| 160 | `matched_image_info_text` | `crates/hatate-core/src/preview_text.rs:190` | #71 | `gtk-6f.md:336` |
| 193 | `comparison_banner_text` | `crates/hatate-core/src/preview_text.rs:224` | #71 | `gtk-6f.md:337` |
| 233 | `comparison_banner_tier` | `crates/hatate-core/src/preview_text.rs:254` | #71 | `gtk-6f.md:338` |
| 249 | `comparison_banner_colour` | `preview_text.rs` `comparison_banner_tier` (the colour is the CSS class of the tier) | #71 | `gtk-6f.md:339` |
| 271 | `comparison_readout` | `crates/hatate-core/src/preview_text.rs:296` | #71 | `gtk-6f.md:340` |

### `gui/review_shortcuts.py` (11 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 36 | `_selected_rows` | `crates/hatate-gtk/src/review_ops.rs:1129` | #71 | `gtk-6f.md:297` |
| 40 | `_selected_entries` | `review_ops.rs` `selected_rows` | #71 | `gtk-6f.md:298` |
| 51 | `_move_selection` | `crates/hatate-gtk/src/review_ops.rs:1108` | #71 | `gtk-6f.md:299` |
| 75 | `_move_to_unreviewed` | `crates/hatate-gtk/src/review_ops.rs:1142` | #71 | `gtk-6f.md:300` |
| 99 | `_toggle_reviewed` | `crates/hatate-core/src/mcp/handlers.rs:39` | #71 | `gtk-6f.md:301` |
| 110 | `_step_candidate` | `crates/hatate-gtk/src/review_ops.rs:1216` | #71 | `gtk-6f.md:302` |
| 126 | `handlers` | `review_ops.rs` `run_review_action` (a `match` on the action id) | #71 | `gtk-6f.md:303` |
| 154 | `_guarded` | `crates/hatate-core/src/boorus/fetch.rs:193` | #71 | `gtk-6f.md:304` |
| 168 | `_guarded.<locals>.run` | `crates/hatate-cli/src/batch_cmd.rs:107` | #71 | `gtk-6f.md:305` |
| 185 | `install` | `crates/hatate-gtk/src/menubar_keys.rs:49` | #71 | `gtk-6f.md:306` |
| 232 | `open_matched_url` | `review_ops.rs` `action_open_matched_url` | #71 | `gtk-6f.md:307` |

### `gui/review_view.py` (29 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 93 | `ReviewViewMixin._build_review_page` | `crates/hatate-gtk/src/review_ops.rs:229` | #71 | `gtk-6f.md:263` |
| 136 | `ReviewViewMixin._build_comparison_card` | `review_view.rs` `ReviewPage::new` (`compare_col`) | #71 | `gtk-6f.md:264` |
| 183 | `ReviewViewMixin._build_comparison_readout` | `review_view.rs` `ReviewPage::new` (`readout_root`) | #71 | `gtk-6f.md:265` |
| 212 | `ReviewViewMixin.set_comparison_readout` | `review_view.rs` `ReviewPage::set_readout`, `fit_readout` | #71 | `gtk-6f.md:266` |
| 232 | `ReviewViewMixin._build_zoom_tools` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`zoom_tools`, `zoom_label`: 383) | #72 | `gtk-6f2.md:168` |
| 263 | `ReviewViewMixin._build_wipe_slider_row` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`wipe_slider`, `slider_row`) | #72 | `gtk-6f2.md:169` |
| 292 | `ReviewViewMixin._build_pair_view` | `review_view.rs` `ReviewPage::new` (the two frames; Side by side only) | #71 | `gtk-6f.md:269` |
| 346 | `ReviewViewMixin._set_review_captions` | `crates/hatate-gtk/src/review_ops.rs:447` | #71 | `gtk-6f.md:270` |
| 368 | `ReviewViewMixin._build_wipe_view` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`wipe_host`, `wipe`), `crates/hatate-gtk/src/wipe_view.rs` `WipeView` | #72 | `gtk-6f2.md:170` |
| 394 | `ReviewViewMixin._build_decision_card` | `review_view.rs` `ReviewPage::new` (`review-rail`), `Keyed` | #71 | `gtk-6f.md:272` |
| 538 | `ReviewViewMixin._build_position_strip` | `review_view.rs` `ReviewPage::new` (`hint_slot`) and `review_ops.rs` `refresh_review_strip` | #71 | `gtk-6f.md:273` |
| 574 | `ReviewViewMixin._run_review_action` | `crates/hatate-gtk/src/review_ops.rs:1197` | #71 | `gtk-6f.md:274` |
| 579 | `ReviewViewMixin._resolved_shortcuts` | `crates/hatate-gtk/src/review_ops.rs:218` | #71 | `gtk-6f.md:275` |
| 583 | `ReviewViewMixin._key_for` | `crates/hatate-gtk/src/review_ops.rs:224` | #71 | `gtk-6f.md:276` |
| 587 | `ReviewViewMixin._hint_bar_groups` | `crates/hatate-gtk/src/review_ops.rs:1056` | #71 | `gtk-6f.md:277` |
| 619 | `ReviewViewMixin._show_shortcuts_dialog` | **OPEN**: not ported. The Review header's Shortcuts button is built and insensitive (`crates/hatate-gtk/src/review_ops.rs:318`, tooltip `The shortcuts list arrives in a later step.`). `docs/gtk-6f.md` section 2 flagged it to ... | OPEN (DAN-2448) | this table (6.j) |
| 667 | `ReviewViewMixin._show_comparison_controls` | `crates/hatate-gtk/src/review_compare.rs:182` `show_comparison_controls` | #72 | `gtk-6f2.md:171` |
| 675 | `ReviewViewMixin._current_review_key` | `review_view.rs` `ReviewPage::current_view` | #71 | `gtk-6f.md:280` |
| 682 | `ReviewViewMixin._on_review_view_changed` | `crates/hatate-gtk/src/review_compare.rs:198` `on_review_view_changed` | #72 | `gtk-6f2.md:172` |
| 697 | `ReviewViewMixin._ensure_comparison_loaded` | `crates/hatate-gtk/src/review_compare.rs:215` `ensure_comparison_loaded`, `on_comparison_loaded`, `show_error_state` (the error state is new, design gate 2.8) | #72 | `gtk-6f2.md:173` |
| 751 | `ReviewViewMixin.full_resolution_match` | `crates/hatate-gtk/src/review_compare.rs:493` `full_resolution_match` | #72 | `gtk-6f2.md:174` |
| 761 | `ReviewViewMixin._apply_wipe_mode` | `crates/hatate-gtk/src/review_compare.rs:354` `apply_wipe_mode` | #72 | `gtk-6f2.md:175` |
| 771 | `ReviewViewMixin._show_differences` | `crates/hatate-gtk/src/review_compare.rs:373` `show_differences`, `on_differences_built`, `present_differences` | #72 | `gtk-6f2.md:176` |
| 804 | `ReviewViewMixin._step_review_candidate` | `crates/hatate-gtk/src/review_ops.rs:554` | #71 | `gtk-6f.md:286` |
| 810 | `ReviewViewMixin._toggle_reviewed_from_review` | `crates/hatate-gtk/src/review_ops.rs:1191` | #71 | `gtk-6f.md:287` |
| 816 | `ReviewViewMixin._pop_out_comparison` | `crates/hatate-gtk/src/review_ops.rs` `wire_review_page` (`pop_out_btn` runs the `compare` review key) -> `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` | #72 | `gtk-6f2.md:177` |
| 819 | `ReviewViewMixin.refresh_review` | `crates/hatate-gtk/src/review_ops.rs:978` | #71 | `gtk-6f.md:289` |
| 860 | `ReviewViewMixin._refresh_review_strip` | `crates/hatate-gtk/src/review_ops.rs:1013` | #71 | `gtk-6f.md:290` |
| 878 | `ReviewViewMixin._refresh_review_keycaps` | `crates/hatate-gtk/src/review_ops.rs:1036` | #71 | `gtk-6f.md:291` |

### `gui/session_autosave.py` (6 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 37 | `SessionAutosaver.__init__` | **port**: `session_ops (autosave_timer, autosave_worker)` | #57 (+#60, #61, #62) | `gtk-shell.md:389` |
| 54 | `SessionAutosaver.mark_saved` | n/a: no revision bookkeeping (D1, DAN-2227) | #57 (+#60, #61, #62) | `gtk-shell.md:391` |
| 60 | `SessionAutosaver.apply_settings` | **port**: `session_ops::apply_autosave_settings` | #57 (+#60, #61, #62) | `gtk-shell.md:392` |
| 72 | `SessionAutosaver.save_now` | **port**: `session_ops::autosave_now` | #57 (+#60, #61, #62) | `gtk-shell.md:393` |
| 95 | `SessionAutosaver.shutdown` | **port**: `session_ops::close_teardown` | #57 (+#60, #61, #62) | `gtk-shell.md:394` |
| 106 | `SessionAutosaver._on_done` | **port**: `session_ops::autosave_now (completion callback)` | #57 (+#60, #61, #62) | `gtk-shell.md:395` |

### `gui/settings_dialog.py` (55 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 47 | `_nonblank_lines` | `settings_values.rs` `nonblank_lines` | #75 | `gtk-6h.md:54` |
| 52 | `_read_mcp_audit_entries` | `crates/hatate-gtk/src/mcp_audit_view.rs:108` | #76 | `gtk-6i.md:52` |
| 77 | `_mcp_time_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:190` | #76 | `gtk-6i.md:53` |
| 91 | `_mcp_target_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:204` | #76 | `gtk-6i.md:54` |
| 101 | `_mcp_outcome_weight` | `crates/hatate-gtk/src/mcp_audit_view.rs:215` | #76 | `gtk-6i.md:55` |
| 125 | `_mcp_outcome_cell` | `crates/hatate-gtk/src/mcp_audit_view.rs:246` | #76 | `gtk-6i.md:56` |
| 166 | `_mcp_outcome_tooltip` | `crates/hatate-gtk/src/mcp_audit_view.rs:329` | #76 | `gtk-6i.md:57` |
| 178 | `SettingsDialog.__init__` | `crates/hatate-gtk/src/settings_values.rs:297` | #76 | `gtk-6i.md:75` |
| 236 | `SettingsDialog._scrollable` | `settings_dialog.rs` `scrollable` | #75 | `gtk-6h.md:56` |
| 249 | `SettingsDialog._build_search_row` | `build`, `on_search`, `jump_to_setting` | #75 | `gtk-6h.md:57` |
| 280 | `SettingsDialog._on_search` | `build`, `on_search`, `jump_to_setting` | #75 | `gtk-6h.md:57` |
| 297 | `SettingsDialog._jump_to_setting` | `crates/hatate-gtk/src/settings_values.rs:297` | #76 | `gtk-6i.md:75` |
| 309 | `SettingsDialog._build_general_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 565 | `SettingsDialog._browse_log_path` | `wire_controls`, button `button.browse` | #75 | `gtk-6h.md:59` |
| 571 | `SettingsDialog._build_engine_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 939 | `SettingsDialog._warn_if_lens_cannot_run` | `warn_if_lens_cannot_run` | #75 | `gtk-6h.md:60` |
| 978 | `SettingsDialog._build_import_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1088 | `SettingsDialog._build_site_logins_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1294 | `SettingsDialog._test_deviantart` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1384 | `SettingsDialog._test_animepictures` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1478 | `SettingsDialog._test_sankaku` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1528 | `SettingsDialog._build_saucenao_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1593 | `SettingsDialog._build_hydrus_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1685 | `SettingsDialog._test_e621` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1763 | `SettingsDialog._test_pixiv` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1845 | `SettingsDialog._on_service_selected` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1853 | `SettingsDialog._fetch_tag_services` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1882 | `SettingsDialog._test_hydrus` | **OPEN**: not ported. The button is built and insensitive with Python's tooltip (`crates/hatate-gtk/src/settings_dialog.rs:410`, `insensitive_network_buttons`); the call is network code and needs a transport differential over ... | OPEN (DAN-2448) | this table (6.j) |
| 1900 | `SettingsDialog._build_tag_namespaces_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 2003 | `SettingsDialog._build_outcome_tags_group` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 2075 | `SettingsDialog._build_shortcuts_tab` | `crates/hatate-gtk/src/shortcuts_page.rs:300` | #76 | `gtk-6i.md:58` |
| 2175 | `SettingsDialog._current_shortcut_bindings` | `crates/hatate-gtk/src/shortcuts_page.rs:412` | #76 | `gtk-6i.md:59` |
| 2181 | `SettingsDialog._reset_shortcuts_to_defaults` | `crates/hatate-gtk/src/shortcuts_page.rs:426` | #76 | `gtk-6i.md:60` |
| 2188 | `SettingsDialog._refresh_shortcut_conflicts` | `crates/hatate-gtk/src/shortcuts_page.rs:436` | #76 | `gtk-6i.md:61` |
| 2212 | `SettingsDialog._build_mcp_tab` | `crates/hatate-gtk/src/settings_mcp.rs:53` (the page as data) and `crates/hatate-gtk/src/settings_dialog.rs:442` (its handlers) | #76 | `gtk-6i.md:62` |
| 2311 | `SettingsDialog._build_mcp_connection_card` | `crates/hatate-gtk/src/settings_mcp.rs:53` and `crates/hatate-gtk/src/mcp_audit_view.rs:450` (the six states) | #76 | `gtk-6i.md:63` |
| 2331 | `SettingsDialog._build_mcp_always_available_group` | `crates/hatate-gtk/src/settings_mcp.rs:53` | #76 | `gtk-6i.md:64` |
| 2346 | `SettingsDialog._build_mcp_tier_checkbox_group` | `crates/hatate-gtk/src/settings_mcp.rs:53` | #76 | `gtk-6i.md:64` |
| 2386 | `SettingsDialog._copy_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs:548` | #76 | `gtk-6i.md:65` |
| 2391 | `SettingsDialog._generate_mcp_token` | `crates/hatate-gtk/src/settings_dialog.rs:556` | #76 | `gtk-6i.md:66` |
| 2398 | `SettingsDialog._refresh_mcp_dry_run_hint` | `crates/hatate-gtk/src/settings_dialog.rs:572`, `crates/hatate-gtk/src/settings_mcp.rs:241` | #76 | `gtk-6i.md:67` |
| 2412 | `SettingsDialog._on_mcp_enabled_toggled` | `crates/hatate-gtk/src/settings_dialog.rs:583`, `crates/hatate-gtk/src/settings_mcp.rs:250`, the toggle handler in `crates/hatate-gtk/src/settings_dialog.rs:442` | #76 | `gtk-6i.md:68` |
| 2417 | `SettingsDialog._show_mcp_enable_summary` | `crates/hatate-gtk/src/settings_dialog.rs:583`, `crates/hatate-gtk/src/settings_mcp.rs:250`, the toggle handler in `crates/hatate-gtk/src/settings_dialog.rs:442` | #76 | `gtk-6i.md:68` |
| 2454 | `SettingsDialog._mcp_server_handle` | `crates/hatate-gtk/src/mcp_bridge.rs:317` | #76 | `gtk-6i.md:69` |
| 2457 | `SettingsDialog._mcp_restart_server` | `crates/hatate-gtk/src/settings_dialog.rs:600` | #76 | `gtk-6i.md:70` |
| 2474 | `SettingsDialog._refresh_mcp_connection` | `crates/hatate-gtk/src/settings_dialog.rs:611` | #76 | `gtk-6i.md:71` |
| 2536 | `SettingsDialog._build_mcp_audit_card` | `crates/hatate-gtk/src/settings_mcp.rs:314` | #76 | `gtk-6i.md:72` |
| 2594 | `SettingsDialog._on_mcp_audit_filter_changed` | `crates/hatate-gtk/src/settings_mcp.rs:314` | #76 | `gtk-6i.md:72` |
| 2597 | `SettingsDialog._on_mcp_audit_follow_toggled` | `crates/hatate-gtk/src/settings_mcp.rs:489` | #76 | `gtk-6i.md:73` |
| 2605 | `SettingsDialog._open_mcp_audit_folder` | `crates/hatate-gtk/src/settings_mcp.rs:314` | #76 | `gtk-6i.md:72` |
| 2608 | `SettingsDialog._on_tabs_current_changed` | `crates/hatate-gtk/src/settings_mcp.rs:482` (the tab came to the front or left it: the MCP page's refresh and timer) | #76 | this table (6.j) |
| 2620 | `SettingsDialog._refresh_mcp_audit_table` | `crates/hatate-gtk/src/settings_mcp.rs:430` | #76 | `gtk-6i.md:74` |
| 2650 | `SettingsDialog.accept` | `accept`, `settings_values::apply`, `MainWindow::apply_settings_dialog` | #75 | `gtk-6h.md:61` |
| 2666 | `SettingsDialog.apply_to_settings` | `crates/hatate-gtk/src/settings_values.rs:297` | #76 | `gtk-6i.md:75` |
| 2827 | `SettingsDialog._update_engine_pipeline` | `update_pipeline`, `settings_values::pipeline_text` | #75 | `gtk-6h.md:62` |

### `gui/settings_search.py` (5 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 38 | `index_tabs` | `hatate-core` `settings_search::{index_tabs, name_of, matches}` | #75 | `gtk-6h.md:66` |
| 60 | `_name_of` | `hatate-core` `settings_search::{index_tabs, name_of, matches}` | #75 | `gtk-6h.md:66` |
| 75 | `matches` | `hatate-core` `settings_search::{index_tabs, name_of, matches}` | #75 | `gtk-6h.md:66` |
| 98 | `highlight` | `crates/hatate-gtk/src/settings_dialog.rs:814` `SettingsDialog::highlight` | #75 | `gtk-6h.md:67` |
| 128 | `highlight.<locals>._revert` | nested in `highlight`: left to 6.h | #58 | `phase6b-pure-core.md:113` |

### `gui/shell.py` (18 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 65 | `split_figures` | **port**: `shell::split_figures` | #57 (+#60, #61, #62) | `gtk-shell.md:317` |
| 92 | `Readout.__init__` | **port**: `shell::Readout` | #57 (+#60, #61, #62) | `gtk-shell.md:318` |
| 102 | `Readout.text` | method of `Readout`; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:320` |
| 105 | `Readout.setText` | method of `Readout`; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:321` |
| 146 | `_ModeStack.minimumSizeHint` | method of `_ModeStack`; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:323` |
| 152 | `_ModeStack.sizeHint` | method of `_ModeStack`; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:324` |
| 158 | `_ModeStack.setCurrentWidget` | method of `_ModeStack`; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:325` |
| 193 | `ShellMixin._build_shell` | **port**: `shell::Shell::new` | #57 (+#60, #61, #62) | `gtk-shell.md:327` |
| 216 | `ShellMixin._build_top_bar` | **port**: `shell::Shell::new` | #57 (+#60, #61, #62) | `gtk-shell.md:328` |
| 244 | `ShellMixin._build_app_mark` | **port**: `shell::Shell::new` | #57 (+#60, #61, #62) | `gtk-shell.md:329` |
| 268 | `ShellMixin._build_run_strip` | **port**: `shell::Shell::new` | #57 (+#60, #61, #62) | `gtk-shell.md:330` |
| 360 | `ShellMixin._placeholder_page` | **port**: `shell::placeholder_page` | #57 (+#60, #61, #62) | `gtk-shell.md:331` |
| 371 | `ShellMixin._build_status_bar` | **port**: `shell::Shell::new` | #57 (+#60, #61, #62) | `gtk-shell.md:332` |
| 397 | `ShellMixin.set_mode` | **port**: `window::MainWindow::set_mode` | #57 (+#60, #61, #62) | `gtk-shell.md:333` |
| 416 | `ShellMixin.ghost_glyph_for` | **port**: `shell::ghost_glyph` | #57 (+#60, #61, #62) | `gtk-shell.md:334` |
| 421 | `ShellMixin.set_ghost` | **port**: `widgets::GhostKanji::set_glyph` | #57 (+#60, #61, #62) | `gtk-shell.md:335` |
| 427 | `ShellMixin.current_mode` | **port**: `shell::Shell::current_mode` | #57 (+#60, #61, #62) | `gtk-shell.md:336` |
| 430 | `ShellMixin._on_mode_changed` | **port**: `window::MainWindow::on_mode_changed` | #57 (+#60, #61, #62) | `gtk-shell.md:337` |

### `gui/table_context_menu.py` (12 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 36 | `count_label` | `table_menu::Item`, `count_label` | #67 | `queue-view.md:162` |
| 107 | `review_state_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 119 | `selection_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 147 | `cache_state_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 159 | `sent_state_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 171 | `_all_reviewed` | `table_menu::all_reviewed` | #67 | `queue-view.md:164` |
| 181 | `_review_mark_label` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 189 | `row_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 349 | `engine_items` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 376 | `_add` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 398 | `build` | `table_menu::build` and `RowLines` | #67 | `queue-view.md:163` |
| 429 | `show` | `hatate-gtk::queue_menu` | #67 | `queue-view.md:165` |

### `gui/table_delegates.py` (8 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 56 | `paint_row_rule` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 69 | `RuledRowDelegate.__init__` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 76 | `RuledRowDelegate.paint` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 84 | `ChipDelegate.paint` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 188 | `ChipDelegate.sizeHint` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 212 | `TagRowDelegate.__init__` | `crates/hatate-gtk/src/review_ops.rs:162` the tag row's namespace colour (`TagRowDelegate` is the tag rail's cell painter; GTK builds the row as widgets) | #71 | this table (6.j) |
| 216 | `TagRowDelegate.sizeHint` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |
| 219 | `TagRowDelegate.paint` | GTK cell widgets and CSS in `queue_view.rs` and `theme.rs`; no painter. Judged by shots | #67 | `queue-view.md:157` |

### `gui/theme.py` (9 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 236 | `chip_label` | the methods above (an unknown key has no Rust counterpart: the enums are closed) | #59 | `phase6a-core-support.md:108` |
| 240 | `status_glyph` | the methods above (an unknown key has no Rust counterpart: the enums are closed) | #59 | `phase6a-core-support.md:108` |
| 252 | `status_weight` | the methods above (an unknown key has no Rust counterpart: the enums are closed) | #59 | `phase6a-core-support.md:108` |
| 259 | `status_glyph_font` | the methods above (an unknown key has no Rust counterpart: the enums are closed) | #59 | `phase6a-core-support.md:108` |
| 267 | `mono_font` | **port**: `theme::css (mono family)` | #57 (+#60, #61, #62) | `gtk-shell.md:347` |
| 282 | `ink_color` | **port**: `tokens (named colours)` | #57 (+#60, #61, #62) | `gtk-shell.md:348` |
| 298 | `palette` | **port**: `tokens::Palette` | #57 (+#60, #61, #62) | `gtk-shell.md:349` |
| 303 | `resolve_mode` | **port**: `theme::resolve_scheme` | #57 (+#60, #61, #62) | `gtk-shell.md:350` |
| 328 | `stylesheet` | **port**: `theme::css (generated from tokens.css; GTK CSS)` | #57 (+#60, #61, #62) | `gtk-shell.md:351` |

### `gui/widgets.py` (98 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 70 | `card` | **port**: `widgets::card` | #57 (+#60, #61, #62) | `gtk-shell.md:412` |
| 98 | `section_label` | **port**: `widgets::section_label` | #57 (+#60, #61, #62) | `gtk-shell.md:413` |
| 116 | `section_header` | **port**: `widgets::section_header` | #57 (+#60, #61, #62) | `gtk-shell.md:414` |
| 139 | `FlowLayout.__init__` | `gtk::FlowBox` (`crates/hatate-gtk/src/activity.rs:160` for the engine chips); a `FlowLayout` class is not needed because the toolkit widget wraps | #57 (+#60, #61, #62) | this table (6.j) |
| 145 | `FlowLayout.addItem` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:417` |
| 149 | `FlowLayout.count` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:418` |
| 152 | `FlowLayout.itemAt` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:419` |
| 155 | `FlowLayout.takeAt` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:420` |
| 161 | `FlowLayout.hasHeightForWidth` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:421` |
| 164 | `FlowLayout.heightForWidth` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:422` |
| 167 | `FlowLayout.setGeometry` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:423` |
| 171 | `FlowLayout.sizeHint` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:424` |
| 174 | `FlowLayout.minimumSize` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:425` |
| 182 | `FlowLayout._arrange` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:426` |
| 211 | `WrappingLabel.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 215 | `WrappingLabel.resizeEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:429` |
| 221 | `WrappingLabel.setText` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:430` |
| 238 | `LeadedLabel.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 242 | `LeadedLabel._document` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:433` |
| 257 | `LeadedLabel.heightForWidth` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:422` |
| 262 | `LeadedLabel.paintEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:435` |
| 276 | `_banner_shell` | **port**: `widgets::banner_shell` | #57 (+#60, #61, #62) | `gtk-shell.md:436` |
| 331 | `run_banner` | **port**: `widgets::run_banner` | #57 (+#60, #61, #62) | `gtk-shell.md:437` |
| 365 | `quota_pause_banner` | **port**: `widgets::quota_pause_banner` | #57 (+#60, #61, #62) | `gtk-shell.md:438` |
| 388 | `meta_line` | **port**: `widgets::meta_line` | #57 (+#60, #61, #62) | `gtk-shell.md:439` |
| 427 | `empty_prose` | **port**: `widgets::empty_prose` | #57 (+#60, #61, #62) | `gtk-shell.md:440` |
| 446 | `nothing_survived` | **port**: `widgets::nothing_survived` | #57 (+#60, #61, #62) | `gtk-shell.md:441` |
| 517 | `restyle` | **port**: `theme::install (the provider swap restyles everything)` | #57 (+#60, #61, #62) | `gtk-shell.md:442` |
| 534 | `heading` | **port**: `widgets::heading` | #57 (+#60, #61, #62) | `gtk-shell.md:443` |
| 540 | `screen_title` | **port**: `widgets::screen_title` | #57 (+#60, #61, #62) | `gtk-shell.md:444` |
| 549 | `display_title` | **port**: `widgets::display_title` | #57 (+#60, #61, #62) | `gtk-shell.md:445` |
| 558 | `muted` | **port**: `widgets::muted` | #57 (+#60, #61, #62) | `gtk-shell.md:446` |
| 564 | `hint` | **port**: `widgets::hint` | #57 (+#60, #61, #62) | `gtk-shell.md:447` |
| 571 | `uppercase_voice` | **port**: `theme::css (caps / mono classes)` | #57 (+#60, #61, #62) | `gtk-shell.md:448` |
| 585 | `pill_button` | **port**: `widgets::pill_button` | #57 (+#60, #61, #62) | `gtk-shell.md:449` |
| 615 | `KeyCap.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 642 | `KeyedButton.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 654 | `KeyedButton.keycap` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:454` |
| 657 | `KeyedButton.key` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:455` |
| 660 | `KeyedButton.set_key` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:456` |
| 666 | `KeyedButton.refit` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:457` |
| 680 | `KeyedButton._place` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:458` |
| 685 | `KeyedButton.resizeEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:429` |
| 689 | `KeyedButton.showEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:460` |
| 694 | `keyed_button` | **port**: `widgets::keyed_button` | #57 (+#60, #61, #62) | `gtk-shell.md:461` |
| 721 | `KeyHintBar.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 732 | `KeyHintBar.set_groups` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:464` |
| 768 | `KeyHintBar._text_label` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:465` |
| 775 | `KeyHintBar.plain_text` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:466` |
| 780 | `KeyHintBar.visible_group_count` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:467` |
| 783 | `KeyHintBar.minimumSizeHint` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:468` |
| 786 | `KeyHintBar.resizeEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:429` |
| 790 | `KeyHintBar.showEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:460` |
| 794 | `KeyHintBar._fit` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:471` |
| 818 | `link_button` | **port**: `widgets::link_button` | #57 (+#60, #61, #62) | `gtk-shell.md:472` |
| 838 | `icon_button` | **port**: `widgets::icon_button` | #57 (+#60, #61, #62) | `gtk-shell.md:473` |
| 866 | `zoom_button` | `crates/hatate-gtk/src/review_view.rs:386` the `zoom_tools` box, buttons `−`, `+`, `Fit`, `100%` (`review_view.rs:396`); the same four in `compare_dialog.rs:167` | #71 | this table (6.j) |
| 879 | `apply_tracking` | `crates/hatate-gtk/src/theme.rs:152` CSS `letter-spacing: %tracking_label%px` (`theme.rs:75`); GTK CSS has the property Qt's QSS lacks, so no function is needed | #57 (+#60, #61, #62) | this table (6.j) |
| 892 | `segmented` | **port**: `widgets::segmented` | #57 (+#60, #61, #62) | `gtk-shell.md:476` |
| 944 | `segmented.<locals>._changed` | nested in `segmented`: **port**: `widgets::segmented` | #57 (+#60, #61, #62) | `gtk-shell.md:476` |
| 957 | `disclosure` | **port**: `widgets::disclosure` | #57 (+#60, #61, #62) | `gtk-shell.md:477` |
| 969 | `disclosure.<locals>._retext` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:478` |
| 980 | `fold` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:479` |
| 987 | `chip` | **port**: `widgets::chip` | #57 (+#60, #61, #62) | `gtk-shell.md:480` |
| 1000 | `roomy` | `crates/hatate-gtk/src/theme.rs:211` `entry.field { padding: 7px 10px }`; no minimum-height call is needed | #57 (+#60, #61, #62) | this table (6.j) |
| 1049 | `GhostKanji.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1057 | `GhostKanji.glyph` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:484` |
| 1060 | `GhostKanji.set_glyph` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:485` |
| 1065 | `GhostKanji._font` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:486` |
| 1071 | `GhostKanji.ghost_rect` | widgets::GhostKanji | #57 (+#60, #61, #62) | `gtk-shell.md:487` |
| 1081 | `GhostKanji._pixmap` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:488` |
| 1105 | `GhostKanji.paintEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:435` |
| 1113 | `see_through` | widgets::GhostKanji (the ghost is drawn behind the page) | #57 (+#60, #61, #62) | `gtk-shell.md:490` |
| 1155 | `Brackets.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1165 | `Brackets.corner_rects` | widgets::brackets | #57 (+#60, #61, #62) | `gtk-shell.md:493` |
| 1177 | `Brackets._fit` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:471` |
| 1186 | `Brackets.eventFilter` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:495` |
| 1192 | `Brackets.paintEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:435` |
| 1199 | `image_frame` | **port**: `widgets::image_frame` | #57 (+#60, #61, #62) | `gtk-shell.md:497` |
| 1217 | `_ElidedLabel.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1221 | `_ElidedLabel.minimumSizeHint` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:468` |
| 1224 | `_ElidedLabel.paintEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:435` |
| 1238 | `FrameCaption.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1254 | `apply_theme` | **port**: `theme::install` | #57 (+#60, #61, #62) | `gtk-shell.md:504` |
| 1270 | `LinkLabel.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1274 | `LinkLabel.mousePressEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:507` |
| 1285 | `WideComboBox.showPopup` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:509` |
| 1304 | `CandidatePicker.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1309 | `CandidatePicker.elided_text` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:512` |
| 1318 | `CandidatePicker._option` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:513` |
| 1323 | `CandidatePicker.paintEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:435` |
| 1353 | `ScaledImageLabel.__init__` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:416` |
| 1357 | `ScaledImageLabel.setPixmap` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:517` |
| 1361 | `ScaledImageLabel.source_pixmap` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:518` |
| 1364 | `ScaledImageLabel.resizeEvent` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:429` |
| 1371 | `ScaledImageLabel.sizeHint` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:424` |
| 1374 | `ScaledImageLabel.minimumSizeHint` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:468` |
| 1377 | `ScaledImageLabel._render` | method of the class above; follows it | #57 (+#60, #61, #62) | `gtk-shell.md:522` |

### `gui/worker_lifecycle.py` (9 defs)

| Line | Python def | Rust home, or the owner | PR | Source row |
|---|---|---|---|---|
| 38 | `WorkerRegistry.__init__` | **port**: `worker::Workers` | #57 (+#60, #61, #62) | `gtk-shell.md:374` |
| 41 | `WorkerRegistry.__len__` | n/a: no parked-thread list (tokio tasks are not QThreads) | #57 (+#60, #61, #62) | `gtk-shell.md:376` |
| 44 | `WorkerRegistry.__contains__` | n/a: same | #57 (+#60, #61, #62) | `gtk-shell.md:377` |
| 48 | `WorkerRegistry.retiring` | n/a: same | #57 (+#60, #61, #62) | `gtk-shell.md:378` |
| 52 | `WorkerRegistry.prune` | n/a: same | #57 (+#60, #61, #62) | `gtk-shell.md:379` |
| 64 | `WorkerRegistry.retire` | **port**: `worker::Slot::retire` | #57 (+#60, #61, #62) | `gtk-shell.md:380` |
| 104 | `WorkerRegistry._prune_soon` | n/a: same | #57 (+#60, #61, #62) | `gtk-shell.md:381` |
| 107 | `WorkerRegistry.retire_attrs` | **port**: `worker::Slot::retire (one slot per attribute) + session_ops::close_teardown` | #57 (+#60, #61, #62) | `gtk-shell.md:382` |
| 129 | `WorkerRegistry.wait_for` | **port**: `session_ops::close_teardown (the autosave wait) + worker::Workers::shutdown` | #57 (+#60, #61, #62) | `gtk-shell.md:383` |


## 5. What is open, what is accepted, and what is not a placeholder

### 5.1 Open items: a Rust home that does not exist yet

Nine rows of section 4 say **OPEN**. They are two features, neither on any merged ticket. The test fails if an OPEN row is not named here, or if a name here has no OPEN row. This PR does not port them (6.j changes no behaviour). **Ruled by Cloud on [DAN-2445](/DAN/issues/DAN-2445) (2026-10-30, option 2 for both): both are ported by [DAN-2448](/DAN/issues/DAN-2448), Phase 6.h-2 (author Virgil, verifier Beatrice, executor Dante, approver Minos), which waits for 6.j to be `done`. Every OPEN row below is owned by DAN-2448.** The tooltip on each insensitive widget and D-C of `docs/gtk-6h.md` stay as they are until then. Phase 6 is complete apart from this section 5.1, and no claim says more: the DAN-1961 phase-complete claim waits for DAN-2448's merge.

| Item | Python | What Rust does today | Why it is open |
|---|---|---|---|
| **O1** Settings > Test buttons and Fetch from Hydrus | `gui/settings_dialog.py:1294` `_test_deviantart`, `:1384` `_test_animepictures`, `:1478` `_test_sankaku`, `:1685` `_test_e621`, `:1763` `_test_pixiv`, `:1882` `_test_hydrus`, `:1853` `_fetch_tag_services`, `:1845` `_on_service_selected` | Built and insensitive, Python's tooltip on each (`settings_dialog.rs:410`, `insensitive_network_buttons`; documented difference D-C of `docs/gtk-6h.md`) | Each makes a live request to a site or to Hydrus. Porting needs the call, a transport differential over https against a scripted server, and a stalled-server test. `docs/gtk-6h.md` and `docs/gtk-6i.md` name the owner as `6.h-2`; no such ticket was filed. |
| **O2** Review > Shortcuts list | `gui/review_view.py:619` `_show_shortcuts_dialog` | The button is built and insensitive; tooltip `The shortcuts list arrives in a later step.` (`review_ops.rs:318`; D7 of `docs/gtk-6f.md`) | `docs/gtk-6f.md` section 2 flagged it to Cloud for 6.f-2 or 6.h; neither took it. The Shortcuts tab of Settings (6.i) edits the bindings; this is the read-only list in the Review screen. |

### 5.2 Accepted gaps for Phase 7

Recorded by the DAN-2421 ruling (Cloud, 2026-10-30) and Oderisi's DAN-2424 q2 rider ledger on DAN-2319. They are gaps, listed and not fixed here: 6.d/6.e Parser Health items, the 6.c `_ (_)` fragment, the D-B and D-A conditions, 6.g F6, F7 and F8, 6.h F4 and F5, the Settings Lens box shot, and from the 6.i read the "Qt fires neither" copy, conflicting rows not marked in the Shortcuts list, the 32-stop Shortcuts Tab order and the recorder's Esc hint.

### 5.3 No disabled placeholder action

6.c registered its not-yet-built menu items as disabled actions marked `DEFERRED(6.x)` (`menu.rs` header). None is left: `menu.rs` has no `DEFERRED(` marker outside its header comment and no `Owner::Later(` entry, and the test `no_menu_action_is_deferred` holds that. The two insensitive buttons of 5.1 are widgets, not menu actions, and are the open items above.

## 6. The 50 Python tests no earlier doc named

`testunion` (an `ast` walk of the 16 Phase 6 test files and the six `test_review_*` files at `76c85c4`, matching test names against the rows of the 6.a to 6.i docs) found 783 tests, 733 named and 50 not: 33 in `test_nothing_survived.py`, 14 in `test_ghost_kanji.py`, 3 in `test_render_states.py`. They are the 50 rows below, one each. `docs/phase6-unmapped-tests-76c85c4.txt` lists them and the test `phase6_coverage.rs` fails if a row is missing, extra, or does not carry what its disposition says.

Dispositions: **pinned** cites the assertion (`file:line`, a line that asserts) and names the mutation that turned it red, with the test that went red (`worktrees/DAN-2223-mutations-b.log`, `tools` outside the repo); **not asserted**, **judged in the shot set** and **n/a** each give the reason. Nothing is called pinned for a pixel, a colour or a gap that no test reads.

| Line | Test | Disposition | Assertion | Red under |
|---|---|---|---|---|
| 48 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_a_fault_in_the_previous_runs_section_counts` | **pinned**: a fault in the section before the last mark counts | `crates/hatate-gtk/src/startup.rs:118` | `faults`: `startup::tests::the_python_cases_of_faults_in_the_previous_run` and 2 more |
| 51 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_one_per_fault_not_per_thread_stack` | **pinned**: two faults, two counts | `crates/hatate-gtk/src/startup.rs:120` | `faults`: same three |
| 54 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_a_kill_that_wrote_nothing_is_zero` | **pinned**: an empty section is zero | `crates/hatate-gtk/src/startup.rs:122` | `floorvalue`: `faults_are_counted_in_the_section_before_the_last`, `the_python_cases_of_faults_in_the_previous_run` |
| 57 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_this_runs_own_section_is_not_counted` | **pinned**: anything after the last mark is this run's | `crates/hatate-gtk/src/startup.rs:124` | `faults`: same three |
| 61 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_older_runs_are_not_counted` | **pinned**: only the section before the last is read | `crates/hatate-gtk/src/startup.rs:126` | `faults`: same three |
| 64 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_a_first_ever_run_has_no_previous_run` | **pinned**: one mark is no previous run | `crates/hatate-gtk/src/startup.rs:128` | `floorvalue`: same two |
| 67 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_a_missing_or_empty_log_is_zero` | **pinned**: an empty log is zero (the startup assertion), and a missing `crash.log` reads as zero (the window assertion: the sandbox has none) | `crates/hatate-gtk/src/startup.rs:130`, `crates/hatate-gtk/tests/smoke.rs:1016` | `floorvalue`: same two; `missinglog` (`.unwrap_or(0)` to `1`): `nothing_survived_page_content` |
| 71 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_the_mark_is_what_install_writes` | **n/a**: the Rust app installs no faulthandler and writes no mark: `crash.log` "is Python's and is not written" (`crates/hatate-core/src/crash_marker.rs:13`). The reader counts what a Python run left. Finding F-B2: a crashed Rust run leaves nothing in `crash.log`, so the Nothing survived page says `no new entry` for it, which is what is on disk. | - | - |
| 77 | `test_nothing_survived.py` `TestSavedSessionAge.test_age_is_read_from_the_store_mtime` | **pinned**: a store written three days ago is three days old, and the older of two stores does not move it | `crates/hatate-core/tests/session_compat.rs:862`, `crates/hatate-core/tests/session_compat.rs:866` | `age` (`.reduce(f64::max)` to `min`): `saved_session_age_is_the_stores_mtime` |
| 88 | `test_nothing_survived.py` `TestSavedSessionAge.test_no_store_is_none` | **pinned**: a profile with no session store has no age (`None`), not zero | `crates/hatate-core/tests/session_compat.rs:876` | `nostore` (`.or(Some(0.0))`): `rule8_nothing_saved_is_an_empty_list_and_says_so` |
| 129 | `test_nothing_survived.py` `TestVariantShown.test_the_unclean_empty_restore_renders_the_variant` | **pinned**: an unclean start with an empty session file shows the variant, titled `Nothing survived.` | `crates/hatate-gtk/tests/smoke.rs:1097`, `crates/hatate-gtk/tests/smoke.rs:970` | `showsurvived` (the call removed): `nothing_survived_variant`; `title`: `nothing_survived_page_content` |
| 154 | `test_nothing_survived.py` `TestVariantShown.test_the_status_bar_echo_stays` | **pinned**: the status bar keeps its line | `crates/hatate-gtk/tests/smoke.rs:1094` | `echo`: `nothing_survived_variant` |
| 159 | `test_nothing_survived.py` `TestVariantShown.test_the_headline_is_serif_36px` | **pinned**: the display-title rule is the serif face at 36 px in both schemes (the sheet; the drawn pixel size is the shot set's) | `crates/hatate-gtk/src/theme.rs:774` | `display` (`%display%px` to `%title%px`): `theme::tests::the_display_title_is_serif_36px` |
| 168 | `test_nothing_survived.py` `TestVariantShown.test_it_explains_itself_in_prose` | **pinned**: the prose says there is no queue to restore and is longer than 80 characters | `crates/hatate-gtk/tests/smoke.rs:972`, `crates/hatate-gtk/tests/smoke.rs:973` | `prose`: `nothing_survived_page_content` |
| 174 | `test_nothing_survived.py` `TestVariantShown.test_the_block_sits_where_the_empty_state_puts_it` | **not asserted**: no test measures the 37% share. Both variants hand `top_weighted` the same `DROP_ZONE_BELOW_WEIGHT` (`crates/hatate-gtk/src/banners.rs:124`), which is the structural half; the pixel half is `survived-dark.png` / `survived-light.png`. Same disposition as `docs/gtk-shell.md:560`. | - | - |
| 187 | `test_nothing_survived.py` `TestVariantShown.test_it_keeps_the_frame_and_brackets` | **pinned**: the page has its four crop marks and its `01 // Queue` label (that the page fills the stack is layout, not asserted) | `crates/hatate-gtk/tests/smoke.rs:1025`, `crates/hatate-gtk/tests/smoke.rs:977` | `nobrackets`, `section`: `nothing_survived_page_content` |
| 194 | `test_nothing_survived.py` `TestVariantShown.test_the_watermark_is_the_lists_not_the_empty_pages` | **pinned**: while the page shows, the ghost is the Queue's 力, not the Empty page's 空 | `crates/hatate-gtk/tests/smoke.rs:1026` | `ghostglyph`: `nothing_survived_page_content` |
| 200 | `test_nothing_survived.py` `TestActionGap.test_the_three_actions_are_12px_apart` | **pinned**: the row's spacing is 12 (`BUTTON_ROW_SPACING`); Python measured pixels between the buttons | `crates/hatate-gtk/tests/smoke.rs:1022` | `gap` (`BUTTON_ROW_SPACING` 12 to 10): `nothing_survived_page_content` |
| 230 | `test_nothing_survived.py` `TestEmptyRhythm.test_the_prose_is_the_mockups_measure_and_leading` | **judged in the shot set**: Python asserts a 560 px measure and a 21 px leading. Rust sets `max_width_chars(60)` (`widgets.rs` `empty_prose`) and the leading comes from the sheet; the measure is in `survived-dark.rects` (prose box 659 x 40 at 1100 x 800, hosted GTK 4.14). Same disposition as `docs/gtk-shell.md:1157` (leading judged in the shot set). Not a pass: a design-read number. | - | - |
| 249 | `test_nothing_survived.py` `TestEmptyRhythm.test_the_nothing_survived_gaps` | **not asserted**: Python measures 16, 16, 18, 28 px between glyph, title, prose, meta line and buttons (its 6 px item spacing plus the spacers 10, 10, 12, 22). Rust has the spacers and no item spacing: from `survived-dark.rects` (hosted, label boxes) glyph to title 10, title to prose 10, prose to meta 12. The empty state reads 10 where Python reads 16 (`empty-dark.rects`: title to prose 10). Finding F-B4 for the design read; no test holds either number. | - | - |
| 263 | `test_nothing_survived.py` `TestBoxedGlyph.test_it_is_a_boxed_cross` | **pinned**: a 42 px square holding U+2715 | `crates/hatate-gtk/tests/smoke.rs:967`, `crates/hatate-gtk/tests/smoke.rs:968` | `glyphbox` (42 to 40): `nothing_survived_page_content` |
| 272 | `test_nothing_survived.py` `TestBoxedGlyph.test_the_box_is_drawn_and_the_cross_is_inked` | **judged in the shot set**: pixels: `survived-dark.png`, `survived-light.png`. Finding F-B3: Rust draws the box in `@ink_28` (`theme.rs` `.survived-glyph`), Python in `ink_46` (`gui/theme.py:448`), and `docs/gtk-shell.md:1628` says a control whose border is its shape uses `ink-46`. Run C measures the box edge against the card. | - | - |
| 292 | `test_nothing_survived.py` `TestBoxedGlyph.test_the_box_and_the_cross_clear_their_contrast_floors` | **not asserted**: no test. The contrast table of run C measures the box (3:1) and the cross (4.5:1) from the hosted shots; F-B3 says the box will not clear 3:1 at `ink_28`. | - | - |
| 308 | `test_nothing_survived.py` `TestMetaLine.test_it_reads_crash_log_and_last_saved_session` | **pinned**: the pairs are Crash log / no new entry / Last saved session / `<age> ago` (the age text itself is `text::format_duration`, tested in `text.rs`; the window test does not fix a clock) | `crates/hatate-gtk/tests/smoke.rs:1016`, `crates/hatate-gtk/tests/smoke.rs:1018` | `metalabel`, `metaage`: `nothing_survived_page_content` |
| 314 | `test_nothing_survived.py` `TestMetaLine.test_figures_are_bright_and_labels_dim` | **pinned**: the classes alternate label, figure, label, figure (the colours are `theme.rs` `.meta-label` / `.meta-figure`; the contrast is run C's) | `crates/hatate-gtk/tests/smoke.rs:1011` | `metakinds`: `nothing_survived_page_content` |
| 320 | `test_nothing_survived.py` `TestMetaLine.test_it_is_uppercase_mono` | **pinned**: every meta label carries `caps` (the rule that turns it uppercase is `theme.rs` `.caps`, judged in the shot set) | `crates/hatate-gtk/tests/smoke.rs:1012` | `metacaps`: `nothing_survived_page_content` |
| 327 | `test_nothing_survived.py` `TestMetaLine.test_the_count_follows_what_the_log_holds` | **pinned**: 0 is `no new entry`, 1 is `1 new entry`, 3 is `3 new entries`; the count is the startup reader's | `crates/hatate-gtk/src/banners.rs:267`, `crates/hatate-gtk/src/startup.rs:118` | `figure`: `banners::tests::the_crash_log_figure_is_a_count_or_none`; `faults`: the startup tests |
| 334 | `test_nothing_survived.py` `TestMetaLine.test_an_unreadable_session_age_is_left_out_not_invented` | **pinned**: with the store gone the meta line holds the crash-log pair alone | `crates/hatate-gtk/tests/smoke.rs:1066` | `ageomit` (`unwrap_or(0.0)` in place of the `if let`): `nothing_survived_page_content` |
| 339 | `test_nothing_survived.py` `TestMetaLine.test_the_text_clears_the_body_contrast_floor` | **not asserted**: no test. Run C's contrast table measures `ink_65` and `ink_100` over the card from the hosted shots, both schemes. | - | - |
| 348 | `test_nothing_survived.py` `TestActions.test_the_three_actions_in_order_uppercase_mono` | **pinned**: Start new search, View crash log, Add files… in tree order, all `caps`, only the first `primary` | `crates/hatate-gtk/tests/smoke.rs:995`, `crates/hatate-gtk/tests/smoke.rs:997`, `crates/hatate-gtk/tests/smoke.rs:999` | `order`, `labels`, `primary`: `nothing_survived_page_content` |
| 360 | `test_nothing_survived.py` `TestActions.test_the_brackets_do_not_swallow_the_actions` | **pinned**: the crop marks are not pointer targets, so they cannot take a click meant for a button (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:933` | `cornertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 367 | `test_nothing_survived.py` `TestActions.test_view_crash_log_opens_the_crash_log` | **pinned**: the button opens the viewer on the crash log | `crates/hatate-gtk/tests/smoke.rs:1038` | `crashbtn`: `nothing_survived_page_content` |
| 375 | `test_nothing_survived.py` `TestActions.test_add_files_adds_files` | **pinned**: the button asks for images (`Add images`) | `crates/hatate-gtk/tests/smoke.rs:1041` | `addbtn`: `nothing_survived_page_content` |
| 45 | `test_ghost_kanji.py` `TestGlyphPerMode.test_each_mode_has_its_own_glyph` | **pinned**: Queue 力, Review 鏡, Activity 動 | `crates/hatate-gtk/src/shell.rs:450` | `ghostreview`: `shell::tests::modes_and_ghosts_are_pythons` |
| 51 | `test_ghost_kanji.py` `TestGlyphPerMode.test_the_empty_glyph_is_in_the_bundled_subset` | **pinned**: Empty is 空 and every ghost glyph is one the bundled kanji face carries | `crates/hatate-gtk/src/shell.rs:463` | `emptyglyph`, `kanjiglyphs`: `shell::tests::modes_and_ghosts_are_pythons` |
| 56 | `test_ghost_kanji.py` `TestGlyphPerMode.test_set_ghost_changes_the_glyph` | **pinned**: `set_glyph` changes what `glyph()` reads | `crates/hatate-gtk/tests/smoke.rs:629` | `setglyph` (`set_text` removed): `empty_state_and_idle_strip` |
| 61 | `test_ghost_kanji.py` `TestGlyphPerMode.test_ink_tier_is_about_six_percent` | **pinned**: ink-05 is 0.05 dark and 0.08 light | `crates/hatate-gtk/src/tokens.rs:327`, `crates/hatate-gtk/src/tokens.rs:340` | `ink05`: `tokens::tests::dark_tokens_are_the_sheets_values`; `ink05light`: `light_tokens_override_only_what_the_light_block_sets` |
| 107 | `test_ghost_kanji.py` `TestGhostShowsOnEveryPage.test_the_glyph_shows_wherever_nothing_sits_on_it` | **judged in the shot set**: Python samples the glyph's stroke pixels on every page for a filled container hiding it. No Rust test does; the shots show it (`empty-*`, `queue-*`, `review-*`, `activity-*`, dark and light) and `docs/gtk-shell.md:1634` records the ghost fix. Run C's pixel table measures ink-05 at stroke points on three pages. | - | - |
| 150 | `test_ghost_kanji.py` `TestNoHitTestInterference.test_a_button_under_the_glyph_still_takes_the_click` | **pinned**: the glyph label and its holder are not pointer targets (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:908`, `crates/hatate-gtk/tests/smoke.rs:912` | `ghosttarget`, `holdertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 167 | `test_ghost_kanji.py` `TestNoHitTestInterference.test_the_ghost_owns_no_child_to_intercept_anything` | **pinned**: same two assertions: nothing under the ghost takes input | `crates/hatate-gtk/tests/smoke.rs:908`, `crates/hatate-gtk/tests/smoke.rs:912` | `ghosttarget`, `holdertarget`: same scenario |
| 188 | `test_ghost_kanji.py` `TestBrackets.test_corners_are_painted_in_ink_65_and_the_interior_is_not` | **judged in the shot set**: the marks are `.corner` boxes with a `@ink_65` border (`theme.rs` `.corner.tl` .. `.br`); their pixels are in the shots (`survived-*`, `queue-*`). Not asserted: the colour of a painted corner. | - | - |
| 219 | `test_ghost_kanji.py` `TestBrackets.test_arms_are_twelve_long_and_two_thick` | **pinned**: each of the four marks measures 12 x 12 (the 2 px arm is the sheet's `2px solid`, judged in the shot set), and the constants are 12 and 2 | `crates/hatate-gtk/tests/smoke.rs:936`, `crates/hatate-gtk/tests/smoke.rs:940` | `cornersize` (`min-width` 12 to 10), `armconst`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 226 | `test_ghost_kanji.py` `TestBrackets.test_brackets_clear_the_non_text_contrast_floor` | **not asserted**: no test. Run C's contrast table measures `ink_65` over the page in both schemes against 3:1. | - | - |
| 232 | `test_ghost_kanji.py` `TestBrackets.test_overlay_is_masked_to_the_marks_and_passes_the_mouse_through` | **pinned**: the mouse half only: no mark is a pointer target. The mask half is n/a: the Rust marks are four small boxes, with nothing drawn between them to mask | `crates/hatate-gtk/tests/smoke.rs:933` | `cornertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 247 | `test_ghost_kanji.py` `TestBrackets.test_a_child_behind_a_corner_still_takes_the_click` | **pinned**: same assertion as the row above (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:933` | `cornertarget`: same scenario |
| 259 | `test_ghost_kanji.py` `TestBrackets.test_overlay_follows_the_frame_and_stays_on_top` | **n/a**: `gtk::Overlay` gives each overlay the child's size and draws it after the child (`widgets::brackets`); there is no overlay to resize or raise by hand. | - | - |
| 297 | `test_ghost_kanji.py` `TestScrollingStaysCheap.test_the_glyph_is_rasterised_once_and_blits_are_microseconds` | **n/a**: Python counts its own pixmap rasterisations. The Rust ghost is a plain label GTK lays out and caches, so there is no counter to read. The scroll cost the claim is about is measured at the list: `shot ... queue-5000-scroll` prints the frame times (`docs/queue-view.md:135`). | - | - |
| 64 | `test_render_states.py` `TestRenderStates.test_queue_review_and_activity_render_in_both_themes` | **judged in the shot set**: `shot --all` renders `queue`, `review`, `activity` (and 92 more states) dark and light, and the CI step `Shot set (under Xvfb)` fails on any non-zero child. Differences: 1100 x 800 and the minimum, not 1440 x 900 (`docs/gtk-6e.md:141`); "the invoking config is untouched" is not asserted (the tool builds a temp profile: `tempfile::tempdir` in `examples/shot.rs`). | - | - |
| 103 | `test_render_states.py` `TestRenderStates.test_names_are_the_mockups` | **n/a**: the Rust names are `<state>-<scheme>.png` and `<state>-min-<scheme>.png` with `index.tsv` mapping each to its state, not the mockup's `-1440` names (Python dark is the unsuffixed default). Written as a deviation D-B1 for the design read. | - | - |
| 113 | `test_render_states.py` `TestRenderStates.test_unknown_state_fails_loudly_and_writes_nothing` | **pinned**: an unknown state exits 2 and writes no PNG; before this ticket it rendered the default window and exited 0 (proof in `worktrees/DAN-2223-unknown-state.log`). Held by the CI step that runs it | `.github/workflows/ci.yml:57` | guard removed (`if false && !known_state`): exit 0 and the PNG written, so the CI step's `if ...; then exit 1` turns red |

### Findings from this table

- **F-B2** `crash.log` is never written by the Rust app, so the Nothing survived page and the Crashed banner read `no new entry` for a crashed Rust run. The protocol (`crash.log.running`, `last_clean_exit`) does mark the crash; only the figure is empty. Not changed here.
- **F-B3** `.survived-glyph` draws its box in `@ink_28`; Python draws `ink_46`, and the border-tier rule (`docs/gtk-shell.md:1628`) says `ink-46`. `ink_28` over the card is below the 3:1 non-text floor. Run C measures it; a one-line CSS change plus a test would follow if it fails, as a fixup on this PR.
- **F-B4** the Nothing survived page and the drop zone have 10 px between headline and prose where Python measures 16 (Qt's 6 px item spacing plus the 10 px spacer). Hosted label boxes: `survived-dark.rects`, `empty-dark.rects`. For the design read; no test holds either number.
- **D-B1** shot names differ from the mockup's (`<state>-<scheme>.png`, not `<state>-1440.png`); sizes are 1100 x 800 and the minimum.
