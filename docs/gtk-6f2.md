# Wipe, Differences, the compare window and the error state (Phase 6.f-2)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of [DAN-2340](/DAN/issues/DAN-2340),
the second half of the Review view that [DAN-2219](/DAN/issues/DAN-2219) split at the end of its run A2
(`docs/gtk-6f.md`). It holds the two maps Phase 6 asks every ticket for (the method map and the Python test map; 6.j
checks the union), the differential, the on-purpose differences with the assertion that pins each, the measured
minimum size and the shot set. Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`.

## 1. The cut

**Ported here** (`hatate-core` and `hatate-gtk`):

- `hatate-core` `compare_view.rs`: `WipeGeometry` (zoom, pan, fit, wheel, drag clamp), `visible_part`,
  `build_difference_overlay` over the merged `image_diff`, `ramp_picture`, `open_local` with `PictureError` (`kind()`,
  `user_message()`, `detail()`), `build_summary` and `human`; `thumb_decode` gains `decode_bytes` and `decode_full`.
- `hatate-gtk` `wipe_view.rs` (the cairo-drawn zoom/pan widget with its wipe), `review_compare.rs` (the Review page's
  Wipe and Differences: the full-resolution load and the overlay build on workers, a stale result dropped),
  `compare_dialog.rs` (the compare window, four modes, the summary), the `compare` review key, the Pop out button and the
  table menu's Compare entry. The Wipe and Differences buttons of the Review page are live.
- The error state (design gate 2.8): a plain-language note, the raw OS text in one detail line, the slider and the note
  insensitive, Differences insensitive.

**Not in this PR:** the Shortcuts list (`_show_shortcuts_dialog`, `gui/review_view.py:619`) is on no ticket yet (section
2 of `docs/gtk-6f.md`); 6.g owns the Hydrus buttons; 6.h owns rebinding a key.

## 2. Differences from Python, on purpose

Each row names the assertion that pins it. Each of those assertions was **reverted and run red** at the line the row
names (`worktrees/DAN-2376-revert.log`), then restored.

| Id | What differs | Why | Pinned by (assertion) | Reverted line, and the result |
|---|---|---|---|---|
| **D-A** | Python paints the coloured pictures under a RED overlay in Differences. Here both pictures are drawn as luma on the R=G=B ramp `#1a1a1a`..`#c5c5c5` (197 is the luma of `#cdc4ba`) and the overlay is that grey with Python's alpha `int(v * 0.82)`. The regions marked are the same; only the hue differs. | Design gate 2.2 asks for a zero-chroma pane. | `smoke::the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours` (`crates/hatate-gtk/tests/smoke.rs:5308`) (max channel spread 0), with the Wipe pane as the control (`crates/hatate-gtk/tests/smoke.rs:5298`, spread over 100) | `ramp_picture` returning the picture's own colours: `no pixel of the 579 x 228 Differences pane has chroma` fails. |
| **D-B** | The Wipe frame's floor is 280 x 170 (Qt: 420 x 420), the comparison page does not borrow the pair's height, and the error detail is one line cut in the middle (full text in the tooltip). The zoom percent after `100%` and the 0.25 floor depend on the widget's fit size, which differs, so the differential compares them layout-free (class L, section 4). | Gate 2.5: every state at the stated minimum is 1000 x 780 inside its shadow, read from the shot's PNG header. The comparison states asked for 854 (Wipe), 873 (Differences) and 909 px (error) on the host toolkit before this. | `smoke::review_comparison_states_keep_the_stated_minimum` (`crates/hatate-gtk/tests/smoke.rs:5074`) (Wipe, Differences, error and no-match, each opened at the smallest size and then prepared, as a shot is) | `stack.set_vhomogeneous(false)` removed: 1000x854; `WIPE_MIN` back to 280 x 260: 1000x831; the one-line detail removed: the error state 1000x796 (`worktrees/DAN-2376-revert.log`, "fit (a)", "(b)", "(c)"). |
| **D-C** | The review zoom readout is **blank** while nothing is loaded; Python prints `0%`. The readout is visible only during the "Loading the match..." moment (after Wipe is chosen, or after a new selection with the comparison showing); that is when the frame is empty, so that is the only place it can be seen. Rust deliberately shows nothing there, as `zoom_percent`'s own doc comment says. Defined narrowly (ruling on [DAN-2375](/DAN/issues/DAN-2375)): only `.compare.zoom_fmt` where Rust is `''` and nothing is loaded; any other `zoom_fmt` pair, or Rust blank with a picture loaded, is unclassified. | Ruling (a), Cloud, DAN-2375. | `smoke::review_zoom_readout_is_blank_while_nothing_is_loaded` (`crates/hatate-gtk/tests/smoke.rs:5002`) (readout visible, frame empty, text empty) | `String::new()` -> `"0%".to_owned()` in the zoom callback (`review_view.rs:420`): `assertion left == right failed` at that line. |
| **D-D** | The error state (gate 2.8, accepted in DAN-2219; class E in the differential). A local file that cannot be opened (a directory, missing, mode 000, not a picture) gives "The local picture could not be opened..." in the note, the raw text in one detail line, and the slider and the note insensitive. Python shows its how-to or "Both pictures are needed to compare them." and leaves them live. | Gate 2.8. | `smoke::review_wipe_with_an_unreadable_local_file_shows_the_error_state` (`crates/hatate-gtk/tests/smoke.rs:4911`), `smoke::review_wipe_with_an_unreadable_local_file_shows_the_error_state` (`crates/hatate-gtk/tests/smoke.rs:4914`), `smoke::review_wipe_with_an_unreadable_local_file_shows_the_error_state` (`crates/hatate-gtk/tests/smoke.rs:4921`); the core error shape is `compare_view::tests::a_missing_file_says_what_to_do_and_leaks_neither_path_nor_os_text` (`crates/hatate-core/src/compare_view.rs:792`) | the note set to `e.detail()` in `show_error_state` (`review_compare.rs:343`): the leak assertion fails with the path and `os error 2`. |
| **D-E** | After an unopenable row, selecting a readable one on Side by side makes the Differences button live **at once**. Python only sets it when a full-resolution load finishes and never resets it on selection, so its button stays disabled until Wipe has been clicked once. Defined narrowly: only `.compare.diff_button_enabled` py False / rs True on the first selection after an unopenable row; any other `diff_button_enabled` pair is unclassified. **A consequence the ruling did not spell out** (B2, seed 5): Python's stale-disabled button also makes a later "view diff" click a no-op after one unopenable row, where Rust enters the Differences page with the note "Both pictures are needed to compare them." (51 steps in seed 5, classed as consequences of D-E; with the Python driver resetting the button on selection while Side by side is active, `HATATE_DE=1`, seed 5 has 0 unclassified). | Ruling (a), Cloud, DAN-2375: Python's behaviour is a slip and a dead control. | `smoke::review_differences_button_is_live_again_after_an_unopenable_row` (`crates/hatate-gtk/tests/smoke.rs:5027`) (unopenable row, back to Side by side, a readable row, no Wipe click) | the `diff` `set_sensitive(true)` removed from `sync_comparison_to_selection` (`review_compare.rs:482`): `assertion failed: r.view_buttons["diff"].is_sensitive()` at that line. |

## 3. Minimum size

The stated minimum is 1010 x 790 outer, which is 1000 x 780 inside the window's shadow (`docs/gtk-6f.md` section 3,
design gate 2.5). Every state below is opened **at the minimum** (`shot ... min <state>`, the window given the stated
minimum as its default size, the state prepared on the shot path with no later resize) and its size is read from the
**PNG header** of the shot. A header that differs from the stated minimum is a FAIL; there is no exceptions column.

**(the hosted `ci` table goes here: state, shot filename, PNG width x height from the header)**

**The fit test was red on the shot path before the fix** (`worktrees/DAN-2376-revert.log`): Wipe 1000x854, Differences
1000x873 and the error state 1000x909 on the host toolkit (GTK 4.24); the fix is three lines, each pinned by the
assertion in section 2 (D-B). The host numbers above are the author's; the hosted numbers (GTK 4.14 / libadwaita 1.5)
are the table, and the verifier's shot headers are the check.

## 4. The differential

The same scripted action sequence runs through Python's `CompareDialog` and `ReviewViewMixin` at `76c85c4` and through
the Rust window (`crates/hatate-gtk/tests/differential.rs`, a test-only driver), 5 seeds of 40 scenarios each, with the
Python side under the pinned venv (Pillow 12.3.0; the system's 13.0 gives a 2-bit different `dhash`, which is not the
port). After every step the visible model is compared: the page and view, the slider and wipe position, the zoom
readout, the note and detail text, the summary lines, every control's sensitivity, and the pane pictures' size and
hash. No network (`data:` URIs, a socket guard). Harness: `worktrees/DAN-2340-tools/h`; logs
`worktrees/DAN-2340-differential.log`, dumps `worktrees/DAN-2340-dump{1..5}.jsonl`, classifier
`worktrees/DAN-2340-an.py.keep`. Head of the run: `b9afcf0`; no code changed in B2.

| seed | steps | D-C | E (error state, per field x3) | L (layout) | D-E | unclassified |
|---|---|---|---|---|---|---|
| 1 | 595 | 239 | 82 | 29 | 3 | **0** |
| 2 | 578 | 285 | 64 | 5 | 0 | **0** |
| 3 | 573 | 320 | 83 | 16 | 0 | **0** |
| 4 | 581 | 234 | 55 | 14 | 0 | **0** |
| 5 | 563 | 301 | 83 | 12 | 7 + 51 consequence rows | 51 -> **0** with the control |

`errors=0` in every seed. **Classes.** D-C and D-E are as defined in section 2. **E** is D-D. **L** is the zoom percent
after a `100%` click and the 0.25 floor, which depend on the widget's fit size (compared layout-free: the factor while
a power of 1.25 from the fit, else the percent; one-sided clamps only; `zoom_label_at_100` is present only when the
percent reaches exactly 100). Seed 2 was rerun first at the frozen head, as the ruling on DAN-2375 asked.

**Negative control** (slider +1 in the Python driver, seed 1, 12 scenarios): 69 steps differ in `.compare.slider` and
`.compare.wipe.position`, 0 at baseline; the harness separates a 1-unit wipe error.

**Error variants fed** (the DAN-2231 self-check): a directory, a missing path, mode 000 and a file that is not a picture,
with the error text diffed per variant (`Is a directory (os error 21)`, `os error 2`, `not a picture the decoder reads`).

## 5. Shots and contrast

**(the shot list and the contrast table go here)**

## 6. Method map

Python definitions in scope at `76c85c4` (located by `ast`), with the Rust each one became. The 21 methods of
`gui/main_window.py` that `docs/gtk-shell.md` section 6 gives to 6.f are in `docs/gtk-6f.md`; these are the ones it left
here. Line numbers are in this PR's head.

### `gui/main_window.py`

| Python | Rust |
|---|---|
| `gui/main_window.py:3224` `_open_compare_dialog` | `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` |
| `gui/main_window.py:3651` `_sync_preview_to_selection` (the mode half; 6.f ported the preview half) | `crates/hatate-gtk/src/review_compare.rs:452` `sync_comparison_to_selection`, called from `review_ops.rs` `on_selection_changed` |

### `gui/review_view.py`

| Python | Rust |
|---|---|
| `gui/review_view.py:232` `_build_zoom_tools` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`zoom_tools`, `zoom_label`: 383) |
| `gui/review_view.py:263` `_build_wipe_slider_row` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`wipe_slider`, `slider_row`) |
| `gui/review_view.py:368` `_build_wipe_view` | `crates/hatate-gtk/src/review_view.rs` `ReviewPage::new` (`wipe_host`, `wipe`), `crates/hatate-gtk/src/wipe_view.rs` `WipeView` |
| `gui/review_view.py:667` `_show_comparison_controls` | `crates/hatate-gtk/src/review_compare.rs:182` `show_comparison_controls` |
| `gui/review_view.py:682` `_on_review_view_changed` | `crates/hatate-gtk/src/review_compare.rs:198` `on_review_view_changed` |
| `gui/review_view.py:697` `_ensure_comparison_loaded` | `crates/hatate-gtk/src/review_compare.rs:215` `ensure_comparison_loaded`, `on_comparison_loaded`, `show_error_state` (the error state is new, design gate 2.8) |
| `gui/review_view.py:751` `full_resolution_match` | `crates/hatate-gtk/src/review_compare.rs:493` `full_resolution_match` |
| `gui/review_view.py:761` `_apply_wipe_mode` | `crates/hatate-gtk/src/review_compare.rs:355` `apply_wipe_mode` |
| `gui/review_view.py:771` `_show_differences` | `crates/hatate-gtk/src/review_compare.rs:372` `show_differences`, `on_differences_built`, `present_differences` |
| `gui/review_view.py:816` `_pop_out_comparison` | `crates/hatate-gtk/src/review_ops.rs` `wire_review_page` (`pop_out_btn` runs the `compare` review key) -> `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` |

### `gui/compare_dialog.py`

| Python | Rust |
|---|---|
| `gui/compare_dialog.py:25` `WipeView` | `crates/hatate-gtk/src/wipe_view.rs:81` `WipeView` over `crates/hatate-core/src/compare_view.rs` `WipeGeometry` |
| `gui/compare_dialog.py:47` `WipeView.__init__` | `crates/hatate-gtk/src/wipe_view.rs:88` `WipeView::new` |
| `gui/compare_dialog.py:72` `_wipe_line_colour` | `crates/hatate-gtk/src/wipe_view.rs:410` (white in both schemes, `on_accent`) |
| `gui/compare_dialog.py:82` `_pane_divider_colour` | `crates/hatate-gtk/src/wipe_view.rs:456` (the divider in `paint`) |
| `gui/compare_dialog.py:92` `_reference` | `crates/hatate-core/src/compare_view.rs:116` `set_reference` |
| `gui/compare_dialog.py:101` `showing_differences` | `crates/hatate-core/src/compare_view.rs:127` `showing_differences`, `crates/hatate-gtk/src/wipe_view.rs:170` `showing_differences` |
| `gui/compare_dialog.py:104` `_reference_size` | `crates/hatate-core/src/compare_view.rs:122` `set_differences` (the reference doubles in width) |
| `gui/compare_dialog.py:118` `_fit_size` | `crates/hatate-core/src/compare_view.rs:146` `fit_size` |
| `gui/compare_dialog.py:126` `_target_rect` | `crates/hatate-core/src/compare_view.rs:151` `target_rect` |
| `gui/compare_dialog.py:138` `_clamp_pan` | `crates/hatate-core/src/compare_view.rs:163` `clamp_pan` |
| `gui/compare_dialog.py:153` `zoom_percent` | `crates/hatate-core/src/compare_view.rs:177` `zoom_percent`, `crates/hatate-gtk/src/wipe_view.rs:184` `zoom_percent` |
| `gui/compare_dialog.py:163` `_apply_zoom` | `crates/hatate-core/src/compare_view.rs:186` `apply_zoom` |
| `gui/compare_dialog.py:186` `_centre` | `crates/hatate-core/src/compare_view.rs:209` `centre` |
| `gui/compare_dialog.py:189` `zoom_in` | `crates/hatate-core/src/compare_view.rs:213` `zoom_in`, `crates/hatate-gtk/src/wipe_view.rs:222` `zoom_in` |
| `gui/compare_dialog.py:192` `zoom_out` | `crates/hatate-core/src/compare_view.rs:217` `zoom_out`, `crates/hatate-gtk/src/wipe_view.rs:229` `zoom_out` |
| `gui/compare_dialog.py:195` `zoom_to_fit` | `crates/hatate-core/src/compare_view.rs:221` `zoom_to_fit`, `crates/hatate-gtk/src/wipe_view.rs:236` `zoom_to_fit` |
| `gui/compare_dialog.py:201` `zoom_to_actual` | `crates/hatate-core/src/compare_view.rs:227` `zoom_to_actual`, `crates/hatate-gtk/src/wipe_view.rs:241` `zoom_to_actual` |
| `gui/compare_dialog.py:210` `set_images` | `crates/hatate-gtk/src/wipe_view.rs:133` `set_images` |
| `gui/compare_dialog.py:215` `set_difference_view` | `crates/hatate-gtk/src/wipe_view.rs:147` `set_difference_view` |
| `gui/compare_dialog.py:230` `clear_difference_view` | `crates/hatate-gtk/src/wipe_view.rs:159` `clear_difference_view` |
| `gui/compare_dialog.py:234` `set_position` | `crates/hatate-gtk/src/wipe_view.rs:175` `set_position` |
| `gui/compare_dialog.py:239` `wheelEvent` | `crates/hatate-core/src/compare_view.rs:239` `wheel`, scroll controller in `crates/hatate-gtk/src/wipe_view.rs:250` `wire` |
| `gui/compare_dialog.py:247` `mousePressEvent` | drag controller in `crates/hatate-gtk/src/wipe_view.rs:250` `wire` |
| `gui/compare_dialog.py:253` `mouseMoveEvent` | `crates/hatate-core/src/compare_view.rs:247` `drag_to`, drag controller in `wire` |
| `gui/compare_dialog.py:259` `mouseReleaseEvent` | drag controller in `wire` |
| `gui/compare_dialog.py:264` `mouseDoubleClickEvent` | click controller in `wire` (double-click fits) |
| `gui/compare_dialog.py:267` `keyPressEvent` | key controller in `wire` (`+`, `-`, `0`, `1`) |
| `gui/compare_dialog.py:282` `resizeEvent` | `set_widget_size` (compare_view.rs:132), called from the resize handler in `wire` |
| `gui/compare_dialog.py:290` `_draw_side` | `crates/hatate-gtk/src/wipe_view.rs:338` `draw_side` |
| `gui/compare_dialog.py:316` `_paint_differences` | `crates/hatate-gtk/src/wipe_view.rs:393` `paint` (the differences branch) |
| `gui/compare_dialog.py:359` `paintEvent` | `crates/hatate-gtk/src/wipe_view.rs:393` `paint` |
| `gui/compare_dialog.py:408` `build_difference_overlay` | `crates/hatate-core/src/compare_view.rs:335` `build_difference_overlay`, `crates/hatate-core/src/compare_view.rs:294` `ramp_picture` (D-A) |
| `gui/compare_dialog.py:451` `LoadedComparison` | `crates/hatate-gtk/src/review_compare.rs:68` `Loaded` |
| `gui/compare_dialog.py:462` `LoadedComparison.__init__` | `Loaded` (a struct literal) |
| `gui/compare_dialog.py:470` `load_comparison` | `crates/hatate-gtk/src/review_compare.rs:80` `load_comparison` |
| `gui/compare_dialog.py:537` `CompareDialog` | `crates/hatate-gtk/src/compare_dialog.rs:47` `CompareDialog` |
| `gui/compare_dialog.py:546` `CompareDialog.__init__` | `crates/hatate-gtk/src/compare_dialog.rs:107` `new` |
| `gui/compare_dialog.py:664` `_on_zoom_changed` | `crates/hatate-gtk/src/compare_dialog.rs` `new` (the `connect_zoom_changed` closure, D-C) |
| `gui/compare_dialog.py:667` `_apply_mode` | `crates/hatate-gtk/src/compare_dialog.rs:286` `apply_mode` |
| `gui/compare_dialog.py:690` `_show_differences` | `crates/hatate-gtk/src/compare_dialog.rs:312` `show_differences`, `crates/hatate-gtk/src/compare_dialog.rs:354` `present_differences` |
| `gui/compare_dialog.py:722` `_build_difference_overlay` | `crates/hatate-gtk/src/review_compare.rs:56` `build_panes` |
| `gui/compare_dialog.py:725` `_load` | `crates/hatate-gtk/src/compare_dialog.rs:413` `open_compare_dialog` (the worker job), `crates/hatate-gtk/src/compare_dialog.rs:375` `loaded` |
| `gui/compare_dialog.py:737` `_build_summary` | `crates/hatate-gtk/src/compare_dialog.rs:483` `summarize`, `crates/hatate-core/src/compare_view.rs:499` `build_summary` |
| `gui/compare_dialog.py:802` `_pil_from_pixmap` | n/a: no `QPixmap`; the pictures are `Rgba` buffers (`rgb_of` drops the alpha) |
| `gui/compare_dialog.py:826` `_pixmap_from_pil_rgba` | `crates/hatate-gtk/src/review_compare.rs:161` `texture_of` (`gdk::Texture` from the `Rgba`) |
| `gui/compare_dialog.py:837` `_pixmap_from_pil` | `crates/hatate-gtk/src/review_compare.rs:161` `texture_of` |
| `gui/compare_dialog.py:849` `_human` | `crates/hatate-core/src/compare_view.rs:480` `human` |

## 7. Python test map

An `ast` walk over every `tests/test_*.py` at `76c85c4` (`worktrees/DAN-2376-tools/scan6f2.py`, run outside the repo)
finds **38** tests that touch a symbol this ticket ports: every definition of `gui/compare_dialog.py`, the ten
`gui/review_view.py` methods of the method map, `_open_compare_dialog`, and the widget names `wipe_view`,
`review_slider`, `zoom_tools`. The walk follows `setUp`, `setUpClass`, class helpers (`self.<m>()`) and module helpers
transitively; the first draft's body-only walk and a bare `_load` and `paintEvent` match gave 49, of which 11 were other
modules' (`TestRule34UsSite`, `TestSentTagFilter`, the ghost-kanji paint tests: they never call a compare symbol). All 38
are rows below: **0 unmapped**.

`smoke::` is a scenario of `crates/hatate-gtk/tests/smoke.rs`; `compare_view::tests::` is a unit test in `hatate-core`.
A row cites the assertion (`file:line`) that makes its claim. A row that says **not asserted** names a Python assertion
no Rust test makes. A row that says reverted turned red when the line it names was changed
(`worktrees/DAN-2376-mutations.log`: M1 a zoom button label, M2 the fallback warning, M3 the zoom ceiling, M4 the actual
size, M5 the zoom anchor).

#### `tests/test_gui_smoke.py` (30)

| Line | Test | Disposition |
|---|---|---|
| 624 | `DialogLifetime.test_compare_dialogs_do_not_accumulate` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5212`): after `close()` the window's reference in `compare_open` is gone. Qt's `_run_dialog` / `deleteLater` has no GTK counterpart. not asserted: three open-close rounds in a row |
| 674 | `CompareViewZoom.test_new_images_start_fitted` | `compare_view::tests::a_new_pair_is_framed_centred_and_fit` (`crates/hatate-core/src/compare_view.rs:604`): a 800 x 400 picture in a 400 x 400 box is 400 x 200 at the centre; `compare_view::tests::a_new_pair_is_framed_centred_and_fit` (`crates/hatate-core/src/compare_view.rs:605`) |
| 680 | `CompareViewZoom.test_actual_size_is_one_image_pixel_per_screen_pixel` | `compare_view::tests::actual_pixels_is_one_picture_pixel_per_screen_pixel` (`crates/hatate-core/src/compare_view.rs:666`) |
| 685 | `CompareViewZoom.test_zoom_keeps_the_point_under_the_cursor` | `compare_view::tests::zoom_keeps_the_point_under_the_pointer` (`crates/hatate-core/src/compare_view.rs:631`) and the y axis on the next line; reverting the anchor to the centre turns it red (`worktrees/DAN-2376-mutations.log` M5) |
| 701 | `CompareViewZoom.test_panning_cannot_lose_the_image_offscreen` | `compare_view::tests::the_pan_cannot_drag_an_edge_inside_the_viewport` (`crates/hatate-core/src/compare_view.rs:655`) |
| 711 | `CompareViewZoom.test_an_axis_that_fits_stays_centred` | `compare_view::tests::the_pan_cannot_drag_an_edge_inside_the_viewport` (`crates/hatate-core/src/compare_view.rs:659`) |
| 718 | `CompareViewZoom.test_zoom_is_bounded` | `compare_view::tests::zoom_is_bounded_and_a_no_op_at_the_bound` (`crates/hatate-core/src/compare_view.rs:640`) and `compare_view::tests::zoom_is_bounded_and_a_no_op_at_the_bound` (`crates/hatate-core/src/compare_view.rs:646`); the ceiling doubled turns it red (M3) |
| 725 | `CompareViewZoom.test_painting_at_every_zoom_and_split` | `smoke::the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours` (`crates/hatate-gtk/tests/smoke.rs:5295`) draws the pane once, at fit; `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4847`) zooms and moves the split on the page. **not asserted:** the Python loop over MIN/1/3/MAX zoom by 0/.5/1 split is not run |
| 735 | `CompareViewZoom.test_wipe_line_colour_is_mode_invariant` | **not asserted:** the line is drawn white in both schemes (`wipe_view.rs:410`), as the token is; no Rust test reads the colour. The Wipe shots, dark and light, show it |
| 755 | `CompareViewZoom.test_empty_view_is_safe` | `compare_view::tests::an_empty_view_is_safe_to_zoom` (`crates/hatate-core/src/compare_view.rs:681`), `compare_view::tests::nothing_loaded_has_no_figure` (`crates/hatate-core/src/compare_view.rs:674`) |
| 766 | `CompareViewZoom.test_only_one_image_still_zooms` | `compare_view::tests::one_picture_still_zooms` (`crates/hatate-core/src/compare_view.rs:692`); on the page, `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4886`) (the no-match row draws the local picture alone) |
| 1898 | `CompareUsesTheSelectedCandidate.test_an_unvisited_candidate_has_its_details_fetched` | Mapped by `docs/gtk-6f.md` to the picker. The compare half is `review_compare.rs:104` (fetch the details when the candidate has neither a direct nor a preview URL) and `compare_dialog.rs:462`. **not asserted:** the seeds name a `data:` URL, so the on-demand fetch is never reached in a window test |
| 1905 | `CompareUsesTheSelectedCandidate.test_a_candidate_that_already_has_a_url_is_not_refetched` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5174`): a candidate with a direct URL loads its pair with no site asked (the engine has no network). **not asserted:** a count of fetches |
| 1910 | `CompareUsesTheSelectedCandidate.test_each_candidate_gets_its_own_picture` | **not asserted:** the compare dialog is opened on the first candidate only; the picker's own switch is `docs/gtk-6f.md`'s row |
| 1941 | `CompareUsesTheSelectedCandidate.test_a_match_that_will_not_load_says_so` | `compare_view::tests::a_match_that_did_not_come_down_says_so_and_a_fallback_is_italic` (`crates/hatate-core/src/compare_view.rs:886`) for the dialog's summary (the line `Couldn't compare the images` and the strong, uncertain line `The match's picture could not...`); on the page `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4888`) |
| 1950 | `CompareUsesTheSelectedCandidate.test_a_failing_fetch_does_not_take_the_window_down` | `smoke::review_wipe_waits_on_a_stalled_site_without_blocking_the_window` (`crates/hatate-gtk/tests/smoke.rs:5153`): the window keeps turning while the site never answers, then the fetch ends. **not asserted:** a refused connection (the stalled site is the only failing fetch) |
| 4555 | `CompareDifferencesMode.test_a_pixmap_survives_the_round_trip_intact` | n/a: no `QPixmap` and no padded scanlines; the pictures are tight `Rgba` buffers |
| 4565 | `CompareDifferencesMode.test_the_overlay_is_the_local_image_s_shape` | `compare_view::tests::identical_pictures_have_a_clear_overlay_and_a_changed_patch_does_not` (`crates/hatate-core/src/compare_view.rs:750`): the overlay is the local picture's size. **not asserted:** with a match of another size (Python resizes it 2x); the window scenarios use one (160 x 120 against 200 x 150) and do not read the overlay's size |
| 4574 | `CompareDifferencesMode.test_it_reports_what_it_found` | `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4862`): the note begins with a digit and carries the sweep hint. **not asserted:** that the text holds a `%` |
| 4582 | `CompareDifferencesMode.test_both_pictures_are_shown_with_the_overlay_over_them` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5198`): the dialog's view switches to the pair with the overlay, and `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5199`). **not asserted:** that the slider is live in Differences (it is asserted live in Wipe, `smoke.rs:5188`), nor the view's three pictures by identity (Qt object identity has no counterpart) |
| 4593 | `CompareDifferencesMode.test_the_view_frames_the_pair_not_one_picture` | `compare_view::tests::side_by_side_frames_twice_the_width` (`crates/hatate-core/src/compare_view.rs:614`): the pair fits the box at twice the width |
| 4602 | `CompareDifferencesMode.test_the_overlay_keeps_its_transparency` | `compare_view::tests::identical_pictures_have_a_clear_overlay_and_a_changed_patch_does_not` (`crates/hatate-core/src/compare_view.rs:745`): an unchanged picture's overlay is clear, and `compare_view::tests::identical_pictures_have_a_clear_overlay_and_a_changed_patch_does_not` (`crates/hatate-core/src/compare_view.rs:769`) |
| 4609 | `CompareDifferencesMode.test_the_wipe_moves_the_split_in_the_differences_view` | `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4866`): the slider's 25 survives the switch to Differences; `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5193`) for the dialog (in Wipe mode) |
| 4617 | `CompareDifferencesMode.test_the_two_sides_are_the_same_size_so_the_split_registers` | **not asserted:** the two sides are drawn from one reference size in `paint`; no test compares their drawn sizes. The Differences shots show both sides |
| 4626 | `CompareDifferencesMode.test_the_wipe_sweeps_the_overlay_across_both_panes` | `smoke::the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours` (`crates/hatate-gtk/tests/smoke.rs:5322`): the patch is marked in the rendered pane. **not asserted:** the share per half at 0.25 and 0.5 (Python measures a grab); the sweep is judged in the Differences shots |
| 4662 | `CompareDifferencesMode.test_the_slider_is_labelled_for_the_mode` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5197`); the `Match` half is `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5183`) |
| 4671 | `CompareDifferencesMode.test_leaving_the_mode_goes_back_to_the_ordinary_wipe` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5207`); on the page `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4833`) |
| 4680 | `CompareDifferencesMode.test_the_overlay_is_built_once_and_reused` | **not asserted:** the dialog keeps `panes` after the first build (`State.panes`, `compare_dialog.rs:315`), and no test counts builds |
| 4690 | `CompareDifferencesMode.test_the_mode_is_unavailable_without_both_images` | `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4887`) on the page; the dialog's own radio when the match did not come down is **not asserted** |
| 4700 | `CompareDifferencesMode.test_the_mode_is_available_once_both_are_loaded` | `smoke::compare_dialog_loads_the_pair_and_switches_modes` (`crates/hatate-gtk/tests/smoke.rs:5182`) |

#### `tests/test_review_frames.py` (6)

| Line | Test | Disposition |
|---|---|---|
| 191 | `FramesAndCaptions.test_the_wipe_view_is_framed_and_its_zoom_floats_inside_the_frame` | By construction: the wipe is in `image_frame(&wipe_host)` and `zoom_tools` is an overlay child of the same host (`review_view.rs:416`), top right. **not asserted** as geometry; the Wipe shots show it |
| 218 | `WipeChrome.test_the_slider_handle_is_a_24px_target_with_a_hairline_track` | **not asserted, and not the same on purpose:** Qt's 24 px handle and 3 px groove are CSS here (`theme.rs` `scale.wipe-slider`: a 1 px trough, a 14 px stamp handle, the mockup's), judged in the shots |
| 239 | `WipeChrome.test_local_and_match_name_the_ends_of_the_slider` | `smoke::review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns` (`crates/hatate-gtk/tests/smoke.rs:4957`); the caps-mono style is the `caps` and `wipe-end-label` classes, judged in the shots |
| 252 | `WipeChrome.test_the_zoom_row_is_boxed_mono_buttons` | `smoke::review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns` (`crates/hatate-gtk/tests/smoke.rs:4966`): the four labels in order; the box and the mono face are `icon-boxed` and the theme, judged in the shots. Renaming `Fit` turns it red (M1) |
| 265 | `WipeChrome.test_wipe_carries_the_one_line_hint_and_a_warning_replaces_it` | `smoke::review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection` (`crates/hatate-gtk/tests/smoke.rs:4832`) for the hint; `smoke::review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns` (`crates/hatate-gtk/tests/smoke.rs:4968`) for the warning that replaces it (M2). **not asserted:** the note sits below the slider row (the shots show it) |
| 278 | `WipeChrome.test_the_wipe_chrome_stays_out_of_side_by_side` | `smoke::review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns` (`crates/hatate-gtk/tests/smoke.rs:4975`). The viewport share is `docs/gtk-6f.md`'s minimum-size table |

#### `tests/test_review_readout.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 153 | `ReadoutInTheHeader.test_zoom_floats_inside_the_frame_not_in_the_header` | By construction the zoom row is the frame's overlay, not a header child (`review_view.rs:416`); it goes with the chrome (`smoke::review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns` (`crates/hatate-gtk/tests/smoke.rs:4975`)). **not asserted:** its position inside the frame (top right, shots) |

#### `tests/test_mcp_images.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 66 | `TestFetchFullResolutionMatch.test_falls_back_to_the_preview_when_no_direct_url_exists` | n/a here: the test is of `mcp_images.fetch_full_resolution_match` (Phase 5, `docs/mcp-handlers.md`); `load_comparison` is only named in its comment. The comparison's own fallback is in the previous table row of `test_review_frames.py:265` |

## 8. Status claims

This PR adds no status claim. The README's work-in-progress banner is untouched; `docs/gtk-6f.md` section 2 now says the
Wipe and Differences buttons and Pop out are live, which is a behaviour statement and not a claim of usability or
parity. The claim grep at the head is in the PR body.
