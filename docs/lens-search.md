# Google Lens, the per-search page work, the site lookups and the runner wiring (Phase 5.c, DAN-2165)

What this ticket ports, where the cut is, how it was checked against the real Python, which
differences could not be matched, and which Python tests map to which Rust ones. Python source of
truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`, `core/lens_browser.py` (Playwright 1.63 in the
Hatate venv), `core/boorus/rule34.py` and `core/reddit.py`. The browser, for the differential and for
the tests, is Helium 156 (`/opt/helium-browser-bin/chrome`, Chromium 156), the same binary on both
sides.

## 1. The cut

| Python | Rust |
|---|---|
| `lens_browser._search_area`, `_widen_search_area`, `_drag`, `_settle`, `_click_if_present`, `_reveal_explicit_results`, `_collect_tiles`, `_one`, and `WHOLE_ENOUGH`, `DRAG_OVERSHOOT_PX`, `SETTLE_*`, `AFTER_*_MAX_MS`, `TILE_RENDER_MAX_MS`, `CLICK_TIMEOUT_MS`, `EXPLICIT_REVEAL_LABELS`, `RESULTS_URL_RE`, `SOURCE_HINT_RE` | `lens_browser/search.rs` (`LensSearch`, the shipped `SearchStep`; `source_hint` is `SOURCE_HINT_RE` walked by hand) |
| `fetch_results_payloads` | `LensBrowser::fetch` (5.b), which `LensClient` calls through `LensFetch` |
| `google_lens.EXACT_TILES_JS` | `engines/lens_prep.rs::EXACT_TILES_JS` |
| the Playwright calls the above make (`query_selector_all`, `ElementHandle.bounding_box/get_attribute`, `page.mouse`, `get_by_text(exact=True)`, `get_by_role("link", name=...)`, `page.on("response")`, `wait_for_url`, `page.evaluate`) | `lens_browser/elements.rs` |
| `boorus.rule34.find_post_by_search`, `LISTING_URL`, `POST_URL`, `LISTING_HASH_SIZE`, `LISTING_MAX_DISTANCE`, `LISTING_THUMB_WORKERS` | `boorus/rule34.rs` |
| `reddit.find_post` (network half, ported in 3.b) and the two lookups behind `LensSites` | `engines/lens_sites.rs` (`OnlineSites`) |
| the Lens wiring of `engine_runner` | `LiveEngines::for_profile` builds `LensBrowser` + `OnlineSites`; `LiveEngines::shutdown` stops the browser at the end of `hatate batch` |

Not here, and who owns it:

* `core/desktop_attention.py` (KWin) has no ticket (5.b's note stands). `WindowAttention` keeps its no-op.
* AppImage packaging of Chromium (`tests/test_appimage_lens.py`) is Phase 7.
* The Lens window's `bring_to_front` is a CDP `Page.bringToFront`; a compositor that ignores it is
  the same case as in Python.

## 2. Changes to merged modules

* `lens_browser.rs`: `UnportedSearch` is gone; `LensSettings::shipped` holds `LensSearch::shipped()`.
  `StepCtx::quiet_attention`, `LensBrowser::profile_dir`.
* `lens_browser/page.rs`: `Locator` holds a JavaScript expression for its matches (so the text and
  role queries share the click/count code), `PageError::bare`, a few items `pub(super)`.
* `engines/lens_rank.rs`: `LensSites` returns `Result<_, Unexpected>`. Python lets a decompression
  bomb in a thumbnail (`dhash` out of the pool) and a hard deadline (`reddit._feed_entries`) out of
  both lookups, and the exception ends `search`; `NoSites` stays as the test double. `resolve_subreddits`
  returns a `Result` for the same reason.
* `engines/lens_prep.rs`: `upload_script_to` (the form action is a parameter; `upload_script` passes the
  shipped constant).
* `engine_backends.rs`: `LiveEngines::for_profile` builds the Lens client over a real `LensBrowser`;
  `LiveEngines::new` keeps `NoBrowser`, so every test that does not mean to start a browser cannot.
* `hatate-cli/src/batch_cmd.rs`: `live.shutdown()` after the batch.
* `tests/common/scripted.rs`: `lens_backend`, as `index_backend` is for the Pawchive index.

## 3. No way to redirect Lens

`LensSearch::with_urls` is the only way to point the page at another address, and only a test calls
it; `LensSearch::shipped()` is the only value the binary builds. No environment variable, flag or
config key changes the Lens URL, an engine host or a site host, and nothing turns certificate
checks off in the browser (`ignore_https_errors: false`, unchanged from 5.b).

## 4. How it was checked

The harness lives outside the tree (`worktrees/DAN-2165-tools`, Python and shell); its log is
`worktrees/DAN-2165-differential.log`. Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`.

**Browser differential** (Python Playwright 1.63 running the real `_one`, against the Rust CDP code,
both driving the same Helium 156, headed, in Xvfb, a fresh temp profile under a temp `HOME`, against
one scripted fake-Lens site on 127.0.0.1 built from synthetic pages; nothing is captured from Google).
Each case is compared on the returned payloads and tiles (or the error class and message), the
request log the site records, the page events the pages report back (mouse down/up/move/click),
the launch argv, `navigator.webdriver`, and every wait requested. Seed 101: 65 scenarios, 0 differing
(whole-image search, a partial area that needs widening, the explicit-results notice, the Visual
matches tab, a rule34 search tile, empty results, "can't read file", the browser closed mid-search
and relaunched, an unanswered upload, a challenge on each page, extra fetches, and 40 generated
timing cases).

What the harness does so the two clients are compared on logic and not on a race, none of it a
tolerance in the product:

* **The one documented class, A:** Python's Playwright launches Chromium with
  `--remote-debugging-pipe`; the Rust client uses `--remote-debugging-port=0` (chromiumoxide). The
  argv differs by that one flag in every case (count: 1 per case) and by nothing else.
* **Start-up pause (`LensSettings::settle_after_launch`, zero as shipped).** A freshly started
  Chromium stalls for about a second. In Python the stall lands on the page's first timer (its first
  command goes out about 6 ms after the launch); in Rust it lands on `Page.navigate` (its first
  command goes out about 130 ms after). Both sides pause 1.5 s after the launch in the harness; the
  four relaunch cases (`gone-*`) match exactly with it and differ by one settle poll without it.
  The knob is test-only and nothing in the shipped binary sets it.
* **Bodies finished within milliseconds of each other** (a fetch chain) are compared as a multiset,
  since the order they finish in is the network's; scroll events are not compared (their delivery
  races a navigating click, and the offset shows in the click's `clientY`, which is).

**Site lookups** (`rule34.find_post_by_search`, `reddit.find_post`): the transport differential over
a local server, 170 cases per seed, comparing the return value, every request and every sleep: a
hit within the distance limit, the nearest post past it, an empty listing, a non-200, a timeout, a
thumbnail that does not decode, a decompression bomb, a found, a missing and a removed Reddit post.
Seeds 101 and 303: 0 differing. Seed 202: one case, a decompression-bomb thumbnail, where Python's
executor cancels the thumbnails it has not started so how many it requests varies from run to run;
Rust asks for all of them, which is within what Python does (the committed test is
`a_decompression_bomb_fails_the_lookup_and_the_other_thumbnails_were_still_asked_for`).

**Session differential** (`hatate batch` with Lens on, a scripted `LensFetch` on both sides): 60
scenarios (seeds 1 to 60 of `sess_py.py`), 2 to 5 pictures each in PNG, JPEG, BMP, GIF, TIFF and
WebP, a few large enough to be re-encoded; the script gives each image pages, empty pages, a browser
failure, an unavailable browser or an unanswered robot check, with rule34 listings of every kind
and candidate pages that answer 404. Python's `SearchWorker.run()` and the Rust `run_batch` search the same folder;
Python opens the Rust session in `load_entries`. 201 session entries, 186 fetches, **0 differing,
0 corrupt rows**. Time is virtual on both sides (Python's `time.sleep`, `monotonic` and `time` are
replaced, because Lens spaces searches 45 s apart; the total slept is compared), and `TZ=UTC`.

**J1, the Lens upload bytes (DAN-2183).** Over those 186 fetches the Hamming distance between the
`dhash` (16x16) of Python's upload bytes and Rust's is at most **1** of 256: 185 fetches at 0 and
1 at 1. 67 of the 186 were re-encoded to JPEG; in 66 of those the two encoders produced identical
bytes. The grant covers it (2 or less).

## 5. Python tests that exercise this code

| Python | Rust |
|---|---|
| `test_lens_browser.py::TestWaitingOnResultsNotTheClock` (4) | `lens_browser_search::a_slow_response_is_still_waited_for`, `a_step_whose_response_never_comes_waits_no_longer_than_the_fixed_wait_it_replaces`, `controls_that_are_not_there_cost_no_click_timeout`, `a_whole_image_search_uploads_reads_both_tabs_and_collects_the_tiles` |
| `TestOnlyCallingForTheUserWhenNeeded`: `test_a_check_after_the_upload_asks_for_the_user`, `…_after_clicking_a_tab_asks_too`, `…_after_the_explicit_notice_asks_too`, `…_on_first_opening_google_asks_too`, `test_an_in_page_recaptcha_counts_as_a_check`, `test_an_unanswered_check_is_reported_and_the_window_goes_quiet`, `test_an_ordinary_search_never_asks` | `lens_browser_search::a_robot_check_after_the_upload_calls_the_user_and_carries_on_once_cleared`, `a_check_after_opening_a_tab_asks_the_user_too`, `a_robot_check_that_goes_unanswered_is_reported_and_the_window_goes_quiet`, and the first assertions of `a_whole_image_search_…` (an ordinary search asks nobody); the explicit-notice and first-open checks are in the browser differential (`challenge-*` cases) |
| `FakePage`-driven `_one` calls in `test_lens_browser.py` | the browser differential runs the real `_one` against the real Chromium on both sides; committed: `a_partial_search_area_is_dragged_out_and_the_widened_results_win`, `a_widen_that_brings_nothing_new_falls_back_to_the_cropped_results`, `the_explicit_results_notice_is_confirmed_when_there_is_no_tab_to_open`, `empty_results_come_back_as_no_payloads_and_no_tiles`, `a_page_that_cannot_read_the_file_ends_in_the_navigation_timeout`, `an_upload_that_never_answers_ends_in_a_browser_error`, `a_window_closed_mid_search_is_reopened_and_the_search_runs_again` |
| `test_search_quality.py::TestRule34SearchTiles` (`test_listing_posts_are_read`, `test_the_closest_post_is_chosen_over_a_variant`, `test_nothing_close_enough_finds_nothing`) | `boorus::rule34::tests::listing_posts_are_read_in_page_order`, `lens_sites_https::the_closest_post_within_the_limit_is_found_over_https`, `the_nearest_post_past_the_limit_is_not_a_match` |
| `…::test_search_puts_the_recovered_post_first` | `lens_batch::lens_finds_matches_and_a_rule34_search_tile_is_recovered_through_the_listing_page` (end to end, over https) |
| `TestSubredditLinksBecomeThePost` (`only_listings_count`, `the_post_holding_the_image_is_found`, `an_unfound_post_falls_back…`, `other_urls_are_untouched…`) | `reddit::tests::only_listings_count_as_subreddits`, `the_post_holding_the_image_is_found`, `an_unfound_post_falls_back_to_the_image_not_the_subreddit`, `other_urls_are_untouched_and_never_looked_up` (3.b); the network half: `lens_sites_https::a_found_post_a_missing_post_and_a_removed_post` |
| `…::test_a_rate_limited_feed_falls_back_and_is_not_remembered`, `test_a_subreddit_with_no_image_to_go_on_is_dropped`, `test_lens_search_hands_back_the_post` | 3.b `reddit::tests`, and `lens_sites_https::a_dropped_connection_or_a_slow_listing_finds_nothing` for the network error |
| `test_lens_browser.py::TestAClosedWindowIsReopened`, `TestNotAskingGoogleTooOften`, `TestTheWindowHasItsOwnClass` | 5.a and 5.b, see `docs/lens-browser.md` section 4 |
| `tests/test_appimage_lens.py` | Phase 7 |
| `tests/live_sources/check_google_lens_dependency.py` | not applicable: it talks to the live service and is never run in CI |
| no Python test covers the runner's Lens wiring end to end | new: `lens_batch` (4 tests: a found match with a rule34 recovery, an unavailable browser latches Lens, an unanswered robot check rests it, three browser failures stand it down) and `batch_cmd::tests::the_batch_engines_put_a_lens_browser_on_the_profiles_lens_profile` |

## 6. Error shape (DAN-2116 item 1)

This ticket adds no error type. A page-level failure in the search half is the `PageError` of 5.b,
which `LensFetchError` carries into `LensError` (5.a); a site lookup that lets an exception out
(a decompression bomb, a hard deadline) is `LensError::Unexpected`, as Python lets it out of `search`.
`kind()`, `user_message()` and `detail()` are 5.a's and 5.b's, and the test that keeps paths and OS
text out of the message is `lens_browser::the_error_shape_keeps_paths_and_os_text_out_of_the_user_message`.
`Entry.error_message` and `MatchStatus` are unchanged on disk.
