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
| 6.h-2 Settings Tests, Fetch from Hydrus, Review Shortcuts | `docs/gtk-6h2.md` | #82 |

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
| `gui/review_view.py` | 29 | `review_ops.rs`, `review_view.rs`, `review_compare.rs`, `wipe_view.rs`, `compare_dialog.rs`, `shortcuts_dialog.rs` | #71, #72, #82 |
| `gui/session_autosave.py` | 6 | none: the file has no Rust module of its own (see its rows) | #57, #60, #61, #62 |
| `gui/settings_dialog.py` | 55 | `settings_dialog.rs`, `settings_mcp.rs`, `settings_spec.rs`, `settings_form.rs`, `mcp_audit_view.rs`, `settings_tests.rs` | #75, #76, #82 |
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
| 619 | `ReviewViewMixin._show_shortcuts_dialog` | `crates/hatate-gtk/src/shortcuts_dialog.rs:52` `ShortcutsDialog::build`, `:178` `action_show_shortcuts` | #82 | `gtk-6h2.md:45` |
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
| 1294 | `SettingsDialog._test_deviantart` | `crates/hatate-gtk/src/settings_tests.rs:173` `plan`, `:309` `run`, `:401` `read_page` | #82 | `gtk-6h2.md:47` |
| 1384 | `SettingsDialog._test_animepictures` | `crates/hatate-gtk/src/settings_tests.rs:173` `plan`, `:309` `run`, `:401` `read_page` | #82 | `gtk-6h2.md:48` |
| 1478 | `SettingsDialog._test_sankaku` | `crates/hatate-gtk/src/settings_tests.rs:173` `plan`, `:309` `run` | #82 | `gtk-6h2.md:49` |
| 1528 | `SettingsDialog._build_saucenao_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1593 | `SettingsDialog._build_hydrus_tab` | `settings_spec.rs` (data) and `settings_form.rs` `Form::build` | #75 | `gtk-6h.md:58` |
| 1685 | `SettingsDialog._test_e621` | `crates/hatate-gtk/src/settings_tests.rs:352` `run_e621` | #82 | `gtk-6h2.md:50` |
| 1763 | `SettingsDialog._test_pixiv` | `crates/hatate-gtk/src/settings_tests.rs:173` `plan`, `:309` `run`, `:401` `read_page` | #82 | `gtk-6h2.md:51` |
| 1845 | `SettingsDialog._on_service_selected` | `crates/hatate-gtk/src/settings_dialog.rs:684` `pick_service` | #82 | `gtk-6h2.md:54` |
| 1853 | `SettingsDialog._fetch_tag_services` | `crates/hatate-gtk/src/settings_dialog.rs:602` `fetch_tag_services`, `:635` `on_services_fetched` | #82 | `gtk-6h2.md:53` |
| 1882 | `SettingsDialog._test_hydrus` | `crates/hatate-gtk/src/settings_dialog.rs:566` `test_hydrus`, `crates/hatate-gtk/src/settings_tests.rs:531` `hydrus_text` | #82 | `gtk-6h2.md:52` |
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


## 5. What is accepted, and what is not a placeholder

### 5.1 Accepted gaps for Phase 7

Recorded by the DAN-2421 ruling (Cloud, 2026-10-30) and Oderisi's DAN-2424 q2 rider ledger on DAN-2319. They are gaps, listed and not fixed here: 6.d/6.e Parser Health items, the 6.c `_ (_)` fragment, the D-B and D-A conditions, 6.g F6, F7 and F8, 6.h F4 and F5, the Settings Lens box shot, and from the 6.i read the "Qt fires neither" copy, conflicting rows not marked in the Shortcuts list, the 32-stop Shortcuts Tab order and the recorder's Esc hint.

### 5.2 No disabled placeholder action

6.c registered its not-yet-built menu items as disabled actions marked `DEFERRED(6.x)` (`menu.rs` header). None is left: `menu.rs` has no `DEFERRED(` marker outside its header comment and no `Owner::Later(` entry, and the test `no_menu_action_is_deferred` holds that.

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
| 67 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_a_missing_or_empty_log_is_zero` | **pinned**: an empty log is zero (the startup assertion), and a missing `crash.log` reads as zero (the window assertion: the sandbox has none) | `crates/hatate-gtk/src/startup.rs:130`, `crates/hatate-gtk/tests/smoke.rs:1022` | `floorvalue`: same two; `missinglog` (`.unwrap_or(0)` to `1`): `nothing_survived_page_content` |
| 71 | `test_nothing_survived.py` `TestFaultsInPreviousRun.test_the_mark_is_what_install_writes` | **n/a**: the Rust app installs no faulthandler and writes no mark: `crash.log` "is Python's and is not written" (`crates/hatate-core/src/crash_marker.rs:13`). The reader counts what a Python run left. Finding F-B2: a crashed Rust run leaves nothing in `crash.log`, so the Nothing survived page says `no new entry` for it, which is what is on disk. | - | - |
| 77 | `test_nothing_survived.py` `TestSavedSessionAge.test_age_is_read_from_the_store_mtime` | **pinned**: a store written three days ago is three days old, and the older of two stores does not move it | `crates/hatate-core/tests/session_compat.rs:862`, `crates/hatate-core/tests/session_compat.rs:866` | `age` (`.reduce(f64::max)` to `min`): `saved_session_age_is_the_stores_mtime` |
| 88 | `test_nothing_survived.py` `TestSavedSessionAge.test_no_store_is_none` | **pinned**: a profile with no session store has no age (`None`), not zero | `crates/hatate-core/tests/session_compat.rs:876` | `nostore` (`.or(Some(0.0))`): `rule8_nothing_saved_is_an_empty_list_and_says_so` |
| 129 | `test_nothing_survived.py` `TestVariantShown.test_the_unclean_empty_restore_renders_the_variant` | **pinned**: an unclean start with an empty session file shows the variant, titled `Nothing survived.` | `crates/hatate-gtk/tests/smoke.rs:1103`, `crates/hatate-gtk/tests/smoke.rs:976` | `showsurvived` (the call removed): `nothing_survived_variant`; `title`: `nothing_survived_page_content` |
| 154 | `test_nothing_survived.py` `TestVariantShown.test_the_status_bar_echo_stays` | **pinned**: the status bar keeps its line | `crates/hatate-gtk/tests/smoke.rs:1100` | `echo`: `nothing_survived_variant` |
| 159 | `test_nothing_survived.py` `TestVariantShown.test_the_headline_is_serif_36px` | **pinned**: the display-title rule is the serif face at 36 px in both schemes (the sheet; the drawn pixel size is the shot set's) | `crates/hatate-gtk/src/theme.rs:776` | `display` (`%display%px` to `%title%px`): `theme::tests::the_display_title_is_serif_36px` |
| 168 | `test_nothing_survived.py` `TestVariantShown.test_it_explains_itself_in_prose` | **pinned**: the prose says there is no queue to restore and is longer than 80 characters | `crates/hatate-gtk/tests/smoke.rs:978`, `crates/hatate-gtk/tests/smoke.rs:979` | `prose`: `nothing_survived_page_content` |
| 174 | `test_nothing_survived.py` `TestVariantShown.test_the_block_sits_where_the_empty_state_puts_it` | **not asserted**: no test measures the 37% share. Both variants hand `top_weighted` the same `DROP_ZONE_BELOW_WEIGHT` (`crates/hatate-gtk/src/banners.rs:124`), which is the structural half; the pixel half is `survived-dark.png` / `survived-light.png`. Same disposition as `docs/gtk-shell.md:560`. | - | - |
| 187 | `test_nothing_survived.py` `TestVariantShown.test_it_keeps_the_frame_and_brackets` | **pinned**: the page has its four crop marks and its `01 // Queue` label (that the page fills the stack is layout, not asserted) | `crates/hatate-gtk/tests/smoke.rs:1031`, `crates/hatate-gtk/tests/smoke.rs:983` | `nobrackets`, `section`: `nothing_survived_page_content` |
| 194 | `test_nothing_survived.py` `TestVariantShown.test_the_watermark_is_the_lists_not_the_empty_pages` | **pinned**: while the page shows, the ghost is the Queue's 力, not the Empty page's 空 | `crates/hatate-gtk/tests/smoke.rs:1032` | `ghostglyph`: `nothing_survived_page_content` |
| 200 | `test_nothing_survived.py` `TestActionGap.test_the_three_actions_are_12px_apart` | **pinned**: the row's spacing is 12 (`BUTTON_ROW_SPACING`); Python measured pixels between the buttons | `crates/hatate-gtk/tests/smoke.rs:1028` | `gap` (`BUTTON_ROW_SPACING` 12 to 10): `nothing_survived_page_content` |
| 230 | `test_nothing_survived.py` `TestEmptyRhythm.test_the_prose_is_the_mockups_measure_and_leading` | **judged in the shot set**: Python asserts a 560 px measure and a 21 px leading. Rust sets `max_width_chars(60)` (`widgets.rs` `empty_prose`) and the leading comes from the sheet; the measure is in `survived-dark.rects` (prose box 659 x 40 at 1100 x 800, hosted GTK 4.14). Same disposition as `docs/gtk-shell.md:1157` (leading judged in the shot set). Not a pass: a design-read number. | - | - |
| 249 | `test_nothing_survived.py` `TestEmptyRhythm.test_the_nothing_survived_gaps` | **not asserted**: Python measures 16, 16, 18, 28 px between glyph, title, prose, meta line and buttons (its 6 px item spacing plus the spacers 10, 10, 12, 22). Rust has the spacers and no item spacing: from `survived-dark.rects` (hosted, label boxes) glyph to title 10, title to prose 10, prose to meta 12. The empty state reads 10 where Python reads 16 (`empty-dark.rects`: title to prose 10). Finding F-B4 for the design read; no test holds either number. | - | - |
| 263 | `test_nothing_survived.py` `TestBoxedGlyph.test_it_is_a_boxed_cross` | **pinned**: a 42 px square holding U+2715 | `crates/hatate-gtk/tests/smoke.rs:973`, `crates/hatate-gtk/tests/smoke.rs:974` | `glyphbox` (42 to 40): `nothing_survived_page_content` |
| 272 | `test_nothing_survived.py` `TestBoxedGlyph.test_the_box_is_drawn_and_the_cross_is_inked` | **judged in the shot set**: pixels: `survived-dark.png`, `survived-light.png`. Finding F-B3: Rust draws the box in `@ink_28` (`theme.rs` `.survived-glyph`), Python in `ink_46` (`gui/theme.py:448`), and `docs/gtk-shell.md:1628` says a control whose border is its shape uses `ink-46`. Run C measures the box edge against the card. | - | - |
| 292 | `test_nothing_survived.py` `TestBoxedGlyph.test_the_box_and_the_cross_clear_their_contrast_floors` | **not asserted**: no test. The contrast table of run C measures the box (3:1) and the cross (4.5:1) from the hosted shots; F-B3 says the box will not clear 3:1 at `ink_28`. | - | - |
| 308 | `test_nothing_survived.py` `TestMetaLine.test_it_reads_crash_log_and_last_saved_session` | **pinned**: the pairs are Crash log / no new entry / Last saved session / `<age> ago` (the age text itself is `text::format_duration`, tested in `text.rs`; the window test does not fix a clock) | `crates/hatate-gtk/tests/smoke.rs:1022`, `crates/hatate-gtk/tests/smoke.rs:1024` | `metalabel`, `metaage`: `nothing_survived_page_content` |
| 314 | `test_nothing_survived.py` `TestMetaLine.test_figures_are_bright_and_labels_dim` | **pinned**: the classes alternate label, figure, label, figure (the colours are `theme.rs` `.meta-label` / `.meta-figure`; the contrast is run C's) | `crates/hatate-gtk/tests/smoke.rs:1017` | `metakinds`: `nothing_survived_page_content` |
| 320 | `test_nothing_survived.py` `TestMetaLine.test_it_is_uppercase_mono` | **pinned**: every meta label carries `caps` (the rule that turns it uppercase is `theme.rs` `.caps`, judged in the shot set) | `crates/hatate-gtk/tests/smoke.rs:1018` | `metacaps`: `nothing_survived_page_content` |
| 327 | `test_nothing_survived.py` `TestMetaLine.test_the_count_follows_what_the_log_holds` | **pinned**: 0 is `no new entry`, 1 is `1 new entry`, 3 is `3 new entries`; the count is the startup reader's | `crates/hatate-gtk/src/banners.rs:267`, `crates/hatate-gtk/src/startup.rs:118` | `figure`: `banners::tests::the_crash_log_figure_is_a_count_or_none`; `faults`: the startup tests |
| 334 | `test_nothing_survived.py` `TestMetaLine.test_an_unreadable_session_age_is_left_out_not_invented` | **pinned**: with the store gone the meta line holds the crash-log pair alone | `crates/hatate-gtk/tests/smoke.rs:1072` | `ageomit` (`unwrap_or(0.0)` in place of the `if let`): `nothing_survived_page_content` |
| 339 | `test_nothing_survived.py` `TestMetaLine.test_the_text_clears_the_body_contrast_floor` | **not asserted**: no test. Run C's contrast table measures `ink_65` and `ink_100` over the card from the hosted shots, both schemes. | - | - |
| 348 | `test_nothing_survived.py` `TestActions.test_the_three_actions_in_order_uppercase_mono` | **pinned**: Start new search, View crash log, Add files… in tree order, all `caps`, only the first `primary` | `crates/hatate-gtk/tests/smoke.rs:1001`, `crates/hatate-gtk/tests/smoke.rs:1003`, `crates/hatate-gtk/tests/smoke.rs:1005` | `order`, `labels`, `primary`: `nothing_survived_page_content` |
| 360 | `test_nothing_survived.py` `TestActions.test_the_brackets_do_not_swallow_the_actions` | **pinned**: the crop marks are not pointer targets, so they cannot take a click meant for a button (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:939` | `cornertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 367 | `test_nothing_survived.py` `TestActions.test_view_crash_log_opens_the_crash_log` | **pinned**: the button opens the viewer on the crash log | `crates/hatate-gtk/tests/smoke.rs:1044` | `crashbtn`: `nothing_survived_page_content` |
| 375 | `test_nothing_survived.py` `TestActions.test_add_files_adds_files` | **pinned**: the button asks for images (`Add images`) | `crates/hatate-gtk/tests/smoke.rs:1047` | `addbtn`: `nothing_survived_page_content` |
| 45 | `test_ghost_kanji.py` `TestGlyphPerMode.test_each_mode_has_its_own_glyph` | **pinned**: Queue 力, Review 鏡, Activity 動 | `crates/hatate-gtk/src/shell.rs:450` | `ghostreview`: `shell::tests::modes_and_ghosts_are_pythons` |
| 51 | `test_ghost_kanji.py` `TestGlyphPerMode.test_the_empty_glyph_is_in_the_bundled_subset` | **pinned**: Empty is 空 and every ghost glyph is one the bundled kanji face carries | `crates/hatate-gtk/src/shell.rs:463` | `emptyglyph`, `kanjiglyphs`: `shell::tests::modes_and_ghosts_are_pythons` |
| 56 | `test_ghost_kanji.py` `TestGlyphPerMode.test_set_ghost_changes_the_glyph` | **pinned**: `set_glyph` changes what `glyph()` reads | `crates/hatate-gtk/tests/smoke.rs:633` | `setglyph` (`set_text` removed): `empty_state_and_idle_strip` |
| 61 | `test_ghost_kanji.py` `TestGlyphPerMode.test_ink_tier_is_about_six_percent` | **pinned**: ink-05 is 0.05 dark and 0.08 light | `crates/hatate-gtk/src/tokens.rs:327`, `crates/hatate-gtk/src/tokens.rs:340` | `ink05`: `tokens::tests::dark_tokens_are_the_sheets_values`; `ink05light`: `light_tokens_override_only_what_the_light_block_sets` |
| 107 | `test_ghost_kanji.py` `TestGhostShowsOnEveryPage.test_the_glyph_shows_wherever_nothing_sits_on_it` | **judged in the shot set**: Python samples the glyph's stroke pixels on every page for a filled container hiding it. No Rust test does; the shots show it (`empty-*`, `queue-*`, `review-*`, `activity-*`, dark and light) and `docs/gtk-shell.md:1634` records the ghost fix. Run C's pixel table measures ink-05 at stroke points on three pages. | - | - |
| 150 | `test_ghost_kanji.py` `TestNoHitTestInterference.test_a_button_under_the_glyph_still_takes_the_click` | **pinned**: the glyph label and its holder are not pointer targets (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:914`, `crates/hatate-gtk/tests/smoke.rs:918` | `ghosttarget`, `holdertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 167 | `test_ghost_kanji.py` `TestNoHitTestInterference.test_the_ghost_owns_no_child_to_intercept_anything` | **pinned**: same two assertions: nothing under the ghost takes input | `crates/hatate-gtk/tests/smoke.rs:914`, `crates/hatate-gtk/tests/smoke.rs:918` | `ghosttarget`, `holdertarget`: same scenario |
| 188 | `test_ghost_kanji.py` `TestBrackets.test_corners_are_painted_in_ink_65_and_the_interior_is_not` | **judged in the shot set**: the marks are `.corner` boxes with a `@ink_65` border (`theme.rs` `.corner.tl` .. `.br`); their pixels are in the shots (`survived-*`, `queue-*`). Not asserted: the colour of a painted corner. | - | - |
| 219 | `test_ghost_kanji.py` `TestBrackets.test_arms_are_twelve_long_and_two_thick` | **pinned**: each of the four marks measures 12 x 12 (the 2 px arm is the sheet's `2px solid`, judged in the shot set), and the constants are 12 and 2 | `crates/hatate-gtk/tests/smoke.rs:942`, `crates/hatate-gtk/tests/smoke.rs:946` | `cornersize` (`min-width` 12 to 10), `armconst`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 226 | `test_ghost_kanji.py` `TestBrackets.test_brackets_clear_the_non_text_contrast_floor` | **not asserted**: no test. Run C's contrast table measures `ink_65` over the page in both schemes against 3:1. | - | - |
| 232 | `test_ghost_kanji.py` `TestBrackets.test_overlay_is_masked_to_the_marks_and_passes_the_mouse_through` | **pinned**: the mouse half only: no mark is a pointer target. The mask half is n/a: the Rust marks are four small boxes, with nothing drawn between them to mask | `crates/hatate-gtk/tests/smoke.rs:939` | `cornertarget`: `ghost_and_brackets_take_no_input_and_the_marks_are_12_by_2` |
| 247 | `test_ghost_kanji.py` `TestBrackets.test_a_child_behind_a_corner_still_takes_the_click` | **pinned**: same assertion as the row above (a click is not synthesized) | `crates/hatate-gtk/tests/smoke.rs:939` | `cornertarget`: same scenario |
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

### 6.1 The hand-offs no owner named

`worktrees/DAN-2223-tools/scan_union.py` (a row of any `docs/*.md` that hands a Python test to ticket 6.X, and 6.X's own doc does not name it) found 145 hand-off rows, 119 distinct tests. This is the triage of each: a **scan false positive**, a test **mapped elsewhere** (the Rust test or the doc row that holds it is cited), or a gap given a disposition here. **Pinned** cites the assertion and names the mutation that turned it red (`worktrees/DAN-2223-mutations-b2.log`); **differential** names the Python-vs-Rust comparison that holds the behaviour, with its case count; **mapped elsewhere** cites the Rust `fn` or doc line. `crates/hatate-gtk/tests/phase6_coverage.rs` holds the list exactly (`docs/phase6-handoffs-76c85c4.txt`).

| Line | Test | Disposition | Rust | Red under |
|---|---|---|---|---|
| 105 | `test_app_chrome.py` `TestPageGutter.test_the_top_bar_and_run_strip_share_the_gutter` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 141 | `test_app_chrome.py` `TestPageRhythm.test_the_chrome_is_the_same_height_on_every_screen` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 1453 | `test_core_utils.py` `TestCrashDiagnostics.test_retiring_a_worker_does_not_warn` | **n/a**: [handed to 6.c] `SearchWorker`'s retirement warning has no Rust counterpart: the run is a task on the runtime, not a thread to retire; the window's teardown is `SearchRun::close` (`docs/gtk-6e.md:329`) | - | - |
| 162 | `test_flat_surfaces.py` `TestRegionsAreFlatHairlines.test_run_strip_is_a_hairline_on_the_flat_page` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 140 | `test_nothing_survived.py` `TestVariantShown.test_every_other_restore_keeps_the_generic_drop_zone` | **mapped elsewhere**: [handed to 6.c] a clean restore shows the rows, and a crash with no session file shows no Nothing survived page (`docs/queue-view.md:381`) | `crates/hatate-gtk/tests/smoke.rs:836`, `crates/hatate-gtk/tests/smoke.rs:1088` | - |
| 381 | `test_nothing_survived.py` `TestActions.test_start_new_search_drops_back_to_the_ordinary_drop_zone` | **pinned**: [handed to 6.c] the page's New search button removes the Nothing survived page, shows the `empty` page again and blanks the strip's note | `crates/hatate-gtk/tests/smoke.rs:1115`, `crates/hatate-gtk/tests/smoke.rs:1120` | `dismissnote` (the strip-note assertion; the page assertions are not mutated): `nothing_survived_variant` |
| 393 | `test_nothing_survived.py` `TestRunStrip.test_the_strip_says_the_last_run_failed` | **pinned**: [handed to 6.c] an unclean start with an empty session file sets the strip's note to `RUN_FAILED_NOTE` | `crates/hatate-gtk/tests/smoke.rs:1102` | `failednote`: `nothing_survived_variant` |
| 401 | `test_nothing_survived.py` `TestRunStrip.test_it_sits_beside_the_idle_count` | **judged in the shot set**: [handed to 6.c] the note sits right of the idle count by layout (the strip is one row, count then note); the position is a pixel fact, judged on `crashed-*` and `survived-*` | - | - |
| 408 | `test_nothing_survived.py` `TestRunStrip.test_it_is_absent_on_an_ordinary_start` | **pinned**: [handed to 6.c] a window built with no crash marker has an empty strip note (this run's new assertion: the other scenarios read the note only after a crash) | `crates/hatate-gtk/tests/smoke.rs:638` | `noteinit`: `empty_state_and_idle_strip` |
| 415 | `test_nothing_survived.py` `TestDismissal.test_a_list_replaces_the_notice_and_the_strip_note` | **not asserted**: [handed to 6.c] adding rows while the page shows dismisses it through `refresh_queue_empty_state` (`crates/hatate-gtk/src/window.rs:668`); only the page's own button is clicked in a test (`nothing_survived_variant`), and no scenario adds a row while the page shows. Removing that call leaves the whole smoke suite green. Finding F-B7 | - | `n_listnote` (window.rs:668 call removed): green |
| 78 | `test_render_states.py` `TestRenderStates.test_interrupted_paused_shows_the_quota_banner_in_both_themes` | **mapped elsewhere**: [handed to 6.c] the banner's words and the strip note are `persisted_quota_pause`; both schemes of the banner are on the `quota-*` shots (`docs/gtk-6e.md:457`) | `crates/hatate-gtk/tests/smoke.rs:1354` | - |
| 116 | `test_run_strip.py` `TestStripGeometry.test_eta_is_right_aligned_and_last` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 184 | `test_run_strip.py` `TestStripVoice.test_figures_are_bold_and_labels_are_mono` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 241 | `test_run_strip.py` `TestIdleStrip.test_empty_window_reads_nothing_queued_eta_dash` | **pinned**: [handed to 6.c] the idle strip of an empty window reads `IDLE_PROGRESS` and `IDLE_ETA` | `crates/hatate-gtk/tests/smoke.rs:635`, `crates/hatate-gtk/tests/smoke.rs:636` | `freshprogress`, `fresheta`: `empty_state_and_idle_strip` |
| 254 | `test_run_strip.py` `TestIdleStrip.test_a_fresh_window_is_not_blank_before_anything_refreshes` | **pinned**: [handed to 6.c] the strip is never blank straight after the window is built, before any refresh has run (`shell.rs` sets both labels at build) | `crates/hatate-gtk/tests/smoke.rs:635` | `freshprogress`: `empty_state_and_idle_strip` |
| 261 | `test_run_strip.py` `TestIdleStrip.test_listed_but_unsearched_images_are_not_called_nothing_queued` | **pinned**: [handed to 6.c] one unsearched row reads `1 not searched`, and emptying the list goes back to the idle text | `crates/hatate-gtk/tests/smoke.rs:665`, `crates/hatate-gtk/tests/smoke.rs:683` | `notsearched`: `run_strip_and_sent_count_follow_the_list` |
| 0 | `test_settings_search.py` `(file)` | **scan false positive**: [handed to 6.c/6.f/6.i] the row (`docs/phase6a-core-support.md:158`) names the whole file, not a test: `test_review_*, test_settings_search, ...` are handed to 6.b/6.f-6.i, and each of their tests has its own row in the doc of the ticket that owns it (the union scan finds none unmapped) | - | - |
| 0 | `test_mcp_settings_tab.py` `(file)` | **scan false positive**: [handed to 6.c/6.f/6.i] the row (`docs/phase6a-core-support.md:158`) names the whole file, not a test: `test_review_*, test_settings_search, ...` are handed to 6.b/6.f-6.i, and each of their tests has its own row in the doc of the ticket that owns it (the union scan finds none unmapped) | - | - |
| 0 | `test_tag_sidecars.py` `(file)` | **scan false positive**: [handed to 6.c/6.f/6.i] the row (`docs/phase6a-core-support.md:158`) names the whole file, not a test: `test_review_*, test_settings_search, ...` are handed to 6.b/6.f-6.i, and each of their tests has its own row in the doc of the ticket that owns it (the union scan finds none unmapped) | - | - |
| 0 | `test_sent_tag_filter.py` `(file)` | **scan false positive**: [handed to 6.c/6.f/6.i] the row (`docs/phase6a-core-support.md:158`) names the whole file, not a test: `test_review_*, test_settings_search, ...` are handed to 6.b/6.f-6.i, and each of their tests has its own row in the doc of the ticket that owns it (the union scan finds none unmapped) | - | - |
| 0 | `test_gui_harness.py` `(file)` | **scan false positive**: [handed to 6.c/6.f/6.i] the row (`docs/phase6a-core-support.md:158`) names the whole file, not a test: `test_review_*, test_settings_search, ...` are handed to 6.b/6.f-6.i, and each of their tests has its own row in the doc of the ticket that owns it (the union scan finds none unmapped) | - | - |
| 240 | `test_nothing_survived.py` `TestEmptyRhythm.test_the_drop_zone_gaps` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 425 | `test_nothing_survived.py` `TestDismissal.test_emptying_the_list_later_is_an_ordinary_empty_queue` | **not asserted**: [handed to 6.c] adding rows while the page shows dismisses it through `refresh_queue_empty_state` (`crates/hatate-gtk/src/window.rs:668`); only the page's own button is clicked in a test (`nothing_survived_variant`), and no scenario adds a row while the page shows. Removing that call leaves the whole smoke suite green. Finding F-B7 | - | `n_listnote` (window.rs:668 call removed): green |
| 102 | `test_queue_view.py` `TestTheEmptyState.test_an_empty_list_shows_the_drop_zone` | **mapped elsewhere**: [handed to 6.c] the empty state is the shell's: `empty_state_and_idle_strip` builds the empty window and reads its heading, section label and the three actions (`docs/queue-view.md:391`) | `crates/hatate-gtk/tests/smoke.rs:611` | - |
| 267 | `test_run_strip.py` `TestIdleStrip.test_a_finished_run_keeps_its_count_and_drops_the_eta` | **pinned**: [handed to 6.c] after a finished run the ETA is back to the idle reading (the count the run ended on is `start_search_runs_the_batch`'s per-row statuses) | `crates/hatate-gtk/tests/smoke.rs:2019` | `idleeta`: `start_search_runs_the_batch` |
| 127 | `test_type_voice.py` `_UppercaseChecks.test_mode_tabs` | **judged in the shot set**: [handed to 6.c/6.e] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 136 | `test_type_voice.py` `_UppercaseChecks.test_mode_group_is_the_mockups_width` | **judged in the shot set**: [handed to 6.c/6.e] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 141 | `test_type_voice.py` `_UppercaseChecks.test_start_search` | **judged in the shot set**: [handed to 6.c/6.e] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 168 | `test_type_voice.py` `_UppercaseChecks.test_empty_queue_buttons` | **judged in the shot set**: [handed to 6.c] Qt pixel geometry, type voice or `sizeHint` of the chrome (gutter, strip hairline, ETA edge, mode tabs, Start button, empty-state gaps); GTK sets the widths from `tokens.css` and the design read measures them on the `empty-*`, `rows-*` and `queue-*` shots (`crates/hatate-gtk/examples/shot.rs` `SHOT_SET`) | - | - |
| 241 | `test_quota_pause_banner.py` `TestActions.test_review_opens_review_on_the_first_waiting_row` | **not asserted**: [handed to 6.d/6.f] `review_the_waiting` (`crates/hatate-gtk/src/window.rs:647`) opens Review on the first waiting row; no scenario clicks the quota banner's `Review the N waiting` button or calls it (`rg review_the_waiting crates/hatate-gtk/tests` finds nothing); making the predicate always false leaves the whole smoke suite green. Finding F-B5 | - | `n_reviewwaiting`: green |
| 322 | `test_gui_smoke.py` `TestMainWindowSignalHandlers.test_hash_and_thumbnail_progress_handlers` | **not asserted**: [handed to 6.d] the hash half is `add_files_hashes_and_dedupes` (status strings, not the gauge or `~Nm left`) and the thumbnail half is `queue_view_thumbnails_load_fail_and_stay_off_the_main_loop` (the pass is idle afterwards); no test reads the progress text of either (`docs/gtk-shell.md:610`) | - | - |
| 914 | `test_gui_smoke.py` `TestTableContextMenu.test_every_actionable_entry_has_a_handler` | **mapped elsewhere**: [handed to 6.d] every entry the menu can show has a handler (`is_wired`), and the menu built for one row and for none is compared against Python's by the direct differential `table_menu` (every handler of every menu called, the method and argument compared: 163,153 / 166,806 / 154,826 cases over seeds 1 to 3, 0 differing; `docs/queue-view.md:82`) and the unit tests of `table_menu.rs` | `crates/hatate-gtk/src/queue_menu.rs:184` | - |
| 920 | `test_gui_smoke.py` `TestTableContextMenu.test_the_same_holds_with_one_row_and_with_none` | **mapped elsewhere**: [handed to 6.d] every entry the menu can show has a handler (`is_wired`), and the menu built for one row and for none is compared against Python's by the direct differential `table_menu` (every handler of every menu called, the method and argument compared: 163,153 / 166,806 / 154,826 cases over seeds 1 to 3, 0 differing; `docs/queue-view.md:82`) and the unit tests of `table_menu.rs` | `crates/hatate-gtk/src/queue_menu.rs:184` | - |
| 927 | `test_gui_smoke.py` `TestTableContextMenu.test_nothing_selected_offers_only_the_select_by_menus` | **mapped elsewhere**: [handed to 6.d] no selection offers only the four Select-by menus, in rank order; also the smoke scenario's `table_menu(&[]).rows.is_none()` | `crates/hatate-core/src/table_menu.rs:530` | - |
| 938 | `test_gui_smoke.py` `TestTableContextMenu.test_labels_follow_the_number_of_rows` | **mapped elsewhere**: [handed to 6.d] a count in a label only past one; one row reads singular, several carry the count | `crates/hatate-core/src/table_menu.rs:517`, `crates/hatate-core/src/table_menu.rs:652` | - |
| 946 | `test_gui_smoke.py` `TestTableContextMenu.test_one_image_only_actions_are_disabled_for_several` | **mapped elsewhere**: [handed to 6.d] several rows switch the single-row entries off | `crates/hatate-core/src/table_menu.rs:652` | - |
| 954 | `test_gui_smoke.py` `TestTableContextMenu.test_those_same_actions_are_enabled_for_one` | **mapped elsewhere**: [handed to 6.d] one row enables the single-row entries | `crates/hatate-core/src/table_menu.rs:621` | - |
| 982 | `test_gui_smoke.py` `TestTableContextMenu.test_each_engine_entry_searches_that_engine` | **mapped elsewhere**: [handed to 6.d] the engine menu lists all ten engines then the opposite one, and a re-search runs the engine it was asked for | `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:2387` | - |
| 1578 | `test_gui_smoke.py` `TestDisplayHelpers.test_finish_time_phrasing_follows_the_distance` | **mapped elsewhere**: [handed to 6.d] the finish time is phrased by distance and an absurd horizon yields none | `crates/hatate-core/src/eta.rs:369` | - |
| 1589 | `test_gui_smoke.py` `TestDisplayHelpers.test_an_absurd_horizon_yields_no_finish_time` | **mapped elsewhere**: [handed to 6.d] the finish time is phrased by distance and an absurd horizon yields none | `crates/hatate-core/src/eta.rs:369` | - |
| 2491 | `test_gui_smoke.py` `TestRowNumbersAreShown.test_rows_are_numbered_from_one` | **mapped elsewhere**: [handed to 6.d] row numbers are positions on screen (`queue_view.rs` header comment); the sibling `test_row_numbers_run_one_to_n_over_visible_rows` is mapped to the same scenario at `docs/queue-view.md:293` | `crates/hatate-gtk/tests/smoke.rs:3382` | - |
| 2503 | `test_gui_smoke.py` `TestRowNumbersAreShown.test_out_of_range_sections_are_blank` | **mapped elsewhere**: [handed to 6.d] row numbers are positions on screen (`queue_view.rs` header comment); the sibling `test_row_numbers_run_one_to_n_over_visible_rows` is mapped to the same scenario at `docs/queue-view.md:293` | `crates/hatate-gtk/tests/smoke.rs:3382` | - |
| 4278 | `test_gui_smoke.py` `TestSelectionSurvivesARefresh.test_dropping_dead_matches_keeps_your_place` | **mapped elsewhere**: [handed to 6.d] dropping dead matches keeps the selection on the same match; editing a tag in the Review panel edits in place | `crates/hatate-core/tests/availability_https.rs:1304`, `crates/hatate-gtk/tests/smoke.rs:4732` | - |
| 4290 | `test_gui_smoke.py` `TestSelectionSurvivesARefresh.test_editing_a_tag_keeps_your_place` | **mapped elsewhere**: [handed to 6.d] dropping dead matches keeps the selection on the same match; editing a tag in the Review panel edits in place | `crates/hatate-core/tests/availability_https.rs:1304`, `crates/hatate-gtk/tests/smoke.rs:4732` | - |
| 5683 | `test_gui_smoke.py` `TestReviewedColumn.test_only_reviewed_rows_are_marked` | **mapped elsewhere**: [handed to 6.d] the Reviewed column's cell text and rank (`docs/phase6a-core-support.md:142`) | `crates/hatate-core/src/status_tier.rs:499` | - |
| 5688 | `test_gui_smoke.py` `TestReviewedColumn.test_a_ticked_cell_explains_itself` | **mapped elsewhere**: [handed to 6.d] the cell's tooltip is compared by the direct differential `table_cells` (tooltip x 12 columns over 600 entries, 0 differing; `docs/queue-view.md:81`) | `crates/hatate-core/src/table_cells.rs:667` | - |
| 5741 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_the_entry_marks_an_unreviewed_row` | **mapped elsewhere**: [handed to 6.d] the Review entry marks unless every selected row is already marked (mixed selection offers to mark); handlers as above; the action differential's `reviewed` op marks and unmarks rows and compares the column, the count and the status line with Python | `crates/hatate-core/src/table_menu.rs:714` | - |
| 5745 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_the_entry_flips_to_unmark_once_marked` | **mapped elsewhere**: [handed to 6.d] the Review entry marks unless every selected row is already marked (mixed selection offers to mark); handlers as above; the action differential's `reviewed` op marks and unmarks rows and compares the column, the count and the status line with Python | `crates/hatate-core/src/table_menu.rs:714` | - |
| 5750 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_a_mixed_selection_offers_to_mark` | **mapped elsewhere**: [handed to 6.d] the Review entry marks unless every selected row is already marked (mixed selection offers to mark); handlers as above; the action differential's `reviewed` op marks and unmarks rows and compares the column, the count and the status line with Python | `crates/hatate-core/src/table_menu.rs:714` | - |
| 5757 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_every_menu_entry_still_has_a_handler` | **mapped elsewhere**: [handed to 6.d] the Review entry marks unless every selected row is already marked (mixed selection offers to mark); handlers as above; the action differential's `reviewed` op marks and unmarks rows and compares the column, the count and the status line with Python | `crates/hatate-core/src/table_menu.rs:714` | - |
| 6028 | `test_gui_smoke.py` `TestExportResults.test_csv_lands_on_disk_with_a_row_per_image` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6036 | `test_gui_smoke.py` `TestExportResults.test_json_lands_on_disk` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6042 | `test_gui_smoke.py` `TestExportResults.test_the_chosen_filter_decides_the_format_not_the_offered_name` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6053 | `test_gui_smoke.py` `TestExportResults.test_a_missing_extension_is_added` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6058 | `test_gui_smoke.py` `TestExportResults.test_under_all_files_the_typed_extension_decides` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6090 | `test_gui_smoke.py` `TestExportResults.test_exporting_a_selection_writes_only_those_rows` | **differential**: [handed to 6.d] the action differential's `export` op (filter csv / json / all, a typed name with and without an extension, scope all or selection) compares the file written and the status line with Python at every step; the Rust scenario is below. Seeds 11 / 12 / 13 x 60 cases: 4 differing steps, all the export failure text (`docs/queue-view.md:92`) | `crates/hatate-gtk/tests/smoke.rs:3900` | - |
| 6098 | `test_gui_smoke.py` `TestExportResults.test_the_status_bar_says_what_was_written` | **mapped elsewhere**: [handed to 6.d] the status line reads `Exported 2 image(s) to out.csv` / `Wrote 2 tag file(s)`; the Files menu offers Export Results and Write Tag Files (`menu.rs` `the_files_menu_is_pythons`) | `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/tests/smoke.rs:3927` | - |
| 6320 | `test_gui_smoke.py` `TestWriteTagFiles.test_the_status_bar_says_what_was_written` | **mapped elsewhere**: [handed to 6.d] the status line reads `Exported 2 image(s) to out.csv` / `Wrote 2 tag file(s)`; the Files menu offers Export Results and Write Tag Files (`menu.rs` `the_files_menu_is_pythons`) | `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/tests/smoke.rs:3927` | - |
| 6103 | `test_gui_smoke.py` `TestExportResults.test_the_files_menu_offers_it` | **mapped elsewhere**: [handed to 6.d] the status line reads `Exported 2 image(s) to out.csv` / `Wrote 2 tag file(s)`; the Files menu offers Export Results and Write Tag Files (`menu.rs` `the_files_menu_is_pythons`) | `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/tests/smoke.rs:3927` | - |
| 6324 | `test_gui_smoke.py` `TestWriteTagFiles.test_the_files_menu_offers_it` | **mapped elsewhere**: [handed to 6.d] the status line reads `Exported 2 image(s) to out.csv` / `Wrote 2 tag file(s)`; the Files menu offers Export Results and Write Tag Files (`menu.rs` `the_files_menu_is_pythons`) | `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/tests/smoke.rs:3927` | - |
| 6186 | `test_gui_smoke.py` `TestWriteTagFiles.test_it_writes_one_file_per_image_beside_the_image` | **differential**: [handed to 6.d] the direct differential `sidecar` (the real `plan()` and `write()` on real trees: read-only folders, existing files, both styles, 5 extensions; plan, result, tree bytes and mode compared, 600 cases / 200 scenarios per seed, 0 differing; `docs/queue-view.md:83`) and the action differential's `write_tags` op | `crates/hatate-gtk/tests/smoke.rs:3973` | - |
| 6193 | `test_gui_smoke.py` `TestWriteTagFiles.test_it_asks_before_writing_anything` | **pinned**: [handed to 6.d] with a file in the way the box is Keep / Replace / Cancel with Keep the default, the text says how many and where, and Cancel writes nothing | `crates/hatate-gtk/tests/smoke.rs:3989`, `crates/hatate-gtk/tests/smoke.rs:3990` | `keepdefault`: `write_tag_files_asks_then_writes_and_keeps_what_is_there` |
| 6199 | `test_gui_smoke.py` `TestWriteTagFiles.test_answering_no_writes_nothing` | **pinned**: [handed to 6.d] with a file in the way the box is Keep / Replace / Cancel with Keep the default, the text says how many and where, and Cancel writes nothing | `crates/hatate-gtk/tests/smoke.rs:3989`, `crates/hatate-gtk/tests/smoke.rs:3990` | `keepdefault`: `write_tag_files_asks_then_writes_and_keeps_what_is_there` |
| 6203 | `test_gui_smoke.py` `TestWriteTagFiles.test_the_prompt_says_how_many_where_and_under_what_name` | **pinned**: [handed to 6.d] with a file in the way the box is Keep / Replace / Cancel with Keep the default, the text says how many and where, and Cancel writes nothing | `crates/hatate-gtk/tests/smoke.rs:3989`, `crates/hatate-gtk/tests/smoke.rs:3990` | `keepdefault`: `write_tag_files_asks_then_writes_and_keeps_what_is_there` |
| 6291 | `test_gui_smoke.py` `TestWriteTagFiles.test_a_missing_file_gets_no_sidecar_and_is_counted` | **differential**: [handed to 6.d] the direct differential `sidecar` (the real `plan()` and `write()` on real trees: read-only folders, existing files, both styles, 5 extensions; plan, result, tree bytes and mode compared, 600 cases / 200 scenarios per seed, 0 differing; `docs/queue-view.md:83`) and the action differential's `write_tags` op | `crates/hatate-gtk/tests/smoke.rs:3973` | - |
| 6308 | `test_gui_smoke.py` `TestWriteTagFiles.test_an_unwritable_target_is_reported_not_raised` | **differential**: [handed to 6.d] the direct differential `sidecar` (the real `plan()` and `write()` on real trees: read-only folders, existing files, both styles, 5 extensions; plan, result, tree bytes and mode compared, 600 cases / 200 scenarios per seed, 0 differing; `docs/queue-view.md:83`) and the action differential's `write_tags` op | `crates/hatate-gtk/tests/smoke.rs:3973` | - |
| 6316 | `test_gui_smoke.py` `TestWriteTagFiles.test_writing_a_selection_touches_only_those_images` | **differential**: [handed to 6.d] the direct differential `sidecar` (the real `plan()` and `write()` on real trees: read-only folders, existing files, both styles, 5 extensions; plan, result, tree bytes and mode compared, 600 cases / 200 scenarios per seed, 0 differing; `docs/queue-view.md:83`) and the action differential's `write_tags` op | `crates/hatate-gtk/tests/smoke.rs:3973` | - |
| 362 | `test_interrupted_banner.py` `TestRowMarker.test_the_restore_flags_the_row_that_was_in_flight_and_resets_it` | **mapped elsewhere**: [handed to 6.d] the restore marks the in-flight row `NotSearched` and flagged, and a clean restore leaves a stale row alone (`mark_interrupted_mid_search`); the window shows it in `restore_after_a_crash_shows_the_banner` | `crates/hatate-core/src/model.rs:636`, `crates/hatate-gtk/tests/smoke.rs:891` | - |
| 370 | `test_interrupted_banner.py` `TestRowMarker.test_a_clean_restore_leaves_a_stale_row_alone` | **mapped elsewhere**: [handed to 6.d] the restore marks the in-flight row `NotSearched` and flagged, and a clean restore leaves a stale row alone (`mark_interrupted_mid_search`); the window shows it in `restore_after_a_crash_shows_the_banner` | `crates/hatate-core/src/model.rs:636`, `crates/hatate-gtk/tests/smoke.rs:891` | - |
| 374 | `test_interrupted_banner.py` `TestRowMarker.test_only_that_rows_engine_cell_says_so_in_italic` | **mapped elsewhere**: [handed to 6.d] the Engine cell of an unsearched flagged row says `interrupted mid-search`, in italic and dim ink; the direct differential `table_cells` compares cell text, ink (dark and light) and italic over 600 entries | `crates/hatate-core/src/table_cells.rs:655` | - |
| 389 | `test_interrupted_banner.py` `TestRowMarker.test_the_marker_is_dim_but_clears_the_text_floor` | **mapped elsewhere**: [handed to 6.d] the Engine cell of an unsearched flagged row says `interrupted mid-search`, in italic and dim ink; the direct differential `table_cells` compares cell text, ink (dark and light) and italic over 600 entries | `crates/hatate-core/src/table_cells.rs:655` | - |
| 424 | `test_interrupted_banner.py` `TestRowMarker.test_it_goes_once_the_row_has_a_result_or_is_searched_again` | **mapped elsewhere**: [handed to 6.d] the marker shows only while the row is unsearched, and a search spends the flag (`batch.md:112`) | `crates/hatate-core/tests/batch_run.rs:1046`, `crates/hatate-core/src/table_cells.rs:655` | - |
| 431 | `test_interrupted_banner.py` `TestRowMarker.test_the_flag_is_not_saved` | **mapped elsewhere**: [handed to 6.d] `interrupted_mid_search` is not a saved field and does not count as a change (`phase6a-core-support.md:146`) | `crates/hatate-core/src/model.rs:636` | - |
| 436 | `test_interrupted_banner.py` `TestRowMarker.test_flagging_does_not_make_an_entry_look_changed` | **mapped elsewhere**: [handed to 6.d] `interrupted_mid_search` is not a saved field and does not count as a change (`phase6a-core-support.md:146`) | `crates/hatate-core/src/model.rs:636` | - |
| 442 | `test_interrupted_banner.py` `TestRowMarker.test_the_search_worker_spends_the_flag` | **mapped elsewhere**: [handed to 6.d] the marker shows only while the row is unsearched, and a search spends the flag (`batch.md:112`) | `crates/hatate-core/tests/batch_run.rs:1046`, `crates/hatate-core/src/table_cells.rs:655` | - |
| 70 | `test_sent_tag_filter.py` `TestTheHelper.test_no_settings_at_all_means_no_filtering` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 75 | `test_sent_tag_filter.py` `TestTheHelper.test_off_sends_everything_even_when_sources_are_unticked` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 79 | `test_sent_tag_filter.py` `TestTheHelper.test_on_honours_the_ticked_sources` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 83 | `test_sent_tag_filter.py` `TestTheHelper.test_on_with_nothing_ticked_sends_nothing` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 89 | `test_sent_tag_filter.py` `TestTheHelper.test_a_stale_source_name_is_ignored_not_fatal` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 95 | `test_sent_tag_filter.py` `TestTheHelper.test_the_returned_list_is_not_the_entrys_own` | **mapped elsewhere**: [handed to 6.d] `TestTheHelper` is `the_tag_filter_is_off_by_default_and_literal_when_on` (`docs/hydrus-import.md:173`; the last test does not apply: `tags_to_send` returns references) | `crates/hatate-core/tests/hydrus_import.rs:1020` | - |
| 156 | `test_sent_tag_filter.py` `TestTheThreeSends.test_off_by_default_every_send_still_carries_every_tag` | **mapped elsewhere**: [handed to 6.d] `TestTheThreeSends` is `the_source_filter_decides_which_tags_are_sent_only_when_it_is_on` for the upload; the other two sends are 4.d (`docs/hydrus-import.md:175`) | `docs/hydrus-import.md:175` | - |
| 162 | `test_sent_tag_filter.py` `TestTheThreeSends.test_off_ignores_unticked_sources` | **mapped elsewhere**: [handed to 6.d] `TestTheThreeSends` is `the_source_filter_decides_which_tags_are_sent_only_when_it_is_on` for the upload; the other two sends are 4.d (`docs/hydrus-import.md:175`) | `docs/hydrus-import.md:175` | - |
| 171 | `test_sent_tag_filter.py` `TestTheThreeSends.test_on_every_send_drops_the_unticked_sources` | **mapped elsewhere**: [handed to 6.d] `TestTheThreeSends` is `the_source_filter_decides_which_tags_are_sent_only_when_it_is_on` for the upload; the other two sends are 4.d (`docs/hydrus-import.md:175`) | `docs/hydrus-import.md:175` | - |
| 179 | `test_sent_tag_filter.py` `TestTheThreeSends.test_no_settings_argument_still_sends_everything` | **mapped elsewhere**: [handed to 6.d] `TestTheThreeSends` is `the_source_filter_decides_which_tags_are_sent_only_when_it_is_on` for the upload; the other two sends are 4.d (`docs/hydrus-import.md:175`) | `docs/hydrus-import.md:175` | - |
| 197 | `test_sent_tag_filter.py` `TestTheExport.test_off_by_default_the_export_carries_every_tag` | **mapped elsewhere**: [handed to 6.d] `TestTheExport` is `export::tests::the_tags_are_the_ones_a_send_would_carry` and the differential `export (settings variants)` (`docs/phase6b-pure-core.md:299`) | `crates/hatate-core/src/export.rs:482` | - |
| 204 | `test_sent_tag_filter.py` `TestTheExport.test_on_the_export_drops_the_unticked_sources` | **mapped elsewhere**: [handed to 6.d] `TestTheExport` is `export::tests::the_tags_are_the_ones_a_send_would_carry` and the differential `export (settings variants)` (`docs/phase6b-pure-core.md:299`) | `crates/hatate-core/src/export.rs:482` | - |
| 214 | `test_sent_tag_filter.py` `TestTheExport.test_tag_count_matches_the_tags_actually_written` | **mapped elsewhere**: [handed to 6.d] `TestTheExport` is `export::tests::the_tags_are_the_ones_a_send_would_carry` and the differential `export (settings variants)` (`docs/phase6b-pure-core.md:299`) | `crates/hatate-core/src/export.rs:482` | - |
| 229 | `test_sent_tag_filter.py` `TestTheExport.test_render_passes_the_settings_through` | **mapped elsewhere**: [handed to 6.d] `TestTheExport` is `export::tests::the_tags_are_the_ones_a_send_would_carry` and the differential `export (settings variants)` (`docs/phase6b-pure-core.md:299`) | `crates/hatate-core/src/export.rs:482` | - |
| 237 | `test_sent_tag_filter.py` `TestTheExport.test_the_column_order_is_unchanged` | **mapped elsewhere**: [handed to 6.d] `TestTheExport` is `export::tests::the_tags_are_the_ones_a_send_would_carry` and the differential `export (settings variants)` (`docs/phase6b-pure-core.md:299`) | `crates/hatate-core/src/export.rs:482` | - |
| 260 | `test_sent_tag_filter.py` `TestTheMigration.test_a_v12_config_with_sources_unticked_does_not_start_filtering` | **mapped elsewhere**: [handed to 6.d] the config migration is Phase 1: `rule2_migration_13_pins_filter_sent_tags_off_only_below_13` (`docs/hydrus-import.md:176`) | `crates/hatate-core/tests/config_compat.rs:470` | - |
| 270 | `test_sent_tag_filter.py` `TestTheMigration.test_a_pre_schema_config_is_treated_the_same` | **mapped elsewhere**: [handed to 6.d] the config migration is Phase 1: `rule2_migration_13_pins_filter_sent_tags_off_only_below_13` (`docs/hydrus-import.md:176`) | `crates/hatate-core/tests/config_compat.rs:470` | - |
| 275 | `test_sent_tag_filter.py` `TestTheMigration.test_an_explicit_opt_in_survives_a_reload` | **mapped elsewhere**: [handed to 6.d] the config migration is Phase 1: `rule2_migration_13_pins_filter_sent_tags_off_only_below_13` (`docs/hydrus-import.md:176`) | `crates/hatate-core/tests/config_compat.rs:470` | - |
| 285 | `test_sent_tag_filter.py` `TestTheMigration.test_the_schema_version_was_bumped_for_this` | **mapped elsewhere**: [handed to 6.d] the config migration is Phase 1: `rule2_migration_13_pins_filter_sent_tags_off_only_below_13` (`docs/hydrus-import.md:176`) | `crates/hatate-core/tests/config_compat.rs:470` | - |
| 289 | `test_sent_tag_filter.py` `TestTheMigration.test_a_fresh_install_defaults_to_off` | **mapped elsewhere**: [handed to 6.d] the config migration is Phase 1: `rule2_migration_13_pins_filter_sent_tags_off_only_below_13` (`docs/hydrus-import.md:176`) | `crates/hatate-core/tests/config_compat.rs:470` | - |
| 306 | `test_sent_tag_filter.py` `TestTheDialogCopy.test_the_two_meanings_are_worded_separately` | **mapped elsewhere**: [handed to 6.d] owned by 6.h, whose doc maps them at `docs/gtk-6h.md:144` to `docs/gtk-6h.md:146` (not asserted / the action differential's `s_set`) | `docs/gtk-6h.md:144` | - |
| 323 | `test_sent_tag_filter.py` `TestTheDialogCopy.test_the_switch_round_trips_through_apply_to_settings` | **mapped elsewhere**: [handed to 6.d] owned by 6.h, whose doc maps them at `docs/gtk-6h.md:144` to `docs/gtk-6h.md:146` (not asserted / the action differential's `s_set`) | `docs/gtk-6h.md:144` | - |
| 339 | `test_sent_tag_filter.py` `TestTheDialogCopy.test_an_unchecked_switch_shows_as_unchecked` | **mapped elsewhere**: [handed to 6.d] owned by 6.h, whose doc maps them at `docs/gtk-6h.md:144` to `docs/gtk-6h.md:146` (not asserted / the action differential's `s_set`) | `docs/gtk-6h.md:144` | - |
| 475 | `test_theme.py` `TestTableItemsAreNeverStyled.test_the_sheet_has_no_table_item_rule` | **n/a**: [handed to 6.d] Qt stylesheet rule on `QTableView::item`; GTK draws the cells from `tokens.css` and no selector styles a cell's item (`theme.rs`) | - | - |
| 532 | `test_theme.py` `TestEngineChipsCarryNoHue.test_the_dead_chip_and_danger_styles_are_gone_from_the_sheet` | **mapped elsewhere**: [handed to 6.d] the engine chips are text, clear 4.5:1, and the sheet holds no fill for them | `crates/hatate-gtk/src/theme.rs:855` | - |
| 544 | `test_theme.py` `TestEngineChipsCarryNoHue.test_engine_chips_render_with_no_background_fill` | **mapped elsewhere**: [handed to 6.d] the engine chips are text, clear 4.5:1, and the sheet holds no fill for them | `crates/hatate-gtk/src/theme.rs:855` | - |
| 272 | `test_gui_smoke.py` `TestMainWindowSignalHandlers.test_search_progress_handlers` | **mapped elsewhere**: [handed to 6.d] `eta::run_strip` fed by the run, and `start_search_runs_the_batch` asserts the count and the idle ETA after the finish (`docs/gtk-6e.md:346` to `:348`) | `crates/hatate-gtk/tests/smoke.rs:1985` | - |
| 282 | `test_gui_smoke.py` `TestMainWindowSignalHandlers.test_progress_label_shows_the_count_then_the_estimate` | **mapped elsewhere**: [handed to 6.d] `eta::run_strip` fed by the run, and `start_search_runs_the_batch` asserts the count and the idle ETA after the finish (`docs/gtk-6e.md:346` to `:348`) | `crates/hatate-gtk/tests/smoke.rs:1985` | - |
| 303 | `test_gui_smoke.py` `TestMainWindowSignalHandlers.test_a_finished_run_keeps_the_count_and_drops_the_estimate` | **mapped elsewhere**: [handed to 6.d] `eta::run_strip` fed by the run, and `start_search_runs_the_batch` asserts the count and the idle ETA after the finish (`docs/gtk-6e.md:346` to `:348`) | `crates/hatate-gtk/tests/smoke.rs:1985` | - |
| 0 | `test_thumbnail_range_is_clamped_to_the_visible_rows.py` `(file)` | **scan false positive**: [handed to 6.d] `docs/phase6a-core-support.md:154` misspells the name; the Python test is `test_the_thumbnail_range_is_clamped_to_the_visible_rows` (`test_gui_smoke.py:3554`) and 6.d maps it at `docs/queue-view.md:284` (not separately asserted) | - | - |
| 3829 | `test_gui_smoke.py` `TestPreviewClearsWhenRowsAreRemoved.test_an_auto_import_removal_leaves_the_watched_row_alone` | **not asserted**: [handed to 6.e] `docs/gtk-6f.md:412`: no scenario removes a row other than the watched one with Review open; the removal path is `queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row` | - | - |
| 328 | `test_gui_smoke.py` `TestMainWindowSignalHandlers.test_auto_import_handler` | **mapped elsewhere**: [handed to 6.e] owned by 6.g: `auto_import.rs:389` to `:393` decide the three results (`docs/gtk-6g.md:317`) | `docs/gtk-6g.md:317` | - |
| 1816 | `test_gui_smoke.py` `TestTagsAppearWhenAMatchLands.test_tags_show_without_reselecting_the_row` | **mapped elsewhere**: [handed to 6.f] the row carries the worker's copy as the search lands (`on_worker_image_updated`), the Review tag panel follows the selection, and the update-selects-the-row class is class 1 of the action differential (`docs/gtk-6e.md:357` to `:359`) | `crates/hatate-gtk/tests/smoke.rs:4662`, `crates/hatate-gtk/tests/smoke.rs:4732` | - |
| 1832 | `test_gui_smoke.py` `TestTagsAppearWhenAMatchLands.test_a_re_search_clears_the_previous_tags` | **mapped elsewhere**: [handed to 6.f] the row carries the worker's copy as the search lands (`on_worker_image_updated`), the Review tag panel follows the selection, and the update-selects-the-row class is class 1 of the action differential (`docs/gtk-6e.md:357` to `:359`) | `crates/hatate-gtk/tests/smoke.rs:4662`, `crates/hatate-gtk/tests/smoke.rs:4732` | - |
| 1846 | `test_gui_smoke.py` `TestTagsAppearWhenAMatchLands.test_an_update_for_another_row_still_follows_the_search` | **mapped elsewhere**: [handed to 6.f] the row carries the worker's copy as the search lands (`on_worker_image_updated`), the Review tag panel follows the selection, and the update-selects-the-row class is class 1 of the action differential (`docs/gtk-6e.md:357` to `:359`) | `crates/hatate-gtk/tests/smoke.rs:4662`, `crates/hatate-gtk/tests/smoke.rs:4732` | - |
| 5774 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_marking_updates_the_rows_and_the_count` | **mapped elsewhere**: [handed to 6.f] marking updates the rows and the strip's `to review` figure, quiet until a row is reviewed | `crates/hatate-gtk/tests/smoke.rs:647` | - |
| 5785 | `test_gui_smoke.py` `TestReviewedInTheContextMenu.test_the_count_stays_quiet_until_something_is_reviewed` | **mapped elsewhere**: [handed to 6.f] marking updates the rows and the strip's `to review` figure, quiet until a row is reviewed | `crates/hatate-gtk/tests/smoke.rs:647` | - |
| 6489 | `test_gui_smoke.py` `TestShowInHydrus.test_h_shows_the_selection` | **mapped elsewhere**: [handed to 6.f] the H key reaches the Hydrus show handler, and without a selection the window asks for one before anything goes out | `crates/hatate-gtk/tests/smoke.rs:7223`, `crates/hatate-gtk/tests/smoke.rs:6205` | - |
| 6504 | `test_gui_smoke.py` `TestShowInHydrus.test_h_with_nothing_selected_says_so_rather_than_acting` | **mapped elsewhere**: [handed to 6.f] the H key reaches the Hydrus show handler, and without a selection the window asks for one before anything goes out | `crates/hatate-gtk/tests/smoke.rs:7223`, `crates/hatate-gtk/tests/smoke.rs:6205` | - |
| 5403 | `test_gui_smoke.py` `TestReviewShortcuts.test_rebinding_takes_effect_without_a_restart` | **not asserted**: [handed to 6.h] the Shortcuts page records, clears, resets, refuses a conflict and saves only what differs (`settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict`), and the Review page reads `resolved_shortcuts()` (`crates/hatate-gtk/src/review_ops.rs:221`) when it builds its hints and key caps; no test rebinds a key and then reads the page's cap, button label or hint bar; making `resolved_shortcuts` ignore the stored bindings leaves the whole smoke suite green. Finding F-B6 | - | `n_rebindhints`: green |
| 5413 | `test_gui_smoke.py` `TestReviewShortcuts.test_reinstalling_does_not_leave_the_old_binding_behind` | **not asserted**: [handed to 6.h] the Shortcuts page records, clears, resets, refuses a conflict and saves only what differs (`settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict`), and the Review page reads `resolved_shortcuts()` (`crates/hatate-gtk/src/review_ops.rs:221`) when it builds its hints and key caps; no test rebinds a key and then reads the page's cap, button label or hint bar; making `resolved_shortcuts` ignore the stored bindings leaves the whole smoke suite green. Finding F-B6 | - | `n_rebindhints`: green |
| 5489 | `test_gui_smoke.py` `TestDynamicShortcutLabels.test_rebinding_updates_button_labels_and_strip` | **not asserted**: [handed to 6.h] the Shortcuts page records, clears, resets, refuses a conflict and saves only what differs (`settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict`), and the Review page reads `resolved_shortcuts()` (`crates/hatate-gtk/src/review_ops.rs:221`) when it builds its hints and key caps; no test rebinds a key and then reads the page's cap, button label or hint bar; making `resolved_shortcuts` ignore the stored bindings leaves the whole smoke suite green. Finding F-B6 | - | `n_rebindhints`: green |
| 214 | `test_review_keycaps.py` `TestKeysInsideTheButtons.test_rebinding_a_key_changes_the_cap_and_unbinding_hides_it` | **not asserted**: [handed to 6.h] the Shortcuts page records, clears, resets, refuses a conflict and saves only what differs (`settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict`), and the Review page reads `resolved_shortcuts()` (`crates/hatate-gtk/src/review_ops.rs:221`) when it builds its hints and key caps; no test rebinds a key and then reads the page's cap, button label or hint bar; making `resolved_shortcuts` ignore the stored bindings leaves the whole smoke suite green. Finding F-B6 | - | `n_rebindhints`: green |
| 313 | `test_review_keycaps.py` `TestHintBar.test_a_rebound_key_is_what_the_bar_shows` | **not asserted**: [handed to 6.h] the Shortcuts page records, clears, resets, refuses a conflict and saves only what differs (`settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict`), and the Review page reads `resolved_shortcuts()` (`crates/hatate-gtk/src/review_ops.rs:221`) when it builds its hints and key caps; no test rebinds a key and then reads the page's cap, button label or hint bar; making `resolved_shortcuts` ignore the stored bindings leaves the whole smoke suite green. Finding F-B6 | - | `n_rebindhints`: green |

### 6.2 The tests of ten other GUI-importing files (DAN-2452)

`testunion.py --gui-importers` lists the test files that import `gui` and finds 86 tests in ten files that the Phase 6 ticket's list does not name and no earlier doc maps. Cloud's ruling on [DAN-2452](/DAN/issues/DAN-2452) (option 2): they are listed here, one class per file; they are not mapped in 6.j. A file that makes a behaviour claim is classed **behaviour: unmapped, Phase 7**; a file that is paint, geometry or type voice is classed **design: shot set** (judged on the shots and the contrast table, never asserted). `crates/hatate-gtk/tests/phase6_coverage.rs` holds the list exactly (`docs/phase6-gui-tests-outside-list-76c85c4.txt`): it fails on a missing id and on an extra one.

| File | Tests | Class | Why | Test ids |
|---|---|---|---|---|
| `test_app_chrome.py` | 9 | **design: shot set** | Qt geometry and paint of the page gutter, menu bar, status bar and candidate picker. GTK sets them from `tokens.css`; the shot set shows them (`empty-*`, `rows-*`, `review-*`) and run C's contrast table measures the status bar text (4.5:1). | `TestPageGutter.test_every_mode_starts_and_ends_32px_in_from_the_window_edge`, `TestPageRhythm.test_top_of_page_edges_are_within_2px_of_the_mockup`, `TestMenuBar.test_menubar_is_28px_with_13px_text_and_a_hairline`, `TestMenuBar.test_first_menu_starts_on_the_page_gutter`, `TestStatusBar.test_geometry_and_voice`, `TestStatusBar.test_top_rule_is_painted`, `TestStatusBar.test_text_clears_4_5_to_1_against_its_background`, `TestCandidatePicker.test_long_match_is_ellipsised_in_mono_with_a_chevron`, `TestCandidatePicker.test_short_text_is_not_ellipsised` |
| `test_flat_surfaces.py` | 6 | **design: shot set** | Qt paint of the flat hairline surfaces and the numbered section labels. The shot set shows them (`activity-*`, `settings-*`) and the contrast table measures the section label text. | `TestRegionsAreFlatHairlines.test_activity_sections_paint_the_page`, `TestRegionsAreFlatHairlines.test_section_rule_is_one_pixel_of_hairline`, `TestNumberedSectionLabels.test_every_labelled_section_carries_its_number`, `TestNumberedSectionLabels.test_the_serif_headings_and_filter_colon_are_gone`, `TestNumberedSectionLabels.test_uppercase_comes_from_the_font_not_the_string`, `TestNumberedSectionLabels.test_section_label_text_clears_body_contrast_on_the_page` |
| `test_focus_indicators.py` | 5 | **design: shot set** | that the focus ring is visible on the queue and Review pages in both schemes: the focus shots and the dark focus-ring row of the contrast table, with the Tab walk of section 8. The one behaviour claim, `test_review_page_container_is_not_a_tab_stop`, has the Rust test `no_clipping_wrapper_on_the_review_page_is_a_tab_stop` (`crates/hatate-gtk/tests/smoke.rs`). | `TestFocusIsVisible.test_queue_dark`, `TestFocusIsVisible.test_queue_light`, `TestFocusIsVisible.test_review_dark`, `TestFocusIsVisible.test_review_light`, `TestFocusIsVisible.test_review_page_container_is_not_a_tab_stop` |
| `test_queue_view.py` | 1 | **design: shot set** | the zebra stripe is quiet but visible: the `rows-*` shots and the contrast table (row against page). | `TestQueueTableChrome.test_the_zebra_stripe_is_quiet_but_visible` |
| `test_run_strip.py` | 7 | **design: shot set** | the strip's gauge track (3:1), fill, 220 px hairline, label order and text floors: the strip on the `rows-*` shots and the contrast table. | `TestStripTrackIsVisible.test_track_edge_clears_3_to_1_against_the_strip`, `TestStripTrackIsVisible.test_fill_is_distinct_from_the_track`, `TestStripTrackIsVisible.test_the_track_token_itself_clears_the_floor`, `TestStripGeometry.test_gauge_is_a_220px_two_pixel_hairline`, `TestStripGeometry.test_run_label_is_leftmost_then_gauge`, `TestStripVoice.test_label_text_clears_the_body_text_floor`, `TestStripVoice.test_readout_text_and_figures_lose_nothing` |
| `test_selected_row_contrast.py` | 2 | **design: shot set** | the selected row's text clears its floor in both schemes: the `rows-*` shots (a selected row) and the contrast table. The real compositor backdrop of an inactive selection is not judgeable from shots (design acceptance line 6). | `TestSelectedRowStaysLegible.test_dark`, `TestSelectedRowStaysLegible.test_light` |
| `test_type_voice.py` | 26 | **design: shot set** | type voice (face, size, case, tracking) of the wordmark, meta line, gear, titles, Activity chips and log toolbar. The renderer draws the real window with the bundled faces; the design read compares the shots with the mockup. | `_WordmarkChecks.test_kanji_mark_is_ryoku_kanji_at_22px`, `_WordmarkChecks.test_word_is_mono_uppercase_tracked_but_text_stays_sentence_case`, `_WordmarkChecks.test_word_is_11px_the_mockups_text_label`, `_WordmarkChecks.test_mark_keeps_its_accessible_name`, `_UppercaseChecks.test_activity_page_buttons`, `_MetaLineChecks.test_meta_line_is_10px_mono`, `_GearChecks.test_gear_is_a_36px_square`, `_GearChecks.test_gear_edge_is_painted`, `_TitleChecks.test_titles_are_source_serif_36px`, `_ActivityChecks.test_engine_chips_are_uppercase_mono_and_keep_their_sentence_case_text`, `_ActivityChecks.test_engine_chips_are_33px_tall_with_an_8px_gap`, `_ActivityChecks.test_on_chips_are_bold_and_off_chips_regular`, `_ActivityChecks.test_vertical_rhythm_follows_the_mockups_spacing_tokens`, `_ActivityChecks.test_pipeline_line_sits_below_the_chips_in_mono_with_bold_names`, `_ActivityChecks.test_show_in_queue_is_a_labelled_boxed_36px_button`, `_ActivityChecks.test_log_glyphs_are_sized_to_share_one_visual_size`, `_ActivityChecks.test_chips_wrap_instead_of_widening_the_window`, `_ActivityChecks.test_log_toolbar_is_three_boxed_36px_buttons`, `_ActivityChecks.test_log_toolbar_sits_right_of_the_section_rule`, `_ActivityChecks.test_log_footer_is_10px_mono`, `_ActivityChecks.test_activity_has_a_title_in_queues_slot`, `TestBareWidgets.test_the_kanji_glyph_exists_in_the_bundled_face`, `TestBareWidgets.test_uppercase_is_painted_not_typed`, `TestBareWidgets.test_other_icon_buttons_stay_borderless_and_small`, `TestBareWidgets.test_stylesheet_rule_for_titles_is_36px`, `TestBareWidgets.test_opt_out_leaves_the_font_alone` |
| `test_tristate_restore.py` | 5 | **behaviour: unmapped, Phase 7** | the restore message for clean, unclean, empty and restored starts. The Rust window has the same cases under other names (`restore_clean_session`, `restore_after_a_crash_shows_the_banner`, `nothing_survived_variant`), but no row maps these five by name, and no claim is made that they do. | `TestTristateRestoreMessage.test_clean_start_with_nothing_ever_saved_is_silent`, `TestTristateRestoreMessage.test_unclean_shutdown_but_no_session_file_is_still_silent`, `TestTristateRestoreMessage.test_clean_shutdown_with_an_empty_session_file_is_silent`, `TestTristateRestoreMessage.test_unclean_shutdown_with_a_session_file_but_nothing_restored`, `TestTristateRestoreMessage.test_entries_do_restore_even_when_unclean` |
| `test_unclean_shutdown_wiring.py` | 3 | **behaviour: unmapped, Phase 7** | the unclean-shutdown flag is stored and a close clears the running marker. Rust has `close_saves_and_marks_a_clean_exit` and the `crash_marker` tests (Phase 2), unmapped by name here. | `TestUncleanShutdownFlagIsStored.test_defaults_to_clean`, `TestUncleanShutdownFlagIsStored.test_records_an_unclean_shutdown_when_told`, `TestCloseEventMarksCleanShutdown.test_close_event_clears_the_running_marker` |
| `test_mcp_bridge.py` | 22 | **behaviour: unmapped, Phase 7** | the Qt-side MCP bridge (main-thread invoker, read tools, local writes, gating and audit, dry run). The Rust MCP controller and handlers are Phase 5 (`mcp_tool_handlers.rs`) and the window's side is 6.i's `mcp_server_starts_with_the_window_answers_over_http_and_stops_on_close` and `mcp_tool_calls_change_the_list_and_the_table_follows`; these 22 are not mapped by name. | `TestMainThreadInvoker.test_runs_the_callable_on_the_thread_that_constructed_it`, `TestMainThreadInvoker.test_returns_the_callables_result`, `TestMainThreadInvoker.test_an_exception_in_the_callable_is_raised_in_the_caller`, `TestMainThreadInvoker.test_a_same_thread_call_runs_directly`, `TestReadTools.test_list_queue_reports_every_row_by_default`, `TestReadTools.test_list_queue_respects_offset_and_limit`, `TestReadTools.test_list_queue_filter_keyword_unreviewed`, `TestReadTools.test_get_entry_reports_the_requested_row`, `TestReadTools.test_get_entry_out_of_range_reports_an_error_not_a_crash`, `TestReadTools.test_list_actions_reports_every_tool_with_its_gate_state`, `TestReadTools.test_list_actions_reports_hydrus_writes_enabled_by_default`, `TestLocalWrites.test_select_candidate_switches_the_active_match`, `TestLocalWrites.test_toggle_reviewed_sets_the_flag`, `TestLocalWrites.test_local_writes_are_never_gated`, `TestGatingAndAudit.test_a_disabled_tier_is_refused_with_the_setting_named`, `TestGatingAndAudit.test_a_hydrus_write_is_refused_without_calling_hydrus_at_all`, `TestGatingAndAudit.test_a_refusal_is_still_recorded_in_the_audit_log`, `TestGatingAndAudit.test_an_allowed_call_is_recorded_too`, `TestDryRun.test_dry_run_changes_nothing_and_says_so`, `TestDryRun.test_dry_run_is_still_audited`, `TestDryRun.test_turning_dry_run_off_performs_the_real_action`, `TestDryRun.test_read_tools_ignore_dry_run` |

Totals: design: shot set: 56, behaviour: unmapped, Phase 7: 30, 86 in all.

## 7. The feature checklist, screen by screen

Every menu item, context-menu item, shortcut, dialog, message box, chooser, tooltip and placeholder of the Python window is listed with the action-differential `op` that drives it, the smoke scenario that asserts it, or the shot that shows it. Section 7.1 is the states, banners and dialogs (the renderer's `SHOT_SET`); 7.2 is the 226 candidates of an `ast` walk of `gui/` at `76c85c4` (`worktrees/DAN-2223-tools/inv.py`: 21 `QAction`, 11 menus, 4 context actions, 2 shortcuts, 136 message-box and logger calls, 10 choosers, 24 tooltips, 18 placeholders), each with the Rust site of its text where the string occurs. The ops are those of `crates/hatate-gtk/tests/differential.rs`; the case counts and the logs that ran them are in the docs of the tickets that own them (`docs/queue-view.md` section 5, `docs/gtk-6e.md`, `docs/gtk-6f.md`, `docs/gtk-6g.md`, `docs/gtk-6h.md`, `docs/gtk-6h2.md`, `docs/gtk-6i.md`), and `worktrees/DAN-22*-differential*.log`. `crates/hatate-gtk/tests/phase6_coverage.rs` holds the list exactly (`docs/phase6-feature-inventory-76c85c4.txt`) and checks every op, shot family and Rust path a row names.

### 7.1 States, banners and dialogs

Every state the renderer draws is a row of its `SHOT_SET` table (`crates/hatate-gtk/examples/shot.rs`, 95 states, dark and light, at 1100x800 and where the table says at the minimum size). This table gives each surface its Python origin and the shot families and scenario that hold it.

| # | Surface | Python | Shots | Asserted by |
|---|---|---|---|---|
| 1 | The empty queue (drop zone) | `_build_drop_zone`, `_refresh_queue_empty_state` | `empty-*` | `crates/hatate-gtk/tests/smoke.rs:611` |
| 2 | The queue: rows, selected, sorted, filtered, filter with no match, row menu, remove | `ImageTableModel`, `FilterBar`, `TableContextMenu` | `rows-*`, `queue-*`, `queue-selected-*`, `queue-sorted-*`, `queue-filter-*`, `queue-filter-empty-*`, `queue-menu-*`, `queue-remove-*` | `crates/hatate-gtk/tests/smoke.rs:3382` |
| 3 | The interrupted banner (a crashed or stopped run) | `_show_run_banner` | `crashed-*` | `crates/hatate-gtk/tests/smoke.rs:865` |
| 4 | The Nothing survived page | `_show_nothing_survived` | `survived-*` | `crates/hatate-gtk/tests/smoke.rs:958` |
| 5 | The quota-pause banner and Continue | `_show_quota_banner` | `quota-*` | `crates/hatate-gtk/tests/smoke.rs:2140` |
| 6 | The failed-run banner | `_show_failed_banner` | `failed-*` | `crates/hatate-gtk/tests/smoke.rs:2436` |
| 7 | The missing-files dialog and the other message boxes | `_on_missing_files_checked`, `show_message` | `dialog-*`, `dialog-nolabel-*` | `crates/hatate-gtk/tests/smoke.rs:1134` |
| 8 | The log viewer | `LogViewerDialog` | `logs-*`, `logs-selected-*`, `logs-selected-body-*` | `crates/hatate-gtk/tests/smoke.rs:1582` |
| 9 | The Activity page | `ActivityView` | `activity-*` | `crates/hatate-gtk/tests/smoke.rs:2865` |
| 10 | Parser health | `ParserHealthDialog` | `parser-health-*`, `parser-health-kb-*` | `crates/hatate-gtk/tests/smoke.rs:2696` |
| 11 | The Review page: matched, empty, no match, candidate picker, Add tags, match conditions | `ReviewView` | `review-*`, `review-empty-*`, `review-nomatch-*`, `review-picker-*`, `review-addtags-*`, `review-conditions-*` | `crates/hatate-gtk/tests/smoke.rs:4662` |
| 12 | Wipe, Differences and their error state | `_build_wipe_view`, `build_difference_overlay` | `review-wipe-*`, `review-differences-*`, `review-wipe-nomatch-*`, `review-error-*` | `crates/hatate-gtk/tests/smoke.rs:5766` |
| 13 | The compare dialog | `CompareDialog` | `review-compare-*`, `review-compare-differences-*`, `review-compare-nomatch-*` | `crates/hatate-gtk/tests/smoke.rs:5891` |
| 14 | Hydrus: empty selection, delete and its confirm, Query Hydrus and its results | `_delete_rows_from_hydrus`, `HydrusQueryDialog` | `hydrus-empty-*`, `hydrus-delete-*`, `hydrus-delete-confirm-*`, `hydrus-query-*`, `hydrus-query-results-*` | `crates/hatate-gtk/tests/smoke.rs:6250` |
| 15 | The Pawchive index window, populated | `PawchiveIndexDialog` | `pawchive-*`, `pawchive-artists-*` | `crates/hatate-gtk/tests/smoke.rs:6934` |
| 16 | Settings: nine tabs, the find box with and without a hit, the Lens box, the MCP page (running, held, audit, tiers) and the Shortcuts page (record, conflict) | `SettingsDialog`, `settings_search` | `settings-0-*`, `settings-8-*`, `settings-find-*`, `settings-find-none-*`, `settings-found-*`, `settings-lens-*`, `settings-lens-box-*`, `settings-mcp-running-*`, `settings-mcp-held-*`, `settings-mcp-audit-*`, `settings-mcp-tiers-*`, `settings-shortcuts-conflict-*`, `settings-shortcuts-record-*` | `crates/hatate-gtk/tests/smoke.rs:7648` |
| 17 | The open menus | `_build_menu` | `menu-*`, `menu-hover-*` | `crates/hatate-gtk/tests/smoke.rs:4443` |
| 18 | Focus rings of every focusable control on each page | (Qt focus frames) | `focus-*`, `focus-files-*`, `focus-folder-*`, `hover-folder-*`, `queue-focus-*`, `queue-focus-selected-*`, `review-focus-combo-*`, `review-focus-mark-*`, `review-focus-next-*`, `settings-focus-ok-*` | `crates/hatate-gtk/tests/smoke.rs:5154` |


Nothing in 7.2 is a disabled placeholder (`no_menu_action_is_deferred`). The only rows that are not covered by a test or a shot are the **by design** ones, each with its reason.

### 7.2 The 226 candidates

| # | Python | Kind | Text | Rust text | Coverage |
|---|---|---|---|---|---|
| 1 | `gui/compare_dialog.py:486` | message | Could not load local image for comparison: %s | `crates/hatate-gtk/src/review_compare.rs:89` | **smoke**: the dialog loads the pair; an unloadable local file shows the error state (`review-error-dark.png`/`review-error-light.png`). `crates/hatate-gtk/tests/smoke.rs:5891`, `crates/hatate-gtk/tests/smoke.rs:5594` |
| 2 | `gui/compare_dialog.py:566` | tooltip | Scroll to zoom where the pointer is, drag to pan, double-click to fit. +... | `crates/hatate-gtk/src/wipe_view.rs:97` | **op**: the dialog's controls and tooltips: `dlg_mode` op, `dlg_slider` op, `dlg_zoom` op, `dlg_close` op; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5891` |
| 3 | `gui/compare_dialog.py:604` | tooltip | Zoom out (-) | `crates/hatate-gtk/src/compare_dialog.rs:164` | **op**: the dialog's controls and tooltips: `dlg_mode` op, `dlg_slider` op, `dlg_zoom` op, `dlg_close` op; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5891` |
| 4 | `gui/compare_dialog.py:609` | tooltip | Zoom in (+) | `crates/hatate-gtk/src/compare_dialog.rs:165` | **op**: the dialog's controls and tooltips: `dlg_mode` op, `dlg_slider` op, `dlg_zoom` op, `dlg_close` op; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5891` |
| 5 | `gui/compare_dialog.py:613` | tooltip | Fit the whole image in the window (0, or double-click) | `crates/hatate-gtk/src/compare_dialog.rs:168` | **op**: the dialog's controls and tooltips: `dlg_mode` op, `dlg_slider` op, `dlg_zoom` op, `dlg_close` op; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5891` |
| 6 | `gui/compare_dialog.py:426` | message | Could not build the difference view: %s | - | **by design**: the Python box carries the OS or Qt text; Rust shows the error state with `user_message()` (DAN-2116): `review-error-dark.png`/`review-error-light.png`. `crates/hatate-gtk/tests/smoke.rs:5594` |
| 7 | `gui/compare_dialog.py:502` | message | Fetching details for the selected match before comparing: %s | `crates/hatate-gtk/src/review_compare.rs:100` | **smoke**: the dialog loads the pair; an unloadable local file shows the error state (`review-error-dark.png`/`review-error-light.png`). `crates/hatate-gtk/tests/smoke.rs:5891`, `crates/hatate-gtk/tests/smoke.rs:5594` |
| 8 | `gui/compare_dialog.py:520` | message | Fetching match at full resolution for comparison: %s | `crates/hatate-gtk/src/review_compare.rs:123` | **smoke**: the dialog loads the pair; an unloadable local file shows the error state (`review-error-dark.png`/`review-error-light.png`). `crates/hatate-gtk/tests/smoke.rs:5891`, `crates/hatate-gtk/tests/smoke.rs:5594` |
| 9 | `gui/compare_dialog.py:530` | message | Fetched %d bytes for the match but Qt couldn't decode it | - | **by design**: the Python box carries the OS or Qt text; Rust shows the error state with `user_message()` (DAN-2116): `review-error-dark.png`/`review-error-light.png`. `crates/hatate-gtk/tests/smoke.rs:5594` |
| 10 | `gui/compare_dialog.py:507` | message | Could not fetch match details for %s: %s | `crates/hatate-core/src/mcp_images.rs:66` | **smoke**: the dialog loads the pair; an unloadable local file shows the error state (`review-error-dark.png`/`review-error-light.png`). `crates/hatate-gtk/tests/smoke.rs:5891`, `crates/hatate-gtk/tests/smoke.rs:5594` |
| 11 | `gui/filter_bar.py:103` | placeholder | Filename contains… | `crates/hatate-gtk/src/filter_bar.rs:87` | **op**: `filter_text` op, `filter_all` op, `filter_clear` op; the bar is `queue-filter-*`. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 12 | `gui/filter_bar.py:119` | tooltip | Show only rows with the statuses you tick. Only the statuses actually pr... | `crates/hatate-gtk/src/filter_bar.rs:26` | **op**: `filter_text` op, `filter_all` op, `filter_clear` op; the bar is `queue-filter-*`. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 13 | `gui/filter_bar.py:145` | tooltip | Show only rows with the Check for Upscaling verdict you tick. | `crates/hatate-gtk/src/filter_bar.rs:28` | **op**: `filter_text` op, `filter_all` op, `filter_clear` op; the bar is `queue-filter-*`. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 14 | `gui/filter_bar.py:152` | tooltip | Show everything again. | `crates/hatate-gtk/src/filter_bar.rs:101` | **op**: `filter_text` op, `filter_all` op, `filter_clear` op; the bar is `queue-filter-*`. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 15 | `gui/filter_bar.py:280` | context-action | All | `crates/hatate-core/src/cache/mod.rs:78` (asserted at `crates/hatate-core/tests/common/lens_results.rs:97`) | **op**: `filter_all` op, `filter_toggle` op. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 16 | `gui/filter_bar.py:281` | tooltip | Tick everything - the same as not filtering on this at all. | `crates/hatate-gtk/src/filter_bar.rs:29` | **op**: `filter_all` op, `filter_toggle` op. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 17 | `gui/filter_bar.py:283` | context-action | None | `crates/hatate-cli/src/args.rs:49` (asserted at `crates/hatate-core/tests/availability_https.rs:58`) | **op**: `filter_all` op, `filter_toggle` op. `crates/hatate-gtk/tests/smoke.rs:3427` |
| 18 | `gui/fonts.py:46` | message | Failed to register font: %s | - | **n/a**: a log line of the Python font registration; the Rust faces are loaded by `fonts.rs` and `fonts_cover_the_status_glyphs_and_kanji` asserts the glyphs. `crates/hatate-gtk/tests/smoke.rs:1620` |
| 19 | `gui/hydrus_query_dialog.py:65` | message | Querying Hydrus for tags: %s | `crates/hatate-gtk/src/hydrus_query.rs:268` | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 20 | `gui/hydrus_query_dialog.py:124` | message | Query Hydrus import complete: %d succeeded, %d duplicates skipped, %d fa... | `crates/hatate-gtk/src/hydrus_query.rs:472` | **smoke**: the window searches, lists and imports what the list does not hold. `crates/hatate-gtk/tests/smoke.rs:6480` |
| 21 | `gui/hydrus_query_dialog.py:63` | message | Query Hydrus / Enter at least one tag. | `crates/hatate-gtk/src/hydrus_query.rs:265` (asserted at `crates/hatate-gtk/tests/smoke.rs:6534`) | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 22 | `gui/hydrus_query_dialog.py:86` | message | Query Hydrus: no files matched %s | `crates/hatate-gtk/src/hydrus_query.rs:334` | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 23 | `gui/hydrus_query_dialog.py:87` | message | Query Hydrus / No files matched those tags. | `crates/hatate-gtk/src/hydrus_query.rs:335` (asserted at `crates/hatate-gtk/tests/smoke.rs:6538`) | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 24 | `gui/hydrus_query_dialog.py:92` | message | Query Hydrus / Select at least one file to import. | `crates/hatate-gtk/src/hydrus_query.rs:353` (asserted at `crates/hatate-gtk/tests/smoke.rs:6467`) | **smoke**: the window searches, lists and imports what the list does not hold. `crates/hatate-gtk/tests/smoke.rs:6480` |
| 25 | `gui/hydrus_query_dialog.py:71` | message | Query Hydrus search failed: %s | `crates/hatate-gtk/src/hydrus_query.rs:302` | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 26 | `gui/hydrus_query_dialog.py:72` | message | Query Hydrus | `crates/hatate-gtk/src/dialogs.rs:542` (asserted at `crates/hatate-gtk/tests/smoke.rs:618`) | **smoke**: the window asks for a tag and a key and says when nothing matched. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 27 | `gui/hydrus_query_dialog.py:112` | message | Failed to download Hydrus file %s: %s | `crates/hatate-gtk/src/hydrus_query.rs:415` | **smoke**: the window searches, lists and imports what the list does not hold. `crates/hatate-gtk/tests/smoke.rs:6480` |
| 28 | `gui/hydrus_query_dialog.py:113` | message | Query Hydrus | `crates/hatate-gtk/src/dialogs.rs:542` (asserted at `crates/hatate-gtk/tests/smoke.rs:618`) | **smoke**: the window searches, lists and imports what the list does not hold. `crates/hatate-gtk/tests/smoke.rs:6480` |
| 29 | `gui/main_window.py:575` | menu | &Files | `crates/hatate-core/src/batch/matched_urls.rs:2` (asserted at `crates/hatate-core/tests/hydrus_client_write.rs:616`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 30 | `gui/main_window.py:576` | action | Add Files… | `crates/hatate-gtk/src/menu.rs:90` (asserted at `crates/hatate-gtk/tests/smoke.rs:723`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 31 | `gui/main_window.py:580` | action | Add Folder… | `crates/hatate-core/src/batch/run.rs:2` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 32 | `gui/main_window.py:584` | action | Query Hydrus… | `crates/hatate-gtk/src/dialogs.rs:542` (asserted at `crates/hatate-gtk/tests/smoke.rs:618`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 33 | `gui/main_window.py:590` | action | Re-check Queued Imports | `crates/hatate-gtk/src/hydrus_ops.rs:353` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 34 | `gui/main_window.py:595` | action | Pawchive Index… | `crates/hatate-core/src/table_menu.rs:219` (asserted at `crates/hatate-gtk/tests/smoke.rs:6545`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 35 | `gui/main_window.py:605` | action | Save Session As… | `crates/hatate-core/src/session/mod.rs:9` (asserted at `crates/hatate-gtk/tests/smoke.rs:1193`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 36 | `gui/main_window.py:606` | shortcut | Ctrl+S | `crates/hatate-gtk/src/menu.rs:99` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 37 | `gui/main_window.py:615` | action | Open Session… | `crates/hatate-core/src/session/named.rs:34` (asserted at `crates/hatate-gtk/tests/smoke.rs:1248`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 38 | `gui/main_window.py:616` | shortcut | Ctrl+O | `crates/hatate-gtk/src/menu.rs:102` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 39 | `gui/main_window.py:617` | tooltip | Loads a previously saved session file, replacing the current list. | - | **by design**: a `GMenu` item carries no tooltip, so the Open Session hint is not shown; accepted gap for Phase 7 (the item itself is `the_files_menu_is_pythons`). `crates/hatate-gtk/src/menu.rs:334` |
| 40 | `gui/main_window.py:625` | action | Export Results… | `crates/hatate-core/src/table_menu.rs:172` (asserted at `crates/hatate-gtk/tests/smoke.rs:3932`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 41 | `gui/main_window.py:640` | action | Write Tag Files… | `crates/hatate-core/src/sidecar.rs:332` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 42 | `gui/main_window.py:658` | action | Open Matched URLs Log | `crates/hatate-gtk/src/menu.rs:107` (asserted at `crates/hatate-gtk/tests/smoke.rs:1601`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 43 | `gui/main_window.py:663` | action | Exit | `crates/hatate-cli/src/bridge_cmd.rs:10` (asserted at `crates/hatate-gtk/examples/shot.rs:230`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 44 | `gui/main_window.py:670` | menu | &Settings | `crates/hatate-cli/src/batch_cmd.rs:12` (asserted at `crates/hatate-core/tests/availability_https.rs:16`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 45 | `gui/main_window.py:671` | action | Preferences… | `crates/hatate-gtk/src/menu.rs:86` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 46 | `gui/main_window.py:674` | action | Edit Match Conditions… | `crates/hatate-gtk/src/menu.rs:151` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 47 | `gui/main_window.py:679` | menu | Theme | `crates/hatate-core/src/boorus/eshuushuu.rs:17` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 48 | `gui/main_window.py:694` | menu | &Help | `crates/hatate-cli/src/args.rs:29` (asserted at `crates/hatate-core/tests/hydrus_import.rs:1017`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 49 | `gui/main_window.py:695` | action | View Logs… | `crates/hatate-core/src/compare_view.rs:557` (asserted at `crates/hatate-gtk/tests/smoke.rs:1213`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 50 | `gui/main_window.py:699` | action | Parser Health… | `crates/hatate-gtk/src/menu.rs:169` (asserted at `crates/hatate-gtk/tests/smoke.rs:2777`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 51 | `gui/main_window.py:708` | action | Clear Search Cache… | `crates/hatate-core/src/table_menu.rs:15` (asserted at `crates/hatate-gtk/tests/smoke.rs:4025`) | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 52 | `gui/main_window.py:716` | action | Clear List and Saved Session… | `crates/hatate-gtk/src/menu.rs:172` | **unit**: the menu tables are `menu.rs`'s `the_files_menu_is_pythons` and `the_other_menus_are_pythons`; every entry has a handler (`phase6_coverage.rs` `no_menu_action_is_deferred`); the open menus are `menu-*`; the keyboard path is `menubar_keyboard_path`. `crates/hatate-gtk/src/menu.rs:334`, `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:4443` |
| 53 | `gui/main_window.py:731` | menu | &Hydrus | `crates/hatate-cli/src/args.rs:14` (asserted at `crates/hatate-cli/tests/batch_cli.rs:47`) | **unit**: the Hydrus menu is in `the_other_menus_are_pythons`; its five handlers are reached by `send` op, `import_url` op, `download_send` op, `show` op, `delete` op, `reconcile` op and `hydrus_menu_entries_and_the_h_key_reach_their_handlers`. `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:7223` |
| 54 | `gui/main_window.py:733` | action | Send File + URL + Tags to Hydrus (upload) | `crates/hatate-core/src/table_menu.rs:333` (asserted at `crates/hatate-gtk/tests/smoke.rs:4473`) | **unit**: the Hydrus menu is in `the_other_menus_are_pythons`; its five handlers are reached by `send` op, `import_url` op, `download_send` op, `show` op, `delete` op, `reconcile` op and `hydrus_menu_entries_and_the_h_key_reach_their_handlers`. `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:7223` |
| 55 | `gui/main_window.py:734` | tooltip | Uploads the local file, associates the matched URL with it, and adds the... | `crates/hatate-core/src/table_menu.rs:163` | **unit**: the Hydrus menu is in `the_other_menus_are_pythons`; its five handlers are reached by `send` op, `import_url` op, `download_send` op, `show` op, `delete` op, `reconcile` op and `hydrus_menu_entries_and_the_h_key_reach_their_handlers`. `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:7223` |
| 56 | `gui/main_window.py:741` | action | Send URL to Hydrus's URL Importer… | `crates/hatate-core/src/missing_files.rs:150` | **unit**: the Hydrus menu is in `the_other_menus_are_pythons`; its five handlers are reached by `send` op, `import_url` op, `download_send` op, `show` op, `delete` op, `reconcile` op and `hydrus_menu_entries_and_the_h_key_reach_their_handlers`. `crates/hatate-gtk/src/menu.rs:360`, `crates/hatate-gtk/tests/smoke.rs:7223` |
| 57 | `gui/main_window.py:760` | menu | Si&tes | `crates/hatate-core/src/availability.rs:104` (asserted at `crates/hatate-core/tests/engine_backends_wiring.rs:232`) | **op**: the Sites menu: `site` op, `sites_all` op toggle each site and all/none; `the_sites_menu_toggles_and_saves`. `crates/hatate-gtk/tests/smoke.rs:2335` |
| 58 | `gui/main_window.py:763` | action | Select All | `crates/hatate-gtk/src/menu.rs:131` (asserted at `crates/hatate-gtk/tests/smoke.rs:2330`) | **op**: the Sites menu: `site` op, `sites_all` op toggle each site and all/none; `the_sites_menu_toggles_and_saves`. `crates/hatate-gtk/tests/smoke.rs:2335` |
| 59 | `gui/main_window.py:767` | action | Select None | `crates/hatate-gtk/src/menu.rs:132` (asserted at `crates/hatate-gtk/tests/smoke.rs:2330`) | **op**: the Sites menu: `site` op, `sites_all` op toggle each site and all/none; `the_sites_menu_toggles_and_saves`. `crates/hatate-gtk/tests/smoke.rs:2335` |
| 60 | `gui/main_window.py:797` | message | Site filter: %s all sites | `crates/hatate-gtk/src/search_run.rs:500` | **op**: `sites_all` op. `crates/hatate-gtk/tests/smoke.rs:2335` |
| 61 | `gui/main_window.py:1359` | context-action | Open image URL in browser | `crates/hatate-gtk/src/review_ops.rs:922` | **smoke**: the matched preview's popup menu (`review-dark.png`/`review-light.png`); its entries are `review_view.rs`. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 62 | `gui/main_window.py:1360` | context-action | Copy image URL | `crates/hatate-gtk/src/review_ops.rs:923` | **smoke**: the matched preview's popup menu (`review-dark.png`/`review-light.png`); its entries are `review_view.rs`. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 63 | `gui/main_window.py:1389` | chooser | Add images / | `crates/hatate-gtk/src/session_ops.rs:68` | **op**: `add_files` op. `crates/hatate-gtk/tests/smoke.rs:688` |
| 64 | `gui/main_window.py:1396` | chooser | Add folder | `crates/hatate-gtk/src/session_ops.rs:82` (asserted at `crates/hatate-gtk/tests/smoke.rs:617`) | **op**: `add_folder` op. `crates/hatate-gtk/tests/smoke.rs:759` |
| 65 | `gui/main_window.py:1567` | message | Hydrus reconcile: %s | `crates/hatate-gtk/src/hydrus_ops.rs:437` | **op**: `reconcile` op. `crates/hatate-gtk/tests/smoke.rs:6409` |
| 66 | `gui/main_window.py:1608` | message | Flagged %d entry(s) whose file is no longer on disk | `crates/hatate-core/src/missing_files.rs:134` | **smoke**: the missing-files dialog. `crates/hatate-gtk/tests/smoke.rs:1134` |
| 67 | `gui/main_window.py:1617` | message | Missing files | `crates/hatate-core/src/missing_files.rs:143` (asserted at `crates/hatate-gtk/tests/smoke.rs:1145`) | **smoke**: the missing-files dialog. `crates/hatate-gtk/tests/smoke.rs:1134` |
| 68 | `gui/main_window.py:1637` | chooser | Save Session As | `crates/hatate-core/src/session/mod.rs:9` (asserted at `crates/hatate-gtk/tests/smoke.rs:1193`) | **op**: `save_session_as` op. `crates/hatate-gtk/tests/smoke.rs:1172` |
| 69 | `gui/main_window.py:1674` | chooser | Export Results | `crates/hatate-core/src/table_menu.rs:172` (asserted at `crates/hatate-gtk/tests/smoke.rs:3932`) | **op**: `export` op (1 differing class, the failure text, `docs/queue-view.md:92`). `crates/hatate-gtk/tests/smoke.rs:3900` |
| 70 | `gui/main_window.py:1707` | message | Exported %d result(s) as %s to %s | `crates/hatate-gtk/src/table_actions.rs:107` (asserted at `crates/hatate-gtk/tests/smoke.rs:3923`) | **op**: `export` op (1 differing class, the failure text, `docs/queue-view.md:92`). `crates/hatate-gtk/tests/smoke.rs:3900` |
| 71 | `gui/main_window.py:1851` | chooser | Open Session | `crates/hatate-core/src/session/named.rs:34` (asserted at `crates/hatate-gtk/tests/smoke.rs:1248`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 72 | `gui/main_window.py:1898` | message | Opened session %s with %d entries | `crates/hatate-gtk/src/session_ops.rs:642` | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 73 | `gui/main_window.py:1924` | message | Clear Saved Session / Empty the current list and discard the saved sessi... | `crates/hatate-gtk/src/session_ops.rs:711` (asserted at `crates/hatate-gtk/tests/smoke.rs:1319`) | **op**: `clear_session` op. `crates/hatate-gtk/tests/smoke.rs:1302` |
| 74 | `gui/main_window.py:2009` | message | Adding %d file(s) to the list (%d duplicate(s) skipped) | `crates/hatate-core/src/batch/run.rs:298` (asserted at `crates/hatate-gtk/tests/smoke.rs:736`) | **smoke**: the hash pass fills the hashes and reads the status strings. `crates/hatate-gtk/tests/smoke.rs:688` |
| 75 | `gui/main_window.py:2248` | message | Clear Search Cache / This deletes all saved search results. Images you'v... | `crates/hatate-core/src/table_menu.rs:15` (asserted at `crates/hatate-gtk/tests/smoke.rs:4025`) | **op**: `clear_cache` op. `crates/hatate-gtk/tests/smoke.rs:4024` |
| 76 | `gui/main_window.py:2258` | message | Search cache cleared via menu action (%d entries removed) | `crates/hatate-gtk/src/table_actions.rs:418` | **op**: `clear_cache` op. `crates/hatate-gtk/tests/smoke.rs:4024` |
| 77 | `gui/main_window.py:2259` | message | Clear Search Cache | `crates/hatate-core/src/table_menu.rs:15` (asserted at `crates/hatate-gtk/tests/smoke.rs:4025`) | **op**: `clear_cache` op. `crates/hatate-gtk/tests/smoke.rs:4024` |
| 78 | `gui/main_window.py:2279` | message | Start Search clicked: %d unsearched image(s) | `crates/hatate-gtk/src/search_run.rs:567` | **op**: `start` op starts, refuses a second run and says when nothing is unsearched. `crates/hatate-gtk/tests/smoke.rs:2057` |
| 79 | `gui/main_window.py:2503` | message | Continuing without SauceNAO: %d unsearched image(s) | `crates/hatate-gtk/src/search_run.rs:1098` | **op**: `continue_without_saucenao` op. `crates/hatate-gtk/tests/smoke.rs:2140` |
| 80 | `gui/main_window.py:2913` | message | pass #%d: generating %d thumbnail(s) for rows %d..%d (%s%s) | - | **n/a**: a debug log line of the thumbnail pass, not UI. |
| 81 | `gui/main_window.py:3025` | message | Reset %d result(s), dropping %d cached search result(s) | `crates/hatate-gtk/src/table_actions.rs:312` (asserted at `crates/hatate-gtk/tests/smoke.rs:4044`) | **op**: `reset` op. `crates/hatate-gtk/tests/smoke.rs:3751` |
| 82 | `gui/main_window.py:3055` | message | %s %d image(s) as reviewed (%d still to review) | `crates/hatate-core/src/queue.rs:239` | **op**: `reviewed` op. `crates/hatate-gtk/tests/smoke.rs:3751` |
| 83 | `gui/main_window.py:3072` | message | Re-search requested for %d image(s): %s | `crates/hatate-core/src/queue.rs:742` | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 84 | `gui/main_window.py:3094` | message | %s-only search requested for %d image(s) | `crates/hatate-gtk/src/search_run.rs:1012` (asserted at `crates/hatate-core/tests/batch_run.rs:382`) | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 85 | `gui/main_window.py:3137` | message | Opposite-engine search requested for %d image(s): %s | `crates/hatate-core/src/batch/run.rs:756` | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 86 | `gui/main_window.py:3217` | message | Removed %d dead match(es) from %s | `crates/hatate-gtk/src/checks.rs:241` | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:2509` |
| 87 | `gui/main_window.py:3237` | message | Opening comparison view for %s | `crates/hatate-gtk/src/compare_dialog.rs:436` | **op**: `open_compare` op opens the dialog and refuses with no match; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5949` |
| 88 | `gui/main_window.py:3426` | message | Opened a Hydrus page for %d file(s)%s | `crates/hatate-gtk/src/hydrus_ops.rs:1080` | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 89 | `gui/main_window.py:3461` | message | Show in Hydrus | `crates/hatate-core/src/shortcuts.rs:92` (asserted at `crates/hatate-gtk/tests/smoke.rs:6181`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 90 | `gui/main_window.py:3553` | message | Hydrus deleted %d of %d requested file(s) | `crates/hatate-gtk/src/hydrus_ops.rs:1278` (asserted at `crates/hatate-gtk/tests/smoke.rs:6262`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 91 | `gui/main_window.py:3563` | message | Delete from Hydrus | `crates/hatate-core/src/table_menu.rs:418` (asserted at `crates/hatate-gtk/tests/differential.rs:201`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 92 | `gui/main_window.py:3588` | message | Removed %d file(s) from the list: %s | `crates/hatate-core/src/queue.rs:271` | **op**: `remove` op. `crates/hatate-gtk/tests/smoke.rs:3751` |
| 93 | `gui/main_window.py:3625` | message | Removed %d %s file(s) from the list | `crates/hatate-core/src/batch/run.rs:338` (asserted at `crates/hatate-gtk/tests/smoke.rs:3830`) | **op**: `remove` op. `crates/hatate-gtk/tests/smoke.rs:3613` |
| 94 | `gui/main_window.py:1276` | message | Selected match for %s is gone (%s) - dropping it and moving to the next | `crates/hatate-core/src/availability.rs:581` (asserted at `crates/hatate-core/tests/search_image_https.rs:318`) | **smoke**: the candidate's fetch failing says so in the Review state (`review-error-dark.png`/`review-error-light.png`). `crates/hatate-gtk/tests/smoke.rs:5594` |
| 95 | `gui/main_window.py:1337` | message | Removed tag %s from %s (cleared via inline edit) | `crates/hatate-gtk/src/review_ops.rs:782` | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:4732` |
| 96 | `gui/main_window.py:1342` | message | Renamed tag %s -> %s on %s | `crates/hatate-gtk/src/review_ops.rs:792` | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:4732` |
| 97 | `gui/main_window.py:1352` | message | Open image URL / This match doesn't have a valid web address to open. | `crates/hatate-gtk/src/review_ops.rs:917` | **smoke**: the matched preview's popup menu (`review-dark.png`/`review-light.png`); its entries are `review_view.rs`. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 98 | `gui/main_window.py:1455` | message | restored %d entry(s) - deferring thumbnails to the viewport pass | `crates/hatate-cli/src/batch_cmd.rs:156` (asserted at `crates/hatate-core/tests/similarity_check_tests.rs:506`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:865` |
| 99 | `gui/main_window.py:1633` | message | Save Session / There's nothing in the list to save. | `crates/hatate-gtk/src/session_ops.rs:503` (asserted at `crates/hatate-gtk/tests/smoke.rs:1223`) | **op**: `save_session_as` op. `crates/hatate-gtk/tests/smoke.rs:1172` |
| 100 | `gui/main_window.py:1649` | message | Saved session to %s (%d entries) | `crates/hatate-gtk/src/session_ops.rs:530` (asserted at `crates/hatate-cli/tests/batch_cli.rs:119`) | **op**: `save_session_as` op. `crates/hatate-gtk/tests/smoke.rs:1172` |
| 101 | `gui/main_window.py:1652` | message | Save Session | `crates/hatate-core/src/session/mod.rs:9` (asserted at `crates/hatate-gtk/tests/smoke.rs:1193`) | **op**: `save_session_as` op. `crates/hatate-gtk/tests/smoke.rs:1172` |
| 102 | `gui/main_window.py:1670` | message | Export Results / There's nothing in the list to export. | `crates/hatate-gtk/src/table_actions.rs:71` (asserted at `crates/hatate-gtk/tests/smoke.rs:3902`) | **op**: `export` op (1 differing class, the failure text, `docs/queue-view.md:92`). `crates/hatate-gtk/tests/smoke.rs:3900` |
| 103 | `gui/main_window.py:1729` | message | Write Tag Files / There's nothing in the list to write tag files for. | `crates/hatate-gtk/src/table_actions.rs:143` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 104 | `gui/main_window.py:1737` | message | Write Tag Files | `crates/hatate-core/src/sidecar.rs:332` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 105 | `gui/main_window.py:1748` | message | Write Tag Files | `crates/hatate-core/src/sidecar.rs:332` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 106 | `gui/main_window.py:1794` | message | Write Tag Files | `crates/hatate-core/src/sidecar.rs:332` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 107 | `gui/main_window.py:1806` | message | Write Tag Files | `crates/hatate-core/src/sidecar.rs:332` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 108 | `gui/main_window.py:1813` | message | Write Tag Files | `crates/hatate-core/src/sidecar.rs:332` | **op**: `write_tags` op. `crates/hatate-gtk/tests/smoke.rs:3973` |
| 109 | `gui/main_window.py:1845` | message | Open Session / A search is running. Stop it before opening a different s... | `crates/hatate-gtk/src/session_ops.rs:563` (asserted at `crates/hatate-gtk/tests/smoke.rs:1293`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 110 | `gui/main_window.py:1859` | message | Open Session | `crates/hatate-core/src/session/named.rs:34` (asserted at `crates/hatate-gtk/tests/smoke.rs:1248`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 111 | `gui/main_window.py:1869` | message | Open Session | `crates/hatate-core/src/session/named.rs:34` (asserted at `crates/hatate-gtk/tests/smoke.rs:1248`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 112 | `gui/main_window.py:1882` | message | Open Session | `crates/hatate-core/src/session/named.rs:34` (asserted at `crates/hatate-gtk/tests/smoke.rs:1248`) | **op**: `open_session` op. `crates/hatate-gtk/tests/smoke.rs:1233` |
| 113 | `gui/main_window.py:1948` | message | Add Files / Please wait for the current batch to finish being added. | `crates/hatate-gtk/src/session_ops.rs:130` (asserted at `crates/hatate-gtk/tests/smoke.rs:809`) | **n/a**: a log line of the Python app (`added %d entry(s)`), not UI; the Rust log sink is `logsink.rs`. |
| 114 | `gui/main_window.py:2028` | message | added %d entry(s) - deferring thumbnails to the viewport pass | - | **n/a**: a debug log line of the Python app (the lazy-thumbnail pass), not UI. |
| 115 | `gui/main_window.py:2150` | message | Query Hydrus / Set your Hydrus access key in Settings first. | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **smoke**: as above; `hydrus-query-dark.png`/`hydrus-query-light.png`. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 116 | `gui/main_window.py:2173` | message | Add tags / Select at least one image first. | `crates/hatate-gtk/src/review_ops.rs:854` (asserted at `crates/hatate-gtk/tests/smoke.rs:4740`) | **smoke**: the Review tag panel adds, removes and edits in place; `review-addtags-dark.png`/`review-addtags-light.png`. `crates/hatate-gtk/tests/smoke.rs:4732` |
| 117 | `gui/main_window.py:2197` | message | Matched URLs Log / The log file doesn't exist yet. | `crates/hatate-gtk/src/session_ops.rs:735` (asserted at `crates/hatate-gtk/tests/smoke.rs:1606`) | **op**: `open_log` op; `logs-dark.png`/`logs-light.png`. `crates/hatate-gtk/tests/smoke.rs:1582` |
| 118 | `gui/main_window.py:2227` | message | Parser for %s looks broken: %s | `crates/hatate-gtk/src/parser_health.rs:473` | **smoke**: the parser-health dialog and its warning. `crates/hatate-gtk/tests/smoke.rs:2696` |
| 119 | `gui/main_window.py:2272` | message | Search / A search is already running. | `crates/hatate-gtk/src/search_run.rs:545` (asserted at `crates/hatate-gtk/tests/smoke.rs:2091`) | **op**: `start` op starts, refuses a second run and says when nothing is unsearched. `crates/hatate-gtk/tests/smoke.rs:2057` |
| 120 | `gui/main_window.py:2276` | message | Search / No unsearched images in the list. | `crates/hatate-gtk/src/search_run.rs:562` (asserted at `crates/hatate-gtk/tests/smoke.rs:2070`) | **op**: `start` op starts, refuses a second run and says when nothing is unsearched. `crates/hatate-gtk/tests/smoke.rs:2057` |
| 121 | `gui/main_window.py:2612` | message | Discarding saved column layout: saved with %s column(s), this build has ... | `crates/hatate-gtk/src/window.rs:304` | **smoke**: the header layout is saved on close and restored, repaired or discarded. `crates/hatate-gtk/tests/smoke.rs:4054` |
| 122 | `gui/main_window.py:2654` | message | Repaired restored column layout: %s | `crates/hatate-gtk/src/window.rs:318` | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:4054` |
| 123 | `gui/main_window.py:2841` | message | lazy thumbnails active: lazy=%s source=%s buffer=%d rows debounce=%dms i... | - | **n/a**: a debug log line of the thumbnail pass, not UI. |
| 124 | `gui/main_window.py:3006` | message | Reset results | `crates/hatate-core/src/table_menu.rs:24` | **op**: `reset` op. `crates/hatate-gtk/tests/smoke.rs:3751` |
| 125 | `gui/main_window.py:3067` | message | Search / A search is already running. | `crates/hatate-gtk/src/search_run.rs:545` (asserted at `crates/hatate-gtk/tests/smoke.rs:2091`) | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 126 | `gui/main_window.py:3087` | message | Search / A search is already running. | `crates/hatate-gtk/src/search_run.rs:545` (asserted at `crates/hatate-gtk/tests/smoke.rs:2091`) | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 127 | `gui/main_window.py:3111` | message | Search / A search is already running. | `crates/hatate-gtk/src/search_run.rs:545` (asserted at `crates/hatate-gtk/tests/smoke.rs:2091`) | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 128 | `gui/main_window.py:3132` | message | Opposite-engine search: %d row(s) had no match to flip, using %r (the op... | `crates/hatate-gtk/src/search_run.rs:1062` | **op**: `start` op and the re-search scenario. `crates/hatate-gtk/tests/smoke.rs:2387` |
| 129 | `gui/main_window.py:3151` | message | Check Match Availability / A check is already running. | `crates/hatate-gtk/src/checks.rs:69` (asserted at `crates/hatate-gtk/tests/smoke.rs:2641`) | **smoke**: the check's refusals, the all-clear text and the Yes/No offer. `crates/hatate-gtk/tests/smoke.rs:2509` |
| 130 | `gui/main_window.py:3154` | message | Check Match Availability / This image has no matches to check. | `crates/hatate-gtk/src/checks.rs:80` (asserted at `crates/hatate-gtk/tests/smoke.rs:2519`) | **smoke**: the check's refusals, the all-clear text and the Yes/No offer. `crates/hatate-gtk/tests/smoke.rs:2509` |
| 131 | `gui/main_window.py:3186` | message | Check Match Availability | `crates/hatate-core/src/table_menu.rs:388` (asserted at `crates/hatate-gtk/tests/smoke.rs:2518`) | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:2509` |
| 132 | `gui/main_window.py:3216` | message | All matches for %s were gone; entry now has no match | `crates/hatate-gtk/src/checks.rs:237` | **smoke**: as above. `crates/hatate-gtk/tests/smoke.rs:2509` |
| 133 | `gui/main_window.py:3226` | message | Compare / This image has no match to compare against yet. | `crates/hatate-gtk/src/compare_dialog.rs:423` (asserted at `crates/hatate-gtk/tests/smoke.rs:5911`) | **op**: `open_compare` op opens the dialog and refuses with no match; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5949` |
| 134 | `gui/main_window.py:3231` | message | Compare / The local file is no longer on disk, so there's nothing to com... | `crates/hatate-core/src/availability.rs:163` (asserted at `crates/hatate-gtk/tests/smoke.rs:5922`) | **op**: `open_compare` op opens the dialog and refuses with no match; `review-compare-dark.png`/`review-compare-light.png`. `crates/hatate-gtk/tests/smoke.rs:5949` |
| 135 | `gui/main_window.py:3245` | message | Check for Upscaling / A check is already running. | `crates/hatate-gtk/src/checks.rs:69` (asserted at `crates/hatate-gtk/tests/smoke.rs:2641`) | **smoke**: the upscale check marks rows and summarises. `crates/hatate-gtk/tests/smoke.rs:2634` |
| 136 | `gui/main_window.py:3383` | message | Show in Hydrus / Set your Hydrus access key in Settings first. | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 137 | `gui/main_window.py:3390` | message | Show in Hydrus / None of the selected files have a hash yet, so there is... | `crates/hatate-gtk/src/hydrus_ops.rs:994` (asserted at `crates/hatate-gtk/tests/smoke.rs:6181`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 138 | `gui/main_window.py:3406` | message | Show in Hydrus / Hydrus doesn't have any of the selected files, so there... | `crates/hatate-core/src/shortcuts.rs:92` (asserted at `crates/hatate-gtk/tests/smoke.rs:6181`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 139 | `gui/main_window.py:3443` | message | Show in Hydrus / Hydrus refused this because the access key lacks the "m... | `crates/hatate-gtk/src/hydrus_ops.rs:148` (asserted at `crates/hatate-gtk/tests/smoke.rs:6317`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 140 | `gui/main_window.py:3453` | message | Show in Hydrus / This Hydrus client is too old for this: opening a page ... | `crates/hatate-core/src/shortcuts.rs:92` (asserted at `crates/hatate-gtk/tests/smoke.rs:6181`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 141 | `gui/main_window.py:3480` | message | Delete from Hydrus / Set your Hydrus access key in Settings first. | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 142 | `gui/main_window.py:3494` | message | Delete from Hydrus / Hydrus isn't holding any of the selected files, so ... | `crates/hatate-core/src/table_menu.rs:418` (asserted at `crates/hatate-gtk/tests/differential.rs:201`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 143 | `gui/main_window.py:3735` | message | Send to Hydrus / Select at least one row. | `crates/hatate-gtk/src/hydrus_ops.rs:176` (asserted at `crates/hatate-gtk/tests/smoke.rs:6163`) | **op**: `send` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 144 | `gui/main_window.py:3738` | message | Send to Hydrus / Set your Hydrus access key in Settings first. | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **op**: `send` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 145 | `gui/main_window.py:3798` | message | Send URL to Hydrus's Importer / Select at least one row. | `crates/hatate-gtk/src/hydrus_ops.rs:50` (asserted at `crates/hatate-gtk/tests/smoke.rs:6177`) | **op**: `import_url` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 146 | `gui/main_window.py:3801` | message | Send URL to Hydrus's Importer / Set your Hydrus access key in Settings f... | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **op**: `import_url` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 147 | `gui/main_window.py:3972` | message | Download + Send to Hydrus / Select at least one row. | `crates/hatate-gtk/src/hydrus_ops.rs:51` (asserted at `crates/hatate-gtk/tests/smoke.rs:6179`) | **op**: `download_send` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 148 | `gui/main_window.py:3975` | message | Download + Send to Hydrus / Set your Hydrus access key in Settings first... | `crates/hatate-gtk/src/hydrus_ops.rs:54` (asserted at `crates/hatate-gtk/tests/smoke.rs:6173`) | **op**: `download_send` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 149 | `gui/main_window.py:542` | message | MCP server did not start: %s | `crates/hatate-gtk/src/mcp_bridge.rs:287` | **n/a**: a debug log line of the lazy-thumbnail setup, not UI. |
| 150 | `gui/main_window.py:1700` | message | Could not export results to %s: %s | - | **by design**: the Python box shows the path and the OS text; Rust shows `user_message()` with neither (DAN-2116), and the one differing class of the action differential is this text (`docs/queue-view.md:92`). `crates/hatate-gtk/tests/smoke.rs:3900` |
| 151 | `gui/main_window.py:1701` | message | Export Results | `crates/hatate-core/src/table_menu.rs:172` (asserted at `crates/hatate-gtk/tests/smoke.rs:3932`) | **op**: `export` op (1 differing class, the failure text, `docs/queue-view.md:92`). `crates/hatate-gtk/tests/smoke.rs:3900` |
| 152 | `gui/main_window.py:2001` | message | All %d file(s) were already in the list, nothing added | `crates/hatate-gtk/src/session_ops.rs:243` | **smoke**: the hash pass fills the hashes and reads the status strings. `crates/hatate-gtk/tests/smoke.rs:688` |
| 153 | `gui/main_window.py:2002` | message | Add Files | `crates/hatate-gtk/src/menu.rs:90` (asserted at `crates/hatate-gtk/tests/smoke.rs:723`) | **smoke**: the hash pass fills the hashes and reads the status strings. `crates/hatate-gtk/tests/smoke.rs:688` |
| 154 | `gui/main_window.py:2188` | message | Added %d tag(s) to %d image(s) | `crates/hatate-gtk/src/review_ops.rs:887` (asserted at `crates/hatate-core/tests/batch_run.rs:382`) | **smoke**: the Review tag panel adds, removes and edits in place; `review-addtags-dark.png`/`review-addtags-light.png`. `crates/hatate-gtk/tests/smoke.rs:4732` |
| 155 | `gui/main_window.py:2408` | message | Could not render SauceNAO quota display: %s | - | **n/a**: a log line (`Could not render SauceNAO quota display`), not UI. |
| 156 | `gui/main_window.py:2627` | message | Could not restore table column layout: %s | - | **n/a**: a log line of the Python app (a header state that would not restore), not UI; the Rust restore repairs or discards a bad layout and asserts it. `crates/hatate-gtk/tests/smoke.rs:4054` |
| 157 | `gui/main_window.py:3401` | message | Show in Hydrus | `crates/hatate-core/src/shortcuts.rs:92` (asserted at `crates/hatate-gtk/tests/smoke.rs:6181`) | **op**: `show` op. `crates/hatate-gtk/tests/smoke.rs:6323` |
| 158 | `gui/main_window.py:3489` | message | Delete from Hydrus | `crates/hatate-core/src/table_menu.rs:418` (asserted at `crates/hatate-gtk/tests/differential.rs:201`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 159 | `gui/main_window.py:3538` | message | Delete from Hydrus failed: %s | `crates/hatate-gtk/src/hydrus_ops.rs:1258` | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 160 | `gui/main_window.py:3539` | message | Delete from Hydrus | `crates/hatate-core/src/table_menu.rs:418` (asserted at `crates/hatate-gtk/tests/differential.rs:201`) | **op**: `delete` op (the box defaults to Cancel). `crates/hatate-gtk/tests/smoke.rs:6250` |
| 161 | `gui/main_window.py:3870` | message | Send URL to Hydrus's Importer | `crates/hatate-gtk/src/hydrus_ops.rs:50` (asserted at `crates/hatate-gtk/tests/smoke.rs:6177`) | **op**: `import_url` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 162 | `gui/main_window.py:4037` | message | Download + Send to Hydrus / None of the selected images have a known dir... | `crates/hatate-gtk/src/hydrus_ops.rs:51` (asserted at `crates/hatate-gtk/tests/smoke.rs:6179`) | **op**: `download_send` op. `crates/hatate-gtk/tests/smoke.rs:6205` |
| 163 | `gui/main_window.py:2164` | message | Skipped %d duplicate(s) already in the list during Query Hydrus import | `crates/hatate-gtk/src/hydrus_query.rs:525` | **smoke**: as above; `hydrus-query-dark.png`/`hydrus-query-light.png`. `crates/hatate-gtk/tests/smoke.rs:6566` |
| 164 | `gui/main_window.py:4077` | message | MCP server did not restart: %s | `crates/hatate-gtk/src/mcp_bridge.rs:307` | **op**: `s_open` op; the tabs are `settings-*`. `crates/hatate-gtk/tests/smoke.rs:7648` |
| 165 | `gui/pawchive_index_dialog.py:80` | placeholder | Artist name, or a pawchive link | `crates/hatate-gtk/src/pawchive_dialog.rs:239` | **smoke**: the Pawchive window's controls and tooltips. `crates/hatate-gtk/tests/smoke.rs:6934` |
| 166 | `gui/pawchive_index_dialog.py:122` | tooltip | Stops after the current request. What was indexed is kept, and indexing ... | `crates/hatate-gtk/src/pawchive_dialog.rs:285` | **smoke**: the Pawchive window's controls and tooltips. `crates/hatate-gtk/tests/smoke.rs:6934` |
| 167 | `gui/pawchive_index_dialog.py:264` | message | Remove artists | `crates/hatate-gtk/src/pawchive_dialog.rs:671` (asserted at `crates/hatate-gtk/tests/smoke.rs:6956`) | **smoke**: removing an artist asks first. `crates/hatate-gtk/tests/smoke.rs:6934` |
| 168 | `gui/review_shortcuts.py:240` | message | Open match in browser / This match doesn't have a valid web address to o... | `crates/hatate-gtk/src/review_ops.rs:917` | **smoke**: the key opens the match, or says it has no web address. `crates/hatate-gtk/tests/smoke.rs:5344` |
| 169 | `gui/review_shortcuts.py:210` | message | No handler for review shortcut %r - not binding it | `crates/hatate-gtk/src/review_ops.rs:1239` | **smoke**: the Review shortcuts are installed and keys step, mark, skip and open the match. `crates/hatate-gtk/tests/smoke.rs:5344` |
| 170 | `gui/review_shortcuts.py:216` | message | Ignoring unparseable shortcut %r for %r | `crates/hatate-gtk/src/review_ops.rs:1313` | **smoke**: the Review shortcuts are installed and keys step, mark, skip and open the match. `crates/hatate-gtk/tests/smoke.rs:5344` |
| 171 | `gui/review_view.py:206` | tooltip | How the selected match compares with your local file. | `crates/hatate-gtk/src/review_view.rs:47` | **smoke**: the readout under the header, its size chip and its drop to the row below. `crates/hatate-gtk/tests/smoke.rs:5007` |
| 172 | `gui/review_view.py:375` | tooltip | Scroll to zoom where the pointer is, drag to pan, double-click to fit. +... | `crates/hatate-gtk/src/wipe_view.rs:97` | **smoke**: the wipe chrome names its ends and zoom buttons (`review-wipe-dark.png`/`review-wipe-light.png`). `crates/hatate-gtk/tests/smoke.rs:5667` |
| 173 | `gui/review_view.py:470` | tooltip | Send the file, its URL and its tags to Hydrus. | `crates/hatate-gtk/src/review_view.rs:559` | **smoke**: the decision card's controls follow the selection (`review-dark.png`/`review-light.png`). `crates/hatate-gtk/tests/smoke.rs:4662` |
| 174 | `gui/review_view.py:488` | tooltip | Hand the URL to Hydrus's own downloader. | `crates/hatate-gtk/src/review_view.rs:575` | **smoke**: the decision card's controls follow the selection (`review-dark.png`/`review-light.png`). `crates/hatate-gtk/tests/smoke.rs:4662` |
| 175 | `gui/review_view.py:494` | tooltip | Download the full-resolution match and send that to Hydrus with the tags... | `crates/hatate-gtk/src/review_view.rs:578` | **smoke**: the decision card's controls follow the selection (`review-dark.png`/`review-light.png`). `crates/hatate-gtk/tests/smoke.rs:4662` |
| 176 | `gui/review_view.py:510` | tooltip | Open the matched page in your browser. | `crates/hatate-gtk/src/review_view.rs:589` | **smoke**: the decision card's controls follow the selection (`review-dark.png`/`review-light.png`). `crates/hatate-gtk/tests/smoke.rs:4662` |
| 177 | `gui/review_view.py:551` | tooltip | Previous image. | `crates/hatate-gtk/src/review_view.rs:604` | **smoke**: the position strip. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 178 | `gui/review_view.py:560` | tooltip | Skip ahead to the next image you haven't sent or marked reviewed. | `crates/hatate-gtk/src/review_view.rs:611` | **smoke**: the position strip. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 179 | `gui/review_view.py:566` | tooltip | Next image. | `crates/hatate-gtk/src/review_view.rs:614` | **smoke**: the position strip. `crates/hatate-gtk/tests/smoke.rs:4662` |
| 180 | `gui/review_view.py:720` | message | Could not load the comparison for %s: %s | - | **by design**: the Python box carries the OS or Qt text; Rust shows the error state with `user_message()` (DAN-2116): `review-error-dark.png`/`review-error-light.png`. `crates/hatate-gtk/tests/smoke.rs:5594` |
| 181 | `gui/session_autosave.py:70` | message | Session autosave every %ds | `crates/hatate-gtk/src/session_ops.rs:784` | **op**: `autosave` op. `crates/hatate-gtk/tests/smoke.rs:1552` |
| 182 | `gui/settings_dialog.py:256` | placeholder | a setting's name - "quota", "cookies", "thumbnail"… | `crates/hatate-gtk/src/settings_dialog.rs:211` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 183 | `gui/settings_dialog.py:566` | chooser | Choose log file | `crates/hatate-gtk/src/settings_dialog.rs:356` | **op**: `s_press` op (Browse); `settings_engine_line_lens_warning_and_browse_follow_python`. `crates/hatate-gtk/tests/smoke.rs:7748` |
| 184 | `gui/settings_dialog.py:686` | tooltip | trace.moe false-positives a lot below ~85%. Results under this are dropp... | `crates/hatate-gtk/src/settings_spec.rs:914` | **op**: `s_set` op, `s_find` op; the tab is `settings-0-dark.png`/`settings-0-light.png`. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 185 | `gui/settings_dialog.py:958` | message | Google Lens was enabled but %s is not installed | `crates/hatate-gtk/src/settings_dialog.rs:390` (asserted at `crates/hatate-gtk/tests/smoke.rs:7719`) | **smoke**: the Lens warning follows the settings. `crates/hatate-gtk/tests/smoke.rs:7748` |
| 186 | `gui/settings_dialog.py:1082` | tooltip | How often to check Hydrus while waiting. | `crates/hatate-gtk/src/settings_spec.rs:1243` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 187 | `gui/settings_dialog.py:1103` | placeholder | PHPSESSID value from a logged-in browser session | `crates/hatate-gtk/src/settings_spec.rs:1607` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 188 | `gui/settings_dialog.py:1129` | placeholder | your e621 username | `crates/hatate-gtk/src/settings_spec.rs:1634` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 189 | `gui/settings_dialog.py:1141` | placeholder | from Account > Manage API Access on e621 | `crates/hatate-gtk/src/settings_spec.rs:1649` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 190 | `gui/settings_dialog.py:1169` | placeholder | _sankakuchannel_session=...; locale=en | `crates/hatate-gtk/src/settings_spec.rs:1680` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 191 | `gui/settings_dialog.py:1205` | placeholder | anime_pictures_jwt=eyJ0eXAiOiJKV1Qi… | `crates/hatate-gtk/src/settings_spec.rs:1716` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 192 | `gui/settings_dialog.py:1250` | placeholder | auth=...; auth_secure=...; userinfo=... | `crates/hatate-gtk/src/settings_spec.rs:1753` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 193 | `gui/settings_dialog.py:1303` | chooser | Test DeviantArt / Paste a DeviantArt deviation URL (ideally an adult one... | `crates/hatate-gtk/src/settings_tests.rs:81` | **smoke**: the prompt's title and preset of the three page Tests are asserted; the whole prompt text of every Test is compared by the transport differential (`docs/gtk-6h2.md` section 5). `crates/hatate-gtk/tests/smoke.rs:8944` |
| 194 | `gui/settings_dialog.py:1382` | message | DeviantArt test on %s succeeded: %s | `crates/hatate-gtk/src/settings_tests.rs:440` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label after a good page, compared by the differential. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 195 | `gui/settings_dialog.py:1394` | chooser | Test Anime-Pictures / Paste an Anime-Pictures post URL (ideally an accou... | `crates/hatate-gtk/src/settings_tests.rs:86` | **smoke**: the prompt's title and preset of the three page Tests are asserted; the whole prompt text of every Test is compared by the transport differential (`docs/gtk-6h2.md` section 5). `crates/hatate-gtk/tests/smoke.rs:8944` |
| 196 | `gui/settings_dialog.py:1476` | message | Anime-Pictures test on %s succeeded: %s | `crates/hatate-gtk/src/settings_tests.rs:473` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label after a good page, compared by the differential. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 197 | `gui/settings_dialog.py:1487` | chooser | Test Sankaku / Paste a Sankaku post URL (ideally one you can only see wh... | `crates/hatate-gtk/src/settings_tests.rs:91` | **differential**: Sankaku's prompt is not in the smoke scenario (its Test has no wrong-site check); the transport differential compares the title, text and preset of every prompt, 84 + 144 + 144 cases, 0 differing (`docs/gtk-6h2.md` section 5). `crates/hatate-gtk/tests/smoke.rs:9227` |
| 198 | `gui/settings_dialog.py:1575` | placeholder | optional, e.g. "sn" | `crates/hatate-gtk/src/settings_spec.rs:1318` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 199 | `gui/settings_dialog.py:1608` | placeholder | empty = "my tags" local service | `crates/hatate-gtk/src/settings_spec.rs:1377` | **op**: `s_set` op, `s_find` op; the tab is `settings-*`. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 200 | `gui/settings_dialog.py:1774` | chooser | Test Pixiv / Paste any Pixiv artwork URL (either the modern /artworks/ f... | `crates/hatate-gtk/src/settings_tests.rs:96` | **smoke**: the prompt's title and preset of the three page Tests are asserted; the whole prompt text of every Test is compared by the transport differential (`docs/gtk-6h2.md` section 5). `crates/hatate-gtk/tests/smoke.rs:8944` |
| 201 | `gui/settings_dialog.py:1843` | message | Pixiv test on %s returned no usable data | `crates/hatate-gtk/src/settings_tests.rs:505` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label after an empty page, asserted at `crates/hatate-gtk/src/settings_tests.rs:654` and `:658`. `crates/hatate-gtk/src/settings_tests.rs:654` |
| 202 | `gui/settings_dialog.py:1874` | message | Fetched %d tag service(s) from Hydrus | `crates/hatate-gtk/src/settings_dialog.rs:658` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the list the Fetch fills, asserted at `crates/hatate-gtk/tests/smoke.rs:9069`. `crates/hatate-gtk/tests/smoke.rs:9069` |
| 203 | `gui/settings_dialog.py:1922` | placeholder | artist:creator | `crates/hatate-gtk/src/settings_spec.rs:1463` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 204 | `gui/settings_dialog.py:1949` | placeholder | highres bad_* meta:* | `crates/hatate-gtk/src/settings_spec.rs:1493` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 205 | `gui/settings_dialog.py:1969` | placeholder | rating | `crates/hatate-gtk/src/settings_spec.rs:1510` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 206 | `gui/settings_dialog.py:1970` | tooltip | The namespace that tag is filed under. Leave it blank to use "rating". | `crates/hatate-gtk/src/settings_spec.rs:1511` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 207 | `gui/settings_dialog.py:1992` | placeholder | character:#RRGGBB | `crates/hatate-gtk/src/settings_spec.rs:1520` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 208 | `gui/settings_dialog.py:2027` | placeholder | hatate:found | `crates/hatate-core/src/tag_rules.rs:426` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 209 | `gui/settings_dialog.py:2037` | placeholder | hatate:not found | `crates/hatate-core/src/tag_rules.rs:427` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 210 | `gui/settings_dialog.py:2049` | placeholder | hatate:few tags | `crates/hatate-core/src/search_engine.rs:1168` | **op**: `s_set` op, `s_find` op. `crates/hatate-gtk/tests/smoke.rs:7722` |
| 211 | `gui/settings_dialog.py:1363` | message | DeviantArt test on %s: still blurred | `crates/hatate-gtk/src/settings_tests.rs:406` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label of a blurred page, asserted at `crates/hatate-gtk/src/settings_tests.rs:637` and `:641`. `crates/hatate-gtk/src/settings_tests.rs:637` |
| 212 | `gui/settings_dialog.py:1457` | message | Anime-Pictures test on %s: still restricted | `crates/hatate-gtk/src/settings_tests.rs:445` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label of a restricted page, asserted at `crates/hatate-gtk/src/settings_tests.rs:645` and `:649`. `crates/hatate-gtk/src/settings_tests.rs:645` |
| 213 | `gui/settings_dialog.py:1830` | message | Pixiv test on %s succeeded: %s | `crates/hatate-gtk/src/settings_tests.rs:502` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label after a good page, compared by the differential. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 214 | `gui/settings_dialog.py:1870` | message | Hydrus reported no tag services at %s | `crates/hatate-gtk/src/settings_dialog.rs:650` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the box `Hydrus reported no tag services.`, compared by the differential. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 215 | `gui/settings_dialog.py:1871` | message | Fetch tag services / Hydrus reported no tag services. | `crates/hatate-gtk/src/settings_dialog.rs:654` | **differential**: the box (title and text) is compared with Python's by the transport differential (`hydrus-fetch-none`, and the random cases), 0 differing. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 216 | `gui/settings_dialog.py:1893` | message | Hydrus connection test to %s: %s | `crates/hatate-gtk/src/settings_tests.rs:531` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label `Connected ✓`, asserted at `crates/hatate-gtk/tests/smoke.rs:9057`. `crates/hatate-gtk/tests/smoke.rs:9057` |
| 217 | `gui/settings_dialog.py:2368` | tooltip | Live - this permission is already on. | `crates/hatate-gtk/src/settings_mcp.rs:39` | **op**: `s_set` op, `m_press` op; `settings-mcp-tiers-*`. `crates/hatate-gtk/tests/smoke.rs:8040` |
| 218 | `gui/settings_dialog.py:2658` | message | Shortcut conflict | `crates/hatate-gtk/src/shortcuts_page.rs:25` (asserted at `crates/hatate-gtk/tests/smoke.rs:8314`) | **op**: `s_ok` op, `s_cancel` op compare config.json after every step. `crates/hatate-gtk/tests/smoke.rs:7613` |
| 219 | `gui/settings_dialog.py:1865` | message | Fetch tag services failed: %s | `crates/hatate-gtk/src/settings_dialog.rs:642` | **differential**: the box is `Could not fetch services: ` and `user_message()`, D-G (`docs/gtk-6h2.md` section 2); the lead-in is compared, 0 differing. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 220 | `gui/settings_dialog.py:1866` | message | Fetch tag services | `crates/hatate-gtk/src/settings_dialog.rs:639` | **differential**: the box title is compared with Python's by the transport differential, 0 differing. `crates/hatate-gtk/tests/smoke.rs:9227` |
| 221 | `gui/settings_dialog.py:1896` | message | Hydrus connection test to %s failed: %s | `crates/hatate-gtk/src/settings_tests.rs:536` | **by design**: Python's logger line carries the URL and the result text; Rust writes a `tracing` event with no URL or address (DAN-2116), not asserted; what the window shows is the label `Failed: ...`, asserted at `crates/hatate-gtk/tests/smoke.rs:9043`. `crates/hatate-gtk/tests/smoke.rs:9043` |
| 222 | `gui/table_context_menu.py:418` | menu | Search with a specific engine | `crates/hatate-core/src/table_menu.rs:141` | **differential**: the direct differential `table_menu` compares every label, enabled flag, tooltip and handler of the context menu over 163,153 cases per seed (`docs/queue-view.md:82`); the open menu is `queue-menu-*`. `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:3751` |
| 223 | `gui/table_context_menu.py:403` | menu | Select by Status | `crates/hatate-core/src/table_menu.rs:148` | **differential**: the direct differential `table_menu` compares every label, enabled flag, tooltip and handler of the context menu over 163,153 cases per seed (`docs/queue-view.md:82`); the open menu is `queue-menu-*`. `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:3751` |
| 224 | `gui/table_context_menu.py:404` | menu | Select by Sent State | `crates/hatate-core/src/table_menu.rs:148` | **differential**: the direct differential `table_menu` compares every label, enabled flag, tooltip and handler of the context menu over 163,153 cases per seed (`docs/queue-view.md:82`); the open menu is `queue-menu-*`. `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:3751` |
| 225 | `gui/table_context_menu.py:405` | menu | Select by Cache State | `crates/hatate-core/src/table_menu.rs:148` | **differential**: the direct differential `table_menu` compares every label, enabled flag, tooltip and handler of the context menu over 163,153 cases per seed (`docs/queue-view.md:82`); the open menu is `queue-menu-*`. `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:3751` |
| 226 | `gui/table_context_menu.py:406` | menu | Select by Review State | `crates/hatate-core/src/table_cells.rs:92` | **differential**: the direct differential `table_menu` compares every label, enabled flag, tooltip and handler of the context menu over 163,153 cases per seed (`docs/queue-view.md:82`); the open menu is `queue-menu-*`. `crates/hatate-core/src/table_menu.rs:729`, `crates/hatate-gtk/tests/smoke.rs:3751` |


## 8. Tab order

`shot --tabs OUTDIR` (`crates/hatate-gtk/examples/shot.rs`) walks Tab from the top of each surface in `TAB_STATES`, dark and light, and for every stop draws the window twice, once with the focus taken away and once with the stop focused and the ring showing; the pixels inside the stop's bounds plus 6 px are compared, and a stop where none changes has **no visible indicator** and fails the run. The crop of each stop with its ring is written as `<state>-<scheme>-tab<n>.png` (`worktrees/DAN-2223/tabs/` on the author's host; the hosted `shots` artifact carries the same files at the head). The table lists every stop, in Tab order, with the changed pixel count in each scheme and the file that shows it. Host GTK is 4.24 and hosted `ci` runs 4.14, so a stop count here is the host's; the verifier's walk at the head is the one that counts.

| # | Surface | Stop | Widget | Changed px (dark / light) | Shows the indicator |
|---|---|---|---|---|---|
| 1 | `empty` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `empty-dark-tab0.png`, `empty-light-tab0.png` |
| 2 | `empty` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `empty-dark-tab1.png`, `empty-light-tab1.png` |
| 3 | `empty` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `empty-dark-tab2.png`, `empty-light-tab2.png` |
| 4 | `empty` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `empty-dark-tab3.png`, `empty-light-tab3.png` |
| 5 | `empty` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `empty-dark-tab4.png`, `empty-light-tab4.png` |
| 6 | `empty` | 5 | GtkButton [text-button.pill.caps.primary] Add files… | 724 / 724 | `empty-dark-tab5.png`, `empty-light-tab5.png` |
| 7 | `empty` | 6 | GtkButton [text-button.pill.caps] Add folder… | 796 / 796 | `empty-dark-tab6.png`, `empty-light-tab6.png` |
| 8 | `empty` | 7 | GtkButton [text-button.pill.caps] Query Hydrus… | 896 / 896 | `empty-dark-tab7.png`, `empty-light-tab7.png` |
| 9 | `rows` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `rows-dark-tab0.png`, `rows-light-tab0.png` |
| 10 | `rows` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `rows-dark-tab1.png`, `rows-light-tab1.png` |
| 11 | `rows` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `rows-dark-tab2.png`, `rows-light-tab2.png` |
| 12 | `rows` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `rows-dark-tab3.png`, `rows-light-tab3.png` |
| 13 | `rows` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `rows-dark-tab4.png`, `rows-light-tab4.png` |
| 14 | `rows` | 5 | GtkText [] Filename contains… | 1184 / 1184 | `rows-dark-tab5.png`, `rows-light-tab5.png` |
| 15 | `rows` | 6 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `rows-dark-tab6.png`, `rows-light-tab6.png` |
| 16 | `rows` | 7 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `rows-dark-tab7.png`, `rows-light-tab7.png` |
| 17 | `rows` | 8 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `rows-dark-tab8.png`, `rows-light-tab8.png` |
| 18 | `rows` | 9 | GtkColumnViewRowWidget [activatable] Thumbnail unavailable — the fi... | 4146 / 4146 | `rows-dark-tab9.png`, `rows-light-tab9.png` |
| 19 | `queue-selected` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `queue-selected-dark-tab0.png`, `queue-selected-light-tab0.png` |
| 20 | `queue-selected` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `queue-selected-dark-tab1.png`, `queue-selected-light-tab1.png` |
| 21 | `queue-selected` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `queue-selected-dark-tab2.png`, `queue-selected-light-tab2.png` |
| 22 | `queue-selected` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `queue-selected-dark-tab3.png`, `queue-selected-light-tab3.png` |
| 23 | `queue-selected` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `queue-selected-dark-tab4.png`, `queue-selected-light-tab4.png` |
| 24 | `queue-selected` | 5 | GtkText [] Filename contains… | 1184 / 1184 | `queue-selected-dark-tab5.png`, `queue-selected-light-tab5.png` |
| 25 | `queue-selected` | 6 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `queue-selected-dark-tab6.png`, `queue-selected-light-tab6.png` |
| 26 | `queue-selected` | 7 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `queue-selected-dark-tab7.png`, `queue-selected-light-tab7.png` |
| 27 | `queue-selected` | 8 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `queue-selected-dark-tab8.png`, `queue-selected-light-tab8.png` |
| 28 | `queue-selected` | 9 | GtkColumnViewRowWidget [activatable] synthetic-0000.png | 4146 / 4146 | `queue-selected-dark-tab9.png`, `queue-selected-light-tab9.png` |
| 29 | `queue-filter` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `queue-filter-dark-tab0.png`, `queue-filter-light-tab0.png` |
| 30 | `queue-filter` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `queue-filter-dark-tab1.png`, `queue-filter-light-tab1.png` |
| 31 | `queue-filter` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `queue-filter-dark-tab2.png`, `queue-filter-light-tab2.png` |
| 32 | `queue-filter` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `queue-filter-dark-tab3.png`, `queue-filter-light-tab3.png` |
| 33 | `queue-filter` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `queue-filter-dark-tab4.png`, `queue-filter-light-tab4.png` |
| 34 | `queue-filter` | 5 | GtkText [] Filename contains… | 3025 / 3029 | `queue-filter-dark-tab5.png`, `queue-filter-light-tab5.png` |
| 35 | `queue-filter` | 6 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `queue-filter-dark-tab6.png`, `queue-filter-light-tab6.png` |
| 36 | `queue-filter` | 7 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `queue-filter-dark-tab7.png`, `queue-filter-light-tab7.png` |
| 37 | `queue-filter` | 8 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `queue-filter-dark-tab8.png`, `queue-filter-light-tab8.png` |
| 38 | `queue-filter` | 9 | GtkButton [text-button.link.caps] Show everything again. | 417 / 417 | `queue-filter-dark-tab9.png`, `queue-filter-light-tab9.png` |
| 39 | `queue-filter` | 10 | GtkColumnViewRowWidget [activatable] synthetic-0010.png | 4146 / 4146 | `queue-filter-dark-tab10.png`, `queue-filter-light-tab10.png` |
| 40 | `crashed` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `crashed-dark-tab0.png`, `crashed-light-tab0.png` |
| 41 | `crashed` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `crashed-dark-tab1.png`, `crashed-light-tab1.png` |
| 42 | `crashed` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `crashed-dark-tab2.png`, `crashed-light-tab2.png` |
| 43 | `crashed` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `crashed-dark-tab3.png`, `crashed-light-tab3.png` |
| 44 | `crashed` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `crashed-dark-tab4.png`, `crashed-light-tab4.png` |
| 45 | `crashed` | 5 | GtkButton [text-button.pill.caps.primary] Resume Queue | 852 / 852 | `crashed-dark-tab5.png`, `crashed-light-tab5.png` |
| 46 | `crashed` | 6 | GtkButton [text-button.pill.caps] View Crash Log | 900 / 900 | `crashed-dark-tab6.png`, `crashed-light-tab6.png` |
| 47 | `crashed` | 7 | GtkButton [text-button.link.caps] Discard & start fresh | 1002 / 1004 | `crashed-dark-tab7.png`, `crashed-light-tab7.png` |
| 48 | `crashed` | 8 | GtkText [] Filename contains… | 1184 / 1184 | `crashed-dark-tab8.png`, `crashed-light-tab8.png` |
| 49 | `crashed` | 9 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `crashed-dark-tab9.png`, `crashed-light-tab9.png` |
| 50 | `crashed` | 10 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `crashed-dark-tab10.png`, `crashed-light-tab10.png` |
| 51 | `crashed` | 11 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `crashed-dark-tab11.png`, `crashed-light-tab11.png` |
| 52 | `crashed` | 12 | GtkColumnViewRowWidget [activatable] Thumbnail unavailable — the fi... | 4146 / 4146 | `crashed-dark-tab12.png`, `crashed-light-tab12.png` |
| 53 | `survived` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `survived-dark-tab0.png`, `survived-light-tab0.png` |
| 54 | `survived` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `survived-dark-tab1.png`, `survived-light-tab1.png` |
| 55 | `survived` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `survived-dark-tab2.png`, `survived-light-tab2.png` |
| 56 | `survived` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `survived-dark-tab3.png`, `survived-light-tab3.png` |
| 57 | `survived` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `survived-dark-tab4.png`, `survived-light-tab4.png` |
| 58 | `survived` | 5 | GtkButton [text-button.pill.caps.primary] Start new search | 1000 / 1000 | `survived-dark-tab5.png`, `survived-light-tab5.png` |
| 59 | `survived` | 6 | GtkButton [text-button.pill.caps] View crash log | 900 / 900 | `survived-dark-tab6.png`, `survived-light-tab6.png` |
| 60 | `survived` | 7 | GtkButton [text-button.pill.caps] Add files… | 724 / 724 | `survived-dark-tab7.png`, `survived-light-tab7.png` |
| 61 | `quota` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `quota-dark-tab0.png`, `quota-light-tab0.png` |
| 62 | `quota` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `quota-dark-tab1.png`, `quota-light-tab1.png` |
| 63 | `quota` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `quota-dark-tab2.png`, `quota-light-tab2.png` |
| 64 | `quota` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `quota-dark-tab3.png`, `quota-light-tab3.png` |
| 65 | `quota` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `quota-dark-tab4.png`, `quota-light-tab4.png` |
| 66 | `quota` | 5 | GtkButton [text-button.pill.caps.primary] Review the 4 waiting → | 1180 / 1180 | `quota-dark-tab5.png`, `quota-light-tab5.png` |
| 67 | `quota` | 6 | GtkButton [text-button.pill.caps] Continue with other engines | 1412 / 1412 | `quota-dark-tab6.png`, `quota-light-tab6.png` |
| 68 | `quota` | 7 | GtkText [] Filename contains… | 1184 / 1184 | `quota-dark-tab7.png`, `quota-light-tab7.png` |
| 69 | `quota` | 8 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `quota-dark-tab8.png`, `quota-light-tab8.png` |
| 70 | `quota` | 9 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `quota-dark-tab9.png`, `quota-light-tab9.png` |
| 71 | `quota` | 10 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `quota-dark-tab10.png`, `quota-light-tab10.png` |
| 72 | `quota` | 11 | GtkColumnViewRowWidget [activatable] Thumbnail unavailable — the fi... | 4146 / 4146 | `quota-dark-tab11.png`, `quota-light-tab11.png` |
| 73 | `failed` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `failed-dark-tab0.png`, `failed-light-tab0.png` |
| 74 | `failed` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `failed-dark-tab1.png`, `failed-light-tab1.png` |
| 75 | `failed` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `failed-dark-tab2.png`, `failed-light-tab2.png` |
| 76 | `failed` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `failed-dark-tab3.png`, `failed-light-tab3.png` |
| 77 | `failed` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `failed-dark-tab4.png`, `failed-light-tab4.png` |
| 78 | `failed` | 5 | GtkButton [text-button.pill.caps.primary] Resume Queue | 852 / 852 | `failed-dark-tab5.png`, `failed-light-tab5.png` |
| 79 | `failed` | 6 | GtkButton [text-button.pill.caps] View Log | 652 / 652 | `failed-dark-tab6.png`, `failed-light-tab6.png` |
| 80 | `failed` | 7 | GtkButton [text-button.link.caps] Dismiss | 514 / 516 | `failed-dark-tab7.png`, `failed-light-tab7.png` |
| 81 | `failed` | 8 | GtkText [] Filename contains… | 1184 / 1184 | `failed-dark-tab8.png`, `failed-light-tab8.png` |
| 82 | `failed` | 9 | GtkToggleButton [text-button.arrow-button.toggle] Status: All | 700 / 700 | `failed-dark-tab9.png`, `failed-light-tab9.png` |
| 83 | `failed` | 10 | GtkToggleButton [text-button.arrow-button.toggle] Site: All | 620 / 620 | `failed-dark-tab10.png`, `failed-light-tab10.png` |
| 84 | `failed` | 11 | GtkToggleButton [text-button.arrow-button.toggle] Upscale: All | 740 / 740 | `failed-dark-tab11.png`, `failed-light-tab11.png` |
| 85 | `failed` | 12 | GtkColumnViewRowWidget [activatable] Thumbnail unavailable — the fi... | 4146 / 4146 | `failed-dark-tab12.png`, `failed-light-tab12.png` |
| 86 | `activity` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `activity-dark-tab0.png`, `activity-light-tab0.png` |
| 87 | `activity` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `activity-dark-tab1.png`, `activity-light-tab1.png` |
| 88 | `activity` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `activity-dark-tab2.png`, `activity-light-tab2.png` |
| 89 | `activity` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `activity-dark-tab3.png`, `activity-light-tab3.png` |
| 90 | `activity` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `activity-dark-tab4.png`, `activity-light-tab4.png` |
| 91 | `activity` | 5 | GtkButton [text-button.pill.caps] Parser health… | 916 / 916 | `activity-dark-tab5.png`, `activity-light-tab5.png` |
| 92 | `activity` | 6 | GtkButton [text-button.pill.caps] Clear search cache… | 1128 / 1128 | `activity-dark-tab6.png`, `activity-light-tab6.png` |
| 93 | `activity` | 7 | GtkButton [text-button.icon-boxed] Switch to Queue and focus the list. | 388 / 388 | `activity-dark-tab7.png`, `activity-light-tab7.png` |
| 94 | `activity` | 8 | GtkToggleButton [text-button.toggle.icon-boxed] Keep the log up to ... | 400 / 400 | `activity-dark-tab8.png`, `activity-light-tab8.png` |
| 95 | `activity` | 9 | GtkButton [text-button.icon-boxed] Copy the whole log. | 388 / 388 | `activity-dark-tab9.png`, `activity-light-tab9.png` |
| 96 | `activity` | 10 | GtkButton [text-button.icon-boxed] Open the folder the log file is in. | 396 / 396 | `activity-dark-tab10.png`, `activity-light-tab10.png` |
| 97 | `activity` | 11 | GtkTextView [view.monospace.log] | 4756 / 4756 | `activity-dark-tab11.png`, `activity-light-tab11.png` |
| 98 | `logs` | 0 | GtkLabel [log-path] Log file: <profile dir> | 13119 / 13119 | `logs-dark-tab0.png`, `logs-light-tab0.png` |
| 99 | `logs` | 1 | GtkTextView [view.monospace.log] | 4384 / 4384 | `logs-dark-tab1.png`, `logs-light-tab1.png` |
| 100 | `logs` | 2 | GtkButton [text-button.pill.plain] Refresh | 564 / 564 | `logs-dark-tab2.png`, `logs-light-tab2.png` |
| 101 | `logs` | 3 | GtkButton [text-button.pill.plain] Copy all | 568 / 568 | `logs-dark-tab3.png`, `logs-light-tab3.png` |
| 102 | `logs` | 4 | GtkButton [text-button.pill.plain] Open log file location | 952 / 952 | `logs-dark-tab4.png`, `logs-light-tab4.png` |
| 103 | `logs` | 5 | GtkButton [text-button.pill.plain] View Crash Log | 776 / 776 | `logs-dark-tab5.png`, `logs-light-tab5.png` |
| 104 | `logs` | 6 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `logs-dark-tab6.png`, `logs-light-tab6.png` |
| 105 | `parser-health` | 0 | GtkLabel [] animepictures | 2387 / 2389 | `parser-health-dark-tab0.png`, `parser-health-light-tab0.png` |
| 106 | `parser-health` | 1 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab1.png`, `parser-health-light-tab1.png` |
| 107 | `parser-health` | 2 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab2.png`, `parser-health-light-tab2.png` |
| 108 | `parser-health` | 3 | GtkLabel [] danbooru | 1783 / 1784 | `parser-health-dark-tab3.png`, `parser-health-light-tab3.png` |
| 109 | `parser-health` | 4 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab4.png`, `parser-health-light-tab4.png` |
| 110 | `parser-health` | 5 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab5.png`, `parser-health-light-tab5.png` |
| 111 | `parser-health` | 6 | GtkLabel [] deviantart | 1889 / 1891 | `parser-health-dark-tab6.png`, `parser-health-light-tab6.png` |
| 112 | `parser-health` | 7 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab7.png`, `parser-health-light-tab7.png` |
| 113 | `parser-health` | 8 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab8.png`, `parser-health-light-tab8.png` |
| 114 | `parser-health` | 9 | GtkLabel [] e621 | 1065 / 1065 | `parser-health-dark-tab9.png`, `parser-health-light-tab9.png` |
| 115 | `parser-health` | 10 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab10.png`, `parser-health-light-tab10.png` |
| 116 | `parser-health` | 11 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab11.png`, `parser-health-light-tab11.png` |
| 117 | `parser-health` | 12 | GtkLabel [] eshuushuu | 1946 / 1950 | `parser-health-dark-tab12.png`, `parser-health-light-tab12.png` |
| 118 | `parser-health` | 13 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab13.png`, `parser-health-light-tab13.png` |
| 119 | `parser-health` | 14 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab14.png`, `parser-health-light-tab14.png` |
| 120 | `parser-health` | 15 | GtkLabel [] gelbooru | 1667 / 1668 | `parser-health-dark-tab15.png`, `parser-health-light-tab15.png` |
| 121 | `parser-health` | 16 | GtkLabel [] not used yet | 2234 / 2236 | `parser-health-dark-tab16.png`, `parser-health-light-tab16.png` |
| 122 | `parser-health` | 17 | GtkLabel [] | 524 / 524 | `parser-health-dark-tab17.png`, `parser-health-light-tab17.png` |
| 123 | `parser-health` | 18 | GtkLabel [] moebooru | 1733 / 1736 | `parser-health-dark-tab18.png`, `parser-health-light-tab18.png` |
| 124 | `parser-health` | 19 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab19.png`, `parser-health-light-tab19.png` |
| 125 | `parser-health` | 20 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab20.png`, `parser-health-light-tab20.png` |
| 126 | `parser-health` | 21 | GtkLabel [] pawchive | 1563 / 1575 | `parser-health-dark-tab21.png`, `parser-health-light-tab21.png` |
| 127 | `parser-health` | 22 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab22.png`, `parser-health-light-tab22.png` |
| 128 | `parser-health` | 23 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab23.png`, `parser-health-light-tab23.png` |
| 129 | `parser-health` | 24 | GtkLabel [] rule34 | 1225 / 1236 | `parser-health-dark-tab24.png`, `parser-health-light-tab24.png` |
| 130 | `parser-health` | 25 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab25.png`, `parser-health-light-tab25.png` |
| 131 | `parser-health` | 26 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab26.png`, `parser-health-light-tab26.png` |
| 132 | `parser-health` | 27 | GtkLabel [] safebooru | 1630 / 1653 | `parser-health-dark-tab27.png`, `parser-health-light-tab27.png` |
| 133 | `parser-health` | 28 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab28.png`, `parser-health-light-tab28.png` |
| 134 | `parser-health` | 29 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab29.png`, `parser-health-light-tab29.png` |
| 135 | `parser-health` | 30 | GtkLabel [] twitter | 1272 / 1280 | `parser-health-dark-tab30.png`, `parser-health-light-tab30.png` |
| 136 | `parser-health` | 31 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab31.png`, `parser-health-light-tab31.png` |
| 137 | `parser-health` | 32 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab32.png`, `parser-health-light-tab32.png` |
| 138 | `parser-health` | 33 | GtkLabel [] xbooru | 1222 / 1224 | `parser-health-dark-tab33.png`, `parser-health-light-tab33.png` |
| 139 | `parser-health` | 34 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab34.png`, `parser-health-light-tab34.png` |
| 140 | `parser-health` | 35 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab35.png`, `parser-health-light-tab35.png` |
| 141 | `parser-health` | 36 | GtkLabel [] zerochan | 1512 / 1512 | `parser-health-dark-tab36.png`, `parser-health-light-tab36.png` |
| 142 | `parser-health` | 37 | GtkLabel [] not used yet | 2012 / 2014 | `parser-health-dark-tab37.png`, `parser-health-light-tab37.png` |
| 143 | `parser-health` | 38 | GtkLabel [] | 302 / 302 | `parser-health-dark-tab38.png`, `parser-health-light-tab38.png` |
| 144 | `parser-health` | 39 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `parser-health-dark-tab39.png`, `parser-health-light-tab39.png` |
| 145 | `review` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `review-dark-tab0.png`, `review-light-tab0.png` |
| 146 | `review` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `review-dark-tab1.png`, `review-light-tab1.png` |
| 147 | `review` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `review-dark-tab2.png`, `review-light-tab2.png` |
| 148 | `review` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `review-dark-tab3.png`, `review-light-tab3.png` |
| 149 | `review` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `review-dark-tab4.png`, `review-light-tab4.png` |
| 150 | `review` | 5 | GtkToggleButton [text-button.toggle.caps] Your copy and the match, ... | 608 / 608 | `review-dark-tab5.png`, `review-light-tab5.png` |
| 151 | `review` | 6 | GtkToggleButton [text-button.toggle.caps] The two in one frame, spl... | 332 / 332 | `review-dark-tab6.png`, `review-light-tab6.png` |
| 152 | `review` | 7 | GtkToggleButton [text-button.toggle.caps] Both pictures side by sid... | 612 / 612 | `review-dark-tab7.png`, `review-light-tab7.png` |
| 153 | `review` | 8 | GtkButton [text-button.icon-boxed] Open the comparison in its own w... | 368 / 368 | `review-dark-tab8.png`, `review-light-tab8.png` |
| 154 | `review` | 9 | GtkButton [text-button.icon-boxed] Show this image in the Queue list. | 384 / 384 | `review-dark-tab9.png`, `review-light-tab9.png` |
| 155 | `review` | 10 | GtkButton [text-button.icon-boxed] Next match candidate (]) | 344 / 344 | `review-dark-tab10.png`, `review-light-tab10.png` |
| 156 | `review` | 11 | GtkToggleButton [toggle] (None) | 1740 / 1740 | `review-dark-tab11.png`, `review-light-tab11.png` |
| 157 | `review` | 12 | GtkListBoxRow [activatable] [Booru] solo | 12624 / 12624 | `review-dark-tab12.png`, `review-light-tab12.png` |
| 158 | `review` | 13 | GtkListBoxRow [activatable] [User] character:reimu | 14140 / 14140 | `review-dark-tab13.png`, `review-light-tab13.png` |
| 159 | `review` | 14 | GtkListBoxRow [activatable] [Hydrus] sketch | 14164 / 14164 | `review-dark-tab14.png`, `review-light-tab14.png` |
| 160 | `review` | 15 | GtkButton [text-button.pill.plain] Add tags… | 572 / 572 | `review-dark-tab15.png`, `review-light-tab15.png` |
| 161 | `review` | 16 | GtkButton [text-button.pill.plain] Remove | 572 / 572 | `review-dark-tab16.png`, `review-light-tab16.png` |
| 162 | `review` | 17 | GtkToggleButton [text-button.toggle.pill.plain] When on, double-cli... | 572 / 572 | `review-dark-tab17.png`, `review-light-tab17.png` |
| 163 | `review` | 18 | GtkButton [pill.primary] Send the file, its URL and its tags to Hyd... | 1620 / 1620 | `review-dark-tab18.png`, `review-light-tab18.png` |
| 164 | `review` | 19 | GtkButton [pill] Hand the URL to Hydrus's own downloader. | 836 / 836 | `review-dark-tab19.png`, `review-light-tab19.png` |
| 165 | `review` | 20 | GtkButton [pill] Download the full-resolution match and send that t... | 832 / 832 | `review-dark-tab20.png`, `review-light-tab20.png` |
| 166 | `review` | 21 | GtkButton [pill] Records that you looked at this one and decided ag... | 908 / 908 | `review-dark-tab21.png`, `review-light-tab21.png` |
| 167 | `review` | 22 | GtkButton [pill] Open the matched page in your browser. | 904 / 904 | `review-dark-tab22.png`, `review-light-tab22.png` |
| 168 | `review` | 23 | GtkButton [pill] Skip ahead to the next image you haven't sent or m... | 1488 / 1488 | `review-dark-tab23.png`, `review-light-tab23.png` |
| 169 | `review` | 24 | GtkButton [pill] Next image. | 1088 / 1088 | `review-dark-tab24.png`, `review-light-tab24.png` |
| 170 | `review-nomatch` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `review-nomatch-dark-tab0.png`, `review-nomatch-light-tab0.png` |
| 171 | `review-nomatch` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `review-nomatch-dark-tab1.png`, `review-nomatch-light-tab1.png` |
| 172 | `review-nomatch` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `review-nomatch-dark-tab2.png`, `review-nomatch-light-tab2.png` |
| 173 | `review-nomatch` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `review-nomatch-dark-tab3.png`, `review-nomatch-light-tab3.png` |
| 174 | `review-nomatch` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `review-nomatch-dark-tab4.png`, `review-nomatch-light-tab4.png` |
| 175 | `review-nomatch` | 5 | GtkToggleButton [text-button.toggle.caps] Your copy and the match, ... | 608 / 608 | `review-nomatch-dark-tab5.png`, `review-nomatch-light-tab5.png` |
| 176 | `review-nomatch` | 6 | GtkToggleButton [text-button.toggle.caps] The two in one frame, spl... | 332 / 332 | `review-nomatch-dark-tab6.png`, `review-nomatch-light-tab6.png` |
| 177 | `review-nomatch` | 7 | GtkToggleButton [text-button.toggle.caps] Both pictures side by sid... | 612 / 612 | `review-nomatch-dark-tab7.png`, `review-nomatch-light-tab7.png` |
| 178 | `review-nomatch` | 8 | GtkButton [text-button.icon-boxed] Open the comparison in its own w... | 368 / 368 | `review-nomatch-dark-tab8.png`, `review-nomatch-light-tab8.png` |
| 179 | `review-nomatch` | 9 | GtkButton [text-button.icon-boxed] Show this image in the Queue list. | 384 / 384 | `review-nomatch-dark-tab9.png`, `review-nomatch-light-tab9.png` |
| 180 | `review-nomatch` | 10 | GtkScrolledWindow [tag-scroller] | 1844 / 1844 | `review-nomatch-dark-tab10.png`, `review-nomatch-light-tab10.png` |
| 181 | `review-nomatch` | 11 | GtkButton [text-button.pill.plain] Add tags… | 512 / 512 | `review-nomatch-dark-tab11.png`, `review-nomatch-light-tab11.png` |
| 182 | `review-nomatch` | 12 | GtkButton [text-button.pill.plain] Remove | 512 / 512 | `review-nomatch-dark-tab12.png`, `review-nomatch-light-tab12.png` |
| 183 | `review-nomatch` | 13 | GtkToggleButton [text-button.toggle.pill.plain] When on, double-cli... | 512 / 512 | `review-nomatch-dark-tab13.png`, `review-nomatch-light-tab13.png` |
| 184 | `review-nomatch` | 14 | GtkButton [pill.primary] Send the file, its URL and its tags to Hyd... | 1440 / 1440 | `review-nomatch-dark-tab14.png`, `review-nomatch-light-tab14.png` |
| 185 | `review-nomatch` | 15 | GtkButton [pill] Hand the URL to Hydrus's own downloader. | 744 / 744 | `review-nomatch-dark-tab15.png`, `review-nomatch-light-tab15.png` |
| 186 | `review-nomatch` | 16 | GtkButton [pill] Download the full-resolution match and send that t... | 744 / 744 | `review-nomatch-dark-tab16.png`, `review-nomatch-light-tab16.png` |
| 187 | `review-nomatch` | 17 | GtkButton [pill] Records that you looked at this one and decided ag... | 816 / 816 | `review-nomatch-dark-tab17.png`, `review-nomatch-light-tab17.png` |
| 188 | `review-nomatch` | 18 | GtkButton [pill] Open the matched page in your browser. | 816 / 816 | `review-nomatch-dark-tab18.png`, `review-nomatch-light-tab18.png` |
| 189 | `review-nomatch` | 19 | GtkButton [pill] Previous image. | 1252 / 1252 | `review-nomatch-dark-tab19.png`, `review-nomatch-light-tab19.png` |
| 190 | `review-nomatch` | 20 | GtkButton [pill] Skip ahead to the next image you haven't sent or m... | 1488 / 1488 | `review-nomatch-dark-tab20.png`, `review-nomatch-light-tab20.png` |
| 191 | `review-nomatch` | 21 | GtkButton [pill] Next image. | 1088 / 1088 | `review-nomatch-dark-tab21.png`, `review-nomatch-light-tab21.png` |
| 192 | `review-picker` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `review-picker-dark-tab0.png`, `review-picker-light-tab0.png` (popover: not judged) |
| 193 | `review-picker` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `review-picker-dark-tab1.png`, `review-picker-light-tab1.png` (popover: not judged) |
| 194 | `review-picker` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `review-picker-dark-tab2.png`, `review-picker-light-tab2.png` (popover: not judged) |
| 195 | `review-picker` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `review-picker-dark-tab3.png`, `review-picker-light-tab3.png` (popover: not judged) |
| 196 | `review-picker` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `review-picker-dark-tab4.png`, `review-picker-light-tab4.png` (popover: not judged) |
| 197 | `review-picker` | 5 | GtkToggleButton [text-button.toggle.caps] Your copy and the match, ... | 608 / 608 | `review-picker-dark-tab5.png`, `review-picker-light-tab5.png` (popover: not judged) |
| 198 | `review-picker` | 6 | GtkToggleButton [text-button.toggle.caps] The two in one frame, spl... | 332 / 332 | `review-picker-dark-tab6.png`, `review-picker-light-tab6.png` (popover: not judged) |
| 199 | `review-picker` | 7 | GtkToggleButton [text-button.toggle.caps] Both pictures side by sid... | 612 / 612 | `review-picker-dark-tab7.png`, `review-picker-light-tab7.png` (popover: not judged) |
| 200 | `review-picker` | 8 | GtkButton [text-button.icon-boxed] Open the comparison in its own w... | 368 / 368 | `review-picker-dark-tab8.png`, `review-picker-light-tab8.png` (popover: not judged) |
| 201 | `review-picker` | 9 | GtkButton [text-button.icon-boxed] Show this image in the Queue list. | 384 / 384 | `review-picker-dark-tab9.png`, `review-picker-light-tab9.png` (popover: not judged) |
| 202 | `review-picker` | 10 | GtkButton [text-button.icon-boxed] Next match candidate (]) | 344 / 344 | `review-picker-dark-tab10.png`, `review-picker-light-tab10.png` (popover: not judged) |
| 203 | `review-picker` | 11 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab11.png`, `review-picker-light-tab11.png` (popover: not judged) |
| 204 | `review-picker` | 12 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab12.png`, `review-picker-light-tab12.png` (popover: not judged) |
| 205 | `review-picker` | 13 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab13.png`, `review-picker-light-tab13.png` (popover: not judged) |
| 206 | `review-picker` | 14 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab14.png`, `review-picker-light-tab14.png` (popover: not judged) |
| 207 | `review-picker` | 15 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab15.png`, `review-picker-light-tab15.png` (popover: not judged) |
| 208 | `review-picker` | 16 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab16.png`, `review-picker-light-tab16.png` (popover: not judged) |
| 209 | `review-picker` | 17 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab17.png`, `review-picker-light-tab17.png` (popover: not judged) |
| 210 | `review-picker` | 18 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab18.png`, `review-picker-light-tab18.png` (popover: not judged) |
| 211 | `review-picker` | 19 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab19.png`, `review-picker-light-tab19.png` (popover: not judged) |
| 212 | `review-picker` | 20 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab20.png`, `review-picker-light-tab20.png` (popover: not judged) |
| 213 | `review-picker` | 21 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab21.png`, `review-picker-light-tab21.png` (popover: not judged) |
| 214 | `review-picker` | 22 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab22.png`, `review-picker-light-tab22.png` (popover: not judged) |
| 215 | `review-picker` | 23 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab23.png`, `review-picker-light-tab23.png` (popover: not judged) |
| 216 | `review-picker` | 24 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab24.png`, `review-picker-light-tab24.png` (popover: not judged) |
| 217 | `review-picker` | 25 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab25.png`, `review-picker-light-tab25.png` (popover: not judged) |
| 218 | `review-picker` | 26 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab26.png`, `review-picker-light-tab26.png` (popover: not judged) |
| 219 | `review-picker` | 27 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab27.png`, `review-picker-light-tab27.png` (popover: not judged) |
| 220 | `review-picker` | 28 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab28.png`, `review-picker-light-tab28.png` (popover: not judged) |
| 221 | `review-picker` | 29 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab29.png`, `review-picker-light-tab29.png` (popover: not judged) |
| 222 | `review-picker` | 30 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab30.png`, `review-picker-light-tab30.png` (popover: not judged) |
| 223 | `review-picker` | 31 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab31.png`, `review-picker-light-tab31.png` (popover: not judged) |
| 224 | `review-picker` | 32 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab32.png`, `review-picker-light-tab32.png` (popover: not judged) |
| 225 | `review-picker` | 33 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab33.png`, `review-picker-light-tab33.png` (popover: not judged) |
| 226 | `review-picker` | 34 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab34.png`, `review-picker-light-tab34.png` (popover: not judged) |
| 227 | `review-picker` | 35 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab35.png`, `review-picker-light-tab35.png` (popover: not judged) |
| 228 | `review-picker` | 36 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab36.png`, `review-picker-light-tab36.png` (popover: not judged) |
| 229 | `review-picker` | 37 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab37.png`, `review-picker-light-tab37.png` (popover: not judged) |
| 230 | `review-picker` | 38 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab38.png`, `review-picker-light-tab38.png` (popover: not judged) |
| 231 | `review-picker` | 39 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab39.png`, `review-picker-light-tab39.png` (popover: not judged) |
| 232 | `review-picker` | 40 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab40.png`, `review-picker-light-tab40.png` (popover: not judged) |
| 233 | `review-picker` | 41 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab41.png`, `review-picker-light-tab41.png` (popover: not judged) |
| 234 | `review-picker` | 42 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab42.png`, `review-picker-light-tab42.png` (popover: not judged) |
| 235 | `review-picker` | 43 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab43.png`, `review-picker-light-tab43.png` (popover: not judged) |
| 236 | `review-picker` | 44 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab44.png`, `review-picker-light-tab44.png` (popover: not judged) |
| 237 | `review-picker` | 45 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab45.png`, `review-picker-light-tab45.png` (popover: not judged) |
| 238 | `review-picker` | 46 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab46.png`, `review-picker-light-tab46.png` (popover: not judged) |
| 239 | `review-picker` | 47 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab47.png`, `review-picker-light-tab47.png` (popover: not judged) |
| 240 | `review-picker` | 48 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab48.png`, `review-picker-light-tab48.png` (popover: not judged) |
| 241 | `review-picker` | 49 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab49.png`, `review-picker-light-tab49.png` (popover: not judged) |
| 242 | `review-picker` | 50 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab50.png`, `review-picker-light-tab50.png` (popover: not judged) |
| 243 | `review-picker` | 51 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab51.png`, `review-picker-light-tab51.png` (popover: not judged) |
| 244 | `review-picker` | 52 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab52.png`, `review-picker-light-tab52.png` (popover: not judged) |
| 245 | `review-picker` | 53 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab53.png`, `review-picker-light-tab53.png` (popover: not judged) |
| 246 | `review-picker` | 54 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab54.png`, `review-picker-light-tab54.png` (popover: not judged) |
| 247 | `review-picker` | 55 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab55.png`, `review-picker-light-tab55.png` (popover: not judged) |
| 248 | `review-picker` | 56 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab56.png`, `review-picker-light-tab56.png` (popover: not judged) |
| 249 | `review-picker` | 57 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab57.png`, `review-picker-light-tab57.png` (popover: not judged) |
| 250 | `review-picker` | 58 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab58.png`, `review-picker-light-tab58.png` (popover: not judged) |
| 251 | `review-picker` | 59 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab59.png`, `review-picker-light-tab59.png` (popover: not judged) |
| 252 | `review-picker` | 60 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab60.png`, `review-picker-light-tab60.png` (popover: not judged) |
| 253 | `review-picker` | 61 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab61.png`, `review-picker-light-tab61.png` (popover: not judged) |
| 254 | `review-picker` | 62 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab62.png`, `review-picker-light-tab62.png` (popover: not judged) |
| 255 | `review-picker` | 63 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab63.png`, `review-picker-light-tab63.png` (popover: not judged) |
| 256 | `review-picker` | 64 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab64.png`, `review-picker-light-tab64.png` (popover: not judged) |
| 257 | `review-picker` | 65 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab65.png`, `review-picker-light-tab65.png` (popover: not judged) |
| 258 | `review-picker` | 66 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab66.png`, `review-picker-light-tab66.png` (popover: not judged) |
| 259 | `review-picker` | 67 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab67.png`, `review-picker-light-tab67.png` (popover: not judged) |
| 260 | `review-picker` | 68 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab68.png`, `review-picker-light-tab68.png` (popover: not judged) |
| 261 | `review-picker` | 69 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab69.png`, `review-picker-light-tab69.png` (popover: not judged) |
| 262 | `review-picker` | 70 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab70.png`, `review-picker-light-tab70.png` (popover: not judged) |
| 263 | `review-picker` | 71 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab71.png`, `review-picker-light-tab71.png` (popover: not judged) |
| 264 | `review-picker` | 72 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab72.png`, `review-picker-light-tab72.png` (popover: not judged) |
| 265 | `review-picker` | 73 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab73.png`, `review-picker-light-tab73.png` (popover: not judged) |
| 266 | `review-picker` | 74 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab74.png`, `review-picker-light-tab74.png` (popover: not judged) |
| 267 | `review-picker` | 75 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab75.png`, `review-picker-light-tab75.png` (popover: not judged) |
| 268 | `review-picker` | 76 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab76.png`, `review-picker-light-tab76.png` (popover: not judged) |
| 269 | `review-picker` | 77 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab77.png`, `review-picker-light-tab77.png` (popover: not judged) |
| 270 | `review-picker` | 78 | GtkListItemWidget [activatable] Gelbooru — 80% (IQDB) — https://gel... | 0 / 0 | `review-picker-dark-tab78.png`, `review-picker-light-tab78.png` (popover: not judged) |
| 271 | `review-picker` | 79 | GtkListItemWidget [activatable] Danbooru — 96% (IQDB) — https://dan... | 0 / 0 | `review-picker-dark-tab79.png`, `review-picker-light-tab79.png` (popover: not judged) |
| 272 | `review-wipe` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `review-wipe-dark-tab0.png`, `review-wipe-light-tab0.png` |
| 273 | `review-wipe` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `review-wipe-dark-tab1.png`, `review-wipe-light-tab1.png` |
| 274 | `review-wipe` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `review-wipe-dark-tab2.png`, `review-wipe-light-tab2.png` |
| 275 | `review-wipe` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `review-wipe-dark-tab3.png`, `review-wipe-light-tab3.png` |
| 276 | `review-wipe` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `review-wipe-dark-tab4.png`, `review-wipe-light-tab4.png` |
| 277 | `review-wipe` | 5 | GtkToggleButton [text-button.toggle.caps] Your copy and the match, ... | 608 / 608 | `review-wipe-dark-tab5.png`, `review-wipe-light-tab5.png` |
| 278 | `review-wipe` | 6 | GtkToggleButton [text-button.toggle.caps] The two in one frame, spl... | 332 / 332 | `review-wipe-dark-tab6.png`, `review-wipe-light-tab6.png` |
| 279 | `review-wipe` | 7 | GtkToggleButton [text-button.toggle.caps] Both pictures side by sid... | 612 / 612 | `review-wipe-dark-tab7.png`, `review-wipe-light-tab7.png` |
| 280 | `review-wipe` | 8 | GtkButton [text-button.icon-boxed] Open the comparison in its own w... | 368 / 368 | `review-wipe-dark-tab8.png`, `review-wipe-light-tab8.png` |
| 281 | `review-wipe` | 9 | GtkButton [text-button.icon-boxed] Show this image in the Queue list. | 384 / 384 | `review-wipe-dark-tab9.png`, `review-wipe-light-tab9.png` |
| 282 | `review-wipe` | 10 | GtkButton [text-button.icon-boxed] Zoom out (-) | 344 / 344 | `review-wipe-dark-tab10.png`, `review-wipe-light-tab10.png` |
| 283 | `review-wipe` | 11 | GtkButton [text-button.icon-boxed] Zoom in (+) | 344 / 344 | `review-wipe-dark-tab11.png`, `review-wipe-light-tab11.png` |
| 284 | `review-wipe` | 12 | GtkButton [text-button.icon-boxed] Fit the whole image (0, or doubl... | 380 / 380 | `review-wipe-dark-tab12.png`, `review-wipe-light-tab12.png` |
| 285 | `review-wipe` | 13 | GtkButton [text-button.icon-boxed] One image pixel per screen pixel... | 456 / 456 | `review-wipe-dark-tab13.png`, `review-wipe-light-tab13.png` |
| 286 | `review-wipe` | 14 | GtkDrawingArea [wipe-view] Scroll to zoom where the pointer is, dra... | 3416 / 3416 | `review-wipe-dark-tab14.png`, `review-wipe-light-tab14.png` |
| 287 | `review-wipe` | 15 | GtkScale [horizontal.wipe-slider] Drag to sweep the wipe across the... | 2428 / 2428 | `review-wipe-dark-tab15.png`, `review-wipe-light-tab15.png` |
| 288 | `review-wipe` | 16 | GtkButton [text-button.icon-boxed] Next match candidate (]) | 344 / 344 | `review-wipe-dark-tab16.png`, `review-wipe-light-tab16.png` |
| 289 | `review-wipe` | 17 | GtkToggleButton [toggle] (None) | 1740 / 1740 | `review-wipe-dark-tab17.png`, `review-wipe-light-tab17.png` |
| 290 | `review-wipe` | 18 | GtkListBoxRow [activatable] [Booru] solo | 12624 / 12624 | `review-wipe-dark-tab18.png`, `review-wipe-light-tab18.png` |
| 291 | `review-wipe` | 19 | GtkListBoxRow [activatable] [User] character:reimu | 14140 / 14140 | `review-wipe-dark-tab19.png`, `review-wipe-light-tab19.png` |
| 292 | `review-wipe` | 20 | GtkListBoxRow [activatable] [Hydrus] sketch | 14164 / 14164 | `review-wipe-dark-tab20.png`, `review-wipe-light-tab20.png` |
| 293 | `review-wipe` | 21 | GtkButton [text-button.pill.plain] Add tags… | 572 / 572 | `review-wipe-dark-tab21.png`, `review-wipe-light-tab21.png` |
| 294 | `review-wipe` | 22 | GtkButton [text-button.pill.plain] Remove | 572 / 572 | `review-wipe-dark-tab22.png`, `review-wipe-light-tab22.png` |
| 295 | `review-wipe` | 23 | GtkToggleButton [text-button.toggle.pill.plain] When on, double-cli... | 572 / 572 | `review-wipe-dark-tab23.png`, `review-wipe-light-tab23.png` |
| 296 | `review-wipe` | 24 | GtkButton [pill.primary] Send the file, its URL and its tags to Hyd... | 1620 / 1620 | `review-wipe-dark-tab24.png`, `review-wipe-light-tab24.png` |
| 297 | `review-wipe` | 25 | GtkButton [pill] Hand the URL to Hydrus's own downloader. | 836 / 836 | `review-wipe-dark-tab25.png`, `review-wipe-light-tab25.png` |
| 298 | `review-wipe` | 26 | GtkButton [pill] Download the full-resolution match and send that t... | 832 / 832 | `review-wipe-dark-tab26.png`, `review-wipe-light-tab26.png` |
| 299 | `review-wipe` | 27 | GtkButton [pill] Records that you looked at this one and decided ag... | 908 / 908 | `review-wipe-dark-tab27.png`, `review-wipe-light-tab27.png` |
| 300 | `review-wipe` | 28 | GtkButton [pill] Open the matched page in your browser. | 904 / 904 | `review-wipe-dark-tab28.png`, `review-wipe-light-tab28.png` |
| 301 | `review-wipe` | 29 | GtkButton [pill] Skip ahead to the next image you haven't sent or m... | 1488 / 1488 | `review-wipe-dark-tab29.png`, `review-wipe-light-tab29.png` |
| 302 | `review-wipe` | 30 | GtkButton [pill] Next image. | 1088 / 1088 | `review-wipe-dark-tab30.png`, `review-wipe-light-tab30.png` |
| 303 | `review-differences` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `review-differences-dark-tab0.png`, `review-differences-light-tab0.png` |
| 304 | `review-differences` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `review-differences-dark-tab1.png`, `review-differences-light-tab1.png` |
| 305 | `review-differences` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `review-differences-dark-tab2.png`, `review-differences-light-tab2.png` |
| 306 | `review-differences` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `review-differences-dark-tab3.png`, `review-differences-light-tab3.png` |
| 307 | `review-differences` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `review-differences-dark-tab4.png`, `review-differences-light-tab4.png` |
| 308 | `review-differences` | 5 | GtkToggleButton [text-button.toggle.caps] Your copy and the match, ... | 608 / 608 | `review-differences-dark-tab5.png`, `review-differences-light-tab5.png` |
| 309 | `review-differences` | 6 | GtkToggleButton [text-button.toggle.caps] The two in one frame, spl... | 332 / 332 | `review-differences-dark-tab6.png`, `review-differences-light-tab6.png` |
| 310 | `review-differences` | 7 | GtkToggleButton [text-button.toggle.caps] Both pictures side by sid... | 612 / 612 | `review-differences-dark-tab7.png`, `review-differences-light-tab7.png` |
| 311 | `review-differences` | 8 | GtkButton [text-button.icon-boxed] Open the comparison in its own w... | 368 / 368 | `review-differences-dark-tab8.png`, `review-differences-light-tab8.png` |
| 312 | `review-differences` | 9 | GtkButton [text-button.icon-boxed] Show this image in the Queue list. | 384 / 384 | `review-differences-dark-tab9.png`, `review-differences-light-tab9.png` |
| 313 | `review-differences` | 10 | GtkButton [text-button.icon-boxed] Zoom out (-) | 344 / 344 | `review-differences-dark-tab10.png`, `review-differences-light-tab10.png` |
| 314 | `review-differences` | 11 | GtkButton [text-button.icon-boxed] Zoom in (+) | 344 / 344 | `review-differences-dark-tab11.png`, `review-differences-light-tab11.png` |
| 315 | `review-differences` | 12 | GtkButton [text-button.icon-boxed] Fit the whole image (0, or doubl... | 380 / 380 | `review-differences-dark-tab12.png`, `review-differences-light-tab12.png` |
| 316 | `review-differences` | 13 | GtkButton [text-button.icon-boxed] One image pixel per screen pixel... | 456 / 456 | `review-differences-dark-tab13.png`, `review-differences-light-tab13.png` |
| 317 | `review-differences` | 14 | GtkDrawingArea [wipe-view] Scroll to zoom where the pointer is, dra... | 3340 / 3340 | `review-differences-dark-tab14.png`, `review-differences-light-tab14.png` |
| 318 | `review-differences` | 15 | GtkScale [horizontal.wipe-slider] Drag to sweep the wipe across the... | 2428 / 2428 | `review-differences-dark-tab15.png`, `review-differences-light-tab15.png` |
| 319 | `review-differences` | 16 | GtkButton [text-button.icon-boxed] Next match candidate (]) | 344 / 344 | `review-differences-dark-tab16.png`, `review-differences-light-tab16.png` |
| 320 | `review-differences` | 17 | GtkToggleButton [toggle] (None) | 1740 / 1740 | `review-differences-dark-tab17.png`, `review-differences-light-tab17.png` |
| 321 | `review-differences` | 18 | GtkListBoxRow [activatable] [Booru] solo | 12624 / 12624 | `review-differences-dark-tab18.png`, `review-differences-light-tab18.png` |
| 322 | `review-differences` | 19 | GtkListBoxRow [activatable] [User] character:reimu | 14140 / 14140 | `review-differences-dark-tab19.png`, `review-differences-light-tab19.png` |
| 323 | `review-differences` | 20 | GtkListBoxRow [activatable] [Hydrus] sketch | 14164 / 14164 | `review-differences-dark-tab20.png`, `review-differences-light-tab20.png` |
| 324 | `review-differences` | 21 | GtkButton [text-button.pill.plain] Add tags… | 572 / 572 | `review-differences-dark-tab21.png`, `review-differences-light-tab21.png` |
| 325 | `review-differences` | 22 | GtkButton [text-button.pill.plain] Remove | 572 / 572 | `review-differences-dark-tab22.png`, `review-differences-light-tab22.png` |
| 326 | `review-differences` | 23 | GtkToggleButton [text-button.toggle.pill.plain] When on, double-cli... | 572 / 572 | `review-differences-dark-tab23.png`, `review-differences-light-tab23.png` |
| 327 | `review-differences` | 24 | GtkButton [pill.primary] Send the file, its URL and its tags to Hyd... | 1620 / 1620 | `review-differences-dark-tab24.png`, `review-differences-light-tab24.png` |
| 328 | `review-differences` | 25 | GtkButton [pill] Hand the URL to Hydrus's own downloader. | 836 / 836 | `review-differences-dark-tab25.png`, `review-differences-light-tab25.png` |
| 329 | `review-differences` | 26 | GtkButton [pill] Download the full-resolution match and send that t... | 832 / 832 | `review-differences-dark-tab26.png`, `review-differences-light-tab26.png` |
| 330 | `review-differences` | 27 | GtkButton [pill] Records that you looked at this one and decided ag... | 908 / 908 | `review-differences-dark-tab27.png`, `review-differences-light-tab27.png` |
| 331 | `review-differences` | 28 | GtkButton [pill] Open the matched page in your browser. | 904 / 904 | `review-differences-dark-tab28.png`, `review-differences-light-tab28.png` |
| 332 | `review-differences` | 29 | GtkButton [pill] Skip ahead to the next image you haven't sent or m... | 1488 / 1488 | `review-differences-dark-tab29.png`, `review-differences-light-tab29.png` |
| 333 | `review-differences` | 30 | GtkButton [pill] Next image. | 1088 / 1088 | `review-differences-dark-tab30.png`, `review-differences-light-tab30.png` |
| 334 | `review-compare` | 0 | GtkDrawingArea [wipe-view] Scroll to zoom where the pointer is, dra... | 5392 / 5392 | `review-compare-dark-tab0.png`, `review-compare-light-tab0.png` |
| 335 | `review-compare` | 1 | GtkCheckButton [text-button.grouped] Wipe | 778 / 780 | `review-compare-dark-tab1.png`, `review-compare-light-tab1.png` |
| 336 | `review-compare` | 2 | GtkButton [text-button.icon-boxed] Zoom out (-) | 344 / 344 | `review-compare-dark-tab2.png`, `review-compare-light-tab2.png` |
| 337 | `review-compare` | 3 | GtkButton [text-button.icon-boxed] Zoom in (+) | 344 / 344 | `review-compare-dark-tab3.png`, `review-compare-light-tab3.png` |
| 338 | `review-compare` | 4 | GtkButton [text-button.icon-boxed] Fit the whole image in the windo... | 380 / 380 | `review-compare-dark-tab4.png`, `review-compare-light-tab4.png` |
| 339 | `review-compare` | 5 | GtkButton [text-button.icon-boxed] One image pixel per screen pixel... | 456 / 456 | `review-compare-dark-tab5.png`, `review-compare-light-tab5.png` |
| 340 | `review-compare` | 6 | GtkScale [horizontal.wipe-slider] | 3352 / 3352 | `review-compare-dark-tab6.png`, `review-compare-light-tab6.png` |
| 341 | `review-compare` | 7 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `review-compare-dark-tab7.png`, `review-compare-light-tab7.png` |
| 342 | `hydrus-empty` | 0 | GtkToggleButton [text-button.toggle.caps] The list: add images, fil... | 540 / 540 | `hydrus-empty-dark-tab0.png`, `hydrus-empty-light-tab0.png` |
| 343 | `hydrus-empty` | 1 | GtkToggleButton [text-button.toggle.caps] One image at a time, your... | 572 / 572 | `hydrus-empty-dark-tab1.png`, `hydrus-empty-light-tab1.png` |
| 344 | `hydrus-empty` | 2 | GtkToggleButton [text-button.toggle.caps] How the run is going, wha... | 620 / 620 | `hydrus-empty-dark-tab2.png`, `hydrus-empty-light-tab2.png` |
| 345 | `hydrus-empty` | 3 | GtkButton [text-button.pill.caps.primary] ▶ Start Search | 904 / 904 | `hydrus-empty-dark-tab3.png`, `hydrus-empty-light-tab3.png` |
| 346 | `hydrus-empty` | 4 | GtkButton [text-button.icon-boxed] Preferences… | 392 / 392 | `hydrus-empty-dark-tab4.png`, `hydrus-empty-light-tab4.png` |
| 347 | `hydrus-empty` | 5 | GtkButton [text-button.pill.caps.primary] Add files… | 724 / 724 | `hydrus-empty-dark-tab5.png`, `hydrus-empty-light-tab5.png` |
| 348 | `hydrus-empty` | 6 | GtkButton [text-button.pill.caps] Add folder… | 796 / 796 | `hydrus-empty-dark-tab6.png`, `hydrus-empty-light-tab6.png` |
| 349 | `hydrus-empty` | 7 | GtkButton [text-button.pill.caps] Query Hydrus… | 896 / 896 | `hydrus-empty-dark-tab7.png`, `hydrus-empty-light-tab7.png` |
| 350 | `hydrus-query` | 0 | GtkTextView [view.monospace] | 3348 / 3348 | `hydrus-query-dark-tab0.png`, `hydrus-query-light-tab0.png` |
| 351 | `hydrus-query` | 1 | GtkButton [text-button.pill.plain] Search Hydrus | 760 / 760 | `hydrus-query-dark-tab1.png`, `hydrus-query-light-tab1.png` |
| 352 | `hydrus-query` | 2 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `hydrus-query-dark-tab2.png`, `hydrus-query-light-tab2.png` |
| 353 | `hydrus-query-results` | 0 | GtkTextView [view.monospace] | 3348 / 3348 | `hydrus-query-results-dark-tab0.png`, `hydrus-query-results-light-tab0.png` |
| 354 | `hydrus-query-results` | 1 | GtkButton [text-button.pill.plain] Search Hydrus | 760 / 760 | `hydrus-query-results-dark-tab1.png`, `hydrus-query-results-light-tab1.png` |
| 355 | `hydrus-query-results` | 2 | GtkListBoxRow [activatable] 00000002cccc… (image/png, 800x600, 7 ta... | 1986 / 1986 | `hydrus-query-results-dark-tab2.png`, `hydrus-query-results-light-tab2.png` |
| 356 | `hydrus-query-results` | 3 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `hydrus-query-results-dark-tab3.png`, `hydrus-query-results-light-tab3.png` |
| 357 | `hydrus-query-results` | 4 | GtkButton [text-button.pill.primary.plain] Import selected | 800 / 800 | `hydrus-query-results-dark-tab4.png`, `hydrus-query-results-light-tab4.png` |
| 358 | `pawchive-artists` | 0 | GtkListBoxRow [activatable] Another Synthetic | 2206 / 2206 | `pawchive-artists-dark-tab0.png`, `pawchive-artists-light-tab0.png` |
| 359 | `pawchive-artists` | 1 | GtkListBoxRow [activatable] Synthetic Artist | 22122 / 22170 | `pawchive-artists-dark-tab1.png`, `pawchive-artists-light-tab1.png` |
| 360 | `pawchive-artists` | 2 | GtkListBoxRow [activatable] Third Synthetic | 22156 / 22192 | `pawchive-artists-dark-tab2.png`, `pawchive-artists-light-tab2.png` |
| 361 | `pawchive-artists` | 3 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab3.png`, `pawchive-artists-light-tab3.png` |
| 362 | `pawchive-artists` | 4 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab4.png`, `pawchive-artists-light-tab4.png` |
| 363 | `pawchive-artists` | 5 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab5.png`, `pawchive-artists-light-tab5.png` |
| 364 | `pawchive-artists` | 6 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab6.png`, `pawchive-artists-light-tab6.png` |
| 365 | `pawchive-artists` | 7 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab7.png`, `pawchive-artists-light-tab7.png` |
| 366 | `pawchive-artists` | 8 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab8.png`, `pawchive-artists-light-tab8.png` |
| 367 | `pawchive-artists` | 9 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab9.png`, `pawchive-artists-light-tab9.png` |
| 368 | `pawchive-artists` | 10 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab10.png`, `pawchive-artists-light-tab10.png` |
| 369 | `pawchive-artists` | 11 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab11.png`, `pawchive-artists-light-tab11.png` |
| 370 | `pawchive-artists` | 12 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab12.png`, `pawchive-artists-light-tab12.png` |
| 371 | `pawchive-artists` | 13 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab13.png`, `pawchive-artists-light-tab13.png` |
| 372 | `pawchive-artists` | 14 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab14.png`, `pawchive-artists-light-tab14.png` |
| 373 | `pawchive-artists` | 15 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab15.png`, `pawchive-artists-light-tab15.png` |
| 374 | `pawchive-artists` | 16 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab16.png`, `pawchive-artists-light-tab16.png` |
| 375 | `pawchive-artists` | 17 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab17.png`, `pawchive-artists-light-tab17.png` |
| 376 | `pawchive-artists` | 18 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab18.png`, `pawchive-artists-light-tab18.png` |
| 377 | `pawchive-artists` | 19 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab19.png`, `pawchive-artists-light-tab19.png` |
| 378 | `pawchive-artists` | 20 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab20.png`, `pawchive-artists-light-tab20.png` |
| 379 | `pawchive-artists` | 21 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab21.png`, `pawchive-artists-light-tab21.png` |
| 380 | `pawchive-artists` | 22 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab22.png`, `pawchive-artists-light-tab22.png` |
| 381 | `pawchive-artists` | 23 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab23.png`, `pawchive-artists-light-tab23.png` |
| 382 | `pawchive-artists` | 24 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab24.png`, `pawchive-artists-light-tab24.png` |
| 383 | `pawchive-artists` | 25 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab25.png`, `pawchive-artists-light-tab25.png` |
| 384 | `pawchive-artists` | 26 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab26.png`, `pawchive-artists-light-tab26.png` |
| 385 | `pawchive-artists` | 27 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab27.png`, `pawchive-artists-light-tab27.png` |
| 386 | `pawchive-artists` | 28 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab28.png`, `pawchive-artists-light-tab28.png` |
| 387 | `pawchive-artists` | 29 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab29.png`, `pawchive-artists-light-tab29.png` |
| 388 | `pawchive-artists` | 30 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab30.png`, `pawchive-artists-light-tab30.png` |
| 389 | `pawchive-artists` | 31 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab31.png`, `pawchive-artists-light-tab31.png` |
| 390 | `pawchive-artists` | 32 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab32.png`, `pawchive-artists-light-tab32.png` |
| 391 | `pawchive-artists` | 33 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab33.png`, `pawchive-artists-light-tab33.png` |
| 392 | `pawchive-artists` | 34 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab34.png`, `pawchive-artists-light-tab34.png` |
| 393 | `pawchive-artists` | 35 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab35.png`, `pawchive-artists-light-tab35.png` |
| 394 | `pawchive-artists` | 36 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab36.png`, `pawchive-artists-light-tab36.png` |
| 395 | `pawchive-artists` | 37 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab37.png`, `pawchive-artists-light-tab37.png` |
| 396 | `pawchive-artists` | 38 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab38.png`, `pawchive-artists-light-tab38.png` |
| 397 | `pawchive-artists` | 39 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab39.png`, `pawchive-artists-light-tab39.png` |
| 398 | `pawchive-artists` | 40 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab40.png`, `pawchive-artists-light-tab40.png` |
| 399 | `pawchive-artists` | 41 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab41.png`, `pawchive-artists-light-tab41.png` |
| 400 | `pawchive-artists` | 42 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab42.png`, `pawchive-artists-light-tab42.png` |
| 401 | `pawchive-artists` | 43 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab43.png`, `pawchive-artists-light-tab43.png` |
| 402 | `pawchive-artists` | 44 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab44.png`, `pawchive-artists-light-tab44.png` |
| 403 | `pawchive-artists` | 45 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab45.png`, `pawchive-artists-light-tab45.png` |
| 404 | `pawchive-artists` | 46 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab46.png`, `pawchive-artists-light-tab46.png` |
| 405 | `pawchive-artists` | 47 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab47.png`, `pawchive-artists-light-tab47.png` |
| 406 | `pawchive-artists` | 48 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab48.png`, `pawchive-artists-light-tab48.png` |
| 407 | `pawchive-artists` | 49 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab49.png`, `pawchive-artists-light-tab49.png` |
| 408 | `pawchive-artists` | 50 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab50.png`, `pawchive-artists-light-tab50.png` |
| 409 | `pawchive-artists` | 51 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab51.png`, `pawchive-artists-light-tab51.png` |
| 410 | `pawchive-artists` | 52 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab52.png`, `pawchive-artists-light-tab52.png` |
| 411 | `pawchive-artists` | 53 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab53.png`, `pawchive-artists-light-tab53.png` |
| 412 | `pawchive-artists` | 54 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab54.png`, `pawchive-artists-light-tab54.png` |
| 413 | `pawchive-artists` | 55 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab55.png`, `pawchive-artists-light-tab55.png` |
| 414 | `pawchive-artists` | 56 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab56.png`, `pawchive-artists-light-tab56.png` |
| 415 | `pawchive-artists` | 57 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab57.png`, `pawchive-artists-light-tab57.png` |
| 416 | `pawchive-artists` | 58 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab58.png`, `pawchive-artists-light-tab58.png` |
| 417 | `pawchive-artists` | 59 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab59.png`, `pawchive-artists-light-tab59.png` |
| 418 | `pawchive-artists` | 60 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab60.png`, `pawchive-artists-light-tab60.png` |
| 419 | `pawchive-artists` | 61 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab61.png`, `pawchive-artists-light-tab61.png` |
| 420 | `pawchive-artists` | 62 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab62.png`, `pawchive-artists-light-tab62.png` |
| 421 | `pawchive-artists` | 63 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab63.png`, `pawchive-artists-light-tab63.png` |
| 422 | `pawchive-artists` | 64 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab64.png`, `pawchive-artists-light-tab64.png` |
| 423 | `pawchive-artists` | 65 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab65.png`, `pawchive-artists-light-tab65.png` |
| 424 | `pawchive-artists` | 66 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab66.png`, `pawchive-artists-light-tab66.png` |
| 425 | `pawchive-artists` | 67 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab67.png`, `pawchive-artists-light-tab67.png` |
| 426 | `pawchive-artists` | 68 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab68.png`, `pawchive-artists-light-tab68.png` |
| 427 | `pawchive-artists` | 69 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab69.png`, `pawchive-artists-light-tab69.png` |
| 428 | `pawchive-artists` | 70 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab70.png`, `pawchive-artists-light-tab70.png` |
| 429 | `pawchive-artists` | 71 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab71.png`, `pawchive-artists-light-tab71.png` |
| 430 | `pawchive-artists` | 72 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab72.png`, `pawchive-artists-light-tab72.png` |
| 431 | `pawchive-artists` | 73 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab73.png`, `pawchive-artists-light-tab73.png` |
| 432 | `pawchive-artists` | 74 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab74.png`, `pawchive-artists-light-tab74.png` |
| 433 | `pawchive-artists` | 75 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab75.png`, `pawchive-artists-light-tab75.png` |
| 434 | `pawchive-artists` | 76 | GtkButton [text-button.pill.plain] Stops after the current request.... | 476 / 476 | `pawchive-artists-dark-tab76.png`, `pawchive-artists-light-tab76.png` |
| 435 | `pawchive-artists` | 77 | GtkButton [text-button.pill.plain] Close | 496 / 496 | `pawchive-artists-dark-tab77.png`, `pawchive-artists-light-tab77.png` |
| 436 | `pawchive-artists` | 78 | GtkListBoxRow [activatable] Third Synthetic | 2930 / 2930 | `pawchive-artists-dark-tab78.png`, `pawchive-artists-light-tab78.png` |
| 437 | `pawchive-artists` | 79 | GtkText [] Artist name, or a pawchive link | 1332 / 1332 | `pawchive-artists-dark-tab79.png`, `pawchive-artists-light-tab79.png` |
| 438 | `settings-0` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-0-dark-tab0.png`, `settings-0-light-tab0.png` |
| 439 | `settings-0` | 1 | GtkNotebook [frame] General | 6192 / 6192 | `settings-0-dark-tab1.png`, `settings-0-light-tab1.png` |
| 440 | `settings-0` | 2 | GtkText [] | 1942 / 1940 | `settings-0-dark-tab2.png`, `settings-0-light-tab2.png` |
| 441 | `settings-0` | 3 | GtkText [] | 1949 / 1946 | `settings-0-dark-tab3.png`, `settings-0-light-tab3.png` |
| 442 | `settings-0` | 4 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab4.png`, `settings-0-light-tab4.png` |
| 443 | `settings-0` | 5 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab5.png`, `settings-0-light-tab5.png` |
| 444 | `settings-0` | 6 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab6.png`, `settings-0-light-tab6.png` |
| 445 | `settings-0` | 7 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab7.png`, `settings-0-light-tab7.png` |
| 446 | `settings-0` | 8 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab8.png`, `settings-0-light-tab8.png` |
| 447 | `settings-0` | 9 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab9.png`, `settings-0-light-tab9.png` |
| 448 | `settings-0` | 10 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab10.png`, `settings-0-light-tab10.png` |
| 449 | `settings-0` | 11 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab11.png`, `settings-0-light-tab11.png` |
| 450 | `settings-0` | 12 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab12.png`, `settings-0-light-tab12.png` |
| 451 | `settings-0` | 13 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab13.png`, `settings-0-light-tab13.png` |
| 452 | `settings-0` | 14 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab14.png`, `settings-0-light-tab14.png` |
| 453 | `settings-0` | 15 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab15.png`, `settings-0-light-tab15.png` |
| 454 | `settings-0` | 16 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab16.png`, `settings-0-light-tab16.png` |
| 455 | `settings-0` | 17 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab17.png`, `settings-0-light-tab17.png` |
| 456 | `settings-0` | 18 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab18.png`, `settings-0-light-tab18.png` |
| 457 | `settings-0` | 19 | GtkText [] | 3103 / 3123 | `settings-0-dark-tab19.png`, `settings-0-light-tab19.png` |
| 458 | `settings-0` | 20 | GtkText [] | 3146 / 3913 | `settings-0-dark-tab20.png`, `settings-0-light-tab20.png` |
| 459 | `settings-0` | 21 | GtkText [] | 2731 / 2751 | `settings-0-dark-tab21.png`, `settings-0-light-tab21.png` |
| 460 | `settings-0` | 22 | GtkText [] | 2733 / 2865 | `settings-0-dark-tab22.png`, `settings-0-light-tab22.png` |
| 461 | `settings-0` | 23 | GtkText [] | 2731 / 2751 | `settings-0-dark-tab23.png`, `settings-0-light-tab23.png` |
| 462 | `settings-0` | 24 | GtkToggleButton [toggle] (None) | 6370 / 13633 | `settings-0-dark-tab24.png`, `settings-0-light-tab24.png` |
| 463 | `settings-0` | 25 | GtkToggleButton [toggle] (None) | 6645 / 7269 | `settings-0-dark-tab25.png`, `settings-0-light-tab25.png` |
| 464 | `settings-0` | 26 | GtkCheckButton [text-button] Generates thumbnails for the rows actu... | 7485 / 22636 | `settings-0-dark-tab26.png`, `settings-0-light-tab26.png` |
| 465 | `settings-0` | 27 | GtkText [] | 3741 / 21759 | `settings-0-dark-tab27.png`, `settings-0-light-tab27.png` |
| 466 | `settings-0` | 28 | GtkCheckButton [text-button] Saves the working list when you close ... | 4952 / 22049 | `settings-0-dark-tab28.png`, `settings-0-light-tab28.png` |
| 467 | `settings-0` | 29 | GtkCheckButton [text-button] Saves the working list periodically, n... | 4250 / 4212 | `settings-0-dark-tab29.png`, `settings-0-light-tab29.png` |
| 468 | `settings-0` | 30 | GtkText [] | 4980 / 24162 | `settings-0-dark-tab30.png`, `settings-0-light-tab30.png` |
| 469 | `settings-0` | 31 | GtkCheckButton [text-button] Log matched URLs to a text file | 5187 / 23848 | `settings-0-dark-tab31.png`, `settings-0-light-tab31.png` |
| 470 | `settings-0` | 32 | GtkText [] | 5638 / 8047 | `settings-0-dark-tab32.png`, `settings-0-light-tab32.png` |
| 471 | `settings-0` | 33 | GtkButton [text-button] Browse… | 406 / 408 | `settings-0-dark-tab33.png`, `settings-0-light-tab33.png` |
| 472 | `settings-0` | 34 | GtkListBoxRow [activatable] User | 11109 / 14881 | `settings-0-dark-tab34.png`, `settings-0-light-tab34.png` |
| 473 | `settings-0` | 35 | GtkCheckButton [text-button] User | 2924 / 2926 | `settings-0-dark-tab35.png`, `settings-0-light-tab35.png` |
| 474 | `settings-0` | 36 | GtkListBoxRow [activatable] Hydrus | 4036 / 4041 | `settings-0-dark-tab36.png`, `settings-0-light-tab36.png` |
| 475 | `settings-0` | 37 | GtkCheckButton [text-button] Hydrus | 2924 / 2926 | `settings-0-dark-tab37.png`, `settings-0-light-tab37.png` |
| 476 | `settings-0` | 38 | GtkListBoxRow [activatable] Search engine | 4717 / 4735 | `settings-0-dark-tab38.png`, `settings-0-light-tab38.png` |
| 477 | `settings-0` | 39 | GtkCheckButton [text-button] Search engine | 2924 / 2926 | `settings-0-dark-tab39.png`, `settings-0-light-tab39.png` |
| 478 | `settings-0` | 40 | GtkListBoxRow [activatable] Booru | 4691 / 4707 | `settings-0-dark-tab40.png`, `settings-0-light-tab40.png` |
| 479 | `settings-0` | 41 | GtkCheckButton [text-button] Booru | 2924 / 2926 | `settings-0-dark-tab41.png`, `settings-0-light-tab41.png` |
| 480 | `settings-0` | 42 | GtkListBoxRow [activatable] Hatate-linux | 4233 / 4242 | `settings-0-dark-tab42.png`, `settings-0-light-tab42.png` |
| 481 | `settings-0` | 43 | GtkCheckButton [text-button] Hatate-linux | 2924 / 2926 | `settings-0-dark-tab43.png`, `settings-0-light-tab43.png` |
| 482 | `settings-0` | 44 | GtkCheckButton [text-button] Off by default, and left off for exist... | 6215 / 6188 | `settings-0-dark-tab44.png`, `settings-0-light-tab44.png` |
| 483 | `settings-0` | 45 | GtkToggleButton [toggle] (None) | 5750 / 19655 | `settings-0-dark-tab45.png`, `settings-0-light-tab45.png` |
| 484 | `settings-0` | 46 | GtkToggleButton [toggle] (None) | 5835 / 6531 | `settings-0-dark-tab46.png`, `settings-0-light-tab46.png` |
| 485 | `settings-0` | 47 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-0-dark-tab47.png`, `settings-0-light-tab47.png` |
| 486 | `settings-0` | 48 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-0-dark-tab48.png`, `settings-0-light-tab48.png` |
| 487 | `settings-1` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-1-dark-tab0.png`, `settings-1-light-tab0.png` |
| 488 | `settings-1` | 1 | GtkNotebook [frame] General | 6172 / 6172 | `settings-1-dark-tab1.png`, `settings-1-light-tab1.png` |
| 489 | `settings-1` | 2 | GtkText [] | 6029 / 6024 | `settings-1-dark-tab2.png`, `settings-1-light-tab2.png` |
| 490 | `settings-1` | 3 | GtkToggleButton [toggle] (None) | 2312 / 2312 | `settings-1-dark-tab3.png`, `settings-1-light-tab3.png` |
| 491 | `settings-1` | 4 | GtkToggleButton [toggle] (None) | 2312 / 2312 | `settings-1-dark-tab4.png`, `settings-1-light-tab4.png` |
| 492 | `settings-1` | 5 | GtkText [] | 4425 / 4423 | `settings-1-dark-tab5.png`, `settings-1-light-tab5.png` |
| 493 | `settings-1` | 6 | GtkCheckButton [text-button] Holds ascii2d, trace.moe, IQDB 3D, Goo... | 3765 / 3765 | `settings-1-dark-tab6.png`, `settings-1-light-tab6.png` |
| 494 | `settings-1` | 7 | GtkCheckButton [text-button] ascii2d.net colour and feature search,... | 3765 / 3765 | `settings-1-dark-tab7.png`, `settings-1-light-tab7.png` |
| 495 | `settings-1` | 8 | GtkCheckButton [text-button] Identifies which anime a screenshot is... | 3765 / 3765 | `settings-1-dark-tab8.png`, `settings-1-light-tab8.png` |
| 496 | `settings-1` | 9 | GtkText [] | 4192 / 13443 | `settings-1-dark-tab9.png`, `settings-1-light-tab9.png` |
| 497 | `settings-1` | 10 | GtkCheckButton [text-button] Same protocol as IQDB, against the 3D/... | 5839 / 23618 | `settings-1-dark-tab10.png`, `settings-1-light-tab10.png` |
| 498 | `settings-1` | 11 | GtkCheckButton [text-button] Google's reverse image search, run in ... | 5089 / 5033 | `settings-1-dark-tab11.png`, `settings-1-light-tab11.png` |
| 499 | `settings-1` | 12 | GtkText [] | 3377 / 19138 | `settings-1-dark-tab12.png`, `settings-1-light-tab12.png` |
| 500 | `settings-1` | 13 | GtkCheckButton [text-button] The reverse image search you get by dr... | 5272 / 23285 | `settings-1-dark-tab13.png`, `settings-1-light-tab13.png` |
| 501 | `settings-1` | 14 | GtkCheckButton [text-button] Google Lens chooses a "search area" ou... | 5558 / 5518 | `settings-1-dark-tab14.png`, `settings-1-light-tab14.png` |
| 502 | `settings-1` | 15 | GtkCheckButton [text-button] yandex.com's reverse image search, ove... | 6258 / 6218 | `settings-1-dark-tab15.png`, `settings-1-light-tab15.png` |
| 503 | `settings-1` | 16 | GtkCheckButton [text-button] Asks pawchive.pw whether it holds this... | 6138 / 6065 | `settings-1-dark-tab16.png`, `settings-1-light-tab16.png` |
| 504 | `settings-1` | 17 | GtkCheckButton [text-button] Compares each picture against the pawc... | 6278 / 6233 | `settings-1-dark-tab17.png`, `settings-1-light-tab17.png` |
| 505 | `settings-1` | 18 | GtkCheckButton [text-button] Skips a fresh IQDB/SauceNAO request if... | 5584 / 5766 | `settings-1-dark-tab18.png`, `settings-1-light-tab18.png` |
| 506 | `settings-1` | 19 | GtkText [] | 4751 / 20747 | `settings-1-dark-tab19.png`, `settings-1-light-tab19.png` |
| 507 | `settings-1` | 20 | GtkCheckButton [text-button] Retrieve tags from matched booru page | 5921 / 24061 | `settings-1-dark-tab20.png`, `settings-1-light-tab20.png` |
| 508 | `settings-1` | 21 | GtkCheckButton [text-button] Orders the match dropdown so its first... | 5217 / 5171 | `settings-1-dark-tab21.png`, `settings-1-light-tab21.png` |
| 509 | `settings-1` | 22 | GtkCheckButton [text-button] While searching, if a match's source p... | 5500 / 5443 | `settings-1-dark-tab22.png`, `settings-1-light-tab22.png` |
| 510 | `settings-1` | 23 | GtkCheckButton [text-button] Some sites list a post but hold it bac... | 5257 / 5194 | `settings-1-dark-tab23.png`, `settings-1-light-tab23.png` |
| 511 | `settings-1` | 24 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-1-dark-tab24.png`, `settings-1-light-tab24.png` |
| 512 | `settings-1` | 25 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-1-dark-tab25.png`, `settings-1-light-tab25.png` |
| 513 | `settings-2` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-2-dark-tab0.png`, `settings-2-light-tab0.png` |
| 514 | `settings-2` | 1 | GtkNotebook [frame] General | 6168 / 6168 | `settings-2-dark-tab1.png`, `settings-2-light-tab1.png` |
| 515 | `settings-2` | 2 | GtkCheckButton [text-button] Some sites cannot supply tags at all. ... | 3814 / 3816 | `settings-2-dark-tab2.png`, `settings-2-light-tab2.png` |
| 516 | `settings-2` | 3 | GtkText [] | 7666 / 7650 | `settings-2-dark-tab3.png`, `settings-2-light-tab3.png` |
| 517 | `settings-2` | 4 | GtkCheckButton [text-button] An image marked Error failed on a time... | 3814 / 3816 | `settings-2-dark-tab4.png`, `settings-2-light-tab4.png` |
| 518 | `settings-2` | 5 | GtkCheckButton [text-button] Applies to any of the Hydrus send acti... | 3814 / 3816 | `settings-2-dark-tab5.png`, `settings-2-light-tab5.png` |
| 519 | `settings-2` | 6 | GtkCheckButton [text-button] As each image in a batch is searched, ... | 3814 / 3816 | `settings-2-dark-tab6.png`, `settings-2-light-tab6.png` |
| 520 | `settings-2` | 7 | GtkText [] | 5430 / 5426 | `settings-2-dark-tab7.png`, `settings-2-light-tab7.png` |
| 521 | `settings-2` | 8 | GtkToggleButton [toggle] (None) | 2512 / 2512 | `settings-2-dark-tab8.png`, `settings-2-light-tab8.png` |
| 522 | `settings-2` | 9 | GtkText [] | 6423 / 6417 | `settings-2-dark-tab9.png`, `settings-2-light-tab9.png` |
| 523 | `settings-2` | 10 | GtkText [] | 6246 / 6241 | `settings-2-dark-tab10.png`, `settings-2-light-tab10.png` |
| 524 | `settings-2` | 11 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-2-dark-tab11.png`, `settings-2-light-tab11.png` |
| 525 | `settings-2` | 12 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-2-dark-tab12.png`, `settings-2-light-tab12.png` |
| 526 | `settings-3` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-3-dark-tab0.png`, `settings-3-light-tab0.png` |
| 527 | `settings-3` | 1 | GtkNotebook [frame] General | 6268 / 6268 | `settings-3-dark-tab1.png`, `settings-3-light-tab1.png` |
| 528 | `settings-3` | 2 | GtkText [] | 2788 / 2788 | `settings-3-dark-tab2.png`, `settings-3-light-tab2.png` |
| 529 | `settings-3` | 3 | GtkCheckButton [text-button] Use JSON API (needs a key, retrieves m... | 3814 / 3816 | `settings-3-dark-tab3.png`, `settings-3-light-tab3.png` |
| 530 | `settings-3` | 4 | GtkCheckButton [text-button] IQDB has no daily cap, so the alternat... | 3814 / 3816 | `settings-3-dark-tab4.png`, `settings-3-light-tab4.png` |
| 531 | `settings-3` | 5 | GtkCheckButton [text-button] Stops the current batch as soon as Sau... | 3814 / 3816 | `settings-3-dark-tab5.png`, `settings-3-light-tab5.png` |
| 532 | `settings-3` | 6 | GtkText [] | 5969 / 5966 | `settings-3-dark-tab6.png`, `settings-3-light-tab6.png` |
| 533 | `settings-3` | 7 | GtkText [] optional, e.g. "sn" | 2788 / 2788 | `settings-3-dark-tab7.png`, `settings-3-light-tab7.png` |
| 534 | `settings-3` | 8 | GtkListBoxRow [activatable] character | 3588 / 3588 | `settings-3-dark-tab8.png`, `settings-3-light-tab8.png` |
| 535 | `settings-3` | 9 | GtkCheckButton [text-button] character | 3798 / 3800 | `settings-3-dark-tab9.png`, `settings-3-light-tab9.png` |
| 536 | `settings-3` | 10 | GtkListBoxRow [activatable] material | 3588 / 3588 | `settings-3-dark-tab10.png`, `settings-3-light-tab10.png` |
| 537 | `settings-3` | 11 | GtkCheckButton [text-button] material | 3798 / 3800 | `settings-3-dark-tab11.png`, `settings-3-light-tab11.png` |
| 538 | `settings-3` | 12 | GtkListBoxRow [activatable] creator | 3588 / 3588 | `settings-3-dark-tab12.png`, `settings-3-light-tab12.png` |
| 539 | `settings-3` | 13 | GtkCheckButton [text-button] creator | 3798 / 3800 | `settings-3-dark-tab13.png`, `settings-3-light-tab13.png` |
| 540 | `settings-3` | 14 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-3-dark-tab14.png`, `settings-3-light-tab14.png` |
| 541 | `settings-3` | 15 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-3-dark-tab15.png`, `settings-3-light-tab15.png` |
| 542 | `settings-4` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-4-dark-tab0.png`, `settings-4-light-tab0.png` |
| 543 | `settings-4` | 1 | GtkNotebook [frame] General | 6180 / 6180 | `settings-4-dark-tab1.png`, `settings-4-light-tab1.png` |
| 544 | `settings-4` | 2 | GtkText []  | 5737 / 5728 | `settings-4-dark-tab2.png`, `settings-4-light-tab2.png` |
| 545 | `settings-4` | 3 | GtkText []  | 2796 / 2796 | `settings-4-dark-tab3.png`, `settings-4-light-tab3.png` |
| 546 | `settings-4` | 4 | GtkText [] empty = "my tags" local service | 2048 / 2048 | `settings-4-dark-tab4.png`, `settings-4-light-tab4.png` |
| 547 | `settings-4` | 5 | GtkButton [text-button] Fetch from Hydrus… | 928 / 928 | `settings-4-dark-tab5.png`, `settings-4-light-tab5.png` |
| 548 | `settings-4` | 6 | GtkCheckButton [text-button] Verify SSL certificate | 3800 / 3800 | `settings-4-dark-tab6.png`, `settings-4-light-tab6.png` |
| 549 | `settings-4` | 7 | GtkCheckButton [text-button] When "Download match ... | 3800 / 3800 | `settings-4-dark-tab7.png`, `settings-4-light-tab7.png` |
| 550 | `settings-4` | 8 | GtkCheckButton [text-button] Everything this app k... | 3800 / 3800 | `settings-4-dark-tab8.png`, `settings-4-light-tab8.png` |
| 551 | `settings-4` | 9 | GtkText [] empty = "hatate: match source" | 2796 / 2796 | `settings-4-dark-tab9.png`, `settings-4-light-tab9.png` |
| 552 | `settings-4` | 10 | GtkButton [text-button] Test connection | 812 / 812 | `settings-4-dark-tab10.png`, `settings-4-light-tab10.png` |
| 553 | `settings-4` | 11 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-4-dark-tab11.png`, `settings-4-light-tab11.png` |
| 554 | `settings-4` | 12 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-4-dark-tab12.png`, `settings-4-light-tab12.png` |
| 555 | `settings-5` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-5-dark-tab0.png`, `settings-5-light-tab0.png` |
| 556 | `settings-5` | 1 | GtkNotebook [frame] General | 6464 / 6464 | `settings-5-dark-tab1.png`, `settings-5-light-tab1.png` |
| 557 | `settings-5` | 2 | GtkCheckButton [text-button] E.g. rewrite booru sites' "artist:" to... | 3774 / 3776 | `settings-5-dark-tab2.png`, `settings-5-light-tab2.png` |
| 558 | `settings-5` | 3 | GtkTextView [view.monospace] | 5628 / 5628 | `settings-5-dark-tab3.png`, `settings-5-light-tab3.png` |
| 559 | `settings-5` | 4 | GtkCheckButton [text-button] Drops booru housekeeping tags (highres... | 3774 / 3776 | `settings-5-dark-tab4.png`, `settings-5-light-tab4.png` |
| 560 | `settings-5` | 5 | GtkTextView [view.monospace] | 6634 / 9148 | `settings-5-dark-tab5.png`, `settings-5-light-tab5.png` |
| 561 | `settings-5` | 6 | GtkCheckButton [text-button] Turns the booru's own age rating for t... | 4418 / 28651 | `settings-5-dark-tab6.png`, `settings-5-light-tab6.png` |
| 562 | `settings-5` | 7 | GtkText [] rating | 2528 / 8035 | `settings-5-dark-tab7.png`, `settings-5-light-tab7.png` |
| 563 | `settings-5` | 8 | GtkCheckButton [text-button] Hydrus has no API to report your actua... | 5351 / 11564 | `settings-5-dark-tab8.png`, `settings-5-light-tab8.png` |
| 564 | `settings-5` | 9 | GtkTextView [view.monospace] | 7124 / 63688 | `settings-5-dark-tab9.png`, `settings-5-light-tab9.png` |
| 565 | `settings-5` | 10 | GtkTextView [view.monospace] Added to every image a match was found... | 3178 / 5692 | `settings-5-dark-tab10.png`, `settings-5-light-tab10.png` |
| 566 | `settings-5` | 11 | GtkTextView [view.monospace] Added to every image no match was foun... | 3894 / 5566 | `settings-5-dark-tab11.png`, `settings-5-light-tab11.png` |
| 567 | `settings-5` | 12 | GtkTextView [view.monospace] Added on top of the found tags when th... | 3894 / 5566 | `settings-5-dark-tab12.png`, `settings-5-light-tab12.png` |
| 568 | `settings-5` | 13 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-5-dark-tab13.png`, `settings-5-light-tab13.png` |
| 569 | `settings-5` | 14 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-5-dark-tab14.png`, `settings-5-light-tab14.png` |
| 570 | `settings-6` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-6-dark-tab0.png`, `settings-6-light-tab0.png` |
| 571 | `settings-6` | 1 | GtkNotebook [frame] General | 6300 / 6300 | `settings-6-dark-tab1.png`, `settings-6-light-tab1.png` |
| 572 | `settings-6` | 2 | GtkText [] PHPSESSID value from a logged-in browser session | 2604 / 2604 | `settings-6-dark-tab2.png`, `settings-6-light-tab2.png` |
| 573 | `settings-6` | 3 | GtkButton [text-button] Runs the real parser again... | 652 / 652 | `settings-6-dark-tab3.png`, `settings-6-light-tab3.png` |
| 574 | `settings-6` | 4 | GtkText [] your e621 username | 2604 / 2604 | `settings-6-dark-tab4.png`, `settings-6-light-tab4.png` |
| 575 | `settings-6` | 5 | GtkText [] from Account > Manage API Access on e621 | 2604 / 2604 | `settings-6-dark-tab5.png`, `settings-6-light-tab5.png` |
| 576 | `settings-6` | 6 | GtkButton [text-button] Sends one authenticated re... | 812 / 812 | `settings-6-dark-tab6.png`, `settings-6-light-tab6.png` |
| 577 | `settings-6` | 7 | GtkText [] _sankakuchannel_session=...; locale=en | 2604 / 2604 | `settings-6-dark-tab7.png`, `settings-6-light-tab7.png` |
| 578 | `settings-6` | 8 | GtkButton [text-button] Fetches a Sankaku post URL... | 764 / 764 | `settings-6-dark-tab8.png`, `settings-6-light-tab8.png` |
| 579 | `settings-6` | 9 | GtkText [] anime_pictures_jwt=eyJ0eXAiOiJKV1Qi… | 2604 / 2604 | `settings-6-dark-tab9.png`, `settings-6-light-tab9.png` |
| 580 | `settings-6` | 10 | GtkButton [text-button] Fetches a post through the... | 964 / 964 | `settings-6-dark-tab10.png`, `settings-6-light-tab10.png` |
| 581 | `settings-6` | 11 | GtkText [] auth=...; auth_secure=...; userinfo=... | 4721 / 10340 | `settings-6-dark-tab11.png`, `settings-6-light-tab11.png` |
| 582 | `settings-6` | 12 | GtkButton [text-button] Fetches a deviation throug... | 5351 / 5359 | `settings-6-dark-tab12.png`, `settings-6-light-tab12.png` |
| 583 | `settings-6` | 13 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-6-dark-tab13.png`, `settings-6-light-tab13.png` |
| 584 | `settings-6` | 14 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-6-dark-tab14.png`, `settings-6-light-tab14.png` |
| 585 | `settings-7` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-7-dark-tab0.png`, `settings-7-light-tab0.png` |
| 586 | `settings-7` | 1 | GtkNotebook [frame] General | 6256 / 6256 | `settings-7-dark-tab1.png`, `settings-7-light-tab1.png` |
| 587 | `settings-7` | 2 | GtkButton [text-button.key-edit] J | 784 / 784 | `settings-7-dark-tab2.png`, `settings-7-light-tab2.png` |
| 588 | `settings-7` | 3 | GtkButton [text-button.pill.plain] Leave "Next image" with no key a... | 542 / 542 | `settings-7-dark-tab3.png`, `settings-7-light-tab3.png` |
| 589 | `settings-7` | 4 | GtkButton [text-button.key-edit] K | 784 / 784 | `settings-7-dark-tab4.png`, `settings-7-light-tab4.png` |
| 590 | `settings-7` | 5 | GtkButton [text-button.pill.plain] Leave "Previous image" with no k... | 542 / 542 | `settings-7-dark-tab5.png`, `settings-7-light-tab5.png` |
| 591 | `settings-7` | 6 | GtkButton [text-button.key-edit] N | 784 / 784 | `settings-7-dark-tab6.png`, `settings-7-light-tab6.png` |
| 592 | `settings-7` | 7 | GtkButton [text-button.pill.plain] Leave "Next image needing review... | 542 / 542 | `settings-7-dark-tab7.png`, `settings-7-light-tab7.png` |
| 593 | `settings-7` | 8 | GtkButton [text-button.key-edit] Shift+N | 784 / 784 | `settings-7-dark-tab8.png`, `settings-7-light-tab8.png` |
| 594 | `settings-7` | 9 | GtkButton [text-button.pill.plain] Leave "Previous image needing re... | 542 / 542 | `settings-7-dark-tab9.png`, `settings-7-light-tab9.png` |
| 595 | `settings-7` | 10 | GtkButton [text-button.key-edit] ] | 784 / 784 | `settings-7-dark-tab10.png`, `settings-7-light-tab10.png` |
| 596 | `settings-7` | 11 | GtkButton [text-button.pill.plain] Leave "Next match candidate" wit... | 542 / 542 | `settings-7-dark-tab11.png`, `settings-7-light-tab11.png` |
| 597 | `settings-7` | 12 | GtkButton [text-button.key-edit] [ | 1110 / 1110 | `settings-7-dark-tab12.png`, `settings-7-light-tab12.png` |
| 598 | `settings-7` | 13 | GtkButton [text-button.pill.plain] Leave "Previous match candidate"... | 630 / 630 | `settings-7-dark-tab13.png`, `settings-7-light-tab13.png` |
| 599 | `settings-7` | 14 | GtkButton [text-button.key-edit] C | 1185 / 1186 | `settings-7-dark-tab14.png`, `settings-7-light-tab14.png` |
| 600 | `settings-7` | 15 | GtkButton [text-button.pill.plain] Leave "Compare with match" with ... | 630 / 630 | `settings-7-dark-tab15.png`, `settings-7-light-tab15.png` |
| 601 | `settings-7` | 16 | GtkButton [text-button.key-edit] O | 1148 / 1151 | `settings-7-dark-tab16.png`, `settings-7-light-tab16.png` |
| 602 | `settings-7` | 17 | GtkButton [text-button.pill.plain] Leave "Open match in browser" wi... | 630 / 630 | `settings-7-dark-tab17.png`, `settings-7-light-tab17.png` |
| 603 | `settings-7` | 18 | GtkButton [text-button.key-edit] H | 1151 / 1152 | `settings-7-dark-tab18.png`, `settings-7-light-tab18.png` |
| 604 | `settings-7` | 19 | GtkButton [text-button.pill.plain] Leave "Show in Hydrus" with no k... | 630 / 630 | `settings-7-dark-tab19.png`, `settings-7-light-tab19.png` |
| 605 | `settings-7` | 20 | GtkButton [text-button.key-edit] Return | 1395 / 1395 | `settings-7-dark-tab20.png`, `settings-7-light-tab20.png` |
| 606 | `settings-7` | 21 | GtkButton [text-button.pill.plain] Leave "Send file + URL + tags to... | 630 / 630 | `settings-7-dark-tab21.png`, `settings-7-light-tab21.png` |
| 607 | `settings-7` | 22 | GtkButton [text-button.key-edit] U | 1382 / 1382 | `settings-7-dark-tab22.png`, `settings-7-light-tab22.png` |
| 608 | `settings-7` | 23 | GtkButton [text-button.pill.plain] Leave "Send URL to Hydrus's URL ... | 630 / 630 | `settings-7-dark-tab23.png`, `settings-7-light-tab23.png` |
| 609 | `settings-7` | 24 | GtkButton [text-button.key-edit] D | 1131 / 1131 | `settings-7-dark-tab24.png`, `settings-7-light-tab24.png` |
| 610 | `settings-7` | 25 | GtkButton [text-button.pill.plain] Leave "Download match + send to ... | 630 / 630 | `settings-7-dark-tab25.png`, `settings-7-light-tab25.png` |
| 611 | `settings-7` | 26 | GtkButton [text-button.key-edit] F5 | 1205 / 1207 | `settings-7-dark-tab26.png`, `settings-7-light-tab26.png` |
| 612 | `settings-7` | 27 | GtkButton [text-button.pill.plain] Leave "Re-search" with no key at... | 630 / 630 | `settings-7-dark-tab27.png`, `settings-7-light-tab27.png` |
| 613 | `settings-7` | 28 | GtkButton [text-button.key-edit] R | 1210 / 1211 | `settings-7-dark-tab28.png`, `settings-7-light-tab28.png` |
| 614 | `settings-7` | 29 | GtkButton [text-button.pill.plain] Leave "Reset result" with no key... | 630 / 630 | `settings-7-dark-tab29.png`, `settings-7-light-tab29.png` |
| 615 | `settings-7` | 30 | GtkButton [text-button.key-edit] Space | 1395 / 1396 | `settings-7-dark-tab30.png`, `settings-7-light-tab30.png` |
| 616 | `settings-7` | 31 | GtkButton [text-button.pill.plain] Leave "Mark / unmark as reviewed... | 630 / 630 | `settings-7-dark-tab31.png`, `settings-7-light-tab31.png` |
| 617 | `settings-7` | 32 | GtkButton [text-button.key-edit] Del | 1432 / 1432 | `settings-7-dark-tab32.png`, `settings-7-light-tab32.png` |
| 618 | `settings-7` | 33 | GtkButton [text-button.pill.plain] Leave "Remove from list" with no... | 630 / 630 | `settings-7-dark-tab33.png`, `settings-7-light-tab33.png` |
| 619 | `settings-7` | 34 | GtkButton [text-button.pill.plain] Reset all to defaults | 2315 / 2318 | `settings-7-dark-tab34.png`, `settings-7-light-tab34.png` |
| 620 | `settings-7` | 35 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-7-dark-tab35.png`, `settings-7-light-tab35.png` |
| 621 | `settings-7` | 36 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-7-dark-tab36.png`, `settings-7-light-tab36.png` |
| 622 | `settings-8` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-8-dark-tab0.png`, `settings-8-light-tab0.png` |
| 623 | `settings-8` | 1 | GtkNotebook [frame] General | 6104 / 6104 | `settings-8-dark-tab1.png`, `settings-8-light-tab1.png` |
| 624 | `settings-8` | 2 | GtkCheckButton [text-button] Enable the MCP server | 3774 / 3776 | `settings-8-dark-tab2.png`, `settings-8-light-tab2.png` |
| 625 | `settings-8` | 3 | GtkText [] | 6679 / 6673 | `settings-8-dark-tab3.png`, `settings-8-light-tab3.png` |
| 626 | `settings-8` | 4 | GtkText [] | 788 / 788 | `settings-8-dark-tab4.png`, `settings-8-light-tab4.png` |
| 627 | `settings-8` | 5 | GtkButton [text-button.icon-boxed] Copy token to clipboard | 376 / 376 | `settings-8-dark-tab5.png`, `settings-8-light-tab5.png` |
| 628 | `settings-8` | 6 | GtkButton [text-button.pill.plain] Generate new token | 920 / 920 | `settings-8-dark-tab6.png`, `settings-8-light-tab6.png` |
| 629 | `settings-8` | 7 | GtkCheckButton [text-button] Dry run — practice mode | 3774 / 3776 | `settings-8-dark-tab7.png`, `settings-8-light-tab7.png` |
| 630 | `settings-8` | 8 | GtkCheckButton [text-button] Let it send files and tags to Hydrus | 2448 / 2576 | `settings-8-dark-tab8.png`, `settings-8-light-tab8.png` |
| 631 | `settings-8` | 9 | GtkCheckButton [text-button] Let it re-search a stuck entry | 4402 / 4389 | `settings-8-dark-tab9.png`, `settings-8-light-tab9.png` |
| 632 | `settings-8` | 10 | GtkCheckButton [text-button] Let it remove rows and reset results | 4056 / 4024 | `settings-8-dark-tab10.png`, `settings-8-light-tab10.png` |
| 633 | `settings-8` | 11 | GtkToggleButton [text-button.toggle.caps] Actions | 4576 / 5081 | `settings-8-dark-tab11.png`, `settings-8-light-tab11.png` |
| 634 | `settings-8` | 12 | GtkToggleButton [text-button.toggle.caps] All calls | 672 / 672 | `settings-8-dark-tab12.png`, `settings-8-light-tab12.png` |
| 635 | `settings-8` | 13 | GtkToggleButton [text-button.toggle.icon-boxed] Keep this up to dat... | 721 / 721 | `settings-8-dark-tab13.png`, `settings-8-light-tab13.png` |
| 636 | `settings-8` | 14 | GtkButton [text-button.icon-boxed] Open the folder the audit log is... | 248 / 248 | `settings-8-dark-tab14.png`, `settings-8-light-tab14.png` |
| 637 | `settings-8` | 15 | GtkText [] | 1000 / 1000 | `settings-8-dark-tab15.png`, `settings-8-light-tab15.png` |
| 638 | `settings-8` | 16 | GtkScrolledWindow [field] Time | 22967 / 29365 | `settings-8-dark-tab16.png`, `settings-8-light-tab16.png` |
| 639 | `settings-8` | 17 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-8-dark-tab17.png`, `settings-8-light-tab17.png` |
| 640 | `settings-8` | 18 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-8-dark-tab18.png`, `settings-8-light-tab18.png` |
| 641 | `settings-mcp-running` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-mcp-running-dark-tab0.png`, `settings-mcp-running-light-tab0.png` |
| 642 | `settings-mcp-running` | 1 | GtkNotebook [frame] General | 6104 / 6104 | `settings-mcp-running-dark-tab1.png`, `settings-mcp-running-light-tab1.png` |
| 643 | `settings-mcp-running` | 2 | GtkCheckButton [text-button] Enable the MCP server | 3774 / 3776 | `settings-mcp-running-dark-tab2.png`, `settings-mcp-running-light-tab2.png` |
| 644 | `settings-mcp-running` | 3 | GtkButton [text-button.pill.plain] Restart server | 748 / 748 | `settings-mcp-running-dark-tab3.png`, `settings-mcp-running-light-tab3.png` |
| 645 | `settings-mcp-running` | 4 | GtkText [] | 6849 / 6837 | `settings-mcp-running-dark-tab4.png`, `settings-mcp-running-light-tab4.png` |
| 646 | `settings-mcp-running` | 5 | GtkText [] | 3550 / 3557 | `settings-mcp-running-dark-tab5.png`, `settings-mcp-running-light-tab5.png` |
| 647 | `settings-mcp-running` | 6 | GtkButton [text-button.icon-boxed] Copy token to clipboard | 376 / 376 | `settings-mcp-running-dark-tab6.png`, `settings-mcp-running-light-tab6.png` |
| 648 | `settings-mcp-running` | 7 | GtkButton [text-button.pill.plain] Generate new token | 920 / 920 | `settings-mcp-running-dark-tab7.png`, `settings-mcp-running-light-tab7.png` |
| 649 | `settings-mcp-running` | 8 | GtkCheckButton [text-button] Dry run — practice mode | 3774 / 3776 | `settings-mcp-running-dark-tab8.png`, `settings-mcp-running-light-tab8.png` |
| 650 | `settings-mcp-running` | 9 | GtkCheckButton [text-button] Let it send files and tags to Hydrus | 2490 / 2499 | `settings-mcp-running-dark-tab9.png`, `settings-mcp-running-light-tab9.png` |
| 651 | `settings-mcp-running` | 10 | GtkCheckButton [text-button] Let it re-search a stuck entry | 4402 / 4389 | `settings-mcp-running-dark-tab10.png`, `settings-mcp-running-light-tab10.png` |
| 652 | `settings-mcp-running` | 11 | GtkCheckButton [text-button] Let it remove rows and reset results | 4056 / 4024 | `settings-mcp-running-dark-tab11.png`, `settings-mcp-running-light-tab11.png` |
| 653 | `settings-mcp-running` | 12 | GtkToggleButton [text-button.toggle.caps] Actions | 4576 / 5081 | `settings-mcp-running-dark-tab12.png`, `settings-mcp-running-light-tab12.png` |
| 654 | `settings-mcp-running` | 13 | GtkToggleButton [text-button.toggle.caps] All calls | 672 / 672 | `settings-mcp-running-dark-tab13.png`, `settings-mcp-running-light-tab13.png` |
| 655 | `settings-mcp-running` | 14 | GtkToggleButton [text-button.toggle.icon-boxed] Keep this up to dat... | 721 / 721 | `settings-mcp-running-dark-tab14.png`, `settings-mcp-running-light-tab14.png` |
| 656 | `settings-mcp-running` | 15 | GtkButton [text-button.icon-boxed] Open the folder the audit log is... | 248 / 248 | `settings-mcp-running-dark-tab15.png`, `settings-mcp-running-light-tab15.png` |
| 657 | `settings-mcp-running` | 16 | GtkText [] | 1000 / 1000 | `settings-mcp-running-dark-tab16.png`, `settings-mcp-running-light-tab16.png` |
| 658 | `settings-mcp-running` | 17 | GtkScrolledWindow [field] Time | 22967 / 29365 | `settings-mcp-running-dark-tab17.png`, `settings-mcp-running-light-tab17.png` |
| 659 | `settings-mcp-running` | 18 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-mcp-running-dark-tab18.png`, `settings-mcp-running-light-tab18.png` |
| 660 | `settings-mcp-running` | 19 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-mcp-running-dark-tab19.png`, `settings-mcp-running-light-tab19.png` |
| 661 | `settings-shortcuts-conflict` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-shortcuts-conflict-dark-tab0.png`, `settings-shortcuts-conflict-light-tab0.png` |
| 662 | `settings-shortcuts-conflict` | 1 | GtkNotebook [frame] General | 6256 / 6256 | `settings-shortcuts-conflict-dark-tab1.png`, `settings-shortcuts-conflict-light-tab1.png` |
| 663 | `settings-shortcuts-conflict` | 2 | GtkButton [text-button.key-edit] J | 784 / 784 | `settings-shortcuts-conflict-dark-tab2.png`, `settings-shortcuts-conflict-light-tab2.png` |
| 664 | `settings-shortcuts-conflict` | 3 | GtkButton [text-button.pill.plain] Leave "Next image" with no key a... | 542 / 542 | `settings-shortcuts-conflict-dark-tab3.png`, `settings-shortcuts-conflict-light-tab3.png` |
| 665 | `settings-shortcuts-conflict` | 4 | GtkButton [text-button.key-edit] J | 784 / 784 | `settings-shortcuts-conflict-dark-tab4.png`, `settings-shortcuts-conflict-light-tab4.png` |
| 666 | `settings-shortcuts-conflict` | 5 | GtkButton [text-button.pill.plain] Leave "Previous image" with no k... | 542 / 542 | `settings-shortcuts-conflict-dark-tab5.png`, `settings-shortcuts-conflict-light-tab5.png` |
| 667 | `settings-shortcuts-conflict` | 6 | GtkButton [text-button.key-edit] N | 784 / 784 | `settings-shortcuts-conflict-dark-tab6.png`, `settings-shortcuts-conflict-light-tab6.png` |
| 668 | `settings-shortcuts-conflict` | 7 | GtkButton [text-button.pill.plain] Leave "Next image needing review... | 542 / 542 | `settings-shortcuts-conflict-dark-tab7.png`, `settings-shortcuts-conflict-light-tab7.png` |
| 669 | `settings-shortcuts-conflict` | 8 | GtkButton [text-button.key-edit] Shift+N | 784 / 784 | `settings-shortcuts-conflict-dark-tab8.png`, `settings-shortcuts-conflict-light-tab8.png` |
| 670 | `settings-shortcuts-conflict` | 9 | GtkButton [text-button.pill.plain] Leave "Previous image needing re... | 542 / 542 | `settings-shortcuts-conflict-dark-tab9.png`, `settings-shortcuts-conflict-light-tab9.png` |
| 671 | `settings-shortcuts-conflict` | 10 | GtkButton [text-button.key-edit] ] | 784 / 784 | `settings-shortcuts-conflict-dark-tab10.png`, `settings-shortcuts-conflict-light-tab10.png` |
| 672 | `settings-shortcuts-conflict` | 11 | GtkButton [text-button.pill.plain] Leave "Next match candidate" wit... | 542 / 542 | `settings-shortcuts-conflict-dark-tab11.png`, `settings-shortcuts-conflict-light-tab11.png` |
| 673 | `settings-shortcuts-conflict` | 12 | GtkButton [text-button.key-edit] [ | 1110 / 1110 | `settings-shortcuts-conflict-dark-tab12.png`, `settings-shortcuts-conflict-light-tab12.png` |
| 674 | `settings-shortcuts-conflict` | 13 | GtkButton [text-button.pill.plain] Leave "Previous match candidate"... | 630 / 630 | `settings-shortcuts-conflict-dark-tab13.png`, `settings-shortcuts-conflict-light-tab13.png` |
| 675 | `settings-shortcuts-conflict` | 14 | GtkButton [text-button.key-edit] C | 1140 / 1141 | `settings-shortcuts-conflict-dark-tab14.png`, `settings-shortcuts-conflict-light-tab14.png` |
| 676 | `settings-shortcuts-conflict` | 15 | GtkButton [text-button.pill.plain] Leave "Compare with match" with ... | 630 / 630 | `settings-shortcuts-conflict-dark-tab15.png`, `settings-shortcuts-conflict-light-tab15.png` |
| 677 | `settings-shortcuts-conflict` | 16 | GtkButton [text-button.key-edit] O | 1148 / 1151 | `settings-shortcuts-conflict-dark-tab16.png`, `settings-shortcuts-conflict-light-tab16.png` |
| 678 | `settings-shortcuts-conflict` | 17 | GtkButton [text-button.pill.plain] Leave "Open match in browser" wi... | 630 / 630 | `settings-shortcuts-conflict-dark-tab17.png`, `settings-shortcuts-conflict-light-tab17.png` |
| 679 | `settings-shortcuts-conflict` | 18 | GtkButton [text-button.key-edit] H | 1151 / 1152 | `settings-shortcuts-conflict-dark-tab18.png`, `settings-shortcuts-conflict-light-tab18.png` |
| 680 | `settings-shortcuts-conflict` | 19 | GtkButton [text-button.pill.plain] Leave "Show in Hydrus" with no k... | 630 / 630 | `settings-shortcuts-conflict-dark-tab19.png`, `settings-shortcuts-conflict-light-tab19.png` |
| 681 | `settings-shortcuts-conflict` | 20 | GtkButton [text-button.key-edit] Return | 1395 / 1395 | `settings-shortcuts-conflict-dark-tab20.png`, `settings-shortcuts-conflict-light-tab20.png` |
| 682 | `settings-shortcuts-conflict` | 21 | GtkButton [text-button.pill.plain] Leave "Send file + URL + tags to... | 630 / 630 | `settings-shortcuts-conflict-dark-tab21.png`, `settings-shortcuts-conflict-light-tab21.png` |
| 683 | `settings-shortcuts-conflict` | 22 | GtkButton [text-button.key-edit] U | 1382 / 1382 | `settings-shortcuts-conflict-dark-tab22.png`, `settings-shortcuts-conflict-light-tab22.png` |
| 684 | `settings-shortcuts-conflict` | 23 | GtkButton [text-button.pill.plain] Leave "Send URL to Hydrus's URL ... | 630 / 630 | `settings-shortcuts-conflict-dark-tab23.png`, `settings-shortcuts-conflict-light-tab23.png` |
| 685 | `settings-shortcuts-conflict` | 24 | GtkButton [text-button.key-edit] D | 1131 / 1131 | `settings-shortcuts-conflict-dark-tab24.png`, `settings-shortcuts-conflict-light-tab24.png` |
| 686 | `settings-shortcuts-conflict` | 25 | GtkButton [text-button.pill.plain] Leave "Download match + send to ... | 630 / 630 | `settings-shortcuts-conflict-dark-tab25.png`, `settings-shortcuts-conflict-light-tab25.png` |
| 687 | `settings-shortcuts-conflict` | 26 | GtkButton [text-button.key-edit] F5 | 1205 / 1207 | `settings-shortcuts-conflict-dark-tab26.png`, `settings-shortcuts-conflict-light-tab26.png` |
| 688 | `settings-shortcuts-conflict` | 27 | GtkButton [text-button.pill.plain] Leave "Re-search" with no key at... | 630 / 630 | `settings-shortcuts-conflict-dark-tab27.png`, `settings-shortcuts-conflict-light-tab27.png` |
| 689 | `settings-shortcuts-conflict` | 28 | GtkButton [text-button.key-edit] R | 1210 / 1211 | `settings-shortcuts-conflict-dark-tab28.png`, `settings-shortcuts-conflict-light-tab28.png` |
| 690 | `settings-shortcuts-conflict` | 29 | GtkButton [text-button.pill.plain] Leave "Reset result" with no key... | 630 / 630 | `settings-shortcuts-conflict-dark-tab29.png`, `settings-shortcuts-conflict-light-tab29.png` |
| 691 | `settings-shortcuts-conflict` | 30 | GtkButton [text-button.key-edit] Space | 1395 / 1396 | `settings-shortcuts-conflict-dark-tab30.png`, `settings-shortcuts-conflict-light-tab30.png` |
| 692 | `settings-shortcuts-conflict` | 31 | GtkButton [text-button.pill.plain] Leave "Mark / unmark as reviewed... | 630 / 630 | `settings-shortcuts-conflict-dark-tab31.png`, `settings-shortcuts-conflict-light-tab31.png` |
| 693 | `settings-shortcuts-conflict` | 32 | GtkButton [text-button.key-edit] Del | 1432 / 1432 | `settings-shortcuts-conflict-dark-tab32.png`, `settings-shortcuts-conflict-light-tab32.png` |
| 694 | `settings-shortcuts-conflict` | 33 | GtkButton [text-button.pill.plain] Leave "Remove from list" with no... | 630 / 630 | `settings-shortcuts-conflict-dark-tab33.png`, `settings-shortcuts-conflict-light-tab33.png` |
| 695 | `settings-shortcuts-conflict` | 34 | GtkButton [text-button.pill.plain] Reset all to defaults | 920 / 920 | `settings-shortcuts-conflict-dark-tab34.png`, `settings-shortcuts-conflict-light-tab34.png` |
| 696 | `settings-shortcuts-conflict` | 35 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-shortcuts-conflict-dark-tab35.png`, `settings-shortcuts-conflict-light-tab35.png` |
| 697 | `settings-shortcuts-conflict` | 36 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-shortcuts-conflict-dark-tab36.png`, `settings-shortcuts-conflict-light-tab36.png` |
| 698 | `settings-hydrus-fetched` | 0 | GtkText [] a setting's name - "quota", "cookies", "thumbnail"… | 3384 / 3384 | `settings-hydrus-fetched-dark-tab0.png`, `settings-hydrus-fetched-light-tab0.png` |
| 699 | `settings-hydrus-fetched` | 1 | GtkNotebook [frame] General | 6180 / 6180 | `settings-hydrus-fetched-dark-tab1.png`, `settings-hydrus-fetched-light-tab1.png` |
| 700 | `settings-hydrus-fetched` | 2 | GtkText []  | 5737 / 5728 | `settings-hydrus-fetched-dark-tab2.png`, `settings-hydrus-fetched-light-tab2.png` |
| 701 | `settings-hydrus-fetched` | 3 | GtkText []  | 2796 / 2796 | `settings-hydrus-fetched-dark-tab3.png`, `settings-hydrus-fetched-light-tab3.png` |
| 702 | `settings-hydrus-fetched` | 4 | GtkText [] empty = "my tags" local service | 1864 / 1864 | `settings-hydrus-fetched-dark-tab4.png`, `settings-hydrus-fetched-light-tab4.png` |
| 703 | `settings-hydrus-fetched` | 5 | GtkToggleButton [image-button.toggle]  | 376 / 376 | `settings-hydrus-fetched-dark-tab5.png`, `settings-hydrus-fetched-light-tab5.png` |
| 704 | `settings-hydrus-fetched` | 6 | GtkButton [text-button] Fetch from Hydrus… | 928 / 928 | `settings-hydrus-fetched-dark-tab6.png`, `settings-hydrus-fetched-light-tab6.png` |
| 705 | `settings-hydrus-fetched` | 7 | GtkCheckButton [text-button] Verify SSL certificate | 3800 / 3800 | `settings-hydrus-fetched-dark-tab7.png`, `settings-hydrus-fetched-light-tab7.png` |
| 706 | `settings-hydrus-fetched` | 8 | GtkCheckButton [text-button] When "Download match ... | 3800 / 3800 | `settings-hydrus-fetched-dark-tab8.png`, `settings-hydrus-fetched-light-tab8.png` |
| 707 | `settings-hydrus-fetched` | 9 | GtkCheckButton [text-button] Everything this app k... | 3800 / 3800 | `settings-hydrus-fetched-dark-tab9.png`, `settings-hydrus-fetched-light-tab9.png` |
| 708 | `settings-hydrus-fetched` | 10 | GtkText [] empty = "hatate: match source" | 2796 / 2796 | `settings-hydrus-fetched-dark-tab10.png`, `settings-hydrus-fetched-light-tab10.png` |
| 709 | `settings-hydrus-fetched` | 11 | GtkButton [text-button] Test connection | 812 / 812 | `settings-hydrus-fetched-dark-tab11.png`, `settings-hydrus-fetched-light-tab11.png` |
| 710 | `settings-hydrus-fetched` | 12 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-hydrus-fetched-dark-tab12.png`, `settings-hydrus-fetched-light-tab12.png` |
| 711 | `settings-hydrus-fetched` | 13 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-hydrus-fetched-dark-tab13.png`, `settings-hydrus-fetched-light-tab13.png` |
| 712 | `shortcuts-dialog` | 0 | GtkScrolledWindow [] Next image: | 2848 / 2848 | `shortcuts-dialog-dark-tab0.png`, `shortcuts-dialog-light-tab0.png` |
| 713 | `shortcuts-dialog` | 1 | GtkButton [text-button.pill.primary.plain.default] Close | 496 / 496 | `shortcuts-dialog-dark-tab1.png`, `shortcuts-dialog-light-tab1.png` |

**713 stops (1426 walks, one per scheme) over 34 surfaces; 0 without a visible indicator.** `failed` is one of them: 13 stops (0 to 12), each with an indicator in both schemes.

**Not judged: `review-picker`.** The candidate picker is a popover, its own surface, and the walk's snapshot is the window's: its 80 stops (the walk's cap; the popover's list cycles) show no change, which measures the tool and not the popover. The picker's ring is on the `review-picker-*` shots; the walk of a popover surface is the fix (run C, or the verifier's walk). Owner: Oderisi's design read for the ring, the author for the walker.
