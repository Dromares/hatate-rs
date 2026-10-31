# Settings Test buttons, Fetch from Hydrus and the Review Shortcuts dialog (Phase 6.h-2)

Work in progress Rust rewrite, not yet a replacement for Hatate. This is the doc of [DAN-2448](/DAN/issues/DAN-2448).
It holds the two maps Phase 6 asks every ticket for (the method map and the Python test map; 6.j checks the union), the
differentials with their counts and documented classes, the on-purpose differences and the shot set.
Python is `Dromares/Hatate-Linux-Redux` at `76c85c4`. Nothing in the tests, the differential or the shots reaches a real
network: every host a Test can name is scripted (`SiteFx`, a local https server with a throw-away CA and per-name leaf
certificates) and the window's net layer refuses any host that is not (`NetConfig::only_scripted_hosts`, test builds only).

## 1. The cut

**Ported here** (`hatate-gtk`; `hatate-core` gains `net::site::cookie_paste_warnings`, `NetConfig::only_scripted_hosts` and
the `net::HeaderMap` re-export; no new dependency):

- `settings_tests.rs`: the six Settings Tests (DeviantArt, Anime-Pictures, Sankaku, e621, Pixiv, Hydrus) as a pure
  `plan(Test, &Typed) -> Plan` (what is asked first and what is refused before any request) and an async `run(job, net,
  booru) -> String` (the request and the label's text). The typed values are read, never the saved ones.
- `settings_dialog.rs`: `wire_tests`, `start_test`, `run_test`, `test_hydrus`, `fetch_tag_services`, `on_services_fetched`,
  `pick_service`. Each Test runs on the worker bridge (`workers.run_async`); its button is insensitive while the request is
  out, the label reads `Testing...`, and the window keeps handling actions.
- `settings_form.rs`: `Combo`, the editable service combo of the Hydrus page (the arrow is hidden until Fetch lists something).
- `dialogs.rs`: `Prompter::get_text` (`QInputDialog.getText`), scripted by `Answer::Text` in tests.
- `shortcuts_dialog.rs`: Review > Shortcuts, the read-only list of every review action and its effective key.
- `examples/shot.rs` and `ci.yml`: the 16 states of section 6, and `TAB_STATES` gains `settings-hydrus-fetched` and
  `shortcuts-dialog`.

**Not in this PR** (named owners): nothing of `gui/settings_dialog.py` or `gui/review_view.py` is left to a later ticket. The
nine rows `docs/phase6-coverage.md` marked OPEN are closed here and its section 5.1 is deleted; the Settings window's other
Phase 6 gaps (`docs/phase6-coverage.md` section 5.2) are Phase 7's, unchanged.

## 2. Differences from Python

| Id | What differs | Why | Pinned by |
|---|---|---|---|
| **D-G** | Where Python prints `str(exc)` after a fixed lead-in (`Could not fetch it: `, `Failed: `, `Couldn't reach e621`, and the box `Could not fetch services: `), Rust prints the same lead-in and `user_message()`: a sentence with no address, query, certificate or OS text. | The Phase 6 data contract (DAN-2116 item 1, gate 2.8). `detail()` goes to the log. | `crates/hatate-gtk/tests/smoke.rs:8824` (e621), `:8972` (the three page Tests), `:9045` (Hydrus): none of `127.0.0.1`, `os error`, `?`, the key. The comparer (`cmp.py`, section 5) counts the class. |
| **D-K** | The HTTP library's own headers: Python's `requests` adds `Accept-Encoding: gzip, deflate, br, zstd` and `Connection: keep-alive` to every request, and `User-Agent` and `Accept` to Hydrus requests; Rust's client sends its own. | Phases 3 to 5 never compared them either (`docs/hydrus-client.md` compares "the access-key and `Content-Type` headers"; `crates/hatate-core/tests/corpus_net_replay.rs:72`: "`requests` adds `Accept`, `Accept-Encoding`, `Connection`, which are not compared"). Everything the app sets itself (`Authorization`, the descriptive `User-Agent` of e621 and the boorus, `Referer`, the cookie header, the Hydrus access key) is compared. | the comparer (`cmp.py`) sets aside exactly those four header names and counts what it set aside; the negative control (section 5) shows it still sees a changed `Cookie` header. **Brought to the approver as a class that rests on that precedent**, not on a ruling. |
| **D-H** | While a Test or Fetch is out its button is insensitive and the label reads `Testing...` (the e621 Test: `Testing as <user>...`, the cookie Tests: `Testing with N cookie(s): ...`); Python blocks the whole GUI thread (`QApplication.processEvents()` between steps). | Nothing blocks the main loop (Phase 6 acceptance). | `smoke.rs:9000` (the button is out), `:9002` (the main loop ticked at least 10 times in 500 ms while the request was out), `:9010` (the button comes back). |
| **D-I** | The service combo's drop-down arrow is its own Tab stop and is hidden until Fetch lists something; Python's `QComboBox` is one stop. | GTK has no editable combo widget; `Combo` is an entry with an arrow button. | section 6.4: `settings-hydrus-fetched` stop 5 has its indicator in both schemes. |
| **D-J** | `NetConfig::only_scripted_hosts` refuses a host that is not scripted. | A smoke run once reached one real site anonymously (`api.anime-pictures.net`, which the Anime-Pictures parser resolves and the first scenario had not scripted); the guard makes that fail loudly. It only restricts, it redirects nothing, and the shipped binary never sets it. | no assertion: `smoke.rs:8696` (`site_fx`) builds every Test fixture with it so that an unscripted host cannot be reached at all; it is a guard, not a behaviour Python has. |
| **D-L** | The Hydrus Test's answer wraps to the page (`Failed: Could not reach Hydrus at ... Is the client running with its API switched on?`). Python's `QLabel("")` (`gui/settings_dialog.py:1680`) does not wrap, so there the line runs off the right edge of a narrow dialog. The Shortcuts dialog's key column ends 8 px short of the list's right edge (R-2); a GTK-side gutter against the overlay scroll bar, with no Python counterpart. | The unwrapped line widened the page past the window at the default 940 x 660, so the entries and Fetch from Hydrus were clipped and the half of the sentence that says what to do was off screen (DAN-2464 R-1). The text itself is unchanged (it is in the transport differential); only the label's `wrap` flag differs, and it matches the six Site Logins answers (`max-width-chars` 60). The 8 px keeps the last glyph clear of the scroll bar (R-2). | `smoke.rs:9711` (the failed line wraps) and `:9694` (its width is within the view, at 940 x 660 and at 560 x 440), `:9680` (the whole page fits the view at 940 x 660); `:9717` (the key labels' 8 px). |

## 3. Method map (`gui/` at `76c85c4`)

| Python | Rust |
|---|---|
| `review_view.py:619` `_show_shortcuts_dialog` | `crates/hatate-gtk/src/shortcuts_dialog.rs:52` `ShortcutsDialog::build`, `:28` `rows`, `:178` `action_show_shortcuts`; the button is wired at `crates/hatate-gtk/src/review_ops.rs:317` |
| `review_view.py:579` `_resolved_shortcuts` | unchanged: 6.f/6.i's (`docs/gtk-6f.md`, `docs/gtk-6i.md`); the dialog reads the same bindings |
| `settings_dialog.py:1294` `_test_deviantart` | `crates/hatate-gtk/src/settings_tests.rs:173` `plan` (`Test::DeviantArt`), `:309` `run`, `:401` `read_page`, `:518` `failed_fetch`; `crates/hatate-gtk/src/settings_dialog.rs:464` `start_test` |
| `settings_dialog.py:1384` `_test_animepictures` | the same functions, `Test::AnimePictures` |
| `settings_dialog.py:1478` `_test_sankaku` | `settings_tests.rs` `plan` and `run` (`Job::Available`, `hatate_core::availability::check_url_available`); no URL check, as in Python |
| `settings_dialog.py:1685` `_test_e621` | `settings_tests.rs:352` `run_e621` |
| `settings_dialog.py:1763` `_test_pixiv` | the same functions as DeviantArt, `Test::Pixiv` |
| `settings_dialog.py:1882` `_test_hydrus` | `crates/hatate-gtk/src/settings_dialog.rs:566` `test_hydrus`, `settings_tests.rs:531` `hydrus_text` |
| `settings_dialog.py:1853` `_fetch_tag_services` | `crates/hatate-gtk/src/settings_dialog.rs:602` `fetch_tag_services`, `:635` `on_services_fetched`, `settings_tests.rs:543` `service_label` |
| `settings_dialog.py:1845` `_on_service_selected` | `crates/hatate-gtk/src/settings_dialog.rs:684` `pick_service` |
| `core/site_access.py:203` `cookie_paste_warnings` | `crates/hatate-core/src/net/site.rs:187` |

## 4. Python test map

An `ast` walk of `tests/test_*.py` at `76c85c4` (`worktrees/DAN-2448-tools/amap6h2.py`, adapted from `worktrees/DAN-2413-tools/amap.py`),
following setUp, the test's own class and its bases, module helpers and the names a test patches, for a reference to any of
`_test_deviantart`, `_test_animepictures`, `_test_sankaku`, `_test_e621`, `_test_pixiv`, `_test_hydrus`, `_fetch_tag_services`,
`_on_service_selected`, `_show_shortcuts_dialog`, the six `*_test_result` labels, `cookie_paste_warnings` or `getText`, found
**20 tests in 4 classes**, and `grep` found one more that names a Test button only in its docstring (`TestDeviantArtParser.test_the_test_deviantart_button_never_persists_a_rotation`).
The walk run over this file reports **0 unmapped** (`python3 -I amap6h2.py <oracle> docs/gtk-6h2.md`). No Python test touches
`_show_shortcuts_dialog`, `_fetch_tag_services`, `_on_service_selected` or `_test_hydrus` (`grep -rn` of the five names in `tests/` finds none), so those are held by the
differentials of section 5 and by the scenarios of section 7. Where a row names an assertion it is the line that makes the row's claim.

| Python test (`tests/` at `76c85c4`) | Rust |
|---|---|
| `test_gui_smoke.py:1140` `TestE621LoginTest.test_half_filled_credentials_are_refused_before_any_request` | `crates/hatate-gtk/tests/smoke.rs:8743` (key missing: `No API key entered. Both halves are needed`), `:8750` (user missing), `:8754` (nothing was sent) |
| `test_gui_smoke.py:1152` `TestE621LoginTest.test_it_sends_the_typed_credentials_not_the_saved_ones` | `crates/hatate-gtk/tests/smoke.rs:8867` (the profile saves `saved-user`/`saved-key`, `:8856` shows the box opened on them, the request carries `typed-user:typed-key`) |
| `test_gui_smoke.py:1163` `TestE621LoginTest.test_it_uses_the_apps_own_header_builder` | `crates/hatate-gtk/tests/smoke.rs:8784` (the app's own `USER_AGENT`), `:8788` (host `e621.net`) |
| `test_gui_smoke.py:1174` `TestE621LoginTest.test_accepted_credentials_are_reported_as_such` | `crates/hatate-gtk/tests/smoke.rs:8770` (`e621 accepted the credentials for synthetic-user. Posts under its global blacklist will be visible to searches.`) |
| `test_gui_smoke.py:1179` `TestE621LoginTest.test_a_401_is_reported_as_a_rejection_with_what_to_do` | `crates/hatate-gtk/tests/smoke.rs:8805` (the 401 case: `e621 rejected the username or API key (401).`) |
| `test_gui_smoke.py:1186` `TestE621LoginTest.test_a_network_error_is_not_reported_as_a_rejection` | `crates/hatate-gtk/tests/smoke.rs:8810` (`Couldn't reach e621. That is a network problem, not a verdict on the credentials.`), `:8824` (no address, key or header in it) |
| `test_gui_smoke.py:1196` `TestE621LoginTest.test_an_unexpected_status_is_neither_accept_nor_reject` | `crates/hatate-gtk/tests/smoke.rs:8805` (the 418 case: `neither an accept nor a rejection`) |
| `test_search_quality.py:854` `TestDeviantArtParser.test_the_test_deviantart_button_never_persists_a_rotation` | does not apply as a test: `settings_tests.rs:309` `run` is given the net layer and the booru session and no `Settings`, so a Test cannot write the saved cookies (structural; no assertion) |
| `test_search_quality.py:3389` `TestAnimePicturesCookiePaste.test_warnings_name_the_right_site_session_cookie` | `crates/hatate-core/src/net/site.rs:627` (names `anime_pictures_jwt`), `:628` (never `sankaku`) |
| `test_search_quality.py:3400` `TestAnimePicturesCookiePaste.test_sankaku_warnings_are_unchanged_by_default` | `crates/hatate-core/src/net/site.rs:590` (`_sankakuchannel_session` named in the advice) |
| `test_urls_and_availability.py:533` `TestCookiePaste.test_clean_paste_produces_no_warnings` | `crates/hatate-core/src/net/site.rs:600` |
| `test_urls_and_availability.py:537` `TestCookiePaste.test_truncated_value_is_flagged` | `crates/hatate-core/src/net/site.rs:595` |
| `test_urls_and_availability.py:544` `TestCookiePaste.test_missing_session_cookie_is_flagged` | `crates/hatate-core/src/net/site.rs:590` |
| `test_urls_and_availability.py:549` `TestCookiePaste.test_short_session_value_is_flagged` | `crates/hatate-core/src/net/site.rs:591` |
| `test_site_access.py:266` `TestCookiePasteWarnings.test_a_clean_paste_has_no_warnings` | `crates/hatate-core/src/net/site.rs:600` |
| `test_site_access.py:270` `TestCookiePasteWarnings.test_a_truncated_value_is_flagged` | `crates/hatate-core/src/net/site.rs:595` |
| `test_site_access.py:274` `TestCookiePasteWarnings.test_an_ascii_ellipsis_is_also_flagged` | `crates/hatate-core/src/net/site.rs:606` |
| `test_site_access.py:278` `TestCookiePasteWarnings.test_no_session_cookie_at_all_is_flagged` | `crates/hatate-core/src/net/site.rs:590` |
| `test_site_access.py:282` `TestCookiePasteWarnings.test_a_suspiciously_short_session_cookie_is_flagged` | `crates/hatate-core/src/net/site.rs:591` |
| `test_site_access.py:286` `TestCookiePasteWarnings.test_a_custom_session_name_is_used_in_the_advice` | `crates/hatate-core/src/net/site.rs:591` (`auth` is named, not the Sankaku cookie), `:627` |
| `test_site_access.py:294` `TestCookiePasteWarnings.test_a_string_with_no_parseable_cookies_has_no_warnings` | `crates/hatate-core/src/net/site.rs:589` (empty), `:616` (`not a cookie string at all`) |

## 5. The differentials

Python is the real `SettingsDialog` and `MainWindow` at `76c85c4`, offscreen (`QT_QPA_PLATFORM=offscreen`), temp `HOME`, run
inside `unshare -rn` so that it cannot reach a network, with `getaddrinfo` patched to the scripted address and
`QInputDialog.getText` and `QMessageBox` scripted. Rust is the window under Xvfb (never the board's display), the same
scenarios driven through the dialog's own controls by a test-only driver that is inert without its environment variables
(`smoke.rs:9227` `settings_transport_differential_driver`, `:9429` `shortcuts_dialog_differential_driver`). Both halves
talk to a scripted https server with a per-name leaf certificate. The harness is outside the repo
(`worktrees/DAN-2448-tools`: `py_driver.py`, `gen.py`, `cmp.py`, `py_shortcuts.py`, `gen_s.py`, `cmp_s.py`, `run_py.sh`,
`run_rs.sh`), the log is `worktrees/DAN-2448-differential.log`.

**Transport differential** (the six Tests and Fetch): each case compares the prompts the dialog asked, the boxes it showed,
the label's text, every request the server saw (method, target with query, every header, body) and the hosts contacted.
`gen.py` writes the 84-case matrix (no randomness: every Test over cancel, blank, wrong site, the corpus page `corpus/<site>/001`
with no cookie, good cookies, a bad cookie and a padded URL, and the 404, 403, 500, empty 200, malformed, aborted and bad-TLS
answers; e621 over six statuses, abort, bad TLS, spaces and a missing half; Hydrus Test and Fetch over their answers) and, for
`r*` seeds, 60 more cases from `random.Random(seed)`.

| Seed | Cases | Differing | Set aside (counted) |
|---|---|---|---|
| matrix-1 | 84 | 0 | D-G label 21, msgs 3; D-K accept-encoding 70, connection 70, hydrus user-agent 14 |
| r2 | 144 (84 + 60) | 0 | D-G label 35, msgs 8; D-K 111 / 111 / 25 |
| r3 | 144 (84 + 60) | 0 | D-G label 40, msgs 16; D-K 93 / 93 / 22 |

**Negative controls** (condition 2): the Rust cookie header swap-cased in the harness turned seed r2 into
`cases=144 differing=47` and the matrix into `cases=84 differing=8`: the comparer still sees a changed `Cookie` header although it sets D-K aside. Three apparent classes during the run were faults of the harness, not of the
port, and are fixed in it: `http.server` folds a leading `//` (Python's recorder now keeps the raw request line), Python's
per-host session pools keep-alive connections across scenarios (`net.close_all()` before each case), and the Hydrus id test of `cmp.py`.

**Review > Shortcuts dialog**: 49 cases per seed from `gen_s.py` (9 fixed cases: none, one override, unbound, an unknown id, a
duplicate key, a padded key, whitespace only, all unbound, all overridden; and 40 from `random.Random('s1')`, `'s2'`, `'s3'`:
a random subset of the 16 review actions with keys from a list of 18, including empty, padded and non-ASCII ones): the
overrides are saved, the dialog is opened from the Review header, and its title, subtitle, each row's two texts in order, the
buttons and the default button are compared.

| Seed | Cases | Differing |
|---|---|---|
| s1 | 49 | 0 |
| s2 | 49 | 0 |
| s3 | 49 | 0 |

Negative control: one perturbed key text turned seed s1 into `cases=49 differing=1`.

**Files that cannot be opened and transitions after the error:** the Tests read no file. The state after an error is `smoke.rs:9500`
(e621: 503, then unreachable, then a re-run that succeeds; a fresh dialog starts empty; the saved label is not carried) and
`smoke.rs:9567` (Hydrus Test and Fetch after a failure re-run clean, keep the list and start empty on a fresh dialog).

## 6. Minimum size, shots, contrast, focus and Tab order

Hosted `ci` run 41098031844 at `bcf3462` (GTK 4.14 / libadwaita 1.5), artifact `shots`, downloaded to
`worktrees/DAN-2448-ci-shots/`: every number below is read from it, not from the author's host (GTK 4.24). The commits after
`bcf3462` change tests, docs and `TAB_STATES` and write no PNG that is measured here; the final head's run is cited in the PR comment.

### 6.1 States and PNG headers (design acceptance 1 and 4)

Read from the PNG header of every file (`worktrees/DAN-2448-states.md`, the output of a `struct.unpack` over bytes 16 to 24).
The stated minimum of the Settings window is 560 x 440 (`settings_dialog::MIN_WIDTH/MIN_HEIGHT`) and of the Shortcuts dialog 400 x 360. **No minimum-size shot differs from its stated minimum (0 of 26).**

| State | Shots (dark and light) | Default size, header | Minimum shots, header | Stated min | Fit |
|---|---|---|---|---|---|
| Tests, running | `settings-tests-running-{dark,light}.png` | 940x660 | `settings-tests-running-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Tests, success | `settings-tests-ok-{dark,light}.png` | 940x660 | `settings-tests-ok-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Tests, error | `settings-tests-error-{dark,light}.png` | 940x660 | `settings-tests-error-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Tests on the page's low rows, running | `settings-tests-running-low-{dark,light}.png` | 940x660 | `settings-tests-running-low-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| the same, success | `settings-tests-ok-low-{dark,light}.png` | 940x660 | `settings-tests-ok-low-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| the same, error | `settings-tests-error-low-{dark,light}.png` | 940x660 | `settings-tests-error-low-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Hydrus Test, running | `settings-hydrus-running-{dark,light}.png` | 940x660 | `settings-hydrus-running-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Hydrus Test, connected | `settings-hydrus-ok-{dark,light}.png` | 940x660 | `settings-hydrus-ok-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Hydrus Test, failed | `settings-hydrus-failed-{dark,light}.png` | 940x660 | `settings-hydrus-failed-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Hydrus service list after Fetch | `settings-hydrus-fetched-{dark,light}.png` | 940x660 | `settings-hydrus-fetched-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| the list open | `settings-hydrus-fetched-open-{dark,light}.png` | 940x660 | `settings-hydrus-fetched-open-min-{dark,light}.png` 560x440 | 560x440 | FIT |
| Shortcuts dialog, no overrides | `shortcuts-dialog-{dark,light}.png` | 400x520 | `shortcuts-dialog-min-{dark,light}.png` 400x360 | 400x360 | FIT |
| Shortcuts dialog, with overrides | `shortcuts-dialog-overrides-{dark,light}.png` | 400x520 | `shortcuts-dialog-overrides-min-{dark,light}.png` 400x360 | 400x360 | FIT |
| Focus: Test button | `settings-focus-test-{dark,light}.png` | 940x660 | (focus shot) | | |
| Focus: Fetch from Hydrus | `settings-focus-fetch-{dark,light}.png` | 940x660 | (focus shot) | | |
| Focus: Close | `shortcuts-focus-close-{dark,light}.png` | 400x520 | (focus shot) | | |

**The fit test on the shot path, red before the fix.** The Shortcuts dialog opened at its minimum on the shot path read **444 x 360 against the stated 400 x 360** (`shortcuts-dialog-overrides-min-{dark,light}.png`, host header, `worktrees/DAN-2448-differential.log` last line): the subtitle did not wrap. `shortcuts_dialog.rs` now wraps it (`4a6f474`) and the hosted header reads 400 x 360. The Hydrus service list was also clipping long names in its popover until the combo propagated its natural width (same commit). The fit test is the PNG header of the `-min-` shots, which `shot --all` writes into `index.tsv` next to the size the surface states (`fit` column: 128 `FIT`, 0 `NOFIT` over the whole set).

### 6.2 Contrast (design acceptance 2 and 3)

`python3 -I worktrees/DAN-2448-tools/contrast.py worktrees/DAN-2448-ci-new` (the extreme-pixel method of DAN-2218: per label box from the shot's `.rects`, the surface is the most common colour in the box and the foreground the pixel farthest from it; every label of the widget tree is a row, insensitive ones included), over the 58 PNGs of the 16 states, output `worktrees/DAN-2448-contrast.md`: **1132 label rows, 0 below 4.5:1 outside the exempt, 30 disabled rows marked exempt.**

| Scheme | Rows | Lowest passing row | Ratio |
|---|---|---|---|
| dark | 551 passed | the key labels `H` and `]` of the Shortcuts dialog, `#cdc4ba`-family ink on `#0d0d0d`-family (`shortcuts-dialog-dark`) | 4.77:1 |
| light | 551 passed | the same labels (`shortcuts-dialog-light`) | 4.80:1 |

The 30 disabled rows are the Test and Fetch buttons while their request is out (`settings-tests-running-*`, `settings-hydrus-running-*`, and the `-min-` and `-low-` shots): in dark `#6d6964` on `#161615` 3.32:1 at the lowest (`#7c7771` 4.08 in the minimum shots), in light `#857f75` on `#ded7c8` 2.77:1 (3.88 in the minimum shots). A disabled control is exempt (WCAG 2.x), and the labels that show the running state (`Testing...`, `Testing as ...`, `Testing with N cookie(s): ...`) are not disabled and pass (11.29:1 in dark, `#cdc4ba` on `#0d0d0d`). The result labels of the success and error states pass at the same ratios as page text.

### 6.3 Focus rings (design acceptance 1)

`worktrees/DAN-2448-tools/focusbox.py` (the ring inside the control's own box: the diff of the focused shot against the same page unfocused, cropped to the control's label box grown by 12 px) and `focusdiff2.py` (the diff against the unfocused page, for the Close button), output `worktrees/DAN-2448-focus.log`:

| Control | Shot | Ring | Against | Ratio (dark / light) |
|---|---|---|---|---|
| Test e621 login... | `settings-focus-test-{dark,light}.png` against `settings-6` | `#cdc4ba` / `#211d17` | the button's surface `#201f1e` / `#d4cdbf` | 9.56 / 10.60 |
| Fetch from Hydrus... | `settings-focus-fetch-{dark,light}.png` against `settings-4` | `#cdc4ba` / `#211d17` | the button's surface `#201f1e` / `#d4cdbf` | 9.56 / 10.60 |
| Close (the default button) | `shortcuts-focus-close-{dark,light}.png` against `shortcuts-dialog` | `#0d0d0d` / `#f6f1e6` inside the pill | the pill's fill `#cdc4ba` / `#211d17` | 11.29 / 14.88 |

All three clear the 3:1 non-text floor in both schemes.

### 6.4 Tab order (design acceptance 7)

`shot --tabs OUTDIR` is not part of hosted `ci` (`ci.yml` runs `shot --all`), so these are **host walks** (GTK 4.24) at the head of this PR, `worktrees/DAN-2448/tabs/tabs.tsv` and its crops `worktrees/DAN-2448/tabs/<state>-<scheme>-tab<n>.png`: 552 stops over the 13 surfaces walked, **0 without a visible indicator, 0 walks failed**; the verifier's walk at the head is the one that counts. The same walk, over every Settings page, shows what this PR adds to the stops: the Site Logins page (`settings-6`) goes from 10 to 15 stops (the five Test buttons are no longer insensitive) and the Hydrus page (`settings-4`) from 11 to 13 (Test connection, Fetch from Hydrus); every other page is unchanged.
Full rows are in `docs/phase6-coverage.md` section 8 (713 stops over 34 surfaces).

**Review Shortcuts dialog** (`shortcuts-dialog`)

| Stop | Widget | Changed px (dark / light) | Shot that shows the indicator |
|---|---|---|---|
| 0 | GtkScrolledWindow [] Next image: | 2848 / 2848 | `shortcuts-dialog-dark-tab0.png`, `shortcuts-dialog-light-tab0.png` |
| 1 | GtkButton [text-button.pill.primary.plain.default] Close | 496 / 496 | `shortcuts-dialog-dark-tab1.png`, `shortcuts-dialog-light-tab1.png` |

**Settings, Hydrus page after Fetch (the list open on the service combo)** (`settings-hydrus-fetched`)

| Stop | Widget | Changed px (dark / light) | Shot that shows the indicator |
|---|---|---|---|
| 0 | GtkText [] a setting's name - "quota", "cookies", ... | 3384 / 3384 | `settings-hydrus-fetched-dark-tab0.png`, `settings-hydrus-fetched-light-tab0.png` |
| 1 | GtkNotebook [frame] General | 6180 / 6180 | `settings-hydrus-fetched-dark-tab1.png`, `settings-hydrus-fetched-light-tab1.png` |
| 2 | GtkText []  | 5737 / 5728 | `settings-hydrus-fetched-dark-tab2.png`, `settings-hydrus-fetched-light-tab2.png` |
| 3 | GtkText []  | 2796 / 2796 | `settings-hydrus-fetched-dark-tab3.png`, `settings-hydrus-fetched-light-tab3.png` |
| 4 | GtkText [] empty = "my tags" local service | 1864 / 1864 | `settings-hydrus-fetched-dark-tab4.png`, `settings-hydrus-fetched-light-tab4.png` |
| 5 | GtkToggleButton [image-button.toggle]  | 376 / 376 | `settings-hydrus-fetched-dark-tab5.png`, `settings-hydrus-fetched-light-tab5.png` |
| 6 | GtkButton [text-button] Fetch from Hydrus… | 928 / 928 | `settings-hydrus-fetched-dark-tab6.png`, `settings-hydrus-fetched-light-tab6.png` |
| 7 | GtkCheckButton [text-button] Verify SSL certificate | 3800 / 3800 | `settings-hydrus-fetched-dark-tab7.png`, `settings-hydrus-fetched-light-tab7.png` |
| 8 | GtkCheckButton [text-button] When "Download match ... | 3800 / 3800 | `settings-hydrus-fetched-dark-tab8.png`, `settings-hydrus-fetched-light-tab8.png` |
| 9 | GtkCheckButton [text-button] Everything this app k... | 3800 / 3800 | `settings-hydrus-fetched-dark-tab9.png`, `settings-hydrus-fetched-light-tab9.png` |
| 10 | GtkText [] empty = "hatate: match source" | 2796 / 2796 | `settings-hydrus-fetched-dark-tab10.png`, `settings-hydrus-fetched-light-tab10.png` |
| 11 | GtkButton [text-button] Test connection | 812 / 812 | `settings-hydrus-fetched-dark-tab11.png`, `settings-hydrus-fetched-light-tab11.png` |
| 12 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-hydrus-fetched-dark-tab12.png`, `settings-hydrus-fetched-light-tab12.png` |
| 13 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-hydrus-fetched-dark-tab13.png`, `settings-hydrus-fetched-light-tab13.png` |

**Settings, Site Logins page (five Test buttons, now sensitive)** (`settings-6`)

| Stop | Widget | Changed px (dark / light) | Shot that shows the indicator |
|---|---|---|---|
| 0 | GtkText [] a setting's name - "quota", "cookies", ... | 3384 / 3384 | `settings-6-dark-tab0.png`, `settings-6-light-tab0.png` |
| 1 | GtkNotebook [frame] General | 6300 / 6300 | `settings-6-dark-tab1.png`, `settings-6-light-tab1.png` |
| 2 | GtkText [] PHPSESSID value from a logged-in browser session | 2604 / 2604 | `settings-6-dark-tab2.png`, `settings-6-light-tab2.png` |
| 3 | GtkButton [text-button] Runs the real parser again... | 652 / 652 | `settings-6-dark-tab3.png`, `settings-6-light-tab3.png` |
| 4 | GtkText [] your e621 username | 2604 / 2604 | `settings-6-dark-tab4.png`, `settings-6-light-tab4.png` |
| 5 | GtkText [] from Account > Manage API Access on e621 | 2604 / 2604 | `settings-6-dark-tab5.png`, `settings-6-light-tab5.png` |
| 6 | GtkButton [text-button] Sends one authenticated re... | 812 / 812 | `settings-6-dark-tab6.png`, `settings-6-light-tab6.png` |
| 7 | GtkText [] _sankakuchannel_session=...; locale=en | 2604 / 2604 | `settings-6-dark-tab7.png`, `settings-6-light-tab7.png` |
| 8 | GtkButton [text-button] Fetches a Sankaku post URL... | 764 / 764 | `settings-6-dark-tab8.png`, `settings-6-light-tab8.png` |
| 9 | GtkText [] anime_pictures_jwt=eyJ0eXAiOiJKV1Qi… | 2604 / 2604 | `settings-6-dark-tab9.png`, `settings-6-light-tab9.png` |
| 10 | GtkButton [text-button] Fetches a post through the... | 964 / 964 | `settings-6-dark-tab10.png`, `settings-6-light-tab10.png` |
| 11 | GtkText [] auth=...; auth_secure=...; userinfo=... | 4721 / 10340 | `settings-6-dark-tab11.png`, `settings-6-light-tab11.png` |
| 12 | GtkButton [text-button] Fetches a deviation throug... | 5351 / 5359 | `settings-6-dark-tab12.png`, `settings-6-light-tab12.png` |
| 13 | GtkButton [text-button.pill.plain] Cancel | 532 / 532 | `settings-6-dark-tab13.png`, `settings-6-light-tab13.png` |
| 14 | GtkButton [text-button.pill.primary.plain.default] OK | 416 / 416 | `settings-6-dark-tab14.png`, `settings-6-light-tab14.png` |

In the Site Logins table the Test buttons are stops 3 (Pixiv), 6 (e621), 8 (Sankaku), 10 (Anime-Pictures) and 12 (DeviantArt), listed by their tooltips. The service combo's arrow (stop 5 of the Hydrus table) is D-I.

### 6.5 Not judgeable from shots

Window chrome, the GPU renderer, the real compositor's backdrop (inactive selection), HiDPI and the hover wash are not
judgeable from these shots; owner: the design read (Oderisi) for the first four and the board for a real-display look.

## 7. Scenarios

`crates/hatate-gtk/tests/smoke.rs`, a second list `SCENARIOS_6H2` at the end so that nothing earlier moved:
`settings_e621_test_sends_python_request_and_says_what_python_says` (`:8727`), `settings_e621_test_sends_the_typed_credentials_not_the_saved_ones` (`:8844`),
`settings_page_tests_ask_first_and_report_gone_failed_and_empty_pages` (`:8878`), `settings_tests_wait_on_a_stalled_server_without_blocking_the_window` (`:8984`),
`settings_hydrus_test_and_fetch_follow_python` (`:9023`), `review_shortcuts_dialog_lists_every_action_with_its_key` (`:9084`),
`settings_e621_test_after_an_error_reruns_clean_and_keeps_its_place` (`:9500`), `settings_hydrus_test_and_fetch_after_an_error_rerun_clean` (`:9567`) and the two drivers of section 5.
Unit tests: `crates/hatate-gtk/src/settings_tests.rs` (`a_url_on_the_wrong_site_stops_before_any_request`, `typed_cookies_are_named_and_checked_before_the_request`,
`a_restricted_page_and_an_empty_pixiv_page_say_what_python_says`, `e621_needs_both_halves_and_trims_them`, `a_service_is_listed_by_name_and_the_first_twelve_characters_of_its_key`) and
`crates/hatate-core/src/net/site.rs` `cookie_paste_warnings_say_what_python_says`, `a_string_with_no_cookie_in_it_has_no_warnings`, `the_advice_names_the_site_it_is_for`.

**The Review Shortcuts dialog** (`smoke.rs:9084`): the button is sensitive with Python's tooltip (`:9089`, `:9090`), the title is `Review Keyboard Shortcuts` (`:9102`),
each row's `label:` and its key (`:9109`, `:9110`), `(unbound)` for none (`:9152`), the one button is Close and it is the default (`:9119`, `:9120`),
and a binding changed in Settings shows on the next opening (`:9151`).

## 8. Revert and red

Every assertion this doc cites as pinning a row was made to fail by editing the shipped line it checks, the named test was run and went red on a test failure (not on a compile error), and the line was restored (`worktrees/DAN-2448-tools/revert.py`, log `worktrees/DAN-2448-revert.log`: each mutation, the verdict and the first failing assertion). **34 of 34 went red.** Two mutations of the first pass did not, and are the findings of the exercise: `smoke.rs:9002` (a 600 ms sleep on the main thread before the request was posted did not lower the tick count, because the timer catches up; a sleep queued on the loop during the pump did, and is the row below) and `smoke.rs:9152` (the test compared the key text with `sd::UNBOUND`, the very constant under test, so changing it stayed green; the assertion now compares with the literal `(unbound)`). `smoke.rs:8754` (nothing was sent for a half-filled pair) is in the same scenario as `:8743` and only fails after it; no single edit turns it red without also failing `:8743`, so it is not separately pinned.

| Assertion (the line the row cites) | Source edited | Mutation | Test |
|---|---|---|---|
| smoke.rs:8743 (e621 half-filled text) | `settings_tests.rs` | `"No {missing} entered. Both halves` to `"Missing {missing}. Both halves` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8867 (typed credentials in the Basic header) | `site.rs` | `base64(format!("{user}:{key}").as_bytes())` to `base64(format!("{key}:{user}").as_bytes())` | `settings_e621_test_sends_the_typed_credentials_not_the_saved_ones` |
| smoke.rs:8784 (the app's own User-Agent) | `site.rs` | `header(&mut h, USER_AGENT_HEADER, USER_AGENT);` to `header(&mut h, USER_AGENT_HEADER, BROWSER_USER_AGENT);` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8770 (accepted text) | `settings_tests.rs` | `"e621 accepted the credentials for {user}.` to `"e621 accepted the login for {user}.` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8805 (401 text) | `settings_tests.rs` | `"e621 rejected the username or API key (401).` to `"e621 refused the username or API key (401).` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8805 (418 text) | `settings_tests.rs` | `which is neither an accept nor a rejection.` to `which is not a verdict.` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8810 (network error text) | `settings_tests.rs` | `"Couldn't reach e621. That is a network problem` to `"Could not reach e621. That is a network problem` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8824 (no OS text in the network error) | `settings_tests.rs` | `See Help > View Logs for the specific reason.".to_owned()` to `See Help > View Logs for the specific reason. os error".to_o` | `settings_e621_test_sends_python_request_and_says_what_python_says` |
| smoke.rs:8944 (the DeviantArt prompt preset) | `settings_tests.rs` | `"https://www.deviantart.com/",` to `"https://www.deviantart.com/x",` | `settings_page_tests_ask_first_and_report_gone_failed_and_empty_pages` |
| smoke.rs:8962 (404 gone text) | `settings_tests.rs` | `"That deviation is gone (HTTP 404)` to `"That deviation is missing (HTTP 404)` | `settings_page_tests_ask_first_and_report_gone_failed_and_empty_pages` |
| smoke.rs:8972 (no HTTP status in the page error) | `settings_tests.rs` | `"Could not fetch it: the site did not answer as expected.` to `"HTTP 500 Could not fetch it: the site did not answer as exp` | `settings_page_tests_ask_first_and_report_gone_failed_and_empty_pages` |
| smoke.rs:9000 (button out while the request is out) | `settings_dialog.rs` | `if let Some(b) = &button {             b.set_sensitive(false` to `let engine = backend.engine.clone();` | `settings_tests_wait_on_a_stalled_server_without_blocking_the_window` |
| smoke.rs:9002 (main loop keeps ticking) | `settings_dialog.rs` | `let engine = backend.engine.clone();         let handle = se` to `let engine = backend.engine.clone();         gtk::glib::idle` | `settings_tests_wait_on_a_stalled_server_without_blocking_the_window` |
| smoke.rs:9043 (Hydrus Failed prefix) | `settings_tests.rs` | `format!("Failed: {}", e.user_message())` to `format!("Error: {}", e.user_message())` | `settings_hydrus_test_and_fetch_follow_python` |
| smoke.rs:9057 (Connected mark) | `settings_tests.rs` | `Ok(true) => "Connected \u{2713}".into()` to `Ok(true) => "Connected".into()` | `settings_hydrus_test_and_fetch_follow_python` |
| smoke.rs:9069 (service label) | `settings_tests.rs` | `format!("{name}  ({head}\u{2026})")` to `format!("{name} ({head})")` | `settings_hydrus_test_and_fetch_follow_python` |
| smoke.rs:9102 (dialog title) | `shortcuts_dialog.rs` | `pub const TITLE: &str = "Review Keyboard Shortcuts";` to `pub const TITLE: &str = "Keyboard Shortcuts";` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9110 (each row's key text) | `shortcuts_dialog.rs` | `(format!("{}:", a.label), shown.to_owned())` to `(format!("{}:", a.label), shown.to_uppercase())` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9120 (Close is the default) | `shortcuts_dialog.rs` | `window.set_default_widget(Some(&close_button));` to `(line removed)` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9152 (unbound) | `shortcuts_dialog.rs` | `pub const UNBOUND: &str = "(unbound)";` to `pub const UNBOUND: &str = "unbound";` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| site.rs:595 (truncated flag) | `site.rs` | `let truncated = \|value: &str\| TRUNCATION_HINTS.iter().any(\|h` to `let truncated = \|_value: &str\| false;` | `cookie_paste_warnings_say` |
| site.rs:590 (no session cookie) | `site.rs` | `if session.is_empty() {         warnings.push(format!(      ` to `if false {         warnings.push(format!(             "No se` | `cookie_paste_warnings_say` |
| site.rs:591 (short session) | `site.rs` | `if len < SUSPICIOUSLY_SHORT_SESSION_CHARS && !truncated(valu` to `if false && len < SUSPICIOUSLY_SHORT_SESSION_CHARS && !trunc` | `cookie_paste_warnings_say` |
| site.rs:600 (a long session value is clean) | `site.rs` | `if len < SUSPICIOUSLY_SHORT_SESSION_CHARS && !truncated(valu` to `if len < 1000 && !truncated(value) {` | `cookie_paste_warnings_say` |
| site.rs:616 (no cookie, no warning) | `site.rs` | `if cookies.is_empty() {         return warnings;     }     l` to `let truncated` | `a_string_with_no_cookie` |
| site.rs:627 (the advice names the site) | `site.rs` | `{session_name}; without it` to `_sankakuchannel_session; without it` | `the_advice_names` |
| settings_tests.rs:637 (DA blurred text) | `settings_tests.rs` | `but the picture is still the BLURRED logged-out copy.` to `but the picture is still the blurred copy.` | `a_restricted_page` |
| settings_tests.rs:645 (AP restricted text) | `settings_tests.rs` | `"Still account-only with these cookies. {}"` to `"Still account-only. {}"` | `a_restricted_page` |
| settings_tests.rs:654 (Pixiv empty page text) | `settings_tests.rs` | `"Page fetched, but no data could be read from it. If this ar` to `"Page fetched, but nothing could be read. If this artwork re` | `a_restricted_page` |
| smoke.rs:9089 (the Shortcuts button is sensitive) | `review_view.rs` | `widgets::icon_button("\u{2328}", "Show all review keyboard s` to `widgets::icon_button("\u{2328}", "Show all review keyboard s` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9090 (Python's tooltip) | `review_view.rs` | `"Show all review keyboard shortcuts."` to `"The shortcuts list arrives in a later step."` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9119 (the one button is Close) | `shortcuts_dialog.rs` | `pub const CLOSE: &str = "Close";` to `pub const CLOSE: &str = "Done";` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9151 (an override shows) | `shortcuts_dialog.rs` | `.find(\|(k, _)\| k == a.id)` to `.find(\|(k, _)\| k == "nope")` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9109 (each row's label has its colon) | `shortcuts_dialog.rs` | `(format!("{}:", a.label), shown.to_owned())` to `(format!("{}", a.label), shown.to_owned())` | `review_shortcuts_dialog_lists_every_action_with_its_key` |
| smoke.rs:9711, :9680 (the failed line wraps, the page fits at 940 x 660) | `settings_spec.rs` | the `hydrus_test_result` `Node::Live` `wrap: true` to `wrap: false` | `settings_hydrus_failed_status_wraps_to_the_page_at_both_window_sizes` (red: `940x660: the page is 944 px wide in a 894 px view`) |
| smoke.rs:9736 (the key labels' 8 px) | `shortcuts_dialog.rs` | `key_label.set_margin_end(8);` removed | `shortcuts_dialog_key_column_keeps_a_gutter_against_the_scroll_bar` (red: `"J": 0 px between the key and the bar`) |
