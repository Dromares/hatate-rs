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
