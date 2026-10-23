# Search orchestration: `search_image`, tag borrowing, band metrics (Phase 3.h, DAN-2064 / DAN-2106)

What this ticket ports, where the cut is, how it was checked against the real Python, which
differences could not be matched, and which Python tests map to which Rust ones. Python source of
truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net, cache, quota and session
modules, the 3.b fetch half (`docs/network-halves.md`), the 3.d availability sweep
(`docs/availability.md`), the 3.f engine runner (`docs/engine-runner.md`) and the Phase 2 pure
halves (`docs/parsers.md`) are reused as they are; the changes to them are listed in section 2.

## 1. The cut

| Python | Rust |
|---|---|
| `core/search_engine.py`: `search_image`, `fetch_candidate_details`, `_dedupe`, `_decide_status`, `_load_local_dimensions`, the search-cache apply/load/save, `_measure_tag_band`, `_finish_tag_band_measurement`, `tag_band_recorder` | `search_engine::SearchEngine::{search_image, fetch_candidate_details, measure_tag_band, finish_tag_band_measurement, tag_band_recorder}`, `dedupe`, `decide_status`, `candidate_from_cache`, `candidate_to_cache`, `apply_cached_result`, `cache_entry_for` |
| `core/tag_borrowing.py` | `tag_borrowing` |
| `core/tag_band_metrics.py`, including `Recorder` and its JSONL file | `tag_band_metrics` |
| `core/hydrus_tag_lookup.hash_file` only | `hydrus_tag_lookup::hash_file` |

Not here, and who owns it: the Hydrus client in `hydrus_tag_lookup` (Phase 4); the headless
`hatate batch` that calls `search_image` per image, the retry-once pass, the SauceNAO-quota pause
and the per-host pacing the GUI worker does between images (3.i and Phase 6); the engine clients
themselves (3.a to 3.e, already merged).

`SearchEngine` owns what the Python module holds as globals or passes per call: the net client,
the booru fetch state (`BooruSession`: parser health, the Data-API latch, the file-hash cache), the
engine runner, the search cache, the profile paths, the clock and the tag-band recorder. The
recorder is resolved once when the engine is built (`HATATE_TAG_BAND_METRICS`), as Python resolves
it once per process.

No `mocked_transport` corpus item exercises a function in this ticket's scope
(`git grep -l -i -E 'search_engine|tag_borrowing|tag_band|hash_file' -- corpus` finds nothing), so
there is no corpus replay to add. The Phase 2 corpus runner is unchanged and has no skip list.

## 2. Changes to earlier code

| Where | What | Why |
|---|---|---|
| `model::Candidate` | session-only `booru_tags_fetched`, `remote_info_fetched` (both `serde(skip)`) | `MatchCandidate` fields `search_image` reads and writes; a session load restores `false` |
| `model::Entry` | session-only `last_searched: Option<f64>` (`serde(skip)`) | `ImageEntry.last_searched`; not stored |
| `session::codec` | the two literals above | the struct grew; the on-disk shape is unchanged |
| `lib.rs` | registers `hydrus_tag_lookup`, `search_engine`, `tag_band_metrics`, `tag_borrowing` | |

Behaviour of the Phase 1 to 3.f modules is not changed.

## 3. How it was checked against the real Python

The harness lives outside the repo (`worktrees/DAN-2064-tools/`); its log is
`worktrees/DAN-2064-differential.log`. For each case, Python `search_image` at `76c85c4` runs with
its `requests` transport scripted and its engines scripted, and the Rust `search_image` runs with
the same scripted engine answers and the same site answers served by a local https server through
`NetClient::new` (host overrides, a throw-away CA). Both sides run on a fixed clock and in a fresh
profile. Compared per case: the error class and message; the resulting entry as the session
serialisation (status, candidates in order, selected index, tags, dimensions, notes, flags); the
session-only fields (`booru_tags_fetched`, `remote_info_fetched`, `page_index`, thumbnail hash);
the cache files written (names and contents, `cached_at` and `availability_checked_at` masked);
the band-metrics rows (as written bytes); the parser-health counters; every request (method, host
and path with query, `User-Agent`, `Referer`, `Cookie`, `Authorization`); which engines were
asked; the `on_engines_done` and `on_engine_finished` calls; every sleep the clock was asked for;
and the Data-API latch for Ascii2D. 386 cases in three runs, at rs head `9d10bb8`:

| run | cases | differing |
|---|---|---|
| run1 (hand-written) | 6 | 0 |
| run2 (generated, random mix) | 300 | 0 |
| run3 (generated, scenarios) | 80 | 0 |

Coverage across the three runs: statuses `good` 61, `poor` 204, `not_found` 105, `error` 10 and 6
searches that raised (decompression bomb, an engine exception); cache hit 61; multipage posts
(a `page_index > 0` candidate, Pages API asked) 7 plus 12 scenario cases; a candidate marked gone and kept 16 (and the all-dead scenario, 10 cases); every engine failing 7;
candidates carrying borrowed tags 9; restricted matches 9; band rows 78; Ascii2D blocked 5; a nonexistent file and a directory as the
file; redirects to a listing 7.

The band-metrics file is read by Python's own `tools/measure_tag_bands.py --jsonl` at `76c85c4`,
used as a black box: the 78 rows Python recorded and the 78 Rust recorded are byte-identical, and
the tool's `report.md` and `summary.json` over the two differ only in the `Source:` path line.

### Differences that could not be matched

| class | cases | what differs | why |
|---|---|---|---|
| `noncanonical_hash_not_cached` | 16 of 386 | Python names the cache file by whatever string `entry.hydrus_hash` holds, so a hash that is not 64 lowercase hex digits (upper case, or a short test value) is cached under that name. Rust caches only under the Phase 1 `CacheKey` (64 lowercase hex), so such an entry is searched normally and no cache file is written. Everything else about the case matches | the Phase 1 cache module (DAN-1982) rejects other keys by design, and this ticket may not change its behaviour. A hash `hash_file` computes or Hydrus reports is always 64 lowercase hex, so the real exposure is a hand-edited session. **Brought back to Cloud on DAN-2106 before the verify ticket opens** |
| `engine_exception_class_not_carried` | 1 of 386 | an engine that raises something that is not its own error type: Python reports `RuntimeError: msg`, Rust reports the message with class `?` | a Rust panic carries no class name. The scripted engines are the only place this happens; the shipped engine clients return typed errors. The message and the entry (status `searching`, no candidates) match |

## 4. Tests

Unit tests next to the code: `search_engine` (13: status decision, dedupe, cache glue, local
dimensions, error class names), `tag_borrowing` (13), `tag_band_metrics` (24), `hydrus_tag_lookup`
(3). End to end: `tests/search_image_https.rs` (7), through the binary's own constructor against a
local https server, with the engines scripted (`tests/common/scripted.rs`):

* `search_image_over_https_through_the_binaries_constructor`: the named https test; engine answer
  to candidate, page fetch, tags, status, selected index and cache entry written.
* `a_second_search_of_the_same_file_is_a_cache_hit`, `use_cache_false_searches_again`.
* `a_gone_page_is_dropped_and_the_image_is_not_found`.
* `every_engine_failing_is_an_error_entry_and_is_not_cached`.
* network error: `a_network_error_on_the_booru_page_leaves_the_match_without_tags`.
* deadline: `a_booru_page_that_trickles_past_the_deadline_does_not_hang_the_search` (the page
  would take 30 s to arrive; the search ends inside 10 s).

## 5. Python test mapping

154 Python tests at `76c85c4` mention an in-scope function (the audit is `map_audit.py`, an AST
walk over `tests/`, counting a test whose class `setUp` or module helper mentions one). Most
exercise a GUI or worker that calls `search_image` with it patched out; those do not apply here.

| Python | Rust |
|---|---|
| `test_boorus_harness::TestDecideStatus` (4) | `search_engine::tests::{few_tags_is_poor, enough_tags_and_no_size_info_is_good, enough_tags_but_a_much_larger_match_is_poor, enough_tags_and_only_slightly_larger_is_good}`; plus `the_height_gain_alone_can_flag_it_and_the_switch_turns_the_comparison_off`, `a_zero_or_missing_size_skips_the_comparison` |
| `test_outcome_tags::TestTheRuleDoesNotFeedItself` (1) | `search_engine::tests::stale_rule_tags_do_not_promote_poor_to_good` |
| `test_outcome_tags::TestHookedIntoASearch` (8), `TestTagBandInstrumentationIsNotInflated` (1) | the differential (the `tags_for_found`, `tags_for_not_found` and `tags_for_low_tag_count` settings are in the generated mix, every status); `search_image_https::a_second_search_of_the_same_file_is_a_cache_hit` for the cache-hit tagging; the band count excluding rule tags is the differential's band rows (78, byte-identical) |
| `test_parsers_and_tags::TestBorrowingTagsFromAnotherMatch` (12) | `tag_borrowing::tests::{a_close_enough_match_lends_its_tags, the_chosen_match_stays_chosen_and_says_where_its_tags_came_from, a_much_weaker_match_may_not_lend, zero_slack_allows_only_an_equal_or_better_match, a_match_with_its_own_tags_borrows_nothing_and_fetches_nothing, a_dead_match_is_not_asked, the_strongest_eligible_match_is_asked_first, it_walks_the_eligible_ones_but_fetches_each_once, the_setting_turns_it_off, a_chosen_index_out_of_range_lends_nothing}`. `test_a_candidate_with_no_similarity_is_skipped` and `test_a_chosen_match_with_no_similarity_lends_nothing` do not apply: a Rust `Candidate.similarity` is an `f64` (`0.0` when the engine gave none), never `None` (stated in `tag_borrowing.rs`) |
| `test_tag_band_metrics::TestBuckets` (2), `TestMeasureEntry` (9), `TestSummarize` (5), `TestRecorder` (4) | the same-named `tag_band_metrics::tests` (`the_boundary_is_min_tags_for_good_not_a_hardcoded_five`, `a_min_of_zero_puts_everything_above_tagless_in_range`, `entry_tags_and_chosen_booru_tags_are_bucketed_separately`, `it_uses_the_real_similarity_gate`, `zero_slack_excludes_a_weaker_match`, `an_eligible_lender_with_no_recorded_tags_is_counted_apart`, `cross_engine_lending_is_distinguishable`, `the_richest_lender_is_reported_not_merely_the_closest`, `an_already_rescued_entry_is_flagged`, `no_candidates_is_recorded_rather_than_crashing`, `measuring_leaves_what_it_measures_alone`, `rates_are_per_band`, `the_two_bucketings_disagree_and_both_are_reported`, `an_empty_band_has_no_rate_rather_than_a_division_error`, `an_unknown_bucketing_is_rejected`, `the_table_and_json_carry_the_rates`, `the_recorder_is_off_unless_the_environment_names_a_file`, `rows_round_trip_and_a_truncated_one_is_skipped`, `an_unwritable_path_does_not_raise`, `unknown_fields_in_a_recording_are_ignored`) |
| `test_tag_band_metrics::TestSearchEngineHook` (4) | the recorder is built once in `SearchEngine::with_recorder`; `the_recorder_is_off_unless_the_environment_names_a_file`; the final tag count after selection and "a measurement failure does not break a search" are the differential's 78 band rows and the 6 raising cases |
| `test_limit_triage::BucketForTests` (2) | do not apply: `bucket_for` there is the limit-triage tool's, not `tag_band_metrics.bucket_for` (the audit matched the name) |
| `test_core_utils::TestFilenameHashes` (1), `TestHashSourceSplit` (1), `test_file_hash_worker` (2) | `hydrus_tag_lookup::tests::{hashes_the_raw_bytes_as_lowercase_hex, an_unreadable_file_or_a_directory_is_an_empty_hash, a_file_over_one_chunk_hashes_the_same_as_in_one_piece}` cover `hash_file`; the filename-derived hash and the worker signals are the Qt worker's (Phase 6) |
| `test_core_utils::TestRateLimitCredit` (2) | `on_engines_done` and `on_engine_finished` are in `SearchOptions`; the differential compares both (`finished`, `done`) in every case |
| `test_search_quality::TestParallelAlwaysMode` (1), `TestWeakResultFallbackEndToEnd` (3), `TestFallbackSurvivesLosingItsReason` (6), `TestCachedResultsAreRechecked` (1), `TestGoogleImagesNeverFakesANotFound` (1) | the differential: `secondary_engine_mode` `always`, `fallback_below_similarity`, `enabled_sites` and `drop_dead_matches` are in the generated mix, with the all-dead scenario (10 cases) for the retry; the engine order and the cache files written are compared |
| `test_search_quality::TestAFetchedPageDoesNotOverwriteAGoneVerdict` (4), `TestATransportFailureFetchingBooruTagsIsNotSilent` (1), `TestStatedFileInfoBeatsAHead` (3) | the differential (`remote_available`, the HEAD requests and the `No tags extracted` / `Could not fetch booru tags` notes are compared; 36 HEADs in run3 alone); `search_image_https::a_network_error_on_the_booru_page_leaves_the_match_without_tags` |
| `test_parsers_and_tags::TestRatingTagReachesTheCandidate` (4), `TestDanbooruRestrictedPosts` (1), `TestSauceNaoMirroredResults` (1); `test_tag_rules::TestRemapRunsBeforeBlacklist...` (4) | the differential (`add_rating_tag`, `drop_restricted_matches`, `enabled_sites`, `tag_namespace_remap` and `tag_blacklist` are in the mix; restricted matches 9) |
| `test_session::TestSentToHydrusResetOnFreshSearch` (2) | the differential: the entry's `sent_to_hydrus` and `hydrus_import_confirmed` are set in the start state and compared after |
| `test_session::TestSessionWriteDurability` (1), `test_multipage::TestResolvingAMultiPhotoTweet` (1) | do not apply: the first is a threading test of the session writer (3.i), the second tests `multipage_resolve` (already ported, 2.k) |
| `test_gui_smoke::*` (45), `test_mcp_images::TestFetchFullResolutionMatch` (4), `test_hydrus_import_validation::TestDownloadValidation` (17), `test_search_quality::TestSendUrlFallsBackToHatatesDownload` (8) | do not apply: GUI windows, the batch worker (retry pass, SauceNAO pause, per-host pacing, staging directory), the MCP image tool and the Hydrus import all call `search_image`, `fetch_candidate_details` or `hash_file` with them patched, or are the Hydrus client's; they belong to 3.i, Phase 4 and Phase 6 |
