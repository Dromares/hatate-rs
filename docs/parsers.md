# Site parsers: the pattern

Derived from `Dromares/Hatate-Linux-Redux` at `76c85c44fea050e06ae91c77ecc5b40e2f23f986`
(`core/boorus/`). This is the contract for `crates/hatate-core/src/boorus/` (DAN-2000,
Phase 2.a of DAN-1961) and the pattern every later site port copies. `danbooru.rs` is the
worked example.

## The pattern

- **One module per site**, named after the Python module (`core/boorus/danbooru.py` is
  `boorus/danbooru.rs`). The module's doc comment says what the site's page or API gives and why
  the parser reads what it reads.
- **Pure functions.** Every hook is a plain `pub fn` taking the body and the post-page URL as
  `&str` and returning owned values. No socket, no file, no global. A parser that needs the
  fetch is told what to fetch by `resolve_fetch_url`, which returns a URL and fetches nothing.
- **Explicit context.** State that Python keeps in a module global is a field of
  `boorus::Context` (the Gelbooru family's memoised Data-API post, Pawchive's creator-name
  cache), passed to every hook as its last argument. The corpus records the same state as
  `context` in `call.json`. A site that needs none ignores the argument. The Gelbooru family's
  `api_post` is the Data-API post already matched to the URL, or `None` for "the API gave
  nothing"; the fetch that fills it, and Gelbooru's 401 latch, are the HTTP client's. rule34 and
  Xbooru keep their own module (and so their own latch and memo) in Python for that reason; the
  hooks themselves read the same thing, so they call Gelbooru's.
- **A `static PARSER: SiteParser`** naming the hooks the module defines; every hook the Python
  module does not define stays `None`. `SiteParser::new(name, matches, parse)` fills the
  required two, and struct-update syntax adds the rest.
- **Registered in `boorus::PARSERS`**, in the order Python's `PARSERS` list has: the first
  `matches` wins, so the order is behaviour.
- **The target is what Python returned at the pinned commit, not the live site.** A parser that Phase 0.d
  marked `drifted` or `dead` still reproduces what Python returned. A live-site fix is a note
  for a later ticket, not part of the port.

What stays out of a site module: HTTP, retries, cookies, the file-hash cache and
`_remote_size.py`'s socket. `boorus::page_info_from_body` is the part of `fetch_page_info` that
runs once the body is in hand.

## Reading a page

`boorus::parsed` is the one way into a body:

- `soup_of(html)` parses with html5ever through `scraper`; Python used bs4 with lxml.
- `json_of(body)` is `serde_json::from_str`; Python used `json.loads`. An `Err` means "this is
  not JSON", which several hooks read as the signal to fall back to HTML.
- `text_of(el)` is bs4's `get_text(strip=True)`, `text_joined(el, sep)` is bs4's `get_text(sep)` (the
  Danbooru 1.x sidebar scrapes read the whole page this way) and `classes_of(el)` is bs4's
  `get("class")`. Use them rather than `scraper`'s own text and class iterators, which differ (see below).
- `pycompat::strip`, `pycompat::split` give Python's whitespace rules for `str.strip` and
  `str.split()`.
- `pycompat::html_unescape` is `html.unescape`, for a site that reads its text with regular
  expressions and not through a tree (JoyReactor). It decodes legacy no-semicolon references
  such as `&amp` and remaps numeric ones as CPython does, which a browser-style decoder does
  not; the named table comes from `markup5ever`, the crate `scraper` already builds on.
- `boorus::json_fields` holds the `int(...)`, size, dimension and absolute-URL guards the
  JSON-API sites (Sankaku, Anime-Pictures) share.

### Where html5ever and bs4/lxml differ

The corpus decides. These are the differences the helpers already absorb, and the ones nobody
has needed yet:

- **Text under `<script>`, `<style>`, `<template>`, and comments.** bs4 leaves them out of
  `get_text()`; `text_of` does the same, whatever tag is being read.
- **Class lists** split on Python whitespace (which includes `\x1f`), not on ASCII whitespace.
- **`json.loads` accepts what `serde_json` rejects:** `NaN`, `Infinity`, `-Infinity` and lone
  surrogate escapes. A body using them is "not JSON" here. No corpus item reaches this.
- **`int(x)` in Python reads what `pycompat::int` does not:** digit separators (`"1_000"`),
  non-ASCII digits, and integers past `i64`. A record using them gives "unknown" here. No corpus
  item reaches this.

A new difference found by a later port is written down here, with the corpus item that exposed
it, and then matched. No item is allowed to fail instead.

## Registering a port in the corpus runner

`crates/hatate-core/tests/corpus_diff.rs` runs every item of every ported parser.

1. Add `tests/corpus_ports/<site>.rs` with a `pub const PORT: Port`, mapping each dotted Python
   callable the site's items use (`core.boorus.<site>.parse_rating`, ...) to a closure calling
   the Rust function. `Call::body()` is the input, `Call::url()` is `extra_args[0]`, and
   `Call::context`/`kwargs` carry the rest. `ok(value)` serialises the result.
2. Add `pub mod <site>;` to `tests/corpus_ports/mod.rs`.
3. Add `&corpus_ports::<site>::PORT` to `PORTS` in `corpus_diff.rs`.
4. If an item `raises`, add the Python class and the Rust error variant to `ERROR_VARIANTS`.

The runner fails an item for a missing input file, a hash that differs from `MANIFEST.json`, or a
callable in `call.json` that the port does not map. There is no skip list. The coverage test
prints the corpus parsers that have no port yet; it asserts nothing, and each port shrinks it.
