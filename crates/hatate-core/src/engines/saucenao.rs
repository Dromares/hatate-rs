//! SauceNAO reverse image search (`core/saucenao.py`).
//!
//! SauceNAO has an official JSON API (needs an API key) that returns structured tag data, and an
//! HTML search page (no key, fewer tags, stricter rate limiting) that Python scrapes when there
//! is no key. This is the half that reads the answers: [`search_from_response`] is what `search`
//! does once the response is in hand, with [`parse_json_response`] for `_search_json`,
//! [`parse_html_response`] for `_search_html` and [`tags_from_result`] for `_tags_from_result`.
//!
//! **The cut.** The request half is Phase 3: the upload and its parameters, the wait for the
//! 30 s burst window before a request ([`SHORT_WINDOW_SECONDS`]), the one retry after a 429 (the
//! client waits the full window, then asks again, and hands this module the *answer to the
//! retry*), the network errors, and the module state Python keeps in globals. A function here
//! that Python would have made write that state returns it instead: [`JsonAnswer::quota`] is
//! `_last_quota` for this answer and [`JsonAnswer::daily_limit_reported`] says whether
//! `_daily_limit_reported` was set by it (the client keeps the latch, and the observation time).
//! The quota-pause file and the reset-time arithmetic are `quota_pause` and `timeutil`.
//!
//! **Shapes the API never sends.** Python reads a few fields unchecked, so a result that is not
//! an object, a `similarity` that is not a number, or an `ext_urls` entry that is not text makes
//! it raise something other than `SauceNaoError`. Here that is a `SauceNaoError` whose message
//! names the field, so the search fails in the same way it does there. A `title`, `index_name`,
//! `thumbnail` or `source` that is not text is read as absent. A name inside a list `characters`
//! that is not text is skipped (Python raises).

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Map, Value};
use tracing::{debug, error, info, warn};

use super::client::{
    Failure, OnTick, ProgressTicker, failure_of, multipart_upload, read_upload, response_text,
    seconds, ticking, urlencode,
};
use super::looks_like_url;
use crate::boorus::parsed::{classes_of, soup_of, text_of};
use crate::boorus::sizes::py_float;
use crate::clock::{Clock, SystemClock};
use crate::config::SauceNaoSettings;
use crate::hard_timeout::HardTimeoutError;
use crate::model::{Tag, TagSource};
use crate::net::{NetClient, NetError, NetRequest};
use crate::pycompat;
use crate::pyrepr::py_str;
use crate::quota_pause::{QuotaPause, QuotaPauseFile};

pub const API_URL: &str = "https://saucenao.com/search.php";

/// SauceNAO's documented burst window. The client waits all of it before a request when the last
/// reading says the window is spent, and after a 429.
pub const SHORT_WINDOW_SECONDS: f64 = 30.0;

/// Substrings SauceNAO uses when the daily allowance is spent, matched case-insensitively against
/// the header's `message`. Deliberately excludes the short-window wording.
pub const DAILY_LIMIT_MARKERS: [&str; 3] =
    ["daily search limit", "daily limit", "search limit exceeded"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SauceNaoMatch {
    pub url: String,
    pub similarity: f64,
    pub title: Option<String>,
    pub source_name: Option<String>,
    pub thumb_url: Option<String>,
    pub tags: Vec<Tag>,
}

/// SauceNAO's two rate-limit windows as its own response reports them: short is the ~30 s burst
/// window, long is the daily quota. Only the JSON API reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SauceNaoQuota {
    pub short_remaining: Option<i64>,
    pub short_limit: Option<i64>,
    pub long_remaining: Option<i64>,
    pub long_limit: Option<i64>,
}

/// `SauceNaoError`: the message is user-facing text (and, for an API error, SauceNAO's own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SauceNaoError(pub String);

impl std::fmt::Display for SauceNaoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SauceNaoError {}

/// What `_search_json` makes of an answer: the quota it read, whether it spent the daily
/// allowance, and the matches or the error.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonAnswer {
    /// `_last_quota` after this answer. `None` when the answer failed before its header was read
    /// (a bad status, a body that is not JSON, no header), which leave the old reading alone.
    pub quota: Option<SauceNaoQuota>,
    /// This answer set `_daily_limit_reported`: `long_remaining` was 0, or the error message says
    /// the daily limit is exhausted. The client latches it until the user resumes.
    pub daily_limit_reported: bool,
    /// `_last_quota_at` was set although the quota could not be read: the answer's `header` is a
    /// truthy value that is not an object, and Python stamps the time before it calls `.get` on
    /// it (and raises). The client takes the time, and keeps the old quota.
    pub stamps_quota_time: bool,
    pub result: Result<Vec<SauceNaoMatch>, SauceNaoError>,
}

/// `settings.api_key and settings.use_json_api`: which of the two pages `search` reads.
pub fn uses_json_api(settings: &SauceNaoSettings) -> bool {
    !settings.api_key.expose().is_empty() && settings.use_json_api
}

/// What `search` makes of the answer: the JSON path when [`uses_json_api`], else the HTML one.
/// For the JSON path `status` and `body` are those of the answer to the retry, when there was one.
/// The quota and the latch are on [`parse_json_response`]'s [`JsonAnswer`]; call that for them.
pub fn search_from_response(
    settings: &SauceNaoSettings,
    status: u16,
    body: &str,
) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
    if uses_json_api(settings) {
        parse_json_response(settings, status, body).result
    } else {
        parse_html_response(status, body)
    }
}

fn failure(message: impl Into<String>) -> SauceNaoError {
    SauceNaoError(message.into())
}

/// `_safe_int`: a quota field that is an integer or a numeric string, else `None`.
fn safe_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Null => None,
        other => {
            let parsed = pycompat::int(other);
            if parsed.is_none() {
                warn!("Could not parse SauceNAO quota value {other} as an integer");
            }
            parsed
        }
    }
}

/// `_search_json` from the answer on: the status mapping, the header checks and quota, then the
/// results.
pub fn parse_json_response(settings: &SauceNaoSettings, status: u16, body: &str) -> JsonAnswer {
    let early = |message: &str| JsonAnswer {
        quota: None,
        daily_limit_reported: false,
        stamps_quota_time: false,
        result: Err(failure(message)),
    };
    match status {
        413 => return early("SauceNAO rejected the image as too large, even after downscaling"),
        429 => {
            warn!("SauceNAO still rate limited after waiting the burst window");
            return early("SauceNAO rate limit reached, slow down the search delay");
        }
        200 => {}
        other => return early(&format!("SauceNAO returned HTTP {other}")),
    }
    let Ok(data) = serde_json::from_str::<Value>(body) else {
        error!("SauceNAO returned non-JSON body despite output_type=2");
        return early("SauceNAO returned invalid JSON");
    };
    // `data.get("header", {})`: a body that is not an object has none to give.
    let header = data
        .get("header")
        .and_then(Value::as_object)
        .filter(|h| !h.is_empty());
    let Some(header) = header else {
        warn!(
            "SauceNAO JSON response had no header object - treating it as a failure, not a confirmed result"
        );
        let mut answer = early("SauceNAO's response didn't include the expected header data");
        // `if not header` passes a non-empty string, list or number, and then `header.get`
        // raises, one statement after the time was taken.
        answer.stamps_quota_time = data
            .as_object()
            .and_then(|object| object.get("header"))
            .is_some_and(|value| !value.is_object() && pycompat::truthy(value));
        return answer;
    };

    let quota = SauceNaoQuota {
        short_remaining: safe_int(header.get("short_remaining")),
        short_limit: safe_int(header.get("short_limit")),
        long_remaining: safe_int(header.get("long_remaining")),
        long_limit: safe_int(header.get("long_limit")),
    };
    debug!("SauceNAO quota: {quota:?}");
    let mut daily_limit_reported = quota.long_remaining == Some(0);
    if daily_limit_reported {
        warn!("SauceNAO's daily search allowance is now spent (long_remaining=0)");
    }
    let answer = |daily_limit_reported, result| JsonAnswer {
        quota: Some(quota),
        daily_limit_reported,
        stamps_quota_time: false,
        result,
    };

    // `header.get("status", 0) < 0`. A status that is not a number makes Python raise.
    let status_code = match header.get("status") {
        None => 0.0,
        Some(value) => match pycompat::number(value) {
            Some(number) => number,
            None => {
                return answer(
                    daily_limit_reported,
                    Err(failure("SauceNAO's status is not a number")),
                );
            }
        },
    };
    if status_code < 0.0 {
        let message = header
            .get("message")
            .map_or_else(|| "Unknown SauceNAO error".to_owned(), py_str);
        // The error text can say so when the response carries no usable quota numbers.
        let lowered = message.to_lowercase();
        if DAILY_LIMIT_MARKERS
            .iter()
            .any(|marker| lowered.contains(marker))
        {
            warn!("SauceNAO reported the daily search limit is exhausted: {message}");
            daily_limit_reported = true;
        }
        error!("SauceNAO reported an error: {message}");
        return answer(daily_limit_reported, Err(SauceNaoError(message)));
    }

    let result = results_of(&data, settings);
    answer(daily_limit_reported, result)
}

/// A Python value read the way an `or []` / `for x in v` would, for the fields the API sends as
/// lists.
fn items_of(value: Option<&Value>, what: &str) -> Result<Vec<Value>, SauceNaoError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(Value::Object(map)) => Ok(map.keys().map(|k| Value::String(k.clone())).collect()),
        Some(Value::String(text)) => {
            Ok(text.chars().map(|c| Value::String(c.to_string())).collect())
        }
        Some(Value::Null) if what == "ext_urls" => Ok(Vec::new()),
        Some(other) if what == "ext_urls" && !pycompat::truthy(other) => Ok(Vec::new()),
        Some(_) => Err(failure(format!("SauceNAO's {what} is not a list"))),
    }
}

/// The result loop of `_search_json`: one candidate per distinct usable URL, best first.
fn results_of(
    data: &Value,
    settings: &SauceNaoSettings,
) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
    let mut matches: Vec<SauceNaoMatch> = Vec::new();
    let mut skipped_no_url = 0usize;
    let empty = Map::new();
    for result in items_of(data.get("results"), "results")? {
        let Some(result) = result.as_object() else {
            return Err(failure(
                "SauceNAO's results held something that is not an object",
            ));
        };
        let rheader = result.get("header").map_or(Some(&empty), Value::as_object);
        let rdata = result.get("data").map_or(Some(&empty), Value::as_object);
        let (Some(rheader), Some(rdata)) = (rheader, rdata) else {
            return Err(failure(
                "SauceNAO's result had a header or data that is not an object",
            ));
        };
        let similarity = match rheader.get("similarity") {
            None => 0.0,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::String(text)) => {
                py_float(text).ok_or_else(|| failure("SauceNAO's similarity is not a number"))?
            }
            Some(_) => return Err(failure("SauceNAO's similarity is not a number")),
        };

        // One SauceNAO result can list the SAME image on several sites; each URL is its own
        // candidate, so per-site filtering applies to each.
        let mut url_list: Vec<String> = Vec::new();
        for url in items_of(rdata.get("ext_urls"), "ext_urls")? {
            match &url {
                Value::String(text) if looks_like_url(text) => url_list.push(text.clone()),
                Value::String(_) => {}
                other if !pycompat::truthy(other) => {}
                _ => {
                    return Err(failure(
                        "SauceNAO's ext_urls held something that is not text",
                    ));
                }
            }
        }
        if url_list.is_empty() {
            skipped_no_url += 1;
            continue; // no usable absolute URL for this result
        }

        let tags = tags_from_result(rdata, settings);
        let text_of_field = |map: &Map<String, Value>, key: &str| -> Option<String> {
            map.get(key).and_then(Value::as_str).map(str::to_owned)
        };
        // `rdata.get("title") or rdata.get("source")`: the last operand is the answer even when
        // it is empty.
        let title = text_of_field(rdata, "title")
            .filter(|t| !t.is_empty())
            .or_else(|| text_of_field(rdata, "source"));
        let mut seen: Vec<&str> = Vec::new();
        for url in &url_list {
            if seen.contains(&url.as_str()) {
                continue;
            }
            seen.push(url);
            matches.push(SauceNaoMatch {
                url: url.clone(),
                similarity,
                title: title.clone(),
                source_name: text_of_field(rheader, "index_name"),
                thumb_url: text_of_field(rheader, "thumbnail"),
                tags: tags.clone(),
            });
        }
    }
    // `-similarity`, stable.
    matches.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    debug!(
        "SauceNAO JSON parsed into {} match(es) ({skipped_no_url} skipped for lacking a usable URL)",
        matches.len()
    );
    Ok(matches)
}

/// `_tags_from_result`: character, material and creator fields of one result's `data`, filtered
/// to the tag types the user enabled, in the order they are enabled.
pub fn tags_from_result(rdata: &Map<String, Value>, settings: &SauceNaoSettings) -> Vec<Tag> {
    let first_truthy = |keys: &[&str]| -> Option<&Value> {
        keys.iter()
            .filter_map(|key| rdata.get(*key))
            .find(|v| pycompat::truthy(v))
    };
    let mut tags = Vec::new();
    for tag_type in &settings.tag_types {
        let raw = match tag_type.as_str() {
            "character" => rdata.get("characters"),
            "material" => rdata.get("material"),
            "creator" => first_truthy(&["creator", "member_name", "author_name"]),
            _ => None,
        };
        let Some(raw) = raw.filter(|v| pycompat::truthy(v)) else {
            continue;
        };
        let names: Vec<String> = match raw {
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            other => py_str(other)
                .split(',')
                .map(|name| pycompat::strip(name).to_owned())
                .collect(),
        };
        for name in names {
            let name = pycompat::strip(&name);
            if name.is_empty() {
                continue;
            }
            let namespace = if settings.namespace.is_empty() {
                tag_type.as_str()
            } else {
                settings.namespace.as_str()
            };
            tags.push(Tag {
                name: name.to_owned(),
                namespace: Some(namespace.to_owned()),
                source: TagSource::SearchEngine,
            });
        }
    }
    tags
}

/// `_search_html` from the answer on: the status mapping, then the result blocks of the page.
/// There is no 429 retry on this path, so a 429 is just another status.
pub fn parse_html_response(status: u16, html: &str) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
    if status == 413 {
        return Err(failure(
            "SauceNAO rejected the image as too large, even after downscaling",
        ));
    }
    if status != 200 {
        return Err(failure(format!("SauceNAO returned HTTP {status}")));
    }
    Ok(parse_html(html))
}

/// The result blocks of SauceNAO's HTML search page.
pub fn parse_html(html: &str) -> Vec<SauceNaoMatch> {
    use scraper::{ElementRef, Selector};
    let soup = soup_of(html);
    let any = Selector::parse("*").expect("selector");
    let has_class = |el: ElementRef<'_>, tag: &str, class: &str| -> bool {
        el.value().name() == tag && classes_of(el).contains(&class)
    };
    let mut matches = Vec::new();
    for block in soup
        .select(&any)
        .filter(|el| has_class(*el, "div", "result"))
    {
        if classes_of(block).contains(&"result-hidden") {
            continue;
        }
        // Python's `select_one` on a tag looks below it, never at the tag itself.
        let descendants = || {
            block
                .descendants()
                .filter_map(ElementRef::wrap)
                .filter(move |d| d.id() != block.id())
        };
        // `div.resulttitle a, div.resultcontentcolumn a`: the first link under either kind of div.
        let link = descendants().find(|a| {
            a.value().name() == "a"
                && a.ancestors().filter_map(ElementRef::wrap).any(|anc| {
                    has_class(anc, "div", "resulttitle")
                        || has_class(anc, "div", "resultcontentcolumn")
                })
        });
        let sim_el = descendants().find(|d| has_class(*d, "div", "resultsimilarityinfo"));
        let (Some(link), Some(sim_el)) = (link, sim_el) else {
            debug!("Result block missing link or similarity element, skipping");
            continue;
        };
        let href = link.value().attr("href").unwrap_or("");
        if !looks_like_url(href) {
            debug!("Result block href {href:?} not a usable absolute URL, skipping");
            continue;
        }
        let similarity = py_float(&text_of(sim_el).replace('%', "")).unwrap_or(0.0);
        let title = descendants()
            .find(|d| has_class(*d, "div", "resulttitle"))
            .map(text_of);
        let thumb_url = descendants()
            .find(|d| d.value().name() == "img")
            .and_then(|img| img.value().attr("src"))
            .map(|src| match src.strip_prefix("//") {
                Some(_) => format!("https:{src}"),
                None => src.to_owned(),
            });
        matches.push(SauceNaoMatch {
            url: href.to_owned(),
            similarity,
            title,
            source_name: None,
            thumb_url,
            tags: Vec::new(),
        });
    }
    matches.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    debug!("SauceNAO HTML parsed into {} match(es)", matches.len());
    matches
}

/// `should_stop`: the batch worker's stop flag, checked while a wait runs.
pub type ShouldStop = Arc<dyn Fn() -> bool + Send + Sync + 'static>;

/// What Python keeps in `saucenao`'s module globals.
#[derive(Debug, Default)]
struct State {
    /// `_last_quota`
    last_quota: Option<SauceNaoQuota>,
    /// `_last_quota_at`: when it was read (`time.time()`), 0 before any reading.
    last_quota_at: f64,
    /// `_daily_limit_reported`
    daily_limit_reported: bool,
}

/// `seconds_until_short_window_clears`: how long to hold off before the next request. Only the
/// burst window counts (the daily allowance does not clear by waiting). 0 whenever there is any
/// headroom at all, or nothing has been observed.
pub fn seconds_until_short_window_clears(
    quota: Option<&SauceNaoQuota>,
    observed_at: f64,
    now: f64,
) -> f64 {
    let Some(quota) = quota else { return 0.0 };
    match quota.short_remaining {
        None => 0.0,
        Some(remaining) if remaining > 0 => 0.0,
        Some(_) => (SHORT_WINDOW_SECONDS - (now - observed_at)).max(0.0),
    }
}

/// `describe_quota`: the status-bar reading of the daily allowance, "used / limit" when the limit
/// is known, "N left" when not, and blank when nothing was observed.
pub fn describe_quota(quota: Option<&SauceNaoQuota>) -> String {
    let Some(quota) = quota else {
        return String::new();
    };
    let Some(remaining) = quota.long_remaining else {
        return String::new();
    };
    match quota.long_limit {
        None => format!("SauceNAO: {remaining} left today"),
        Some(limit) => format!("SauceNAO: {}/{limit} used today", limit - remaining),
    }
}

/// `describe_short_window`: the ~30 s burst window, for a tooltip.
pub fn describe_short_window(quota: Option<&SauceNaoQuota>) -> String {
    let Some(quota) = quota else {
        return String::new();
    };
    let Some(remaining) = quota.short_remaining else {
        return String::new();
    };
    match quota.short_limit {
        None => format!("{remaining} remaining in the current ~30s window"),
        Some(limit) => format!(
            "{}/{limit} used in the current ~30s window",
            limit - remaining
        ),
    }
}

/// `record_quota_pause`: persists that a batch paused because the daily quota ran out. A write
/// that fails is logged and goes no further.
pub fn record_quota_pause(file: &QuotaPauseFile, clock: &dyn Clock, searched: i64, remaining: i64) {
    let pause = QuotaPause::starting_at(clock.wall(), searched, remaining);
    if let Err(err) = file.record(&pause) {
        warn!("Could not persist the quota-pause state: {err}");
    }
}

/// The request half of `core/saucenao.py`: `search`, and the module state it keeps (the last
/// quota reading and the daily-limit latch).
///
/// With an API key and `use_json_api` the key and the image are POSTed to the JSON API (waiting
/// out the burst window first when the last reading says it is spent, and once more, after a 429,
/// before the one retry); without them the HTML page is POSTed to and scraped. What an answer
/// means is [`parse_json_response`] and [`parse_html_response`]. Waits go through the injected
/// [`Clock`], so a test sees the sleeps, not the time they take. The address is the constructor's;
/// `new` uses the real one. Clones share the state.
#[derive(Clone)]
pub struct SauceNaoClient {
    net: NetClient,
    api_url: String,
    clock: Arc<dyn Clock>,
    state: Arc<Mutex<State>>,
}

impl SauceNaoClient {
    /// The real saucenao.com, on the system clock.
    pub fn new(net: NetClient) -> Self {
        Self::with_api_url(net, API_URL)
    }

    /// The same client against another address (a local https server in tests). Not reachable
    /// from `config.json`, the environment or a flag.
    pub fn with_api_url(net: NetClient, api_url: &str) -> Self {
        SauceNaoClient {
            net,
            api_url: api_url.to_owned(),
            clock: Arc::new(SystemClock::default()),
            state: Arc::default(),
        }
    }

    /// The same client on another clock (a test's, which answers instantly and records).
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// `get_last_quota`: the most recently observed quota, `None` before the first JSON search.
    pub fn get_last_quota(&self) -> Option<SauceNaoQuota> {
        self.state.lock().unwrap().last_quota
    }

    /// `_last_quota_at`: when the last reading was taken (`time.time()`), 0 before the first.
    pub fn last_quota_at(&self) -> f64 {
        self.state.lock().unwrap().last_quota_at
    }

    /// `_daily_limit_reported`: the latch alone, without the `long_remaining == 0` reading
    /// [`SauceNaoClient::is_daily_quota_exhausted`] also counts.
    pub fn daily_limit_reported(&self) -> bool {
        self.state.lock().unwrap().daily_limit_reported
    }

    /// `is_daily_quota_exhausted`: whether the daily allowance is spent as of the last answer.
    /// Only the daily window counts; a bare 429 does not.
    pub fn is_daily_quota_exhausted(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.daily_limit_reported
            || state
                .last_quota
                .is_some_and(|quota| quota.long_remaining == Some(0))
    }

    /// `reset_daily_limit_flag`: for when the user resumes a paused search.
    pub fn reset_daily_limit_flag(&self) {
        self.state.lock().unwrap().daily_limit_reported = false;
    }

    /// `search`. `timeout` is both the per-read timeout and a deadline over each request;
    /// `on_tick` is the UI countdown, see [`OnTick`]; `should_stop` lets Stop interrupt a wait.
    pub async fn search(
        &self,
        image_path: &Path,
        settings: &SauceNaoSettings,
        timeout: f64,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
        if uses_json_api(settings) {
            debug!(
                "Searching SauceNAO (JSON API, db={}) for {}",
                settings.dbmask,
                image_path.display()
            );
            self.search_json(image_path, settings, timeout, on_tick, should_stop)
                .await
        } else {
            debug!(
                "Searching SauceNAO (HTML scrape, no API key configured) for {}",
                image_path.display()
            );
            self.search_html(image_path, settings, timeout, on_tick)
                .await
        }
    }

    /// `_sleep_interruptibly`: waits, but lets Stop be Stop (checked every quarter second).
    pub(crate) async fn sleep_interruptibly(
        &self,
        wait: f64,
        label: &str,
        on_tick: Option<OnTick>,
        should_stop: &Option<ShouldStop>,
    ) {
        let deadline = self.clock.monotonic() + wait;
        let _ticker = ProgressTicker::start(label, wait, on_tick);
        loop {
            let remaining = deadline - self.clock.monotonic();
            if remaining <= 0.0 || should_stop.as_ref().is_some_and(|stop| stop()) {
                return;
            }
            self.clock.sleep(remaining.min(0.25)).await;
        }
    }

    /// `_wait_for_short_window`: sleeps out the burst window if the last reading says it is spent.
    async fn wait_for_short_window(
        &self,
        on_tick: Option<OnTick>,
        should_stop: &Option<ShouldStop>,
    ) -> f64 {
        let wait = {
            let state = self.state.lock().unwrap();
            seconds_until_short_window_clears(
                state.last_quota.as_ref(),
                state.last_quota_at,
                self.clock.wall(),
            )
        };
        if wait <= 0.0 {
            return 0.0;
        }
        info!(
            "SauceNAO's {}s burst window is spent - waiting {wait:.1}s before the next request",
            SHORT_WINDOW_SECONDS as i64
        );
        self.sleep_interruptibly(wait, "SauceNAO burst limit", on_tick, should_stop)
            .await;
        wait
    }

    /// The POST both paths make: `net.post(API_URL, params=params, files=files, timeout=timeout,
    /// deadline=timeout)` with the upload read first. Returns the status and decoded text.
    async fn post(
        &self,
        image_path: &Path,
        query: &[(&str, QueryValue<'_>)],
        timeout: f64,
        on_tick: Option<OnTick>,
        what: &str,
    ) -> Result<(u16, String), SauceNaoError> {
        let (data, filename) = read_upload(image_path)
            .await
            .map_err(|e| SauceNaoError(format!("Could not read image: {}", e.0)))?;
        let limit = seconds(timeout);
        let upload = multipart_upload("file", &filename, &data);
        let request = self
            .request(query, &upload.content_type, upload.body, limit)
            .map_err(|e| SauceNaoError(format!("SauceNAO request failed: {e}")))?;
        match ticking(
            "Waiting on SauceNAO",
            timeout,
            on_tick,
            self.net.send(request),
        )
        .await
        {
            Ok(resp) => {
                let status = resp.status.as_u16();
                debug!("SauceNAO{what} responded HTTP {status}");
                Ok((status, response_text(&resp)))
            }
            Err(err) => Err(match failure_of(&err, limit) {
                Failure::Deadline(HardTimeoutError(why)) => {
                    error!(
                        "SauceNAO{what} request for {} exceeded the hard deadline",
                        image_path.display()
                    );
                    SauceNaoError(format!("SauceNAO request timed out: {why}"))
                }
                Failure::Transport(why) => {
                    error!("SauceNAO{what} request failed");
                    SauceNaoError(format!("SauceNAO request failed: {why}"))
                }
            }),
        }
    }

    fn request(
        &self,
        query: &[(&str, QueryValue<'_>)],
        content_type: &str,
        body: bytes::Bytes,
        limit: std::time::Duration,
    ) -> Result<NetRequest, NetError> {
        // `params=` as `requests` encodes it, not the `url` crate's form encoding.
        let pairs: Vec<(&str, &str)> = query
            .iter()
            .map(|(name, value)| match value {
                QueryValue::Plain(text) => (*name, *text),
                QueryValue::Secret(secret) => (*name, secret.expose()),
            })
            .collect();
        let url = format!("{}?{}", self.api_url, urlencode(&pairs));
        let request = NetRequest::post(&url, limit)?;
        Ok(request
            .header("Content-Type", content_type)?
            .with_body(body)
            .with_deadline(limit))
    }

    async fn search_json(
        &self,
        image_path: &Path,
        settings: &SauceNaoSettings,
        timeout: f64,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
        let dbmask = settings.dbmask.to_string();
        let api_key = crate::net::Secret::new(settings.api_key.expose());
        let query = [
            ("output_type", QueryValue::Plain("2")),
            ("api_key", QueryValue::Secret(&api_key)),
            ("db", QueryValue::Plain(&dbmask)),
        ];
        self.wait_for_short_window(on_tick.clone(), &should_stop)
            .await;
        let (mut status, mut body) = self
            .post(image_path, &query, timeout, on_tick.clone(), " JSON")
            .await?;
        if status == 429 {
            // Retried once rather than given up on, because giving up is not free: the image
            // falls through to the secondary engine, gets a weaker result, and that result is
            // cached and marked searched. The wait is unconditional: a 429 means the window really
            // is spent whatever the last reading said, and the reading may be stale or absent.
            warn!("SauceNAO rate limit hit (429) - waiting out the burst window and retrying once");
            self.sleep_interruptibly(
                SHORT_WINDOW_SECONDS,
                "SauceNAO burst limit",
                on_tick.clone(),
                &should_stop,
            )
            .await;
            if should_stop.as_ref().is_some_and(|stop| stop()) {
                return Err(SauceNaoError(
                    "Search stopped while waiting out SauceNAO's rate limit".to_owned(),
                ));
            }
            (status, body) = self
                .post(image_path, &query, timeout, on_tick, " JSON")
                .await?;
            debug!("SauceNAO retry responded HTTP {status}");
        }

        let answer = parse_json_response(settings, status, &body);
        if answer.quota.is_some() || answer.stamps_quota_time {
            let mut state = self.state.lock().unwrap();
            state.last_quota_at = self.clock.wall();
            if let Some(quota) = answer.quota {
                state.last_quota = Some(quota);
            }
            if answer.daily_limit_reported {
                state.daily_limit_reported = true;
            }
        }
        answer.result
    }

    async fn search_html(
        &self,
        image_path: &Path,
        settings: &SauceNaoSettings,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
        let dbmask = settings.dbmask.to_string();
        let query = [("db", QueryValue::Plain(&dbmask))];
        let (status, body) = self
            .post(image_path, &query, timeout, on_tick, " (HTML)")
            .await?;
        parse_html_response(status, &body)
    }
}

/// A query value, secret or not.
enum QueryValue<'a> {
    Plain(&'a str),
    Secret(&'a crate::net::Secret),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(tag_types: &[&str], namespace: &str) -> SauceNaoSettings {
        SauceNaoSettings {
            api_key: "k".into(),
            tag_types: tag_types.iter().map(|t| (*t).to_owned()).collect(),
            namespace: namespace.to_owned(),
            ..SauceNaoSettings::default()
        }
    }

    fn answer(header: Value, results: Value) -> JsonAnswer {
        let body = json!({"header": header, "results": results}).to_string();
        parse_json_response(&settings(&["character", "material"], ""), 200, &body)
    }

    fn mirrored(ext_urls: Value) -> Vec<SauceNaoMatch> {
        answer(
            json!({"status": 0, "long_remaining": 100, "long_limit": 100}),
            json!([{
                "header": {
                    "similarity": "97.16",
                    "index_name": "Index #29: e621.net - 1234.jpg",
                    "thumbnail": "https://img1.saucenao.com/booru/t.jpg",
                },
                "data": {"ext_urls": ext_urls},
            }]),
        )
        .result
        .unwrap()
    }

    #[test]
    fn a_quota_field_is_an_integer_or_a_numeric_string_else_absent() {
        assert_eq!(safe_int(Some(&json!("5000"))), Some(5000));
        assert_eq!(safe_int(Some(&json!(4854))), Some(4854));
        assert_eq!(safe_int(Some(&json!("not_a_number"))), None);
        assert_eq!(safe_int(Some(&json!(""))), None);
        assert_eq!(safe_int(Some(&json!(null))), None);
        assert_eq!(safe_int(None), None);
        assert_eq!(safe_int(Some(&json!(7.9))), Some(7));
        assert_eq!(safe_int(Some(&json!([1]))), None);
    }

    #[test]
    fn only_the_daily_window_or_its_message_counts_as_the_daily_limit() {
        let flag = |header| answer(header, json!([])).daily_limit_reported;
        assert!(!flag(
            json!({"status": 0, "short_remaining": 5, "short_limit": 6, "long_remaining": 82, "long_limit": 100})
        ));
        // The burst window empty is not the daily limit.
        assert!(!flag(
            json!({"status": 0, "short_remaining": 0, "short_limit": 6, "long_remaining": 40, "long_limit": 100})
        ));
        assert!(flag(
            json!({"status": 0, "short_remaining": 4, "short_limit": 6, "long_remaining": 0, "long_limit": 100})
        ));
        assert!(flag(json!({"status": 0, "long_remaining": "0"})));
        // An over-quota reply need not carry numbers; its message says so.
        for message in [
            "Daily Search Limit Exceeded.",
            "your DAILY LIMIT is spent",
            "Search Limit Exceeded",
        ] {
            assert!(flag(json!({"status": -1, "message": message})), "{message}");
        }
        // A bad key or a transient failure must not halt a batch.
        assert!(!flag(json!({"status": -1, "message": "Invalid API key."})));
        assert!(!flag(json!({"status": -1, "message": "30 second limit"})));
        assert!(!flag(json!({"status": -1})));
    }

    #[test]
    fn an_error_status_carries_the_message_and_the_quota_read_before_it() {
        let a = answer(
            json!({"status": -2, "message": "Invalid API key.", "short_limit": 6}),
            json!([]),
        );
        assert_eq!(a.result.unwrap_err().0, "Invalid API key.");
        assert_eq!(a.quota.unwrap().short_limit, Some(6));
        let a = answer(json!({"status": -2}), json!([]));
        assert_eq!(a.result.unwrap_err().0, "Unknown SauceNAO error");
        // A positive status (SauceNAO sends 1 for some index failures) is not an error.
        assert!(answer(json!({"status": 1}), json!([])).result.is_ok());
    }

    #[test]
    fn a_bad_status_or_body_fails_before_any_quota_is_read() {
        let s = settings(&["character"], "");
        let rows = [
            (
                413,
                "{}",
                "SauceNAO rejected the image as too large, even after downscaling",
            ),
            (
                429,
                "{}",
                "SauceNAO rate limit reached, slow down the search delay",
            ),
            (500, "{}", "SauceNAO returned HTTP 500"),
            (200, "<html>", "SauceNAO returned invalid JSON"),
            (
                200,
                "{}",
                "SauceNAO's response didn't include the expected header data",
            ),
            (
                200,
                r#"{"header": {}}"#,
                "SauceNAO's response didn't include the expected header data",
            ),
            (
                200,
                "null",
                "SauceNAO's response didn't include the expected header data",
            ),
        ];
        for (status, body, message) in rows {
            let a = parse_json_response(&s, status, body);
            assert_eq!(a.result.unwrap_err().0, message, "{status} {body}");
            assert_eq!((a.quota, a.daily_limit_reported), (None, false));
        }
    }

    #[test]
    fn every_mirrored_url_is_a_candidate_and_a_repeat_is_not() {
        let both = mirrored(json!([
            "https://chan.sankakucomplex.com/post/show/12345",
            "https://e621.net/posts/67890",
        ]));
        let urls: Vec<_> = both.iter().map(|m| m.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://chan.sankakucomplex.com/post/show/12345",
                "https://e621.net/posts/67890"
            ]
        );
        assert!(both.iter().all(|m| m.similarity == 97.16));
        assert!(
            both.iter()
                .all(|m| m.source_name.as_deref() == Some("Index #29: e621.net - 1234.jpg"))
        );
        assert_eq!(
            mirrored(json!([
                "https://e621.net/posts/1",
                "https://e621.net/posts/1"
            ]))
            .len(),
            1
        );
    }

    #[test]
    fn a_result_with_no_usable_url_is_skipped() {
        assert!(mirrored(json!(["not a url", ""])).is_empty());
        assert!(mirrored(json!([])).is_empty());
        assert!(mirrored(json!(null)).is_empty());
        assert!(mirrored(json!("https://a.example")).is_empty()); // iterated by character
        assert_eq!(mirrored(json!([" //a.example/1 ", null])).len(), 1);
    }

    #[test]
    fn matches_sort_by_similarity_with_ties_in_result_order() {
        let a = answer(
            json!({"status": 0}),
            json!([
                {"header": {"similarity": 50}, "data": {"ext_urls": ["https://a.example/1"]}},
                {"header": {"similarity": "90.5"}, "data": {"ext_urls": ["https://a.example/2", "https://a.example/3"]}},
                {"header": {"similarity": 90.5}, "data": {"ext_urls": ["https://a.example/4"]}},
                {"data": {"ext_urls": ["https://a.example/5"]}},
            ]),
        );
        let order: Vec<_> = a
            .result
            .unwrap()
            .into_iter()
            .map(|m| (m.url.rsplit('/').next().unwrap().to_owned(), m.similarity))
            .collect();
        assert_eq!(
            order,
            [
                ("2".into(), 90.5),
                ("3".into(), 90.5),
                ("4".into(), 90.5),
                ("1".into(), 50.0),
                ("5".into(), 0.0)
            ]
        );
    }

    #[test]
    fn the_title_is_the_title_else_the_source_and_the_last_operand_stands_when_empty() {
        let title = |data: Value| {
            let mut data = data;
            data["ext_urls"] = json!(["https://a.example/1"]);
            answer(json!({"status": 0}), json!([{"header": {}, "data": data}]))
                .result
                .unwrap()[0]
                .title
                .clone()
        };
        assert_eq!(
            title(json!({"title": "T", "source": "S"})),
            Some("T".into())
        );
        assert_eq!(title(json!({"title": "", "source": "S"})), Some("S".into()));
        assert_eq!(title(json!({"source": "S"})), Some("S".into()));
        assert_eq!(title(json!({"title": "T", "source": ""})), Some("T".into()));
        assert_eq!(title(json!({"title": "", "source": ""})), Some("".into()));
        assert_eq!(
            title(json!({"title": "T", "source": null})),
            Some("T".into())
        );
        assert_eq!(title(json!({})), None);
        assert_eq!(title(json!({"title": null, "source": null})), None);
    }

    #[test]
    fn shapes_the_api_never_sends_fail_the_search_with_a_named_field() {
        for results in [
            json!(null),
            json!(5),
            json!(["junk"]),
            json!([{"header": 5, "data": {"ext_urls": ["https://a.example"]}}]),
            json!([{"header": {"similarity": null}, "data": {"ext_urls": ["https://a.example"]}}]),
            json!([{"header": {"similarity": "x"}, "data": {"ext_urls": ["https://a.example"]}}]),
            json!([{"header": {}, "data": {"ext_urls": [5]}}]),
            json!([{"header": {}, "data": {"ext_urls": 5}}]),
        ] {
            let a = answer(json!({"status": 0}), results.clone());
            assert!(a.result.is_err(), "{results}");
        }
        // Missing keys have Python's defaults.
        assert!(
            parse_json_response(&settings(&[], ""), 200, r#"{"header": {"status": 0}}"#)
                .result
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn tags_follow_the_enabled_types_in_their_order_and_the_namespace_override() {
        let data = json!({
            "characters": "Rem, Ram ,, ",
            "material": ["Re:Zero", " ", "Other"],
            "creator": "",
            "member_name": "member",
            "author_name": "author",
        });
        let rdata = data.as_object().unwrap();
        let tags = |types: &[&str], ns: &str| -> Vec<String> {
            tags_from_result(rdata, &settings(types, ns))
                .iter()
                .map(Tag::display)
                .collect()
        };
        assert_eq!(
            tags(&["character", "material"], ""),
            [
                "character:Rem",
                "character:Ram",
                "material:Re:Zero",
                "material:Other"
            ]
        );
        assert_eq!(
            tags(&["creator", "character"], ""),
            ["creator:member", "character:Rem", "character:Ram"]
        );
        assert_eq!(tags(&["character"], "src"), ["src:Rem", "src:Ram"]);
        assert_eq!(tags(&["unknown", "series"], ""), Vec::<String>::new());
        assert!(tags_from_result(&Map::new(), &settings(&["character", "creator"], "")).is_empty());
        let all = tags_from_result(rdata, &settings(&["material"], ""));
        assert!(all.iter().all(|t| t.source == TagSource::SearchEngine));
    }

    #[test]
    fn non_text_fields_are_printed_as_python_prints_them_before_splitting() {
        let data = json!({"characters": 12, "creator": ["a", 5, "b"]});
        let tags: Vec<_> = tags_from_result(
            data.as_object().unwrap(),
            &settings(&["character", "creator"], ""),
        )
        .iter()
        .map(Tag::display)
        .collect();
        // A non-text member of a list is skipped; Python would raise.
        assert_eq!(tags, ["character:12", "creator:a", "creator:b"]);
    }

    const HTML: &str = r#"<html><body>
      <div class="result"><table><tr>
        <td class="resulttableimage"><img src="//img.example/t1.jpg"></td>
        <td class="resulttablecontent">
          <div class="resulttitle"><strong>Title one</strong> <a href="https://danbooru.donmai.us/posts/1">link</a></div>
          <div class="resultsimilarityinfo">91.5%</div>
        </td></tr></table></div>
      <div class="result result-hidden"><div class="resulttitle"><a href="https://a.example/hidden">x</a></div><div class="resultsimilarityinfo">99%</div></div>
      <div class="result"><div class="resultcontentcolumn"><a href="/relative">x</a></div><div class="resultsimilarityinfo">80%</div></div>
      <div class="result"><div class="resultcontentcolumn"><a href="https://b.example/2">x</a></div><div class="resultsimilarityinfo">oops</div><img src=""></div>
      <div class="result"><div class="resulttitle"><a href="https://c.example/3">x</a></div></div>
    </body></html>"#;

    #[test]
    fn the_html_page_gives_visible_blocks_with_a_link_and_a_similarity() {
        let got = parse_html(HTML);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://danbooru.donmai.us/posts/1");
        assert_eq!(got[0].similarity, 91.5);
        assert_eq!(got[0].title.as_deref(), Some("Title onelink"));
        assert_eq!(
            got[0].thumb_url.as_deref(),
            Some("https://img.example/t1.jpg")
        );
        assert_eq!((got[0].source_name.clone(), got[0].tags.len()), (None, 0));
        // An unreadable similarity is 0, an empty `src` stays empty, no title element is None.
        assert_eq!(got[1].url, "https://b.example/2");
        assert_eq!(
            (
                got[1].similarity,
                got[1].thumb_url.as_deref(),
                got[1].title.as_deref()
            ),
            (0.0, Some(""), None)
        );
    }

    #[test]
    fn the_html_path_maps_the_status_without_a_429_special_case() {
        assert_eq!(parse_html_response(200, HTML).unwrap().len(), 2);
        assert_eq!(
            parse_html_response(413, HTML).unwrap_err().0,
            "SauceNAO rejected the image as too large, even after downscaling"
        );
        assert_eq!(
            parse_html_response(429, HTML).unwrap_err().0,
            "SauceNAO returned HTTP 429"
        );
        assert!(parse_html_response(200, "").unwrap().is_empty());
    }

    #[test]
    fn search_reads_the_json_page_only_with_a_key_and_the_json_setting() {
        let mut s = SauceNaoSettings::default();
        assert!(!uses_json_api(&s));
        s.api_key = "k".into();
        assert!(uses_json_api(&s));
        s.use_json_api = false;
        assert!(!uses_json_api(&s));
        assert_eq!(search_from_response(&s, 200, HTML).unwrap().len(), 2);
        s.use_json_api = true;
        assert_eq!(
            search_from_response(&s, 200, HTML).unwrap_err().0,
            "SauceNAO returned invalid JSON"
        );
    }
}

#[cfg(test)]
mod quota_tests {
    //! The Python tests of the quota helpers (`tests/test_core_utils.py`: `TestSauceNaoQuotaText`,
    //! `TestSauceNaoShortWindowText`, `TestSauceNaoBurstWindow`, `TestQuotaPausePersistence`).
    use super::*;
    use crate::clock::ManualClock;
    use crate::paths::Paths;

    fn short(remaining: Option<i64>) -> SauceNaoQuota {
        SauceNaoQuota {
            short_remaining: remaining,
            short_limit: Some(17),
            ..SauceNaoQuota::default()
        }
    }

    #[test]
    fn headroom_means_no_waiting() {
        // Pausing at one remaining would halve a batch's throughput to avoid a 429 that has not
        // happened.
        for remaining in [1, 5, 17] {
            let quota = short(Some(remaining));
            assert_eq!(
                seconds_until_short_window_clears(Some(&quota), 1000.0, 1000.0),
                0.0
            );
        }
    }

    #[test]
    fn an_exhausted_window_waits_the_rest_of_it_and_never_a_negative_time() {
        let quota = short(Some(0));
        assert_eq!(
            seconds_until_short_window_clears(Some(&quota), 1000.0, 1010.0),
            SHORT_WINDOW_SECONDS - 10.0
        );
        assert_eq!(
            seconds_until_short_window_clears(Some(&quota), 1000.0, 1100.0),
            0.0
        );
        assert_eq!(
            seconds_until_short_window_clears(Some(&quota), 1000.0, 9999.0),
            0.0
        );
    }

    #[test]
    fn nothing_observed_yet_does_not_wait_and_the_daily_limit_is_not_this_functions_business() {
        assert_eq!(seconds_until_short_window_clears(None, 0.0, 1000.0), 0.0);
        let unknown = short(None);
        assert_eq!(
            seconds_until_short_window_clears(Some(&unknown), 0.0, 1000.0),
            0.0
        );
        let spent_day = SauceNaoQuota {
            short_remaining: Some(5),
            short_limit: Some(17),
            long_remaining: Some(0),
            long_limit: Some(5000),
        };
        assert_eq!(
            seconds_until_short_window_clears(Some(&spent_day), 1000.0, 1000.0),
            0.0
        );
    }

    fn daily(remaining: Option<i64>, limit: Option<i64>) -> SauceNaoQuota {
        SauceNaoQuota {
            long_remaining: remaining,
            long_limit: limit,
            ..SauceNaoQuota::default()
        }
    }

    #[test]
    fn the_daily_allowance_reads_as_used_out_of_the_limit_else_what_is_left_else_nothing() {
        assert_eq!(
            describe_quota(Some(&daily(Some(40), Some(100)))),
            "SauceNAO: 60/100 used today"
        );
        assert_eq!(
            describe_quota(Some(&daily(Some(40), None))),
            "SauceNAO: 40 left today"
        );
        // "No search made this session" is not "no quota left".
        assert_eq!(describe_quota(None), "");
        assert_eq!(describe_quota(Some(&daily(None, None))), "");
        assert_eq!(
            describe_quota(Some(&daily(Some(0), Some(100)))),
            "SauceNAO: 100/100 used today"
        );
    }

    #[test]
    fn the_burst_window_reads_as_a_tooltip() {
        let with = |remaining, limit| SauceNaoQuota {
            short_remaining: remaining,
            short_limit: limit,
            ..SauceNaoQuota::default()
        };
        assert_eq!(
            describe_short_window(Some(&with(Some(2), Some(6)))),
            "4/6 used in the current ~30s window"
        );
        assert_eq!(
            describe_short_window(Some(&with(Some(2), None))),
            "2 remaining in the current ~30s window"
        );
        assert_eq!(describe_short_window(None), "");
        assert_eq!(describe_short_window(Some(&with(None, None))), "");
    }

    fn profile() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::Builder::new()
            .prefix("hatate-dan2059-pause-")
            .tempdir()
            .unwrap();
        let home: std::ffi::OsString = dir.path().join("home").into();
        let config: std::ffi::OsString = dir.path().join("config").into();
        let paths = Paths::from_lookup(|k| match k {
            "HOME" => Some(home.clone()),
            "XDG_CONFIG_HOME" => Some(config.clone()),
            _ => None,
        })
        .unwrap();
        (dir, paths)
    }

    #[test]
    fn a_recorded_pause_round_trips_and_clears() {
        let (_dir, paths) = profile();
        let file = QuotaPauseFile::new(&paths);
        assert_eq!(file.read(), None);
        // 2026-01-15 13:00 UTC.
        let clock = ManualClock::new(1_768_482_000.0, 0.0);
        record_quota_pause(&file, &clock, 42, 58);
        let state = file.read().unwrap();
        assert_eq!((state.searched, state.remaining), (42, 58));
        assert_eq!(state.paused_at, 1_768_482_000.0);
        // The reset time is an absolute, timezone-aware ISO time: the next 00:00 UTC.
        assert_eq!(state.reset_at, "2026-01-16T00:00:00+00:00");
        file.clear().unwrap();
        assert_eq!(file.read(), None);
        // Clearing when nothing was recorded does not raise.
        file.clear().unwrap();
    }

    #[test]
    fn a_pause_that_cannot_be_written_is_logged_and_goes_no_further() {
        let (_dir, paths) = profile();
        let file = QuotaPauseFile::new(&paths);
        // The directory it would be written to is a file.
        let target = paths.saucenao_quota_pause();
        let parent = target.parent().unwrap();
        std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
        std::fs::write(parent, "not a directory").unwrap();
        record_quota_pause(&file, &ManualClock::new(1.0, 0.0), 1, 1);
        assert_eq!(file.read(), None);
    }
}
