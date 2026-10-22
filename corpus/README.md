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

## Reproducing

The generator is not in this repo (Rust-only tree) and not in the Python repo. It was a throwaway
that (1) ran the Python tests at `76c85c4` with every parser entry point wrapped to log the arguments
of each top-level call, (2) grouped the calls into distinct items, (3) wrote each item to disk, and
(4) re-read each item from disk and ran the real Python callables with sockets disabled to produce
`expected.json`. The exact command and the environment are in the PR description. The Python
callables ran on CPython 3.14.8 with beautifulsoup4 4.15.0, lxml 6.1.3, soupsieve 2.10 and
requests 2.34.2.

## Coverage and gaps

Per-parser counts are in the PR description (DAN-1965). All 20 booru parsers have items. Of the 10
engines in `core/engines.py`, eight have items (IQDB and IQDB 3D share one parser). SauceNAO has only
its pure tag helper, and the Pawchive index has none. The reddit feed resolver (`core/reddit.py`) is
not an engine and has none. For these, parsing is inline with the HTTP call and the Python tests
mock the HTTP layer instead of passing a body. They are gaps, not omissions:

| Gap | Why |
|---|---|
| SauceNAO result page and JSON API | `_search_html` / `_search_json` parse inside the request; only `_tags_from_result` is a pure function (5 items) |
| Pawchive index (`core/pawchive_index.py`) | its tests build images and indexes in memory; there is no HTML/JSON body fixture |
| Reddit feed resolver (`core/reddit.py`) | `_feed_entries` parses `net.get(...).text` inline; the Python tests mock `net.get` |
| ascii2d bovw (`/search/bovw/`) result variant | one `kind` per fixture; no `kind="bovw"` body in the tests |
| `pawchive.parse`, `pawchive.parse_dimensions`, `redditpost.parse_dimensions` on some items | need a network lookup (creator profile, remote file header); listed per item in `skipped_network_callables` |
