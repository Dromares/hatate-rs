//! Retry rules as a `tower::retry::Policy` (docs/net.md sections 1 and 3).

use std::time::Duration;

use http::HeaderMap;
use http::StatusCode;
use tokio::time::{Instant, Sleep, sleep};
use tower::retry::Policy;

use super::error::NetError;
use super::request::{Method, NetRequest, NetResponse, RetryMode};

/// `RETRY_AFTER_DEFAULT` and `RETRY_AFTER_MAX` (`core/net.py:54-55`).
pub const RETRY_AFTER_DEFAULT: Duration = Duration::from_secs(2);
pub const RETRY_AFTER_MAX: Duration = Duration::from_secs(10);

/// A `Retry-After` longer than this is capped (Python has no cap; `inf` must not panic).
const MAX_POLITE_WAIT: f64 = 1.0e6;

/// The wait `core/net.py:_retry_after` allows: the header's seconds when they are
/// a number in `0..=10`, 2 s when absent, and `None` (do not retry) for an HTTP
/// date, a negative, a NaN or anything above 10 s.
pub fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let Some(raw) = headers.get(http::header::RETRY_AFTER) else {
        return Some(RETRY_AFTER_DEFAULT);
    };
    let secs: f64 = raw.to_str().ok()?.trim().parse().ok()?;
    let max = RETRY_AFTER_MAX.as_secs_f64();
    (0.0..=max)
        .contains(&secs)
        .then(|| Duration::from_secs_f64(secs))
}

/// The Pawchive crawl's wait: `Retry-After` as a float, else `default_wait`
/// (`core/pawchive_index.py:420-424`). An empty header counts as absent; a
/// negative or NaN is no wait; a huge value is capped.
pub fn polite_wait(headers: &HeaderMap, default_wait: Duration) -> Duration {
    let parsed = headers
        .get(http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<f64>().ok());
    match parsed {
        None => default_wait,
        Some(secs) if secs.is_nan() || secs <= 0.0 => Duration::ZERO,
        Some(secs) => Duration::from_secs_f64(secs.min(MAX_POLITE_WAIT)),
    }
}

/// One instance per call; `attempts` counts the retries already made.
#[derive(Clone, Default)]
pub(crate) struct RetryRules {
    attempts: u32,
}

impl Policy<NetRequest, NetResponse, NetError> for RetryRules {
    type Future = Sleep;

    fn retry(
        &mut self,
        req: &mut NetRequest,
        result: &mut Result<NetResponse, NetError>,
    ) -> Option<Sleep> {
        match req.retry {
            RetryMode::Never => None,
            RetryMode::Once429 => {
                if self.attempts > 0 || !matches!(req.method, Method::Get | Method::Head) {
                    return None;
                }
                let resp = result.as_ref().ok()?;
                if resp.status != StatusCode::TOO_MANY_REQUESTS {
                    return None;
                }
                let wait = retry_after(&resp.headers)?;
                if let (Some(deadline), Some(started)) = (req.deadline, req.started) {
                    let spare =
                        deadline.saturating_sub(Instant::now().saturating_duration_since(started));
                    if wait >= spare {
                        return None;
                    }
                }
                self.attempts += 1;
                tracing::info!(
                    host = req.url.host_str().unwrap_or(""),
                    wait_secs = wait.as_secs_f64(),
                    "answered 429, retrying once"
                );
                Some(sleep(wait))
            }
            RetryMode::Polite {
                max_retries,
                default_wait,
            } => {
                if self.attempts >= max_retries {
                    return None;
                }
                let wait = match result {
                    Err(e) if e.is_transport() => Duration::ZERO,
                    Ok(resp) if resp.status == StatusCode::TOO_MANY_REQUESTS => {
                        polite_wait(&resp.headers, default_wait)
                    }
                    _ => return None,
                };
                self.attempts += 1;
                tracing::info!(
                    host = req.url.host_str().unwrap_or(""),
                    wait_secs = wait.as_secs_f64(),
                    attempt = self.attempts,
                    "retrying"
                );
                Some(sleep(wait))
            }
        }
    }

    fn clone_request(&mut self, req: &NetRequest) -> Option<NetRequest> {
        Some(req.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    fn headers(value: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = value {
            h.insert(http::header::RETRY_AFTER, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn retry_after_follows_core_net() {
        assert_eq!(retry_after(&headers(None)), Some(Duration::from_secs(2)));
        assert_eq!(
            retry_after(&headers(Some("3"))),
            Some(Duration::from_secs(3))
        );
        assert_eq!(retry_after(&headers(Some("0"))), Some(Duration::ZERO));
        assert_eq!(
            retry_after(&headers(Some("10"))),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            retry_after(&headers(Some("1.5"))),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(retry_after(&headers(Some("10.5"))), None);
        assert_eq!(retry_after(&headers(Some("-1"))), None);
        assert_eq!(retry_after(&headers(Some("nan"))), None);
        assert_eq!(retry_after(&headers(Some("inf"))), None);
        assert_eq!(
            retry_after(&headers(Some("Wed, 21 Oct 2026 07:28:00 GMT"))),
            None
        );
    }

    #[test]
    fn polite_wait_follows_pawchive_index() {
        let default = Duration::from_secs(30);
        assert_eq!(polite_wait(&headers(None), default), default);
        assert_eq!(polite_wait(&headers(Some("")), default), default);
        assert_eq!(polite_wait(&headers(Some("junk")), default), default);
        assert_eq!(
            polite_wait(&headers(Some("45")), default),
            Duration::from_secs(45)
        );
        assert_eq!(polite_wait(&headers(Some("-4")), default), Duration::ZERO);
        assert_eq!(
            polite_wait(&headers(Some("inf")), default),
            Duration::from_secs_f64(1.0e6)
        );
    }
}
