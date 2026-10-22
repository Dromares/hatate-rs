# Availability: is the match's page still there (Phase 3.d, DAN-2060)

What this ticket ports, where the cut is, how it was checked against the real Python, which
differences could not be matched, and which Python tests map to which Rust ones. Python source of
truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net layer (`docs/net.md`), the 3.b
fetch half (`docs/network-halves.md`) and the Phase 2 pure halves (`docs/parsers.md`) are reused as
they are; the changes to them are listed in section 2.

## 1. The cut

Ported here: all of `core/availability.py` as `hatate_core::availability`.

| Python | Rust (`availability::`) |
|---|---|
| `check_url_available`, `_page_says_available` (the GET with the body unread unless a 200 needs scanning, under a wall-clock deadline over the whole exchange) | `check_url_available`, `page_says_available` |
| Sankaku's API second opinion `sankaku_post_exists`, `SANKAKU_POST_ID_RE` | `sankaku_post_exists` |
| `SOFT_404_MARKERS`, `SOFT_404_REDIRECTS`, `INDEX_REDIRECT_PATHS`, `SOFT_404_SCAN_BYTES`, `_soft_404_markers_for`, `has_soft_404_detection`, `_redirect_to_index`, `_redirect_means_gone`, `_page_title` | `soft_404_markers_for`, `has_soft_404_detection`, `redirect_to_index`, `redirect_means_gone`, `page_title`, the consts |
| `url_is_structurally_dead`, `drop_structurally_dead` (the runner imports the first) | the same names |
| `AVAILABILITY_CHECK_HOSTS`, `_should_availability_check` | `AVAILABILITY_CHECK_HOSTS`, `should_availability_check` |
| `_reactor_full_fallback` | `reactor_full_fallback` |
| `_sweep_candidate_availability` (5 workers), `_recheck_cached_candidates` | `sweep_candidate_availability`, `recheck_cached_candidates` |
| the rotated DeviantArt cookie capture during a check | `check_url_available` / `page_says_available` call `boorus::fetch::capture_rotated_deviantart_cookies` with the response of the last hop |
| `core.hydrus_import.normalize_url_for_hydrus` (the one pure function the sweep needs from the Hydrus import) | `hydrus_url::normalize_url_for_hydrus` |

Not here, and who owns it: the search worker that decides **when** to sweep and acts on the
verdicts (`search_image`, `drop_dead_matches`, the `fallback_below_similarity` retry) is 3.h; the
manual "re-check these matches" Qt worker (`workers/availability_worker.py`) is Phase 6; the Hydrus
client and the rest of `hydrus_import` are Phase 4.

No `mocked_transport` corpus item exercises a function in this ticket's scope
(`git grep -l availab corpus/` finds only `corpus/formats/` session files that carry the
`remote_available` fields, and a Pawchive page), so there is no corpus replay to add. The Phase 2
corpus runner is unchanged and has no skip list.

## 2. Changes to earlier code

| Where | What | Why |
|---|---|---|
| `net::NetRequest::with_body_only_on(status)` and `transport.rs` | the body is read only from an answer with this status; any other is returned with an empty body and the connection dropped unread | `stream=True` plus a `resp.raw.read` made only for a 200 |
| `net::transport` | a gzip or deflate body that ends inside its framing is read to its end as urllib3's decoder does, on a probed read | found by the differential (a truncated gzip body) |
| `net::setcookie` (new, `http.cookiejar`'s default policy) | which `Set-Cookie` lines `Response.cookies` would keep for a response to a URL | the DeviantArt renewal must save exactly what `requests` saw. Replaces the hand-rolled `Set-Cookie` split in `boorus::fetch::capture_rotated_deviantart_cookies` (3.b), which took every `Set-Cookie` line without the jar's domain, path and expiry rules |
| `remote::set_header` / `requests_accepts_header_value` | `requests` 2.34's `check_header_validity` (`^\S[^\r\n]*\Z\|^\Z`) | found by the differential; also a latent gap in the 3.b referer paths, which call the same function |
| `remote::Raised::BodyRead` | a urllib3 exception `requests` did not wrap because the caller read the raw response | `resp.raw.read` in the availability GET |
| `model::Candidate::record_availability` | a verdict is stored with its date; an inconclusive check clears the date | `MatchCandidate.record_availability` |
| `tests/common/net_server.rs` | `Reply::closing`, `Reply::content_length`: a body that breaks off mid-way | test support |

Every earlier test is unchanged and green.

## 3. The transport differential

The real Python at `76c85c4` (its `core/` copied out, all 73 files compared byte for byte with
`git show 76c85c4:<file>`) ran with `requests`' `HTTPAdapter.send` replaced by a script, plus
`core.net`'s `time.sleep` (recorded, not slept). Everything above it was the real code: `requests`' `Session.request`
and redirect handling, `http.cookiejar`, real `urllib3` response objects (including gzip decoding
and a connection that breaks mid-body), `core.net`'s deadline, and `core.availability` itself. The
same scripted answers were served to the Rust code from one local https server (hosts reached
through `NetConfig::with_host_override`, TLS through `NetClient::new`). Compared, per scenario: the
return value or the error class and message; every request (method, URL with query, the headers
Python sets, the cookies sent); every wait; and the state left behind (the candidates' verdicts and
dates, the progress ticks, the saved DeviantArt cookies and whether `config.json` was written).

The harness is outside the repo (Rust only in the tree); its log is
`worktrees/DAN-2060-differential.log`, one block per seed.

| Seed | Scenarios | Python requests compared |
|---|---|---|
| 20602060 | 1,500 | 2,279 |
| 9 | 3,000 | 4,066 |
| 4242 | 3,000 | 4,268 |
| **Total** | **7,500** | **10,613** |

The scenarios cover every function above: the status codes; every soft-404 marker set at the
scan-size edge (before, at and past 60,000 bytes) in head, body and cut multibyte positions; every
redirect-to-index and redirect-means-gone shape the module names; Sankaku's API with malformed
bodies and every status; JoyReactor's fallback; the sweep and the cached re-check with mixed
verdicts and ticks; the DeviantArt renewal with expired, foreign-domain and
quoted cookies; bodies that break or do not decode; and the hard-timeout path (an answer delayed past
the deadline).

**Result: 0 unexplained differences in 7,500 scenarios.** Everything else is one of the four
classes below, counted by the harness (every count is over the three seeds; the per-seed split is
in the log).

## 4. What is not matched, with counts

| Class | What differs | Count | Why it is kept |
|---|---|---|---|
| **V1** a body that breaks or does not decode | the exception class is the same urllib3 class (`DecodeError`, `ProtocolError`); the message is the HTTP library's own words (`error decoding response body: Invalid gzip header` against `('Received response with content-encoding: gzip, but failed to decode it.', error('Error -3 while decompressing data: unknown compression method'))`) | 31 (3 + 20 + 8) | the message is the library's. The class, and therefore what the caller does, match |
| **V2** explicit cookies after a redirect to another host | `requests` puts the `cookies=` dict into the session's jar as domain-less cookies and so sends them on every hop, including a hop to another host. The net layer drops `Cookie` from a hop whose host differs from the original's, unless the cookie's scope names the new host (`docs/net.md` section 8) | 613 (195 + 212 + 206) | this is the Phase 1 rule: the URLs come from search results, which are not trusted, and the user's site logins must not leave their site. Matching Python would send a pasted Sankaku or DeviantArt cookie to whatever host an engine's result redirects to. The first request of a redirect chain matches; only a later hop differs, and only by the `Cookie` header. The verdict was the same in every case the harness could compare |
| **V3** a `Referer` holding a control character | `requests` accepts a value with a C0 control character other than tab, CR and LF, or DEL, and `http.client` sends it as it is. The HTTP stack here cannot form such a header, so no request is made and the check answers "unknown". In 162 of the 335 scenarios the verdicts differ (Python true or false, Rust `None`); in the other 173 both answer the same | 335 (68 + 125 + 142) | the check's own callers never build one: the sweep passes no `Referer`, and the manual worker passes `referer_for_candidate`, the origin of a candidate's own URL. A URL holding a raw control character is not a shape any engine or site sends. The failure is the safe one: "unknown" keeps the match |
| **V4** a cookie set by a redirect response | `requests` keeps a `Set-Cookie` from a redirect's answer in the request's jar and sends it on the next hop when the jar's rules allow. The net layer sends only the caller's cookies on each hop | 344 (61 + 131 + 152) | the same Phase 1 design as V2 (the net layer has no jar). Only the `Cookie` header on a hop after a redirect differs; the verdict was the same in every scenario (an unequal verdict outside V3 would have been unexplained). |

The harness also watched for a fifth class, **V5** (the `url` crate writing `%27` where `requests`
sends `'` in a query); it had 0 hits.

V2, V3 and V4 change what is sent or whether it is sent, never a verdict that can drop a match: a
verdict of "gone" needs a 404 or 410, or a marker in a body, and none of the three can produce one.
They go to Cloud on DAN-2060 as the classes this ticket could not match, before the verify ticket
opens.

## 5. The tests

`crates/hatate-core/tests/availability_https.rs` (69 tests) drives every function through
`NetClient::new` (rustls with the explicit CryptoProvider) against a local https server; the pure
functions are plain tests. `hatate_core::net::setcookie` has its own unit tests.

| Rule | Test |
|---|---|
| network-client test over the shipped scheme (DAN-1909) | `availability_check_runs_over_tls_through_the_binaries_constructor` |
| one deadline: the head never arrives | `a_head_that_never_arrives_is_unknown_at_the_deadline` |
| one deadline over the body, not each read | `the_deadline_covers_the_body_not_each_read` |
| one network error | `a_network_failure_is_unknown`, `a_network_failure_is_unknown_never_gone_for_the_api` |
| no engine or site host is redirected in the shipped binary | `tests/net_host_override.rs::a_host_with_no_override_is_not_redirected` (3.b) |

## 6. Python tests that exercise a function in scope

Found by name over `tests/` at `76c85c4` (87 test functions call one of the functions above;
`test_urls_and_availability.py` holds 71 of them). All Rust names are in `availability_https.rs`
unless a path is given.

| Python tests | Rust |
|---|---|
| `TestAvailabilityDetection` (6) | `only_a_definite_404_or_410_is_gone`, `success_is_available`, `a_403_and_server_errors_are_unknown_not_gone`, `a_network_failure_is_unknown`, `a_missing_url_is_safe` (the 403 and the server errors share one test) |
| `TestStructurallyDeadUrls` (4 of 5) | `a_numeric_sankaku_id_is_structurally_dead`, `a_modern_sankaku_id_is_not_structurally_dead`, `other_sites_are_never_called_structurally_dead`, `a_cached_result_is_cleaned_of_structurally_dead_matches` |
| `TestStructurallyDeadUrls::test_such_a_match_never_becomes_a_candidate` | does not apply here: `_append_candidate` is the engine runner's (3.f), which calls `url_is_structurally_dead` |
| `TestSankakuPostExists` (8) | `a_numeric_id_is_dead_whatever_became_of_the_post`, `a_live_post_with_a_picture_is_available`, `a_post_whose_picture_is_withheld_counts_as_gone`, `a_deleted_status_counts_as_gone`, `a_network_failure_is_unknown_never_gone_for_the_api`, `rate_limiting_is_unknown_too`, `other_sites_are_left_to_the_ordinary_check`, `both_sankaku_url_shapes_are_recognised` |
| `TestSoftRedirectDetection` (9) | `a_sankaku_empty_page_redirect_is_gone`, `a_sankaku_not_found_redirect_on_another_subdomain_is_gone`, `a_live_sankaku_post_survives_the_not_found_rule` (also covers `test_live_sankaku_post_is_available`), `the_landing_matches_whatever_the_locale_prefix_or_trailing_slash`, `the_rule_does_not_leak_to_other_sites`, `a_network_failure_is_still_unknown_for_sankaku`, `sankaku_and_the_moebooru_hosts_are_enrolled_for_checking`. `test_non_string_final_url_is_handled` does not apply: a Rust URL is always a string (the differential's `final` argument covers `None`) |
| `TestGelbooruDeletedPosts` (5), `search_quality::TestRule34Soft404`, `test_parsers_and_tags::TestGoneByRedirectNotByWording` (2) | `a_deleted_post_redirecting_to_the_list_is_gone` (Gelbooru, Safebooru, Rule34), `a_live_gelbooru_post_is_available`, `a_list_url_does_not_report_itself_gone`, `gelbooru_has_no_wording_to_match_on`, `a_gelbooru_network_failure_is_still_unknown` |
| `TestSankakuEmptyBody` (5) | `a_real_empty_sankaku_page_is_detected`, `a_real_post_page_is_not_flagged`, `the_head_alone_is_enough`, `the_first_sankaku_markers_appear_early_in_the_document`, `an_unrecognised_body_leaves_the_match_alone` |
| `TestAvailabilityLoggingIsDiagnosable` (4) | `success_says_the_body_was_scanned`, `a_marker_miss_reports_the_page_title`, `a_body_that_is_not_html_reports_no_title`, `success_says_when_a_host_has_no_markers` (the log text is captured through a `tracing` subscriber) |
| `TestMoebooruDeletedPosts` (6) | `a_deleted_moebooru_post_is_detected`, `a_live_moebooru_post_is_not_condemned_by_the_hidden_login_template`, `the_placeholder_image_is_a_second_signal`, `both_moebooru_signals_are_registered_for_every_host`, `sankaku_and_the_moebooru_hosts_are_enrolled_for_checking`, `a_moebooru_network_failure_is_still_unknown` |
| `TestReactorFullResFallback` (5) | `a_confirmed_dead_full_url_falls_back_to_plain`, `available_and_unknown_are_left_alone`, `a_dead_non_reactor_or_plain_reactor_candidate_is_reported_dead_as_normal`; and in the sweep `the_sweep_marks_every_verdict_and_falls_back_for_a_dead_reactor_full_file` |
| `TestIndexRedirectDetection` (4) | `a_bounce_to_the_index_is_detected`, `a_real_post_is_never_flagged_as_an_index_bounce`, `the_index_rule_is_scoped_to_the_originating_host`, `an_index_bounce_is_unverifiable_rather_than_gone` |
| `TestSankakuAuthenticatedChecks::test_sankaku_is_checked_on_its_post_page` | `sankaku_and_pixiv_are_checked_on_their_post_page` |
| `TestSankakuAuthenticatedChecks` other 4 (cookie parsing, scoping, the manual worker's cookies) | do not apply here: they test `cookies_for_url` and the worker, which are `net::site` (Phase 1) and Phase 6. The cookie the check sends is covered by `a_renewed_session_cookie_is_captured_during_an_availability_check` and `the_sankaku_login_goes_to_the_sankaku_api_host_and_no_other` |
| `TestAvailabilityChecksUseSiteHeaders` (7) | `sankaku_is_checked_with_a_browser_user_agent`, `other_sites_keep_the_honest_descriptive_agent`, `an_explicit_referer_still_wins`, `headers_for_url_covers_the_right_sites`, `deviantart_is_fetched_with_a_browser_user_agent_and_its_own_referer`, `a_renewed_session_cookie_is_captured_during_an_availability_check`, `the_no_content_page_is_still_read_as_gone` |
| `TestPixivUrlNormalisation` (3), `TestRule34UsUrlNormalisation` (5), `TestLegacyUrlsRewrittenForHydrus` (3 of 4) | `hydrus_url::tests::{danbooru_and_twitter_legacy_forms_are_rewritten_once, rule34_us_encoded_slash_is_decoded_in_the_r_parameter_only, pixiv_legacy_urls_become_the_artwork_page, a_malformed_rule34_us_host_is_the_value_error_urlparse_raises}`, and the differential's 24 `normalize_url_for_hydrus` scenarios |
| `TestLegacyUrlsRewrittenForHydrus::test_the_rewritten_url_is_what_hydrus_is_asked_about_and_sent` | does not apply: `send_url_to_importer` is the Hydrus client (Phase 4) |
| `TestReactorFullResUrlRewrite` (6), `TestSankakuLegacyUrlRewrite` (5), `TestReferer` (3), `TestPixivCookieScoping` (2) | do not call a function in this ticket: `canonicalize_url`, `referer_for_candidate` (`remote::tests::the_referer_is_for_the_candidates_own_urls_only`, 3.b) and `cookies_for_url` |
| `search_quality::TestCachedResultsAreRechecked` (4) | `a_cached_match_that_has_since_gone_is_dropped`, `an_unknown_answer_keeps_the_cached_match`, and `the_sweep_skips_candidates_that_already_have_a_verdict_and_ticks_around_the_work` (one definition of "gone": `recheck_cached_candidates` calls `sweep_candidate_availability`). `test_it_uses_the_same_sweep_a_fresh_search_uses` and `test_it_is_skipped_when_the_user_has_not_asked_for_it` assert on `inspect.getsource` of `search_image`, which is 3.h's; the `drop_dead_matches` check lives there |
| `search_quality::TestFallbackSurvivesLosingItsReason` (2) | do not apply here: they run `search_image` with `check_url_available` patched; 3.h owns `search_image`. The function they patch is covered above |
| `test_workers_background.py::TestAvailabilityWorker` (4 that call a function in scope) | do not apply here: the Qt worker is Phase 6. The calls it makes are `check_url_available`, `has_soft_404_detection` and `normalize_url_for_hydrus`, all covered above |
| `test_workers_background.py::TestAvailabilityWorker::test_a_check_that_raises_is_counted_as_done_without_taking_the_run_down` | the sweep's counterpart is in the differential: a URL `urlparse` refuses raises `ValueError` inside a sweep and is logged, and the run carries on (`a_url_urlparse_refuses_raises_a_value_error_not_an_unknown` for the direct call) |
| `test_availability_persistence.py::TestRecordAvailability` (2) | `a_definite_verdict_is_dated_and_an_inconclusive_one_clears_the_date`. The reload tests in the same file are the session store (Phase 1, `session_compat.rs`) |
