//! `core/hydrus_import_poll.py` and the logic of `workers/hydrus_import_poll_worker.py` without Qt
//! (Phase 4.d, DAN-2131): confirming that Hydrus's own downloader has finished importing a URL,
//! as opposed to having accepted the request.
//!
//! `POST /add_urls/add_url` only says Hydrus *accepted* the URL; the fetch and import happen
//! afterwards. `GET /add_urls/get_url_files` reflects the database: `url_file_statuses` is empty
//! while nothing has been imported and gains an entry (status 2, already in the database, or 3,
//! previously deleted) once something has.
//!
//! **Time.** Python reads `time.monotonic()` and calls `time.sleep()`; here both go through the
//! [`Clock`], so a test hands in one that answers at once and reads the sleeps back.
//!
//! **Errors.** A `HydrusError` from a poll request is logged and retried at the next round, like
//! Python. What Python lets propagate (an answer of the wrong shape: `AttributeError`,
//! `TypeError`) is `Err` here, with [`ErrorKind::UnexpectedResponse`].
//!
//! **The worker.** Python checks the pending entries on a `ThreadPoolExecutor` of at most 8 and
//! handles each result as it completes. Here the round's checks run concurrently (at most
//! [`MAX_CONCURRENT_CHECKS`] in flight, on the caller's task) and their results are handled in
//! the order the entries were given; the bookkeeping that follows a confirmed import
//! (`finish_url_import`) therefore starts after the round's checks are all in, where Python's
//! starts as soon as the first one is. The set of requests is the same; their interleaving was
//! never fixed in Python either. Signals are the methods of [`PollEvents`]; an entry is read, not
//! written, while the poll runs (the GUI handler that replaces `hydrus_hash` is a queued slot
//! there, and Phase 6 here).

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;

use serde_json::Value;
use tracing::{debug, info, warn};

use crate::clock::Clock;
use crate::config::Settings;
use crate::error::ErrorKind;
use crate::hydrus_client::{HydrusClient, HydrusError, dict, iterate, type_name};
use crate::hydrus_import::finish_url_import;
use crate::model::Entry;
use crate::pycompat::truthy;

/// Statuses that mean "Hydrus has resolved this URL to a real file". Status 0 ("not in database,
/// ready for import") is a rare transient state, treated as an empty list: keep waiting.
pub const RESOLVED_STATUSES: [i64; 2] = [2, 3];

pub const DEFAULT_TIMEOUT: f64 = 60.0;
pub const DEFAULT_INTERVAL: f64 = 2.0;

/// A cap on simultaneous status checks per round, so a large batch does not hammer Hydrus.
pub const MAX_CONCURRENT_CHECKS: usize = 8;

/// `status in RESOLVED_STATUSES`: Python's `==`, so `2.0` is a hit and `True` is not.
fn is_resolved(status: Option<&Value>) -> bool {
    matches!(status, Some(Value::Number(n))
        if n.as_f64().is_some_and(|f| f == 2.0 || f == 3.0))
}

/// `extract_confirmed_hash`: the file hash if Hydrus has confirmed an import for this URL, or
/// `None` if it is still in progress (or genuinely has nothing).
///
/// Python returns whatever the `hash` field holds; a hash that is not a string is
/// [`ErrorKind::UnexpectedResponse`] here (the same class as 4.c's C2).
pub fn extract_confirmed_hash(
    get_url_files_response: &Value,
) -> Result<Option<String>, HydrusError> {
    let statuses = dict(get_url_files_response, "get")?.get("url_file_statuses");
    let rows = match statuses {
        Some(v) if truthy(v) => iterate(v)?,
        _ => Vec::new(),
    };
    for row in &rows {
        let row = dict(row, "get")?;
        if is_resolved(row.get("status"))
            && let Some(hash) = row.get("hash").filter(|h| truthy(h))
        {
            return match hash {
                Value::String(s) => Ok(Some(s.clone())),
                other => Err(HydrusError::local(
                    ErrorKind::UnexpectedResponse,
                    format!(
                        "Hydrus returned a {} for the file hash, not a string",
                        type_name(other)
                    ),
                )),
            };
        }
    }
    Ok(None)
}

/// `poll_single_url_import`'s optional arguments.
pub struct PollOptions<'a> {
    pub timeout: f64,
    pub interval: f64,
    /// `stop_check()`: asked before every request.
    pub stop_check: Option<&'a (dyn Fn() -> bool + Sync)>,
    /// `on_progress(remaining_seconds, timeout)`: once per check, so a caller can show a countdown.
    pub on_progress: Option<&'a (dyn Fn(f64, f64) + Sync)>,
}

impl Default for PollOptions<'_> {
    fn default() -> Self {
        PollOptions {
            timeout: DEFAULT_TIMEOUT,
            interval: DEFAULT_INTERVAL,
            stop_check: None,
            on_progress: None,
        }
    }
}

/// `poll_single_url_import`: checks Hydrus every `interval` seconds until it confirms the URL has
/// actually been imported (not just accepted) or `timeout` seconds pass. The confirmed hash, or
/// `None` if it never confirmed in time (the import may still be in progress: not necessarily a
/// failure).
pub async fn poll_single_url_import(
    client: &HydrusClient,
    url: &str,
    options: &PollOptions<'_>,
    clock: &dyn Clock,
) -> Result<Option<String>, HydrusError> {
    let start = clock.monotonic();
    loop {
        if options.stop_check.is_some_and(|stop| stop()) {
            return Ok(None);
        }
        let data = match client.get_url_files(url).await {
            Ok(data) => data,
            Err(e) if e.is_hydrus_error_in_python() => {
                warn!("get_url_files failed while polling {url}: {e} (will retry)");
                Value::Object(serde_json::Map::new())
            }
            Err(e) => return Err(e),
        };

        if let Some(hash) = extract_confirmed_hash(&data)? {
            return Ok(Some(hash));
        }

        let elapsed = clock.monotonic() - start;
        let remaining = (options.timeout - elapsed).max(0.0);
        if let Some(progress) = options.on_progress {
            progress(remaining, options.timeout);
        }
        if remaining <= 0.0 {
            return Ok(None);
        }
        clock.sleep(options.interval.min(remaining)).await;
    }
}

// -- The worker, without Qt ---------------------------------------------------------------------

/// One entry waiting on Hydrus, with the URL that was queued for it.
pub struct PollItem<'a> {
    pub entry: &'a Entry,
    pub url: String,
}

/// `HydrusImportPollWorker`'s signals.
pub trait PollEvents {
    /// `entry_resolved(entry, confirmed_hash or None, warning or None)`: `index` is the item's
    /// position in the list given. The warning is what `ImportResult.warning` carries on the
    /// synchronous paths.
    fn entry_resolved(&mut self, index: usize, confirmed_hash: Option<&str>, warning: Option<&str>);
    /// `wait_countdown(label, seconds remaining, total seconds)`; `("", 0.0, 0.0)` clears it.
    fn wait_countdown(&mut self, label: &str, remaining: f64, total: f64);
    /// `finished_all()`.
    fn finished_all(&mut self);
}

/// `HydrusImportPollWorker.__init__`'s arguments beyond the client and the entries.
#[derive(Debug, Clone, Copy)]
pub struct PollWorkerOptions<'a> {
    pub timeout: f64,
    pub interval: f64,
    /// Only read for `set_hydrus_duplicate_relationships` and `write_hydrus_provenance_note`.
    pub settings: Option<&'a Settings>,
}

impl Default for PollWorkerOptions<'_> {
    fn default() -> Self {
        PollWorkerOptions {
            timeout: DEFAULT_TIMEOUT,
            interval: DEFAULT_INTERVAL,
            settings: None,
        }
    }
}

/// `HydrusImportPollWorker.run`: after queuing URLs with Hydrus's own downloader, polls Hydrus to
/// confirm each one actually finished importing.
///
/// Elapsed time is tracked per entry from a real timestamp, so the timeout is honoured however
/// large the batch is. An entry that never confirms in time is resolved with no hash and left in
/// the list (it may still be downloading).
pub async fn run_import_poll(
    client: &HydrusClient,
    items: &[PollItem<'_>],
    options: &PollWorkerOptions<'_>,
    stop: &(dyn Fn() -> bool + Sync),
    events: &mut dyn PollEvents,
    clock: &dyn Clock,
) -> Result<(), HydrusError> {
    let total = items.len();
    info!(
        "Polling Hydrus to confirm {total} queued import(s) (up to {:.0}s each)",
        options.timeout
    );

    let start_times: Vec<f64> = items.iter().map(|_| clock.monotonic()).collect();
    let mut pending: Vec<usize> = (0..total).collect();

    while !pending.is_empty() && !stop() {
        let mut still_pending: Vec<usize> = Vec::new();
        let limit = MAX_CONCURRENT_CHECKS.min(pending.len());
        let checks: Vec<_> = pending
            .iter()
            .map(|&i| check_one(client, &items[i]))
            .collect();
        let results = join_bounded(checks, limit).await;

        for (&i, checked) in pending.iter().zip(results) {
            if stop() {
                // Stop requested: ignore this result, emit nothing.
                continue;
            }
            let item = &items[i];
            if let Some(hash) = checked? {
                info!(
                    "Confirmed Hydrus import for {} (hash={hash})",
                    item.entry.filename()
                );
                // Done here, past the stop check: a relationship already written into the user's
                // library cannot be taken back, so it is not written speculatively.
                let warning = relate(item.entry, client, options.settings, &hash, clock).await;
                events.entry_resolved(i, Some(&hash), warning.as_deref());
                continue;
            }

            let elapsed = clock.monotonic() - start_times[i];
            if elapsed >= options.timeout {
                warn!(
                    "Could not confirm Hydrus import for {} within {:.0}s - leaving it in the \
                     list (it may still be downloading)",
                    item.entry.filename(),
                    options.timeout
                );
                events.entry_resolved(i, None, None);
            } else {
                still_pending.push(i);
            }
        }

        pending = still_pending;
        if !pending.is_empty() && !stop() {
            debug!(
                "{}/{total} still pending confirmation, next check in {:.0}s",
                pending.len(),
                options.interval
            );
            events.wait_countdown(
                &format!("Confirming {} Hydrus import(s)", pending.len()),
                options.interval,
                options.interval,
            );
            clock.sleep(options.interval).await;
        }
    }

    // Clear the countdown display.
    events.wait_countdown("", 0.0, 0.0);
    info!("Hydrus import poll finished");
    events.finished_all();
    Ok(())
}

/// `_check_one`: one status check. A `HydrusError` is a warning and "not yet".
async fn check_one(
    client: &HydrusClient,
    item: &PollItem<'_>,
) -> Result<Option<String>, HydrusError> {
    let data = match client.get_url_files(&item.url).await {
        Ok(data) => data,
        Err(e) if e.is_hydrus_error_in_python() => {
            warn!(
                "get_url_files failed for {}: {e} (will retry)",
                item.entry.filename()
            );
            Value::Object(serde_json::Map::new())
        }
        Err(e) => return Err(e),
    };
    extract_confirmed_hash(&data)
}

/// `_relate`: the bookkeeping that follows a confirmed import. Never fails: it runs after Hydrus
/// has confirmed the import, and losing the confirmation over what follows it would be worse than
/// the missing relationship or note. `entry.hydrus_hash` is still the local copy's hash here.
async fn relate(
    entry: &Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    confirmed_hash: &str,
    clock: &dyn Clock,
) -> Option<String> {
    match finish_url_import(
        entry,
        client,
        settings,
        entry.hydrus_hash.as_deref(),
        confirmed_hash,
        clock,
    )
    .await
    {
        Ok(warning) => warning,
        Err(e) => {
            warn!(
                "Could not finish the Hydrus import bookkeeping for {}: {e}",
                entry.filename()
            );
            None
        }
    }
}

/// Runs the futures to completion with at most `limit` in flight at once, and returns their
/// outputs in the order given (a `ThreadPoolExecutor` of `limit` workers, awaited together).
async fn join_bounded<F: Future>(futures: Vec<F>, limit: usize) -> Vec<F::Output> {
    let count = futures.len();
    let mut futures: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut results: Vec<Option<F::Output>> = (0..count).map(|_| None).collect();
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    std::future::poll_fn(|cx| {
        loop {
            while active.len() < limit.max(1) && next < count {
                active.push(next);
                next += 1;
            }
            let before = active.len();
            active.retain(|&i| match futures[i].as_mut().poll(cx) {
                Poll::Ready(output) => {
                    results[i] = Some(output);
                    false
                }
                Poll::Pending => true,
            });
            if active.is_empty() && next >= count {
                return Poll::Ready(());
            }
            if active.len() == before {
                return Poll::Pending;
            }
        }
    })
    .await;
    results
        .into_iter()
        .map(|r| r.expect("every future completed"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn join_bounded_keeps_order_and_never_exceeds_the_limit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let futures: Vec<_> = (0..20)
            .map(|n| {
                let (live, peak) = (&live, &peak);
                async move {
                    let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    tokio::task::yield_now().await;
                    live.fetch_sub(1, Ordering::SeqCst);
                    n * 2
                }
            })
            .collect();
        let out = join_bounded(futures, 8).await;
        assert_eq!(out, (0..20).map(|n| n * 2).collect::<Vec<_>>());
        assert_eq!(peak.load(Ordering::SeqCst), 8);
        assert!(
            join_bounded(Vec::<std::future::Ready<u8>>::new(), 8)
                .await
                .is_empty()
        );
    }

    #[test]
    fn only_status_two_and_three_with_a_hash_confirm() {
        let hash = "a".repeat(64);
        let ok = |status: Value| {
            extract_confirmed_hash(
                &json!({"url_file_statuses": [{"status": status, "hash": hash}]}),
            )
            .unwrap()
        };
        assert_eq!(ok(json!(2)), Some(hash.clone()));
        assert_eq!(ok(json!(3)), Some(hash.clone()));
        assert_eq!(ok(json!(2.0)), Some(hash.clone()));
        assert_eq!(ok(json!(0)), None);
        assert_eq!(ok(json!(1)), None);
        assert_eq!(ok(json!(true)), None);
        assert_eq!(ok(json!("2")), None);
        assert_eq!(extract_confirmed_hash(&json!({})).unwrap(), None);
        assert_eq!(
            extract_confirmed_hash(&json!({"url_file_statuses": []})).unwrap(),
            None
        );
        assert_eq!(
            extract_confirmed_hash(&json!({"url_file_statuses": [{"status": 2, "hash": ""}]}))
                .unwrap(),
            None
        );
        // The first resolved row with a hash wins; an unresolved row before it is skipped.
        assert_eq!(
            extract_confirmed_hash(&json!({"url_file_statuses": [
                {"status": 0, "hash": "x"}, {"status": 3, "hash": "y"}, {"status": 2, "hash": "z"}]}))
            .unwrap(),
            Some("y".to_owned())
        );
    }

    #[test]
    fn an_answer_of_the_wrong_shape_is_python_s_attribute_error() {
        let message = |v: Value| extract_confirmed_hash(&v).unwrap_err().message().to_owned();
        assert_eq!(message(json!([])), "'list' object has no attribute 'get'");
        assert_eq!(
            message(json!({"url_file_statuses": [3]})),
            "'int' object has no attribute 'get'"
        );
        assert_eq!(
            message(json!({"url_file_statuses": 5})),
            "'int' object is not iterable"
        );
        assert_eq!(
            message(json!({"url_file_statuses": "ab"})),
            "'str' object has no attribute 'get'"
        );
        let e = extract_confirmed_hash(&json!({"url_file_statuses": [{"status": 2, "hash": [1]}]}))
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::UnexpectedResponse);
    }
}
