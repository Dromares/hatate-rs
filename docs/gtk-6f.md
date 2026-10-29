# The Review view (Phase 6.f)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of the sixth-phase ticket that gives
the window its Review page (DAN-2219), in Side by side mode. It holds the two maps Phase 6 asks every ticket for (the
method map and the Python test map; 6.j checks the union), the differential, the on-purpose differences, the measured
minimum size and the shot set. Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`. Wipe, Differences, the compare
dialog and the error state are the second half, [DAN-2340](/DAN/issues/DAN-2340) ("6.f-2"); every map row that
belongs to it says so.

## 1. The cut

**Ported here** (all in `hatate-gtk`):

- The Review page in Side by side: the two framed panes with their captions and meta lines, the comparison readout,
  the candidate picker (a `DropDown` over the entry's matches) with the lazy candidate fetch off the main loop,
  the Match N of M stepper, the tag panel (list, Add tags, Remove, inline edit), Mark reviewed, Open match, Show in
  Queue, Previous / Next / Next unreviewed, the position strip, and the review keys (`review_keys.rs`: the Qt
  spelling of each binding as a GTK trigger).
- The selection wiring: `on_selection_changed` drives the preview, captions, readout, picker and tag list, and the
  tail of `_on_worker_image_updated` selects the row and refreshes them. "Review the N waiting" is live.
- Edit match conditions (`MatchConditionsDialog` as an `AlertDialog` over the four controls; `action_edit_conditions`
  stores and saves on OK and does nothing on Cancel). The menu entry is live.
- The no-match text and the picker's score marks (`~` only on an estimated score).

**Defects found in the code this builds on, fixed here:**

- `worker.rs` (6.c) built its runtime with `enable_time()` only; any socket on it panicked with "IO is disabled".
  Start Search could not have reached IQDB or SauceNAO in the merged window; the scripted engine never opens a socket,
  so no gate saw it. Fixed with `.enable_io()`; `worker::tests::the_worker_runtime_can_do_network_io` fails without it.
- `QueueView::select_row` is one selection update, as Qt's `ClearAndSelect` is: re-selecting the selected row reports
  no change.
- `search_run.rs::replace_row` pushed a `RowChanged` but never refreshed the Queue's cells; `refresh_entry_row` does both.
- The keyed buttons and the hint bar pinned the window wider than the stated minimum (section 3).

## 2. Not in this PR

- **6.f-2 (DAN-2340):** Wipe, Differences, the compare dialog and its zoom/pan widget, the Pop out button, and the
  error state (design gate 2.8). The Wipe and Differences buttons, Pop out and the pane click are present and
  insensitive with the tooltip `The comparison window arrives in a later step.`
- **6.g:** the Hydrus buttons (Send to Hydrus, Send URL, Download and send, Show in Hydrus) are present and
  insensitive with the tooltip `Sending to Hydrus arrives in a later step.`
- **Not on any ticket as written: the Shortcuts list.** `_show_shortcuts_dialog` (`gui/review_view.py:619`) is not
  in this PR and is not on DAN-2340's list. The Shortcuts button is present and insensitive with the tooltip `The
  shortcuts list arrives in a later step.` It is flagged on DAN-2219 for Cloud to place (6.f-2 or 6.h).
- **6.h:** rebinding a key. The page reads the resolved shortcuts at build and the keycaps follow `resolved_shortcuts()`;
  there is no path to change a binding in this build.

## 3. Minimum size

The stated minimum is 1010 x 790 (outer; the window is 1000 x 780 inside its shadow). Design gate 2.5 asks that
every state fits it and that the window really is that size when given it.

Measured at the head of this PR, the Review page in both schemes. `examples/shot.rs` gives the window `min` (the
default size elsewhere) and prints the outer rect as GTK reports it (`SHOT` lines). Two toolkits, same states:

- **Hosted `ci`** (GTK 4.14 / libadwaita 1.5, the toolkit the design gate is judged on): run 40850518131 at `545c27f`,
  `worktrees/DAN-2219-ci-545c27f.log`.
- **The author's host** (GTK 4.24 / libadwaita 1.10): every state below reads 1000 x 780 at `min`, in both schemes
  (`worktrees/DAN-2219-host-sizes.log`).

At `min` (1000 x 780 inside the shadow; the stated minimum is 1010 x 790 outer):

| State | dark (`ci`) | light (`ci`) |
|---|---|---|
| `review-min` | 1000 x 780 | 1000 x 780 |
| `review-empty-min` | 1000 x 780 | 1000 x 780 |
| `review-nomatch-min` | 1000 x 780 | 1000 x 780 |
| `review-picker-min` | 1000 x 780 | 1000 x 780 |
| `review-addtags-min` | 1000 x 780 | 1000 x 780 |
| `review-conditions-min` | 1000 x 780 | 1000 x 780 |
| `review-focus-mark-min` | 1000 x 780 | 1000 x 780 |

At the default size (outer 1090 x 790):

| State | dark (`ci`) | light (`ci`) |
|---|---|---|
| `review` | 1090 x 790 | 1090 x 790 |
| `review-empty` | 1090 x 790 | 1090 x 790 |
| `review-nomatch` | 1090 x 790 | 1090 x 790 |
| `review-picker` | 1090 x 790 | 1090 x 790 |
| `review-addtags` | 1090 x 790 | 1090 x 790 |
| `review-conditions` | 1090 x 790 | 1090 x 790 |

The popover of `review-picker` is its own surface: 385 x 84 at the default size and 340 x 84 at `min`, in both schemes.

Two things were wrong with the first measurement of a *selected* row, and the smoke scenario that guards the minimum
never saw them because it opens Review with an empty queue (it does not any more, see below):

1. **The keyed buttons kept the page wide.** `Keyed::wrap_when_narrow` drops the key under the label when the column
   is too narrow for both, but the label's own minimum was its text, so with the key beside the label the rail asked
   430 px instead of 340 and the wrap (which only fires when the column is narrower than the line) could never fire.
   A window that opened at its default 1100 px could not be narrowed to the stated minimum (content minimum 1101 px,
   measured). The label now ellipsizes, so its minimum is not its text. Guard:
   `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` (red at the parent: content
   minimum 1091 x 749 against 1010 x 790).
2. **The hint bar asked for all its groups until its first tick.** A box's minimum is its children's, so the bar's
   tail only left after the window was already sized (1028 px wide at `min`; a window never gets smaller). The bar now
   starts with its first group only and the tick restores the rest. Guard:
   `smoke::a_fresh_hint_bar_asks_for_its_first_group_only` (red at the parent: a four-group bar asks 1016 px, its first
   group alone 239).

Two more things were wrong, found in the design read and fixed in the same PR:

3. **The Not found row and the Add tags dialog read 812 px tall at `min` on `ci`** (`review-nomatch-min`,
   `review-addtags-min`), 32 px over the stated height, while every other state read 780 and the scenarios that resize
   the window afterwards read 780 too. The `TALLEST` and `FRAME` lines `shot` prints (`worktrees/DAN-2219-ci-31345bd.log`)
   show the settled content minimum was 758 on `ci` in every state: 812 was a **first-layout peak the window keeps**.
   It reproduces on the author's host on the shot path (frame 2: content minimum 809, the window 1000 x 809 for one
   frame, then back to 780). Cause: `Keyed::wrap_when_narrow`'s tick read a row with no width yet (`0`) as too narrow
   and wrapped every key under its label, so the rail's button grid asked 4 x 50 where it asks 32 + 32 + 50 + 32 once
   laid out. `key_wraps(beside, width)` is now false at width 0. Guard:
   `review_view::tests::a_row_with_no_width_yet_does_not_wrap_its_key` (red without the `width > 0`), and the hosted
   `shot` step reading 1000 x 780 for both states. That test pins the helper, not its call site: putting the call
   `let wrapped = key_wraps(beside, width);` back to `beside > width` leaves it, and both `…keeps_the_stated_minimum`
   scenarios, green (DAN-2366 mutation M-B); only the hosted `shot` step reads that, and it asserts nothing. The two smoke scenarios
   `review_with_a_not_found_row_selected_keeps_the_stated_minimum` and `review_with_add_tags_open_keeps_the_stated_minimum`
   assert the same outcome, but they resize a window that is already shown, so they pass before the fix as well: they are
   outcome guards, not the regression test. The Add tags dialog itself asks nothing extra (its own minimum is 0).
4. **The readout and the title were cut with room to spare**, under the header (`SIMILARIT…` with about 600 px free) and
   beside the tools (`01 // COMPAR…`, `SIMILARIT…` in the 1440 x 900 shot, host and `ci` toolkits alike). Cause: GTK
   allocates an ellipsizing label its natural width, and Pango ellipsizes a tracked label (CSS `letter-spacing`) at that
   very width, so the label is drawn cut by its own spacing. Nothing in the header ellipsizes now: the title and the
   readout's parts sit in a scrolled window with no scroll policy and `propagate_natural_width` (`review_view::give_way`),
   which asks its natural width and no minimum, so a squeezed header clips them instead of cutting a word, and the
   header's minimum still does not include them. Guards, each shown red with the scrolled window removed (`ellipsize End`
   put back on the title: `'01 // Compare' is drawn cut beside the tools (107 px given)`; on the readout:
   `'Similarity' is drawn cut beside the tools (76 px given)`): `smoke::the_readout_shows_in_the_row_under_the_header_once_it_has_dropped_there`,
   which asserts `layout().is_ellipsized()` is false on the title and both readout words while the readout is beside the
   tools. `smoke::the_readout_line_under_the_header_is_not_cut_and_carries_the_size_chip` reads the line under the header
   (words not ellipsized, `▲ 2× larger` chip shown); it goes red when the readout's parts are put back on `ellipsize End`
   with no `give_way`: `smoke.rs:4423` at `0f8db17` (the `ellipsize()` assertion), `'Similarity' may be cut on the line under the header` (DAN-2366 mutation M-D).
   The scrolled windows are not Tab stops (`give_way` calls `set_focusable(false)`; GTK4 scrolled windows take the focus
   by default, so the first Tab on Review had landed on the static title, DAN-2366). Guard:
   `smoke::no_clipping_wrapper_on_the_review_page_is_a_tab_stop`, with a match selected so the readout's wrapper exists:
   it reads `is_focusable` on every scrolled window and viewport around the title and around the readout's label (the
   Tab chain keys on it).
   Red with `set_focusable(false)` removed from `give_way`: `GtkScrolledWindow around the title is a Tab stop`; red
   with it left only on the title's wrapper: `GtkScrolledWindow around the readout is a Tab stop`.
   `set_focusable(false)` was not enough (DAN-2384): a scrolled window that is not focusable still answers `child_focus`
   with `true` and moves nothing, so Tab stalled on Show in Queue and the rest of the page was unreachable by keyboard.
   That stall was what the walk above had hit. `give_way` also calls `set_can_focus(false)`, which takes the wrapper out
   of the chain. Guard: `smoke::tab_walks_the_review_page_from_the_header_to_the_footer`, which taps real Tab key events
   (`common::xtest`) from Side by side and asserts the walk never stays on one widget and reaches Show in Queue and then
   Next. Red with `set_can_focus(false)` removed: `Tab 2 stays on GtkButton Some("Show this image in the Queue list.")`.
   The chip was empty by data: the shot fixture gave the local file no size, so the comparison saw 0 x 0; it does now
   (640 x 480 against 1280 x 960 reads `▲ 2× larger`).

## 4. Differences from Python, on purpose

| # | Difference | Why |
|---|---|---|
| D1 | At the stated minimum the readout drops to a line under the header until the compare column can hold it beside the tools (the mockup's place at wider windows). | Minimum size (section 3); `fit_readout`. |
| D2 | The keyed buttons in the rail wrap their key under the label when the column is too narrow (the mockup's `.btn-grid .btn--keyed { flex-wrap: wrap }`), the label ellipsizes, and rail buttons use 8 px side padding (the switch 6 px). | Minimum size. |
| D3 | The hint bar asks for no width of its own, shows its first group until it has been given one, and loses its tail at narrow widths. | Minimum size. |
| D4 | `update_preview(None)` returns early in Python, before `refresh_review`, so "Match 1 of 3" and the steppers outlive a deselect. Here they follow the state: nothing selected reads `No match`. | A Python quirk, not a feature. |
| D5 | The Mark button's label flips only on the next selection change in Python; here it follows the marked state at once. | As above. |
| D6 | The picker marks an estimated score with `~` only where Python's `score_is_measured` says so: the flag **or** an engine that reports a real similarity. | Found by the differential's `detail` key (section 5); `review_ops::tests::picker_item_marks_only_estimated_scores`. |
| D7 | Wipe, Differences, Pop out, the Hydrus buttons and the Shortcuts list are present and insensitive with a tooltip that says so. | Section 2. |
| D8 | The Add tags dialog is an `AlertDialog` with a text view, and the Edit match conditions dialog an `AlertDialog` over four controls. | The dialog seam (`Prompter`), as 6.c's D1. |
| D9 | The `01 // Compare` title is clipped, not ellipsized, when the header is squeezed (`give_way`). | Hosted `ci`'s fonts are wider than the author's and the header asked 1016 px of the stated 1010 (measured on `ci`: the header 596 px against the pictures' 580); with the title giving way the column's width is the pictures', 580, on both hosts. The window's content minimum is 1000 on both. |
| D10 | With no candidates the picker is a disabled drop-down that reads GTK's own `(None)`. Python clears its combo (an empty disabled box), and the mockup (`review-no-match.html`) draws a disabled picker reading `No candidates`. | Not changed here: the string would need a placeholder row the picker's model does not have. Judged in the design read; a follow-up if it counts. |
| D11 | An image with no tags shows an empty bordered tag panel. Python's list is empty too; the mockup's no-match screen puts an empty note under the picker (`Nothing matched this image. No engine returned a match above the similarity floor. Add tags by hand, or search again from the Queue.`). | Not changed here: copy for a state Python does not have; the Not found frame and `Match — none` say it already. Judged in the design read. |

`docs/gtk-6e.md` D5 (the status line after a run reads `Search finished` where Python's ends at `Ready`) is
changed there to the ruled race class; section 5 has the counts.

## 5. The action differential

The 6.e scenario file (40 cases per seed, seeds 1 to 4) is run against both sides under the exact matcher of the
[DAN-2339](/DAN/issues/DAN-2339) ruling: the pair `Ready` / `Search finished` and nothing else differing in the step.
Python is the oracle at `76c85c4` (`worktrees/DAN-2219-oracle`); the log is `worktrees/DAN-2219-differential.log`
(first pass) and `worktrees/DAN-2219-differential-c1.log` (the head of this PR). The digest of each step gained a
`detail` key: the tag list, the match label, the picker's items and index, and the matched caption.

| Seed | Steps | Differing, unruled | Ready race (steps) | Scenarios with it | D3 of 6.e (dialogs) |
|---|---|---|---|---|---|
| 1 | 186 | 0 | 26 | 9 | 2 |
| 2 | 189 | 0 | 35 | 13 | 5 |
| 3 | 193 | 0 | 30 | 11 | 8 |
| 4 | 179 | 0 | 31 | 10 | 4 |

The race class is 122 of 747 steps in 43 of 160 scenarios. In every one of them the digest's `detail` key reads an
identical pane on both sides: the scripted candidates have no site to fetch from, so Python's extra detail fetch
(which is what overwrites its status line with `Ready`) changes nothing the pane shows. That is condition 2 of the
ruling. The `detail` key found a real defect in the port on its first run (D6 above).

## 6. Shots and contrast

Rendered by `crates/hatate-gtk/examples/shot.rs` (a test-only tool; the shipped binary has no way to do this) on
Xvfb with a temp profile and synthetic sessions: three rows with 640 x 480 pictures, a Good one with two matches and
three tags, a Not found one, a Good one with one match. The set below is the **host toolkit's** (GTK 4.24),
`worktrees/DAN-2219-shots`; the `ci` job renders the same states on the CI toolkit (GTK 4.14 / libadwaita 1.5) and
uploads them as the `shots` artifact (`.github/workflows/ci.yml` gains the Review states in this PR). Files, each
`-dark` and `-light`, at 1100 x 800 and (`-min`) the stated minimum:

- `review`: row 1 selected, Side by side, two matches, tag list, readout
- `review-empty`: nothing selected
- `review-nomatch`: the Not found row
- `review-picker`: the candidate picker open (the popup is its own surface: `review-picker-*-popover.png`)
- `review-addtags`, `review-conditions`: the two dialogs
- `review-focus-{combo,mark,next,addtags,taglist,view}`: that control focused with the ring showing (`review-focus-mark` at the minimum)

The contrast table is `shots/contrast-host.md`, computed by `worktrees/DAN-2219-tools/contrast.py` (the extreme-pixel
method, the label boxes from each shot's `.rects`; in the dialog shots only the dialog's own labels, the page behind
the scrim is not read): 1970 rows (1508 pass, 458 disabled-exempt, **4 fail**). The four fails are one label:

| review-nomatch-dark | — | #635f5b | #0d0d0d | 3.07 | dark | FAIL |
| review-nomatch-light | — | #878277 | #e8e1d2 | 2.94 | light | FAIL |
| review-nomatch-min-dark | — | #635f5b | #0d0d0d | 3.07 | dark | FAIL |
| review-nomatch-min-light | — | #878277 | #e8e1d2 | 2.94 | light | FAIL |

That is the `—` the frame caption shows for a row with no match. It is the same caption class (`frame-caption`,
`ink_65`) as `Match — none` beside it, which measures 5.26:1 (dark) in the same shot; the dash is a one-pixel stroke,
no pixel of which reaches full coverage, so the extreme-pixel method reads its anti-aliased edge. It is a measurement
limit of a thin glyph, not a lighter colour. The least of the passing rows:

| review-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |
| review-empty-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |
| review-empty-min-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |
| review-focus-addtags-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |
| review-focus-combo-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |
| review-focus-mark-dark | ETA — | #827c76 | #0d0d0d | 4.71 | dark | pass |

Per-state outer rects at the minimum are the table in section 3. The Tab order and the focus rings are in the
`review-focus-*` shots.

## 7. Method map

Python definitions in scope at `76c85c4`, with the Rust each one became (located by name with `rg`, then read; a row that
says something else is a deliberate difference). Definitions of `gui/main_window.py` are the 21 that `docs/gtk-shell.md`
section 6 gives to 6.f.

### `gui/main_window.py`

| Python | Rust |
|---|---|
| `gui/main_window.py:1099` `_update_preview` | `crates/hatate-gtk/src/review_ops.rs:352` |
| `gui/main_window.py:1158` `_populate_candidate_combo` | `crates/hatate-gtk/src/review_ops.rs:497` |
| `gui/main_window.py:1186` `_sync_candidate_combo_tooltip` | `crates/hatate-gtk/src/review_ops.rs:523` |
| `gui/main_window.py:1194` `_on_candidate_combo_changed` | `crates/hatate-gtk/src/review_ops.rs:535` |
| `gui/main_window.py:1207` `_ensure_candidate_loaded` | `crates/hatate-gtk/src/review_ops.rs:566` |
| `gui/main_window.py:1238` `_run_dialog` | `dialogs.rs` `Prompter::match_conditions` (the dialog is an `AlertDialog`; no helper to port) |
| `gui/main_window.py:1265` `_on_candidate_fetched` | `crates/hatate-gtk/src/review_ops.rs:625` |
| `gui/main_window.py:1293` `_refresh_tag_list` | `crates/hatate-gtk/src/review_ops.rs:677` |
| `gui/main_window.py:1314` `_on_edit_tags_toggled` | `crates/hatate-gtk/src/review_ops.rs:734` |
| `gui/main_window.py:1319` `_on_tag_item_edited` | `crates/hatate-gtk/src/review_ops.rs:746` |
| `gui/main_window.py:1347` `_show_matched_preview_menu` | `crates/hatate-gtk/src/review_ops.rs:885` |
| `gui/main_window.py:2170` `_add_tags_to_current` | `crates/hatate-gtk/src/review_ops.rs:832` |
| `gui/main_window.py:2481` `_review_the_waiting` | `crates/hatate-gtk/src/window.rs:591` |
| `gui/main_window.py:3037` `_set_rows_reviewed` | `crates/hatate-gtk/src/table_actions.rs:323` |
| `gui/main_window.py:3224` `_open_compare_dialog` | **6.f-2** (DAN-2340) |
| `gui/main_window.py:3651` `_sync_preview_to_selection` | `review_ops.rs` `on_selection_changed` / `update_preview` (the mode half is 6.f-2) |
| `gui/main_window.py:3689` `action_open_matched_url` | `crates/hatate-gtk/src/review_ops.rs:939` |
| `gui/main_window.py:3693` `action_show_in_queue` | `crates/hatate-gtk/src/review_ops.rs:963` |
| `gui/main_window.py:3704` `_current_entry` | `review_ops.rs` `current_id` |
| `gui/main_window.py:3719` `_remove_selected_tags` | `crates/hatate-gtk/src/review_ops.rs:784` |
| `gui/main_window.py:4099` `action_edit_conditions` | `crates/hatate-gtk/src/review_ops.rs:813` |

### `gui/review_view.py`

| Python | Rust |
|---|---|
| `gui/review_view.py:93` `_build_review_page` | `crates/hatate-gtk/src/review_ops.rs:229` |
| `gui/review_view.py:136` `_build_comparison_card` | `review_view.rs` `ReviewPage::new` (`compare_col`) |
| `gui/review_view.py:183` `_build_comparison_readout` | `review_view.rs` `ReviewPage::new` (`readout_root`) |
| `gui/review_view.py:212` `set_comparison_readout` | `review_view.rs` `ReviewPage::set_readout`, `fit_readout` |
| `gui/review_view.py:232` `_build_zoom_tools` | **6.f-2** (Wipe's zoom row) |
| `gui/review_view.py:263` `_build_wipe_slider_row` | **6.f-2** |
| `gui/review_view.py:292` `_build_pair_view` | `review_view.rs` `ReviewPage::new` (the two frames; Side by side only) |
| `gui/review_view.py:346` `_set_review_captions` | `crates/hatate-gtk/src/review_ops.rs:447` |
| `gui/review_view.py:368` `_build_wipe_view` | **6.f-2** |
| `gui/review_view.py:394` `_build_decision_card` | `review_view.rs` `ReviewPage::new` (`review-rail`), `Keyed` |
| `gui/review_view.py:538` `_build_position_strip` | `review_view.rs` `ReviewPage::new` (`hint_slot`) and `review_ops.rs` `refresh_review_strip` |
| `gui/review_view.py:574` `_run_review_action` | `crates/hatate-gtk/src/review_ops.rs:1197` |
| `gui/review_view.py:579` `_resolved_shortcuts` | `crates/hatate-gtk/src/review_ops.rs:218` |
| `gui/review_view.py:583` `_key_for` | `crates/hatate-gtk/src/review_ops.rs:224` |
| `gui/review_view.py:587` `_hint_bar_groups` | `crates/hatate-gtk/src/review_ops.rs:1056` |
| `gui/review_view.py:619` `_show_shortcuts_dialog` | **not ported** (section 2): the button is insensitive with its tooltip |
| `gui/review_view.py:667` `_show_comparison_controls` | **6.f-2** (Wipe's chrome) |
| `gui/review_view.py:675` `_current_review_key` | `review_view.rs` `ReviewPage::current_view` |
| `gui/review_view.py:682` `_on_review_view_changed` | **6.f-2** (the Wipe and Differences buttons are insensitive) |
| `gui/review_view.py:697` `_ensure_comparison_loaded` | **6.f-2** |
| `gui/review_view.py:751` `full_resolution_match` | **6.f-2** |
| `gui/review_view.py:761` `_apply_wipe_mode` | **6.f-2** |
| `gui/review_view.py:771` `_show_differences` | **6.f-2** |
| `gui/review_view.py:804` `_step_review_candidate` | `crates/hatate-gtk/src/review_ops.rs:554` |
| `gui/review_view.py:810` `_toggle_reviewed_from_review` | `crates/hatate-gtk/src/review_ops.rs:1191` |
| `gui/review_view.py:816` `_pop_out_comparison` | **6.f-2** (Pop out is insensitive with its tooltip) |
| `gui/review_view.py:819` `refresh_review` | `crates/hatate-gtk/src/review_ops.rs:978` |
| `gui/review_view.py:860` `_refresh_review_strip` | `crates/hatate-gtk/src/review_ops.rs:1013` |
| `gui/review_view.py:878` `_refresh_review_keycaps` | `crates/hatate-gtk/src/review_ops.rs:1036` |

### `gui/review_shortcuts.py`

| Python | Rust |
|---|---|
| `gui/review_shortcuts.py:36` `_selected_rows` | `crates/hatate-gtk/src/review_ops.rs:1129` |
| `gui/review_shortcuts.py:40` `_selected_entries` | `review_ops.rs` `selected_rows` |
| `gui/review_shortcuts.py:51` `_move_selection` | `crates/hatate-gtk/src/review_ops.rs:1108` |
| `gui/review_shortcuts.py:75` `_move_to_unreviewed` | `crates/hatate-gtk/src/review_ops.rs:1142` |
| `gui/review_shortcuts.py:99` `_toggle_reviewed` | `crates/hatate-core/src/mcp/handlers.rs:39` |
| `gui/review_shortcuts.py:110` `_step_candidate` | `crates/hatate-gtk/src/review_ops.rs:1216` |
| `gui/review_shortcuts.py:126` `handlers` | `review_ops.rs` `run_review_action` (a `match` on the action id) |
| `gui/review_shortcuts.py:154` `_guarded` | `crates/hatate-core/src/boorus/fetch.rs:193` |
| `gui/review_shortcuts.py:168` `run` | `crates/hatate-cli/src/batch_cmd.rs:107` |
| `gui/review_shortcuts.py:185` `install` | `crates/hatate-gtk/src/menubar_keys.rs:49` |
| `gui/review_shortcuts.py:232` `open_matched_url` | `review_ops.rs` `action_open_matched_url` |

### `gui/add_tags_dialog.py`

| Python | Rust |
|---|---|
| `gui/add_tags_dialog.py:16` `__init__` | `dialogs.rs` `Prompter::add_tags` (an `AlertDialog` with a text view) |
| `gui/add_tags_dialog.py:36` `get_tags` | `add_tags.rs` `parse_added_tags` |

### `gui/match_conditions_dialog.py`

| Python | Rust |
|---|---|
| `gui/match_conditions_dialog.py:17` `__init__` | `dialogs.rs` `Prompter::match_conditions` (four controls over OK and Cancel) |
| `gui/match_conditions_dialog.py:57` `apply_to` | `review_ops.rs` `action_edit_conditions` (the closure stores and saves on OK) |

### `gui/preview_text.py`

| Python | Rust |
|---|---|
| `gui/preview_text.py:43` `human_size` | `crates/hatate-core/src/preview_text.rs:49` |
| `gui/preview_text.py:54` `matched_caption` | `crates/hatate-core/src/preview_text.rs:70` |
| `gui/preview_text.py:74` `no_candidate_text` | `crates/hatate-core/src/preview_text.rs:90` |
| `gui/preview_text.py:89` `_dimensions` | `crates/hatate-core/src/boorus/eshuushuu.rs:557` |
| `gui/preview_text.py:93` `local_image_info_text` | `crates/hatate-core/src/preview_text.rs:129` |
| `gui/preview_text.py:104` `local_frame_meta` | `crates/hatate-core/src/preview_text.rs:140` |
| `gui/preview_text.py:111` `_local_info_parts` | `crates/hatate-core/src/preview_text.rs:111` |
| `gui/preview_text.py:129` `matched_frame_label` | `crates/hatate-core/src/preview_text.rs:147` |
| `gui/preview_text.py:143` `matched_frame_meta` | `crates/hatate-core/src/preview_text.rs:181` |
| `gui/preview_text.py:160` `matched_image_info_text` | `crates/hatate-core/src/preview_text.rs:190` |
| `gui/preview_text.py:193` `comparison_banner_text` | `crates/hatate-core/src/preview_text.rs:224` |
| `gui/preview_text.py:233` `comparison_banner_tier` | `crates/hatate-core/src/preview_text.rs:254` |
| `gui/preview_text.py:249` `comparison_banner_colour` | `preview_text.rs` `comparison_banner_tier` (the colour is the CSS class of the tier) |
| `gui/preview_text.py:271` `comparison_readout` | `crates/hatate-core/src/preview_text.rs:296` |

### `gui/compare_dialog.py`

All 44 definitions (`CompareView`, `CompareDialog` and their helpers, 857 lines) are **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)).


## 8. Python test map

An `ast` walk over every `tests/test_*.py` at `76c85c4` (`worktrees/DAN-2219-tools/map/scan6f.py`, run outside the repo)
finds **114** tests whose source mentions a symbol this ticket ports (the method names of the ticket, the modules
`review_view`, `review_shortcuts`, `preview_text`, `compare_dialog`, `add_tags_dialog`, `match_conditions_dialog`). The
walk follows class helpers as well as test bodies: a test also owns `setUp`, `setUpClass` and every `self.<m>()` it
reaches, transitively. The first version read test bodies only and missed the 16 tests of `AddTagsDialog.get_tags` in
`tests/test_add_tags_dialog.py` and `tests/test_thin_modules.py`, which build the dialog in `setUp` and `_dialog`
(DAN-2347). The rows below are the **194** that `docs/gtk-shell.md` section 7 assigns to 6.f (or to 6.d/6.f) plus the
tests the walk finds that section does not list; the walk's other hits are rows that section already maps to another
ticket (6.c: 2, 6.d: 2, 6.h: 3, core: 2, n/a: 4) and are not repeated. Submitted at zero unmapped by the
class-helper-aware walk (101 hits are rows here, 13 are another owner's, 0 are in neither).

`smoke::` is a scenario of `crates/hatate-gtk/tests/smoke.rs`; `module::tests::` is a unit test in that module;
`preview_text::tests::` is in `hatate-core`. A row that says `not asserted` names a Python assertion no Rust test
makes; `geometry` rows are judged in the shot set and the measured tables, as in the earlier Phase 6 docs.


#### `tests/test_add_tags_dialog.py` (6)

| Line | Test | Disposition |
|---|---|---|
| 45 | `GetTags.test_a_plain_line_has_no_namespace` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: `  cat ` is `(None, "cat")` |
| 51 | `GetTags.test_namespace_colon_name_splits_cleanly` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: ` character : reimu ` is `(Some("character"), "reimu")`; `add_tags::tests::only_the_first_colon_splits`; `smoke::review_tag_panel_adds_removes_and_edits_in_place`: ` series : touhou ` adds `[User] series:touhou`. The tight `character:reimu` is not its own input; it is the same split |
| 56 | `GetTags.test_a_space_before_the_colon_must_not_end_up_in_the_namespace` | DAN-81 regression. `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: the namespace is `character`, not `character `. Reverting `strip(head)` in `parse_added_tags` turns it red (DAN-2347 `worktrees/DAN-2219-selfcheck-2347.log`) |
| 67 | `GetTags.test_a_trailing_space_before_the_colon_alone_is_also_stripped` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`, through the same `strip(head)` line. not asserted as its own input: `character :reimu` (a space on one side only) is not fed; the both-sides input is |
| 71 | `GetTags.test_blank_lines_are_skipped` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: the empty line between `cat` and `character : reimu` is dropped. not asserted: a whitespace-only line (`   `) is not fed; `strip` runs before the empty filter, so it takes the same path |
| 75 | `GetTags.test_one_tag_per_line` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: three tags from four lines (one blank), in order; `add_tags::tests::python_line_boundaries_end_a_tag`: five lines on five different line boundaries give five tags |

#### `tests/test_app_chrome.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 272 | `ReviewToolbar.test_three_boxed_36px_icon_buttons` | 6.f: the Review toolbar's three boxed buttons; geometry, shot set (Pop out and Shortcuts are insensitive in this build, Show in Queue is live) |

#### `tests/test_flat_surfaces.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 175 | `RegionsAreFlatHairlines.test_review_columns_paint_the_page` | 6.f: the Review columns paint the page; geometry, shot set |

#### `tests/test_gui_smoke.py` (110)

| Line | Test | Disposition |
|---|---|---|
| 343 | `MainWindowSignalHandlers.test_selection_and_preview_handlers` | `smoke::review_page_follows_the_selection` (the selection drives the preview, captions, readout, picker and tag list; nothing selected empties them); `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop` for the picker |
| 624 | `DialogLifetime.test_compare_dialogs_do_not_accumulate` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 674 | `CompareViewZoom.test_new_images_start_fitted` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 680 | `CompareViewZoom.test_actual_size_is_one_image_pixel_per_screen_pixel` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 685 | `CompareViewZoom.test_zoom_keeps_the_point_under_the_cursor` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 701 | `CompareViewZoom.test_panning_cannot_lose_the_image_offscreen` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 711 | `CompareViewZoom.test_an_axis_that_fits_stays_centred` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 718 | `CompareViewZoom.test_zoom_is_bounded` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 725 | `CompareViewZoom.test_painting_at_every_zoom_and_split` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 735 | `CompareViewZoom.test_wipe_line_colour_is_mode_invariant` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 755 | `CompareViewZoom.test_empty_view_is_safe` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 766 | `CompareViewZoom.test_only_one_image_still_zooms` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 1898 | `CompareUsesTheSelectedCandidate.test_an_unvisited_candidate_has_its_details_fetched` | `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop`: the third match has nothing fetched, the fetch goes to the site (a socket that never answers) and the page says `Loading picture…`; what the fetch fills in on arrival is not asserted |
| 1905 | `CompareUsesTheSelectedCandidate.test_a_candidate_that_already_has_a_url_is_not_refetched` | `smoke::review_page_follows_the_selection`: rows whose matches are already fetched select without asking anyone; that `ensure_candidate_loaded` skips a candidate that already has its URL fields is its guard, not asserted by its own test |
| 1910 | `CompareUsesTheSelectedCandidate.test_each_candidate_gets_its_own_picture` | not asserted: that each candidate shows its own picture. The picker scenario asserts the label, the caption and the entry's match follow the pick, with the same 4x3 test picture behind every candidate |
| 1941 | `CompareUsesTheSelectedCandidate.test_a_match_that_will_not_load_says_so` | not asserted: the text for a match whose picture will not load. A stalled fetch is covered (`smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop`); a fetch that returns an error is not driven |
| 1950 | `CompareUsesTheSelectedCandidate.test_a_failing_fetch_does_not_take_the_window_down` | `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop`: the main loop keeps running (a mark action and 200 ms of ticks while the fetch waits); a fetch that fails outright is not driven |
| 3798 | `PreviewClearsWhenRowsAreRemoved.test_a_picture_is_shown_while_a_row_is_selected` | `smoke::review_page_follows_the_selection`: the picture is shown while a row is selected (`local.shows_picture()`) |
| 3804 | `PreviewClearsWhenRowsAreRemoved.test_removing_another_row_keeps_your_place` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`; that the Review page follows is `smoke::review_page_follows_the_selection` |
| 3820 | `PreviewClearsWhenRowsAreRemoved.test_removing_the_selected_row_moves_to_the_next` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`; that the Review page follows is `smoke::review_page_follows_the_selection` |
| 3829 | `PreviewClearsWhenRowsAreRemoved.test_an_auto_import_removal_leaves_the_watched_row_alone` | not asserted: the auto-import removal of a row other than the watched one, with Review open. The removal is 6.e's `replace_row` / `remove_row` path; the differential does not script auto-import |
| 3837 | `PreviewClearsWhenRowsAreRemoved.test_an_auto_import_removing_the_watched_row_moves_on` | not asserted: as above, for the watched row |
| 3846 | `PreviewClearsWhenRowsAreRemoved.test_the_matched_side_follows_as_well` | `smoke::review_page_follows_the_selection`: the matched pane, caption and label follow the selected row |
| 3857 | `PreviewClearsWhenRowsAreRemoved.test_the_tag_list_and_readout_follow_as_well` | `smoke::review_page_follows_the_selection`: the tag list and the readout follow the selected row |
| 3869 | `PreviewClearsWhenRowsAreRemoved.test_removing_everything_clears_it` | `smoke::review_page_follows_the_selection`: selecting nothing empties the page again; removing every row is 6.d's (`smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`) |
| 3875 | `PreviewClearsWhenRowsAreRemoved.test_selecting_again_afterwards_still_works` | `smoke::review_page_follows_the_selection`: the page is filled again after it was emptied |
| 3895 | `PreviewText.test_the_caption_names_the_site_and_the_score` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement`: `Matched image: Danbooru (95% similar)` |
| 3900 | `PreviewText.test_the_caption_omits_what_it_does_not_have` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement` |
| 3905 | `PreviewText.test_a_site_with_no_score_still_reads` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement` |
| 3910 | `PreviewText.test_not_searched_and_not_found_are_not_confused` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match` |
| 3920 | `PreviewText.test_searching_says_so` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match` |
| 3926 | `PreviewText.test_an_error_carries_its_message` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`: `Search error: ascii2d blocked` |
| 3933 | `PreviewText.test_an_error_with_no_message_does_not_say_none` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match`: `Search error: ` with an empty message |
| 3939 | `PreviewText.test_match_info_omits_pieces_that_are_unknown` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason` |
| 3947 | `PreviewText.test_match_info_with_nothing_at_all_says_so` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason` |
| 3953 | `PreviewText.test_no_candidate_is_blank_not_a_placeholder` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`: `matched_image_info_text(None)` is empty |
| 3957 | `PreviewText.test_human_size_scales_units` | `preview_text::tests::sizes_are_human_sized_and_never_past_gb` |
| 3964 | `PreviewText.test_the_banner_is_empty_without_a_candidate` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade`: no candidate, no text |
| 3971 | `PreviewText.test_a_bigger_match_reads_as_good` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade` |
| 3978 | `PreviewText.test_a_smaller_match_is_not_green` | `preview_text::tests::the_banner_says_whether_the_match_is_an_upgrade` |
| 3987 | `PreviewText.test_the_banner_states_the_similarity` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 3994 | `PreviewText.test_local_info_reports_unreadable_files_plainly` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk`: an unreadable file reads the size only |
| 4185 | `NoTagsReasonIsShown.test_the_reason_appears_under_the_match_info` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; the label under the match info is `matched_note`, set by `update_preview` (not asserted by a window test) |
| 4192 | `NoTagsReasonIsShown.test_it_is_on_its_own_line` | not asserted: that the reason is on its own line. It is its own label (`matched_note`, wrapped); the shape is judged in the shot set |
| 4200 | `NoTagsReasonIsShown.test_a_match_with_tags_shows_no_reason` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; the label under the match info is `matched_note`, set by `update_preview` (not asserted by a window test) |
| 4205 | `NoTagsReasonIsShown.test_a_reason_still_shows_when_nothing_else_is_known` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`; the label under the match info is `matched_note`, set by `update_preview` (not asserted by a window test) |
| 4213 | `NoTagsReasonIsShown.test_the_reason_survives_the_search_cache` | not asserted here: the search cache keeps the reason on the candidate. The cache is core (Phase 5) |
| 4482 | `BorrowedTagsAreLabelled.test_the_preview_names_the_match_the_tags_came_from` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason`: the line names the match the tags came from |
| 4492 | `BorrowedTagsAreLabelled.test_a_match_with_its_own_tags_says_nothing_extra` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason` |
| 4499 | `BorrowedTagsAreLabelled.test_the_provenance_survives_the_search_cache` | not asserted here: the search cache keeps the provenance on the candidate. The cache is core (Phase 5) |
| 4555 | `CompareDifferencesMode.test_a_pixmap_survives_the_round_trip_intact` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4565 | `CompareDifferencesMode.test_the_overlay_is_the_local_image_s_shape` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4574 | `CompareDifferencesMode.test_it_reports_what_it_found` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4582 | `CompareDifferencesMode.test_both_pictures_are_shown_with_the_overlay_over_them` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4593 | `CompareDifferencesMode.test_the_view_frames_the_pair_not_one_picture` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4602 | `CompareDifferencesMode.test_the_overlay_keeps_its_transparency` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4609 | `CompareDifferencesMode.test_the_wipe_moves_the_split_in_the_differences_view` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4617 | `CompareDifferencesMode.test_the_two_sides_are_the_same_size_so_the_split_registers` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4626 | `CompareDifferencesMode.test_the_wipe_sweeps_the_overlay_across_both_panes` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4662 | `CompareDifferencesMode.test_the_slider_is_labelled_for_the_mode` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4671 | `CompareDifferencesMode.test_leaving_the_mode_goes_back_to_the_ordinary_wipe` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4680 | `CompareDifferencesMode.test_the_overlay_is_built_once_and_reused` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4690 | `CompareDifferencesMode.test_the_mode_is_unavailable_without_both_images` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 4700 | `CompareDifferencesMode.test_the_mode_is_available_once_both_are_loaded` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 5083 | `ReviewDecisionCardSizing.test_declared_minimum_width_fits_the_button_grid` | `smoke::minimum_size_fits_every_state_in_both_schemes` and `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide`: at the stated minimum the page asks for no more than the window gives it, with a match selected; section 3 for the numbers |
| 5097 | `ReviewDecisionCardSizing.test_buttons_dont_clip_when_squeezed_to_the_declared_minimum` | `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide`; the decision card's buttons at the minimum are in `review-min-{dark,light}.png` |
| 5213 | `ReviewShortcuts.test_every_registered_action_has_a_handler` | `review_keys::tests::every_default_binding_has_a_trigger` (every registered action has a trigger); the handler per id is `run_review_action`, driven for six of them (next and previous row, next and previous unreviewed, mark, open match) by `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5222 | `ReviewShortcuts.test_they_are_scoped_to_the_table_not_the_window` | not asserted: that the keys are scoped to the table and the page and not the window. The controllers are on `review.root` and the Queue's table (`wire_review_page`) |
| 5233 | `ReviewShortcuts.test_j_and_k_work_on_the_review_page` | `smoke::review_keys_step_mark_skip_and_open_the_match`: J / K on the Review page as real X key events (under Xvfb; skipped elsewhere, as the scenario prints) |
| 5249 | `ReviewShortcuts.test_sending_the_current_image_away_keeps_your_place` | **6.g**: sending to Hydrus. The Send buttons are insensitive in this build; the selection rule after a removal is 6.d's (`smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`) |
| 5268 | `ReviewShortcuts.test_sending_the_last_row_away_lands_on_the_new_last_row` | **6.g**: sending to Hydrus. The Send buttons are insensitive in this build; the selection rule after a removal is 6.d's (`smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`) |
| 5276 | `ReviewShortcuts.test_a_hydrus_confirmation_removing_your_row_moves_you_on` | **6.g**: sending to Hydrus. The Send buttons are insensitive in this build; the selection rule after a removal is 6.d's (`smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`) |
| 5293 | `ReviewShortcuts.test_a_hydrus_confirmation_elsewhere_leaves_you_where_you_are` | **6.g**: sending to Hydrus. The Send buttons are insensitive in this build; the selection rule after a removal is 6.d's (`smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`) |
| 5304 | `ReviewShortcuts.test_review_page_buttons_move_between_images` | `smoke::review_page_follows_the_selection` (`smoke.rs:4084`, first row: Previous off; `:4099`, last row: Next off): Previous / Next page buttons are insensitive at the ends and move through `next_row` / `prev_row`; `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5319 | `ReviewShortcuts.test_typing_in_the_filter_box_is_not_swallowed` | `smoke::review_keys_step_mark_skip_and_open_the_match`: a text field keeps its characters (the scenario's last block) |
| 5334 | `ReviewShortcuts.test_j_and_k_step_through_the_list` | `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5344 | `ReviewShortcuts.test_stepping_stops_at_the_ends_rather_than_wrapping` | `smoke::review_keys_step_mark_skip_and_open_the_match`: stops at the end rather than wrapping |
| 5355 | `ReviewShortcuts.test_n_skips_to_images_still_needing_a_decision` | `smoke::review_keys_step_mark_skip_and_open_the_match`: N skips what is decided |
| 5363 | `ReviewShortcuts.test_n_says_so_rather_than_doing_nothing_at_the_end` | `smoke::review_keys_step_mark_skip_and_open_the_match`: `Nothing left to review after this one` |
| 5370 | `ReviewShortcuts.test_shift_n_searches_backwards` | `smoke::review_keys_step_mark_skip_and_open_the_match`: `prev_unreviewed` |
| 5376 | `ReviewShortcuts.test_an_action_needing_one_row_says_so_on_a_multi_selection` | not asserted: the multi-selection message of the single-row actions |
| 5385 | `ReviewShortcuts.test_an_action_needing_a_selection_says_so_when_there_is_none` | `smoke::review_keys_step_mark_skip_and_open_the_match`: Open match with no match says so in the status line; the other single-row actions' no-selection message is not asserted |
| 5393 | `ReviewShortcuts.test_navigation_works_with_nothing_selected` | `smoke::review_keys_step_mark_skip_and_open_the_match`: from nothing, next lands on the first |
| 5403 | `ReviewShortcuts.test_rebinding_takes_effect_without_a_restart` | 6.h (rebinding is the Settings dialog): not asserted here. The page reads the resolved shortcuts at build and on `resolved_shortcuts()` |
| 5413 | `ReviewShortcuts.test_reinstalling_does_not_leave_the_old_binding_behind` | 6.h: not asserted (no rebinding path in this build) |
| 5426 | `ReviewShortcuts.test_a_cleared_binding_installs_no_shortcut` | `review_keys::tests::a_spelling_with_no_key_is_none` |
| 5436 | `ReviewShortcuts.test_an_unparseable_binding_is_skipped_rather_than_fatal` | `review_keys::tests::a_spelling_with_no_key_is_none`: a spelling GTK cannot take is `None` and installs nothing |
| 5467 | `DynamicShortcutLabels.test_button_labels_show_effective_bindings` | not asserted: the caps are set from the resolved binding by `refresh_review_keycaps`; judged in the shot set |
| 5475 | `DynamicShortcutLabels.test_position_strip_hint_uses_dynamic_bindings` | not asserted: the strip's text; judged in the shot set. Its narrow-width behaviour is `smoke::a_fresh_hint_bar_asks_for_its_first_group_only` |
| 5489 | `DynamicShortcutLabels.test_rebinding_updates_button_labels_and_strip` | 6.h: not asserted (no rebinding path in this build) |
| 5511 | `DynamicShortcutLabels.test_shortcuts_dialog_shows_all_actions_with_bindings` | **not ported in this PR**: the Shortcuts button is built and insensitive with its tooltip (`review_ops.rs`, the `shortcuts_btn` block). `_show_shortcuts_dialog` is on neither this ticket's nor DAN-2340's list as written; flagged on the ticket for Cloud (section 2) |
| 5545 | `ShowInQueue.test_show_in_queue_from_review_selects_current_row` | not asserted for the Review page. `action_show_in_queue` selects the current row and goes to Queue; the Activity page's button is `smoke::activity_page_states_the_engines_and_tails_the_log` (`show_in_queue.emit_clicked()`) |
| 5573 | `ShowInQueue.test_show_in_queue_from_activity_switches_to_queue` | `smoke::activity_page_states_the_engines_and_tails_the_log`: Show in Queue goes to Queue |
| 5821 | `ReviewedFromTheKeyboard.test_space_marks_and_unmarks` | `smoke::review_keys_step_mark_skip_and_open_the_match`: Space marks, and the button reads `Unmark reviewed` |
| 5829 | `ReviewedFromTheKeyboard.test_n_skips_rows_already_reviewed` | `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5839 | `ReviewedFromTheKeyboard.test_marking_then_pressing_n_advances_past_it` | `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5847 | `ReviewedFromTheKeyboard.test_n_says_so_when_the_list_is_finished` | `smoke::review_keys_step_mark_skip_and_open_the_match` |
| 5855 | `ReviewedFromTheKeyboard.test_space_on_a_mixed_selection_marks_rather_than_unmarks` | not asserted: Space on a mixed selection (the scenario marks one row) |
| 5863 | `ReviewedFromTheKeyboard.test_a_pass_resumes_where_it_left_off_after_a_restart` | 6.c/6.d: not asserted here. The position is the selection; restoring a session's selection is not in this build |
| 5897 | `ReviewedFromTheKeyboard.test_space_needs_a_selection_and_says_so` | not asserted: Space with nothing selected |
| 6542 | `SideBySideIsSharp.test_it_fills_the_box_at_device_pixels` | does not apply as written: Qt scaled a `QPixmap` into the box. The pane hands the decoded texture to a `gtk::Picture` (`ContentFit::Contain`, `can_shrink`), which draws at device pixels; sharpness is judged in the shot set. The 'does not hold the box open' half is `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` and the minimum-size shots with 640x480 pictures |
| 6552 | `SideBySideIsSharp.test_it_redraws_from_the_source_when_the_box_grows` | does not apply as written: Qt scaled a `QPixmap` into the box. The pane hands the decoded texture to a `gtk::Picture` (`ContentFit::Contain`, `can_shrink`), which draws at device pixels; sharpness is judged in the shot set. The 'does not hold the box open' half is `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` and the minimum-size shots with 640x480 pictures |
| 6564 | `SideBySideIsSharp.test_a_large_picture_does_not_hold_the_box_open` | does not apply as written: Qt scaled a `QPixmap` into the box. The pane hands the decoded texture to a `gtk::Picture` (`ContentFit::Contain`, `can_shrink`), which draws at device pixels; sharpness is judged in the shot set. The 'does not hold the box open' half is `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` and the minimum-size shots with 640x480 pictures |
| 6571 | `SideBySideIsSharp.test_the_local_picture_is_decoded_past_the_old_700px` | does not apply as written: Qt scaled a `QPixmap` into the box. The pane hands the decoded texture to a `gtk::Picture` (`ContentFit::Contain`, `can_shrink`), which draws at device pixels; sharpness is judged in the shot set. The 'does not hold the box open' half is `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` and the minimum-size shots with 640x480 pictures |
| 6608 | `SideBySideReusesTheFullMatch.test_a_redraw_keeps_the_full_resolution_match` | does not apply: the matched pane is given the decoded texture and scales on draw; there is no scaled copy to reuse or to borrow |
| 6613 | `SideBySideReusesTheFullMatch.test_another_candidate_does_not_borrow_it` | does not apply: the matched pane is given the decoded texture and scales on draw; there is no scaled copy to reuse or to borrow |
| 6711 | `RemovalKeepsYourPlace.test_an_auto_import_removing_the_selected_row_lands_on_the_next` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row`; the Review page's half is `smoke::review_page_follows_the_selection` |
| 6718 | `RemovalKeepsYourPlace.test_removing_another_row_leaves_the_selection_alone` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row` |
| 6723 | `RemovalKeepsYourPlace.test_the_remove_key_lands_on_the_next_row_too` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row` |
| 6728 | `RemovalKeepsYourPlace.test_removing_the_last_row_lands_on_the_one_above` | 6.d: `smoke::queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row` |

#### `tests/test_review_commit_caption.py` (4)

| Line | Test | Disposition |
|---|---|---|
| 51 | `SendCaption.test_caption_text_and_position_beneath_send` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test; the caption's text is a constant (`send_caption`) and its tooltip is asserted by no test |
| 62 | `SendCaption.test_send_is_unchanged` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 87 | `SendCaption.test_contrast_dark` | the contrast table (section 6): the caption measures in both schemes |
| 90 | `SendCaption.test_contrast_light` | the contrast table (section 6) |

#### `tests/test_review_frames.py` (22)

| Line | Test | Disposition |
|---|---|---|
| 56 | `CaptionText.test_local_meta_is_the_mockups_line` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk` (`local_frame_meta`) |
| 65 | `CaptionText.test_local_meta_is_empty_for_an_unreadable_file` | `preview_text::tests::the_local_info_reads_the_header_and_the_disk`: an unreadable file's meta is empty |
| 68 | `CaptionText.test_a_measured_match_is_named_by_its_site_alone` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score` |
| 72 | `CaptionText.test_an_unmeasured_score_keeps_its_qualifier` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`; the picker's own `~` is `review_ops::tests::picker_item_marks_only_estimated_scores` |
| 77 | `CaptionText.test_a_match_with_no_site_and_no_score_is_just_match` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score`: no site and no score is `Match` |
| 80 | `CaptionText.test_match_meta_uses_the_caption_separators` | `preview_text::tests::the_match_info_names_borrowed_tags_and_an_incomplete_reason` (`matched_frame_meta`) |
| 95 | `FramesAndCaptions.test_the_two_frames_are_equal_and_carry_four_bracket_corners` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 114 | `FramesAndCaptions.test_the_caption_sits_below_its_frame_and_nothing_sits_above` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 127 | `FramesAndCaptions.test_captions_are_mono_caps_and_say_what_the_mockup_says` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test; the captions' text is `smoke::review_page_follows_the_selection` |
| 147 | `FramesAndCaptions.test_an_unmeasured_match_keeps_its_ranking_qualifier_in_the_caption` | `preview_text::tests::the_frame_label_repeats_only_an_unmeasured_score` |
| 157 | `FramesAndCaptions.test_the_fixture_window_starts_no_network_fetch` | not asserted as a claim. The Review scenarios run with `retrieve_tags_from_booru` off and fetch nothing; only `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop` opens a socket, on purpose |
| 165 | `FramesAndCaptions.test_no_match_says_so_in_the_caption` | `smoke::review_page_follows_the_selection`: `Match — none` and `—` for a row with no match |
| 170 | `FramesAndCaptions.test_a_long_caption_elides_instead_of_widening_the_window` | not asserted with a long caption; the caption labels ellipsize (`FrameCaption`), and the window keeps the minimum with a real caption in `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` |
| 178 | `FramesAndCaptions.test_a_parsers_reason_gets_its_own_line_and_only_when_there_is_one` | not asserted: `matched_note` is its own label; shown only when the parser gave a reason (shot set) |
| 191 | `FramesAndCaptions.test_the_wipe_view_is_framed_and_its_zoom_floats_inside_the_frame` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 218 | `WipeChrome.test_the_slider_handle_is_a_24px_target_with_a_hairline_track` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 239 | `WipeChrome.test_local_and_match_name_the_ends_of_the_slider` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 252 | `WipeChrome.test_the_zoom_row_is_boxed_mono_buttons` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 265 | `WipeChrome.test_wipe_carries_the_one_line_hint_and_a_warning_replaces_it` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 278 | `WipeChrome.test_the_wipe_chrome_stays_out_of_side_by_side` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR |
| 289 | `ViewportBudget.test_the_picture_gets_at_least_55_percent_of_the_window_height` | the per-state outer rects at the minimum (section 3): the pictures keep their share at 1000 x 780; not a test |
| 301 | `ViewportBudget.test_the_footer_still_fits_and_sits_where_the_page_ends` | `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide` (the footer fits and sits at the end of the page); section 3 |

#### `tests/test_review_keycaps.py` (18)

| Line | Test | Disposition |
|---|---|---|
| 109 | `KeyCapWidget.test_text_is_the_key_as_given_and_the_capitals_are_the_fonts` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 116 | `KeyCapWidget.test_a_cap_keeps_its_own_height_inside_a_taller_row` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 128 | `KeyCapWidget.test_keyed_button_keeps_its_label_as_text_and_exposes_the_cap` | `smoke::review_keys_step_mark_skip_and_open_the_match`: `mark_btn.text()` is the label without the cap (`Keyed::text`) |
| 134 | `KeyCapWidget.test_an_empty_key_hides_the_cap_and_gives_the_room_back` | not asserted: `Keyed::set_key` hides the cap for an empty key (no rebinding path in this build) |
| 146 | `KeysInsideTheButtons.test_every_button_that_has_a_key_shows_it_as_a_cap` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test; the caps are set by `refresh_review_keycaps` |
| 156 | `KeysInsideTheButtons.test_each_cap_sits_inside_its_button_clear_of_the_label` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 174 | `KeysInsideTheButtons.test_key_names_read_against_the_surface_they_sit_on` | the contrast table (section 6): cap text against its button, both schemes |
| 195 | `KeysInsideTheButtons.test_the_mark_label_flip_still_fits_at_the_cards_declared_minimum` | `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide`; `Unmark reviewed` is the longer label and the shots at the minimum show both |
| 214 | `KeysInsideTheButtons.test_rebinding_a_key_changes_the_cap_and_unbinding_hides_it` | 6.h: not asserted (no rebinding path in this build) |
| 227 | `MatchStepperKeys.test_bracket_keys_follow_their_steppers_and_dim_with_them` | `smoke::review_page_follows_the_selection` (`smoke.rs:4083`, first match) and `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop` (`smoke.rs:4292-4293`, last match): the Previous / Next match buttons and their bracket caps (`prev_match_key` / `next_match_key`) dim at the ends; the caps' geometry is the shot set |
| 240 | `MatchStepperKeys.test_a_cap_sits_immediately_after_its_stepper` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 250 | `HintBar.test_default_bar_reads_like_the_mockups` | not asserted: the hint bar's text; judged in the shot set |
| 260 | `HintBar.test_the_figures_are_strong_and_the_words_are_not` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 266 | `HintBar.test_the_bar_carries_the_mockups_1px_rule_across_the_page` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 281 | `HintBar.test_the_bar_is_centred_between_the_previous_and_next_buttons` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 291 | `HintBar.test_a_narrow_bar_drops_its_tail_and_keeps_the_position` | `smoke::a_fresh_hint_bar_asks_for_its_first_group_only` and `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide`: the bar asks for its first group until it has a width, then drops its tail |
| 313 | `HintBar.test_a_rebound_key_is_what_the_bar_shows` | 6.h: not asserted (no rebinding path in this build) |
| 322 | `HintBar.test_the_bar_updates_as_the_selection_moves` | not asserted: the bar's text as the selection moves (`hint_bar_groups` is rebuilt by `refresh_review_strip`) |

#### `tests/test_review_readout.py` (10)

| Line | Test | Disposition |
|---|---|---|
| 46 | `ReadoutParts.test_a_measured_score_reads_similarity_with_a_good_glyph` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 53 | `ReadoutParts.test_a_ranking_never_says_similarity` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 60 | `ReadoutParts.test_a_smaller_match_draws_no_up_arrow` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 65 | `ReadoutParts.test_no_candidate_says_nothing` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 70 | `ReadoutParts.test_the_tooltip_keeps_what_the_banner_said` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |
| 113 | `ReadoutInTheHeader.test_the_readout_is_28px_and_shows_every_part` | `smoke::review_page_follows_the_selection`: the readout's value and label (`96%`, `Similarity`) and that it shows; the 28 px height is geometry |
| 124 | `ReadoutInTheHeader.test_a_ranking_row_is_marked_in_the_window` | `preview_text::tests::the_readout_splits_the_banner_into_parts`: a ranking never says similarity; the window's marker is `smoke::review_page_follows_the_selection` only for a measured one |
| 129 | `ReadoutInTheHeader.test_the_banner_row_is_gone` | does not apply: there is no banner row; the readout is the only element (`readout_root`) |
| 133 | `ReadoutInTheHeader.test_the_readout_fits_the_compare_column` | `smoke::review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide`: `fit_readout` moves the readout under the header when it does not fit |
| 153 | `ReadoutInTheHeader.test_zoom_floats_inside_the_frame_not_in_the_header` | **6.f-2** ([DAN-2340](/DAN/issues/DAN-2340)): Wipe, Differences and the compare dialog are not in this PR (the zoom row is Wipe's) |

#### `tests/test_review_tag_rail.py` (7)

| Line | Test | Disposition |
|---|---|---|
| 80 | `ReviewTagRail.test_match_label_sits_in_a_hairline_box_with_the_steppers` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 92 | `ReviewTagRail.test_steppers_are_36px_boxed_triangles_with_the_bracket_keys` | geometry, judged in the shot set and the measured tables (section 6), not asserted by a test |
| 103 | `ReviewTagRail.test_steppers_walk_the_candidates_and_disable_at_the_ends` | `smoke::review_page_follows_the_selection` (`smoke.rs:4083`): Previous match is insensitive on the first match; `smoke::review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop` (`smoke.rs:4270`, middle: both live; `smoke.rs:4292-4293`, last: Next and its bracket cap insensitive, Previous live) |
| 116 | `ReviewTagRail.test_no_candidates_means_no_steppers_and_the_picker_stays_below` | `smoke::review_page_follows_the_selection`: a row with no match reads `No match` and its picker is insensitive |
| 127 | `ReviewTagRail.test_rows_are_32px_and_at_least_five_full_rows_show_at_1440x900` | not measured at 1440 x 900: the shot set is 1100 x 800 and the minimum (section 6) |
| 141 | `ReviewTagRail.test_the_marker_is_dimmed_the_tag_is_bright_and_each_row_is_ruled` | the contrast table (section 6): the marker and the tag text |
| 165 | `ReviewTagRail.test_a_tag_without_a_marker_draws_as_one_bright_run` | `smoke::review_tag_panel_adds_removes_and_edits_in_place`: a tag with no source prefix reads as one run; the colour is the contrast table |

#### `tests/test_search_quality.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 4532 | `APartialSearchIsNotAnError.test_the_preview_says_why` | `preview_text::tests::the_placeholder_says_why_the_row_has_no_match` |

#### `tests/test_similarity_marking.py` (3)

| Line | Test | Disposition |
|---|---|---|
| 202 | `ThePreviewPanelSaysIt.test_a_measured_caption_is_unchanged` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement` |
| 207 | `ThePreviewPanelSaysIt.test_an_ordinal_caption_says_so_in_words` | `preview_text::tests::the_caption_says_a_ranking_is_not_a_measurement` |
| 212 | `ThePreviewPanelSaysIt.test_the_banner_marks_a_ranking` | `preview_text::tests::the_readout_splits_the_banner_into_parts` |

#### `tests/test_thin_modules.py` (10)

| Line | Test | Disposition |
|---|---|---|
| 410 | `AddTagsDialog.test_the_target_is_named_in_the_prompt` | `add_tags::tests::the_target_is_named_in_the_prompt` (new in DAN-2347): `add_tags::prompt("7 selected images")` is the whole label; `smoke::review_tag_panel_adds_removes_and_edits_in_place` asserts the label `Prompter::add_tags` is handed for `a.png`. Reverting the `format!` line turns the new test red. The label widget itself is the `AlertDialog` body (geometry, shot set) |
| 417 | `AddTagsDialog.test_one_tag_per_line` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`; `add_tags::tests::python_line_boundaries_end_a_tag` |
| 421 | `AddTagsDialog.test_blank_and_whitespace_only_lines_are_dropped` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: the empty line is dropped. not asserted: tab and space-only lines (`strip` runs before the empty filter) and the trailing newline as such (`smoke::review_tag_panel_adds_removes_and_edits_in_place` ends its text with `\n` and adds no empty tag) |
| 427 | `AddTagsDialog.test_nothing_typed_yields_no_tags` | not asserted: `parse_added_tags("")` is not fed. `splitlines("")` gives no lines, and `add_tags_to_current` returns before touching a queue entry when the list is empty (`review_ops.rs:860`); `smoke::review_tag_panel_adds_removes_and_edits_in_place` asserts the Cancel path adds nothing |
| 430 | `AddTagsDialog.test_a_namespace_prefix_is_split_off` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: ` character : reimu ` splits; `smoke::review_tag_panel_adds_removes_and_edits_in_place`: `series : touhou` |
| 434 | `AddTagsDialog.test_only_the_first_colon_splits` | `add_tags::tests::only_the_first_colon_splits`: `series:a:b` is `(Some("series"), "a:b")`; swapping `split_once` for `rsplit_once` turns it red |
| 442 | `AddTagsDialog.test_both_halves_are_stripped` | DAN-81. `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`; `smoke::review_tag_panel_adds_removes_and_edits_in_place`: ` series : touhou ` is `series:touhou`. Reverting `strip(head)` or `strip(name)` turns `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped` red |
| 451 | `AddTagsDialog.test_an_empty_namespace_half_is_no_namespace` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: `:bare` is `(None, "bare")`; `add_tags::tests::a_colon_alone_is_an_empty_name`: `:` is `(None, "")`. Keeping `Some(head)` for an empty head turns both red. not asserted: `: reimu` with a space after the colon (the name's `strip` is `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`'s) |
| 460 | `AddTagsDialog.test_an_unnamespaced_line_has_no_namespace` | `add_tags::tests::blank_lines_are_skipped_and_both_halves_are_stripped`: `cat` is `(None, "cat")` |
| 464 | `AddTagsDialog.test_everything_typed_here_is_sourced_as_USER` | `add_tags::tests::every_tag_is_a_user_tag`; `smoke::review_tag_panel_adds_removes_and_edits_in_place`: the added rows read `[User] …` |

#### `tests/test_type_voice.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 149 | `_UppercaseChecks.test_review_page_buttons` | 6.f: the Review buttons' case; judged in the shot set (the labels are sentence-case in code, caps are CSS) |

## 9. Status claims

This ticket changes no status claim. The README and the window's About text say work in progress; no text in the new
surfaces says usable, parity, feature-complete, released, replacement or stable (the sweep is in the PR).
