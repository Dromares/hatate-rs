# Google Lens, the browser lifecycle: Chromium lookup, launch, job loop, warm-up, challenge (Phase 5.b, DAN-2164)

What this ticket ports, where the cut is, how it was checked against the real Python, which
differences could not be matched, and which Python tests map to which Rust ones. Python source of
truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`, `core/lens_browser.py` (Playwright 1.63 in the
Hatate venv). The browser, for the differential and for the tests, is Helium 156
(`/opt/helium-browser-bin/chrome`, Chromium 156), the same binary on both sides; `/usr/bin/chromium`
on the build machine is broken (`libsimdutf.so.36` missing).

## 1. The cut

| Python (`core/lens_browser.py`) | Rust |
|---|---|
| `playwright_available`, `_browsers_root`, `chromium_installed`, `DependencyStatus`, `check_dependencies`, `classify_launch_failure`, `bundled_browser_message`, `missing_dependency_message`, `missing_chromium_message`, `MISSING_PLAYWRIGHT`, `MISSING_CHROMIUM` | `lens_browser/deps.rs` |
| the `_launch_context` argv and profile, `PROFILE_DIR`, `WINDOW_CLASS` | `lens_browser/launch.rs` |
| the page calls the lifecycle makes (`_turn_off_safesearch_blur`, `_warm_up`, `_is_challenged`, `_wait_out_challenge`, `RECAPTCHA_SELECTOR`, `CHALLENGE_WAIT_MS`, `CHALLENGE_POLL_MS`, `WARMUP_SEARCHES`) | `lens_browser/page.rs` |
| `LensBrowser` (`start`, `shutdown`, `fetch`, `_run`, `_drain`, `_launch`, `_serve`), `_Job`, the module-level `shutdown`, `_unavailable`, `_browser_gone`, `_first_line` | `lens_browser.rs`; `LensBrowser: LensFetch` |
| `LensBrowserError`, `LensBrowserUnavailable`, `LensChallengeUnanswered` | not redefined: 5.a's `LensFetchError` / `LensMissing` in `engines/lens_search.rs` |

Not here, and who owns it:

* The per-search half (`_search_area` through `_one`, `fetch_results_payloads`) is 5.c (DAN-2165).
  `LensBrowser` takes it as a `SearchStep`; the shipped step is `UnportedSearch`, which answers
  `LensBrowserUnavailable`, and `LiveEngines` is not rewired, so `hatate batch` prints what it printed
  before this ticket.
* `core/desktop_attention.py` (the KWin taskbar/attention script, `tests/test_thin_modules.py`) has no
  ticket. `WindowAttention` is a trait here (`quiet`, `ask_for_input`) with a no-op shipped
  implementation. It needs an owner before Lens is wired for Plasma users.

## 2. Chromium without Playwright

The Rust app reports only Chromium as a missing dependency (`LensMissing::Chromium`; there is no
`Playwright` to miss). `find_chromium` looks, in order:

1. the AppImage's bundled browser (`PLAYWRIGHT_BROWSERS_PATH` inside the AppImage; when it is
   missing the AppImage is damaged and `PATH` is **not** consulted);
2. Playwright's existing download, `ms-playwright/chromium-*` under `PLAYWRIGHT_BROWSERS_PATH` or the
   cache directory, read as `chromium_installed` reads it (newest revision, the headless shell does
   not count), so a current user needs no new download;
3. `chromium`, `chromium-browser`, `google-chrome-stable`, `google-chrome` on `PATH`, in that order.

The launch keeps Python's page-observable state: headed, `lens_profile` under the data directory,
`--class=hatate-lens`, no `--enable-automation`, `--disable-blink-features=AutomationControlled`,
1280x900 (Playwright's window insets and screen are reproduced), `en-US`.

### Dependency-message copy (granted class D)

Python's text names `pip` and Playwright, neither of which exists in this build. Python (`<py>` is
`sys.executable`):

```
Google Lens needs Playwright and a Chromium build, and they are not installed for the Python running this app (<py>). Install them with:
    <py> -m pip install playwright
    <py> -m playwright install chromium
then restart.
```
```
Google Lens has Playwright installed for the Python running this app (<py>), but the Chromium build it drives is missing. Install it with:
    <py> -m playwright install chromium
then restart. It is about 150MB, and nothing else in this app needs it.
```

Rust (`missing_chromium_message`, and `missing_dependency_message`, which is the same text):

```
Google Lens needs a Chromium-based browser and none was found. Install Chromium or Google Chrome with your package manager, so that one of `chromium`, `chromium-browser`, `google-chrome-stable` or `google-chrome` is on your PATH, then restart. Nothing else in this app needs it.
```

`bundled_browser_message` (the AppImage case) is unchanged from Python, character for character.

## 3. How it was checked

**Browser differential.** Python's Playwright code at `76c85c4` and the Rust CDP code drive the same
Chromium against the same scripted fake-Lens site, served from 127.0.0.1 by the harness. Each run is
headed on Xvfb, with a fresh temp profile under a temp `HOME`. Compared: the returned payloads and
tiles or the error class and message; the site's request log (method, path with query, upload byte
hash) and request headers; the page actions in order; the launched browser's argv from
`/proc/<pid>/cmdline`; `navigator.webdriver`; every wait requested; attention calls; module state.
Challenge timing uses a **scaled wait on both sides** (the scenario sets `CHALLENGE_WAIT_MS` and
`CHALLENGE_POLL_MS` to small values on both; the Rust side through the `LensSettings` fields `challenge_wait_ms` and `challenge_poll_ms`, the
Python side by patching the module constants), with the requested waits compared exactly.

The harness lives outside the repo (the tree is Rust only); its log is
`worktrees/DAN-2164-differential.log`. Three seeds (101, 202, 303), run in the foreground:

| seed | cases | differing | unexplained |
|---|---|---|---|
| 101 | 159 | 0 | 0 |
| 202 | 156 | 0 | 0 |
| 303 | 158 | 0 | 0 |
| total | 473 | 0 | 0 |

Each seed is 120 dependency-check cases (bundled, the Playwright download in each layout, each `PATH`
name, none) and about 40 browser cases: launch state, warm-up variants, SafeSearch toggle (already
off, off, no control), challenges that clear within the wait (DOM and `/sorry` kinds) and ones that do
not, a challenge during warm-up, the window closed once and twice, in the challenge check and in the
wait-out, launch failures (exits, executable missing, no directory), a profile another Chromium
holds, shutdown with a job ahead, the four error answers, and random mixes.

"0 differing" means every compared field matched or fell in one of the classes below. The classes are
counted where the harness meets them (a launch counts once per browser started; a case can meet a class
more than once). Totals over the three seeds:

| class | what | count | status |
|---|---|---|---|
| D | dependency message copy (section 2) | 6 | **granted** by the ticket |
| P3 | a browser on `PATH`: Python says missing, Rust finds it | 95 | **granted** (plan decision: `PATH` lookup) |
| A | the transport flag: Playwright launches with `--remote-debugging-pipe`, `chromiumoxide` with `--remote-debugging-port=0`; the rest of the argv is equal | 153 | not granted; brought to Cloud |
| B | launch-failure text after the `The browser could not be started: ` prefix: Playwright's call log vs the process output | 6 | not granted; brought to Cloud |
| E | a failed navigation (`net::ERR_*`): Playwright's next `goto` is "interrupted by another navigation to chrome-error://chromewebdata/", so one more wait is requested by the Rust side (`[3000, 3000]` vs `[3000, 3000, 2000]`, and `[]` vs `[2000, 2000]`) | 6 | not granted; brought to Cloud |
| P1 | `PLAYWRIGHT_BROWSERS_PATH=0`: Python cannot tell where the browsers are and carries on to the launch; Rust has no browser | 14 | not granted; brought to Cloud |
| P2 | a `chromium-*` directory with no executable inside: Python's check says present, Rust's says missing | 13 | not granted; brought to Cloud |

## 4. Python tests that exercise this code

`tests/test_lens_browser.py`, `tests/test_lens_dependency_alerts.py`, `tests/test_appimage_lens.py`,
`tests/test_thin_modules.py` (`desktop_attention`), `tests/live_sources/check_google_lens_dependency.py`.
The browser tests below are `#[ignore]` and need a Chromium (`HATATE_TEST_CHROMIUM` or one on `PATH`)
and a display; the rest run in the hosted `cargo test`.

| Python | Rust |
|---|---|
| `TestWaitingOnResultsNotTheClock` (4 tests) | 5.c: the per-search waits |
| `TestAClosedWindowIsReopened.test_the_interrupted_search_is_handed_back_to_run_again`, `test_only_once` | `lens_browser_cdp::a_window_closed_under_a_search_is_reopened_and_the_search_run_again_once` |
| `…test_any_other_failure_reaches_the_caller_as_a_lens_error` | `lens_browser::first_line_is_the_first_line_of_the_stripped_text_or_the_type_name`, `a_closed_target_is_gone_and_so_is_any_text_that_says_closed`, `lens_browser_cdp::a_job_ahead_of_shutdown_still_runs_and_a_page_error_is_its_first_line` |
| `…test_a_relaunch_that_fails_answers_the_waiting_search` | `lens_browser_cdp::a_launch_that_fails_fails_every_waiting_job_and_names_a_missing_browser` |
| `TestNotAskingGoogleTooOften`: `test_an_unanswered_check_rests_lens_through_a_new_search`, `test_the_rest_ends*`, `test_a_resting_lens_says_so_on_the_row`, `test_searches_are_spaced_start_to_start`, `test_two_arriving_together_are_both_spaced`, `test_stopping_ends_the_wait`, `test_resting_until_survives_a_concurrent_clear` | 5.a (pacing and stand-down in `engines/lens_search.rs`, DAN-2163); not 5.b |
| `…test_the_browser_does_not_say_it_is_automated` | `lens_browser_cdp::the_page_reads_as_the_python_launch_does` (`navigator.webdriver` as the page reads it), `launch::the_argv_keeps_the_page_observable_state_of_the_python_launch` |
| `TestTheWindowHasItsOwnClass.test_chromium_is_launched_with_it` | same two tests (`--class=hatate-lens` from `/proc/<pid>/cmdline`) |
| `TestOnlyCallingForTheUserWhenNeeded.test_the_script_touches_only_the_lens_window`, `test_outside_kde_nothing_is_run` | not applicable: `desktop_attention` is not ported (section 1) |
| `…test_a_check_after_the_upload / a_tab / the_explicit_notice / first_opening_google_asks_too`, `test_an_ordinary_search_never_asks` | 5.c: those checks sit in the per-search half; the lifecycle half is `lens_browser_cdp::a_robot_check_that_clears_within_the_wait_is_waited_out_and_the_user_is_called` |
| `…test_an_in_page_recaptcha_counts_as_a_check` | `a_robot_check_that_clears_within_the_wait_is_waited_out_and_the_user_is_called` (the `RECAPTCHA_SELECTOR` branch is also in the differential's `challenge-dom-*` cases) |
| `…test_an_unanswered_check_is_reported_and_the_window_goes_quiet` | `lens_browser_cdp::a_robot_check_that_is_not_answered_ends_the_search_unanswered` |
| `test_lens_dependency_alerts`: `test_no_playwright_package` | not applicable: there is no Playwright package to miss |
| `…test_playwright_but_no_chromium`, `test_both_present` | `deps::nothing_found_reports_chromium_only_with_copy_that_names_no_pip`, `deps::the_playwright_download_is_found_by_its_newest_revision` |
| `…test_an_unreadable_layout_is_not_reported_as_missing`, `test_browsers_inside_the_package_cannot_be_read` | `deps::chromium_installed_is_three_valued_and_ignores_the_headless_shell` |
| `…test_a_downloaded_chromium_is_found`, `test_an_empty_registry_means_nothing_was_installed`, `test_the_headless_shell_alone_does_not_count`, `test_a_missing_registry_directory_means_nothing_was_installed` | `deps::the_root_follows_the_override_then_the_cache`, `chromium_installed_is_three_valued_and_ignores_the_headless_shell`, `the_playwright_download_is_found_by_its_newest_revision` |
| `…test_playwrights_own_wording_is_recognised`, `test_anything_else_is_not_a_dependency_problem`, `test_the_helper_rewrites_only_the_dependency_case` | `deps::only_an_executable_that_is_not_there_is_a_missing_dependency`; Playwright's own wording does not exist here, so `classify_launch_failure` reads the spawn error (`launch::a_missing_executable_fails_as_a_spawn_error_not_found`) |
| `…test_a_missing_dependency_is_reported_with_which_one`, `test_a_missing_chromium_is_announced_with_its_fix`, `test_a_browser_that_would_not_start_is_still_announced` | `lens_browser_cdp::no_browser_at_all_is_the_missing_chromium_answer_before_any_launch`, `a_launch_that_fails_fails_every_waiting_job_and_names_a_missing_browser` |
| `…test_a_missing_playwright_says_the_package` | not applicable (no Playwright) |
| `…test_a_whole_batch_raises_it_once`, `test_an_alert_reaches_its_listeners`, the alert-key tests, `test_the_engine_runner_records_the_error_and_returns_false` | 5.a (`engine_alerts.rs`, the runner) |
| `test_appimage_lens.py` | Phase 7 |
| `test_thin_modules.py` (`desktop_attention`) | not applicable: unported (section 1) |
| `test_gui_smoke.py`, `test_search_quality.py` references | GUI and per-search, not this ticket |
| `live_sources/check_google_lens_dependency.py` | not applicable: a live-network check; nothing here contacts Google |

New with no Python counterpart: the profile another Chromium holds
(`a_profile_another_chromium_holds_is_named_in_the_error`), shutdown with a job ahead
(`a_job_ahead_of_shutdown_still_runs_and_a_page_error_is_its_first_line`), the browser outliving a ten
second wait (`the_browser_outlives_a_ten_second_wait`), the warm-up
(`the_warm_up_turns_safesearch_off_once_and_runs_both_searches`), and the error shape
(`lens_browser::the_error_shape_keeps_paths_and_os_text_out_of_the_user_message`).

The two connection states 5.a left unreachable are reached through `LensClient::with_connection_sink`:
`WaitingForLens` by `a_search_under_way_waits_on_lens_and_a_failed_image_does_not_leave_it_waiting`,
`NotReachable` by `a_browser_that_cannot_start_is_not_reachable_and_stays_so_for_the_run`
(`tests/google_lens_search.rs`).

## 5. Dependencies

`chromiumoxide` 0.9.1, default features off (the plan's CDP client; its tokio, tungstenite and `which`
closure is the `Cargo.lock` growth), and `futures`, which polls its handler stream. No other crate.
