//! Yandex reverse image search (`core/yandex.py`): the results page and the choice of what to
//! keep.
//!
//! Yandex has no API. Python posts the picture, gets a `cbirId`, and fetches the "sites" tab
//! for it; the page is server-rendered and its results are embedded as JSON in a `data-state`
//! attribute (`initialState.cbirSites.sites`): page URL, domain, a thumbnail and the size of the
//! copy that page holds. [`parse_sites`] reads that block; [`keep_post_pages`] keeps only the
//! results on a post page of a site that has a parser (Pinterest pins, wallpaper hosts and
//! listing or tag-search pages carry nothing to read tags from) and scores them by position.
//!
//! `search` is cut in two at the two answers it reads. [`upload_from_response`] is the upload's
//! answer (a captcha, or the `cbirId`) and [`results_from_response`] the "sites" page's (a
//! captcha, no results block, or the results). The module state Python keeps in globals
//! (`_blocked`, `_consecutive_failures`, `_resting_until`) is [`YandexState`], an argument.
//! The request half (reading and re-encoding the picture, the two requests and their headers,
//! the deadline, the network errors, `reset_blocked_flag` at the start of a search, the pacing
//! between searches) is [`YandexClient`] (Phase 3.e, DAN-2061).

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use tracing::{debug, error};

use super::client::{
    Failure, OnTick, multipart_upload_as, read_upload, response_text, ticking, urlencode,
};
use super::session::{Session, SessionRequest};
use super::{Response, UnexpectedInput};
use crate::boorus;
use crate::clock::{Clock, SystemClock};
use crate::hard_timeout::HardTimeoutError;
use crate::image_prep;
use crate::net::{Method, NetClient};
use crate::pycompat;
use crate::pyresponse::PyResponse;
use crate::pyurl;
use crate::sites::canonicalize_url;

/// Level with Google Lens's start: same kind of engine, same kind of guess. Below ascii2d's
/// colour search, so neither overturns a measured IQDB or SauceNAO match on a position alone.
pub const SIMILARITY_START: f64 = 80.0;
pub const SIMILARITY_STEP: f64 = 2.0;
pub const SIMILARITY_FLOOR: f64 = 50.0;
pub const MAX_RESULTS: usize = 8;

/// Where both requests go. The first is the upload (`rpt=imageview&format=json`, the picture as
/// the `upfile` part, `request` = [`UPLOAD_REQUEST`]), the second the "sites" tab for the id.
pub const SEARCH_URL: &str = "https://yandex.com/images/search";
/// What the upload endpoint is asked for. Without it the answer is a full HTML page rather than
/// the small JSON that carries the `cbirId`.
pub const UPLOAD_REQUEST: &str = r#"{"blocks":[{"block":"b-page_type_search-by-image__link"}]}"#;

/// `USER_AGENT`.
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
/// The `Referer` every request carries.
pub const REFERER: &str = "https://yandex.com/images/";
/// Yandex matches a downscaled picture as well as the original, and a 40MB PNG off a network
/// share is a long upload for nothing.
pub const UPLOAD_MAX_SIDE: u32 = 1600;
pub const UPLOAD_JPEG_QUALITY: u8 = 90;
/// The least time between the starts of two Yandex searches, however they were started.
pub const MIN_SECONDS_BETWEEN_SEARCHES: f64 = 10.0;

/// Failures in a row before the engine stands down for the run. A page layout change fails every
/// image the same way, and one bad answer is not evidence of anything.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// After a challenge, how long before Yandex is asked again.
pub const CHALLENGE_REST_SECONDS: f64 = 30.0 * 60.0;

/// Where a SmartCaptcha shows up: a redirect to `/showcaptcha`, or the challenge served in place
/// of the answer.
const CHALLENGE_MARKERS: [&str; 3] = ["showcaptcha", "smartcaptcha", "checkcaptcha"];

/// One result worth a row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct YandexMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub source_name: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// A post page, as opposed to a listing, a tag search or a profile. Python compiles it with
/// `re.IGNORECASE`, under which `i` also matches `ı` and `İ` and `[a-z]` matches them too; `(?i)`
/// has neither, so they are spelled out (`ſ` and the Kelvin sign, which Python also adds, `(?i)`
/// already has). `$` is `\n?\z`, since Python's also matches before one final newline.
static POST_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)",
        r"[?&][i\u{131}\u{130}]d=\d+",
        r"|/posts?/(?:show/|v[i\u{131}\u{130}]ew/)?\d+",
        r"|/posts?/[0-9a-z\u{131}\u{130}]{8,}(?:[/?#]|\n?\z)",
        r"|/artworks/\d+|[i\u{131}\u{130}]llust_[i\u{131}\u{130}]d=\d+",
        r"|/status(?:es)?/\d+",
        r"|/art/[^/?#]+-\d+",
        r"|/comments/[0-9a-z\u{131}\u{130}]+",
        r"|zerochan\.net/\d+",
    ))
    .expect("post url regex")
});

/// Danbooru's mirrors (`^(?:[a-z0-9-]+\.)?donma[i\u{131}\u{130}]\.(?:us|moe)$`, `re.IGNORECASE`): Yandex indexed
/// some posts under them, and only the main host has a parser.
static DANBOORU_MIRROR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[a-z0-9\-\u{131}\u{130}]+\.)?donma[i\u{131}\u{130}]\.(?:us|moe)\z")
        .expect("danbooru mirror regex")
});

static DATA_STATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"data-state="([^"]*)""#).expect("data-state regex"));
static UTM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[?&]utm_[a-z]+=[^&#]*").expect("utm regex"));

/// The "sites" results embedded in a results page, as Yandex gives them. `None` when the page
/// has no results block at all, which is not the same as a block with nothing in it (`Some` of an
/// empty list). A `data-state` that does not parse as JSON is skipped, as Python skips it.
///
/// `Err` where Python raises `AttributeError`: a block that parses to something that is not an
/// object (`5`, `[]`, `null`), or an `initialState` that is neither an object nor empty.
pub fn parse_sites(page: &str) -> Result<Option<Vec<Value>>, UnexpectedInput> {
    for m in DATA_STATE_RE.captures_iter(page) {
        let raw = &m[1];
        if !raw.contains("cbirSites") {
            continue;
        }
        let Ok(state) = serde_json::from_str::<Value>(&pycompat::html_unescape(raw)) else {
            continue;
        };
        let Value::Object(state) = state else {
            return Err(UnexpectedInput);
        };
        // `(state.get("initialState") or {}).get("cbirSites")`
        let block = match state.get("initialState") {
            None => None,
            Some(initial) if !pycompat::truthy(initial) => None,
            Some(Value::Object(initial)) => initial.get("cbirSites"),
            Some(_) => return Err(UnexpectedInput),
        };
        if let Some(Value::Object(block)) = block {
            return Ok(Some(match block.get("sites") {
                Some(Value::Array(sites)) => sites.clone(),
                _ => Vec::new(),
            }));
        }
    }
    Ok(None)
}

/// The results worth a row: a post page on a site with a parser, at most [`MAX_RESULTS`] of them,
/// the same post once, in Yandex's order.
///
/// Scored by position among the ones kept, not among everything Yandex returned: the Pinterest
/// pins ahead of a booru post say nothing about how good the post is.
///
/// `Err` where Python raises on a value it reads without checking: a truthy `url` that is not a
/// string, a `thumb` or `originalImage` that is truthy and not an object, a `thumb.url` that is
/// truthy and not a string, or an `originalImage.url` that is neither a string nor `null` (Python
/// would carry it through; a [`YandexMatch`] cannot).
pub fn keep_post_pages(sites: &[Value]) -> Result<Vec<YandexMatch>, UnexpectedInput> {
    let mut matches: Vec<YandexMatch> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for site in sites {
        let Some(site) = site.as_object() else {
            continue;
        };
        // `post_url(site.get("url") or "")`
        let raw_url = match site.get("url") {
            Some(v) if pycompat::truthy(v) => v.as_str().ok_or(UnexpectedInput)?,
            _ => "",
        };
        let Some(url) = post_url(raw_url)? else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let thumb = match site.get("thumb") {
            Some(v) if pycompat::truthy(v) => v.as_object().ok_or(UnexpectedInput)?.get("url"),
            _ => None,
        };
        let thumb: Option<String> = match thumb {
            Some(v) if pycompat::truthy(v) => {
                let s = v.as_str().ok_or(UnexpectedInput)?;
                Some(match s.strip_prefix("//") {
                    Some(_) => format!("https:{s}"),
                    None => s.to_string(),
                })
            }
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        let original = match site.get("originalImage") {
            Some(v) if pycompat::truthy(v) => Some(v.as_object().ok_or(UnexpectedInput)?),
            _ => None,
        };
        let field = |name: &str| original.and_then(|o| o.get(name));
        // `thumb or original.get("url")`
        let thumb_url = match thumb {
            Some(t) if !t.is_empty() => Some(t),
            _ => match field("url") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(UnexpectedInput),
            },
        };
        let similarity =
            (SIMILARITY_START - matches.len() as f64 * SIMILARITY_STEP).max(SIMILARITY_FLOOR);
        let source_name = host_label(&url)?;
        matches.push(YandexMatch {
            url,
            thumb_url,
            similarity,
            source_name,
            width: field("width").and_then(pycompat::int),
            height: field("height").and_then(pycompat::int),
        });
        if matches.len() >= MAX_RESULTS {
            break;
        }
    }
    Ok(matches)
}

/// The canonical post URL, or `None` if this is not a post page on a site the booru parsers can
/// read.
///
/// `Err` where Python's `boorus.find_parser` raises `ValueError` out of `urlparse` (see
/// [`boorus::host::raises_value_error`]), which `post_url` does not catch.
pub fn post_url(url: &str) -> Result<Option<String>, UnexpectedInput> {
    // Yandex tags every outbound link with its own utm_ parameters.
    let mut url = UTM_RE.replace_all(pycompat::strip(url), "").into_owned();
    if !url.contains('?') && url.contains('&') {
        url = url.replacen('&', "?", 1);
    }
    let Ok(mut parts) = pyurl::try_urlparse(&url, "") else {
        return Ok(None);
    };
    if DANBOORU_MIRROR_RE.is_match(&parts.netloc) && parts.netloc != "danbooru.donmai.us" {
        parts.netloc = "danbooru.donmai.us".to_string();
        url = pyurl::urlunparse(&parts);
    }
    if !POST_URL_RE.is_match(&url) {
        return Ok(None);
    }
    let url = canonicalize_url(&url);
    if boorus::host::raises_value_error(&url) {
        return Err(UnexpectedInput);
    }
    Ok(boorus::find_parser(&url).map(|_| url))
}

/// The host of `url`, lower-cased and without a leading `www.`, or `None` when it has none. `Err`
/// where `urlparse` raises `ValueError`, which Python does not catch here.
fn host_label(url: &str) -> Result<Option<String>, UnexpectedInput> {
    let netloc = pyurl::try_urlparse(url, "")
        .map_err(|_| UnexpectedInput)?
        .netloc
        .to_lowercase();
    let host = netloc.strip_prefix("www.").unwrap_or(&netloc);
    Ok((!host.is_empty()).then(|| host.to_string()))
}

/// The module state `core/yandex.py` keeps in globals and the searches read and write: the
/// stand-down latch (`_blocked`), the failures in a row, and when the rest after a captcha ends
/// (`_resting_until`, on the caller's monotonic clock, in seconds). A fresh state is the
/// import-time one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct YandexState {
    pub blocked: bool,
    pub consecutive_failures: u32,
    pub resting_until: Option<f64>,
}

impl YandexState {
    /// `is_resting`: whether a captcha rest is still running at `now`.
    pub fn is_resting(&self, now: f64) -> bool {
        self.resting_until.is_some_and(|until| now < until)
    }

    /// `is_blocked`: stood down, or resting.
    pub fn is_blocked(&self, now: f64) -> bool {
        self.blocked || self.is_resting(now)
    }

    /// `reset_blocked_flag`, called whenever a search starts: clears a stand-down, but not the
    /// rest after a challenge.
    pub fn reset_blocked_flag(&mut self) {
        self.blocked = false;
        self.consecutive_failures = 0;
    }

    /// `end_rest`: lets Yandex be asked again now.
    pub fn end_rest(&mut self) {
        self.resting_until = None;
    }
}

/// What a captcha message needs of the clock. Python reads it where it builds the message; the
/// port is handed it.
#[derive(Debug, Clone, Copy)]
pub struct RestClock<'a> {
    /// `time.monotonic()`, in seconds.
    pub now: f64,
    /// The local clock time [`CHALLENGE_REST_SECONDS`] from now, as `%H:%M` (`resting_until()`).
    pub rest_ends_at: &'a str,
}

/// `YandexError` and its subclass `YandexBlockedError`, plus the case where Python raises
/// something else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YandexError {
    /// `YandexError`: a failure with its user-facing message. The state counted it.
    Failed(String),
    /// `YandexBlockedError`: a captcha (the state is resting) or the stand-down after
    /// [`MAX_CONSECUTIVE_FAILURES`] (the state is latched).
    Blocked(String),
    /// Python raised out of `search` on an answer the service never gives; see
    /// [`UnexpectedInput`]. The state is as Python left it when it raised.
    Unexpected(UnexpectedInput),
}

impl std::fmt::Display for YandexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YandexError::Failed(message) | YandexError::Blocked(message) => f.write_str(message),
            YandexError::Unexpected(raised) => raised.fmt(f),
        }
    }
}

impl std::error::Error for YandexError {}

impl From<UnexpectedInput> for YandexError {
    fn from(raised: UnexpectedInput) -> Self {
        YandexError::Unexpected(raised)
    }
}

/// `_looks_like_a_challenge`: a challenge marker in the final URL, or in the first 6000
/// characters of the page.
fn looks_like_a_challenge(response: &Response<'_>) -> bool {
    let url = response.url.to_lowercase();
    if CHALLENGE_MARKERS.iter().any(|marker| url.contains(marker)) {
        return true;
    }
    let head: String = response
        .text
        .chars()
        .take(6000)
        .collect::<String>()
        .to_lowercase();
    CHALLENGE_MARKERS.iter().any(|marker| head.contains(marker))
}

/// `_challenged`: starts the rest and says so.
fn challenged(state: &mut YandexState, clock: RestClock<'_>) -> YandexError {
    state.resting_until = Some(clock.now + CHALLENGE_REST_SECONDS);
    YandexError::Blocked(format!(
        "Yandex asked for a captcha, so it is resting until {} rather than being asked again \
         straight away. The other engines carry on.",
        clock.rest_ends_at
    ))
}

/// `_count_failure`: counts one more failed image; the third in a row stands the engine down.
fn count_failure(state: &mut YandexState, message: String) -> YandexError {
    state.consecutive_failures += 1;
    if state.consecutive_failures < MAX_CONSECUTIVE_FAILURES {
        return YandexError::Failed(message);
    }
    state.blocked = true;
    YandexError::Blocked(format!(
        "{message} That has now happened to {MAX_CONSECUTIVE_FAILURES} images in a row, so \
         Yandex is standing down for the rest of this run. Start the search again to retry it."
    ))
}

/// `_cbir_id`: the search id in the upload's JSON answer. `json` is `resp.json()`, `None` when
/// it would not decode. `Ok(None)` where Python's `except (ValueError, AttributeError,
/// IndexError, TypeError)` catches. `Err` where it does not: `blocks` a non-empty object
/// (`KeyError`), or an id that is truthy and not a string (Python would carry it into the next
/// request; a string id cannot hold it).
fn cbir_id(json: Option<&Value>) -> Result<Option<String>, UnexpectedInput> {
    let Some(Value::Object(answer)) = json else {
        return Ok(None);
    };
    // `(answer.get("blocks") or [])[0]`
    let first = match answer.get("blocks") {
        Some(Value::Array(blocks)) => blocks.first(),
        Some(Value::Object(blocks)) if !blocks.is_empty() => return Err(UnexpectedInput),
        _ => None,
    };
    // `(block.get("params") or {}).get("cbirId") or None`
    let Some(Value::Object(block)) = first else {
        return Ok(None);
    };
    let Some(Value::Object(params)) = block.get("params") else {
        return Ok(None);
    };
    match params.get("cbirId") {
        Some(Value::String(id)) if !id.is_empty() => Ok(Some(id.clone())),
        Some(id) if pycompat::truthy(id) => Err(UnexpectedInput),
        _ => Ok(None),
    }
}

/// What `search` does with the upload's answer: a captcha rests the engine, an answer with no
/// search id counts a failure, and otherwise the id for the "sites" request. `json` is the
/// answer's `resp.json()`, `None` when it would not decode.
///
/// The "sites" request is made with `cbir_id`, `rpt=imageview`, `cbir_page=sites` and `url` =
/// `https://avatars.mds.yandex.net/get-images-cbir/<id>/orig` against [`SEARCH_URL`]; that and
/// the headers are the client's.
pub fn upload_from_response(
    state: &mut YandexState,
    clock: RestClock<'_>,
    upload: &Response<'_>,
    json: Option<&Value>,
) -> Result<String, YandexError> {
    if looks_like_a_challenge(upload) {
        return Err(challenged(state, clock));
    }
    match cbir_id(json)? {
        Some(id) => Ok(id),
        None => Err(count_failure(
            state,
            format!(
                "Yandex's upload answer (HTTP {}) carried no search id - the endpoint may have \
                 changed.",
                upload.status
            ),
        )),
    }
}

/// What `search` does with the "sites" page: a page with no results block is a captcha (rest) or
/// a failed image (counted), and one with a block, even an empty one, is a success that clears
/// the count and goes through [`keep_post_pages`].
pub fn results_from_response(
    state: &mut YandexState,
    clock: RestClock<'_>,
    page: &Response<'_>,
) -> Result<Vec<YandexMatch>, YandexError> {
    let Some(sites) = parse_sites(page.text)? else {
        if looks_like_a_challenge(page) {
            return Err(challenged(state, clock));
        }
        return Err(count_failure(
            state,
            format!(
                "Yandex's results page (HTTP {}) had no results block in it - the page layout \
                 may have changed.",
                page.status
            ),
        ));
    };
    state.consecutive_failures = 0;
    Ok(keep_post_pages(&sites)?)
}

// ----------------------------------------------------------------------
// The request half (Phase 3.e)
// ----------------------------------------------------------------------

/// The module state of `core/yandex.py`: [`YandexState`] and the time the next search may start
/// (`_next_turn`, on the clock's monotonic reading).
#[derive(Debug, Default)]
struct Inner {
    state: YandexState,
    next_turn: Option<f64>,
}

/// The request half of `core/yandex.py`: `search`, `is_blocked`, `is_resting`, `resting_until`,
/// `reset_blocked_flag`, `end_rest` and `wait_for_turn`.
///
/// Two requests per image on one `requests.Session` (so its cookies, which Yandex sets with the
/// upload's answer, go with the second): the picture, re-encoded and fitted in a 1600 px square
/// ([`image_prep::reencode_jpeg`]), POSTed to [`SEARCH_URL`], then a GET of the "sites" tab for
/// the `cbirId` it answers with. What the answers mean is [`upload_from_response`] and
/// [`results_from_response`].
///
/// The module state Python keeps in globals is the client's; clones share it. The address comes
/// from the constructor, so a test serves it from a local server; `new` uses the real one.
#[derive(Clone)]
pub struct YandexClient {
    net: NetClient,
    search_url: String,
    clock: Arc<dyn Clock>,
    inner: Arc<Mutex<Inner>>,
}

impl YandexClient {
    /// The real yandex.com.
    pub fn new(net: NetClient) -> Self {
        Self::with_search_url(net, SEARCH_URL)
    }

    /// The same client against another address (a local https server in tests). Not reachable
    /// from `config.json`, the environment or a flag.
    pub fn with_search_url(net: NetClient, search_url: &str) -> Self {
        YandexClient {
            net,
            search_url: search_url.to_owned(),
            clock: Arc::new(SystemClock::default()),
            inner: Arc::default(),
        }
    }

    /// The clock the rest and the pacing are measured on.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `is_blocked`: stood down, or resting after a captcha.
    pub fn is_blocked(&self) -> bool {
        self.lock().state.is_blocked(self.clock.monotonic())
    }

    /// The state Python keeps in `_blocked`, `_consecutive_failures` and `_resting_until`.
    pub fn state(&self) -> YandexState {
        self.lock().state
    }

    /// `is_resting`.
    pub fn is_resting(&self) -> bool {
        self.lock().state.is_resting(self.clock.monotonic())
    }

    /// `resting_until`: when the rest ends, as a local clock time, e.g. "21:41". Empty when it
    /// is not resting.
    pub fn resting_until(&self) -> String {
        let monotonic = self.clock.monotonic();
        let until = {
            let inner = self.lock();
            match inner.state.resting_until {
                Some(until) if inner.state.is_resting(monotonic) => until,
                _ => return String::new(),
            }
        };
        self.clock
            .local_hhmm(self.clock.wall() + (until - monotonic))
    }

    /// `reset_blocked_flag`: called whenever a search starts. Clears a stand-down, but not the
    /// rest after a challenge.
    pub fn reset_blocked_flag(&self) {
        self.lock().state.reset_blocked_flag();
    }

    /// `end_rest`: lets Yandex be asked again now.
    pub fn end_rest(&self) {
        let mut inner = self.lock();
        inner.state.end_rest();
        inner.next_turn = None;
    }

    /// `wait_for_turn`: holds a search until [`MIN_SECONDS_BETWEEN_SEARCHES`] have passed since
    /// the last one started. The turn is claimed before waiting, so two searches arriving
    /// together are spaced out rather than both let go. `should_stop` is polled every second.
    pub async fn wait_for_turn(
        &self,
        on_tick: Option<OnTick>,
        should_stop: Option<super::saucenao::ShouldStop>,
    ) {
        let now = self.clock.monotonic();
        let start_at = {
            let mut inner = self.lock();
            // `max(now, _next_turn or now)`
            let start_at = now.max(inner.next_turn.filter(|t| *t != 0.0).unwrap_or(now));
            inner.next_turn = Some(start_at + MIN_SECONDS_BETWEEN_SEARCHES);
            start_at
        };
        let wait = start_at - now;
        if wait <= 0.0 {
            return;
        }
        loop {
            let left = start_at - self.clock.monotonic();
            if left <= 0.0 || should_stop.as_ref().is_some_and(|stop| stop()) {
                return;
            }
            if let Some(tick) = &on_tick {
                tick("Pacing Yandex", left, wait);
            }
            self.clock.sleep(left.min(1.0)).await;
        }
    }

    /// The `RestClock` the pure half's messages need.
    fn rest_clock<'a>(&self, rest_ends_at: &'a str) -> RestClock<'a> {
        RestClock {
            now: self.clock.monotonic(),
            rest_ends_at,
        }
    }

    /// `prepare_upload`: the picture as a JPEG no larger than [`UPLOAD_MAX_SIDE`]. Always
    /// re-encoded; an animation gives its first frame.
    async fn prepare_upload(&self, image_path: &Path) -> Result<Vec<u8>, YandexError> {
        let (data, _) = read_upload(image_path)
            .await
            .map_err(|e| YandexError::Failed(format!("Could not read image: {}", e.0)))?;
        let encoded = tokio::task::spawn_blocking(move || {
            // Python's `except (OSError, ValueError, ...)`; a decoder that panics is the same
            // refusal, not a crash.
            std::panic::catch_unwind(|| {
                image_prep::reencode_jpeg(&data, UPLOAD_MAX_SIDE, UPLOAD_JPEG_QUALITY)
            })
            .unwrap_or_else(|_| Err("the image decoder panicked".to_owned()))
        })
        .await
        .unwrap_or_else(|join| Err(join.to_string()));
        encoded.map_err(|why| {
            debug!("Yandex could not re-encode the picture: {why}");
            YandexError::Failed(
                "not in a format that can be sent to Yandex (video and JPEG XL are the usual \
                 reasons); the other engines can still search it"
                    .to_owned(),
            )
        })
    }

    /// `search`: upload to Yandex and return the results on post pages of sites the app can read
    /// tags from, in Yandex's order.
    ///
    /// `timeout` is both the per-read timeout and a deadline over each of the two exchanges, as
    /// `run_with_hard_timeout(..., timeout)` is around each in Python. `on_tick` is the UI
    /// countdown, see [`OnTick`].
    pub async fn search(
        &self,
        image_path: &Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> Result<Vec<YandexMatch>, YandexError> {
        let jpeg = self.prepare_upload(image_path).await?;
        let mut session = Session::new(
            self.net.clone(),
            vec![
                ("User-Agent", USER_AGENT.to_owned()),
                ("Referer", REFERER.to_owned()),
            ],
        );
        let page = ticking(
            "Waiting on Yandex",
            timeout,
            on_tick,
            self.exchange(&mut session, jpeg, timeout),
        )
        .await?;

        let page_text = response_text(&page);
        let page_url = page.url.as_str().to_owned();
        let sites_page = Response {
            status: page.status.as_u16(),
            url: &page_url,
            text: &page_text,
        };
        let ends_at = self.rest_ends_at();
        let matches = {
            let mut inner = self.lock();
            results_from_response(&mut inner.state, self.rest_clock(&ends_at), &sites_page)?
        };
        debug!(
            "Yandex: {} on a post page the app can read for {}",
            matches.len(),
            image_path.display()
        );
        Ok(matches)
    }

    /// The local clock time a rest that starts now ends.
    fn rest_ends_at(&self) -> String {
        self.clock
            .local_hhmm(self.clock.wall() + CHALLENGE_REST_SECONDS)
    }

    /// Everything `search` does inside its `ProgressTicker`: the upload, what its answer means,
    /// and the "sites" page.
    async fn exchange(
        &self,
        session: &mut Session,
        jpeg: Vec<u8>,
        timeout: f64,
    ) -> Result<crate::net::NetResponse, YandexError> {
        let upload_url = format!(
            "{}?{}",
            self.search_url,
            urlencode(&[
                ("rpt", "imageview"),
                ("format", "json"),
                ("request", UPLOAD_REQUEST),
            ])
        );
        let upload = multipart_upload_as("upfile", "image.jpg", "image/jpeg", &jpeg);
        let uploaded = session
            .send(
                SessionRequest {
                    method: Method::Post,
                    url: &upload_url,
                    upload: Some(&upload),
                },
                timeout,
                Instant::now(),
            )
            .await
            .map_err(transport_error)?;

        let text = response_text(&uploaded);
        let url = uploaded.url.as_str().to_owned();
        let answer = Response {
            status: uploaded.status.as_u16(),
            url: &url,
            text: &text,
        };
        let json = PyResponse::from_net(uploaded.clone()).json().ok();
        let ends_at = self.rest_ends_at();
        let cbir_id = {
            let mut inner = self.lock();
            upload_from_response(
                &mut inner.state,
                self.rest_clock(&ends_at),
                &answer,
                json.as_ref(),
            )?
        };

        let page_url = format!(
            "{}?{}",
            self.search_url,
            urlencode(&[
                ("cbir_id", &cbir_id),
                ("rpt", "imageview"),
                ("cbir_page", "sites"),
                (
                    "url",
                    &format!("https://avatars.mds.yandex.net/get-images-cbir/{cbir_id}/orig"),
                ),
            ])
        );
        session
            .send(
                SessionRequest {
                    method: Method::Get,
                    url: &page_url,
                    upload: None,
                },
                timeout,
                Instant::now(),
            )
            .await
            .map_err(transport_error)
    }
}

/// `except HardTimeoutError` and `except requests.RequestException` around the two requests.
fn transport_error(failure: Failure) -> YandexError {
    match failure {
        Failure::Deadline(HardTimeoutError(why)) => {
            error!("Yandex request exceeded the hard deadline");
            YandexError::Failed(format!("Yandex request timed out: {why}"))
        }
        Failure::Transport(why) => {
            error!("Yandex request failed");
            YandexError::Failed(format!("Yandex request failed: {why}"))
        }
    }
}

#[cfg(test)]
mod tests {
    //! `tests/test_yandex.py::TestReadingTheResultsPage` and `TestStandingDown` at `76c85c4`, test for
    //! test (the latter through the two halves of `search`, see the end of the module). The
    //! `test_a_resting_yandex_says_so_on_the_row` is the engine runner's and `TestTakingPart` is
    //! engine settings: Phase 3.

    use super::*;
    use serde_json::json;

    /// `_site(url, domain, w, h)`: Yandex tags every outbound link with its own utm_ parameters.
    fn site(url: &str, domain: &str) -> Value {
        let tagged = if url.contains('?') {
            format!("{url}&utm_medium=organic&utm_source=yandexsmartcamera")
        } else {
            format!("{url}?utm_medium=organic&utm_source=yandexsmartcamera")
        };
        json!({
            "title": "t",
            "url": tagged,
            "domain": domain,
            "thumb": {"url": "//avatars.mds.yandex.net/i?id=abc&n=13", "width": 148, "height": 90},
            "originalImage": {"url": "https://img.example/1.jpg", "width": 1000, "height": 1400},
        })
    }

    /// `html.escape(text, quote=True)`.
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    /// `_page(sites)`: other data-state blobs first, then the one carrying cbirSites,
    /// HTML-escaped as served.
    fn page(sites: &[Value]) -> String {
        let other = escape(&json!({"form": {"action": "/images/search"}}).to_string());
        let state = escape(&json!({"initialState": {"cbirSites": {"sites": sites}}}).to_string());
        format!(
            "<html><div data-state=\"{other}\"></div><div class=\"CbirSites\" data-state=\"{state}\"></div></html>"
        )
    }

    fn sites() -> Vec<Value> {
        vec![
            site(
                "https://ru.pinterest.com/pin/267753140320940932/",
                "ru.pinterest.com",
            ),
            site(
                "https://rule34.us/index.php?r=posts%2Findex&q=dirtynicky",
                "rule34.us",
            ),
            site(
                "https://rule34.us/index.php?r=posts%2Fview&id=757670",
                "rule34.us",
            ),
            site(
                "https://safebooru.org/index.php?page=post&s=list&tags=baku-p",
                "safebooru.org",
            ),
            site(
                "https://kagamihara.donmai.us/posts/6307169?q=1girl",
                "kagamihara.donmai.us",
            ),
            site("https://telegra.ph/Some-post-01-01", "telegra.ph"),
            site(
                "https://xbooru.com/index.php?page=post&s=view&id=855502",
                "xbooru.com",
            ),
        ]
    }

    #[test]
    fn finds_the_sites_block_among_the_others() {
        let sites = sites();
        assert_eq!(
            parse_sites(&page(&sites)).unwrap().unwrap().len(),
            sites.len()
        );
    }

    #[test]
    fn a_page_with_no_block_is_not_an_empty_answer() {
        assert_eq!(
            parse_sites("<html><div data-state=\"{}\"></div></html>"),
            Ok(None)
        );
        assert_eq!(parse_sites(&page(&[])), Ok(Some(vec![])));
    }

    #[test]
    fn keeps_only_post_pages_the_app_can_read() {
        // Pinterest and Telegraph carry no tags; a tag search or a post list is not the post,
        // even on a site with a parser.
        let found = parse_sites(&page(&sites())).unwrap().unwrap();
        let urls: Vec<String> = keep_post_pages(&found)
            .unwrap()
            .into_iter()
            .map(|m| m.url)
            .collect();
        assert_eq!(urls.len(), 3);
        assert!(urls[0].contains("rule34.us") && urls[0].contains("757670"));
        assert!(urls[1].starts_with("https://danbooru.donmai.us/posts/6307169"));
        assert!(urls[2].contains("855502"));
        assert!(!urls.iter().any(|u| u.contains("utm_")));
    }

    #[test]
    fn scored_by_position_among_the_kept() {
        let found = parse_sites(&page(&sites())).unwrap().unwrap();
        let matches = keep_post_pages(&found).unwrap();
        let scores: Vec<f64> = matches.iter().map(|m| m.similarity).collect();
        assert_eq!(scores, [80.0, 78.0, 76.0]);
        assert!(
            matches[0]
                .thumb_url
                .as_deref()
                .unwrap()
                .starts_with("https://avatars.mds.yandex.net/")
        );
        assert_eq!(
            (matches[0].width, matches[0].height),
            (Some(1000), Some(1400))
        );
    }

    #[test]
    fn the_same_post_twice_is_one_row() {
        let sites = sites();
        let twice = [sites[2].clone(), sites[2].clone()];
        assert_eq!(keep_post_pages(&twice).unwrap().len(), 1);
    }

    // ---- what Python's code does that its tests do not say ----

    #[test]
    fn a_dozen_posts_are_cut_at_eight_and_the_score_floors_at_fifty() {
        let many: Vec<Value> = (1..=12)
            .map(|id| site(&format!("https://danbooru.donmai.us/posts/{id}"), "d"))
            .collect();
        let matches = keep_post_pages(&many).unwrap();
        assert_eq!(matches.len(), MAX_RESULTS);
        assert_eq!(matches[7].similarity, 66.0);
        assert!(matches.iter().all(|m| m.similarity >= SIMILARITY_FLOOR));
    }

    #[test]
    fn a_danbooru_mirror_is_the_main_host_and_a_protocol_relative_thumb_gets_https() {
        let mirror = json!({
            "url": "https://kagamihara.donmai.us/posts/6307169?q=1girl&utm_source=y",
            "thumb": {"url": "//t.example/a.jpg"},
            "originalImage": {"url": "https://o.example/b.jpg", "width": "1000", "height": 1400.9},
        });
        let m = keep_post_pages(&[mirror]).unwrap().remove(0);
        assert_eq!(m.url, "https://danbooru.donmai.us/posts/6307169?q=1girl");
        assert_eq!(m.thumb_url.as_deref(), Some("https://t.example/a.jpg"));
        // `int("1000")` and `int(1400.9)`.
        assert_eq!((m.width, m.height), (Some(1000), Some(1400)));
        assert_eq!(m.source_name.as_deref(), Some("danbooru.donmai.us"));
    }

    #[test]
    fn with_no_thumb_the_original_image_is_the_thumbnail() {
        let no_thumb = json!({
            "url": "https://danbooru.donmai.us/posts/1",
            "originalImage": {"url": "https://o.example/b.jpg"},
        });
        let m = keep_post_pages(&[no_thumb]).unwrap().remove(0);
        assert_eq!(m.thumb_url.as_deref(), Some("https://o.example/b.jpg"));
        assert_eq!((m.width, m.height), (None, None));
    }

    #[test]
    fn a_value_python_would_raise_on_is_an_error_and_not_an_empty_answer() {
        // `(site.get("thumb") or {}).get("url")` on a list is an AttributeError.
        let bad_thumb = json!({"url": "https://danbooru.donmai.us/posts/1", "thumb": [1]});
        assert_eq!(keep_post_pages(&[bad_thumb]), Err(UnexpectedInput));
        // `"x".strip()` is fine, `5 .strip()` is not.
        assert_eq!(keep_post_pages(&[json!({"url": 5})]), Err(UnexpectedInput));
        // A site that is not an object is skipped, as `isinstance(site, dict)` skips it.
        assert_eq!(keep_post_pages(&[json!("x"), json!(null)]), Ok(vec![]));
        // A block that parses to a list: `state.get` is an AttributeError.
        assert_eq!(
            parse_sites("<div data-state=\"[&quot;cbirSites&quot;]\"></div>"),
            Err(UnexpectedInput)
        );
        // One that is not JSON at all is skipped and the search goes on.
        assert_eq!(
            parse_sites(
                "<div data-state=\"cbirSites {\"></div><div data-state=\"{&quot;initialState&quot;: {&quot;cbirSites&quot;: {}}}\"></div>"
            ),
            Ok(Some(vec![]))
        );
    }

    #[test]
    fn a_post_url_python_cannot_parse_is_dropped_or_an_error_as_python_does() {
        // `urlparse` raises on the unbalanced bracket and `post_url` catches it.
        assert_eq!(post_url("https://[danbooru.donmai.us/posts/1"), Ok(None));
        // With no scheme, `find_parser` puts one on and `urlparse` then raises, which
        // `post_url` does not catch.
        assert_eq!(
            post_url("pixiv.net\u{ff0f}/comments/1"),
            Err(UnexpectedInput)
        );
    }

    #[test]
    fn dotted_and_dotless_capital_i_count_as_i_under_ignorecase() {
        // Python's `re.IGNORECASE` lets `i` match `ı` and `İ`; `(?i)` alone does not.
        assert!(
            post_url("https://danbooru.donmai.us/posts/v\u{131}ew/12")
                .unwrap()
                .is_some()
        );
        assert!(
            post_url("https://danbooru.donmai.us/pOSts/vIEw/12")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            post_url("https://DONMA\u{130}.us/posts/12")
                .unwrap()
                .as_deref(),
            Some("https://danbooru.donmai.us/posts/12")
        );
    }

    #[test]
    fn a_data_state_without_cbir_sites_is_not_read_at_all() {
        // Python skips it before it decodes it, so a blob that would raise is never looked at.
        let page = format!(
            "<div data-state=\"[1]\"></div><div data-state=\"5\"></div>{}",
            page(&sites())
        );
        assert_eq!(parse_sites(&page).unwrap().unwrap().len(), sites().len());
    }

    // ---- `tests/test_yandex.py::TestStandingDown`, through the two halves of `search` -----------

    const CLOCK: RestClock<'static> = RestClock {
        now: 5000.0,
        rest_ends_at: "21:41",
    };
    const UPLOAD_URL: &str = "https://yandex.com/images/search";
    const CAPTCHA_URL: &str = "https://yandex.com/showcaptcha?cc=1&retpath=x";

    fn answer<'a>(url: &'a str, text: &'a str) -> Response<'a> {
        Response {
            status: 200,
            url,
            text,
        }
    }

    fn upload_ok() -> Value {
        json!({"blocks": [{"params": {"cbirId": "1617445/abc"}}]})
    }

    /// `YandexState` after `search(upload, page)`, as the Python tests drive it.
    fn run(
        state: &mut YandexState,
        upload: &Response<'_>,
        json: Option<&Value>,
        page: Option<&Response<'_>>,
    ) -> Result<Vec<YandexMatch>, YandexError> {
        upload_from_response(state, CLOCK, upload, json)?;
        results_from_response(state, CLOCK, page.expect("a page"))
    }

    fn good_upload(
        state: &mut YandexState,
        page_text: &str,
    ) -> Result<Vec<YandexMatch>, YandexError> {
        run(
            state,
            &answer(UPLOAD_URL, ""),
            Some(&upload_ok()),
            Some(&answer(UPLOAD_URL, page_text)),
        )
    }

    #[test]
    fn a_good_answer() {
        let mut state = YandexState::default();
        let matches = good_upload(&mut state, &page(&sites())).unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(state, YandexState::default());
    }

    #[test]
    fn a_captcha_rests_yandex_through_a_new_search() {
        let mut state = YandexState::default();
        let error = run(
            &mut state,
            &answer(CAPTCHA_URL, "<html>captcha</html>"),
            None,
            None,
        )
        .unwrap_err();
        assert!(matches!(error, YandexError::Blocked(_)));
        state.reset_blocked_flag(); // what starting a search does
        assert!(state.is_blocked(CLOCK.now));
        assert!(error.to_string().contains(CLOCK.rest_ends_at));
    }

    #[test]
    fn a_captcha_on_the_results_page_counts_too() {
        let mut state = YandexState::default();
        let error = run(
            &mut state,
            &answer(UPLOAD_URL, ""),
            Some(&upload_ok()),
            Some(&answer(CAPTCHA_URL, "<html>captcha</html>")),
        )
        .unwrap_err();
        assert!(matches!(error, YandexError::Blocked(_)));
        assert!(state.is_resting(CLOCK.now));
    }

    #[test]
    fn a_challenge_marker_in_the_body_counts_but_only_in_its_first_6000_characters() {
        for marker in ["showcaptcha", "SmartCaptcha", "checkcaptcha"] {
            let mut state = YandexState::default();
            let text = format!("<html>{marker}</html>");
            assert!(
                upload_from_response(&mut state, CLOCK, &answer(UPLOAD_URL, &text), None).is_err()
            );
            assert!(state.is_resting(CLOCK.now), "{marker}");
        }
        let mut state = YandexState::default();
        let late = format!("{}showcaptcha", "x".repeat(6000));
        let error =
            upload_from_response(&mut state, CLOCK, &answer(UPLOAD_URL, &late), None).unwrap_err();
        assert!(matches!(error, YandexError::Failed(_)), "{error:?}");
        assert!(!state.is_resting(CLOCK.now));
    }

    #[test]
    fn the_rest_ends() {
        let mut state = YandexState::default();
        let _ = run(&mut state, &answer(CAPTCHA_URL, ""), None, None);
        assert!(state.is_blocked(CLOCK.now));
        assert!(state.is_blocked(CLOCK.now + CHALLENGE_REST_SECONDS - 1.0));
        assert!(!state.is_blocked(CLOCK.now + CHALLENGE_REST_SECONDS + 1.0));
    }

    #[test]
    fn a_changed_page_stands_down_after_a_run_of_them() {
        let mut state = YandexState::default();
        let broken = "<html>no results block</html>";
        for _ in 0..MAX_CONSECUTIVE_FAILURES - 1 {
            let error = good_upload(&mut state, broken).unwrap_err();
            assert!(matches!(error, YandexError::Failed(_)), "{error:?}");
        }
        let error = good_upload(&mut state, broken).unwrap_err();
        assert!(matches!(error, YandexError::Blocked(_)));
        assert!(error.to_string().contains("3 images in a row"));
        assert!(state.is_blocked(CLOCK.now));
        assert!(!state.is_resting(CLOCK.now)); // not a captcha: a new search retries
        state.reset_blocked_flag();
        assert!(!state.is_blocked(CLOCK.now));
    }

    #[test]
    fn a_success_clears_the_run_of_failures() {
        let mut state = YandexState::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES - 1 {
            assert!(good_upload(&mut state, "<html></html>").is_err());
        }
        good_upload(&mut state, &page(&[])).unwrap();
        let error = good_upload(&mut state, "<html></html>").unwrap_err();
        assert!(matches!(error, YandexError::Failed(_)), "{error:?}");
    }

    #[test]
    fn an_upload_with_no_search_id_counts_as_a_failure_and_says_what_it_got() {
        for json in [
            None,
            Some(json!(null)),
            Some(json!([])),
            Some(json!({})),
            Some(json!({"blocks": []})),
            Some(json!({"blocks": [{}]})),
            Some(json!({"blocks": [{"params": {"cbirId": ""}}]})),
            Some(json!({"blocks": [{"params": null}]})),
            Some(json!({"blocks": ["x"]})),
            Some(json!({"blocks": "x"})),
        ] {
            let mut state = YandexState::default();
            let upload = Response {
                status: 502,
                url: UPLOAD_URL,
                text: "",
            };
            let error =
                upload_from_response(&mut state, CLOCK, &upload, json.as_ref()).unwrap_err();
            assert_eq!(
                error,
                YandexError::Failed(
                    "Yandex's upload answer (HTTP 502) carried no search id - the endpoint may \
                     have changed."
                        .to_owned()
                ),
                "{json:?}"
            );
            assert_eq!(state.consecutive_failures, 1);
        }
    }

    #[test]
    fn an_answer_python_raises_on_is_an_error_that_counts_nothing() {
        for json in [
            json!({"blocks": {"a": 1}}),
            json!({"blocks": [{"params": {"cbirId": 7}}]}),
        ] {
            let mut state = YandexState::default();
            let error =
                upload_from_response(&mut state, CLOCK, &answer(UPLOAD_URL, ""), Some(&json))
                    .unwrap_err();
            assert_eq!(error, YandexError::Unexpected(UnexpectedInput), "{json}");
            assert_eq!(state, YandexState::default());
        }
    }
}
