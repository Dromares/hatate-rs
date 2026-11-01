# Running searches from the window (Phase 6.e)

Written while the rewrite was work in progress (0.0.x); the current status is in `README.md`. This is the doc of the sixth-phase ticket
that gives the window its search run (DAN-2218). It holds the two maps Phase 6 asks every ticket for (the
method map and the Python test map; 6.j checks the union), the differential, the on-purpose differences, the
measured minimum size and the shot set. Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`.

## 1. The cut

**Ported here** (all in `hatate-gtk` unless a core path is named):

- Start, stop and progress: `action_toggle_search`, `action_start_search`, `action_stop_search`, the worker
  launch over `batch/run.rs` (three variants: ordinary, re-search with an engine override, continue without
  SauceNAO), progress and the live ETA, the rate-limit status and countdown, live row updates, the auto-import
  removal, and every ending. `Ending::Failed` is the Failed banner (call 2b) with a path-free message, not a crash.
- The SauceNAO quota readout, the quota-pause banner's live half (the persisted half is 6.c's) and Continue with
  other engines.
- Engine alerts, with the listener open only while a run is, and `engine_alerts.unsubscribe`.
- The Lens connection in the status line, and `core/desktop_attention.py` as `KdeAttention` (a no-op off Plasma
  or without `qdbus6`; the KWin script is a string constant ported from Python's).
- Re-search (three entry points), the availability check with its "remove the dead matches" question, the
  upscale check with its results box.
- The Activity page, Parser Health with its broken-parser warning, and the Sites menu.

**Core changes** (additive; the defaults are what `hatate batch` already does): `batch::run::search_pending_with`
and `PendingOptions` (an engine override per row, bypass the cache, a one-shot "continue without SauceNAO"),
`Reporter::updated` and `::continuing_without_saucenao`, `Entry::drop_unavailable_candidates`,
`availability::check_entry_candidates`, and `desktop_attention::KdeAttention`.

**Left to later tickets:**

| Ticket | Owns |
|---|---|
| 6.d | the Queue table: selection, preview, the tag list, the context menu that calls the re-search entry points, Clear Search Cache. The row a live update lands on is the queue model's; selecting it and refreshing the preview is `DEFERRED(6.d)` (`search_run.rs`) |
| 6.d/6.f | "Review the N waiting" (the banner's button is built and disabled: `window.rs`, `DEFERRED(6.d/6.f)`), and the Activity page's "Show in Queue" selecting the current row |
| 6.g | the Hydrus actions; what auto-import does with a send is `batch/run.rs` (Phase 5) |
| Phase 7 | `core/parser_probe.py` and so Parser Health's live check (Cloud's ruling, DAN-2290, option 1): the dialog shipped with the check off. **Closed by 7.c (DAN-2501): the check is ported and on.** The shipped window gives the dialog `LiveProbe` (`Env::live_probe`), so the button is enabled and the "Live check" column is there. A window with no check (a test's) still shows the disabled button, its tooltip and the caption "The live check is not part of this build yet.". The seam (`ProbeRunner`) is unchanged, so the scripted-probe tests run over it |

`docs/phase5-coverage.md` carries the clause for the probe. Nothing in this ticket claims more than the README does.

## 2. The worker bridge

Every search, availability check, upscale check and Parser Health check runs off the main thread through 6.c's
bridge (`docs/gtk-shell.md` section 2). `smoke::the_window_keeps_working_while_an_engine_stalls` is this ticket's
stalled-server test: an engine that does not answer, the theme action handled meanwhile, the run still active, and
then it finishes. `smoke::parser_health_dialog_and_warning` asserts the same for the (scripted) Parser Health
check: the window's timers keep turning while it waits.

## 3. Minimum size

`MIN_WIDTH x MIN_HEIGHT` is unchanged (1010 x 790). The `minimum_size_fits_every_state_in_both_schemes` scenario
now also renders the Failed banner and the Activity page. What each state needs on the **host toolkit** (GTK 4.24;
log `worktrees/DAN-2218-smoke-ea900c0.log`, head `ea900c0`; re-measured after the chip change of DAN-2297 in `worktrees/DAN-2321-smoke.log` with the same numbers; the hosted run on GTK 4.14 passed at that head and still gave Activity 909 px: see the finding below):

| State | Dark | Light |
|---|---|---|
| Queue, empty | 779 x 557 | 779 x 557 |
| Queue, rows | 779 x 520 | 779 x 520 |
| Queue, Crashed banner | 793 x 729 | 793 x 729 |
| Queue, "Nothing survived." | 860 x 624 | 860 x 624 |
| Queue, quota banner | 905 x 729 | 905 x 729 |
| Queue, **Failed banner** (new) | 876 x 729 | 876 x 729 |
| **Activity** (new) | 779 x 663 | 779 x 663 |
| Review, 340 px rail in | 1000 x 548 | 1000 x 548 |

The worst case is still 1000 x 729 against the stated 1010 x 790 (the window reads 1000 x 780 on the outer rect).

**GTK 4.14 finding and fix (DAN-2324).** The hosted run on 4.14 printed `OUTER activity ... window=1000x780 content=1000x909`: the
Activity page was allocated 129 px more than the window, so its status bar and log-path line ran off the bottom. The cause
is the engine chips' `FlowBox`: asked for a height with no width, 4.14 lays it out at its minimum width, one chip per line,
and the page then asks for the sum. 4.24 does not, so the host never showed it and the fit test (`ch >= need`) could not see
it either. The chips now say `set_min_children_per_line(5)` (two rows at most), and the scenario asserts both
`content height <= window height` and that the height asked for with no width (`unconstrained_min_h`, printed on every
`OUTER` line) is at most the stated minimum. At `5e05e25` the hosted GTK 4.14 run prints `OUTER activity dark|light
window=1000x780 content=1000x780 needs=777x653 unconstrained_min_h=653` (job 122472998643, `worktrees/DAN-2218-ci-5e05e25.log`),
and `activity-min-*.png` from that run's artifact is 1000 x 780 with the log-path line and the status bar in view.
Parser Health is its own dialog and has its own minimum: 760 x 460, rendered at it in `parser-health-min-*` below.

## 4. Differences from Python, on purpose

| # | Difference | Why |
|---|---|---|
| D1 | The live check in Parser Health is off (see section 1). **Closed by 7.c (DAN-2501): on in the shipped window.** | Cloud, DAN-2290, option 1. |
| D2 | The Failed banner's glyph is a plain `×`; call 2b draws a boxed one with an `ink_46` border, and `ink_46` is a control-border tier, not a token the banner family uses. | For Oderisi's read (item 1 of the design acceptance: a deviation is written, not discovered). |
| D3 | An engine alert's remedy is in the dialog's **body**, after the question; Python puts it in the box's detailed text. The same goes for the dead-match list and the upscale details. | The dialog seam (`Prompter`) has no detail area. Ruled by Cloud on DAN-2291: it stays as is and is documented. It is class 2 of the differential (section 5). |
| D4 | The Lens connection state is said in the status line ("Google Lens: waiting for an answer…"); Python shows nothing of it. | The ticket's "render `LensConnection`". |
| D5 | After a run ends, the status line reads "Search finished" where Python's, with a row selected, ends at "Ready" (its candidate-detail fetch overwrites it). | A documented race class of the differential. Ruled by Cloud on DAN-2339 (option 1): the pair `Ready` / `Search finished` and nothing else differing in the step. Measured by 6.f on seeds 1 to 4: 26 / 35 / 30 / 31 steps in 9 / 13 / 11 / 10 scenarios, and in every one the digest's candidate-detail key reads an identical pane on both sides (the scripted candidates have no site to fetch details from). See `docs/gtk-6f.md` section 5. |
| D6 | "Review the N waiting" is disabled for every state; Python enables it when rows are waiting. | 6.d/6.f own the selection and Review (section 1). |
| D7 | A Yes/No question's default is No (6.c's D2): the availability question ("Remove them from this image's match list?") defaults to No. | Design gate 2.6. Asserted in `smoke::the_availability_check_offers_to_drop_the_dead_matches`. |

## 5. The action differential

The same scripted action sequence runs through Python's `MainWindow` at `76c85c4` (temp `HOME`,
`QT_QPA_PLATFORM=offscreen`, driven through its actions and slots) and through the Rust window (Xvfb, driven
through its `gio::Action`s and model calls by `crates/hatate-gtk/tests/differential.rs`, which skips without
`HATATE_DIFF_SCENARIOS`). IQDB and SauceNAO are scripted on both sides, and **both sides run under `unshare -rn`**
(an empty network namespace: nothing reaches a real host; every URL in a scenario is `example.invalid`). After every
step the harness compares `session.db` rows, `config.json`, the quota-pause file, the visible model (status
line, run strip, countdown, quota label, banners, button, rows) and every dialog's text, buttons and default.
The harness (`gen.py` the scenario generator, `py_driver.py`, `cmp.py`, `run_all.sh`) lives outside the tree, in
`worktrees/DAN-2218-tools`; the log is `worktrees/DAN-2218-differential.log`. Command per seed:
`./run_all.sh <seed> 40 --all`.

At the head the differential ran on (`ea900c0`; later commits touch tests and the shot example only):

| Seed | Cases | Steps | Differing steps | Differences | Class 1 | Class 2 | Unclassified | Errors |
|---|---|---|---|---|---|---|---|---|
| 1 | 40 | 186 | 28 | 30 | 26 steps in 9 scenarios | 2 dialogs in 2 | 0 | 0 |
| 2 | 40 | 189 | 40 | 45 | 35 steps in 13 | 5 dialogs in 5 | 0 | 0 |
| 3 | 40 | 193 | 38 | 46 | 30 steps in 11 | 8 dialogs in 8 | 0 | 0 |
| 4 | 40 | 179 | 35 | 39 | 31 steps in 10 | 4 dialogs in 4 | 0 | 0 |

160 cases, 747 steps, **0 unclassified differences, 0 errors**.

- **Class 1: "Ready" vs "Search finished" (D5).** Python selects the row it is searching and its
  candidate-detail fetch ends the status line at "Ready"; Rust says "Search finished". The count for a given seed
  moves a little from run to run (seed 1: 23 once, 26 otherwise; seed 2: 32 once, 35 otherwise): it follows the
  Python side's candidate-fetch timing, and every step that varies is inside this class. So the count is a range
  (26 to 35 steps per seed here), not a number to pin. Deferred to 6.f (DAN-2219): its acceptance reruns this
  differential with a selection and the class has to close.
- **Class 2: the alert remedy in the body (D3).** 2 to 8 dialogs per seed; the remedy text is the same, only its
  place differs.

Everything else is equal at every step: `session.db` rows, `config.json`, the quota-pause file, banners, the run
strip, the button, the rows and every other dialog.

**Unopenable inputs (DAN-2231, added in the DAN-2321 fixup).** The generator takes `HATATE_DIFF_UNOPENABLE=1`, which
makes some rows' files a directory (`@dir`) or absent (`@missing`) in half the run, quota and alert scenarios; both
drivers create them the same way. Seed 11, 40 cases, at the head of the fixup (`./run_all.sh 11 40 --all` with the
variable set, log `worktrees/DAN-2218-differential.log`): 193 steps, 34 differing steps, 39 differences, **0
unclassified, 0 errors**, 34 steps all in the two classes above (class 1: 29 steps in 9 scenarios; class 2: 5 dialogs
in 5). 13 scenarios carried an unopenable row (8 directories, 12 missing files). What the rows do is equal on both
sides: they are searched with an empty hash and end `not_found` like a scripted answer; there is no error text to diff,
because the engines are scripted and never read the file. **Mode 000 is not a variant here**: both sides run under
`unshare -rn`, where the process is root in its own user namespace and reads a mode-000 file. The reads that do fail
loudly (the hash pass over a bad path) are 3.i's, in `batch_run` and its differential (`docs/batch.md`).

## 8. Shots and contrast

Rendered by `crates/hatate-gtk/examples/shot.rs` (a test-only tool; the shipped binary has no way to do this), on
Xvfb with a temp profile and synthetic sessions, at 1100 x 800 (outer 1090 x 790) and at the stated minimum
(`min`, outer 1000 x 780). The set below is the **host toolkit's** (GTK 4.24), `worktrees/DAN-2218/shots`. The `ci`
job renders the same states on the CI toolkit (GTK 4.14 / libadwaita 1.5) and uploads them as the `shots` artifact
of the run at the head (`.github/workflows/ci.yml` gains `failed`, `activity`, `parser-health` and the four
minimum-size states in this PR). Host files:

- `failed-{dark,light}.png`, `failed-min-{dark,light}.png`: the Failed banner (synthetic message, rows loaded)
- `activity-{dark,light}.png`, `activity-min-{dark,light}.png`: the Activity page
- `parser-health-{dark,light}.png`, `parser-health-min-{dark,light}.png`: Parser Health, the live-check button
  disabled with its caption (DAN-2290)
- `quota-{dark,light}.png`, `quota-min-{dark,light}.png`: the quota-pause banner (6.c's, with 6.e's live half)
- `empty-*`, `rows-*`, `menu-*` (and `menu-*-popover.png`), `focus-*`: the shell around them

The contrast table is `shots/contrast-host.md`, computed by `worktrees/DAN-2218-tools/contrast.py` (the extreme-pixel
method, the label boxes from each shot's `.rects`), at the head of DAN-2321: 742 rows (694 pass, 48 disabled-exempt,
**0 fail**). It was 24 fails at `24a6d64`, all the six "off" engine chips on the Activity page in the four Activity shots
(2.71 to 3.46:1). That was the mockup's own spec (`activity.html:17`, `--ink-45`, which `tokens.css` calls "large text or
non-text only, never body"). Oderisi ruled on it (DAN-2297, 10:10Z): off chips read in `ink_65`, on chips in `ink` at
weight 700 (`theme.rs`, `.chip` and `.chip.on`); `ink_58` was tried and still failed at 4.38 / 4.30:1. The chips now
measure, the least first:

| Shot | Element | fg | bg | ratio | scheme |
|---|---|---|---|---|---|
| activity-dark | trace.moe · off | #7e7973 | #0d0d0d | 4.51 | dark |
| activity-dark | Pawchive · off | #88837d | #0d0d0d | 5.17 | dark |
| activity-dark | IQDB · primary (on) | #cdc4ba | #0d0d0d | 11.29 | dark |
| activity-light | trace.moe · off | #69635b | #e8e1d2 | 4.56 | light |
| activity-light | Pawchive · off | #5e5950 | #e8e1d2 | 5.34 | light |
| activity-light | IQDB · primary (on) | #211d17 | #e8e1d2 | 12.88 | light |

The other two Activity shots (`activity-min-*`) give the same figures to within the antialiasing (the full rows are in
the table). Pinned by `theme::tests::the_engine_chips_are_text_and_clear_four_and_a_half` (the CSS rules and the tokens'
contrast, both schemes). Rows below the fold of the Parser Health list were unmeasured here; since DAN-2472 the list scrolls with a
persistent bar and `parser-health-bottom-*` shows them (`phase6-contrast.md` section 5). The disabled "Clear Search Cache…" button, the
gear and the disabled live-check button are exempt as disabled controls (the table says so row by row).

**Tab order**, from `shots/tab-order.txt` (the shot tool's `tab`, `tab-failed`, `tab-activity` states): the queue
walks Review, Activity, Start Search, the empty-state buttons, Queue; with the Failed banner up, Start Search is
followed by Resume Queue, View Log, Dismiss. On the Activity page it walks Parser health…, Show in Queue, the follow
toggle, Copy, Open folder, the log, Queue. **The ten engine chips are not Tab stops** (Oderisi's ruling 2, DAN-2297:
they say how the engines are set, Settings changes it, and their tooltips stay for the pointer). Taking the chips out
needed `set_can_focus(false)` on the FlowBox; marking only its `FlowBoxChild` wrappers `focusable(false)` left the Tab walk
stuck on Start Search (found in this fixup), so `smoke::activity_page_states_the_engines_and_tails_the_log` now walks Tab
twelve times, asserts none lands in the chip row and that it reaches Parser health….

**Parser Health first focus** (Oderisi's item A, DAN-2297): the site names are selectable labels, and the first one took the
dialog's first focus and so opened already selected. Close now takes it (`window.set_focus`), the names stay selectable so
they can be copied, and `smoke::parser_health_dialog_and_warning` asserts that no label holds a selection on open, that
some label is still selectable, and that the first focus is Close. With the `set_focus` line removed that scenario goes red
("a site name opened already selected"). The dialog's own Tab walk is still not judged from a shot (owner: the executor,
DAN-2297).

**Not measured by the author** (named, not passed): non-text boundaries (3:1 for the chips' borders, the banner
frame and the focus ring on the new surfaces; the `ink-18` hairlines are 6.c's deliberate sub-3:1 set), the focus
ring on the Failed banner's buttons, hover wash, window chrome, GPU renderer, HiDPI. The verifier's table covers
the first two; the last four are "not judgeable from shots" and owned by the board's own session.

## 9. Status claims

This ticket changed no status claim. At its head the README and the window title said work in progress; no text in the
new surfaces says usable, parity, feature-complete, released, replacement or stable (the sweep is in the PR).


## 6. Method map

Every `def` this ticket ports, by `file:line` at `76c85c4`, and the ones it leaves. 6.j checks the union against
every `def` in `gui/`. `_on_auto_imported` is listed by the ticket although `docs/gtk-shell.md` gave it to 6.g; the
removal of an imported row from the run's list is here, the import is Phase 5's.

### `gui/main_window.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 257 | `_format_finish_time` | `hatate_core::eta::format_finish_time` (6.a), called from `search_run.rs` `finish_time` |  |
| 388 | `_engine_alert_forwarder` | `hatate-gtk/src/search_run.rs` `launch_search_worker` (the listener posts to the window's inbox; the window drains it while it is alive) |  |
| 756 | `_build_sites_menu` | `menu.rs` Sites menu table, built from `all_site_options()`; its check items are `install_site_actions` |  |
| 781 | `_on_site_toggled` | `crates/hatate-gtk/src/search_run.rs` `fn on_site_toggled` `pub fn on_site_toggled(&self, site: &str, checked: bool) {` `on_site_toggled` |  |
| 791 | `_set_all_sites` | `crates/hatate-gtk/src/search_run.rs` `fn set_all_sites` `pub fn set_all_sites(&self, checked: bool) {` `set_all_sites` |  |
| 2201 | `action_parser_health` | `crates/hatate-gtk/src/parser_health.rs` `fn action_parser_health` `pub fn action_parser_health(&self) {` `action_parser_health` |  |
| 2217 | `_warn_about_broken_parsers` | `crates/hatate-gtk/src/parser_health.rs` `fn warn_about_broken_parsers` `pub fn warn_about_broken_parsers(&self) {` `warn_about_broken_parsers` |  |
| 2264 | `action_toggle_search` | `crates/hatate-gtk/src/search_run.rs` `fn action_toggle_search` `pub fn action_toggle_search(&self) {` `action_toggle_search` |  |
| 2270 | `action_start_search` | `crates/hatate-gtk/src/search_run.rs` `fn action_start_search` `pub fn action_start_search(&self) {` `action_start_search` |  |
| 2282 | `action_stop_search` | `crates/hatate-gtk/src/search_run.rs` `fn action_stop_search` `pub fn action_stop_search(&self) {` `action_stop_search` |  |
| 2289 | `_on_worker_progress` | `crates/hatate-gtk/src/search_run.rs` `fn on_worker_progress` `fn on_worker_progress(&self, done: usize, total: usize) {` `on_worker_progress` |  |
| 2295 | `_refresh_run_progress_label` | `session_ops.rs` `refresh_run_progress_label` (6.c); its live half is `search_run.rs` `live_eta_text` |  |
| 2327 | `_idle_run_text` | `session_ops.rs` `idle_run_text` (6.c) |  |
| 2336 | `_on_worker_waiting` | `crates/hatate-gtk/src/search_run.rs` `fn on_worker_waiting` `fn on_worker_waiting(&self, seconds: f64) {` `on_worker_waiting` |  |
| 2339 | `_on_wait_countdown` | `crates/hatate-gtk/src/search_run.rs` `fn on_wait_countdown` `fn on_wait_countdown(&self, label: &str, remaining: f64, _total: f64) {` `on_wait_countdown` |  |
| 2347 | `_on_auto_imported` | `crates/hatate-gtk/src/search_run.rs` `fn on_auto_imported` `fn on_auto_imported(&self, entry: Entry, result: &ImportResult) {` `on_auto_imported` (the row leaves the list when the run imported it; the import itself is `batch/run.rs`, Phase 5). The ticket lists it; `gtk-shell.md` had left it to 6.g |  |
| 2373 | `_on_worker_image_updated` | `crates/hatate-gtk/src/search_run.rs` `fn on_worker_image_updated` `fn on_worker_image_updated(&self, index: Option<usize>, entry: Entry) {` `on_worker_image_updated` | the row model follows the worker's copy; selection and the preview refresh are `DEFERRED(6.d)` (`crates/hatate-gtk/src/search_run.rs` `fn on_worker_image_updated` `} else {`) |
| 2400 | `_refresh_saucenao_quota_label` | `crates/hatate-gtk/src/search_run.rs` `fn refresh_saucenao_quota_label` `pub fn refresh_saucenao_quota_label(&self) {` `refresh_saucenao_quota_label` |  |
| 2411 | `_render_saucenao_quota_label` | `crates/hatate-gtk/src/search_run.rs` `fn refresh_saucenao_quota_label` `pub fn refresh_saucenao_quota_label(&self) {` (the same function renders) |  |
| 2417 | `_on_paused_out_of_quota` | `crates/hatate-gtk/src/search_run.rs` `fn on_paused_out_of_quota` `fn on_paused_out_of_quota(&self, searched: i64, remaining: i64) {` `on_paused_out_of_quota` |  |
| 2441 | `_show_quota_banner` | `crates/hatate-gtk/src/banners.rs` `fn show_quota_banner` `pub fn show_quota_banner(&self, paused_at: f64, remaining: i64, reset_at: Option<f64>) {` `show_quota_banner` (6.c) |  |
| 2472 | `_hide_quota_banner` | `crates/hatate-gtk/src/banners.rs` `fn hide_quota_banner` `pub fn hide_quota_banner(&self) {` `hide_quota_banner` (6.c) |  |
| 2481 | `_review_the_waiting` | the banner's button, built and disabled (`crates/hatate-gtk/src/window.rs` `fn set_mode` `if self.s.shell.show_mode(key, &ghost) {`) | **left to 6.d/6.f**: needs the table's row selection and Review |
| 2491 | `_resume_without_saucenao` | `crates/hatate-gtk/src/search_run.rs` `fn resume_without_saucenao` `pub fn resume_without_saucenao(&self) {` `resume_without_saucenao` |  |
| 2507 | `_on_continuing_without_saucenao` | `crates/hatate-gtk/src/search_run.rs` `fn on_continuing_without_saucenao` `fn on_continuing_without_saucenao(&self, _searched: i64, remaining: i64) {` `on_continuing_without_saucenao` |  |
| 2521 | `_on_engine_alert` | `crates/hatate-gtk/src/search_run.rs` `fn on_engine_alert` `fn on_engine_alert(&self, alert: &EngineAlert) {` `on_engine_alert` | the remedy is in the body (D3) |
| 2548 | `_on_worker_finished` | `crates/hatate-gtk/src/search_run.rs` `fn on_worker_finished` `fn on_worker_finished(&self, ending: &Ending) {` `on_worker_finished`, `on_run_ended` |  |
| 3061 | `_research_rows` | `crates/hatate-gtk/src/search_run.rs` `fn research_rows` `pub fn research_rows(&self, ids: &[EntryId]) {` `research_rows` | 6.d's context menu calls it (`crates/hatate-gtk/src/queue_menu.rs` `fn run_menu_action` `Action::Research => self.research_rows(ids),`, wired here after #67 merged) |
| 3076 | `_research_rows_with_engine` | `crates/hatate-gtk/src/search_run.rs` `fn research_rows_with_engine` `pub fn research_rows_with_engine(&self, ids: &[EntryId], engine: &str) {` `research_rows_with_engine` | 6.d's context menu calls it (`crates/hatate-gtk/src/queue_menu.rs` `fn run_menu_action` `Action::ResearchWithEngine(engine) => self.research_rows_with_engine(ids, engine),`) |
| 3100 | `_research_rows_with_opposite_engine` | `crates/hatate-gtk/src/search_run.rs` `fn research_rows_with_opposite_engine` `pub fn research_rows_with_opposite_engine(&self, ids: &[EntryId]) {` `research_rows_with_opposite_engine` | 6.d's context menu calls it (`crates/hatate-gtk/src/queue_menu.rs` `fn run_menu_action` `Action::ResearchWithOppositeEngine => self.research_rows_with_opposite_engine(ids),`) |
| 3146 | `_check_match_availability` | `crates/hatate-gtk/src/checks.rs` `fn check_match_availability` `pub fn check_match_availability(&self, id: EntryId) {` `check_match_availability` |  |
| 3168 | `_on_availability_progress` | `crates/hatate-gtk/src/checks.rs` `fn on_availability_progress` `fn on_availability_progress(&self, done: usize, total: usize) {` `on_availability_progress` |  |
| 3172 | `_on_availability_finished` | `crates/hatate-gtk/src/checks.rs` `fn on_availability_finished` `pub fn on_availability_finished(&self, id: EntryId, checked_copy: Entry, result: EntryCheck) {` `on_availability_finished` |  |
| 3209 | `_remove_dead_candidates` | `crates/hatate-gtk/src/checks.rs` `fn remove_dead_candidates` `pub fn remove_dead_candidates(&self, id: EntryId) {` `remove_dead_candidates` |  |
| 3240 | `_check_rows_for_upscaling` | `crates/hatate-gtk/src/checks.rs` `fn check_rows_for_upscaling` `pub fn check_rows_for_upscaling(&self, ids: &[EntryId]) {` `check_rows_for_upscaling` |  |
| 3260 | `_on_upscale_entry_checked` | `crates/hatate-gtk/src/checks.rs` `fn on_upscale_entry_checked` `fn on_upscale_entry_checked(&self, id: EntryId, checked: Entry, result: &UpscaleCheckResult) {` `on_upscale_entry_checked` |  |
| 3281 | `_on_upscale_check_finished` | `crates/hatate-gtk/src/checks.rs` `fn on_upscale_check_finished` `fn on_upscale_check_finished(&self, ids: &[EntryId]) {` `on_upscale_check_finished` |  |
| 3304 | `_launch_search_worker` | `crates/hatate-gtk/src/search_run.rs` `fn launch_search_worker` `pub fn launch_search_worker(&self, ids: Vec<EntryId>, opts: LaunchOptions) -> bool {` over `hatate_core::batch::run::search_pending_with` |  |

### `gui/activity_view.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 53 | `_build_activity_page` | `crates/hatate-gtk/src/activity.rs` `fn build` `pub fn build(log_path: &std::path::Path) -> Rc<ActivityPage> {` `build` |  |
| 79 | `_build_engines_card` | `crates/hatate-gtk/src/activity.rs` `fn build` `pub fn build(log_path: &std::path::Path) -> Rc<ActivityPage> {` `build` (the chips) |  |
| 128 | `_engine_state` | `crates/hatate-gtk/src/activity.rs` `fn engine_state` `pub fn engine_state(s: &Settings, engine_id: &str) -> (bool, &'static str) {` `engine_state` |  |
| 158 | `refresh_engines` | `crates/hatate-gtk/src/activity.rs` `fn refresh_engines` `pub fn refresh_engines(&self) {` `refresh_engines` |  |
| 199 | `_build_log_card` | `crates/hatate-gtk/src/activity.rs` `fn build` `pub fn build(log_path: &std::path::Path) -> Rc<ActivityPage> {` `build` (the log card) |  |
| 235 | `_tail_log` | `crates/hatate-gtk/src/activity.rs` `fn tail_log` `pub fn tail_log(&self) {` `tail_log` |  |
| 256 | `_on_follow_toggled` | `crates/hatate-gtk/src/activity.rs` `fn on_follow_toggled` `fn on_follow_toggled(&self) {` `on_follow_toggled` |  |
| 264 | `_copy_log` | `crates/hatate-gtk/src/activity.rs` `fn copy_log` `fn copy_log(&self) {` `copy_log` |  |
| 267 | `_open_log_folder` | `crates/hatate-gtk/src/activity.rs` `fn open_log_folder` `fn open_log_folder(&self) {` `open_log_folder` |  |
| 274 | `set_activity_live` | `crates/hatate-gtk/src/activity.rs` `fn set_activity_live` `pub fn set_activity_live(&self, live: bool) {` `set_activity_live` |  |

### `gui/parser_health_dialog.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 46 | `__init__ (the worker)` | `ProbeRunner` (the seam the live check will use) | **left to Phase 7 per DAN-2290**: no probe ships; 7.c ships `LiveProbe` behind the seam |
| 50 | `run (the worker)` | `ProbeRunner::run` | **left to Phase 7 per DAN-2290**; 7.c: `LiveProbe::run` is `parser_probe.run` |
| 66 | `__init__ (the dialog)` | `crates/hatate-gtk/src/parser_health.rs` `fn build` `fn build(main: &MainWindow, probe: Option<Arc<dyn ProbeRunner>>) -> Rc<ParserHealthDialog> {` `ParserHealthDialog::build` |  |
| 120 | `_populate` | `crates/hatate-gtk/src/parser_health.rs` `fn populate` `pub fn populate(&self) {` `populate` |  |
| 155 | `_start_probe` | `crates/hatate-gtk/src/parser_health.rs` `fn start_probe` `fn start_probe(self: &Rc<Self>) {` (the check button; disabled in this build, DAN-2290) |  |
| 168 | `_on_progress` | `crates/hatate-gtk/src/parser_health.rs` `fn on_progress` `fn on_progress(&self, done: usize, total: usize, site: &str) {` `on_progress` |  |
| 174 | `_on_done` | `crates/hatate-gtk/src/parser_health.rs` `fn on_done` `fn on_done(&self, results: Vec<ProbeResult>) {` `on_done` |  |
| 198 | `closeEvent` | `crates/hatate-gtk/src/parser_health.rs` `fn start_probe` `fn start_probe(self: &Rc<Self>) {` `start_probe`: the worker's callback holds a weak handle, so a result that arrives after the dialog is gone is dropped |  |

### `core/desktop_attention.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 37 | `available` | `hatate_core::desktop_attention::KdeAttention::available` |  |
| 41 | `quiet` | `KdeAttention::quiet` (the `WindowAttention` trait, `lens_browser.rs`) |  |
| 46 | `ask_for_input` | `KdeAttention::ask_for_input` |  |
| 51 | `script_for` | `desktop_attention::script_for` |  |
| 64 | `_qdbus` | `KdeAttention::qdbus` (`qdbus6`, then `qdbus-qt6`) |  |
| 68 | `_apply` | `KdeAttention::apply` |  |
| 92 | `_run` | `desktop_attention::run` (5 s timeout, non-zero exit is not an error) |  |

### `workers/availability_worker.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 34 | `__init__` | `hatate_core::availability::check_entry_candidates` (arguments) |  |
| 40 | `stop` | the `ctx.stop_flag()` passed to `check_entry_candidates` (`crates/hatate-gtk/src/checks.rs` `fn check_match_availability` `ctx.stop_flag(),`); `retire_checks` stops both checks at close |  |
| 43 | `_check_one` | `hatate_core::availability::check_one_candidate` |  |
| 71 | `run` | `check_entry_candidates`, on the worker bridge (`crates/hatate-gtk/src/checks.rs` `fn check_match_availability` `pub fn check_match_availability(&self, id: EntryId) {`) |  |

### `workers/upscale_check_worker.py`

| Line | Python | Rust | Left to |
|---|---|---|---|
| 26 | `__init__` | `hatate_core::upscale_detect::check_entries` (arguments) |  |
| 30 | `run` | `check_entries`, on the worker bridge (`crates/hatate-gtk/src/checks.rs` `fn check_rows_for_upscaling` `pub fn check_rows_for_upscaling(&self, ids: &[EntryId]) {`) |  |

## 7. Python test map

### Tests that touch 6.e's symbols

The set to map is the union of two, so a test that exercises 6.e behaviour without naming one of its symbols is not missed (DAN-2296 found 33 of those). (1) An `ast` walk over every `tests/test_*.py` at `76c85c4` (`scan.py`, run outside the repo) finds **124** tests whose source mentions a symbol this ticket ports (the method names of the ticket, the modules `activity_view`, `parser_health_dialog`, `desktop_attention`, `availability_worker`, `upscale_check_worker`, `parser_probe`, `SearchWorker`, `engine_alerts.unsubscribe`, the run strip's labels and the sites menu). (2) The **59** rows `docs/gtk-shell.md` section 7 gives to 6.e (any disposition naming it, including `is 6.e's`). The union is **150** tests (33 are in both). **115** have a row below (82 from the walk, 33 from the second set). The other **35** are rows `gtk-shell.md` section 7 maps only to another ticket (6.c, 6.d, 6.g, n/a) and are not repeated. **Unmapped: 0**, by `mapcheck.py` (outside the repo, `worktrees/DAN-2321/mapcheck.py`), which also greps every Rust test name cited below. A row whose disposition is `6.h` or `6.f` hands the test to that ticket, with the reason; 6.j's union check reads it there.

`6.e` rows name the Rust test that covers them (`smoke::` is a scenario of `crates/hatate-gtk/tests/smoke.rs`; `module::tests::` is a unit test; `batch_run::` is `crates/hatate-core/tests/batch_run.rs`). A row that says `not asserted` names a Python test that no Rust test in the tree checks. Where the action differential compares the same field after every step, the row says so: that is evidence from a run, and CI does not run it. There are no tests in `tests/` for `gui/activity_view.py` or `core/parser_probe.py`'s window half beyond those below; the Activity page's own tests are Rust's (`smoke::activity_page_states_the_engines_and_tails_the_log` and `activity::tests`), and have no Python counterpart.


#### `tests/test_app_chrome.py` (2)

| Line | Test | Disposition |
|---|---|---|
| 105 | `TestPageGutter.test_the_top_bar_and_run_strip_share_the_gutter` | 6.c: the gutter, judged in the shot set (geometry) |
| 141 | `TestPageRhythm.test_the_chrome_is_the_same_height_on_every_screen` | 6.c: the chrome's height, judged in the shot set (geometry) |

#### `tests/test_core_utils.py` (6)

| Line | Test | Disposition |
|---|---|---|
| 1453 | `TestCrashDiagnostics.test_retiring_a_worker_does_not_warn` | **core**: `SearchWorker`'s retirement warning has no Rust counterpart: the run is a task on the runtime, not a thread to retire; the window's teardown is `SearchRun::close` and the 6.c close scenarios |
| 1710 | `TestParserProbe.test_progress_is_reported_for_each_site` | **7.c (DAN-2501)**: `crates/hatate-core/src/parser_probe.rs` `fn progress_is_reported_for_each_site` `assert_eq!(sites, ["danbooru", "e621"]);`; the dialog's seam reports it, and the scripted-probe test is `smoke::parser_health_dialog_and_warning` |
| 1720 | `TestParserProbe.test_every_probe_url_matches_the_parser_it_claims` | **7.c (DAN-2501)**: `crates/hatate-core/src/parser_probe.rs` `fn every_probe_url_matches_the_parser_it_claims` `assert_eq!(name, site, "{url}");` |
| 1959 | `TestMyAnimeListSite.test_it_is_offered_in_the_sites_menu` | **6.e**: `smoke::the_sites_menu_toggles_and_saves` builds the menu from `ALL_SITE_OPTIONS` (`menu::all_site_options`) and asserts its order against the saved config; that these three sites are in it is the list's contents, from `hatate_core::sites` (Phase 3) |
| 2164 | `TestMangaUpdatesSite.test_it_is_offered_in_the_sites_menu` | **6.e**: `smoke::the_sites_menu_toggles_and_saves` builds the menu from `ALL_SITE_OPTIONS` (`menu::all_site_options`) and asserts its order against the saved config; that these three sites are in it is the list's contents, from `hatate_core::sites` (Phase 3) |
| 2278 | `TestPahealSite.test_it_is_offered_in_the_sites_menu` | **6.e**: `smoke::the_sites_menu_toggles_and_saves` builds the menu from `ALL_SITE_OPTIONS` (`menu::all_site_options`) and asserts its order against the saved config; that these three sites are in it is the list's contents, from `hatate_core::sites` (Phase 3) |

#### `tests/test_flat_surfaces.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 162 | `TestRegionsAreFlatHairlines.test_run_strip_is_a_hairline_on_the_flat_page` | 6.c: the strip's hairline, judged in the shot set |

#### `tests/test_gui_smoke.py` (48)

| Line | Test | Disposition |
|---|---|---|
| 272 | `TestMainWindowSignalHandlers.test_search_progress_handlers` | **6.e**: `smoke::start_search_runs_the_batch` asserts the gauge, the per-row results and the finish; the rate-limit status line and the `waiting Ns` countdown text are `eta::the_countdown_rounds_halves_to_even`, and the window's handler for them is not driven by a test: the action differential compares it after every step (evidence from a run, not a test in the tree) (`wait`, `status`) |
| 282 | `TestMainWindowSignalHandlers.test_progress_label_shows_the_count_then_the_estimate` | **6.e**: the strip's text is `eta::the_run_strip_follows_the_state_of_the_run`; `smoke::start_search_runs_the_batch` asserts the `0/3` and `3/3 searched` counts; the window's live `ETA ~…` reading is not asserted by a window test: the action differential compares it after every step (evidence from a run, not a test in the tree) (`run_eta`) |
| 303 | `TestMainWindowSignalHandlers.test_a_finished_run_keeps_the_count_and_drops_the_estimate` | **6.e**: `smoke::start_search_runs_the_batch` asserts the count stays and the ETA returns to the idle reading after the run; `eta::a_finished_run_has_nothing_left` |
| 350 | `TestMainWindowSignalHandlers.test_saucenao_quota_label_survives_odd_data` | **6.e**: does not apply as written: `SauceNaoQuota` is a typed struct, so a limit that arrives as a string cannot be built (the parse that tolerates it is the engine client's, Phase 4); the label's text is `saucenao::the_daily_allowance_reads_as_used_out_of_the_limit_else_what_is_left_else_nothing`; `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` asserts the window's label (`SauceNAO 2/200 used today`, no colon) and its burst-window tooltip after a live pause; the action differential also compares it after every step (evidence from a run, not a test in the tree) (`quota_label`) |
| 403 | `TestEngineAlertsReachTheUser.test_the_channel_is_open_only_while_a_search_is_running` | **6.e**: `smoke::engine_alerts_reach_the_user_only_during_a_run`: nobody hears an alert before the run, one box during it, nobody after it (the listener comes off at the end); `engine_alerts::an_unsubscribed_listener_stops_hearing` for the channel itself |
| 427 | `TestEngineAlertsReachTheUser.test_closing_mid_run_takes_the_listener_off_too` | **6.e**: not asserted: a run that is stopped part-way never reaches the finish handler in Python, so its listener has to come off at close; in Rust `SearchRun::close` calls the same `drop_listener`, and `smoke::close_saves_and_marks_a_clean_exit` closes a window, but no test closes mid-run and then raises an alert |
| 438 | `TestEngineAlertsReachTheUser.test_a_forwarder_whose_window_is_gone_is_harmless` | **6.e**: does not apply: Python's forwarder emits a Qt signal on a window whose C++ half can already be gone. The Rust listener posts to a `Send` inbox and the window drains it only while it is alive (`SearchRun::post`, `MainWindow::downgrade` / `upgrade`); there is no object to emit on |
| 449 | `TestEngineAlertsReachTheUser.test_an_alert_becomes_a_box_and_a_status_line` | **6.e**: `smoke::engine_alerts_reach_the_user_only_during_a_run` asserts the box's kind, title and buttons; the remedy sits in the body, not a detail area (documented class, section 4, D3). That the status line names the engine is not asserted |
| 467 | `TestEngineAlertsReachTheUser.test_enabling_lens_without_its_dependencies_warns_at_once` | **6.h**: this is the Settings dialog's own Lens toggle (`gui/settings_dialog.py`, 6.h: Preferences in `gtk-shell.md`), not the run. 6.e ports the run-time end of the alert channel (`smoke::engine_alerts_reach_the_user_only_during_a_run`), and the alert text is core's (`docs/google-lens.md`, the row naming this test and the next). The early warning at toggle time moves to 6.h's map |
| 488 | `TestEngineAlertsReachTheUser.test_a_working_install_says_nothing` | **6.h**: the same toggle, with a working install; see the row above |
| 1491 | `TestWorkerCallbacksDuringClose.test_availability_finished_is_a_no_op_while_closing` | **6.e**: `smoke::the_availability_check_offers_to_drop_the_dead_matches` covers the handler's outcomes, and ends by setting `closing` and calling the handler again: no box is raised |
| 1816 | `TestTagsAppearWhenAMatchLands.test_tags_show_without_reselecting_the_row` | **6.e**: not asserted: the tag panel is 6.d/6.f's and the Rust window shows no tags yet. 6.e's half is that the row carries the worker's copy as the search lands: `smoke::start_search_runs_the_batch` asserts the rows' final statuses, and `batch_run::updated_comes_after_each_search_and_before_its_progress` asserts the event that replaces the row comes after each search |
| 1832 | `TestTagsAppearWhenAMatchLands.test_a_re_search_clears_the_previous_tags` | **6.e**: not asserted: the tag panel belongs to 6.d/6.f. The row model follows the worker's copy (`on_worker_image_updated`), and that the previous result's tags are gone once a row is `Searching` again is the core `Entry` reset, which `batch_run::updated_comes_after_each_search_and_before_its_progress` observes through the events, not the tags |
| 1846 | `TestTagsAppearWhenAMatchLands.test_an_update_for_another_row_still_follows_the_search` | **6.f (DAN-2219)**: not asserted here. "An update selects that row and the panel describes it" is class 1 of the action differential (DAN-2291: Python selects the row and fetches its candidates, so its status line reads `Ready`); the selection and the panel are 6.d/6.f, which reruns that class to zero. 6.e's half is the same as the row above |
| 1983 | `TestParserHealthDialog.test_it_lists_every_site_even_before_anything_is_known` | **6.e**: `parser_health::tests::every_site_is_listed_before_anything_is_known`; `smoke::parser_health_dialog_and_warning` asserts the 13 rows and the `not used yet` cells |
| 1989 | `TestParserHealthDialog.test_a_broken_parser_shows_in_the_session_column` | **6.e**: `parser_health::tests::a_broken_parser_shows_in_the_session_column_with_its_problem`; `smoke::parser_health_dialog_and_warning` asserts the dialog's cells |
| 1996 | `TestParserHealthDialog.test_live_results_fill_the_check_column` | **6.e, per DAN-2290; the check is on since 7.c (DAN-2501)**: a window with no check still shows the disabled button with its tooltip and caption, and the shipped window has one. The check column's filling is `parser_health::tests::live_results_fill_the_check_column_and_a_failure_names_its_problem` and `smoke::parser_health_dialog_and_warning`, over a scripted probe (the seam the real check will use) |
| 2008 | `TestParserHealthDialog.test_an_all_clear_says_so` | **6.e, per DAN-2290**: `parser_health::tests::the_summary_line_says_all_clear_names_the_broken_and_never_clears_a_check_that_did_not_run`, over scripted results |
| 2015 | `TestParserHealthDialog.test_a_check_that_could_not_run_is_not_reported_as_all_clear` | **6.e, per DAN-2290**: `parser_health::tests::the_summary_line_says_all_clear_names_the_broken_and_never_clears_a_check_that_did_not_run`, over scripted results (the live check itself is off in this build) |
| 2020 | `TestParserHealthDialog.test_the_check_runs_off_the_gui_thread` | **6.e, per DAN-2290**: `smoke::parser_health_dialog_and_warning` runs the scripted probe off the main thread (a 150 ms pause) and asserts the window's timers keep turning, the button is off meanwhile and the progress and summary follow |
| 2093 | `TestRateLimitIsMeasuredEngineToEngine.test_engine_to_engine_gap_is_never_shortened` | **core**: `batch_run::booru_time_counts_toward_the_gap_and_the_engine_to_engine_interval_stays_whole` asserts the interval the engines see (answer to answer) is 60 s however the time was spent: 140 s elapsed for three images, not less. The worker loop is 3.i's |
| 2111 | `TestRateLimitIsMeasuredEngineToEngine.test_non_engine_work_does_not_add_to_the_batch` | **core**: the same test: the booru page's 20 s vanishes into the gap (two waits of 40 s slept, not 60 s) |
| 2588 | `TestPerHostPacing.test_with_no_overrides_every_host_keeps_the_global_gap` | **core**: with no `engine_delays` set, the global delay is the gap: `batch_run::the_gap_between_images_is_waited_out_with_a_countdown_and_not_after_the_last` and `batch_run::booru_time_counts_toward_the_gap_and_the_engine_to_engine_interval_stays_whole` |
| 2598 | `TestPerHostPacing.test_a_skipped_host_imposes_no_wait` | **core**: `batch_run::a_hosts_own_interval_replaces_the_global_gap_for_that_host_only`, first half: IQDB alone at 5 s against a global 60 s sleeps 10 s over three images, and SauceNAO is never called |
| 2613 | `TestPerHostPacing.test_a_longer_override_extends_the_wait` | **core**: the same test, second half: a 90 s override against a global 10 s sleeps 180 s over three images |
| 2621 | `TestPerHostPacing.test_a_stricter_host_paces_the_whole_wave` | **core**: `batch_run::a_stricter_host_paces_the_whole_wave`: IQDB waits for SauceNAO's 70 s |
| 2630 | `TestPerHostPacing.test_a_broken_engine_report_falls_back_instead_of_hammering` | **core**: does not apply as written (Python feeds per-engine clocks by a callback that can stop firing; the Rust batch has no such callback). The failure mode it guards, no report so no wait owed, is `batch_run::no_engine_report_falls_back_to_the_global_delay` |
| 4369 | `TestFailedSearchesAreRetriedOnce.test_a_failed_image_is_tried_again` | **core**: `batch_run::a_network_fault_is_retried_once_after_the_first_pass` asserts three searches for two images and that every row ends Good (the second attempt's result stands) |
| 4378 | `TestFailedSearchesAreRetriedOnce.test_a_successful_image_is_not_tried_again` | **core**: the same test (the image that succeeded is not among the retries: `searched` is 3, not 4) and `batch_run::several_failures_are_all_retried_and_the_image_that_succeeded_is_not` |
| 4383 | `TestFailedSearchesAreRetriedOnce.test_an_image_that_keeps_failing_is_not_retried_forever` | **core**: `batch_run::a_persistent_failure_is_retried_exactly_once`: two attempts, then the row stays `Error` |
| 4393 | `TestFailedSearchesAreRetriedOnce.test_the_setting_turns_it_off` | **core**: `batch_run::retries_off_means_one_attempt` |
| 4400 | `TestFailedSearchesAreRetriedOnce.test_several_failures_are_all_retried` | **core**: `batch_run::several_failures_are_all_retried_and_the_image_that_succeeded_is_not` (added by this PR; the map showed the core test covered one failure only) |
| 4411 | `TestFailedSearchesAreRetriedOnce.test_a_not_found_is_not_a_failure` | **core**: `batch_run::a_not_found_is_not_a_failure_and_is_not_retried` (added by this PR) |
| 4419 | `TestFailedSearchesAreRetriedOnce.test_stopping_cancels_the_retry_pass` | **core**: `batch_run::stopping_cancels_the_retry_pass`; the window's Stop is `smoke::stop_search_finishes_the_image_under_way` |
| 4439 | `TestFailedSearchesAreRetriedOnce.test_an_exhausted_daily_quota_cancels_the_retry_pass` | **core**: `batch_run::an_exhausted_quota_cancels_the_retry_pass` |
| 4457 | `TestFailedSearchesAreRetriedOnce.test_progress_accounts_for_the_extra_attempts` | **core**: `batch_run::progress_accounts_for_the_extra_attempts` |
| 4751 | `TestBatchCarriesOnWithoutSauceNao.test_it_stops_by_default` | **core**: `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it` (paused after the first image, two left `NotSearched`, the pause on disk); the window's end is `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` |
| 4755 | `TestBatchCarriesOnWithoutSauceNao.test_with_the_toggle_it_finishes_the_batch` | **core**: `batch_run::continue_without_saucenao_carries_on_and_says_so_once` asserts `Ending::Finished` and three searched |
| 4759 | `TestBatchCarriesOnWithoutSauceNao.test_the_later_images_are_searched_without_saucenao` | **core**: the same test; this PR adds the assertion that SauceNAO is asked exactly once, while the allowance was still there |
| 4765 | `TestBatchCarriesOnWithoutSauceNao.test_those_results_are_marked_provisional` | **core**: the same test asserts `searched_without_saucenao` is `[false, true, true]` and that only the complete result is cached |
| 4770 | `TestBatchCarriesOnWithoutSauceNao.test_it_says_so_once_rather_than_per_image` | **core**: `batch_run::continue_without_saucenao_carries_on_and_says_so_once` |
| 4899 | `TestPauseIsPersistedWhenTheBatchStops.test_pausing_records_the_counts` | **core**: `batch_run::an_exhausted_saucenao_quota_pauses_the_run_and_records_it`; `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` asserts the pause file's counts `(1, 2)` |
| 4948 | `TestPausedBannerContinueButton.test_continue_actually_launches_a_search_without_saucenao` | **6.e**: `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` clicks Continue with other engines and asserts the banner and the pause file go, the run takes the other engine and every row is searched; core: `batch_run::the_one_shot_override_continues_without_saucenao_and_without_it_the_run_pauses` |
| 5002 | `TestResumeWithoutSaucenaoOverride.test_without_the_override_it_still_pauses` | **core**: `batch_run::the_one_shot_override_continues_without_saucenao_and_without_it_the_run_pauses`, first half: without the override the run is `PausedForQuota { searched: 1, remaining: 2 }` |
| 5008 | `TestResumeWithoutSaucenaoOverride.test_the_override_finishes_the_batch_despite_the_saved_setting_being_off` | **core**: the same test, second half: with `force_continue_without_saucenao` the run finishes with three searched, the one `continuing` line is said, and the saved setting stays off |
| 5012 | `TestResumeWithoutSaucenaoOverride.test_the_saved_setting_is_unchanged_by_the_override` | **6.e**: the same scenario asserts the saved `continue_without_saucenao_on_quota` is as the user has it after the one-shot |
| 5144 | `TestModeStackSizeFloor.test_queue_floor_ignores_activitys_width_and_reviews_height` | **6.e**: does not apply as written: the Rust window's floor is the stated `MIN_WIDTH x MIN_HEIGHT` (`set_size_request` on the window), not the maximum over its pages, and the mode stack is not homogeneous (`shell.rs`, `set_hhomogeneous(false)`). What stands is that no page makes the window grow past it: `smoke::minimum_size_fits_every_state_in_both_schemes` asserts the outer rect stays within the stated size for every state, Activity with every extra engine on included |
| 5165 | `TestModeStackSizeFloor.test_switching_to_activity_grows_without_clipping` | **6.e**: `smoke::minimum_size_fits_every_state_in_both_schemes`, its `activity` state: the content area it is given at the stated minimum is at least what the page needs (width and height at that width) **and no taller than the window**, and the height the page asks for with no width is at most the stated minimum (the last two added after DAN-2324's 4.14 finding, section 3), both schemes |

#### `tests/test_hydrus_import_poll.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 123 | `TestTimeout.test_the_final_sleep_never_overshoots_the_timeout` | does not apply: the poll's timeout is core (Phase 5); the match is a `SearchWorker` mention |

#### `tests/test_hydrus_provenance_note.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 580 | `TestAutoImportWiring.test_nothing_is_confirmed_or_noted_when_every_setting_is_off` | 6.g: auto-import settings (the run passes them to `batch/run.rs`, which Phase 5 differentialed) |

#### `tests/test_interrupted_banner.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 483 | `TestStripNote.test_starting_a_search_clears_it` | **6.e**: `smoke::starting_a_search_clears_the_stopped_note` (added by this PR) restores after a crash, reads `stopped ...` on the strip, starts a search and asserts the note is empty; the row's `interrupted_mid_search` flag is spent by `batch_run::a_search_spends_the_interrupted_note` |

#### `tests/test_lens_browser.py` (3)

| Line | Test | Disposition |
|---|---|---|
| 445 | `TestOnlyCallingForTheUserWhenNeeded.test_the_script_touches_only_the_lens_window` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` and `desktop_attention::tests::the_script_is_pythons_with_the_class_repr_quoted` for the script; the Lens flow's calls to `quiet` and `ask_for_input` are Phase 4's (`lens_browser`), with this ticket supplying the implementation (the script touches only the Lens window's class) |
| 452 | `TestOnlyCallingForTheUserWhenNeeded.test_outside_kde_nothing_is_run` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` and `desktop_attention::tests::the_script_is_pythons_with_the_class_repr_quoted` for the script; the Lens flow's calls to `quiet` and `ask_for_input` are Phase 4's (`lens_browser`), with this ticket supplying the implementation (nothing is run outside Plasma) |
| 514 | `TestOnlyCallingForTheUserWhenNeeded.test_an_ordinary_search_never_asks` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` and `desktop_attention::tests::the_script_is_pythons_with_the_class_repr_quoted` for the script; the Lens flow's calls to `quiet` and `ask_for_input` are Phase 4's (`lens_browser`), with this ticket supplying the implementation (an ordinary search never asks) |

#### `tests/test_lens_dependency_alerts.py` (2)

| Line | Test | Disposition |
|---|---|---|
| 76 | `TestAlertsAreSaidOnce.test_a_listener_that_throws_does_not_take_the_search_down` | **core**: `engine_alerts::a_listener_that_panics_does_not_stop_the_others_or_the_caller` |
| 85 | `TestAlertsAreSaidOnce.test_an_unsubscribed_listener_stops_hearing` | **core**: `engine_alerts::an_unsubscribed_listener_stops_hearing` |

#### `tests/test_nothing_survived.py` (6)

| Line | Test | Disposition |
|---|---|---|
| 140 | `TestVariantShown.test_every_other_restore_keeps_the_generic_drop_zone` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |
| 381 | `TestActions.test_start_new_search_drops_back_to_the_ordinary_drop_zone` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |
| 393 | `TestRunStrip.test_the_strip_says_the_last_run_failed` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |
| 401 | `TestRunStrip.test_it_sits_beside_the_idle_count` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |
| 408 | `TestRunStrip.test_it_is_absent_on_an_ordinary_start` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |
| 415 | `TestDismissal.test_a_list_replaces_the_notice_and_the_strip_note` | 6.c: the Nothing-survived page and the strip's crashed note. **Not in `docs/gtk-shell.md` section 7** (the file is not among the 20 it lists); flagged for 6.j |

#### `tests/test_quota_pause_banner.py` (9)

| Line | Test | Disposition |
|---|---|---|
| 96 | `TestNoModal.test_a_quota_pause_raises_no_modal` | **6.e**: `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` pauses a live run and asserts `count_shown() == 0` with the banner visible (one scheme; Python loops `MODES`, and the banner in both schemes is judged in the `quota-*` shots) |
| 116 | `TestNoModal.test_a_pause_with_nothing_recorded_still_raises_no_modal` | **6.e**: not asserted: the branch exists (`search_run.rs`, `on_paused_out_of_quota`, `None` case: the status line is built from the run's counts and the banner still shows), but the scenario always has the pause file on disk, because the core writes it before the window hears of the pause; no test removes it first |
| 228 | `TestActions.test_review_is_the_primary_and_continue_is_the_secondary` | **6.e**, the half a test can read: `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` asserts Continue's words and that Review is not offered while nothing has been decided. Not asserted: Review's label `Review the N waiting →` and its visibility with rows waiting (`banners.rs`, `show_quota_banner`), and which action is primary and their order, which are judged in the `quota-*` shots (6.c) |
| 241 | `TestActions.test_review_opens_review_on_the_first_waiting_row` | **6.e/6.d/6.f**: not asserted: the button "Review the N waiting" is built, and stays disabled until 6.d and 6.f give it a row selection and Review (`DEFERRED(6.d/6.f)`, `window.rs`); the first waiting row's selection is theirs |
| 248 | `TestActions.test_review_is_hidden_when_nothing_is_waiting` | **6.e**: not asserted: Python hides the button when nothing is waiting; here it is disabled for every state (see 241) |
| 254 | `TestActions.test_continue_launches_without_saucenao_and_clears_the_banner` | **6.e**: `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` |
| 270 | `TestActions.test_clearing_the_list_retires_the_banner` | **6.e/6.c**: not asserted by a test for the quota banner specifically: `hide_quota_banner` is the one path; `smoke::clear_session_matches_the_empty_list_quirk` clears the list, and the strip's note is asserted cleared by `a_quota_pause_shows_the_banner_and_continue_resumes` after Continue |
| 319 | `TestStrip.test_the_strip_reads_paused_the_count_and_eta_paused` | **6.e**: `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` asserts the note `paused — quota exhausted` and `ETA paused` after a live pause. The SauceNAO readout (`saucenao_quota_label`) is pinned in the same scenario (`SauceNAO 2/200 used today`); the action differential also compares it after every step, which is a run, not a test |
| 327 | `TestStrip.test_the_run_ending_does_not_wipe_the_pause_from_the_strip` | **6.e**: the same scenario asserts, after the run has ended (`Search finished`), that the note and `ETA paused` are still there |

#### `tests/test_render_states.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 78 | `TestRenderStates.test_interrupted_paused_shows_the_quota_banner_in_both_themes` | 6.c: the quota banner in both schemes is `smoke::persisted_quota_pause` and the shot set |

#### `tests/test_run_banner.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 89 | `TestRunBanner.test_resume_button_routes_to_action_start_search` | **6.e**: `smoke::start_search_runs_the_batch` runs the `toggle-search` action that Resume Queue routes to; that the Resume button itself starts a search is not asserted (the Crashed banner's buttons are 6.c's, `smoke::restore_after_a_crash_shows_the_banner`) |

#### `tests/test_run_strip.py` (11)

| Line | Test | Disposition |
|---|---|---|
| 116 | `TestStripGeometry.test_eta_is_right_aligned_and_last` | 6.c: geometry of the strip, judged in the shot set |
| 144 | `TestStripVoice.test_mockup_vocabulary_while_running` | **6.e**: `eta::the_run_strip_follows_the_state_of_the_run` and `eta::the_countdown_rounds_halves_to_even` for the words; `shell::tests` holds the strip's sample text; the window assembling them while running is the action differential compares it after every step (evidence from a run, not a test in the tree) |
| 151 | `TestStripVoice.test_gauge_means_searched_and_sent_is_a_separate_count` | **6.e**: `smoke::start_search_runs_the_batch` asserts `3/3 searched` and the sent count separately (`run_strip_and_sent_count_follow_the_list` for the sent half) |
| 160 | `TestStripVoice.test_quota_reads_without_a_colon` | **6.e**: `saucenao::the_daily_allowance_reads_as_used_out_of_the_limit_else_what_is_left_else_nothing` for the text (Python's label drops the colon: `refresh_saucenao_quota_label` does the same, `replacen("SauceNAO: ", "SauceNAO ")`); the window's label is pinned by `smoke::a_quota_pause_shows_the_banner_and_continue_resumes` (`SauceNAO 2/200 used today`: the colon is gone and nothing else was added), and the action differential also compares it after every step (evidence from a run, not a test in the tree) (`quota_label`) |
| 170 | `TestStripVoice.test_eta_reads_in_its_own_readout` | **6.e**: `eta::nothing_is_said_until_two_intervals_have_passed`, `eta::the_baseline_is_the_first_signal_and_the_total_is_reread`, `eta::the_run_strip_follows_the_state_of_the_run` |
| 184 | `TestStripVoice.test_figures_are_bold_and_labels_are_mono` | 6.c: bold figures and mono labels are judged in the shot set |
| 213 | `TestStripVoice.test_a_new_reading_leaves_no_old_text_under_it` | does not apply: Qt painted stale child labels; a `gtk::Label` has one text |
| 241 | `TestIdleStrip.test_empty_window_reads_nothing_queued_eta_dash` | **6.c**: `smoke::run_strip_and_sent_count_follow_the_list` |
| 254 | `TestIdleStrip.test_a_fresh_window_is_not_blank_before_anything_refreshes` | **6.c**: `smoke::run_strip_and_sent_count_follow_the_list` |
| 261 | `TestIdleStrip.test_listed_but_unsearched_images_are_not_called_nothing_queued` | **6.c**: `smoke::run_strip_and_sent_count_follow_the_list` |
| 267 | `TestIdleStrip.test_a_finished_run_keeps_its_count_and_drops_the_eta` | **6.e**: `smoke::start_search_runs_the_batch`; `eta::a_finished_run_has_nothing_left` |

#### `tests/test_search_quality.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 4256 | `TestSendUrlFallsBackToHatatesDownload.test_auto_import_downloads_when_no_match_hydrus_can_take` | 6.g: Send-URL fallback; the run's own auto-import is `batch/run.rs` (Phase 5) |

#### `tests/test_thin_modules.py` (16)

| Line | Test | Disposition |
|---|---|---|
| 254 | `TestDesktopAttention.test_not_available_off_plasma` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` |
| 258 | `TestDesktopAttention.test_not_available_with_no_desktop_set_at_all` | **6.e**: `desktop_attention::tests::the_desktop_name_is_matched_case_insensitively_and_the_qt6_binary_name_is_a_fallback` (an empty desktop) |
| 264 | `TestDesktopAttention.test_the_desktop_name_is_matched_case_insensitively` | **6.e**: `desktop_attention::tests::the_desktop_name_is_matched_case_insensitively_and_the_qt6_binary_name_is_a_fallback` |
| 271 | `TestDesktopAttention.test_not_available_on_plasma_without_qdbus` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` |
| 276 | `TestDesktopAttention.test_the_qt5_era_binary_name_is_accepted_as_a_fallback` | **6.e**: `desktop_attention::tests::the_desktop_name_is_matched_case_insensitively_and_the_qt6_binary_name_is_a_fallback` |
| 282 | `TestDesktopAttention.test_nothing_is_run_when_the_desktop_is_not_plasma` | **6.e**: `desktop_attention::tests::off_plasma_or_without_qdbus_every_call_does_nothing` |
| 290 | `TestDesktopAttention.test_quiet_takes_the_window_off_the_taskbar_and_drops_attention` | **6.e**: `desktop_attention::tests::quiet_and_ask_load_start_and_unload_the_kwin_script_in_python_order` |
| 295 | `TestDesktopAttention.test_ask_for_input_puts_it_back_and_flags_it` | **6.e**: the same test (its `ask_for_input` half) |
| 300 | `TestDesktopAttention.test_the_script_only_touches_the_named_window_class` | **6.e**: `desktop_attention::tests::the_script_is_pythons_with_the_class_repr_quoted` |
| 308 | `TestDesktopAttention.test_the_script_handles_both_the_old_and_new_kwin_apis` | **6.e**: `desktop_attention::tests::the_script_is_pythons_with_the_class_repr_quoted` (the whole script is compared, both KWin APIs in it) |
| 315 | `TestDesktopAttention.test_the_dbus_calls_are_made_in_the_order_kwin_needs` | **6.e**: `desktop_attention::tests::quiet_and_ask_load_start_and_unload_the_kwin_script_in_python_order` (the four calls and their arguments, against a fake `qdbus6` on a temp `PATH`) |
| 333 | `TestDesktopAttention.test_the_script_file_is_deleted_afterwards` | **6.e**: `desktop_attention::tests::the_script_file_goes_afterwards_and_a_nonzero_exit_is_not_an_error` |
| 347 | `TestDesktopAttention.test_the_temp_file_is_cleaned_up_even_when_a_call_fails` | **6.e**: `desktop_attention::tests::a_command_that_cannot_start_is_swallowed_after_one_call_and_the_file_still_goes` |
| 360 | `TestDesktopAttention.test_nothing_is_unlinked_when_the_temp_file_could_not_be_created` | **6.e**: not asserted: the temp file is made with `std::fs::write` into `std::env::temp_dir()`, and no test makes that fail; the write's error is returned before any removal, as in Python (`path` unset) |
| 374 | `TestDesktopAttention.test_a_subprocess_error_is_swallowed` | **6.e**: `desktop_attention::tests::a_command_that_cannot_start_is_swallowed_after_one_call_and_the_file_still_goes` |
| 381 | `TestDesktopAttention.test_run_never_lets_a_nonzero_exit_become_an_exception` | **6.e**: `desktop_attention::tests::the_script_file_goes_afterwards_and_a_nonzero_exit_is_not_an_error` (every call exits 7; all four are made and nothing is raised); the 5 s timeout kill is not asserted |

#### `tests/test_url_importer_duplicate_relationships.py` (1)

| Line | Test | Disposition |
|---|---|---|
| 433 | `TestAutoImportWiring.test_nothing_is_confirmed_or_related_when_both_settings_are_off` | 6.g: auto-import settings (as above) |

#### `tests/test_workers_background.py` (4)

| Line | Test | Disposition |
|---|---|---|
| 126 | `TestAvailabilityWorker.test_the_post_page_is_checked_for_a_soft_404_site` | **core**: the soft-404 and reactor-full decisions are `hatate_core::availability` (Phase 4), in `availability_https.rs` and the module's tests; the worker's shell here only forwards them (`smoke::the_availability_check_offers_to_drop_the_dead_matches`) |
| 139 | `TestAvailabilityWorker.test_the_post_page_is_normalised_before_the_soft_404_decision` | **core**: the soft-404 and reactor-full decisions are `hatate_core::availability` (Phase 4), in `availability_https.rs` and the module's tests; the worker's shell here only forwards them (`smoke::the_availability_check_offers_to_drop_the_dead_matches`) |
| 186 | `TestAvailabilityWorker.test_a_check_that_raises_is_counted_as_done_without_taking_the_run_down` | **core**: `availability_https::the_manual_check_records_each_verdict_and_counts_only_the_gone` (a check that raises is counted done); see `hatate-core/src/availability.rs` `check_one_candidate` |
| 237 | `TestAvailabilityWorker.test_the_reactor_full_fallback_is_NOT_applied_on_this_path` | **core**: the soft-404 and reactor-full decisions are `hatate_core::availability` (Phase 4), in `availability_https.rs` and the module's tests; the worker's shell here only forwards them (`smoke::the_availability_check_offers_to_drop_the_dead_matches`) |

