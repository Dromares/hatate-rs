# The network halves of the booru fetch, remote fetches and Reddit feed (Phase 3.b, DAN-2058)

What this ticket ports, where the cut is, how it was checked against the real Python, and which
Python tests map to which Rust ones. Python source of truth: `Dromares/Hatate-Linux-Redux` at
`76c85c4`. The Phase 1 net layer (`docs/net.md`) and the Phase 2 pure halves (`docs/parsers.md`)
are reused as they are; the two changes to them are listed below.

## 1. The cut

Ported here:

| Python | Rust |
|---|---|
| `core/boorus/__init__.py`: `fetch_page_info`, `fetch_tags`, `local_md5`, `resolve_fetch_url` followed, `BooruError` / `BooruContentGoneError`, the parser-health counters | `boorus::fetch` (`fetch_page_info`, `fetch_tags`, `network_context`, `BooruSession`), `parser_health` |
| the Gelbooru family's Data-API fetch and its 401 latch (one per module: Gelbooru and Safebooru share one, rule34 and Xbooru have their own) | `boorus::dapi` |
| Pawchive's creator lookup (512-entry LRU) and original-size read (1024-entry LRU) | `boorus::fetch` fills the `Context` under Python's cache keys; `lru` |
| `core/boorus/_remote_size.py` | `boorus::remote_size` (the ranged read stops at the header, as `ImageFile.Parser` does) |
| `redditpost.parse_dimensions` with the real `remote_size` | registered in `redditpost::PARSER`; answers from the `Context` the fetch filled |
| `core/remote.py`: `download_bytes`, `fetch_text`, `fetch_remote_info`, `_format_from_content_type`, `referer_for_candidate` | `remote` |
| `core/reddit.py` network half: `_feed_entries`' GET, `find_post`, the session cache, the loop in `resolve_listing` | `reddit::{fetch_feed_entries, find_post, resolve_listing_online, FeedCache}` |

Not here: `core/availability.py` (3.d) and the engines (3.a, 3.c, 3.e). `core/net.py`'s retry,
pacing and deadline are the Phase 1 layer. Two small additions to it: a per-host address override
on `NetConfig` (how a test, and only a test, points an engine or site host at a local server; no
env var, flag or config key does this in the shipped binary) and a probed body read
(`BodyProbe`: `iter_content(CHUNK)` that stops as soon as a predicate on the bytes so far holds).

Changes to earlier code: `pyimage::image_size_checked` (the decompression-bomb check as an error
instead of "not an image"; `image_size` keeps its answer), `BooruSession::seed_artist` (a test
starts a post from a creator already looked up, as Python's tests do), and the `Raised` enum of
`remote` gained `UnicodeEncode`.

## 2. The transport differential

The real Python at `76c85c4` (its `core/` copied out byte for byte, 97 files, compared with
`git show 76c85c4:<file>`) ran with one layer replaced: `requests`' `HTTPAdapter.send`, plus
`time.sleep` in `core.net`. Everything above it was the real code: `core.net` (the 429 retry, the
host wait, the deadline reads), `requests`' `Session.request`, redirect handling, `Response.text`
and `.json`, and real `urllib3` response objects. The same scripted answers were served to the Rust
code by a local https server. Compared: the return value or the error class and message; every
request (method, URL with query, the headers Python sets explicitly, cookies sent); every sleep
(as the gap between Rust's requests); and the module state afterwards (health counters, latch,
memo, caches). The Rust side ran in a network namespace with loopback only.

| Family | Scenarios | Calls | Unexplained differences |
|---|---|---|---|
| booru (`fetch_page_info` through every parser, Data API, Pawchive, Reddit post, DeviantArt, Sankaku, `image_size`) | 600 | 917 | 0 |
| remote (`download_bytes`, `fetch_text`, `fetch_remote_info`, `_format_from_content_type`, `referer_for_candidate`) | 600 | 600 | 0 |
| Reddit feed and `local_md5` (`_feed_entries`, `find_post`, `resolve_listing`, `local_md5`) | 400 | 674 | 0 |

Matched on the way, each one a difference the first runs showed: the exact spelling of the request
URL as `requests` writes it (`pyurl::requote_uri`), header values sent as Latin-1, base64 as
`binascii.a2b_base64` reads it, a deadline checked before the URL, the decompression bomb raised
from `image_size` and not cached, a `Referer` that `http.client` cannot encode raised as
`UnicodeEncodeError` once the URL is prepared (not before, and not when no time is left), and a
response `Set-Cookie` folded into DeviantArt's session cookie.

### Differences that are not matched

Each is counted over the runs above, and listed on DAN-2058 for Cloud.

| Class | Count | What differs | Why it stays |
|---|---|---|---|
| **C1** explicit cookies after a redirect | 10 (booru 9, remote 1) | Python sends `cookies={...}` to the host a redirect lands on; Rust drops `Cookie` from a hop to another host | The Phase 1 decision of `docs/net.md` section 8, "Credentials do not leave their host": the URLs come from search results, which are not trusted |
| **L3** a failed fetch's words | 31 | Both raise `BooruError("Could not fetch <url>: ...")`; after the colon Python has `requests`' text, Rust has hyper's | The text is the HTTP library's. The class, the URL, and the health counter match |
| **U1** an apostrophe in a query | 21 | `requests` sends `?x=it's`, the `url` crate writes `%27` | The WHATWG query encode set; `NetRequest` is built on `url::Url` |
| **S1** a Content-Length past `i64` | 9 | Python's int has no ceiling and returns 1e20; the Rust size is `Option<i64>` and reads `None` | A file of more than 8 EiB; the format half matches |
| **G1** a Data-API `width`/`height` sent as a JSON string | 6 | `gelbooru.parse_dimensions` returns `post["height"]` as it came, a `str`; the Phase 2 hook reads it with `int()` and `BooruPageInfo.height` is `Option<i64>` | Phase 2 behaviour; every corpus item carries integers |
| **P1** a size Pillow reads from a format the Rust reader does not | 31 | AVIF, TIFF, PPM: Python reads a size from the first bytes, `pyimage` says "not an image" and the result is `(None, None)` | `pyimage` reads the five formats an image host serves (PNG, JPEG, GIF, WebP, BMP). Pillow's other readers are libavif and a TIFF tag table; ICO decodes the embedded image to open, so it needs a decoder. No ICO in the runs was one Python opened |

## 3. Corpus items that needed a socket

`tests/corpus_net_replay.rs`:

* The four `core.reddit._feed_entries` `mocked_transport` items: the server serves each item's
  recorded response, the first request on the wire is the recorded one, and the result and the
  session cache are `expected.json`'s. The item was recorded with `core.net.get` replaced, above
  the net layer's one retry of a 429, so a 429 item sees that request twice on the wire.
* The 14 items with `skipped_network_callables` (17 callable runs: `pawchive.parse` x5,
  `pawchive.parse_dimensions` x10, `redditpost.parse_dimensions` x2). The corpus holds no answer
  for these, so `tests/data/net_replay_pawchive_reddit.json` has two synthetic answer sets per run
  (`ok`: a generated 321x123 PNG and a made-up creator name; `down`: 500 and 404) with, for each,
  the one request the real Python made and the value it returned. The test runs the item's input
  and context through `network_context` and the registered hook, and checks the request on the wire
  and the value.

The Phase 2 corpus runner (`corpus_diff.rs`) is unchanged and keeps skipping those callables.

## 4. Where the tests are

| Rule | Test |
|---|---|
| The whole fetch through the binary's own constructor (rustls, explicit provider) over https (DAN-1909) | `tests/net_boorus_https.rs::fetch_page_info_over_https_through_the_binaries_constructor` |
| One deadline and one network error per client | `a_page_that_trickles_past_the_deadline_is_a_hard_timeout`, `a_network_error_is_a_booru_error_and_counts_as_a_failure`; `download_bytes_network_error_and_deadline_are_both_nothing`; `a_feed_network_error_is_nothing_and_a_hard_deadline_is_raised`; `the_ranged_read_stops_at_the_header_and_a_failure_is_not_cached_but_a_miss_is` |
| No engine or site host is redirected in the shipped binary | `tests/net_host_override.rs::a_host_with_no_override_is_not_redirected` |

## 5. Python tests that exercise a function in scope

Found by name over `tests/` at `76c85c4` (142 test functions mention one of the functions or
helpers above). Grouped by the file; "Phase 2" is a parser hook, which `corpus_diff.rs` already runs
against its corpus items.

| Python tests | Rust |
|---|---|
| `test_parsers_and_tags.py::TestBooruFetchSemantics` (404 is gone, 503 is an error, an unparsed site is not fetched) | `http_404_and_410_are_the_post_being_gone_and_other_statuses_are_fetch_errors`, `a_url_no_parser_claims_makes_no_request` |
| `TestRatingParsing`, `TestSankakuExplainsAnEmptyResult`, `TestMangadexParser.test_no_tags_are_generated` | Phase 2: `boorus::tests::{rating_is_kept_out_of_the_tag_list, a_hook_that_panics_costs_its_own_answer_and_nothing_else, incomplete_reason_is_asked_only_of_a_match_missing_its_tags_or_its_file, a_page_is_assembled_from_every_hook_the_parser_defines}` and the mangadex corpus port |
| `test_search_quality.py::TestAnimePicturesAccountOnlyPosts` | `a_status_the_parser_reads_as_an_account_wall_is_restricted_not_an_error`; the plain 403 in `http_404_and_410_...` |
| `TestDeviantArtParser` rotation (2), `test_urls_and_availability.py::TestAvailabilityChecksUseSiteHeaders` rotation | `a_deviantart_session_cookie_that_rotated_is_saved_and_a_challenge_pages_is_not` (three cases: a 2xx saves, a 403 does not, a fetch with no settings cannot). The availability caller is 3.d |
| `TestAFetchedPageDoesNotOverwriteAGoneVerdict` | `a_url_no_parser_claims_makes_no_request` (the caller's "not alive" verdict is 3.d's) |
| `TestSankakuParser` (hash lookup, fallback), `TestLocalMd5` (3) | `sankaku_is_looked_up_by_the_local_files_md5`, `local_md5_hashes_a_file_reuses_the_digest_and_rehashes_a_changed_one`, `local_md5_of_nothing_a_missing_file_or_a_directory_is_none_and_not_fatal` |
| `TestPawchiveParser` (dimensions from the head of the original, no original, a failure not remembered, a failed creator lookup) | `pawchive_asks_for_the_creator_once_and_reads_the_originals_size_by_range`, `pawchive_makes_no_size_request_for_a_post_the_archive_does_not_hold`, `the_ranged_read_stops_at_the_header_and_a_failure_is_not_cached_but_a_miss_is`, `a_failed_creator_lookup_is_remembered_as_a_miss_and_not_asked_again`, `a_creator_answer_that_is_not_an_object_costs_the_post_all_its_tags` |
| `TestRedditParser` (the feed it fetches, UA, dimensions from the original) | `a_reddit_post_reads_its_originals_size_with_reddits_own_user_agent`; the rest is Phase 2 |
| `TestSubredditLinksBecomeThePost` (6) | `reddit::tests::{only_listings_count_as_subreddits, the_post_holding_the_image_is_found, an_unfound_post_falls_back_to_the_image_not_the_subreddit, a_subreddit_with_no_image_to_go_on_is_dropped, other_urls_are_untouched_and_never_looked_up}`, `find_post_tries_the_newest_feed_then_the_top_and_resolve_listing_falls_back_to_the_image`, `a_feed_is_fetched_once_per_session_and_only_a_200_is_remembered` (a rate-limited feed is not remembered) |
| `test_xbooru_api.py::TestFetchPostFromApi` (7), `TestApiMemo`, `test_parsers_and_tags.py::TestGelbooruApiRefusal` | `the_data_api_answer_is_a_post_only_from_a_good_200_and_a_failure_is_remembered`, `the_data_api_is_asked_once_for_the_file_the_preview_and_the_size`, `a_401_latches_gelbooru_and_safebooru_shares_it_but_rule34_and_xbooru_do_not` |
| `test_xbooru_api.py::TestDimensions` (5), `TestParse` | Phase 2 (the hooks over a given API post) |
| `test_urls_and_availability.py::TestReferer` (3) | `remote::tests::the_referer_is_for_the_candidates_own_urls_only` |
| `test_multipage.py::TestPageListRequestHeaders` | `fetch_text_uses_the_sites_headers_and_a_hard_deadline_escapes_it` |
| `test_search_quality.py::TestAnimePicturesParser.test_format_uses_the_same_names_as_a_content_type_lookup` | `remote::tests::formats_come_from_the_content_type` |
| `test_core_utils.py::TestParserProbe.test_progress_is_reported_for_each_site` | `progress::tests::*` (the one-second ticker the fetch runs under) |
| `test_boorus_harness.py::TestBoorusConstructionAndParse` | Phase 2 corpus |
| `test_boorus_harness.py::TestModuleGlobalLeak` (3) | Does not apply: they guard Python's module globals leaking between tests; the Rust state is a `BooruSession` per owner |
| `test_parsers_and_tags.py::TestParsedOnce` (2) | Does not apply: they count BeautifulSoup parses, an implementation detail of the Python parser |
| `test_parsers_and_tags.py`, `test_search_quality.py` parse-hook tests (`parse_dimensions` of Moebooru, Gelbooru, Danbooru, Eshuushuu, Paheal, Rule34.us, Pixiv, AnimePictures, DeviantArt, Twitter, Sankaku, the parser-contract check) | Phase 2 corpus and the `boorus::*` unit tests |
| `test_core_utils.py::TestRedditSite`, `test_net.py::TestHostRateLimitHeaders`, `test_site_access.py::TestHeadersForUrl`, `TestHydrusUrlImporterGuard`, `TestGoogleLens*` | Match `reddit.` as a host name only: Phase 1 `net`, `sites`, Phase 2 `engines` |
| `test_gui_smoke.py`, `test_mcp_images.py`, `test_session.py`, `test_outcome_tags.py`, `test_pawchive_index.py`, `test_workers_background.py`, and in `test_search_quality.py` the `TestParallelAlwaysMode`, `TestStatedFileInfoBeatsAHead`, `TestBorrowedThumbnails`, `TestNeverMeasuredOrdinalGetsARealScore`, `TestSendUrlFallsBackToHatatesDownload`, `TestPawchivePagesByHash`, `TestExactCopiesAreUploadedWhenHydrusCantFetchThem`, `TestRule34SearchTiles` | Do not apply here: they test a GUI, MCP, session, search or Hydrus caller with `download_bytes`, `fetch_text` or `fetch_remote_info` replaced by a mock; those callers port in later phases and their tests move with them |
