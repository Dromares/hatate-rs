# Network behaviour: what the Python app does, and what `hatate_core::net` copies

Derived from `Dromares/Hatate-Linux-Redux` at `76c85c44fea050e06ae91c77ecc5b40e2f23f986`
(`origin/main` when DAN-1980 was filed). Every `file:line` below is a path in that
repository at that commit. This is the contract for `crates/hatate-core/src/net/`
(DAN-1980, Phase 1.c of DAN-1961). It describes behaviour only; no engine or site
parser is ported here (that is Phase 2).

Config values are named by **key only**. No cookie, token, API key or Hydrus access
key value appears in this file, in a fixture or in a test.

## 1. The one Python choke point: `core/net.py`

Almost every outbound call (IQDB, SauceNAO, trace.moe, booru pages, availability
checks, Reddit, Pawchive lookup) goes through `core/net.py`. Callers that do not are
listed in section 7.

| Behaviour | Python | Rust (`hatate_core::net`) |
|---|---|---|
| One shared `requests.Session` per host (`netloc`, lower-cased), kept for the process, so connections are reused | `core/net.py:69-83` | one `reqwest::Client` (pooled per host by hyper) for the process |
| Session keeps **no** `Set-Cookie`: `DefaultCookiePolicy(allowed_domains=[])`. Each request carries exactly the cookies its caller passes | `core/net.py:25-28`, `:81` | reqwest cookie store left **off**; cookies are sent as an explicit `Cookie` header from the per-request `Cookies` value |
| Default `User-Agent` `hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)` unless the caller set one | `core/net.py:50`, `:119` | `net::USER_AGENT`, same string; set only when the request has no `User-Agent` |
| `timeout` is mandatory (keyword-only, no default) and is requests' per-connect / per-read timeout | `core/net.py:108-117` | `NetRequest::timeout` is required by the constructor; applied as connect + read timeout |
| Optional `deadline` (seconds, whole exchange). The body is read in the calling thread in 64 KiB chunks; past the deadline the connection is closed and `HardTimeoutError` raised. Per-request `timeout` is clamped to the time left | `core/net.py:57`, `:125-133`, `:184-200`; `core/hard_timeout.py:35-60` | `NetRequest::deadline: Option<Duration>`; every connect, header wait and body read is limited to what is left of it; error kind `DeadlineExceeded` |
| 429 on **GET or HEAD** is retried **once**, only when `retry_429` is true (default true) | `core/net.py:137-145` | tower `Retry` with `RetryRules` (`RetryMode::Once429`, the default), one extra attempt; `RetryMode::Never` is `retry_429=False` |
| The retry wait is `Retry-After` seconds when it is a number in `0..=10`; **2 s** when the header is absent; **no retry** when it is an HTTP date, negative, or above 10 s | `core/net.py:52-55`, `:203-211` | same constants (`RETRY_AFTER_DEFAULT = 2s`, `RETRY_AFTER_MAX = 10s`) |
| The retry is skipped when the wait would not fit in what is left of `deadline` | `core/net.py:139-140` | same |
| POST is never retried on 429 (SauceNAO does its own, section 4) | `core/net.py:137` (`method in ("get","head")`) | same: the retry policy only matches GET and HEAD |
| A host that reports `x-ratelimit-remaining` **and** `x-ratelimit-reset` is taken at its word: remaining `< 1` records `resume_at = now + max(0, reset)`; remaining `>= 1` clears it. Both headers must parse as floats, else the response is ignored | `core/net.py:162-181` | `HostGate::note_response`, run on every response (see "How the Rust stack is ordered") |
| The next request to a host with a `resume_at` in the future sleeps until it. If the wait exceeds `MAX_HOST_WAIT = 30 s` it is **refused at once** with `RateLimited`, not waited out | `core/net.py:59-62`, `:148-159` | `HostGate::wait_for_reopen`, once before the first attempt; error kind `RateLimited` |
| `close_all()` drops every pooled connection and every `resume_at` | `core/net.py:86-93` | `NetClient::close_all` clears the gate and swaps in a fresh `reqwest::Client` |
| `Response.url` is the final URL after redirects (used by Hydrus import) | `core/hydrus_import.py:249` | `NetResponse::url` |

The header names are matched case-insensitively (requests' `headers` is a
case-insensitive dict; `core/net.py:168-169` reads lower-case names).

### How the Rust stack is ordered

`NetClient::send` runs a request through a tower stack, outermost first:

1. **Admit** waits out the host's `resume_at` once (or refuses it), then stamps the
   time the deadline is measured from. This is `_wait_for_host` followed by
   `started = time.monotonic()` (`core/net.py:121-122`), so a host wait is not charged
   to the deadline.
2. **Retry** (`tower::retry`) applies the `RetryMode` of the request.
3. **Pace** runs on every attempt: it honours `min_gap` and records the response's
   rate-limit headers.
4. **Transport** is the `reqwest` exchange: per-read timeouts, the deadline, manual
   redirects, the body read.

Where this differs from the Python on purpose:

- Rate-limit headers are noted on **every** response, retries included. Python notes
  only the first response of a call (`core/net.py:136`).
- A non-finite `x-ratelimit-reset` is clamped (NaN counts as 0, anything above
  1,000,000 s as 1,000,000 s) because Python's `inf` would panic a Rust `Duration`.
  `inf` therefore refuses the host (over `MAX_HOST_WAIT`) until `close_all`, as
  Python's does.
- Cookies and credential headers are not forwarded when a redirect leaves the host
  they were attached for (section 8).

## 2. Timeouts

There is **no global HTTP timeout**. Every call passes one. The values, by call site:

| Call | Timeout | Source |
|---|---|---|
| Search engines (IQDB, IQDB 3D, SauceNAO, ascii2d, trace.moe, Google Images/Lens, Yandex, Pawchive lookup) and booru page fetches | `Settings.search_timeout`, default **30.0 s** | `core/config.py:195`; passed at `core/engine_runner.py:371-373,404,446,477,520,594,679,729` |
| IQDB / IQDB 3D | `max(search_timeout, 45.0)` | `core/iqdb.py:37`, `:72` |
| Google Lens | budget floor 90.0 s | `core/google_lens.py:108` |
| Gelbooru, Rule34, Xbooru API calls | **15 s** | `core/boorus/gelbooru.py:278`, `core/boorus/rule34.py:90`, `core/boorus/xbooru.py:88` |
| Remote image size probe (range read, first 512 KiB) | **10 s** | `core/boorus/_remote_size.py:25-26,45` |
| Pawchive creator-name lookup | **10 s** | `core/boorus/pawchive.py:72,307-310` |
| Reddit feed (`find_post`) | **15 s** default | `core/reddit.py:82` |
| Hydrus "where does this URL forward to" HEAD | **15 s** | `core/hydrus_import.py:233,244` |
| Pawchive index crawl, per request | 30 s default; creators list 120 s | `core/pawchive_index.py:406,366` |
| Hydrus API (all calls) | `HydrusSettings.timeout`, default **30.0 s**, deliberately separate from `search_timeout` | `core/config.py:157`, `core/hydrus_client.py:140,158,167` |
| A search that takes `>= 45 s` and returns nothing is not cached as a confident "not found" | `SLOW_SEARCH_THRESHOLD_SECONDS = 45.0` | `core/engine_runner.py:39` (engine layer, Phase 2) |

`hatate_core::net` takes the timeout from the caller per request; it does not
hard-code any of the values above. They are listed so Phase 2 can pass the right ones.

## 3. Pacing and concurrency

Python has **no per-booru request rate limit**. Politeness is enforced in three
places, none of which is inside `core/net.py`:

1. **Between images, per search engine** (the user-visible "delay"). Global default
   `delay_min_seconds = 45.0`, `delay_max_seconds = 75.0` (`core/config.py:194,198`).
   Each gap is `random.uniform(min, max)` (`workers/search_worker.py:430,453`).
   `engine_delays: {engine_id: [min, max]}` overrides it per engine; empty by default
   (`core/config.py:199`). Rules (`core/rate_limit.py`):
   - `engine_interval`: engines in `LOCAL_ENGINES` (only `pawchiveindex`,
     `core/engines.py:73`) get `(0, 0)`; no override, or an unparsable pair, or a
     negative number, falls back to the global pair; a reversed pair is swapped
     (`:74-95`).
   - `EngineRateLimiter.wait_needed`: the **maximum** of each engine's remaining gap,
     never the minimum; an engine never queried imposes no wait; a negative elapsed
     time counts as zero elapsed (`:118-144`).
   - `remaining_delay`: the full delay when no engine finished, `max(0, delay - elapsed)`
     otherwise, and the full delay if `elapsed < 0` (`:27-52`). Used as the fallback
     when no engine reported completion (`workers/search_worker.py:472`).
   - A result served from the search cache skips the wait (`workers/search_worker.py:423-428`).
2. **Engines in one wave run in parallel**, one thread each, up to the number of
   engines (`core/engine_runner.py:266-267`).
3. **Availability sweep**: `AVAILABILITY_SWEEP_WORKERS = 5` (`core/availability.py:91,135-137`).
4. **Pawchive index crawl** is the only polite HTTP client in the tree
   (`core/pawchive_index.py:388-430`): at least `REQUEST_DELAY = 1.0 s` between requests
   (`:57,408`), up to `MAX_RETRIES = 3` retries (`:58,407`), a 429 waits
   `Retry-After` (float) or `RETRY_WAIT = 30.0 s` when absent or unparsable
   (`:59,420-428`), a transport error is retried without extra wait (`:413-418`), and the
   pause is interruptible so Stop is honoured within 0.2 s (`:397-404`).

What the Rust layer provides for these: `NetRequest::with_min_gap` (a least gap per
host, applied on every attempt and measured from when the response arrived, like
`self._last` at `core/pawchive_index.py:414,419`), `RetryMode::pawchive()` for the
retry loop above, and `net::pacing`, the ported `engine_interval` / `remaining_delay` /
`EngineRateLimiter` as pure functions on `f64` seconds (the caller owns the clock and
the random number: `pacing::uniform(min, max, unit)`). "Interruptible so Stop is
honoured" becomes cancellation: dropping the `send` future stops the wait. Wiring all
of this to engines is Phase 2.

## 4. SauceNAO quota (shape only; the pause file belongs to 1.d)

- `API_URL = https://saucenao.com/search.php` (`core/saucenao.py:55`). The JSON API is
  used when `saucenao.api_key` is set and `saucenao.use_json_api` is true
  (`core/saucenao.py:365`); the key goes in the **query string** (`api_key`,
  `:376-380`), so the layer must never print a request URL unredacted. Without a key,
  the HTML page is scraped with the same URL and no key (`:547-571`).
- Quota is read from the JSON `header` object, four optional integers:
  `short_remaining`, `short_limit` (burst window), `long_remaining`, `long_limit`
  (daily). Values may arrive as ints or numeric strings; anything unparsable becomes
  "unknown", not an error (`core/saucenao.py:31-41,444-451`).
- Burst window: `SHORT_WINDOW_SECONDS = 30.0` (`:99`). Before each request, if the last
  reading had `short_remaining == 0`, wait `30 - (now - observed_at)` (min 0); any
  headroom waits zero; no reading waits zero (`:113-131,152-170,398`).
- HTTP 429 from SauceNAO: wait the full 30 s unconditionally, then retry **once**; a
  second 429 is an error ("slow down the search delay") (`:402-423`). This is a POST,
  so it is not the generic retry of section 1.
- HTTP 413 is an error ("too large"); any other non-200 is an error (`:419-425`).
- Daily allowance spent: `long_remaining == 0` or a negative `header.status` whose
  `header.message` (lower-cased) contains `daily search limit`, `daily limit` or
  `search limit exceeded` (`:110,458-472`). A bare 429 never counts as daily
  (`:223-237`). It resets at the next **00:00 UTC**, computed from the clock, not read
  from the API (`:248-259`).
- When a batch stops on the daily quota the app writes
  `saucenao_quota_pause.json` = `{"paused_at": float, "searched": int, "remaining": int,
  "reset_at": "<ISO-8601 +00:00>"}` (`:278-299`). The exact rules are in
  `docs/file-formats.md` section 5.5; reading and writing that file is ticket 1.d.
  `hatate_core::net` does not touch it.
- The quota counters are process-global state (`:86-105`). In Rust they belong to the
  SauceNAO engine (Phase 2), not to the net layer. The layer only guarantees that
  the `api_key` query value is redacted everywhere.

## 5. User agents

| Who | User-Agent | Source |
|---|---|---|
| Default (booru pages, IQDB, SauceNAO, trace.moe, Gelbooru, Rule34, Xbooru, Pawchive, availability, downloads) | `hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)` | `core/net.py:50` |
| Pixiv, Sankaku, DeviantArt/`fav.me`, ascii2d, Google Images | Chrome 120 on Linux: `Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36` | `core/site_access.py:31,52,68`; `core/ascii2d.py:37-40`; `core/google_images.py:65-68` |
| Yandex | same with Chrome **140** | `core/yandex.py:56-59` |
| Reddit and `redd.it` | `linux:hatate-linux:1.0 (reverse image tagger)` | `core/reddit.py:31`, `core/site_access.py:105-109` |
| e621 / e926 / e6ai | the default UA plus `Authorization` (e621 blocks generic agents) | `core/site_access.py:116-118` |

## 6. Site access: headers, cookies, credentials (`core/site_access.py`)

Host matching is on the URL's **hostname**: equal to the domain or ending in
`.` + domain, lower-cased; a malformed URL matches nothing. Substring matching was a
security bug (it handed a login to `anime-pictures.net.evil.example`)
(`core/site_access.py:368-381`).

**Headers** (`headers_for_url`, `:92-119`; first match wins):

| Host | Headers |
|---|---|
| `pixiv.net` | `User-Agent` (Chrome 120), `Referer: https://www.pixiv.net/`, `Accept: application/json` (`:30-37`) |
| `sankakucomplex.com` | `User-Agent` (Chrome 120), `Referer: https://chan.sankakucomplex.com/` (`:51-57`) |
| `deviantart.com`, `fav.me` | `User-Agent` (Chrome 120), `Referer: https://www.deviantart.com/` (`:67-73`) |
| `reddit.com`, `redd.it` | `User-Agent` only (Reddit's) |
| `e621.net`, `e926.net`, `e6ai.net` | default `User-Agent` + `Authorization: Basic base64(user:key)`, **only** when settings are given and **both** `e621_username` and `e621_api_key` are non-empty after trimming (`:76-89,110-118`) |
| anything else | none (caller falls back to the default UA) |

**Cookies** (`cookies_for_url`, `:250-286`), each only when the user configured it:

| Host | Config key | Cookie sent |
|---|---|---|
| `pixiv.net` | `pixiv_session_cookie` (trimmed, non-empty) | `PHPSESSID=<value>` (`:273-274`) |
| `sankakucomplex.com` | `sankaku_cookies` | parsed `name=value` pairs; `None` if nothing parses (`:275-277`) |
| `anime-pictures.net` (incl. `api.`) | `animepictures_cookies` | parsed pairs; a lone token with no `;`, no `=` and no whitespace becomes `anime_pictures_jwt=<token>` (`:168-200,278-279`) |
| `deviantart.com`, `fav.me` | `deviantart_cookies` | parsed pairs; `None` if nothing parses (`:283-285`) |

`parse_cookie_string` (`:122-152`): `\r` and `\n` count as `;`; split on `;`; each part is
trimmed; parts without `=` or with an empty name are dropped; split on the **first**
`=` only (base64 and JWT values contain `=`); name and value trimmed; a later duplicate
name wins.

**DeviantArt cookie rotation** (`capture_rotated_deviantart_cookies`, `:289-365`): the
shared session keeps no cookies (section 1), so DeviantArt's sliding session renewal is
captured by hand. Only for `deviantart.com`/`fav.me`; only for a 2xx or 304 response (a
challenge page's `Set-Cookie` must never overwrite a working session); only the names
`auth`, `auth_secure`, `userinfo` with non-empty values; nothing happens when no session
is configured; nothing is saved when every value is unchanged; otherwise the new values
are merged over the configured pairs and re-joined as `k=v; k=v`. The log line carries
names only, never values (`:364-365`). The Rust port is the pure merge; the
`config.json` save is ticket 1.d.

Not ported in this ticket: `cookie_paste_warnings` (`:203-247`, advice shown while the
user pastes a cookie in the settings dialog), which belongs with the GUI.

## 7. Everything that does not go through `core/net.py`

These use `requests` directly. The Rust layer gives them the same client; the
differences that matter are kept as per-request options.

| Caller | What differs | Source |
|---|---|---|
| ascii2d | fresh `Session` per search, `allow_redirects=True`, hard timeout in a thread | `core/ascii2d.py:114-125` |
| Google Images (public) | `Session` with `Accept-Language: en-US,en;q=0.9`, `Referer: https://www.google.com/`, and a `SOCS` consent cookie on `.google.com` | `core/google_images.py:433-462` |
| Google Images (Vision API) | plain `requests.post` to `https://vision.googleapis.com/v1/images:annotate` | `core/google_images.py:64,254` |
| Yandex | `Session`, `Referer: https://yandex.com/images/`, two calls (upload, then results page) | `core/yandex.py:268-300` |
| Pawchive index | own `Session`, the polite client of section 3 | `core/pawchive_index.py:365,441` |
| Hydrus client | `requests.get/post` to the user's own `api_url` (default `http://127.0.0.1:45869`); header `Hydrus-Client-API-Access-Key` when `access_key` is non-empty; `verify=HydrusSettings.verify_ssl` (default true) so a user with a self-signed Hydrus can turn certificate checks off; all failures are re-raised as one "could not reach Hydrus" error | `core/hydrus_client.py:127-173`, `core/config.py:153-157` |
| Settings dialog test button | `requests.get(timeout=20)` | `gui/settings_dialog.py:1726` |

## 8. Redirects, TLS, proxies

- **Redirects.** `requests` follows up to 30 redirects for GET and POST by default and
  **does not follow them for HEAD** unless `allow_redirects=True` is passed. The calls
  that pass it: `core/availability.py:570`, `core/remote.py:137`,
  `core/hydrus_import.py:245`, `core/ascii2d.py:122`, `core/google_images.py:457`. Both
  HEAD callers in the tree pass it. The Rust layer has `Redirects::{Default, Follow,
  Stop}` per request: `Default` follows for GET and POST and stays put for HEAD. It
  follows up to 30 hops (`NetConfig::max_redirects`), turns a POST into a GET on 301,
  302 and 303 (dropping the body) and repeats it on 307 and 308.
- **Credentials do not leave their host.** `Cookie` and any header marked secret
  (`Authorization`, `Proxy-Authorization`, `Hydrus-Client-API-Access-Key`, or one set with
  `NetRequest::secret_header`) are dropped from a redirect hop whose host differs from the
  host of the original request, and from any https-to-http hop. The URLs the app fetches
  come from search results, which are not trusted (`core/site_access.py:368-381`). Cookies
  built by `net::site::cookies_for_url` carry a scope (`pixiv.net`; `deviantart.com` and
  `fav.me`; and so on) that widens this to the site's own domains, so `fav.me` to
  `deviantart.com` still works. Ordinary headers (`Referer`, `User-Agent`) travel.
- **TLS.** Python verifies against `certifi`'s bundle (requests default) for every host
  except Hydrus, where `verify_ssl` decides (section 7). Rust: rustls through reqwest's
  `rustls-no-provider` build, so reqwest uses the process-wide `CryptoProvider` and
  panics if there is none (the rv-rs failure, DAN-1909). `net::install_crypto_provider`
  installs `ring` once and is called by `NetClient::new`; a second install is harmless.
  Server certificates are checked by the platform verifier (the system trust store, which
  on Linux is what distributions ship, the same Mozilla lineage as certifi).
  `NetConfig::with_root_certificate_der` adds a trust anchor and
  `NetConfig::without_tls_verification` is the Hydrus `verify_ssl = false` switch.
- **Proxies.** There is **no proxy setting** in `Settings` and no `proxies=` argument
  anywhere (`git grep -n -i proxy -- core gui workers` finds only a comment at
  `core/engine_runner.py:38` and a docstring at `core/tracemoe.py:136`). `requests`
  honours `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` from the environment
  (`trust_env=True`). `reqwest` does the same by default, so the Rust layer adds no
  proxy field and leaves the system-proxy behaviour on.
- **Compression.** `requests` decodes `gzip` and `deflate`; the Rust client enables
  those two and nothing more.

## 9. What is secret, and how the Rust layer treats it

Config values that reach the wire, **by key name only**:

| Key | Where it ends up |
|---|---|
| `saucenao.api_key` | query string `api_key` (`core/saucenao.py:376-380`) |
| `hydrus.access_key` | header `Hydrus-Client-API-Access-Key` (`core/hydrus_client.py:129-131`) |
| `e621_username`, `e621_api_key` | header `Authorization: Basic ...` (`core/site_access.py:84-89`) |
| `pixiv_session_cookie`, `sankaku_cookies`, `animepictures_cookies`, `deviantart_cookies` | `Cookie` (`core/site_access.py:273-285`) |
| `mcp.token` | not a net-layer secret (it protects the local MCP server, `core/config.py` `McpSettings`) |

Rust rules, all tested with synthetic values (`crates/hatate-core/tests/net_redaction.rs`):

1. Secrets live in `Secret` / `Cookies` values whose `Debug` prints `[redacted]` (cookies
   print their **names** only, as the Python log does at `core/site_access.py:364-365`).
   They have no `Display` and no `Serialize`.
2. Error values never carry a URL with its query or userinfo: `NetError` stores a
   redacted URL (scheme, host, port and path only). `reqwest::Error` is stripped with
   `without_url()` before it is wrapped.
3. `tracing` events carry the host and path, never the query, a header value or a cookie.
4. `Debug` of `NetRequest`, `NetClient` and `NetConfig` redacts the `Authorization`,
   `Cookie`, `Proxy-Authorization` and `Hydrus-Client-API-Access-Key` headers and
   every secret-typed field.

## 10. Where each rule is tested

All local; no live site is contacted. TLS certificates (a throw-away CA and a leaf for
`127.0.0.1`) are generated by `tests/common/mod.rs` on every run; nothing is committed.

| Rule | Test |
|---|---|
| A real TLS request completes through the constructor the binaries use (DAN-1909) | `tests/net_https_local.rs::real_tls_request_completes_through_the_binaries_constructor` |
| The client builds in a fresh process with no provider installed | `tests/net_fresh_process.rs` (one test, so its own process; it asserts `CryptoProvider::get_default()` is `None` before and `Some` after) |
| Second install is harmless | `src/net/provider.rs::second_install_is_harmless` |
| Untrusted CA, wrong name, `verify_tls = false` | `tests/net_https_local.rs` (the other three tests) |
| Host `x-ratelimit-*` wait, 30 s refusal, junk headers, `inf` | `src/net/stack_tests.rs` (exact, paused clock); `tests/net_http_behaviour.rs` (measured) |
| 429 retry once, wait values, HEAD, POST, `Never`, deadline fit | `src/net/stack_tests.rs`; `src/net/retry.rs`; `tests/net_http_behaviour.rs` |
| Pawchive pattern (3 retries, 30 s default, 1 s gap) | `src/net/stack_tests.rs::polite_*`; `tests/net_http_behaviour.rs::polite_mode_retries_a_429_up_to_three_times` |
| Minimum gap, per host, concurrent | `src/net/stack_tests.rs::min_gap_*`; `tests/net_http_behaviour.rs::min_gap_spaces_requests_by_at_least_the_gap` |
| Per-read timeout; deadline stops a trickling body | `tests/net_http_behaviour.rs::a_server_that_never_answers_times_out`, `::the_deadline_stops_a_body_that_trickles_in` |
| Redirects: follow, HEAD, POST to GET, loop limit, credentials stay on their host | `tests/net_http_behaviour.rs` |
| `between-search` arithmetic (`rate_limit.py`) | `src/net/pacing.rs` tests |
| Site headers, cookies, host matching, rotation | `src/net/site.rs` tests |
| Secrets are redacted in `Debug`, logs and errors | `tests/net_redaction.rs` (synthetic values, each with a positive control that the secret was really sent) |
