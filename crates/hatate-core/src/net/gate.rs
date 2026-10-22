//! Per-host gating: the `x-ratelimit-*` wait of `core/net.py:148-181`, and the
//! least-gap-between-requests of the Pawchive crawl (`core/pawchive_index.py:408`).
//!
//! All timing uses `tokio::time`, so a paused test clock controls it.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use http::HeaderMap;
use tokio::time::{Instant, sleep, sleep_until};
use tower::{Layer, Service};

use super::error::{ErrorKind, NetError};
use super::request::{NetRequest, NetResponse};

/// The longest a request waits for a host's window to reset. Past this it is
/// refused at once (`core/net.py:59-62`).
pub const MAX_HOST_WAIT: Duration = Duration::from_secs(30);

/// A reset longer than this is treated as this long (an `inf` header must not
/// panic `Duration`).
const MAX_RESET: f64 = 1.0e6;

#[derive(Default)]
struct HostState {
    /// `core/net.py` `_resume_at`: when a host that ran out takes requests again.
    resume_at: Option<Instant>,
    /// Earliest start of the next request that asked for a minimum gap.
    not_before: Option<Instant>,
}

/// Shared per-host state for one client.
#[derive(Clone, Default)]
pub struct HostGate {
    hosts: Arc<Mutex<HashMap<String, HostState>>>,
}

impl HostGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every host (`close_all`, `core/net.py:86-93`).
    pub fn clear(&self) {
        self.hosts.lock().unwrap().clear();
    }

    /// How long until `host` takes requests again, if it said it ran out.
    pub fn resume_in(&self, host: &str) -> Option<Duration> {
        let hosts = self.hosts.lock().unwrap();
        let at = hosts.get(host)?.resume_at?;
        let left = at.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    }

    /// `_wait_for_host`: sleeps out a reset of up to `MAX_HOST_WAIT`, refuses a longer one.
    pub(crate) async fn wait_for_reopen(
        &self,
        host: &str,
        req: &NetRequest,
    ) -> Result<(), NetError> {
        let Some(wait) = self.resume_in(host) else {
            return Ok(());
        };
        if wait > MAX_HOST_WAIT {
            return Err(NetError::new(
                ErrorKind::RateLimited,
                Some(&req.url),
                format!(
                    "{host} has no requests left for another {}s",
                    wait.as_secs()
                ),
            ));
        }
        tracing::info!(
            host,
            wait_secs = wait.as_secs_f64(),
            "host has no requests left, waiting for it to reset"
        );
        sleep(wait).await;
        Ok(())
    }

    /// Reserves the next slot for `host` and sleeps until it, so concurrent
    /// requests are spaced `gap` apart.
    pub(crate) async fn wait_min_gap(&self, host: &str, gap: Duration) {
        let start = {
            let mut hosts = self.hosts.lock().unwrap();
            let state = hosts.entry(host.to_owned()).or_default();
            let now = Instant::now();
            let start = state.not_before.map_or(now, |t| t.max(now));
            state.not_before = Some(start + gap);
            start
        };
        if start > Instant::now() {
            sleep_until(start).await;
        }
    }

    /// Notes a finished exchange: the host's own rate-limit headers, and the gap
    /// measured from when the response arrived.
    pub(crate) fn note_response(
        &self,
        host: &str,
        headers: Option<&HeaderMap>,
        gap: Option<Duration>,
    ) {
        let mut hosts = self.hosts.lock().unwrap();
        let state = hosts.entry(host.to_owned()).or_default();
        if let Some(gap) = gap {
            state.not_before = Some(Instant::now() + gap);
        }
        let Some(headers) = headers else { return };
        let Some((remaining, reset)) = rate_limit_headers(headers) else {
            return;
        };
        if remaining < 1.0 {
            state.resume_at = Some(Instant::now() + Duration::from_secs_f64(reset));
        } else {
            state.resume_at = None;
        }
    }
}

/// `x-ratelimit-remaining` and `x-ratelimit-reset`, both required, both floats
/// (`core/net.py:162-181`). The reset is clamped to `0..=MAX_RESET` seconds; a
/// NaN counts as 0, as Python's `max(0.0, nan)` does.
fn rate_limit_headers(headers: &HeaderMap) -> Option<(f64, f64)> {
    let get = |name: &str| headers.get(name)?.to_str().ok()?.trim().parse::<f64>().ok();
    let remaining = get("x-ratelimit-remaining")?;
    let reset = get("x-ratelimit-reset")?;
    let reset = if reset.is_nan() {
        0.0
    } else {
        reset.clamp(0.0, MAX_RESET)
    };
    Some((remaining, reset))
}

/// Outermost layer: waits out the host's reset once, then starts the clock the
/// deadline is measured from (`core/net.py:121-122`).
#[derive(Clone)]
pub(crate) struct AdmitLayer {
    pub(crate) gate: HostGate,
}

impl<S> Layer<S> for AdmitLayer {
    type Service = Admit<S>;
    fn layer(&self, inner: S) -> Admit<S> {
        Admit {
            inner,
            gate: self.gate.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Admit<S> {
    inner: S,
    gate: HostGate,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S> Service<NetRequest> for Admit<S>
where
    S: Service<NetRequest, Response = NetResponse, Error = NetError> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = NetResponse;
    type Error = NetError;
    type Future = BoxFuture<Result<NetResponse, NetError>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), NetError>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: NetRequest) -> Self::Future {
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let gate = self.gate.clone();
        Box::pin(async move {
            let host = req.host_key();
            gate.wait_for_reopen(&host, &req).await?;
            req.started = Some(Instant::now());
            inner.call(req).await
        })
    }
}

/// Inside the retry layer, so it runs on every attempt: honours `min_gap`, and
/// notes each response's rate-limit headers.
#[derive(Clone)]
pub(crate) struct PaceLayer {
    pub(crate) gate: HostGate,
}

impl<S> Layer<S> for PaceLayer {
    type Service = Pace<S>;
    fn layer(&self, inner: S) -> Pace<S> {
        Pace {
            inner,
            gate: self.gate.clone(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Pace<S> {
    inner: S,
    gate: HostGate,
}

impl<S> Service<NetRequest> for Pace<S>
where
    S: Service<NetRequest, Response = NetResponse, Error = NetError> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = NetResponse;
    type Error = NetError;
    type Future = BoxFuture<Result<NetResponse, NetError>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), NetError>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: NetRequest) -> Self::Future {
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let gate = self.gate.clone();
        Box::pin(async move {
            let host = req.host_key();
            let gap = req.min_gap;
            if let Some(gap) = gap {
                gate.wait_min_gap(&host, gap).await;
            }
            let result = inner.call(req).await;
            gate.note_response(&host, result.as_ref().ok().map(|r| &r.headers), gap);
            result
        })
    }
}
