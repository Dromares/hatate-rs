# The Hydrus client, write half (Phase 4.b, DAN-2129)

What this ticket ports, where the cut is, how it was checked against the real Python, what is not
matched, and which Python tests map to which Rust ones. Python source of truth:
`Dromares/Hatate-Linux-Redux` at `76c85c4`. It extends the 4.a `HydrusClient` (`docs/hydrus-client.md`):
the transport, the timeout, the access-key header, the status mapping and `HydrusError` are 4.a's, as
they are. 4.a's read calls are not changed.

## 1. The cut

Ported here, in `crates/hatate-core/src/hydrus_client/write.rs` (a child module of `hydrus_client`, so
the public paths are `hydrus_client::HydrusClient::…`):

| Python (`core/hydrus_client.py`) | Rust |
|---|---|
| `show_files_in_client` | `HydrusClient::show_files_in_client` |
| `_relationship_failure`, `set_file_relationship`, `set_kings`, `DUPLICATE_BETTER`, `DUPLICATE_ALTERNATE` | `set_file_relationship`, `set_kings`, `DUPLICATE_BETTER`, `DUPLICATE_ALTERNATE` (the rewording is private) |
| `_notes_failure`, `set_note` | `set_note` |
| `delete_files`, `undelete_files`, `deletion_states` | `delete_files`, `undelete_files`, `deletion_states` (`DeletionState`, `DeletionStates`) |
| `import_file`, `associate_url`, `import_url`, `add_tags` | the same names |

Not here: the importers and workers that call these (`workers/` download and send, the duplicate and
note wiring, the URL importer) are 4.c and 4.d; every GUI caller is Phase 6. Nothing in 4.a changes.

New error kinds (`error::ErrorKind`, `#[non_exhaustive]`; the enum is 4.a's): `FileUnreadable` (a local
file `import_file` was about to send could not be read) and `ImportRejected` (Hydrus answered 200 to an
import but its own `status` is 4 or 7). Both are Python errors with no HTTP status; `user_message()` for
each names no path and no Hydrus text, `detail()` and `message()` carry them.

Shared registration: this PR adds two variants to `error.rs` and one `mod write;` / `pub use` pair to
`hydrus_client.rs`. Nothing else in the tree changes.

## 2. How it is built

- **Transport.** Every call goes through 4.a's `post_json`, `post_raw` and `get`: Python's direct
  `requests` calls to the user's own `api_url`, no 429 retry, no host wait. `verify_ssl` is the client's
  certificate switch and `timeout` is per connect and per read. The binaries have no flag, variable or
  setting that redirects the client anywhere but `hydrus.api_url`.
- **Bodies are Python's, byte for byte.** `json.dumps` separators and key order, `ensure_ascii`; the
  differential compares the body bytes of every request. Notable shapes: `set_file_relationship` sends
  `relationships: [{hash_a, hash_b, relationship, do_default_content_merge}]` and never `delete_a` or
  `delete_b`; `set_note` never sends `merge_cleverly`; `show_files_in_client` never sends
  `system_hash_locked`; `associate_url` names the field `url_to_add`; `import_file` sends the file's
  bytes with `Content-Type: application/octet-stream`.
- **Named failures.** `set_file_relationship` and `set_kings` turn a 403 or 404 into the two messages
  Python wrote (the "Edit File Relationships" checkbox, "too old"); `set_note` does the same for
  "Edit File Notes". Every other failure, 401 and 409 and 5xx included, is passed through as 4.a's
  `_handle` wrote it. `show_files_in_client`, `delete_files`, `undelete_files` and the rest have no
  rewording.
- **`delete_files` checks first.** It reads `deletion_states`, sends only the hashes that are `present`
  (a hash Hydrus never saw would be written as deleted), keeps duplicates and order, and returns what it
  sent. A state check that fails as a `HydrusError` is "every hash unknown": nothing is deleted.
- **`add_tags` retries once.** The first attempt uses the field the service value looks like it belongs
  in; a `HydrusError` there is retried with the other field, and the second failure is the one returned.
  An answer Python would raise a raw exception on is not retried, here or there.
- **`import_url`** looks a service name up through the services Hydrus lists only when there are tags
  and the value is not a hex key; a name that does not resolve imports without the tags and warns.
- **Errors (DAN-2116 item 1).** `HydrusError` is 4.a's type: `kind()`, `message()` (Python's
  `str(exc)`), `user_message()` (no URL, query, access key, hash, path or OS text) and `detail()`.
  `Entry.error_message` and `MatchStatus` are not touched.

## 3. The transport differential

The real Python at `76c85c4` (`core/`, `workers/` and `tests/` exported with `git archive`; CPython 3.14,
the repo's venv) ran with one layer replaced: `requests.adapters.HTTPAdapter.send`. Everything above it
was the real code: `core.hydrus_client`, `requests`' `Session.request`, redirect handling, `json=`
encoding, `Response.text` and `.json()`. The same scripted answers were served, in request order, to the
Rust code by a local server, through `HydrusClient::new`. Compared, per scenario: the return value or the
error class and message and cause; every request (method, URL with query, the access-key and
`Content-Type` headers, the body bytes, the `timeout=` and `verify=` Python passed against the
settings); the files read; and every sleep (none).

The harness is outside the repo (the tree is Rust only); its log is `worktrees/DAN-2129-differential.log`.

| Seed | Scenarios | Operations | Python requests compared |
|---|---|---|---|
| 1 | 3,000 | 3,287 | 3,280 |
| 2 | 3,000 | 3,286 | 3,296 |
| 3 | 3,000 | 3,244 | 3,285 |
| **Total** | **9,000** | **9,817** | **9,861** |

Operations by function (all three seeds): `add_tags` 1,318, `delete_files` 1,161, `import_url` 1,001,
`import_file` 898, `show_files_in_client` 876, `set_note` 863, `set_file_relationship` 846, `set_kings`
818, `deletion_states` 781, `undelete_files` 682, `associate_url` 573.

The scenarios cover every function in section 1 with: a typical answer; answers mutated at a random
node; junk bodies of every JSON type; non-JSON, empty and truncated bodies; every status from the
401, 403, 404, 409, 429, 500, 502, 503, 400, 418, 204, 304, 599 set (with 0, 250, 350 and 400-character
texts, multibyte text at the cut); redirects of 301, 302, 303, 307 and 308; timeouts and refused
connections; `api_url` with a trailing slash, a path prefix, a double slash, a query, a fragment, no
scheme and no host; access keys that are empty, padded, hold a line break or a control character;
`tag_service_key` empty, a name, a hex key, padded; hashes that are empty, upper-case, 70 characters,
quoted, non-ASCII (`İ`, `ß`, final sigma) and equal to each other with different case; relationship,
note and page text with quotes, newlines, emoji and 5,000 characters; `deletion_states` rows of every
state and odd rows (a null `file_id`, no `hash`, numbers and lists as the hash); uploads of 0 to 140,000
bytes from a good path, a missing file and a directory; and `import_file` answers with every `status`.

**Result: 0 unexplained differences in 9,000 scenarios.** Everything else is one of the classes below,
counted by the harness over the three seeds.

Matched on the way, each one a difference the first runs showed: the cause of an error that `raise …
from exc` made its own cause (the harness reads the requests exception at the bottom of the chain);
the CPython 3.14 text of the unhashable-key `TypeError` (`cannot use 'list' as a dict key (unhashable
type: 'list')`); a non-string `hash` row in `deletion_states` no longer stops a delete (class W1).

## 4. What is not matched, with counts

C1, C2, C7, C8 and C9 are the 4.a classes of the same name (`docs/hydrus-client.md` section 4), seen
again through the write calls; the 4.a text says why each is kept. W1 is new.

| Class | What differs | Count | Why it is kept |
|---|---|---|---|
| **C1** library words | a `Could not reach Hydrus at {api_url}: ` message keeps its prefix; the tail is the HTTP library's own words. The cause class (timeout, connection, request refused) matches in every scenario | 808 messages (263 + 269 + 276) | the message is the library's; the class, and so what the caller does, match |
| **C2** typed fields | Python carries a JSON value along untyped; the Rust fields are typed, so the same answer is `Err(UnexpectedResponse)` with a message that says which field. Here: a `page_key` that is not a string; and, in `import_url`, 4.a's `list_tag_services` refusing a service whose `name` is not a string, where Python only raises if its loop reaches that service (`name.strip()` on a number) and otherwise goes on to the POST | 55 operations (16 + 20 + 19); in 14 of them (1 + 7 + 6) Python goes on to make a request the Rust side never makes | a Hydrus never sends these. A typed error before a write is the safe reading; 4.a's code is not changed here |
| **W1** extra dict key | `deletion_states` returns Python's dict, which also holds a key a row carried as its `hash` when that is a number or a bool; the Rust map holds string hashes only. `delete_files` is unaffected (it looks hashes up by the strings it was given) | 10 operations (2 + 6 + 2) | no caller can look a string hash up under a number. A row whose hash is a list or an object raises in both (`TypeError`, matched) |
| **C7** control character in the key | an access key holding a control character other than tab: `requests` accepts it and `http.client` sends it; the HTTP stack here cannot form such a header, so no request is made | 53 scenarios (13 + 21 + 19) | a Hydrus access key is 64 hexadecimal digits. The failure names no key |
| **C8** stale `Content-Type` after a redirect | after a 301, 302 or 303 to a POST, `requests` sends the follow-up GET without `Content-Type`; the Phase 1 net layer keeps it | 69 requests (17 + 21 + 31) | Phase 1 behaviour, not changed here; the header on a bodiless GET does nothing |
| **C9** relative `Location` | a relative `Location` against a request path with empty segments resolves to `//d` here and `/d` in `urljoin` | 1 request | Phase 1 behaviour, not changed here |

A key with a character past U+00FF is not in the differential: Python's `HTTPAdapter.send` sits above
`http.client`, which is where the `UnicodeEncodeError` is raised, so the scripted transport cannot
observe it. A one-off with real `requests` against a real local server gave
`UnicodeEncodeError: 'latin-1' codec can't encode characters in position 0-1: ordinal not in range(256)`,
the text 4.a's `with_key` returns as `UnexpectedResponse`, and `an_error_for_a_person…` asserts that no
key appears in `user_message()`.

## 5. The tests

`crates/hatate-core/tests/hydrus_client_write.rs` (45 tests) drives every function through
`HydrusClient::new` against a local server; `hydrus_client/write.rs` has 4 unit tests for the pure parts.

| Rule | Test |
|---|---|
| network-client test over the shipped scheme, plain http (the default `api_url`) | `a_write_over_the_default_plain_http_scheme_carries_the_key_and_json_headers` |
| https with verification on refuses a self-signed server, before anything is sent | `a_write_over_https_with_certificate_checks_on_is_refused_before_anything_is_sent` |
| https with verification off reaches it, for a JSON write and a raw upload | `a_write_over_https_with_certificate_checks_off_reaches_a_self_signed_server` |
| one timeout | `a_write_that_times_out_is_a_typed_net_error_after_the_configured_wait` |
| one network error | `a_write_to_an_unreachable_hydrus_names_the_api_url_and_carries_no_status`, `a_connection_failure_is_retried_once_too_as_python_retries_any_hydrus_error` |
| the error for a person carries no URL, query, key, hash, path or OS text | `an_error_for_a_person_has_no_url_query_key_hash_or_os_text` (includes a missing-file open: no path, no `os error`, no `Errno`), `the_two_new_failures_and_the_named_statuses_each_read_as_what_to_do` |

Shared 4.a tests (`https_with_checks_on_trusts_a_root_the_caller_adds`, the settings switch winning over
the net config, no 429 retry) cover the constructor these calls share.

## 6. Python tests that exercise a function in scope

Found by name over `tests/` at `76c85c4` (160 test functions name one of the functions in section 1).
Rust names are in `tests/hydrus_client_write.rs` unless a path is given.

| Python tests | Rust |
|---|---|
| `test_core_utils.py::TestHydrusDelete` (7) | `a_held_file_is_deleted_with_a_reason_after_the_state_check`, `a_hash_hydrus_never_had_is_never_sent_and_nor_is_one_already_deleted` (two tests: never had; already deleted), `only_the_held_files_of_a_mixed_selection_are_sent_in_the_order_given`, `an_empty_selection_makes_no_request_and_a_failed_state_check_deletes_nothing`, `deletion_states_tell_every_case_apart`, `undelete_posts_the_hashes_and_an_empty_list_does_nothing` |
| `test_core_utils.py::TestShowFilesInHydrus` (6) | `show_files_asks_for_a_file_search_page_holding_the_hashes_and_focuses_it` (also "not locked"), `show_files_with_no_hashes_never_reaches_the_network`, `show_files_surfaces_a_missing_permission_as_a_403_and_an_old_client_as_a_404`, `show_files_with_an_answer_that_has_no_page_key_is_not_fatal` |
| `test_hydrus_client_write_path.py::TestImportFile` (7) | `the_file_bytes_are_the_whole_request_body_and_the_answer_comes_back_verbatim`, `an_unreadable_local_file_is_refused_before_any_request`, `hydrus_answering_200_with_a_refusal_status_is_still_a_failure` (also "no note"), `the_success_statuses_and_an_answer_with_no_status_are_passed_through` |
| `...::TestAssociateUrl` (4) | `the_url_field_is_url_to_add_and_a_hash_is_a_one_element_list` (also "no hash, no key"), `an_associate_failure_reaches_the_caller` |
| `...::TestImportUrl` (10), `TestResolveToServiceKey::test_an_unreachable_hydrus_resolves_to_nothing_rather_than_raising` | `the_minimal_import_sends_the_url_and_no_page_side_effects` (also "no tags, no lookup"), `a_destination_page_is_named_only_when_asked_for`, `tags_go_under_the_resolved_hex_key_and_a_name_is_looked_up` (also "names are never sent under the keys field", "the response is returned"), `the_explicit_key_wins_over_the_setting_and_the_setting_over_my_tags` (also "empty default falls back to my tags"), `an_unresolvable_service_imports_the_file_without_the_tags` (includes the unreachable Hydrus) |
| `...::TestAddTags` (8) | `a_hex_looking_service_tries_the_keys_field_first_and_a_name_the_names_field`, `the_empty_setting_writes_to_my_tags_and_an_explicit_key_wins_over_the_setting`, `a_wrong_first_guess_is_retried_with_the_other_field_in_both_directions` (also "a name that is really a key"), `both_attempts_failing_returns_the_second_error`, `an_empty_tag_list_is_still_sent_rather_than_silently_dropped` |
| `test_hydrus_duplicate_relationships.py::TestClientWrappers` (6), `::TestErrorStatusCodes` (1) | `the_relationship_request_is_the_documented_shape_and_never_asks_for_a_deletion` (also the enum values), `a_file_cannot_be_paired_with_itself_and_a_missing_hash_is_refused_before_the_request`, `set_kings_sends_the_hashes_and_does_nothing_for_an_empty_list`, `a_relationship_403_and_404_become_the_two_messages_a_user_can_act_on`, `any_other_relationship_failure_is_passed_through_unchanged` |
| `test_hydrus_provenance_note.py::TestClientWrapper` (4) | `the_note_request_is_the_documented_shape_and_replaces_rather_than_merges` (also "replaced, not appended"), `a_missing_note_hash_or_name_is_refused_before_the_request`, `a_note_403_and_404_become_the_two_messages_a_user_can_act_on` |
| every other test that names a function in scope | does not apply here, one reason each: `TestFeatureOff`, `TestFeatureOn`, `TestDegradesGracefully`, `TestReadBackFailures`, `TestAutoImportWiring` and `TestBatchPollWorkerWiring` in `test_hydrus_duplicate_relationships.py`, `test_hydrus_provenance_note.py` and `test_url_importer_duplicate_relationships.py` test the importer and the batch poll worker that decide whether to call these (4.c, 4.d), with the client mocked; `test_hydrus_import_validation.py::TestDownloadValidation` tests the download worker's validation and mocks `import_file` (4.c); `test_search_quality.py` (`TestSendUrlFallsBackToHatatesDownload`, `TestHydrusUrlImporterGuard`, `TestExactCopiesAreUploadedWhenHydrusCantFetchThem`, `TestLegacyUrlsRewrittenForHydrus`), `test_session.py::TestHydrusSentState` and `test_outcome_tags.py::TestIdempotencyAndTransitions` test the URL importer and the session's sent state with the client mocked (4.c, 4.d); `test_gui_smoke.py` (`TestDeleteFromHydrus`, `TestHydrusReconcileAction`, `TestSendUrlToHydrusImporter`, `TestShowInHydrus`, `TestTagsAppearWhenAMatchLands`) tests Qt actions with the client mocked (Phase 6) |
