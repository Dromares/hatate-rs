# The Pawchive index (Phase 4.f, DAN-2128)

What this ticket ports, where the cut is, how it was checked against the real Python, what is not
matched (with counts), and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net layer (`docs/net.md`), the Phase 3
image modules (`docs/image-compare.md`), the clock (`clock::Clock`) and the shared error kind
(`error::ErrorKind`, 4.a) are reused as they are; the small changes to them are in section 2.

Premise (DAN-1547): Pawchive is the site where 884 of 884 originals answered 404. This ticket makes
**no live request** in code, tests, CI or verification. Every Pawchive answer is scripted on a local
server, and the scripted answers include a 404 original and a creator with zero posts.

## 1. The cut

| Python | Rust |
|---|---|
| `core/pawchive_index.py`: `Creator`, `IndexMatch`, `CrawlResult`, `similarity_for`, `fingerprints`, `creator_from_url`, `PawchiveIndex` (`creators`, `add_creator`, `remove_creator`, `known_pages`, `add_images`, `finish_creator`, `image_count`, `is_empty`, `search`), `find_creators`, `_Fetcher`, `index_creator` | `pawchive_index::{Creator, IndexMatch, CrawlResult, similarity_for, fingerprints, creator_from_url, PawchiveIndex, PawchiveCrawler::{find_creators, index_creator}}` (`_Fetcher` is `PawchiveCrawler`'s private `polite_get` and `pause`) |
| `workers/pawchive_index_worker.py`, the `run()` bodies without Qt | `pawchive_index::run_index_jobs`, `find_creators_for_ui` |
| `core/tag_rules.py::apply_inline_edit` and `InlineEditResult`, the last unported `tag_rules` function | `tag_rules::{apply_inline_edit, InlineEditResult, InlineEditAction}` |
| (Phase 6 data contract, DAN-2116 item 1) | `pawchive_index::PawchiveError` (`kind`, `message`, `user_message`, `detail`); `ErrorKind::{Database, NotFound}` |

Not here, and who owns it:

- **Wiring the index into the engine runner.** `engine_backends::LiveEngines::pawchive_index` still
  answers `IndexAnswer::Empty`, as in Phase 3. Turning it into `is_empty()`, `fingerprints(path)` and
  `search(hash16, hydrus_hash)` over the user's `pawchive_index.db` changes what `hatate batch` does
  with that file, so it is a separate, small change (proposed to Cloud in the submission comment), not
  something this port does quietly.
- The Qt classes themselves (`PawchiveIndexWorker`, `PawchiveFindWorker`) and the Index window:
  Phase 6.

## 2. How it is built

- **The database.** `PawchiveIndex` opens SQLite at the path it is given (`paths.pawchive_index_db()`
  in the app) with Python's schema text, executed on every call as Python's `_connect` does, and a
  transaction that commits when the call succeeds. Python's own `PawchiveIndex` opens a file written
  here and the reverse (section 3, "file differential"). The row cache is process-wide-invalidated by
  every write, as Python's `_generation` is (a write through any `PawchiveIndex` drops every
  instance's cache).
- **The crawl.** Python crawls with a plain `requests.Session` of its own, not `core/net.py`: no host
  wait and no net-layer 429 retry. `PawchiveCrawler` does the same through a `NetClient` of its own
  (`RetryMode::Never`, no host gate) and does its own pausing: one request a second, 429 waited out
  (`Retry-After`, or 30 s), four tries on a transport error, Stop checked every 0.2 s. The pauses go
  through a `Clock`, so a test (and the differential) compares the sleeps Python asked for. Not
  `RetryMode::Polite`: that Phase 1 mode caps waits and has no Stop or clock hook. The hosts are
  injected (`PawchiveCrawler::with_hosts`); no setting, flag or environment variable redirects them.
  `with_timeouts` shortens the 30 s and 120 s request timeouts for a test; the binaries do not call it.
- **Request spelling.** `User-Agent` is the app's, the listing offset is the `o` parameter placed
  ahead of a `#fragment` as `requests` does, and the URL is quoted with `requote_uri`.
- **Answers.** Python reads every answer as untyped JSON. A body that is not JSON is Python's
  `JSONDecodeError`, a `ValueError` the crawl catches: `CrawlResult.error` carries CPython's exact
  words and position (`Expecting value: line 1 column 1 (char 0)`, `pawchive_index/json_error.rs`).
  A body that is JSON but the wrong shape raises an `AttributeError` or `TypeError` in Python that
  its `except` does not catch; that is `Err(PawchiveError)` of kind `UnexpectedResponse` here with
  Python's text. A `title` that is a number or a boolean is stored as text, as the TEXT column does;
  a list or an object fails the whole batch of 25 rows, as `sqlite3` binding does.
- **Errors (DAN-2116 item 1).** `PawchiveError::kind()` is the shared `error::ErrorKind`
  (`Net(net::ErrorKind)` wraps the net layer's kind). `message()` is Python's `str(exc)`, the text
  `CrawlResult.error` carries. `user_message()` says what failed and what to do, with no URL, query,
  path, access key or OS text; `detail()` is the raw text for a log. `CrawlResult.error` stays one
  string, with the kind beside it (`error_kind`, `error_user_message()`); nothing is persisted.
- **Changes to earlier modules.** `error.rs`: two variants. `hydrus_client.rs`: the `user_message`
  match gets an arm for them, and `require_host` and `type_name` become `pub(crate)`.
  `engines/pawchive_lookup.rs`: `files_of` becomes `pub(crate)`. `image_compare.rs`: `Hash::from_hex`,
  `Hash::xor_popcount` and `dhash_pair` (one decode, both hashes). `pyresponse.rs`: `json_text`
  (the text `json()` parses, so the error position can be found). No behaviour changes.

## 3. The differentials

The real Python at `76c85c4` (`core/`, `workers/` and `tests/` exported with `git archive`;
CPython 3.14.8, `requests` 2.34.2, Pillow 12.3.0, the repo's venv) ran with two layers replaced:
`requests.adapters.HTTPAdapter.send` (scripted answers; redirects, `Response.json()`, `raise_for_status`,
and the `requests` exceptions are the real code) and `pawchive_index.time` (a clock that never waits
and records every sleep). The same scripted answers were served, in request order, to the Rust code by
a local https server (a throw-away CA), through `PawchiveCrawler` on `NetClient::new` (rustls with the
explicit `CryptoProvider`), with a `ManualClock`. Compared per case: every op's return value or the
error class and message; every request (method, path with query, `User-Agent`); Python's `timeout=`
(30.0, 120.0 for the creator list) and host; every sleep and the final clock readings; every progress
callback; every row of both tables of the index database in rowid order (timestamps included) and
`sqlite_master`; the creator cache file's bytes; and the `search` results.

The harness is outside the repo (the tree is Rust only). Its log is
`worktrees/DAN-2128-differential.log`.

| Seed | Cases | Ops | Requests | Sleeps | Searches | DB rows | Progress lines |
|---|---|---|---|---|---|---|---|
| 1 | 2,000 | 8,343 | 19,362 | 62,645 | 4,513 | 6,806 | 7,749 |
| 2 | 2,000 | 7,816 | 20,457 | 60,773 | 4,118 | 6,090 | 7,867 |
| 3 | 2,000 | 7,956 | 21,174 | 60,614 | 4,407 | 6,770 | 8,091 |
| **Total** | **6,000** | **24,115** | **60,993** | **184,032** | **13,038** | **19,666** | **23,707** |

(Counts exclude the 24 cases of class C2 and the 1 of class C4 below, which are set aside.)

Ops over the three seeds (generated, before the exclusions): `crawl` 4,583, `add_images` 5,131,
`find` 1,961, `add_creator` 1,926, `creators` 2,612, `search` 1,732, `known_pages` 1,218,
`image_count` 1,213, `is_empty` 1,211, `fingerprint` 721, `url` 688, `inline_edit` 580,
`finish_creator` 237, `remove_creator` 231, `similarity` 116 (every distance from 0 to 130).

The crawls cover: 0 to 120 posts (page boundaries at 49, 50, 51, 100 and 101 posts, with the final
empty page both answered `[]` and left to 404), 0 to 3 files per post, image and non-image
extensions, ids and titles of every JSON type, thumbnails of PNG (RGB, RGBA, L, palette), JPEG
(colour, grey, 4:4:4), GIF, BMP, WebP, truncated, empty and not-an-image bodies, **404 originals**
(and 403, 410, 500, 502, 503), **creators with zero posts** (194 crawls) and unknown artists (82),
429 with `Retry-After` of every spelling and 429 six times in a row, redirects (301, 302, a loop that
exceeds 30), connection failures and timeouts (retried four times, then reported or raised),
profiles of every JSON shape and every kind of malformed JSON (mutated by truncation, deletion,
insertion, duplication and swapping), listing answers of every shape, Stop after 0 to 40 polls of the
stop callback (197 crawls stopped), refreshes over an index that already knows some pages, and
service and user ids with spaces, `%`, `+`, `;`, `&`, `=`, `'` and non-ASCII characters. `find` covers
the cache absent, fresh, one second short of a week, stale, corrupt, `null`, not UTF-8, a list of
non-objects, an object, a number and a string; the remote list of every shape and 20 status codes;
queries that are empty, blank, upper case, non-ASCII or substrings; `favorited` of every JSON type.

**File differential.** For every case that wrote an index, each app's file was opened by the other
app's loader and searched with the same queries (built from the file's own rows: 0 to 33 flipped
bits, with and without a SHA-256, upper-case hashes, limits and distances): **4,207 index files and
68,580 searches, each answered four ways (Python loader and Rust loader, Python-written file and
Rust-written file): 0 differing.**

**Result: 0 unexplained differences in 6,000 cases.** Everything else is one of the classes below.

Matched on the way, each one a difference the first runs showed: CPython's JSON error words and
positions (C scanner rules, including the 3.13 trailing-comma messages); the reason phrase in
`raise_for_status` (CPython's table, section 4 class C5); a `title` that is a number or boolean;
`sqlite3`'s refusal of a list or object (and the file Python's `_connect` leaves behind when the
statement after it fails); `Retry-After` parsed as `float()` parses it.

## 4. What is not matched, with counts

| Class | What differs | Count | Why it is kept |
|---|---|---|---|
| **C1** library words | a request that fails keeps its kind and its place in the flow (a connection failure, a timeout, 31 redirects); the message is the HTTP library's own words (`scripted connection failure` against `connection failed: error sending request: ...`). The cause class matches in every case | 859 messages (278 + 280 + 301) | the message is the library's; the class, and so what the caller does, matches. Same class as DAN-2126's C1 and DAN-2060's V1 |
| **C2** numeric profile name | a profile whose `name` is a number or a boolean: Python carries it along untyped (`CrawlResult.name` is an `int`, the column holds its text); `CrawlResult.name` is a string here, so this is `Err(UnexpectedResponse)` ("'name' is a int where a string is expected"). A list or an object, which `sqlite3` refuses, is matched exactly | 24 cases (10 + 8 + 6), set aside | Pawchive sends a string or nothing. A typed error is the safe reading |
| **C3** cookie jar | `requests.Session` keeps cookies a response sets and sends them back; the crawler sends none. Not in the compared headers (`User-Agent` only) | not generated | Pawchive's API is anonymous and sets none the crawl needs |
| **C4** JSON | `json.loads` accepts `NaN`, `Infinity`, `-Infinity`, lone surrogate escapes and integers past 64 bits, which `serde_json` reads as "not JSON" or as floats. A body using them is an error here with `serde_json`'s words | 1 case (set aside); the same class as DAN-2126's C4 and `docs/parsers.md` | the site does not send them |
| **C5** reason phrase | `raise_for_status` prints the reason of the server's status line. The net layer does not keep it, so CPython's standard phrase for the status stands in. The same text unless a server writes a different one | 0 differences in 6,000 cases (the scripted server sends CPython's phrases) | not observable through the net layer; the status number, the URL and the class are exact |
| **C6** negative `limit` | `find_creators(limit=-1)` and `search(limit=-1)` slice from the end in Python; the Rust `limit` is a `usize` | not generated | no caller passes one |
| **C7** very large body | the net layer caps one body at 512 MiB; Python streams without a limit. The creator list is ~15 MB, a thumbnail ~33 KB | not generated | as DAN-2126's C6 |
| **C8** decompression bomb | a thumbnail past Pillow's pixel limit raises `DecompressionBombError`, which escapes `index_creator` in Python; here `Err(UnexpectedResponse)` with the same text. The rows fingerprinted before it are kept in both | not generated (needs a 179-megapixel body) | the escape is the same, the type is the shared one |
| **C9** shortened timeouts in the harness | Python is given 30.0 and 120.0 and the log checks that it passes them; the Rust crawler under test is built with `with_timeouts(0.25, 0.25)` so a timeout case does not take 30 s | all timeout cases | `a_slow_site_is_a_timeout_error_after_the_requests_wait` covers the timeout path; the default values are the constants `30.0` and `120.0` in `crawl.rs` |

C1 and C5 change only the words of a failure that is the same failure. C2, C4 and C8 change the
outcome in a case Pawchive does not produce. C3, C6, C7 and C9 are not reachable by the differential.

## 5. The tests

`crates/hatate-core/tests/pawchive_index.rs` (43 tests) drives the index, the crawl, the artist
search and the worker bodies; the pure functions have unit tests in `tag_rules.rs`. Pawchive is a
local https server with a throw-away CA, reached through `NetClient::new` (the binaries'
constructor: rustls with the explicit `CryptoProvider`). Pawchive has no setting to turn certificate
checks off, so the two schemes the shipped code uses are https with the root trusted and https with
it refused.

| Rule | Test |
|---|---|
| network-client test over the shipped scheme: https, verification on, a self-signed server is refused | `a_self_signed_site_is_refused_by_the_shipped_client` |
| https with a trusted root: the crawl, the listing, the thumbnails, the creator list | every `world()` test, e.g. `every_page_of_posts_and_every_image_is_indexed` |
| one timeout | `a_slow_site_is_a_timeout_error_after_the_requests_wait` |
| one network error | `a_dead_host_is_retried_then_reported_in_the_result` |
| the 404 original and the creator with zero posts | `non_images_are_skipped_and_a_404_original_is_counted_not_fatal`, `a_creator_with_no_posts_is_indexed_as_empty`, `an_unknown_artist_is_an_error_not_a_crawl` |
| the error for a person has no URL, query, path or OS text; a missing-folder open | `a_missing_folder_is_an_io_error_that_names_no_path_for_a_person`, `a_database_that_is_not_one_is_a_database_error_that_names_no_path`, `a_failed_creator_list_is_an_http_error_that_names_no_url_for_a_person`, `a_dead_host_is_retried_then_reported_in_the_result` |
| 429 waited out; the default wait | `being_rate_limited_is_waited_out_not_fatal`, `a_429_with_no_usable_retry_after_waits_thirty_seconds` |
| the schema is Python's | `the_schema_is_pythons_schema` |
| no engine or site host is redirected in the shipped binary | `tests/net_host_override.rs::a_host_with_no_override_is_not_redirected` (3.b); `PawchiveCrawler::with_hosts` is a library constructor and no binary calls it |

## 6. Python tests that exercise a function in scope

Found by name over `tests/` at `76c85c4`. Rust names are in `tests/pawchive_index.rs` unless a path
is given.

| Python tests | Rust |
|---|---|
| `test_pawchive_index.py::TestTheIndex` (10) | `an_edited_copy_is_found_and_scored_as_the_same_picture`, `a_different_picture_is_not_a_match`, `the_same_sha256_is_the_very_same_file`, `one_result_per_post_on_its_closest_image`, `new_images_are_searchable_straight_away`, `removing_an_artist_removes_their_images`, `artists_report_what_was_indexed`, `a_lightly_cropped_copy_still_surfaces_as_one_to_review`, `the_scale_keeps_variants_below_the_auto_import_line`, `links_name_the_artist` |
| `test_pawchive_index.py::TestIndexingAnArtist` (6) | `every_page_of_posts_and_every_image_is_indexed`, `a_refresh_fetches_only_what_is_new`, `non_images_are_skipped_and_a_404_original_is_counted_not_fatal`, `stopping_keeps_what_was_done_and_the_next_run_carries_on`, `being_rate_limited_is_waited_out_not_fatal`, `an_edited_copy_of_an_indexed_image_is_found_after_a_crawl` |
| `test_pawchive_index.py::TestFindingArtistsByName` (1) | `the_creator_list_is_downloaded_once_then_kept_for_a_week` |
| `test_pawchive_index.py::TestTheIndexAsASearchEngine` (6) | These call `search_engine._collect_pawchive_index`, `core.rate_limit.engine_interval`, `_engine_waves` and `_resolve_multipage_candidate`, which are the engine runner's, pacing and multipage code ported in Phases 1 and 3 (`engine_runner::tests`, `net/pacing.rs`, `tests/multipage_tests.rs`). The index half they use (`add_images`, `search`, `is_empty` on a missing file) is `an_edited_copy_is_found...`, `the_same_sha256_is_the_very_same_file` and `an_index_that_does_not_exist_yet_is_empty_and_costs_no_file`. Wiring `LiveEngines` is section 1 |
| `test_workers_background.py::TestPawchiveIndexWorker` (8) | `each_job_is_indexed_in_order_and_reported`, `progress_from_the_crawler_is_forwarded`, `a_stop_requested_mid_job_is_visible_to_the_crawler_and_ends_the_batch`, `a_stop_before_the_run_starts_indexes_nothing`, `a_failure_mid_batch_ends_the_run_with_the_error_and_keeps_what_was_reported` (covers both crash tests); "the index is opened at the path the worker was given" is the index the caller passes (asserted in `run_jobs`); "the job list is copied" does not apply: `jobs` is a borrowed slice the caller cannot change mid-run; `finished_all` is the return of `run_index_jobs` |
| `test_workers_background.py::TestPawchiveFindWorker` (3) | `the_find_worker_body_emits_results_with_no_error`, `the_find_worker_body_emits_a_failure_as_text_beside_an_empty_list`, `the_find_worker_body_emits_no_matches_as_an_empty_list_and_no_error` |
| `test_parsers_and_tags.py::TestInlineTagEdit` (10) | `tag_rules::tests::inline_edit_clearing_the_text_deletes_the_tag`, `inline_edit_whitespace_only_counts_as_cleared`, `inline_edit_a_rename_keeps_the_original_source`, `inline_edit_a_bare_name_clears_the_namespace`, `inline_edit_the_remap_rules_apply_to_hand_typed_edits`, `inline_edit_renaming_onto_an_existing_tag_drops_the_older_one`, `inline_edit_an_ordinary_rename_drops_nothing`, `inline_edit_the_edited_tag_keeps_its_slot` (the object-identity test), `split_tag_text_splits_on_the_first_colon` (both `split_tag_text` tests, 3.g); plus `inline_edit_a_duplicate_after_the_target_is_dropped_too` and `inline_edit_a_target_that_is_not_in_the_list_changes_nothing` |
| `test_gui_smoke.py::TestPawchiveIndexWindow`, `test_gui_harness.py` (the Index dialog) | Qt window: Phase 6 |
| `test_outcome_tags.py`, `test_core_utils.py`, `test_tag_rules.py`, `test_search_quality.py` (they set `enable_pawchive_index`) | do not call a function in scope: a settings flag |
