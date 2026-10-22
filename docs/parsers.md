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
- **Raw JSON values.** Pixiv's `parse_file_url`, `parse_preview_url` and `parse_dimensions` return
  whatever JSON held at `urls.original`, `urls.regular`, `width` and `height` in Python. Here a
  URL is a string and a dimension is a whole number (`100.0` counts); any other type reads as
  absent, and a non-string `regular` falls through to `small`. Pixiv sends neither. Twitter's
  `_as_int` is `int(v)`, so it is ported as that (`json_fields::int_of`, which also reads
  `"1_0"` and non-ASCII digits in a string).
- **Non-JSON bodies and big integers reach Pixiv, Twitter and the Reddit feed too** (found by the
  DAN-2034 differential, 14,853 generated cases). A body with `NaN`, `Infinity` or a lone-surrogate
  escape (`"a\ud800"`) parses in `json.loads` and not in `serde_json`, so the hook reads it as
  non-JSON. A `width`, `height` or `pageCount` of 2^63 or more is a Python `int`; here it reads as
  absent (dimensions) or as one page (`pageCount`). The sites send neither. No corpus item
  reaches either.
- **`<meta id="meta-preload-data">` inside `<noscript>`.** lxml (Python) finds it; `scraper` does
  not. Only Pixiv's legacy HTML fallback reads that meta, and the real one sits in `<head>`. `<template>`, `<svg>`,
  `<textarea>`, comments, unclosed tags and a BOM matched.
- **`sizes::py_int` and U+001C to U+001F.** `pycompat::strip` is `str.strip()`, which removes
  those four characters; `int()` does not (`int("\x1c5")` is `ValueError`), so `py_int` reads `5`
  where Python raises. It is reached through `json_fields::int_of` (Twitter `width` and `height`,
  Pixiv `pageCount`) when the JSON holds a string. Documented, not fixed: the sites send plain
  digits, and the fix is a behaviour change for its own ticket.
- **Where Python raises on a shape the site never sends** (a Pixiv `tags` that is a list, a tag
  that is not an object, a `userName` that is a number, a Twitter `author` that is a list) the
  hook gives what `fetch_page_info` makes of a raised hook: no tags, no URL, `(None, None)`, one
  page.
- **`re.IGNORECASE`.** Python's `[a-z]` under IGNORECASE also matches U+0130 and U+0131; `(?i)`
  in the `regex` crate does not. Reddit's URL patterns are the ones affected. No corpus item
  reaches it. `$` is written `\n?\z`, because Python's `$` also matches before a final newline.
- **A hook that needs a socket takes the read as an argument.** `redditpost::parse_dimensions`
  calls Python's `_remote_size.image_size` for a file on `i.redd.it`; here the caller passes that
  read (`remote_size(file_url, user_agent)`). The registered hook in `PARSER` answers from
  `Context::remote_image_sizes`, which `boorus::fetch` fills by the ranged read (Phase 3.b,
  `docs/network-halves.md`). The corpus runner still passes a read that fails the item if it is
  called, and `tests/corpus_net_replay.rs` runs those items over https.
- **Engine code that is half parse, half request** (`core/reddit.py`) is split the same way:
  `reddit::feed_entries(status, text)` is what `_feed_entries` does once the response is in hand,
  `reddit::resolve_listing` takes the feed lookup as an argument, and the GET, the session
  cache and the loop over the feeds are `reddit::{fetch_feed_entries, find_post,
  resolve_listing_online}` (Phase 3.b). The runner's `Call::response_status` and
  `Call::call` carry the `mocked_transport` items' recorded response.

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

## Engine result parsers (`engines/`, DAN-2008)

`crates/hatate-core/src/engines/` holds what an engine's answer says once it is in hand:
`iqdb`, `ascii2d`, `tracemoe`, `pawchive_lookup` (the exact-hash lookup, not the booru parser
`boorus::pawchive`) and `saucenao`. The corpus family is `engines`, one runner unit per Python
callable (`core.iqdb._parse_results`, `core.saucenao.search`, ...).

- **Where Python's engine functions are cut.** A Python `search` uploads, waits, reads the
  answer and interprets it. The interpreting is here, as a pure function of the response: the
  status to error mapping (413, 429, anything but 200), the check that an HTML page is the
  engine's and not a block page, the JSON header checks, and the parse. `iqdb::search_from_response`,
  `ascii2d::search_from_responses`, `tracemoe::search_from_response`,
  `pawchive_lookup::lookup_from_response` and `saucenao::search_from_response` are those halves.
  The request half is Phase 3: reading and downscaling the file, the multipart body and headers,
  the deadline, network errors (`RequestException`, `HardTimeoutError`, `OSError`), IQDB's 45 s
  floor on the timeout, SauceNAO's burst-window wait and its one retry after a 429, and every
  module global.
- **A module global becomes a return value.** `ascii2d._blocked` is `Ascii2dError::Blocked` (the
  client sets the latch); SauceNAO's `_last_quota` and `_daily_limit_reported` are
  `JsonAnswer::quota` and `JsonAnswer::daily_limit_reported`, and the client keeps the latch and
  the observation time. A second request that depends on the first answer (ascii2d's feature
  search, at `ascii2d::bovw_url`) is a second argument, and a failed one is passed as `None`.
- **A `mocked_transport` item compares the last response.** The runner compares only `returns` or
  the error class. The port feeds the last scripted response's status and body to the pure half
  (for SauceNAO's 429-then-200 item that is the answer to the retry), and each port's own test
  holds the recorded error message, request address, sleep count and `state_after` to what the
  port gives.
- **Shapes the API never sends.** Python reads some fields unchecked and raises something other
  than the engine's error. Where that would be a whole search failing, SauceNAO's port returns a
  `SauceNaoError` naming the field (a result that is not an object, a `similarity` that is not
  a number, an `ext_urls` entry that is not text, a `status` that is not a number). Elsewhere a
  field is read as the type the API sends and anything else is absent: a trace.moe or Pawchive
  `title`, `image`, SauceNAO `title`, `index_name`, `thumbnail` or `source` that is not text, a
  trace.moe `anilist` that is a boolean (Python reads it as an integer id and prints `True`), a
  non-text member of a list `characters` (skipped), and a trace.moe or Pawchive body that is not
  an object (no matches). Integers past `i64` read as absent (an IQDB or ascii2d width).
- **`re.IGNORECASE` on "similarity".** Python folds `s` with U+017F and `i` with U+0130 and
  U+0131; `(?i)` here does not, so IQDB's percent pattern spells the word out. `\s` is
  `[\s\x1c-\x1f]` as above.
- **IQDB's tables are selected by their ancestors.** `div#pages table` and `td.image a` are
  matched as bs4 does, with the ancestor allowed to sit outside the element searched, and class
  lists split on Python whitespace (`parsed::classes_of`). html5ever and lxml build different
  trees from a `<table>` that opens inside an unclosed `<tr>`; both agree on well-formed markup,
  which is all IQDB serves and all the corpus holds.
- **`float()` of page text** is `sizes::py_float` (SauceNAO HTML similarity, IQDB's percent):
  any script's digits, single underscores between digits, `inf` and `nan`.
- **`sites::guess_site_name`** (`core/sites.py`) is ported with its `HOST_MAP` because IQDB labels
  each match with it; `classify_site` comes with the code that uses it. `canonicalize_url` is here too, for Yandex
  (DAN-2009).

## Engine result parsers: Yandex and Google Images (DAN-2009)

`crates/hatate-core/src/engines/` holds what an engine's answer says once it is in hand
(`yandex.rs`, `google_images.rs`). The request half is Phase 3 and nothing here opens a socket.
What `search` does with each answer is here too (DAN-2054, below); the corpus items for
`core.yandex.search` and `core.google_images.search` are `mocked_transport` items and replay
through those halves.

- **Where Python raises on an input the service never sends.** The engine code reads JSON it did
  not check: `site["thumb"]` that is a list, a `url` that is a number, Vision's `responses` that
  is a string. Python raises, the exception leaves `search`, and the engine runner reports the
  engine as failed. That is not "no results", which would be recorded as a search that found
  nothing and cached. So `parse_sites`, `keep_post_pages`, `post_url`, `parse_results` and
  `parse_vision_results` return `Result<_, engines::UnexpectedInput>`, and the Phase 3 client maps
  the `Err` to its own failure. `parse_lens_results` cannot raise and returns a plain `Vec`.
  The one case Python does *not* raise on and `Err` stands in for anyway: `keep_post_pages` with an
  `originalImage.url` that is neither a string nor `null` and no non-empty `thumb.url` (Python keeps
  the value in `thumb_url`; a `YandexMatch` cannot hold it).
- **`urlparse` and `urljoin` raise `ValueError`.** CPython 3.14 refuses an unbalanced or invalid
  bracketed host and a non-ASCII netloc that NFKC normalisation turns into one with `/ ? # @ :`.
  Yandex's `post_url` and Google's `host_label` catch it; Google's `_resolve_href` does not, so one
  such link makes `parse_results` raise, and `find_parser` (every site's `matches` is `on_host`,
  and Danbooru's is asked first) raises out of `post_url`. `pyurl::try_urlparse`,
  `pyurl::try_urljoin` and `boorus::host::raises_value_error` say when. `urlparse` and `urljoin`
  themselves are unchanged and still read such a URL, for the booru hooks that skip it. The
  IPv6 test is `ipaddress.IPv6Address`'s algorithm written out (scope id, embedded IPv4, at most
  one `::`); NFKC is `icu_normalizer` (Unicode 17 data, CPython 3.14.8 has 16.0), already in the
  lockfile through `url`.
- **`re.IGNORECASE` again.** `i` also matches `ı` and `İ` in Python (and so does `[a-z]`), which
  `(?i)` does not give; `POST_URL_RE`, the Danbooru mirror pattern, the reactor pattern and the
  Google domain pattern spell them out. `ſ` and the Kelvin sign `(?i)` already has.
- **`$` and lookahead.** `$` is `\n?\z`. `REACTOR_PICS_POST_RE`'s `(?!full/)(.+)$` has no
  lookahead to land on, so `sites::canonicalize_url` reads the rest by hand (and drops one final
  newline, as `$` leaves it out of the group).
- **`isalpha()`** (the Lens title test) is general category `L`, so it is `\p{L}`, not
  `char::is_alphabetic` (which counts marks and number letters).
- **JS string literals in a Lens blob.** `"((?:[^"\\]|\\.){2,400})"` ports as written (`.` stops at a newline in both, and a
  literal over 400 characters is not matched in either). `_unescape` turns `\uXXXX` into a character
  by hand. A surrogate escape (`\ud83d`) is a lone surrogate in Python and cannot be a Rust
  `char`; it becomes U+FFFD, and a pair is two of them, where Python keeps two surrogates.
- **html5ever and libxml2 (lxml) differ on two shapes Google's markup does not use.** A heading
  inside a `<p>` closes the `<p>` in html5ever and does not in libxml2, and a `</a>` across an open
  `<h3>` closes differently. Python's `parse_results` finds the anchor by the heading under it, so
  `<p><a href><span><h3>` is a result in Python and not here. Found by the differential run
  below; no corpus item has either shape. A page built from `div`s, tables and well-formed anchors
  (the SERP in the Python tests, 300+ random ones per 6,000 cases) matches.
- **`json.loads`** (Yandex's `data-state`): `NaN`/`Infinity` and lone surrogates are "not JSON"
  here (as above), an integer past `u64` is a float here and exact in Python, and nesting past
  serde_json's 128 levels is "not JSON" where Python parses deeper (and raises `RecursionError`
  further down). `int(width)` stops at `i64`, as before.
- **Measured against Python.** The corpus has 19 items for these five callables. Beyond them, a
  throw-away generator (outside the repo: a Python script and a Rust test) ran `post_url`,
  `canonicalize_url`, `parse_sites`, `keep_post_pages`, `parse_results`, `parse_lens_results`,
  `parse_vision_results`, `is_google_host`, `host_label`, `urlparse` and `post_url_for_image` on
  100,000 random inputs against the real modules at `76c85c4` (CPython 3.14.8, bs4 4.15, lxml),
  comparing values, and "raised" with "returned an error" (17,284 cases Python raised on). The only
  differences are `parse_results` on markup with a heading inside a `<p>`, a `</a>` across an open
  heading, or characters deleted from tags (1,601 of 8,840 such cases); the 5,169 well-formed
  `parse_results` pages and every other function had none.

## Google Lens parsers (`engines/google_lens.rs`, DAN-2010)

`parse_lens_payload`, `parse_exact_matches` and `parse_exact_tiles` of `core/google_lens.py`:
pure functions of the response body or of the tile list the page script returns. The rest of the
module (`search()`, the upload script, `prepare_for_lens`, the rest latch, `rule34_searches`,
`taggable_first`, `keep_taggable_first`) needs the browser or the booru registry and is Phase 5.

- **A tile is a JSON value.** `parse_exact_tiles` takes `&serde_json::Value`: Python's
  `tiles or []` and `isinstance(tile, dict)` become "an array, whose object entries are read".
  `str(tile.get("text") or "")` is `pyrepr::py_str` of a truthy value, so a number in `text` is
  its `repr`.
- **Code points, not bytes.** The 2,500-character window of `_label_is_near` and the 3..200
  length of a title are counted in `char`s. `str.isalpha` is `pyalpha::is_alpha` (the L
  categories, not Rust's `Alphabetic`, which also takes `Ⅷ` and combining marks).
- **`re.IGNORECASE`.** The image half of `_IMAGE_OWNERS` spells `s` and `i` as classes taking
  U+017F, U+0130 and U+0131, as Python does (see `pycase`). `$` is `\n?\z` (Python's `$` also
  matches before a final newline).
- **Surrogate escapes.** `\uD800`..`\uDFFF` in a literal is a lone surrogate in Python and
  U+FFFD (one code point) here. No corpus item reaches it.
- **The thumbnail check decodes the image.** `_is_placeholder_thumb` base64-decodes the data URI
  as `binascii.a2b_base64` does for a `str` (non-ASCII is an error; characters outside the
  alphabet are skipped; padding never ends the data; a bad length is an error) and asks
  `pyimage::image_size` for the size, as `Image.open(...).size`. **GIF** is Pillow's
  header walk exactly; **PNG** (CRC of every chunk before the first `IDAT`, the colour
  type/bit depth table), **JPEG** (marker walk to `SOS`, 8 bit, 1/3/4 components), **BMP** (the DIB
  header, bit depth, compression, bitfield masks, palette size) and **WebP** (RIFF bounds, the
  first chunk's size field) read the header and the checks that stand between it and the size.
  Later damage (PNG filters, JPEG tables past `SOS`, a WebP's bitstream) is not looked for, and
  other formats Pillow can open (TIFF, ICO, ...) read as "not an image". Where Python's
  `Image.open` raises `DecompressionBombError` (over 178 million pixels) the whole search fails;
  here the size reads as "not an image" and the thumbnail is kept.
- **Google hosts.** `is_google_host` and `host_label` are `google_images`' (DAN-2009), which
  Python imports from `core/google_images.py` too. A literal whose netloc `urlparse` refuses
  (`http://[bad/`) is skipped, as `host_label(...) or ""` skips it.
- **Differential (not in the repo).** About 80,000 generated bodies and tile lists (zerochan
  image/page pairs, escape forms, every Google host shape, label windows at 2,500, unicode digits
  in ids, GIF/PNG/JPEG/WebP/BMP thumbnails of 1 to 40 pixels with truncations and bit flips,
  mutated base64) were run through Python 3.14.8 with Pillow 12.3 and through the port: no
  difference outside the documented classes above.

## Yandex and Google Images: what `search` does with the answers (DAN-2054)

`yandex::upload_from_response` and `yandex::results_from_response` are the two answers
`yandex.search` reads (the upload's, then the "sites" page), and `google_images::public_from_response`
and `google_images::vision_from_response` are the two routes of `google_images.search`, picked by
`google_images::vision_key`. Each takes the answer as `engines::Response` (or a status and the
decoded JSON for Vision) and opens nothing.

- **Where the cut is.** In Phase 2 (here): the captcha and consent-wall tests, the HTTP status
  mapping (413, the other non-200, Vision's 400/401/403/429 and reasons), the per-image error in a
  Vision 200, the Lens-or-classic choice, the JavaScript-shell test, the rest-until text, and the
  hand-off to the parse functions. In Phase 3 (not here): reading and downscaling the picture,
  the two Yandex requests (`SEARCH_URL`, `UPLOAD_REQUEST`, the `cbir_id`/`cbir_page`/`url`
  parameters) and the Google upload and Vision body, headers and consent cookie, the deadline and
  network errors, `reset_blocked_flag` at the start of a search, the pacing between Yandex
  searches, and the local clock.
- **Module globals are an argument or a return.** Yandex's `_blocked`, `_consecutive_failures`
  and `_resting_until` are `yandex::YandexState`, passed `&mut`; the rest ends at
  `now + CHALLENGE_REST_SECONDS` on the caller's monotonic clock, and the `HH:MM` the message
  prints is passed in (`RestClock`), because the port reads no clock. Google's `_blocked` is
  `GoogleImagesError::latches()` on the error (`Blocked` and `JavaScript`), as ascii2d's is
  `Blocked`.
- **Python's exception classes** are `YandexError::{Failed, Blocked}` and
  `GoogleImagesError::{Failed, Blocked, JavaScript}`. `Unexpected` is where Python raises
  something else out of `search` (`AttributeError`, `KeyError`, `TypeError`): a Vision payload that
  is not an object, an `error` or `message` or `status` that is truthy and not the type read, a
  `responses` that is not a list of objects, a Yandex `blocks` that is a non-empty object. The state
  is as Python left it at that point. One case Python does not raise on and `Unexpected` stands in
  for: a Yandex `cbirId` that is truthy and not a string (a number, a list, an object), which
  Python formats into the next request.
- **Checks read a prefix.** Yandex's and Google's challenge tests read the first 6000 characters,
  the JavaScript shell test the first 20000 (characters, not bytes), lower-cased.
- **Differential (not in the repo).** 4,000 generated answers (Yandex upload/page pairs with every
  shape of `blocks` and `cbirId`, markers at and past the prefix, three starting failure counts;
  Google public answers over status, final URL, marker and padding; Vision payloads over status
  and the shapes of `error`, `message`, `status` and `responses`) were run through the real
  `search` at `76c85c4` with the transport scripted, and through the port: no difference in
  result, error class, message or state, outside the non-string `cbirId` class above (155 cases,
  skipped).

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
