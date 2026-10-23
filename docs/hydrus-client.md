# The Hydrus client, read half (Phase 4.a, DAN-2126)

What this ticket ports, where the cut is, how it was checked against the real Python, what is not
matched, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. The Phase 1 net layer (`docs/net.md`), the config
(`HydrusSettings`) and the model (`Entry`, `Tag`) are reused as they are.

## 1. The cut

Ported here:

| Python | Rust |
|---|---|
| `core/hydrus_client.py`: `_looks_like_service_key`, `_extract_current_tags`, `HydrusError` (with `status_code`), `HydrusFile`, `HydrusClient.__init__`, `_headers`, `_get`, `_post`, `_handle`, `test_connection`, `get_services`, `list_tag_services`, `search_files`, `get_file_metadata`, `get_tags_for_hashes`, `get_tags_and_sizes_for_hashes`, `filter_known_hashes`, `missing_permission`, `get_thumbnail`, `file_id_for_hash`, `get_file_path_url`, `download_file`, `get_url_info`, `get_url_files`, `_resolve_to_service_key`, `HYDRUS_PERMISSIONS` and the two permission ids the read half names | `hydrus_client` (`looks_like_service_key`, `HydrusError`, `HydrusFile`, `HashInfo`, `HydrusClient::{new, with_net_config, get, post_json, post_raw, test_connection, get_services, list_tag_services, search_files, get_file_metadata, get_tags_for_hashes, get_tags_and_sizes_for_hashes, filter_known_hashes, missing_permission, fetch_thumbnail, file_id_for_hash, get_file_path_url, download_file, get_url_info, get_url_files, resolve_to_service_key}`) |
| the rest of `core/hydrus_tag_lookup.py`: `TagLookupResult`, `HYDRUS_FILENAME_RE`, `hash_from_filename`, `verify_filename_hashes`, `parse_tag`, `_size_matches`, `apply_existing_hydrus_tags` (`hash_file` came with 3.h) | `hydrus_tag_lookup` |
| `workers/hydrus_lookup_worker.py`, the body of `run()` without Qt | `hydrus_tag_lookup::run_tag_lookup` |
| (Phase 6 data contract, DAN-2116) | `error::ErrorKind`, `HydrusSettings::is_configured`, `HydrusClient::fetch_thumbnail` |

Not here: every write call (`set_file_relationship`, `set_kings`, `set_note`, `delete_files`,
`undelete_files`, `deletion_states`, `show_files_in_client`, `import_file`, `associate_url`,
`import_url`, `add_tags`) is 4.b; import and reconcile are 4.c and 4.d. `post_json` and `post_raw`
(`_post`) are here because the write half is built on them; nothing in this ticket calls them
except the tests. The Qt `HydrusTagLookupWorker` class itself is Phase 6; its `run()` is
`run_tag_lookup`, which a GUI calls off its UI thread.

Shared registration: this PR creates `crates/hatate-core/src/error.rs` (DAN-2116 item 1). Whichever
Phase 4 PR merges second merges `origin/main` and adds its variants to `ErrorKind`.

## 2. How it is built

- **Transport.** Python calls `requests.get/post` straight at the user's own `api_url`, not through
  `core/net.py`: no 429 retry, no host wait, no rate-limit bookkeeping. `HydrusClient` does the same
  through a `NetClient` of its own (`RetryMode::Never`, `without_host_gate`). `HydrusSettings.verify_ssl`
  is that client's `verify_tls`, and wins over a `NetConfig` the caller passes. The timeout is
  `HydrusSettings.timeout`, per connect and per read (`remote::secs`). `HydrusClient::new` is the
  binaries' constructor: rustls with the explicit `CryptoProvider`. No env var, flag or setting
  redirects it anywhere but the user's `hydrus.api_url`.
- **Request spelling.** The URL is `api_url + path`, then the query as `requests` writes it
  (`urlencode`, `quote_plus`, then `requote_uri`), placed ahead of a `#fragment` of the URL's own. A
  JSON body is `json.dumps` with its default separators and `ensure_ascii`, and `Content-Type:
  application/json` unless the caller names one. The access key is a header, sent as Latin-1, and
  `requests`' refusal of a value that starts with whitespace or holds a line break is kept. A URL with
  no host (`http:///x`) is refused, as urllib3 does.
- **Answers.** Python reads every answer as untyped JSON. Where the body is not the shape the code
  reads, Python raises an `AttributeError` or `TypeError` that its own `except HydrusError` handlers
  do not catch. That is `ErrorKind::UnexpectedResponse` here, carrying Python's own exception text
  (`'list' object has no attribute 'get'`, `'int' object is not iterable`, `cannot use 'list' as a set
  element (unhashable type: 'list')`, `invalid literal for int() with base 10: 'x'`), and
  `filter_known_hashes`, `file_id_for_hash`, `missing_permission` and `resolve_to_service_key` pass it
  on exactly where Python's `except HydrusError` does. `apply_existing_hydrus_tags` returns it as `Err`;
  `run_tag_lookup` makes it a `TagLookupResult` with the text, as the worker's blanket `except` does.
- **Errors (DAN-2116 item 1).** `HydrusError::kind()` is the shared `error::ErrorKind` (`Net(net::ErrorKind)`
  wraps the net layer's kind; no variant is copied). `message()` is Python's `str(exc)`, the text
  `Entry.error_message` and the logs carry. `user_message()` says what failed and what to do, with no
  URL, query, access key, path or OS text; `detail()` is the raw text for a log. `Entry.error_message`
  and `MatchStatus` do not change on disk.
- **Thumbnails (DAN-2116 item 3).** `fetch_thumbnail(hash) -> Result<Vec<u8>, HydrusError>`: Python's
  `get_thumbnail` answers `None` for every failure; this answers a typed error with `kind()` and
  `status_code()`. Granted by name on the ticket as a documented class, not a difference. The
  differential maps Python `None` to Rust `Err` and compares the cause (the HTTP status, or the
  exception class: timeout or connection).
- **`HydrusSettings::is_configured`** is true iff `access_key` is non-empty. Whether Hydrus answers is a
  separate `HydrusStatus`, out of scope.

## 3. The transport differential

The real Python at `76c85c4` (`core/`, `workers/` and `tests/` exported with `git archive`; CPython 3.14.8, the repo's venv) ran with
one layer replaced: `requests.adapters.HTTPAdapter.send`, plus `time.sleep` (recorded). Everything
above it was the real code: `core.hydrus_client`, `core.hydrus_tag_lookup`, `HydrusTagLookupWorker`,
`requests`' `Session.request`, redirect handling, `Response.text` and `.json()`, and real
`urllib3` response objects (including a connection that breaks mid-body). The same scripted answers
were served, in request order, to the Rust code by a local server, through `HydrusClient::new`. The
Rust side ran in a network namespace with loopback only. Compared, per scenario: the return value or
the error class and message; every request (method, URL with query, the access-key and `Content-Type`
headers, the body bytes, the `timeout=` and `verify=` Python passed against the settings); the
files written; every sleep (none); and the entries' tags and hashes after a tag lookup.

The harness is outside the repo (the tree is Rust only); its log is `worktrees/DAN-2126-differential.log`.

| Seed | Scenarios | Operations | Python requests compared |
|---|---|---|---|
| 1 | 3,000 | 3,892 | 3,284 |
| 2 | 3,000 | 4,008 | 3,297 |
| 3 | 3,000 | 3,984 | 3,260 |
| **Total** | **9,000** | **11,884** | **9,841** |

The scenarios cover every function in section 1: each endpoint with its typical answer, answers
mutated at a random node, junk bodies of every JSON type, non-JSON bodies, every status from 100 to
599 (including 401, 403, 429 and 5xx, with 200-, 300- and 400-character texts and multibyte text at
the cut), same-host redirects of 301, 302, 303, 307 and 308 up to three deep, transport errors
(refused, timed out), `api_url` with a trailing slash, a path prefix, a double slash, a query, a
fragment, no scheme and no host, access keys with leading and trailing spaces, line breaks and
control and non-ASCII characters, tag searches with quotes, `&`, `#`, `%`, `'`, `~`, `*`, `+`,
emoji and the empty list, downloads of 0 to 140,000 bytes to a good path, a missing folder and a
directory, tag lookups over files of several sizes with the remap on and off and earlier Hydrus tags
on the entry, `verify_filename_hashes` over genuine, lying and absent files at several sample
sizes, and `hash_from_filename` over 16 name shapes.

**Result: 0 unexplained differences in 9,000 scenarios.** Everything else is one of the classes
below, counted by the harness (over the three seeds).

Matched on the way, each one a difference the first runs showed: where a URL's own query and
fragment sit when `params` are added; `http:///path` refused; the access key sent as Latin-1 and
refused as `requests` refuses it; `download_file`'s non-200 error carrying no `status_code` (the
message names the status, the exception holds none); the Python 3.14 text of the set-element
`TypeError`.

## 4. What is not matched, with counts

| Class | What differs | Count | Why it is kept |
|---|---|---|---|
| **C1** library words | a `Could not reach Hydrus at {api_url}: ` or `Could not download file N: ` message keeps its prefix; the tail is the HTTP library's own words (`refused` against `error sending request: client error (SendRequest): connection closed before message completed`). The cause class (timeout, connection, request refused) matches in every scenario | 525 messages (172 + 195 + 158) | the message is the library's; the class, and therefore what the caller does, match. Same class as DAN-2060's V1 |
| **C2** typed fields | Python carries a JSON value along untyped: a tag that is a number, `size` or `width` that is a float, a file id that is a string, a service name that is `null`, a `file_ids` that is not a list. The Rust fields are typed, so the same answer is `Err(UnexpectedResponse)` with a message that says which field and what type. A mixed-type tag list, which Python fails on with a `TypeError` whose text depends on set order, is the same error | 160 operations (63 + 42 + 55) | a Hydrus never sends these. The Python results for them are `int`s and `None`s that no caller can use as a tag or a size. A typed error is the safe reading |
| **C3** zero timeout | `HydrusSettings.timeout <= 0` (hand-edited): `requests` raises `ValueError` from urllib3 before sending, which `except RequestException` does not catch. Rust treats the timeout as zero, so the call fails at once with a timeout error | not reachable by the differential (the scripted adapter sits below the check); `a_timeout_is_a_typed_net_error_after_the_configured_wait` covers the timeout path | the setting is documented as hand-edit only, and the failure is the same call failing, with a typed error |
| **C4** JSON | `json.loads` accepts `NaN`, `Infinity`, `-Infinity`, lone surrogate escapes and integers past 64 bits, which `serde_json` reads as "not JSON" or as floats. A body using them is `{}` here | 0 in the generated scenarios (none is generated; `PyResponse::json` is the Phase 3 reader) | the same class as `docs/parsers.md` (the `json.loads` notes). Hydrus does not send them |
| **C5** download breaks mid-body | Python streams to disk: a body that breaks off leaves the part written so far and raises `requests`' `ChunkedEncodingError`, which is not a `HydrusError`. The net layer reads the whole body first, so Rust writes nothing and returns `HydrusError` (`Net`). With an unwritable destination as well, Python fails at `open` before it reads, Rust fails at the read | 49 (35 in the operation's result, 14 in the file left behind) | an incomplete download is a failed download either way. No partial file is the safer outcome. `a_failed_download_leaves_no_file_and_says_which_one` |
| **C6** very large download | the net layer caps one response body at `DEFAULT_MAX_BODY_BYTES` (512 MiB), so a larger file is `Err(Net(BodyTooLarge))`; Python streams without a limit | not generated (a 512 MiB body) | `download_file` is used to read back an image to compare or hash; streaming to disk needs a streaming mode in the Phase 1 net layer, which this ticket does not change |
| **C7** control character in the key | an access key holding a control character other than tab (`\x01`): `requests` accepts it and `http.client` sends it; the HTTP stack here cannot form such a header, so no request is made and the call fails as "could not reach Hydrus" | 64 scenarios (24 + 16 + 24) | a Hydrus access key is 64 hexadecimal digits. The failure names no key. The same limit as DAN-2060's V3 |
| **C8** stale `Content-Type` after a redirect | after a 301, 302 or 303 answer to a POST, `requests` sends the follow-up GET without `Content-Type`; the Phase 1 net layer keeps the original one on the bodiless GET | 48 requests (15 + 19 + 14) | Phase 1 behaviour, not changed here. The header on a GET with no body does nothing. Hydrus does not redirect its API |
| **C9** relative `Location` | a relative `Location` (`../d`) against a request path with empty segments (`///verify_access_key`, from an `api_url` ending in `/`) resolves to `//d` in the net layer's WHATWG join and to `/d` in `urljoin` | 6 requests (1 + 3 + 2) | Phase 1 behaviour, not changed here. Needs a redirect and a trailing slash in the URL |
| **C10** unwritable destination | Python raises the `OSError` (`FileNotFoundError`, `IsADirectoryError`); Rust returns `HydrusError` of kind `Io` with the same text (`[Errno 2] No such file or directory: '...'`) | 139 operations (49 + 49 + 41) | the shared error shape (DAN-2116): a caller gets one error type with `user_message()` that names no path |

C1 and C10 change only the type or the words of a failure that is the same failure. C2, C5, C6, C7
and C3 change an outcome in a case no Hydrus produces. C8 and C9 are Phase 1 net-layer behaviour found by
this differential; they go to Cloud on DAN-2126 with the others before the verify ticket opens.

## 5. The tests

`crates/hatate-core/tests/hydrus_client.rs` (40 tests) drives every function through
`HydrusClient::new` (rustls with the explicit CryptoProvider) against a local server; the pure
functions have unit tests in `hydrus_client.rs` and `hydrus_tag_lookup.rs`;
`tests/hydrus_tag_lookup.rs` (10 tests) drives the tag lookup and the worker body.

| Rule | Test |
|---|---|
| network-client test over the shipped scheme, plain http (the default `api_url`) | `the_default_scheme_is_plain_http_and_the_key_goes_in_a_header` |
| https with verification on refuses a self-signed server | `https_with_certificate_checks_on_refuses_a_self_signed_server` |
| https with verification off accepts it | `https_with_certificate_checks_off_accepts_a_self_signed_server` |
| https with a root the caller trusts; the settings switch wins over the net config | `https_with_checks_on_trusts_a_root_the_caller_adds`, `the_settings_switch_wins_over_the_net_config` |
| a download over https checks the certificate like every call | `a_download_over_https_checks_the_certificate_like_every_other_call` |
| one timeout | `a_timeout_is_a_typed_net_error_after_the_configured_wait` |
| one network error | `a_network_error_names_the_api_url_and_carries_no_status`, `a_post_to_an_unreachable_hydrus_names_the_url_too` |
| the error for a person carries no URL, query, key or OS text | `an_error_for_a_person_has_no_url_query_key_or_os_text`, `an_unwritable_destination_is_an_io_error_that_names_no_path_for_a_person`, unit `an_error_for_a_person_names_no_secret_whatever_its_kind` |
| no 429 retry (Hydrus calls bypass `core/net.py`) | `a_429_is_not_retried_because_hydrus_calls_bypass_the_net_retry` |
| `is_configured` | unit `a_default_config_is_not_configured_and_a_key_makes_it_so` |
| no engine or site host is redirected in the shipped binary | `tests/net_host_override.rs::a_host_with_no_override_is_not_redirected` (3.b); the Hydrus client takes no host override at all |

## 6. Python tests that exercise a function in scope

Found by name over `tests/` at `76c85c4` with the harness's `find_tests.py` (162 test functions name
one of the functions in section 1). Rust names are in `tests/hydrus_client.rs` unless a path is given.

| Python tests | Rust |
|---|---|
| `test_core_utils.py::TestFilenameHashes` (7) | `hydrus_tag_lookup::tests::{a_hydrus_style_name_gives_its_hash_and_an_ordinary_one_gives_nothing, the_derived_hash_equals_the_real_one, verification_passes_on_a_genuine_store_and_rejects_names_that_lie, verification_declines_when_there_is_nothing_to_verify_or_a_file_is_unreadable, the_sample_is_every_nth_candidate_up_to_the_sample_size}` |
| `test_core_utils.py::TestHydrusSizeCrossCheck` (3) | `tests/hydrus_tag_lookup.rs::a_size_that_differs_blocks_the_import_and_a_missing_one_does_not`; unit `a_size_that_is_unknown_zero_or_unreadable_passes_and_a_different_one_does_not` |
| `test_core_utils.py::TestHydrusTagLookupFailureIsDistinguishableFromZeroTags` (2) | `tests/hydrus_tag_lookup.rs::a_lookup_failure_is_distinguishable_from_zero_tags` |
| `test_core_utils.py::TestHydrusThumbnails` (6) | `known_hashes_asks_for_identifiers_only_and_a_null_file_id_is_not_known`, `hashes_hydrus_never_saw_are_not_known_when_it_answers_no_rows`, `a_thumbnail_is_bytes_and_every_failure_is_a_typed_error` (the 403 that Python answers `None` for is `Err(PermissionDenied)`, the granted class), `an_empty_hash_list_makes_no_request` |
| `test_tag_rules.py::TestExistingHydrusTagsAreActuallyRemapped` (2) | `tests/hydrus_tag_lookup.rs::a_remapped_namespace_reaches_the_imported_tag_only_when_the_remap_is_on` |
| `test_thin_workers.py::TestHydrusTagLookupWorker` (5) | `tests/hydrus_tag_lookup.rs::{the_lookup_worker_turns_an_unreadable_answer_into_an_error_result, the_lookup_worker_still_answers_zero_for_an_empty_batch}`; "the count is emitted" and "entries and settings are passed straight through" assert a mock call and a Qt signal, which `run_tag_lookup`'s return value replaces |
| `test_hydrus_client_write_path.py::TestResolveToServiceKey` (8) | `a_service_name_resolves_to_its_key_and_a_key_is_passed_through`; unit `an_empty_string_an_odd_length_and_non_hex_are_names_not_keys`, `hydrus_own_default_local_key_is_a_key_though_it_is_not_64_long` |
| `...::TestDownloadFile` (6) | `a_download_is_written_whole_and_the_key_stays_out_of_the_url`, `a_failed_download_leaves_no_file_and_says_which_one`, the https and timeout tests in section 5 (the `timeout=` and `verify=` arguments are what the settings give, compared in every differential request) |
| `...::TestUrlInfoReaders` (3) | `url_info_and_url_files_ask_their_endpoints_with_the_url_as_a_parameter` |
| `...::TestRequestPlumbing` (8) | `no_access_key_means_no_access_key_header`, `the_default_scheme_is_plain_http_and_the_key_goes_in_a_header`, `a_raw_post_sends_bytes_as_they_are_with_the_callers_headers`, `a_json_post_sends_python_json_and_an_absent_body_is_an_empty_object`, `a_network_error_names_the_api_url_and_carries_no_status`, `a_post_to_an_unreachable_hydrus_names_the_url_too`, `a_401_a_403_and_other_statuses_are_told_apart_by_status`, `a_200_with_an_unparseable_body_is_an_empty_object_not_a_crash` |
| `...::TestThumbnailReader` (2) | `a_thumbnail_is_bytes_and_every_failure_is_a_typed_error` |
| `...::TestServiceAndSearchReaders` (7) | `tag_services_are_filtered_by_hydrus_own_type_label`, `a_response_with_no_services_key_is_an_empty_mapping`, `search_files_json_encodes_the_tag_list_the_way_requests_does`, `file_metadata_becomes_rows_with_their_current_tags_only` (also the `HydrusFile` default tags), `the_default_scheme_is_plain_http_and_the_key_goes_in_a_header` (`test_connection`) |
| `test_hydrus_duplicate_relationships.py::TestPermissionProbe` (5), `test_hydrus_provenance_note.py::TestPermissionTable` (1) | unit `the_permission_table_is_hydrus_own`; `missing_permission_names_the_checkbox_and_says_where`, `a_catch_all_key_and_an_inconclusive_probe_are_not_missing` |
| `test_hydrus_duplicate_relationships.py::TestErrorStatusCodes` (3) | `a_401_a_403_and_other_statuses_are_told_apart_by_status` (401, 403, 404, 500 carry their status); "403 and 404 become the two messages" is `_relationship_failure`, 4.b |
| `test_url_importer_duplicate_relationships.py::TestFileIdLookup` (6) | `file_id_for_hash_matches_case_insensitively_and_a_null_id_is_none`, `an_empty_hash_list_makes_no_request` |
| `test_hydrus_query_dialog.py::TestImport::test_a_namespaced_tag_keeps_the_rest_of_its_colons` | unit `a_tag_splits_at_the_first_colon_only` |
| every other test that names a function in scope | does not apply here, one reason each: `TestHashSourceSplit` and `test_file_hash_worker.py` test `workers/file_hash_worker.py`'s split (3.i / Phase 6) and only call `hash_file`; `TestImportFile`, `TestAssociateUrl`, `TestImportUrl`, `TestAddTags`, `TestShowFilesInHydrus`, `TestFeatureOff`, `TestDegradesGracefully`, `TestClientWrappers` are the write half (4.b) or the importer that calls it (4.c, 4.d); `test_hydrus_import_poll*.py`, `TestAutoImportWiring`, `TestBatchPollWorkerWiring`, `test_search_quality.py` and `test_sent_tag_filter.py` mock `get_url_files`/`get_url_info` for the importer (4.c, 4.d); `test_thumbnail_worker.py`, `test_gui_smoke.py::TestShowInHydrus` and `test_hydrus_query_dialog.py` (other than the row above) mock the client for GUI workers and dialogs (Phase 6); `test_mcp_bridge.py` patches `HydrusClient` for the MCP bridge (Phase 6/7); `tests/live_sources/check_hydrus.py` talks to a live Hydrus, which no test, CI or verification here may |
