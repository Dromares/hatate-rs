# corpus

A language-neutral golden corpus for the Rust port of Hatate's site parsers and search-engine
result parsers (DAN-1965, Phase 0.a of DAN-1961). Python source of truth:
`Dromares/Hatate-Linux-Redux` at commit `76c85c44fea050e06ae91c77ecc5b40e2f23f986` (`76c85c4`).

Each item is a recorded input plus what the Python code returns on it. The Python app is a black
box here: its test fixtures are the inputs and its output is the oracle. `corpus/formats/` (DAN-1966)
is separate and covers file formats; this README covers `corpus/boorus/`, `corpus/engines/` and
`corpus/MANIFEST.json`.

**Nothing here comes from a live site or a real profile.** Every input is a fixture that already
sits, public, in the Python repo's `tests/`. The Python repo keeps no fixture files; its fixtures are
inline strings and JSON-shaped literals in the test modules, so the "source fixture" of an item is a
named constant (`tests/test_boorus_harness.py::E621_BODY`) or, for bodies the tests build in a
helper, the call site (`tests/test_parsers_and_tags.py:1045 (_danbooru)`).

## Layout

```
corpus/MANIFEST.json                         one record per item (schema below)
corpus/boorus/<parser>/<NNN>/                one item per distinct call, one dir per parser module
corpus/engines/<module>.<callable>/<NNN>/    same, for engine result parsers (a leading `_` is dropped from the callable)
                                             `kind: "mocked_transport"` items (DAN-2001) hold a whole engine call, see below
    input.html | input.json | input.xml | input.txt     first argument, byte-identical to the fixture
    call.json                                the rest of the call: extra args, kwargs, state, callables
    expected.json                            what Python returned, serialised as below
```

`.gitattributes` marks these directories `-text`. Bodies carry CRLF, tabs and trailing whitespace
exactly as in the Python source, and git must not normalise them. The test in `hatate-core` hashes
the raw bytes, so a normalised checkout fails it.

## What an item means

`input.*` is the first positional argument. If the Python fixture was a `str`, the file is its UTF-8
bytes with nothing added. If the fixture was a `dict`/`list`/`None` (the engines that take a decoded
payload), the file is that value in the canonical JSON below.

`call.json` holds everything else the Python callable needed:

| Key | Meaning |
|---|---|
| `callables` | Dotted Python names run on this input, in sorted order. Every public `parse*` hook of the booru module (plus `incomplete_reason`) is run on every booru item, not only the hooks the Python test happened to call. Engine items run the one callable that was recorded. |
| `extra_args` | Positional args after the input. For boorus this is `[url]`, the post-page URL the parser sees. |
| `kwargs` | Keyword args (`base_url` for IQDB, `min_similarity` for trace.moe). |
| `context` | Module state the callable reads. `api_post` (gelbooru, safebooru, rule34, xbooru): the Data-API post for this URL, or `null` for "the API returned nothing", which forces the HTML path. `artist_cache` (pawchive): creator names already looked up. |
| `skipped_network_callables` | Hooks that tried to open a socket on this input and are therefore not in `callables` or `expected.json`. They need a network fixture, which this corpus does not have. |

The Python modules keep `api_post` and `artist_cache` in process-wide globals that the Python tests
seed. Here they are explicit inputs. Where the test left the memo empty, the real code would go to
the network, so the corpus fixes `api_post` to `null`. That is a definition of the input, not a
recording of what the test saw.

## Serialisation rules for `expected.json` and `call.json`

1. UTF-8, no BOM, one trailing `\n`.
2. Two-space indent, **object keys sorted** (by Unicode code point), `ensure_ascii` off: non-ASCII
   characters are written as themselves, control characters use JSON escapes.
3. No `NaN` or `Infinity` (the generator would fail); none occur.
4. Integers are written bare. Floats use Python's shortest round-trip `repr` (`0.5`, `98.6`,
   `1e-05`). A Rust consumer should compare parsed numbers, not text.
5. Python tuples and sets become arrays; sets are sorted by their serialised form. A tuple and a
   list are not distinguishable in the corpus.
6. Dataclasses become objects of their fields in declaration order (then sorted, per rule 2).
   Enums become their `.value`. A `Tag` is `{"name", "namespace", "source"}`, with `source` the enum
   value (`"Booru"`).
7. `expected.json` is `{"results": {"<dotted callable>": {"returns": <value>} | {"raises": "<ExcClass>"}}}`.
   `raises` records the exception class name only; messages are not compared.
8. `None` is `null`. `bytes` never occurs in a result.

Each item was generated twice and compared before it was written.

## MANIFEST.json

Top level: `python_repo`, `python_commit`, `item_count`, `items`. Each item:

| Key | Meaning |
|---|---|
| `id` | `<parser>-<NNN>`, unique |
| `family` | `boorus` or `engines` |
| `item_kind` | only on `mocked_transport` items (below); absent means a plain parser item |
| `parser` | Python module short name |
| `dir` | item directory, relative to the repo root |
| `input`, `call`, `expected` | file names inside `dir` |
| `input_sha256`, `call_sha256`, `expected_sha256` | SHA-256 of the raw file bytes |
| `source_fixture` | the named constant or call site the input came from |
| `source_fixture_is_named_constant` | `true` for a constant, `false` for a call site |
| `also_used_by` | up to 5 other test call sites that fed the same input |
| `python_commit` | `76c85c4…` |
| `python_callables`, `skipped_network_callables` | as in `call.json` |
| `phase_0d_status`, `phase_0d_note` | below |

`crates/hatate-core/tests/corpus_manifest.rs` checks that every entry's files exist and hash to the
recorded values, that no item directory is missing from the manifest, and that `item_count` and
`python_commit` are right. That puts the corpus under CI.

### Phase 0.d status

`phase_0d_status` is the DAN-1964 premise-probe result (probe document rev 2, 2026-10-22) for the
**site or engine the parser reads**, one of `live-match`, `drifted`, `dead`, `not-probed`. It is a
per-parser fact with one exception: an item whose URL carries a fixture id that the probe listed as
dead is marked `dead`. Most inputs are small recorded shapes (a few tags, a few URLs), not full live
pages, so a `drifted` parser does not mean its fixture is wrong: it means the live site no longer
returns what the fixture encodes, and a port that matches the Python output is matching a shape the
site stopped serving. `phase_0d_note` names the probe row.

`not-probed` means the probe produced no usable shape evidence, and the note says why: Zerochan
answered 503 throughout; SauceNAO, ascii2d and IQDB sit behind a challenge or need an upload;
trace.moe's remote fetch failed; Google Images and Lens were out of scope.

## Mocked-transport items (DAN-2001)

Some engine code parses inline with the HTTP call, so the Python tests feed it a body through a mocked
HTTP layer instead of calling a parser. Those items are `corpus/engines/<module>.<callable>/<NNN>/`
with `call.json` `"kind": "mocked_transport"`, and the manifest item carries `"item_kind":
"mocked_transport"`. The callable is a whole engine call (`core.saucenao.search`,
`core.reddit._feed_entries`, `core.ascii2d.search`, `core.iqdb.search`, `core.iqdb.search_3d`,
`core.pawchive_lookup.lookup`, `core.yandex.search`, `core.google_images.search`).

| File | Meaning |
|---|---|
| `input.*` | the body of the **last** scripted response, the one the parser reads. A JSON body is the mock's `.json()` value in the canonical JSON above (`null` is a valid body: the mock answered `.json()` with `None`); a text body is its UTF-8 bytes, byte-identical. |
| `call.json` `args` | the Python call's own arguments. `image_path` is always `"<image>"` (the engine reads the file only to upload it, and the replay stubs that read); `settings` is the `SauceNaoSettings` the test passed (`__dataclass__` tag as in the tag-helper items); `timeout`, `sha256`, `subreddit`, `feed` as called. |
| `call.json` `request` | the requests the Python code made, in order, as seen at the outermost transport layer it called (`layer`: `net` = `core.net.get/post`, `session` = `requests.Session.get/post`, `requests` = `requests.post`): `method`, `url`, `params`, `file_fields` / `data_fields` (field **names** only, never values), `json_body`, `has_headers` (header **names** only), `timeout`, `allow_redirects`. |
| `call.json` `responses` | the scripted answers, in order. Each has `status` and optionally `url` and `headers`; the last has `"body": "input"` and `body_kind` (`json` or `text`); earlier ones carry their `text` / `json` inline. `json_unset: true` means the test's mock gave `.json()` no usable value (an unconfigured `MagicMock`); a replay must make `.json()` fail, and the expected result shows the code never needed it. |
| `call.json` `state_before` | always the fresh-start sentence: every module-level counter, latch and cache is at its import-time value. A recorded call whose module state had been preset by an earlier step of the test (a spent burst window, a prior failure count, a latched block) is **not** an item; a fresh replay would not reproduce it. |
| `expected.json` | `{"results": {"<callable>": {"returns": <value>} \| {"raises": "<ExcClass>", "message": "<str(exc)>"}, "effects": {...}}}`. `message` is recorded for these items because the text is user-facing behaviour; a Rust port may compare it. |
| `expected.json` `effects` | `sleeps`: burst-window waits the code asked for (seconds, via `saucenao._sleep_interruptibly`, which the replay stubs); `state_after`: the module state the call left behind (SauceNAO quota reading and daily-limit latch; whether the reddit feed was cached; ascii2d, Yandex and Google Images block latches and Yandex's failure counter). |

Replay rules the generator used, and a Rust harness should copy: the transport is replaced at **every**
layer by one script (a request that arrives at a layer or URL other than the recorded one fails the
replay, as does a response left unused), uploads read nothing from disk, and no socket is opened.

Two deliberate normalisations:

- **API keys.** The tests use the dummy key `"k"`. Every `api_key` / `key` value, in `args`, in
  `params` and in the request URL, is the placeholder `"<api-key>"`. The replay used the placeholder
  itself, so the items reproduce from what is on disk; the key's value never affects a parse.
- **Wall-clock text.** Yandex's captcha message embeds the local time the rest ends ("resting until
  06:04"). It is written `resting until <HH:MM>` in `expected.json`, and a replay should compare it so.

## Reproducing

The generator is not in this repo (Rust-only tree) and not in the Python repo. It was a throwaway
that (1) ran the Python tests at `76c85c4` with every parser entry point wrapped to log the arguments
of each top-level call, (2) grouped the calls into distinct items, (3) wrote each item to disk, and
(4) re-read each item from disk and ran the real Python callables with sockets disabled to produce
`expected.json`. The exact command and the environment are in the PR description. The Python
callables ran on CPython 3.14.8 with beautifulsoup4 4.15.0, lxml 6.1.3, soupsieve 2.10 and
requests 2.34.2.

## Coverage and gaps

Per-parser counts are in the PR descriptions (DAN-1965, DAN-2001). All 20 booru parsers have items.
Nine of the 10 engines in `core/engines.py` have items (IQDB and IQDB 3D share one parser); the tenth,
the Pawchive index, is not a Phase 2 gap (below). DAN-2001
closed the engines whose parsing is inline with the HTTP call, as `mocked_transport` items (above),
43 in all: `saucenao.search` 14, `google_images.search` 10, `yandex.search` 5, `pawchive_lookup.lookup` 5,
`reddit.feed_entries` 4, `ascii2d.search` 3, `iqdb.search` 1, `iqdb.search_3d` 1.

What is left, and why:

| Gap | Why |
|---|---|
| SauceNAO **HTML** result page (`_search_html`) | every `saucenao.search` test passes `use_json_api=True` and a JSON dict; no Python test feeds the HTML path a body. Only the JSON API path has items |
| ascii2d bovw (`/search/bovw/`) result variant | one `kind` per fixture; no test feeds a `kind="bovw"` body |
| `tracemoe.search` | no Python test calls it with a mocked body (`parse_results` is the only thing tested, and it has items) |
| `google_lens.search` | drives a browser (`lens_browser`), not a mocked HTTP layer; its pure parsers already have items |
| `remote.py` fetch helpers, `availability.py` probes | not engines; they read headers / status of a remote file, outside the parser corpus |
| calls whose module state a test had preset (a spent burst window, a latched block, a prior failure count) | a fresh replay would not reproduce them, so they are not items (5 recorded calls skipped for this); 2 more were skipped because the call took a callable argument other than `on_tick`, or was rejected before any request was made |
| `pawchive.parse`, `pawchive.parse_dimensions`, `redditpost.parse_dimensions` on some items | need a network lookup (creator profile, remote file header); listed per item in `skipped_network_callables` |

The Pawchive index (`core/pawchive_index.py`) is **not** a Phase 2 gap: it crawls, hashes images with
`dhash` and keeps a SQLite index, so it moves to Phase 4 with image compare.
