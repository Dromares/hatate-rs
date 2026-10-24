# The engine runner, ranking and tag rules (Phase 3.f, DAN-2063)

Python source: `core/engine_runner.py`, `core/ranking.py`, and from `core/tag_rules.py` the five
functions the runner and `search_image` call (`apply_namespace_remap`, `apply_tag_blacklist`,
`apply_outcome_tags`, `countable_tags`, `with_rating_tag`) with the helpers they use
(`is_tag_blacklisted`, `split_tag_text`, `outcome_tags_for`). `apply_inline_edit` and its result type
stay in Phase 4 with the review table that calls them.

## Where the cut is

| Rust | Python |
|---|---|
| `engine_runner::{engine_waves, planned_engines, fallback_is_unnecessary, no_match}` | `_engine_waves`, `planned_engines`, `_fallback_is_unnecessary`, `_no_match` |
| `EngineRunner::run_engines_parallel` | `_run_engines_parallel`, `_run_engine_isolated`, `_run_engine` |
| `EngineRunner::collect_*`, `append_candidate`, `guess_host`, `EngineRunner::retry_could_help` | `_collect_*`, `_append_candidate`, `_guess_host`, `retry_could_help` |
| `ranking` | `core/ranking.py` entire |
| `tag_rules`, `pyfnmatch` | the functions above; `fnmatch.fnmatchcase` |
| `sites::classify_site` | `core/sites.py` `classify_site` (the site lists are still Phase 4) |

The wave loop itself is `search_engine.search_image` (3.h, DAN-2064): it asks `engine_waves`, asks
`fallback_is_unnecessary` before the last wave when there is more than one, and calls
`run_engines_parallel` once per wave. Nothing here owns it.

## The engines are behind a trait

The collectors call each engine through `engine_runner::Engines`, the Rust form of "call
`iqdb.search(...)`, read `ascii2d.is_blocked()`". `engine_backends::LiveEngines` is the shipped
implementation over the Phase 3 clients; a test (or the differential) implements the trait with
scripted answers. The collectors are the Python collectors: the same error prefix per engine, the
same slow-search rule (an empty answer that took 45 s or more is an error, not a confident
NOT_FOUND), the same notes for an engine that stood down.

`run_engines_parallel` runs each engine of a wave of two or more on its own tokio task. A panic is
that engine's failure with the panic message, as Python's `except Exception` around
`future.result()` makes it. A wave of one engine returns `Err(Raised)` for what Python lets
propagate: an exception that is not the engine's own error type (`YandexError::Unexpected`,
`GoogleImagesError::Unexpected`, `PawchiveError::HardTimeout`, or a panic). `on_engine_finished` is
told either way.

## What a build without a Lens browser answers

The Google Lens engine half is `docs/google-lens.md` (Phase 5.a), the browser `docs/lens-browser.md`
(5.b) and `docs/lens-search.md` (5.c). `LiveEngines::new` has a `LensClient` over `NoBrowser`, whose
`LensFetch` answers `LensBrowserUnavailable`; `LiveEngines::for_profile` (what `hatate batch` builds)
puts the real browser there. A build without a browser (and a machine without a Chromium) answers
as Python does when the engine cannot run:

* The pacing `wait_for_turn` runs before every search that is not blocked (45 s between starts,
  requested of the injected clock). The search reads the file first (`Could not read image: [Errno 2]
  ...` if it cannot), runs `prepare_for_lens` (a file Python refuses as "not in a format Google Lens
  accepts" is refused here with the same text, and does not latch), and then raises
  `GoogleLensUnavailableError`, which latches the engine. The first image gets
  `Google Lens: <message>`; later images, until `reset_blocked_flag`, get
  `Google Lens: not searched (it stood down earlier this run)`. The message is
  `LENS_UNAVAILABLE_MESSAGE`. In Python it is the text of the missing-dependency error for the
  interpreter, which has no counterpart here, so the differential scripts Python with the same
  text. `announce_unavailable` (the once-per-session GUI alert) goes to `engine_alerts`.
* `LensError::Unexpected` is what Python lets propagate from `google_lens.search`; the runner returns
  `Raised` for it, as for the other engines.

The Pawchive index was in this list until Phase 4.h: `LiveEngines::for_profile` now searches the
profile's `pawchive_index.db` (`docs/pawchive-index.md` section 7). A `LiveEngines::new` with no
index handed in answers as an index with nothing in it: `index.is_empty()` is true, so
`_collect_pawchive_index` returns False with no error and without reading the file.

## Differences that are not differences found

The differential (`worktrees/DAN-2063-differential.log`) found none in its cases. What it cannot
show, by construction:

* Log lines. Python's `logging` and `tracing` are not compared.
* The text of an exception Python raises on input the services never send (`UnexpectedInput`): the
  Rust text is fixed (see `docs/parsers.md`), Python's is that of whatever it raised
  (`AttributeError: ...`). It reaches the user only as `"<engine>: <text>"` on a row.
* `rank_candidates` with a NaN similarity: Python's `sorted` on NaN keys is order-dependent;
  Rust treats NaN as equal. No engine produces one.
* The order candidates arrive from a wave of two or more engines is the order the engines finish,
  in both languages. The differential fixes it by giving each scripted engine a distinct delay.
