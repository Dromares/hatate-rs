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
  page's three buttons and their shortcuts. (`run_review_action` had no arm for the `H` key, Show in Hydrus, until the DAN-2396
  fixup: it logged "No handler" and did nothing. `smoke::hydrus_menu_entries_and_the_h_key_reach_their_handlers` pins it.)
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
| **D-A** | Delete from Hydrus asks with **Delete / Cancel**, Cancel the default and Escape. Python's box is Yes / No with No the default. | The ticket text ("Delete is destructive, so Cancel is the default") and design gate 2.6. | `smoke::hydrus_delete_defaults_to_cancel_and_only_deletes_after_the_answer` (`smoke.rs:5752`, `d.default == Response::Cancel`; `smoke.rs:5751`, `Buttons::DeleteCancel`) |
| **D-B** | A failure box's headline is `{prefix}: {user_message()}` and the raw text is in a detail line; Python writes `{prefix}: {str(exc)}`. The prefix is equal in every case. `Entry.error_message` still holds `str(exc)`, byte-compatible. | DAN-2116 item 1: the window shows `user_message()`, `detail()` goes to a detail line. | `smoke::hydrus_show_opens_a_page_and_explains_a_refusal` (`smoke.rs:5860`, the 500 case: the headline is the prefix and `user_message()`; `smoke.rs:5866`, no server words in it) |
| **D-C** | A second Hydrus request while one is running gets a box ("already under way") instead of queuing behind it on the GUI thread. | Python's send loops ran on the GUI thread; here nothing blocks the loop. | `smoke::hydrus_requests_leave_the_main_loop_running_while_the_server_stalls` (`smoke.rs:5934`, the box text; the window keeps turning at `smoke.rs:5917`-`5912`) |
| **D-D** | The Query Hydrus window's result rows wrap, and its stated minimum is 420 x 480 (was 420 x 400); the Pawchive Index window states a minimum, 720 x 680 (Python states none). | Gate 2.5: every state fits its stated minimum. | `smoke::pawchive_and_query_windows_keep_their_stated_minimum` (`smoke.rs:6115`, Query; `smoke.rs:6137`, Pawchive) |

Each pinning assertion was **reverted and run red** at the line named, then restored (`worktrees/DAN-2220-revert.log`):

| Row | Line changed | Result |
|---|---|---|
| D-A | `dialogs.rs:173`: `DeleteCancel` defaults to `Response::Delete` | `assertion left == right failed: destructive: Cancel is the default` at `smoke.rs:5752` |
| D-B | `hydrus_ops.rs:158`: `exc.user_message()` becomes `exc.detail()` | `Hydrus could not open the page: Hydrus returned HTTP 500: scripted-boom-text` at `smoke.rs:5860` |
| D-C | `hydrus_ops.rs:187`: the box text changed | `assertion failed: f.script.last_shown().text.starts_with("A Hydrus request is already under way.")` at `smoke.rs:5934` |
| D-D | `hydrus_query::MIN_HEIGHT` 480 to 400; `pawchive_dialog::MIN_HEIGHT` 680 to 560 | `the Query Hydrus window is 333x441; its stated minimum is 420x400`; `the Pawchive Index window is 671x627; its stated minimum is 720x560` |

## 3. Minimum size

The two new windows are their own surfaces and state their own minimum:

| Window | Stated minimum | Content need on the host (GTK 4.24) | Held by |
|---|---|---|---|
| Query Hydrus (`hydrus_query::MIN_WIDTH/HEIGHT`) | 420 x 480 | 333 x 441 | `smoke::pawchive_and_query_windows_keep_their_stated_minimum` |
| Pawchive Index (`pawchive_dialog::MIN_WIDTH/HEIGHT`) | 720 x 680 | 671 x 627 | the same scenario |

The fit test measures the window's **child** at the stated width, so the window's own size request cannot hide an
overflow. It was run red by lowering each stated height (400 for Query: `the Query Hydrus window is 333x441; its stated
minimum is 420x400`; 560 for Pawchive: `671x627 ... 720x560`). The numbers that count are the PNG headers of the hosted shot set (GTK 4.14):

**Hosted `ci` run 40982732901 at `1a44fc9` (all steps success, 00:55:48Z to 01:09:41Z), `shots` artifact (GTK 4.14 / libadwaita 1.5), size read from each PNG header of the 24 `hydrus-*`/`pawchive-*` files (the 12 pairs):**

| state | dark shot | light shot | width x height (dark / light) | stated | ok |
|---|---|---|---|---|---|
| Query Hydrus, empty | `hydrus-query-min-dark.png` | `hydrus-query-min-light.png` | 420 x 480 / 420 x 480 | 420 x 480 | yes |
| Query Hydrus, results (one wrapped row) | `hydrus-query-results-min-dark.png` | `hydrus-query-results-min-light.png` | 420 x 480 / 420 x 480 | 420 x 480 | yes |
| Query Hydrus, search button focused | `hydrus-query-focus-min-dark.png` | `hydrus-query-focus-min-light.png` | 420 x 480 / 420 x 480 | 420 x 480 | yes |
| Pawchive Index, empty index | `pawchive-min-dark.png` | `pawchive-min-light.png` | 720 x 680 / 720 x 680 | 720 x 680 | yes |
| Main window, empty page with Query Hydrus live | `hydrus-empty-min-dark.png` | `hydrus-empty-min-light.png` | 1000 x 780 / 1000 x 780 | 1000 x 780 | yes |
| Delete from Hydrus box | `hydrus-delete-min-dark.png` | `hydrus-delete-min-light.png` | 1000 x 780 / 1000 x 780 | 1000 x 780 | yes |
| Pawchive Index, three artists, one indexing (`576db59`) | `pawchive-artists-min-dark.png` | `pawchive-artists-min-light.png` | 720 x 680 / 720 x 680 | 720 x 680 | yes |
| Delete from Hydrus confirm (`576db59`) | `hydrus-delete-confirm-min-dark.png` | `hydrus-delete-confirm-min-light.png` | 1000 x 780 / 1000 x 780 | 1000 x 780 | yes |

**0 of 8 rows differ from the stated minimum** (the last two rows were added at `576db59`, read from the run 40992212821 artifact). The Query window's default size is 520 x 520 and the Pawchive window's
760 x 680 (`hydrus-query-*.png`, `pawchive-*.png`). The Hydrus *search* error box and the Pawchive Remove question are
`AdwAlertDialog`s over a window that is already at its minimum; they are not shot separately. These numbers were re-read
from the artifact of the code head `1a44fc9`; commits after it change only this file, so the artifact stays that head's. The fit test was red on the
host before the stated heights were raised (section 2, D-D).


## 4. The differential

The same scripted action sequence runs through Python's `MainWindow` at `76c85c4` (temp `HOME`,
`QT_QPA_PLATFORM=offscreen`) and the Rust window (Xvfb, driven through its `gio::Action`s and model calls from the
test-only driver `tests/differential.rs` + `tests/differential/hydrus_diff.rs`). Each side talks to its own scripted
Hydrus built from the same table. After every step the harness compares the rows (each column's text, order, selection,
status tier), the status text, every dialog (text, buttons, default button, result, detail line), the `session.db` rows,
`config.json` and the server's request log (method, path, decoded query, key header, body). The harness lives outside the
repo (`worktrees/DAN-2220-tools`: `gen_h.py` the generator, `py_driver.py`, `cmp.py`); the log is
`worktrees/DAN-2220-differential-c2.log`.

| Seed | Cases | Steps | Differing steps |
|---|---|---|---|
| 1 | 40 | 563 | 0 |
| 2 | 40 | 578 | 0 |
| 3 | 40 | 531 | 0 |
| 4 | 40 | 514 | 0 |

Documented classes inside those zero-difference runs, counted per seed by the harness (dialogs):

| Seed | D-A (Delete box) | D-B (failure headline) | Poll-round order (comparer rule) |
|---|---|---|---|
| 1 | 4 | 0 | 0 |
| 2 | 8 | 5 | 3 steps |
| 3 | 9 | 1 | 0 |
| 4 | 14 | 8 | 0 |

The comparer rule: the `get_url_files` requests of one confirmation poll round are compared as a multiset, because
Python's own `HydrusImportPollWorker` checks the pending entries concurrently in a thread pool. The table is from the
re-run at the head with the fixed generator. Earlier runs of the same seeds (same case, step and zero-difference totals)
used a generator that iterated a `set` of Hydrus hashes, so one seed gave a different scenario file on every run and the
per-seed class counts quoted before this commit could not be reproduced; the generator now iterates `sorted(...)`, and
three generations of one seed give the same file.

Negative controls:
altering one URL in the Rust request log gives 13 differences, dropping one request gives 35; perturbing the failure
prefix or leaking the server's words into the tail each gives 1.

**Not covered by the differential (smoke only):** the Query Hydrus window, the Pawchive window, Download and Send (it
reaches the network path), the https case and the stalled-server case.

## 5. Shots and contrast

Hosted `ci` run 40982732901 at `1a44fc9` (`ci` job: fmt, clippy, `cargo test --workspace` and the shot set, success,
00:55:48Z to 01:09:41Z; `test` 00:57:41Z to 01:05:03Z, shot set 01:05:03Z to 01:09:37Z), artifact `shots`, 312 files. The numbers
below are read from that artifact. Commits after `1a44fc9` change only this file.

**Shots** (dark and light for each): `hydrus-empty`, `hydrus-empty-min`, `hydrus-delete`, `hydrus-delete-min`,
`hydrus-query`, `hydrus-query-min`, `hydrus-query-results`, `hydrus-query-results-min`, `hydrus-query-focus`,
`hydrus-query-focus-min`, `pawchive`, `pawchive-min`. Minimum-size headers are in section 3.

**Contrast** (extreme-pixel method, `worktrees/DAN-2220-tools/contrast.py`, a copy of `DAN-2218-tools/contrast.py`, over
the 24 shots above; the full 244-row table is `worktrees/DAN-2220-contrast-c2.md`, not committed): 232 rows pass the 4.5:1
text floor, 8 are disabled controls (exempt, listed as such), **4 rows FAIL** at `1a44fc9`. The four were one element in four shots: the Pawchive artist field's **placeholder**
("Artist name, or a pawchive link"), `#7f7a74` on `#201f1e` **3.87** dark and `#726c63` on `#d4cdbf` **3.29** light, because
the entry was a stock libadwaita entry with its own fill. **Resolved at `576db59`** (design read DAN-2395, F1): the entry
takes the `field` class, so it is transparent over the page and the shared rule `entry.field text > placeholder
{ color: @ink_65 }` applies unchanged (no token touched). Measured on the hosted `576db59` shots, extreme-pixel inside the
entry's box inset 3 px (so the focus ring is not read as text): `#8a847e` on `#0d0d0d` **5.26** dark, `#5c574f` on `#e8e1d2`
**5.50** light, in `pawchive-dark/light`, `pawchive-min-*`, `pawchive-artists-*` and `pawchive-artists-min-*`. Lowest passing row per state (dark / light): Query Hydrus window (empty, focus and min shots) `Query Hydrus`
heading 11.29 / 12.88; with results 11.29 / 12.63 (`creator:synthetic artist`); empty page `ETA` column 5.05 / 5.20;
Pawchive `Artist` header 5.26 / 5.50; Delete box `Delete from Hydrus` 10.70 / 14.35.

**Selection colour (design acceptance 5).** At `32b026d` the Query results list and the Pawchive artist list painted the
toolkit's selection (`#cce0f8` light, `#23374f` dark) and not the stamp colours the Queue and the tag list use.
`1a44fc9` gives both lists the `pick-list` class (`background: @stamp_bg; color: @stamp_fg` on a selected row), as
`.queue` and `.tag-list` already do. In the `1a44fc9` results shots no measured row is below 11.29 dark / 12.63 light (the
`creator:synthetic artist` row above); the table does not say which rows are selected, so the selection colour itself is
for the design read to judge on the shot.

**Focus ring (non-text 3:1 floor).** `hydrus-query-focus-min-<scheme>` is the empty window with Search Hydrus focused;
the pixel diff against `hydrus-query-min-<scheme>` (`worktrees/DAN-2220-tools/focusdiff.py`, log
`worktrees/DAN-2220-focus-c2.log`) is one cluster, 152 x 42 px at (24, 185), 760 px, the 2 px ring around the button:
dark `#cdc4ba` over the page `#0d0d0d` **11.29:1**, light `#211d17` over `#e8e1d2` **12.88:1**. A second 20 px cluster at
x = 22 (1 px wide) is the tag box's text caret, not a ring. Only the Search Hydrus ring is measured; the other focusable
controls (tag box, results rows, Cancel, Import selected, the Pawchive entry and buttons) take the same theme rule and are
not shot focused. The same comparison is not shot for the Pawchive window.

**Tab order** (revised at `576db59` for DAN-2395 F5 and the Tab ruling, and again for DAN-2399). The stops are read off
`is_focusable` and `is_sensitive` by `smoke::query_and_pawchive_windows_follow_the_design_read`, which pins: Import selected is
insensitive until a row is selected (so it is no stop), an empty results list, its scroller and an empty artist table are not
focusable, the status label is not focusable, and the Pawchive entry carries the `field` class. Those pins could not see a stall:
at `8872a2f` a scroller that is not focusable around a list that is not focusable swallowed Tab, so in an empty Query Hydrus
window Tab stayed on Search Hydrus (Cancel out of reach) and in an empty Pawchive index it stayed on Remove selected (the
selectable status label did the same; Close out of reach), and the table scroller sent Tab from Close to nothing. `set_focusable(false)`
is not enough, as `gtk-6f.md` found for the review page (DAN-2384); `set_can_focus(false)` on the empty scroller and on the status
label is. `smoke::tab_walks_the_query_hydrus_and_pawchive_windows_to_their_footers` now walks them with real Tab key events (XTEST,
the way `tab_walks_the_review_page_from_the_header_to_the_footer` does; skipped with a reason off Xvfb, a failure under `CI`) and
asserts every press moves focus. What it shows, with the stops it asserts: Query Hydrus, empty: tag box (first stop), Search Hydrus,
Cancel, then back to the tag box. With a search done and a row selected: tag box, Search Hydrus, the rows, Cancel, Import selected.
Pawchive Index, empty index: the name/link entry (first stop, pinned), Find / Add, Add artists from my matches, Close, then back
to the entry (Index selected, Index / refresh all and Remove selected are insensitive until a row exists, or is selected, since
DAN-2401, so they are not stops; the walk asserts the exact list and the three `is_sensitive` states). With an artist added
(and selected) the three are live and Tab reaches Remove selected. With an artist indexed the entry is still the first stop
(pinned on a reopened window; without `set_focus(&window, Some(&query))` the table row is), and the table row comes *after* Close in
the cycle, because the table sits above the entry in the widget tree: Tab from Close wraps to the row, then the entry. That order is
what the walk records, not an intended one; a design call on it is Oderisi's. Walk logs: `worktrees/DAN-2399-mutation.log`.

**Design-read re-read fixup (DAN-2398 N1 and P3, DAN-2401, on top of `174a41c`).**
- **N1, the artist table:** the header and every row use one `gtk::SizeGroup` per column and one alignment per column (`RIGHT_ALIGNED`:
  Images, Posts, Indexed right-aligned; Artist and Service left), through one `cell()` function, so a row cannot drift with its last
  cell. The header sits in a one-row list of its own with the same `pick-list` class and a 1 px side margin for the scroller's border, so
  the header cells share the rows' padding. `smoke::pawchive_artist_columns_share_one_edge_in_the_header_and_every_row` seeds three
  artists (a 4-digit image count, a blank Posts cell, `not yet`) and measures every cell's allocated left and right edge in window
  coordinates, header against each row, within half a pixel (the design read asks for 1 px); it also pins the header's `xalign` per column.
  Red on: no `SizeGroup`; numeric columns left-aligned; the header's 1 px margin removed (`worktrees/DAN-2401-mutation.log`). One line
  is not pinned: `set_hexpand(i == 0)` (all columns expanding gives the same edges, because the size group equalises them).
- **P3-1:** `dialogs::detail_box` (the file-name box of the Delete from Hydrus question) starts at its top-left (`yalign` 0, no blank
  first line) and is indented 8 px (`@space-2`); pinned by `smoke::message_detail_box_starts_at_its_top_left_and_is_indented`.
  **P3-2:** a disabled primary button in the two windows takes Stop's hollow dim look (`window.pick-window button.pill.primary:disabled`,
  pinned by `theme::tests::pick_windows_get_the_well_the_border_and_row_dividers`). **P3-3:** see Tab order.

**Design-read fixup (DAN-2395, `8a7eede`..`576db59`).**
- **F1:** the Pawchive entry takes `field` (placeholder above). **F2:** `window.pick-window` (a class on the Query and Pawchive
  windows) gives their tag box the sunken well and their lists and tag box an `@ink_58` border that goes to `@ink` on focus
  (`theme.rs`). Measured on the hosted shots, border rows against the page: dark `#7c7771` **4.38:1**, light `#6c685e`
  **4.27:1** (floor 3:1); no `#ffffff` or `#1e1e1e` pixel in `hydrus-query-results-*`, `hydrus-query-focus-*` or
  `pawchive-artists-*` (0 each, both schemes). **F6:** `.pick-list > row` has the tag list's `@ink_18` divider.
- **F3:** the artist list says `No artists indexed yet. Add one below.` when empty (the `hint` tier), and the new shots
  `pawchive-artists-{dark,light}` and `-min-` show three artists (one selected, one being indexed: Stop enabled, progress
  line `Indexing Third Synthetic (fanbox): 3 of 8`). **F4:** `hydrus-delete-confirm-{dark,light}` and `-min-` show the
  destructive confirm, Cancel default with its ring, Delete in the stamp style. Both are in the CI shot loop.
- **F5:** see Tab order. **F7 not done:** the Pawchive window is titled by the window manager (title text
  `Pawchive Index - N artist(s), ...`, tested); an in-window heading would cost about 40 px of the stated 680 and chrome is not
  judgeable from shots, so this is left to Oderisi's re-read.
- **Found while shooting, fixed:** the Pawchive progress bar was libadwaita's accent blue and the entry's focus ring was its
  accent-blue outline, the only chroma in the window (read by eye on the local shot; not measured). Both now take the palette (`window.pick-window progressbar`,
  `window.pick-window entry.field:focus-within`); entry ring measured `#cdc4ba` dark / `#211d17` light on the page. The main
  window's filter bar uses the same `entry.field` and was **not** changed or re-shot here; whether it shows the same accent
  outline is for the re-read.
- Hosted `ci` run 40992212821 at `576db59` (success; shots artifact 328 files). PNG headers: `pawchive-artists-min-*`
  720 x 680, `hydrus-delete-confirm-min-*` 1000 x 780, `pawchive-min-*` 720 x 680, `hydrus-query*-min-*` 420 x 480 (0 differ
  from stated). Full contrast table over the artifact, `worktrees/DAN-2220-contrast-d3-6g.md` (384 rows for the 6.g shots:
  348 pass, 36 disabled-control rows exempt, 0 FAIL); script `worktrees/DAN-2220-tools/contrast.py`.
- Revert-and-red on the new pins: `worktrees/DAN-2220-revert-design.log`, 9 changes, each red (import always sensitive, entry
  without `field`, status label focusable, empty table focusable, no `pick-window` class, border on `@ink_46`, no row divider,
  each of the two scrollers set focusable). A tenth, the same scroller mutation before its assertion looked at the
  ScrolledWindow and not its Viewport, survived; the assertion was fixed in `370c1fb` and the mutation then went red. The
  rules for the well, the progress bar and the entry ring are pinned by `theme::tests::pick_windows_get_the_well_the_border_and_row_dividers`
  and were not mutated.

**Not judgeable from shots:** window chrome, GPU renderer, the real compositor's inactive-selection backdrop, HiDPI,
hover wash (owner: Oderisi's design read and the board's live check).

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
| `_hairline` (:29) | the `field` CSS class on the table and the results (`pawchive_dialog.rs:225`, `:226`); no function |
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
| `HydrusTagLookupWorker` (`hydrus_lookup_worker.py:15`) | the worker bridge job in `lookup_existing_hydrus_tags` over `hydrus_tag_lookup::run_tag_lookup` |
| `HydrusReconcileWorker` (`hydrus_reconcile_worker.py:16`) | the job in `start_hydrus_reconcile` over `hydrus_reconcile::run_reconcile` |
| `HydrusImportPollWorker` (`hydrus_import_poll_worker.py:21`) | `start_import_poll` over `hydrus_import_poll::run_import_poll` |
| `PawchiveIndexWorker` (`pawchive_index_worker.py:19`), `PawchiveFindWorker` (:54) | the jobs in `start_indexing` and `find_or_add` over `pawchive_index::run_index_jobs` and `find_creators_for_ui` |
| `ThumbnailWorker._known_to_hydrus` (`thumbnail_worker.py:48`), `_thumbnail_source` (:45) | `thumbnails::known_to_hydrus` and the Hydrus branch of the decode job |

## 7. Python test map

Two scans, both in `worktrees/DAN-2220-tools/` and run outside the repo. Section 7 has **0 unmapped by each**.

1. **Symbols** (`scan6g.py`): an `ast` walk over every `tests/test_*.py` at `76c85c4` finds **92** tests that touch a
   symbol this ticket ports: every definition of `gui/hydrus_query_dialog.py` and `gui/pawchive_index_dialog.py`, the
   sixteen `gui/main_window.py` methods of the method map, and the five worker classes. It follows `setUp`,
   `setUpClass`, class helpers (`self.<m>()`) and module helpers transitively. Seven more hits were the `_hairline`
   helper's name in other modules' paint tests (`test_app_chrome`, `test_flat_surfaces`, `test_ghost_kanji`); they never
   call the Pawchive helper and are not rows. `test_hydrus_query_dialog.py` reaches the dialog through a mixin the walk
   does not follow, so its other 16 tests are added by hand: **108 rows**.
2. **Other docs** (`scan_docs.py REPO`, `--all` lists every name): every row of every `docs/*.md` other than this file
   whose disposition says `6.g` and names a `test_...` must find that name in this section. Earlier tickets handed 6.g
   tests by name that the symbol walk cannot reach, because they test a menu, a key, a thumbnail pass or the batch's
   staging through names the walk does not list. It found **37**: `gtk-6e.md` (3), `gtk-6f.md` (2), `gtk-shell.md` (17)
   and `queue-view.md` (15). They are rows below: **145 rows** in all.

The rows that say "goes red" were reverted and run (twelve changes, all red, `worktrees/DAN-2396-revert.log`).

`mk7b.py ORACLE REPO` builds the section from both scans and fails on a test with no disposition. 6.j re-runs
`scan_docs.py REPO` and expects `0`; a later ticket that hands 6.g a test in a docs row adds it to this section or the
count is not zero.

`smoke.rs` is `crates/hatate-gtk/tests/smoke.rs`; `hydrus_query::tests::` is a unit test of `hydrus_query.rs`. A row cites
the assertion (`file:line`) that makes its claim. **not asserted** names a Python assertion no Rust test makes, with where
the behaviour is covered instead. "The differential" is the action differential of section 4 (seeds 1-4, 160 cases, 2,186
steps, 0 differing); a row that leans on it names what in the scripted server or the generator exercises the case
(`worktrees/DAN-2220-tools/gen_h.py`). It does not cover Download and Send, a refused connection, a filtered queue or the
two windows.

#### `tests/test_gui_smoke.py` (80)

| Line | Test | Disposition |
|---|---|---|
| 328 | `TestMainWindowSignalHandlers.test_auto_import_handler` | `auto_import.rs:389`, `:390`, `:391` and `:393`: what `_on_auto_imported` decides for the three results Python feeds it: a success leaves the list when Remove after import is on, an unconfirmed one and a failure stay, and with the setting off nothing leaves. **not asserted:** the window handler `search_run.rs` `on_auto_imported` itself (6.e) is not called by a window test; Python's test only checks that it does not raise |
| 1001 | `TestTableContextMenu.test_a_removal_while_the_menu_is_open_does_not_shift_the_action` | n/a here: the test is 6.d's row-menu identity (`docs/queue-view.md`); `HydrusImportPollWorker` appears only as a patched fixture |
| 1508 | `TestWorkerCallbacksDuringClose.test_hydrus_import_poll_finished_is_a_no_op_while_closing` | **not asserted:** the guard is `hydrus_ops.rs:804` (`closing` returns before touching the rows, DAN-128); no scenario closes the window mid-poll |
| 1734 | `TestDeleteFromHydrus.test_confirmed_delete_removes_only_what_hydrus_confirmed` | `smoke.rs:5772`, `smoke.rs:5773`, `smoke.rs:5785`: the rows leave the list, the delete endpoint is reached, the status line says one file; the differential adds the stubborn-file and mixed-state cases (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides) |
| 1747 | `TestDeleteFromHydrus.test_saying_no_deletes_nothing` | `smoke.rs:5765`, `smoke.rs:5766`, `smoke.rs:5767`: Cancel says `Delete cancelled`, the delete flag stays false, all 3 rows stay. Python's No is Cancel here (D-A) |
| 1757 | `TestDeleteFromHydrus.test_a_file_hydrus_still_holds_afterwards_stays_in_the_list` | **not asserted** in a window test; the differential's `stubborn` server (15% of cases, a delete that Hydrus does not honour) is this case, and the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 1768 | `TestDeleteFromHydrus.test_nothing_held_means_nothing_is_asked` | `smoke.rs:5757`, `smoke.rs:5759` asks only about the files Hydrus holds ("1 of the selected file(s) aren't in Hydrus"). **not asserted:** the case where none is held (no box at all); in the differential's delete steps, whose rows carry no hash or a deleted one |
| 1779 | `TestDeleteFromHydrus.test_without_an_access_key_it_refuses_early` | `smoke.rs:5704`, `smoke.rs:5705`, `smoke.rs:5706`: the Delete box names the missing key, and the server saw no request |
| 2888 | `TestSendFilesToHydrus.test_nothing_selected_asks_for_a_selection` | `smoke.rs:5679` for Send (`Select at least one row.`); the other actions' empty selection is the same first line of `hydrus_batch_start` |
| 2895 | `TestSendFilesToHydrus.test_a_missing_access_key_stops_before_sending` | `smoke.rs:5689`, `smoke.rs:5706`: the key box for Send, and no request |
| 2906 | `TestSendFilesToHydrus.test_the_windows_settings_reach_the_send` | `smoke.rs:5629`, `smoke.rs:5636`: the user's `verify_ssl` and `api_url` reach the client (an https server with the test CA) |
| 2918 | `TestSendFilesToHydrus.test_each_selected_row_is_sent_once` | `smoke.rs:5612`: two selected rows, two `add_file` posts |
| 2926 | `TestSendFilesToHydrus.test_an_unselected_row_is_never_sent` | `smoke.rs:5606`: the unselected row is untouched |
| 2935 | `TestSendFilesToHydrus.test_a_failure_is_reported_and_does_not_stop_the_rest` | `smoke.rs:5659`, `smoke.rs:5662`, `smoke.rs:5664` for the failure box and the count; **not asserted:** that a failure in the middle does not stop the rest. the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides with scripted `add_file` sequences that mix 200, veto and 500 |
| 2947 | `TestSendFilesToHydrus.test_a_warning_counts_as_sent_but_is_surfaced` | **not asserted** in a window test; the differential's "previously deleted" (status 3) and veto replies, and the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 2956 | `TestSendFilesToHydrus.test_rows_stay_when_remove_after_import_is_off` | `smoke.rs:5606`: after a send the sent rows stay in the list (the unit setting is off); with it on, the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (`remove_after_import` is on in 25% of the cases) |
| 2963 | `TestSendFilesToHydrus.test_only_successful_rows_are_removed_when_that_is_on` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (`remove_after_import` on in 25% of the cases) |
| 2976 | `TestSendFilesToHydrus.test_the_table_matches_the_entries_after_removal` | **not asserted** by a window test; the differential compares every row's text and the row order after each step (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides) |
| 2999 | `TestSendUrlToHydrusImporter.test_a_file_hatate_downloaded_is_done_and_not_polled` | **not asserted** in a window test; the differential's Import URL steps over rows that carry a hash (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides) |
| 3016 | `TestSendUrlToHydrusImporter.test_nothing_selected_asks_for_a_selection` | `smoke.rs:5679` for Send (`Select at least one row.`); the other actions' empty selection is the same first line of `hydrus_batch_start` |
| 3022 | `TestSendUrlToHydrusImporter.test_a_missing_access_key_stops_before_queueing` | `smoke.rs:5698`, `smoke.rs:5706`: the box for Import URL, no request |
| 3030 | `TestSendUrlToHydrusImporter.test_each_selected_url_is_queued` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (the request log has one `add_url` per URL) |
| 3038 | `TestSendUrlToHydrusImporter.test_an_entry_with_no_url_is_skipped_not_failed` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 3047 | `TestSendUrlToHydrusImporter.test_a_failure_is_surfaced` | for Send: `smoke.rs:5659`, `smoke.rs:5664`. For Import URL and Download and Send **not asserted** in a window test; the differential's failing `add_url` replies (`reply(..., 0.3)`) (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides); Download and Send is not in the differential (its network path) |
| 3053 | `TestSendUrlToHydrusImporter.test_queued_is_not_treated_as_imported` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 3063 | `TestSendUrlToHydrusImporter.test_removal_waits_for_confirmation_rather_than_removing_at_once` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (the poll window is pinned to 0.4 s so both sides finish it inside a step) |
| 3077 | `TestSendUrlToHydrusImporter.test_confirmed_entries_are_removed_unconfirmed_ones_kept` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 3087 | `TestSendUrlToHydrusImporter.test_a_resolved_entry_records_its_hash` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (`hydrus_hash` is in the `session.db` rows compared) |
| 3094 | `TestSendUrlToHydrusImporter.test_an_unresolved_entry_is_marked_not_confirmed` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 3129 | `TestDownloadAndSendToHydrus.test_nothing_selected_asks_for_a_selection` | `smoke.rs:5679` for Send (`Select at least one row.`); the other actions' empty selection is the same first line of `hydrus_batch_start` |
| 3135 | `TestDownloadAndSendToHydrus.test_a_missing_access_key_stops_before_downloading` | `smoke.rs:5700`, `smoke.rs:5706`: the box for Download and Send, and no request |
| 3146 | `TestDownloadAndSendToHydrus.test_each_selected_row_is_downloaded_and_sent` | **not asserted:** Download and Send reaches the real download path, which the namespace blocks; the core half is `docs/hydrus-import.md` (`download_and_send`) |
| 3153 | `TestDownloadAndSendToHydrus.test_a_row_with_no_direct_url_is_skipped_not_failed` | **not asserted:** as above |
| 3161 | `TestDownloadAndSendToHydrus.test_a_failure_is_surfaced` | for Send: `smoke.rs:5659`, `smoke.rs:5664`. For Import URL and Download and Send **not asserted** in a window test; the differential's failing `add_url` replies (`reply(..., 0.3)`) (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides); Download and Send is not in the differential (its network path) |
| 3167 | `TestDownloadAndSendToHydrus.test_only_successful_rows_are_removed` | **not asserted:** as above |
| 3178 | `TestDownloadAndSendToHydrus.test_the_downloaded_files_are_cleaned_up` | **not asserted:** as above; the staging directory is a `tempfile::TempDir`, which removes itself on drop |
| 3200 | `TestDownloadAndSendToHydrus.test_cleanup_happens_even_when_every_download_fails` | **not asserted:** as above (the `TempDir` drop covers it by construction) |
| 3278 | `TestAutoImportStagingIsCleanedUp.test_the_staging_directory_is_removed_after_a_batch` | n/a here: core. `batch_auto_import.rs:928` (one directory while the run is on) and `:936` (listing empty after it), mapped in `docs/batch.md:114` |
| 3285 | `TestAutoImportStagingIsCleanedUp.test_it_is_removed_even_when_the_batch_is_stopped` | n/a here: core. `batch_auto_import.rs:997`, mapped in `docs/batch.md:114` |
| 3305 | `TestAutoImportStagingIsCleanedUp.test_it_is_removed_even_when_the_run_raises` | n/a here: core. `batch_auto_import.rs:1021` (a worker that dies midway), mapped in `docs/batch.md:114` |
| 3570 | `TestFilteredRowsAreNeverActedOnByAccident.test_sending_to_hydrus_only_sends_visible_rows` | **not asserted** in a window test: the selection passed to the action is the visible one (6.d's filter row, `docs/queue-view.md`); the differential has no filter |
| 4103 | `TestHydrusReconcileAction.test_nothing_waiting_never_contacts_hydrus` | `smoke.rs:5887` says nothing waits (after a confirm). **not asserted:** that the server saw no request on that second call |
| 4113 | `TestHydrusReconcileAction.test_a_queued_entry_is_confirmed_when_hydrus_has_it` | `smoke.rs:5884`, `smoke.rs:5885`: `confirmed 1 of 1`, the row's `hydrus_import_confirmed` |
| 4124 | `TestHydrusReconcileAction.test_an_unknown_file_stays_queued` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (reconcile steps over `trashed`/`deleted` hashes) |
| 4135 | `TestHydrusReconcileAction.test_hydrus_being_unreachable_is_not_a_crash` | `smoke.rs:5917`, `smoke.rs:5923`, `smoke.rs:5928`, `smoke.rs:5934`, `smoke.rs:5940` for a stalled server (the loop keeps turning, one request at a time); a refused connection is **not asserted** |
| 4147 | `TestHydrusReconcileAction.test_the_action_says_so_when_there_is_nothing_to_do` | `smoke.rs:5887`: `Nothing is waiting on Hydrus - every sent file is confirmed` |
| 4152 | `TestHydrusReconcileAction.test_the_quiet_pass_stays_silent_when_nothing_changed` | **not asserted** in a window test; the quiet pass runs after the session restore in every differential case (the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides) |
| 4165 | `TestHydrusReconcileAction.test_the_quiet_pass_speaks_up_when_it_changed_something` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 5249 | `TestReviewShortcuts.test_sending_the_current_image_away_keeps_your_place` | `smoke.rs:6778`: a real Send with Remove after import on, from the row you are on, leaves the row that followed selected. The removal rule is 6.d's (`smoke.rs:3326`). **not asserted:** the J and K presses Python makes after it (6.f's `next_row`/`prev_row`) |
| 5268 | `TestReviewShortcuts.test_sending_the_last_row_away_lands_on_the_new_last_row` | `smoke.rs:6783`: the same Send from the last row leaves the new last row selected |
| 5276 | `TestReviewShortcuts.test_a_hydrus_confirmation_removing_your_row_moves_you_on` | n/a here: Review's current-row hand-over after a removal is 6.f (`docs/gtk-6f.md`); the removal itself is `hydrus_ops::remove_entries` |
| 5293 | `TestReviewShortcuts.test_a_hydrus_confirmation_elsewhere_leaves_you_where_you_are` | n/a here: as above |
| 5926 | `TestHydrusMenu.test_the_hydrus_menu_exists` | `menu.rs:363`-`menu.rs:365`: the menu titles are `_Files`, `_Hydrus`, `Si_tes`, `_Settings`, `_Help` |
| 5929 | `TestHydrusMenu.test_it_offers_the_three_documented_actions` | `menu.rs:367`: the Hydrus menu's labels are the two Send entries and Re-check Queued Imports, with one separator (a separator is not an action) |
| 5938 | `TestHydrusMenu.test_the_files_menu_still_has_its_recheck_entry` | `menu.rs:337`: the Files menu's labels still hold `Re-check Queued Imports` |
| 5945 | `TestHydrusMenu.test_both_menus_share_one_recheck_action` | `menu.rs:439`, `menu.rs:440`: both entries carry the action `reconcile`; `menu.rs:446`: it is registered once. Changing the Hydrus entry's action name goes red |
| 5955 | `TestHydrusMenu.test_the_recheck_tooltip_comes_from_the_one_constant` | n/a here: menu items carry no tooltips (`GMenu` has none), difference D4 of `docs/gtk-shell.md` |
| 5962 | `TestHydrusMenu.test_hydrus_is_not_buried_behind_help` | `menu.rs:363`-`menu.rs:365`: `_Hydrus` is the second title and `_Help` the last, ahead of `_Settings` |
| 5972 | `TestHydrusMenu.test_send_upload_triggers_the_real_handler` | `smoke.rs:6711-6500`: activating `send-to-hydrus` with nothing selected shows `Send to Hydrus` / `Select at least one row.`. Replacing the action's arm in `window.rs` with `{}` goes red |
| 5980 | `TestHydrusMenu.test_send_url_triggers_the_real_handler` | `smoke.rs:6721-6510`: activating `import-url-to-hydrus` shows `Send URL to Hydrus's Importer` / `Select at least one row.` |
| 5989 | `TestHydrusMenu.test_recheck_triggers_the_real_handler` | `smoke.rs:6730-6515`: activating `reconcile` says `Nothing is waiting on Hydrus - every sent file is confirmed` |
| 6378 | `TestShowInHydrus.test_it_shows_the_selected_files` | `smoke.rs:5810`, `smoke.rs:5811`, `smoke.rs:5812`: `Showing 1 file(s) in Hydrus` and the `new_page` call |
| 6388 | `TestShowInHydrus.test_one_file_gets_its_own_name_on_the_page` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides (the `new_page` body is in the request log) |
| 6394 | `TestShowInHydrus.test_files_hydrus_does_not_have_are_left_out_and_counted` | **not asserted** in a window test; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides |
| 6404 | `TestShowInHydrus.test_nothing_in_hydrus_says_so_instead_of_opening_an_empty_page` | `smoke.rs:5801`, `smoke.rs:5803`: the explanation box, and no page opened |
| 6413 | `TestShowInHydrus.test_rows_with_no_hash_yet_are_explained` | `smoke.rs:5803`, `smoke.rs:5804`: `None of the selected files have a hash yet` |
| 6423 | `TestShowInHydrus.test_no_access_key_is_reported_before_any_request` | `smoke.rs:5702`, `smoke.rs:5706`: the Show box names the key; no request |
| 6432 | `TestShowInHydrus.test_a_403_names_the_permission_that_is_missing` | `smoke.rs:5832`, `smoke.rs:5836`, `smoke.rs:5838`: the Warning box says the key lacks the `manage pages` permission. Python's headline carries `str(exc)`; here the headline is `user_message()` (D-B) |
| 6445 | `TestShowInHydrus.test_a_404_says_the_client_is_too_old` | **not asserted** in a window test; the 404 text is `hydrus_ops::show_failure_text`; the action differential (seeds 1-4, 160 cases, 0 differing steps) compares it on both sides for the 403 and 500 replies |
| 6454 | `TestShowInHydrus.test_any_other_failure_is_reported_not_raised` | `smoke.rs:5860`, `smoke.rs:5866`: a 500 gives the headline `Hydrus could not open the page: Hydrus answered with an error (HTTP 500).` without the server's words (D-B), and nothing raises |
| 6462 | `TestShowInHydrus.test_an_unreachable_hydrus_is_reported` | **not asserted:** a refused connection; the stalled-server scenario is `smoke.rs:5917` |
| 6472 | `TestShowInHydrus.test_the_menu_offers_it_and_wires_it` | `table_menu.rs:664`: the entry reads `Show 3 selected images in Hydrus` for three rows (the count form of Python's `Show 2 selected images`); `table_menu.rs:644`: enabled when a selected row has a hash; `queue_menu.rs:80` and `queue_menu.rs:198`: the entry has a handler |
| 6489 | `TestShowInHydrus.test_h_shows_the_selection` | `smoke.rs:6748`: `guarded_review_action("show_in_hydrus")` on a selected row opens the page with that row's hash. This path had no handler for the `H` key before this fixup; removing the arm goes red |
| 6504 | `TestShowInHydrus.test_h_with_nothing_selected_says_so_rather_than_acting` | `smoke.rs:6736`: `Show in Hydrus: select an image first`, and the next line asserts the server was asked nothing |
| 6518 | `TestShowInHydrus.test_the_menu_greys_it_out_when_no_row_has_a_hash` | `table_menu.rs:687`: `Show 3 selected images in Hydrus` is among the disabled entries when no selected row has a hash |
| 6652 | `TestPawchiveIndexWindow.test_a_pasted_link_adds_its_artist` | `smoke.rs:6418`, `smoke.rs:6422`, `smoke.rs:6429`: the status line, the row `["42", "fanbox", "0", "", "not yet"]`, and the duplicate message |
| 6658 | `TestPawchiveIndexWindow.test_artists_with_pawchive_matches_can_be_added_in_one_go` | **not asserted:** `add_from_matches` (the `Add artists from my matches` button) has no scenario; the button is live and its enabled state is read in the shots |
| 6663 | `TestPawchiveIndexWindow.test_removing_asks_first_and_defaults_to_no` | `smoke.rs:6477`, `smoke.rs:6478`, `smoke.rs:6479`, `smoke.rs:6482`, `smoke.rs:6483`: `Remove artists`, default Cancel, the two rows stay until the answer, then `Removed 1 artist(s).` |
| 6677 | `TestPawchiveIndexWindow.test_name_search_results_are_ticked_then_added` | `smoke.rs:6433`, `smoke.rs:6435`, `smoke.rs:6442`, `smoke.rs:6445`, `smoke.rs:6449`: the search disables the button, the two found labels, the status, then the ticked one is added |
| 6686 | `TestPawchiveIndexWindow.test_the_menu_opens_one_window_and_quitting_closes_it` | `smoke.rs:6499`, `smoke.rs:6527`: one window (`Rc::ptr_eq`), and `pawchive_dialog` is `None` after the main window closes |

#### `tests/test_hydrus_import_poll_worker.py` (2)

| Line | Test | Disposition |
|---|---|---|
| 37 | `TestHydrusImportPollWorkerStop.test_stop_cancels_inflight_futures` | n/a here: the worker body is the merged `hatate_core::hydrus_import_poll::run_import_poll`, mapped in `docs/hydrus-import-url.md` |
| 73 | `TestHydrusImportPollWorkerStop.test_stop_before_submit_emits_finished` | n/a here: the worker body is the merged `hatate_core::hydrus_import_poll::run_import_poll`, mapped in `docs/hydrus-import-url.md` |

#### `tests/test_hydrus_provenance_note.py` (4)

| Line | Test | Disposition |
|---|---|---|
| 580 | `TestAutoImportWiring.test_nothing_is_confirmed_or_noted_when_every_setting_is_off` | n/a here: core; the window hands its settings to the run unchanged. `batch_auto_import.rs:570`, `:571`: with every setting off the import is not confirmed (`confirmed == None`), and `:628`: no note is written for an unconfirmed import (`docs/batch.md:122`) |
| 634 | `TestBatchPollWorkerWiring.test_the_note_is_written_and_no_warning_is_carried` | n/a here: core (`docs/hydrus-import.md`, `docs/hydrus-import-url.md`) |
| 643 | `TestBatchPollWorkerWiring.test_entry_resolved_carries_the_note_warning` | n/a here: core, as above |
| 652 | `TestBatchPollWorkerWiring.test_nothing_is_written_when_the_setting_is_off` | n/a here: core, as above |

#### `tests/test_hydrus_query_dialog.py` (24)

| Line | Test | Disposition |
|---|---|---|
| 105 | `TestConstruction.test_a_non_empty_existing_set_is_adopted_by_reference` | **not asserted:** the set is built from the queue at open (`hydrus_query.rs:494`); the visible effect (`[already in list]`) is `smoke.rs:5980` |
| 115 | `TestConstruction.test_an_empty_existing_set_is_not_adopted` | n/a here: Python's `set()` aliasing; the window builds a fresh set on every open |
| 128 | `TestConstruction.test_the_client_is_built_from_the_hydrus_settings_it_was_given` | `smoke.rs:6048` (no key: no window) and the search going to the scripted server's URL (`smoke.rs:5974`) |
| 135 | `TestConstruction.test_several_files_can_be_selected_at_once` | `smoke.rs:5992`: `select_results(&[0, 1])` selects two rows |
| 144 | `TestSearch.test_one_tag_per_line_is_anded_and_blank_lines_are_dropped` | `hydrus_query::tests::the_query_is_one_tag_per_non_blank_line` (`hydrus_query.rs:585`) |
| 148 | `TestSearch.test_no_tags_at_all_warns_and_asks_hydrus_nothing` | `smoke.rs:6055`, `smoke.rs:6060`: `Enter at least one tag.` |
| 156 | `TestSearch.test_each_matching_file_becomes_one_row_carrying_its_details` | `smoke.rs:5976`, `smoke.rs:5980` and `hydrus_query::tests::a_result_line_is_the_twelve_character_hash_then_the_facts` (`hydrus_query.rs:554`) |
| 170 | `TestSearch.test_a_file_already_in_the_working_list_is_marked_as_such` | `smoke.rs:5980` |
| 177 | `TestSearch.test_no_matches_says_so_rather_than_leaving_an_empty_list` | `smoke.rs:6059` |
| 184 | `TestSearch.test_a_second_search_replaces_the_first_results_rather_than_appending` | **not asserted:** `on_searched` clears the list first (`hydrus_query.rs:295`); no scenario searches twice |
| 192 | `TestSearch.test_a_hydrus_failure_is_reported_and_leaves_the_previous_results_alone` | **not asserted:** the search-failure box has no scenario; the stalled search is `smoke.rs:6514-6276` |
| 203 | `TestSearch.test_a_metadata_failure_is_reported_too` | **not asserted:** as above |
| 213 | `TestImport.test_nothing_selected_warns_and_downloads_nothing` | `smoke.rs:5986`, `smoke.rs:5988` |
| 220 | `TestImport.test_a_selected_file_is_downloaded_and_becomes_an_entry` | `smoke.rs:6003`, `smoke.rs:6004`, `smoke.rs:6005`, `smoke.rs:6006`: one `get_files/file`, the hash, the `<hash>.png` path, the file on disk |
| 238 | `TestImport.test_a_missing_mime_falls_back_to_jpg_rather_than_no_extension` | **not asserted:** the fallback is in `on_imported`; no scenario serves a file with no mime |
| 246 | `TestImport.test_hydrus_tags_arrive_on_the_entry_marked_as_hydrus_tags` | `smoke.rs:6007`, `smoke.rs:6008`, `smoke.rs:6009`, `smoke.rs:6010`: `solo` and `series:foo`, both `TagSource::Hydrus` |
| 262 | `TestImport.test_a_namespaced_tag_keeps_the_rest_of_its_colons` | `hydrus_query::tests::a_tag_splits_on_its_first_colon_only` (`hydrus_query.rs:573`) |
| 269 | `TestImport.test_the_namespace_remap_setting_is_applied_to_imported_tags` | **not asserted:** `apply_namespace_remap` is called in `on_imported`; the scenario's settings carry no remap |
| 277 | `TestImport.test_a_file_already_in_the_working_list_is_skipped_and_not_downloaded` | `smoke.rs:5993`, `smoke.rs:5998`: `1 duplicate(s) ... skipped` and one download for two selected |
| 286 | `TestImport.test_the_same_file_selected_twice_in_one_go_is_only_imported_once` | **not asserted:** two selected rows with one hash; the dedupe is by hash in `import_selected` |
| 298 | `TestImport.test_an_imported_hash_is_recorded_so_a_later_query_marks_it` | **not asserted:** a second query after an import |
| 304 | `TestImport.test_one_failed_download_costs_that_file_and_not_the_selection` | **not asserted:** a failing `get_files/file` |
| 323 | `TestImport.test_every_file_failing_still_closes_the_dialog_with_no_entries` | **not asserted:** as above |
| 331 | `TestImport.test_the_download_directory_is_a_throwaway_temp_dir` | **not asserted:** the directory is a `tempfile::TempDir` (`Cargo.toml` moves `tempfile` to a normal dependency) |

#### `tests/test_search_quality.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 4256 | `TestSendUrlFallsBackToHatatesDownload.test_auto_import_downloads_when_no_match_hydrus_can_take` | n/a here: core. `batch_auto_import.rs:760`, `:767`: the download succeeds and one `add_file` goes out; the window runs it through `batch/run.rs` |

#### `tests/test_thumbnail_worker.py` (15)

| Line | Test | Disposition |
|---|---|---|
| 106 | `TestThumbnailSource.test_source_defaults_to_local_when_there_are_no_settings` | n/a here: `Settings` always exists, so there is no "no settings" branch; the source defaults to `"hydrus"` (`config/model.rs:242`, Python's own settings default) and is read at the start of every pass (`thumbnails.rs:234`) |
| 110 | `TestThumbnailSource.test_source_is_read_from_settings` | `smoke.rs:6596`: with `thumbnail_source = "hydrus"` in the settings the held file is drawn from Hydrus (its own file is unreadable); `smoke.rs:3455` for `"off"` (no row decodes) |
| 128 | `TestThumbnailSource.test_hydrus_source_without_an_access_key_falls_back_to_local` | `smoke.rs:6679`, `smoke.rs:6680`: source `"hydrus"` with no access key draws the file from disk (`Loaded`) and the server heard nothing. Reverting `thumbnails.rs:234` (`is_configured()` dropped) goes red |
| 217 | `TestDecodeFromBytes.test_bytes_are_scaled_down_to_the_requested_size` | `thumb_decode.rs:263`: `decode_bytes(.., 300)` of a 3000 x 100 picture is 300 x 10; the Hydrus bytes go through it (`thumbnails.rs:255`) |
| 225 | `TestDecodeFromBytes.test_bytes_already_small_enough_are_not_rescaled` | `thumb_decode.rs:261`: `decode_bytes` with a limit above the picture keeps its 3000 x 100; `thumb_decode.rs:152` for the file path (50 x 30 left as it is) |
| 232 | `TestDecodeFromBytes.test_unrecognisable_bytes_yield_None` | `thumb_decode.rs:264`: HTML bytes decode to `None` |
| 240 | `TestKnownToHydrus.test_no_hashes_means_no_call_is_made_at_all` | `thumbnails.rs:380`: rows with no hash (`None`, and the empty string) make no request |
| 247 | `TestKnownToHydrus.test_hashes_are_asked_for_in_batches` | `thumbnails.rs:394`: 261 hashes are asked as batches of 256 and 5. The constant is asserted too (`thumbnails.rs:386`): `HASH_CHECK_BATCH = 257` goes red |
| 265 | `TestKnownToHydrus.test_files_with_no_hash_are_not_asked_about` | `thumbnails.rs:409`: only the row with a hash is in the request |
| 275 | `TestKnownToHydrus.test_a_hydrus_error_stops_asking_and_keeps_what_was_answered` | `thumbnails.rs:425`, `thumbnails.rs:426`: the first batch's 256 answers are kept, the second batch fails, and a third is never asked. Reverting the `break` goes red |
| 299 | `TestKnownToHydrus.test_a_stop_request_abandons_the_remaining_batches` | `thumbnails.rs:440`: a stop raised during the first batch leaves one call of two. Reverting the stop check goes red |
| 334 | `TestHydrusThumbnailPath.test_a_known_hash_is_served_from_hydrus_without_reading_the_file` | `smoke.rs:6596`: the held row's own file is unreadable (`garbage`), so its picture can only be Hydrus's; `smoke.rs:6600-6389` count one thumbnail fetch |
| 350 | `TestHydrusThumbnailPath.test_a_hash_hydrus_does_not_know_is_decoded_from_disk` | `smoke.rs:6597`, `smoke.rs:6598`: the two rows Hydrus does not hold are drawn from their files; `smoke.rs:6600-6389`: no thumbnail request for them (one fetch, for the held row) |
| 361 | `TestHydrusThumbnailPath.test_an_empty_hydrus_thumbnail_falls_back_to_the_file` | `smoke.rs:6689`, `smoke.rs:6695`: with the thumbnail endpoint answering an empty body the row is `Loaded` from its file, and Hydrus was asked first. Reverting the `or_else` fallback goes red |
| 372 | `TestHydrusThumbnailPath.test_an_undecodable_hydrus_thumbnail_falls_back_to_the_file` | `smoke.rs:6689`, `smoke.rs:6695`: the same with bytes that are not a picture (`mode 1`); `smoke.rs:6627` for the case where the file is unreadable too (`Failed`) |

#### `tests/test_url_importer_duplicate_relationships.py` (8)

| Line | Test | Disposition |
|---|---|---|
| 433 | `TestAutoImportWiring.test_nothing_is_confirmed_or_related_when_both_settings_are_off` | n/a here: core. `batch_auto_import.rs:570`, `:571` (`docs/batch.md:121`) |
| 491 | `TestBatchPollWorkerWiring.test_the_relationship_is_recorded_and_no_warning_is_carried` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 502 | `TestBatchPollWorkerWiring.test_entry_resolved_carries_the_warning` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 511 | `TestBatchPollWorkerWiring.test_the_relationship_is_paired_before_the_hash_is_emitted` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 528 | `TestBatchPollWorkerWiring.test_an_unconfirmed_entry_emits_three_arguments_too` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 536 | `TestBatchPollWorkerWiring.test_nothing_is_related_when_the_setting_is_off` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 543 | `TestBatchPollWorkerWiring.test_no_settings_at_all_reads_as_off` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |
| 551 | `TestBatchPollWorkerWiring.test_an_unexpected_failure_does_not_take_the_confirmation_down` | n/a here: core (`docs/hydrus-import-url.md`); the window's side is `PollEvents::entry_resolved` carrying the warning, compared in the differential's Import URL steps |

#### `tests/test_workers_background.py` (11)

| Line | Test | Disposition |
|---|---|---|
| 416 | `TestPawchiveIndexWorker.test_each_job_is_indexed_in_order_and_reported` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 424 | `TestPawchiveIndexWorker.test_the_index_is_opened_at_the_path_the_worker_was_given` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 431 | `TestPawchiveIndexWorker.test_progress_from_the_crawler_is_forwarded_to_the_gui` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 442 | `TestPawchiveIndexWorker.test_request_stop_is_visible_to_the_crawler_mid_job` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 461 | `TestPawchiveIndexWorker.test_a_stop_before_the_run_starts_indexes_nothing` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 469 | `TestPawchiveIndexWorker.test_a_crash_mid_batch_still_emits_finished_all` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 480 | `TestPawchiveIndexWorker.test_a_crash_after_one_success_keeps_the_result_already_reported` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 494 | `TestPawchiveIndexWorker.test_the_job_list_is_copied_so_the_caller_cannot_mutate_it_mid_run` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 517 | `TestPawchiveFindWorker.test_results_are_emitted_with_no_error` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 523 | `TestPawchiveFindWorker.test_a_failure_is_emitted_as_text_beside_an_empty_list` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |
| 533 | `TestPawchiveFindWorker.test_no_matches_is_an_empty_list_and_still_no_error` | n/a here: core, mapped in `docs/pawchive-index.md:173-174`; the window's side is `smoke.rs:6456-6217` (Stop live while indexing, the crawl requests reach the server) |

## 8. Status claims

This PR adds no status claim. The README's work-in-progress banner is untouched. The claim grep at the head is in the PR
body.
