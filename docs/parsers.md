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
  cache and remote image sizes), passed to every hook as its last argument. The corpus records the same state as
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
- **`int(x)` in Python and `pycompat::int`.** `pycompat::int` reads a string as `int()` does
  (`sizes::py_int`: surrounding whitespace, a sign, any script's decimal digits, single
  underscores), since DeviantArt's `int(original.get("width") or 0)` is applied to whatever the
  page holds. It still stops at `i64`: an integer past it is "unknown" here. No corpus item
  reaches that.

- **`get_text(separator)` without `strip`** is `parsed::text_joined`: the same strings as
  `text_of`, unstripped and joined with the separator. `parsed::page_text` is the whole document
  (`soup.get_text(" ")`). Moebooru's rating and size scrapes and rule34us's size line use them.
- **`urllib.parse`.** Python's `urljoin`, `urlparse(...).query`, `parse_qs` and `unquote` are
  `crate::pyurl`, not the `url` crate, because the two disagree on the inputs a broken page
  produces (a `src` with no scheme, a bare `?`, a stray `%`). `tests/pyurl_oracle.rs` holds
  CPython 3.14.8's answers for those inputs as data. CPython 3.14 changed `urljoin` (it keeps an
  empty `?` or `#`, and no longer splits `;params`); the port follows 3.14.
- **Numbers.** Python's `int` is unbounded and the ports use `i64`: a width past `i64::MAX`
  reads as absent. Paheal's `int(str(attr))` is `sizes::py_int` (sign, surrounding whitespace,
  single underscores, any script's decimal digits).
- **Where Python raises** on a shape the site never sends (e621's `post` that is a string, a
  `file` that is a list) the Rust hook returns what `fetch_page_info` would make of it: nothing.

- **`str.casefold()`, `re.IGNORECASE` and `\b`** (e-shuushuu's description matching, DAN-2005).
  `regex`'s `(?i)` and `\b`, and `str::to_lowercase`, are not Python's: `(?i)` does not let `i`
  match `ı` or `İ`, `\w` counts combining marks, `to_lowercase` leaves `ß` and `ς`, and the crate
  has no lookahead. `crate::pycase` holds Python's rules (`casefold`, the first-character
  `lower`/`upper` that `_sre` uses, `re._casefix._EXTRA_CASES`, `\w` as letters, numbers and `_`),
  and a parser that needs them matches by hand over characters. The two tables in it were read
  off CPython 3.14.8 (Unicode 16.0); `std` is newer, so a character added to Unicode after 16.0
  is the one place they can differ (54 unassigned-in-16.0 code points and two capitals added
  for existing small letters, found by comparing every code point). The one `(?i)` regex that
  stays (`eshuushuu::DIMENSIONS_RE`) spells out the letters whose Python cases `(?i)` lacks.
- **A field read unchecked.** Python hands a JSON field through as it is: Zerochan's `full` and
  `width`, MangaDex's `baseUrl` and `hash` (an f-string prints any value). The ports read the
  type the API sends. A `full` that is not a string, a `width` that is not a JSON integer, and a
  `baseUrl` or `hash` that is not a non-empty string give nothing, where Python returns the value
  or prints it into the URL. Zerochan's `tags` is walked as Python walks it (a string by
  character, an object by key; a number raises, which is no tags). MangaDex's `dataSaver` is
  indexed as Python indexes it (a string by character; an object that is reached, or a number,
  raises, which is no pages). None of these is a shape the API sends.

A new difference found by a later port is written down here, with the corpus item that exposed
it, and then matched. No item is allowed to fail instead.

- **A request the parser used to make itself** (Pawchive, DAN-2007). Python's `pawchive.parse`
  fetches the creator's profile on a cache miss, and `parse_dimensions` reads the first bytes of
  the original over a Range request. Neither is here. The module says what is needed
  (`pawchive::artist_profile_request`, `pawchive::dimensions_request`), the engine client does
  the fetch, and the answer goes into `Context` under Python's own cache key
  (`Context::remember_artist`, `Context::remember_image_size`) before the hooks run. A hook that
  finds its entry missing answers as Python does when the request fails: no artist tag,
  `(None, None)`. The 512-entry and 1024-entry LRU bounds on those two caches, the timeouts and
  the user agent belong to the client; a failed request is simply not remembered.
- **`str(x)` and `repr(x)` of a JSON value** (`crate::pyrepr`). An f-string key
  (`f"{service}/{user}"`), `str(author_id)` and `str(t).strip()` over a tag list print whatever
  the API sent: `True`, `None`, `1e+16`, `['a']`. `py_str` and `py_repr` print what CPython 3.14
  prints, with `str.isprintable()` read off CPython (Unicode 16.0) as a range table. This replaces
  the private copy Anime-Pictures had.
- **`csv.reader`** (`crate::pycsv`). Pawchive's tag list is a Postgres array literal read with
  `next(csv.reader(StringIO(text), quotechar='"', escapechar="\\"))`. `first_record` is
  `_csv.c`'s state machine (3.14) for that dialect, including its odd corners: only the first
  record is read, a quoted field may span lines, a backslash before the end of the input is a
  newline, a bare `\r` inside an unquoted field and a field over 131072 characters are
  `csv.Error` (no tags). A plain `split(',')` is wrong on all of them.
- **`urllib.parse.quote`** is `pyurl::quote` (default `safe="/"`, UTF-8, upper-case hex).
- **Python's `\s` and `$` in a hand-written regex** (DeviantArt's state line). Python's `\s`
  counts U+001C..U+001F and the `regex` crate's does not, so the state regex spells
  `[\s\x1c-\x1f]`; Python's `$` also matches before one final `\n`, so the "id at the end of the
  URL" pattern ends `\n?\z`.
- **A state of an unexpected shape.** DeviantArt's `deviation` table that is a list, a `types`
  entry whose `t` is a list, a `state` that is the number 5: Python raises, and the hook has
  nothing to say (`[]`, `None`, `(None, None)`). Pawchive's `attachments` that is the number 5
  is the same. `types` is a dict keyed by `t`, so a later entry of the same name replaces an
  earlier one.

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
