//! The polite crawl (`_Fetcher`, `index_creator`), the artist search (`find_creators`) and the
//! bodies of the two Qt workers (`workers/pawchive_index_worker.py`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value};
use tracing::{debug, info};

use super::db::{ImageRow, PawchiveIndex};
use super::{
    API, CREATORS_CACHE_MAX_AGE, CrawlResult, IMAGE_EXTENSIONS, MAX_RETRIES, PAGE_SIZE,
    PawchiveError, REQUEST_DELAY, RETRY_WAIT, THUMBNAIL_URL, USER_AGENT, fingerprints, py_text,
};
use crate::boorus::sizes::py_float;
use crate::clock::{Clock, SystemClock};
use crate::engines::client::urlencode;
use crate::engines::pawchive_lookup::files_of;
use crate::error::ErrorKind;
use crate::hydrus_client::{py_dumps, require_host, type_name};
use crate::image_compare::Source;
use crate::net::{Method, NetClient, NetError, NetRequest, RetryMode};
use crate::pycompat::{self, truthy};
use crate::pyresponse::PyResponse;
use crate::pyurl::requote_uri;
use crate::remote::secs;

/// A `requests` timeout of 30 seconds, the crawl's for every request.
const CRAWL_TIMEOUT: f64 = 30.0;
/// `find_creators`' timeout for the whole creator list.
const LIST_TIMEOUT: f64 = 120.0;
/// Rows are written this many at a time.
const FLUSH_EVERY: usize = 25;

/// The Pawchive client: one place that talks to the site.
pub struct PawchiveCrawler {
    net: NetClient,
    api_base: String,
    thumbnail_base: String,
    clock: Arc<dyn Clock>,
}

impl PawchiveCrawler {
    /// The real Pawchive, on the real clock.
    pub fn new(net: NetClient) -> Self {
        Self::with_hosts(net, API, THUMBNAIL_URL)
    }

    /// The same crawler against other addresses: the API up to `/api/v1` and the thumbnail host
    /// up to `/thumbnail/data` (a local https server in tests). Not reachable from `config.json`,
    /// the environment or a flag.
    pub fn with_hosts(net: NetClient, api_base: &str, thumbnail_base: &str) -> Self {
        PawchiveCrawler {
            net,
            api_base: api_base.to_owned(),
            thumbnail_base: thumbnail_base.to_owned(),
            clock: Arc::new(SystemClock::default()),
        }
    }

    /// The clock the pauses, `Retry-After` waits and the creator cache's age are read from.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// `session.get(url, params=..., timeout=..., headers={"User-Agent": ...})`. The single
    /// `o` parameter is placed ahead of a `#fragment`, as `requests` does.
    fn request(
        &self,
        url: &str,
        offset: Option<usize>,
        timeout: f64,
    ) -> Result<NetRequest, NetError> {
        let (mut url, fragment) = match url.split_once('#') {
            Some((head, fragment)) => (head.to_owned(), Some(fragment.to_owned())),
            None => (url.to_owned(), None),
        };
        if let Some(offset) = offset {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&urlencode(&[("o", &offset.to_string())]));
        }
        if let Some(fragment) = fragment {
            url.push('#');
            url.push_str(&fragment);
        }
        let url = requote_uri(&url);
        require_host(&url)?;
        NetRequest::new(Method::Get, &url, secs(timeout))?
            .header("User-Agent", USER_AGENT)
            .map(|req| req.with_retry(RetryMode::Never).without_host_gate())
    }

    async fn fetch(
        &self,
        url: &str,
        offset: Option<usize>,
        timeout: f64,
    ) -> Result<PyResponse, NetError> {
        let req = self.request(url, offset, timeout)?;
        Ok(PyResponse::from_net(self.net.send(req).await?))
    }

    /// `find_creators`: artists whose name contains `query`, most-favourited first.
    ///
    /// Needs Pawchive's full creator list (~15 MB), downloaded once and kept in `cache` for a week.
    /// `limit` is Python's default of 50 in the app.
    pub async fn find_creators(
        &self,
        query: &str,
        refresh: bool,
        cache: &Path,
        limit: usize,
    ) -> Result<Vec<Value>, PawchiveError> {
        let mut creators: Option<Value> = None;
        if !refresh && cache_is_fresh(cache, self.clock.wall()) {
            creators = std::fs::read(cache)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .filter(|v| !v.is_null());
        }
        let creators = match creators {
            Some(v) => v,
            None => {
                let url = format!("{}/creators", self.api_base);
                let resp = self
                    .fetch(&url, None, LIST_TIMEOUT)
                    .await
                    .map_err(|e| PawchiveError::net(&e))?;
                raise_for_status(&resp)?;
                let creators = resp.json().map_err(|e| json_error(&e))?;
                if let Some(parent) = cache.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(err) = std::fs::write(cache, py_dumps(&creators)) {
                    debug!("Could not cache pawchive's creator list: {err}");
                }
                creators
            }
        };

        let needle = pycompat::strip(query).to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        // `for c in creators`: a list's items, a dict's keys and a string's characters (none of
        // them dicts); a number, a boolean or `null` is not iterable.
        let items: &[Value] = match &creators {
            Value::Array(items) => items,
            Value::Object(_) | Value::String(_) => &[],
            other => {
                return Err(PawchiveError::unexpected(format!(
                    "'{}' object is not iterable",
                    type_name(other)
                )));
            }
        };
        let mut found: Vec<&Map<String, Value>> = items
            .iter()
            .filter_map(Value::as_object)
            .filter(|c| {
                let name = c
                    .get("name")
                    .filter(|v| truthy(v))
                    .map(py_text)
                    .unwrap_or_default();
                let public = c
                    .get("public_id")
                    .filter(|v| truthy(v))
                    .map(py_text)
                    .unwrap_or_default();
                format!("{name} {public}").to_lowercase().contains(&needle)
            })
            .collect();
        // `found.sort(key=lambda c: (-(c.get("favorited") or 0), str(c.get("name") or "").lower()))`
        let mut keyed: Vec<(Favorited, String, &Map<String, Value>)> =
            Vec::with_capacity(found.len());
        for c in found.drain(..) {
            let favorited = favorited_of(c.get("favorited"))?;
            let name = c
                .get("name")
                .filter(|v| truthy(v))
                .map(py_text)
                .unwrap_or_default();
            keyed.push((favorited, name.to_lowercase(), c));
        }
        keyed.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        Ok(keyed
            .into_iter()
            .take(limit)
            .map(|(_, _, c)| Value::Object(c.clone()))
            .collect())
    }

    /// `pause`: sleeps in small steps so Stop is honoured quickly. `false` if stopped.
    async fn pause(&self, seconds: f64, should_stop: &(dyn Fn() -> bool + Send + Sync)) -> bool {
        let end = self.clock.monotonic() + seconds;
        while self.clock.monotonic() < end {
            if should_stop() {
                return false;
            }
            let step = 0.2f64.min((end - self.clock.monotonic()).max(0.0));
            self.clock.sleep(step).await;
        }
        !should_stop()
    }

    /// `_Fetcher.get`: one polite GET. `Ok(None)` is a Stop; an `Err` is the last failure once the
    /// retries are used up (Python re-raises it).
    async fn polite_get(
        &self,
        state: &mut FetchState<'_>,
        url: &str,
        offset: Option<usize>,
    ) -> Result<Option<PyResponse>, PawchiveError> {
        for attempt in 0..=MAX_RETRIES {
            let wait = state.last + state.delay - self.clock.monotonic();
            if !self.pause(wait, state.should_stop).await {
                return Ok(None);
            }
            let resp = match self.fetch(url, offset, CRAWL_TIMEOUT).await {
                Ok(resp) => resp,
                Err(err) => {
                    state.last = self.clock.monotonic();
                    debug!("pawchive request failed: {err}");
                    if attempt == MAX_RETRIES {
                        return Err(PawchiveError::net(&err));
                    }
                    continue;
                }
            };
            state.last = self.clock.monotonic();
            if resp.status == 429 && attempt < MAX_RETRIES {
                let wait = resp
                    .header("Retry-After")
                    .filter(|v| !v.is_empty())
                    .map_or(RETRY_WAIT, |v| py_float(&v).unwrap_or(RETRY_WAIT));
                info!("pawchive is rate limiting - waiting {wait:.0}s");
                if !self.pause(wait, state.should_stop).await {
                    return Ok(None);
                }
                continue;
            }
            return Ok(Some(resp));
        }
        Ok(None)
    }

    /// `index_creator`: indexes (or refreshes) one artist. Safe to stop at any point: what was
    /// fingerprinted is kept, and the next run carries on from there.
    ///
    /// A failure Python's own handler catches (`requests.RequestException`, `ValueError`) is in
    /// [`CrawlResult::error`]; one that escapes Python is the `Err`.
    pub async fn index_creator(
        &self,
        index: &PawchiveIndex,
        service: &str,
        user_id: &str,
        delay: f64,
        should_stop: &(dyn Fn() -> bool + Send + Sync),
        on_progress: &mut (dyn FnMut(&str, usize, usize) + Send),
    ) -> Result<CrawlResult, PawchiveError> {
        let mut result = CrawlResult::new(service, user_id);
        let mut state = FetchState {
            delay,
            should_stop,
            last: 0.0,
        };
        match self
            .crawl(index, &mut result, &mut state, on_progress)
            .await
        {
            Ok(Flow::Early) => return Ok(result),
            Ok(Flow::Finished) => {}
            Err(Abort::Caught(err)) => {
                result.error = Some(err.message().to_owned());
                result.error_kind = Some(err.kind());
            }
            Err(Abort::Escaped(err)) => return Err(err),
        }
        info!(
            "Pawchive index: {}/{} ({:?}) - {} posts, {} images, {} new, {} without a thumbnail{}{}",
            service,
            user_id,
            result.name,
            result.posts,
            result.images,
            result.new,
            result.unfingerprinted,
            if result.stopped { " - stopped" } else { "" },
            result
                .error
                .as_deref()
                .map_or(String::new(), |e| format!(" - {e}")),
        );
        Ok(result)
    }

    async fn crawl(
        &self,
        index: &PawchiveIndex,
        result: &mut CrawlResult,
        state: &mut FetchState<'_>,
        progress: &mut (dyn FnMut(&str, usize, usize) + Send),
    ) -> Result<Flow, Abort> {
        let (service, user_id) = (result.service.clone(), result.user_id.clone());
        let base = format!("{}/{service}/user/{user_id}", self.api_base);

        let Some(profile) = self
            .polite_get(state, &format!("{base}/profile"), None)
            .await
            .map_err(Abort::Caught)?
        else {
            result.stopped = true;
            return Ok(Flow::Early);
        };
        if profile.status == 200 {
            let body = profile.json().map_err(|e| Abort::Caught(json_error(&e)))?;
            result.name = name_of(&body)?;
        } else if profile.status == 404 {
            result.error = Some("pawchive has no such artist".to_owned());
            result.error_kind = Some(ErrorKind::NotFound);
            return Ok(Flow::Early);
        }
        index
            .add_creator(&service, &user_id, result.name.as_deref())
            .map_err(Abort::Escaped)?;

        let shown = |result: &CrawlResult| match result.name.as_deref() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => user_id.clone(),
        };

        let mut posts: Vec<Map<String, Value>> = Vec::new();
        let mut offset = 0usize;
        loop {
            progress(
                &format!("Listing {}'s posts", shown(result)),
                posts.len(),
                0,
            );
            let Some(resp) = self
                .polite_get(state, &format!("{base}/posts"), Some(offset))
                .await
                .map_err(Abort::Caught)?
            else {
                result.stopped = true;
                return Ok(Flow::Early);
            };
            if resp.status != 200 {
                result.error = Some(format!("listing posts failed: HTTP {}", resp.status));
                result.error_kind = Some(ErrorKind::HttpStatus);
                return Ok(Flow::Early);
            }
            let batch = resp.json().map_err(|e| Abort::Caught(json_error(&e)))?;
            let len = match &batch {
                v if !truthy(v) => 0,
                Value::Array(items) => {
                    posts.extend(items.iter().filter_map(Value::as_object).cloned());
                    items.len()
                }
                Value::Object(map) => map.len(),
                Value::String(s) => s.chars().count(),
                other => {
                    return Err(Abort::Escaped(PawchiveError::unexpected(format!(
                        "'{}' object is not iterable",
                        type_name(other)
                    ))));
                }
            };
            if len < PAGE_SIZE {
                break;
            }
            offset += PAGE_SIZE;
        }
        result.posts = posts.len();

        let known = index
            .known_pages(&service, &user_id)
            .map_err(Abort::Escaped)?;
        let mut todo: Vec<(String, i64, String, Option<Value>)> = Vec::new();
        for post in &posts {
            let post_id = post
                .get("id")
                .filter(|v| truthy(v))
                .map(py_text)
                .unwrap_or_default();
            if let Some(attachments) = post.get("attachments")
                && truthy(attachments)
                && matches!(attachments, Value::Number(_) | Value::Bool(_))
            {
                return Err(Abort::Escaped(PawchiveError::unexpected(format!(
                    "'{}' object is not iterable",
                    type_name(attachments)
                ))));
            }
            for (page_index, path) in files_of(post).into_iter().enumerate() {
                if !IMAGE_EXTENSIONS
                    .iter()
                    .any(|ext| path.to_lowercase().ends_with(ext))
                {
                    continue;
                }
                result.images += 1;
                let page_index = page_index as i64;
                if !known.contains(&(post_id.clone(), page_index)) {
                    todo.push((
                        post_id.clone(),
                        page_index,
                        path.to_owned(),
                        post.get("title").cloned(),
                    ));
                }
            }
        }

        let mut rows: Vec<ImageRow> = Vec::new();
        let outcome = self
            .fingerprint_all(index, result, state, progress, &todo, &mut rows, &shown)
            .await;
        // Whatever was fingerprinted is kept however this ends - a network failure halfway
        // through an artist costs nothing done.
        index.add_images(&rows).map_err(Abort::Escaped)?;
        outcome?;
        if !result.stopped {
            index
                .finish_creator(&service, &user_id, result.name.as_deref(), result.posts)
                .map_err(Abort::Escaped)?;
            progress(
                &format!("Indexed {}", shown(result)),
                todo.len(),
                todo.len(),
            );
        }
        Ok(Flow::Finished)
    }

    #[allow(clippy::too_many_arguments)]
    async fn fingerprint_all(
        &self,
        index: &PawchiveIndex,
        result: &mut CrawlResult,
        state: &mut FetchState<'_>,
        progress: &mut (dyn FnMut(&str, usize, usize) + Send),
        todo: &[(String, i64, String, Option<Value>)],
        rows: &mut Vec<ImageRow>,
        shown: &dyn Fn(&CrawlResult) -> String,
    ) -> Result<(), Abort> {
        let (service, user_id) = (result.service.clone(), result.user_id.clone());
        for (done, (post_id, page_index, path, title)) in todo.iter().enumerate() {
            progress(
                &format!("Fingerprinting {}", shown(result)),
                done,
                todo.len(),
            );
            let url = format!("{}{path}", self.thumbnail_base);
            let Some(resp) = self
                .polite_get(state, &url, None)
                .await
                .map_err(Abort::Caught)?
            else {
                result.stopped = true;
                break;
            };
            let (mut h8, mut h16) = (None, None);
            if resp.status == 200 {
                (h8, h16) = fingerprints(Source::Bytes(&resp.content))
                    .map_err(|e| Abort::Escaped(PawchiveError::bomb(&e)))?;
            }
            if h16.is_none() {
                result.unfingerprinted += 1;
            }
            rows.push(ImageRow {
                service: service.clone(),
                user_id: user_id.clone(),
                post_id: post_id.clone(),
                page_index: *page_index,
                sha256: sha256_in_path(path),
                path: path.clone(),
                title: title_of(title.as_ref())?,
                dhash8: h8.map(|h| format!("{:016x}", h.0[3])),
                dhash16: h16.map(|h| {
                    format!(
                        "{:016x}{:016x}{:016x}{:016x}",
                        h.0[0], h.0[1], h.0[2], h.0[3]
                    )
                }),
            });
            result.new += 1;
            if rows.len() >= FLUSH_EVERY {
                index.add_images(rows).map_err(Abort::Escaped)?;
                rows.clear();
            }
        }
        Ok(())
    }
}

struct FetchState<'a> {
    delay: f64,
    should_stop: &'a (dyn Fn() -> bool + Send + Sync),
    /// `_last`: when the last request ended, on the clock's monotonic reading.
    last: f64,
}

enum Flow {
    /// `return result` before the summary line.
    Early,
    Finished,
}

enum Abort {
    /// What `except (requests.RequestException, ValueError)` catches: `CrawlResult.error`.
    Caught(PawchiveError),
    /// Anything else, which leaves Python's `index_creator`.
    Escaped(PawchiveError),
}

/// `_SHA256_IN_PATH_RE`: the SHA-256 a path ends with (`/<64 hex>.<ext>`), or `""`.
fn sha256_in_path(path: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/([0-9a-f]{64})\.[A-Za-z0-9]+$").expect("sha256 path regex"));
    RE.captures(path)
        .map_or_else(String::new, |c| c[1].to_owned())
}

/// `(profile.json() or {}).get("name")`.
fn name_of(body: &Value) -> Result<Option<String>, Abort> {
    if !truthy(body) {
        return Ok(None);
    }
    let Value::Object(map) = body else {
        return Err(Abort::Escaped(PawchiveError::unexpected(format!(
            "'{}' object has no attribute 'get'",
            type_name(body)
        ))));
    };
    match map.get("name") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Abort::Escaped(PawchiveError::unexpected(format!(
            "'name' is a {} where a string is expected",
            type_name(other)
        )))),
    }
}

/// A post's `title` as the column holds it.
fn title_of(title: Option<&Value>) -> Result<Option<String>, Abort> {
    match title {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Abort::Escaped(PawchiveError::unexpected(format!(
            "'title' is a {} where a string is expected",
            type_name(other)
        )))),
    }
}

/// `-(c.get("favorited") or 0)` as a sort key.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct Favorited(f64);

fn favorited_of(value: Option<&Value>) -> Result<Favorited, PawchiveError> {
    match value {
        None => Ok(Favorited(0.0)),
        Some(v) if !truthy(v) => Ok(Favorited(0.0)),
        Some(Value::Bool(_)) => Ok(Favorited(1.0)),
        Some(Value::Number(n)) => Ok(Favorited(n.as_f64().unwrap_or(0.0))),
        Some(other) => Err(PawchiveError::unexpected(format!(
            "bad operand type for unary -: '{}'",
            type_name(other)
        ))),
    }
}

/// `cache.exists() and time.time() - cache.stat().st_mtime < CREATORS_CACHE_MAX_AGE`.
fn cache_is_fresh(cache: &Path, now: f64) -> bool {
    let Ok(modified) = std::fs::metadata(cache).and_then(|m| m.modified()) else {
        return false;
    };
    let Ok(since_epoch) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    now - since_epoch.as_secs_f64() < CREATORS_CACHE_MAX_AGE
}

/// `resp.raise_for_status()`: `404 Client Error: Not Found for url: ...`.
fn raise_for_status(resp: &PyResponse) -> Result<(), PawchiveError> {
    if resp.status < 400 {
        return Ok(());
    }
    let reason = http::StatusCode::from_u16(resp.status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("");
    let class = if resp.status < 500 {
        "Client"
    } else {
        "Server"
    };
    Err(PawchiveError::status(
        resp.status,
        format!(
            "{} {class} Error: {reason} for url: {}",
            resp.status, resp.url
        ),
    ))
}

/// `resp.json()` failing: Python's `JSONDecodeError` is a `ValueError`.
fn json_error(err: &serde_json::Error) -> PawchiveError {
    PawchiveError::unexpected(err.to_string())
}

/// What the Qt `PawchiveIndexWorker.run` leaves behind: the results it emitted, and the failure
/// that ended it early (Python logs it with `log.exception` and emits `finished_all`).
#[derive(Debug, Default)]
pub struct IndexJobsOutcome {
    pub results: Vec<CrawlResult>,
    pub aborted: Option<PawchiveError>,
}

/// `PawchiveIndexWorker.run`, without Qt: indexes a list of artists one after another. `stop` is
/// `request_stop`; `on_progress` is the `progress` signal and `on_creator_done` the `creator_done`
/// signal. The caller emits `finished_all` when this returns.
pub async fn run_index_jobs(
    crawler: &PawchiveCrawler,
    index: &PawchiveIndex,
    jobs: &[(String, String)],
    stop: &AtomicBool,
    on_progress: &mut (dyn FnMut(&str, usize, usize) + Send),
    on_creator_done: &mut (dyn FnMut(&CrawlResult) + Send),
) -> IndexJobsOutcome {
    let mut outcome = IndexJobsOutcome::default();
    let should_stop = || stop.load(Ordering::SeqCst);
    for (service, user_id) in jobs {
        if should_stop() {
            break;
        }
        match crawler
            .index_creator(
                index,
                service,
                user_id,
                REQUEST_DELAY,
                &should_stop,
                on_progress,
            )
            .await
        {
            Ok(result) => {
                on_creator_done(&result);
                outcome.results.push(result);
            }
            Err(err) => {
                // A background tool must not be able to take the app down.
                tracing::error!("Pawchive indexing failed: {err}");
                outcome.aborted = Some(err);
                break;
            }
        }
    }
    outcome
}

/// `PawchiveFindWorker.run`, without Qt: the artists found and the error text, as the `done`
/// signal carries them.
pub async fn find_creators_for_ui(
    crawler: &PawchiveCrawler,
    query: &str,
    cache: &Path,
) -> (Vec<Value>, Option<PawchiveError>) {
    match crawler.find_creators(query, false, cache, 50).await {
        Ok(found) => (found, None),
        Err(err) => {
            tracing::warn!("Pawchive artist search failed: {err}");
            (Vec::new(), Some(err))
        }
    }
}
