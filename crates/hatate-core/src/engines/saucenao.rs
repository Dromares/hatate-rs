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

use serde::Serialize;
use serde_json::{Map, Value};
use tracing::{debug, error, warn};

use super::looks_like_url;
use crate::boorus::parsed::{classes_of, soup_of, text_of};
use crate::boorus::sizes::py_float;
use crate::config::SauceNaoSettings;
use crate::model::{Tag, TagSource};
use crate::pyrepr::py_str;
use crate::pycompat;

pub const API_URL: &str = "https://saucenao.com/search.php";

/// SauceNAO's documented burst window. The client waits all of it before a request when the last
/// reading says the window is spent, and after a 429.
pub const SHORT_WINDOW_SECONDS: f64 = 30.0;

/// Substrings SauceNAO uses when the daily allowance is spent, matched case-insensitively against
/// the header's `message`. Deliberately excludes the short-window wording.
pub const DAILY_LIMIT_MARKERS: [&str; 3] = ["daily search limit", "daily limit", "search limit exceeded"];

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
        warn!("SauceNAO JSON response had no header object - treating it as a failure, not a confirmed result");
        return early("SauceNAO's response didn't include the expected header data");
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
        result,
    };

    // `header.get("status", 0) < 0`
    if header
        .get("status")
        .and_then(pycompat::number)
        .is_some_and(|status| status < 0.0)
    {
        let message = header
            .get("message")
            .map_or_else(|| "Unknown SauceNAO error".to_owned(), py_str);
        // The error text can say so when the response carries no usable quota numbers.
        let lowered = message.to_lowercase();
        if DAILY_LIMIT_MARKERS.iter().any(|marker| lowered.contains(marker)) {
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
        Some(Value::String(text)) => Ok(text.chars().map(|c| Value::String(c.to_string())).collect()),
        Some(Value::Null) if what == "ext_urls" => Ok(Vec::new()),
        Some(other) if what == "ext_urls" && !pycompat::truthy(other) => Ok(Vec::new()),
        Some(_) => Err(failure(format!("SauceNAO's {what} is not a list"))),
    }
}

/// The result loop of `_search_json`: one candidate per distinct usable URL, best first.
fn results_of(data: &Value, settings: &SauceNaoSettings) -> Result<Vec<SauceNaoMatch>, SauceNaoError> {
    let mut matches: Vec<SauceNaoMatch> = Vec::new();
    let mut skipped_no_url = 0usize;
    let empty = Map::new();
    for result in items_of(data.get("results"), "results")? {
        let Some(result) = result.as_object() else {
            return Err(failure("SauceNAO's results held something that is not an object"));
        };
        let rheader = result.get("header").map_or(Some(&empty), Value::as_object);
        let rdata = result.get("data").map_or(Some(&empty), Value::as_object);
        let (Some(rheader), Some(rdata)) = (rheader, rdata) else {
            return Err(failure("SauceNAO's result had a header or data that is not an object"));
        };
        let similarity = match rheader.get("similarity") {
            None => 0.0,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::String(text)) => py_float(text)
                .ok_or_else(|| failure("SauceNAO's similarity is not a number"))?,
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
                _ => return Err(failure("SauceNAO's ext_urls held something that is not text")),
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
    for block in soup.select(&any).filter(|el| has_class(*el, "div", "result")) {
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
                    has_class(anc, "div", "resulttitle") || has_class(anc, "div", "resultcontentcolumn")
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
