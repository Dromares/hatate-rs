//! The tower stack against a scripted transport on a paused clock: the waits are
//! exact, nothing sleeps for real, and no socket is involved.

use std::collections::VecDeque;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use tokio::time::Instant;
use tower::Service;
use url::Url;

use super::*;

type Script = VecDeque<Result<NetResponse, NetError>>;

#[derive(Clone)]
struct Mock {
    script: Arc<Mutex<Script>>,
    calls: Arc<Mutex<Vec<(Instant, NetRequest)>>>,
}

impl Mock {
    fn new(script: Vec<Result<NetResponse, NetError>>) -> Self {
        Mock {
            script: Arc::new(Mutex::new(script.into())),
            calls: Arc::default(),
        }
    }

    fn times(&self) -> Vec<Duration> {
        let calls = self.calls.lock().unwrap();
        let t0 = calls.first().map_or_else(Instant::now, |c| c.0);
        calls.iter().map(|c| c.0 - t0).collect()
    }

    fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Service<NetRequest> for Mock {
    type Response = NetResponse;
    type Error = NetError;
    type Future = Pin<Box<dyn Future<Output = Result<NetResponse, NetError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), NetError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: NetRequest) -> Self::Future {
        self.calls.lock().unwrap().push((Instant::now(), req));
        let next = self.script.lock().unwrap().pop_front();
        Box::pin(ready(next.unwrap_or_else(|| Ok(resp(200, &[])))))
    }
}

fn resp(status: u16, headers: &[(&str, &str)]) -> NetResponse {
    let mut map = HeaderMap::new();
    for (n, v) in headers {
        map.insert(
            HeaderName::from_bytes(n.as_bytes()).unwrap(),
            HeaderValue::from_str(v).unwrap(),
        );
    }
    NetResponse {
        status: StatusCode::from_u16(status).unwrap(),
        headers: map,
        url: Url::parse("http://h.example/").unwrap(),
        body: Bytes::new(),
    }
}

fn get(url: &str) -> NetRequest {
    NetRequest::get(url, Duration::from_secs(30)).unwrap()
}

fn client(mock: &Mock) -> NetClient {
    NetClient::with_service(mock.clone(), NetConfig::default())
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

// --- x-ratelimit-* (core/net.py:148-181) ------------------------------------

#[tokio::test(start_paused = true)]
async fn a_host_that_ran_out_is_waited_for() {
    let mock = Mock::new(vec![
        Ok(resp(
            200,
            &[
                ("x-ratelimit-remaining", "0.0"),
                ("x-ratelimit-reset", "22"),
            ],
        )),
        Ok(resp(200, &[])),
        Ok(resp(200, &[])),
    ]);
    let c = client(&mock);
    c.send(get("http://h.example/a")).await.unwrap();
    c.send(get("http://h.example/b")).await.unwrap();
    // The first call said "none left, reset in 22 s": the second waited for it.
    assert_eq!(mock.times(), vec![secs(0), secs(22)]);
    // And only that host: a different one goes straight through.
    c.send(get("http://other.example/")).await.unwrap();
    assert_eq!(mock.times()[2], secs(22));
}

#[tokio::test(start_paused = true)]
async fn remaining_requests_clear_the_wait() {
    let mock = Mock::new(vec![
        Ok(resp(
            200,
            &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "10")],
        )),
        Ok(resp(
            200,
            &[("x-ratelimit-remaining", "5"), ("x-ratelimit-reset", "10")],
        )),
    ]);
    let c = client(&mock);
    c.send(get("http://h.example/")).await.unwrap();
    // Let the 10 s pass by waiting on it, then the host reports headroom.
    c.send(get("http://h.example/")).await.unwrap();
    assert!(c.gate().resume_in("h.example").is_none());
    c.send(get("http://h.example/")).await.unwrap();
    assert_eq!(mock.times(), vec![secs(0), secs(10), secs(10)]);
}

#[tokio::test(start_paused = true)]
async fn one_header_alone_or_junk_is_ignored() {
    for headers in [
        vec![("x-ratelimit-remaining", "0")],
        vec![("x-ratelimit-reset", "20")],
        vec![
            ("x-ratelimit-remaining", "zero"),
            ("x-ratelimit-reset", "20"),
        ],
        vec![
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "soon"),
        ],
    ] {
        let mock = Mock::new(vec![Ok(resp(200, &headers))]);
        let c = client(&mock);
        c.send(get("http://h.example/")).await.unwrap();
        c.send(get("http://h.example/")).await.unwrap();
        assert_eq!(mock.times(), vec![secs(0), secs(0)], "{headers:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_wait_past_thirty_seconds_is_refused_at_once() {
    let mock = Mock::new(vec![Ok(resp(
        200,
        &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "31")],
    ))]);
    let c = client(&mock);
    c.send(get("http://h.example/")).await.unwrap();
    let before = Instant::now();
    let err = c
        .send(get("http://h.example/?k=SYNTHETICKEY"))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::RateLimited);
    assert_eq!(Instant::now(), before, "refused without waiting");
    assert_eq!(mock.count(), 1, "the request never left");
    assert!(!format!("{err}").contains("SYNTHETICKEY"));
    // Exactly MAX_HOST_WAIT is still waited out.
    let mock = Mock::new(vec![Ok(resp(
        200,
        &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "30")],
    ))]);
    let c = client(&mock);
    c.send(get("http://h.example/")).await.unwrap();
    c.send(get("http://h.example/")).await.unwrap();
    assert_eq!(mock.times()[1], secs(30));
}

#[tokio::test(start_paused = true)]
async fn an_infinite_reset_does_not_panic() {
    let mock = Mock::new(vec![Ok(resp(
        200,
        &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "inf")],
    ))]);
    let c = client(&mock);
    c.send(get("http://h.example/")).await.unwrap();
    assert_eq!(
        c.send(get("http://h.example/")).await.unwrap_err().kind(),
        ErrorKind::RateLimited
    );
    // close_all forgets it (core/net.py:86-93).
    c.close_all().unwrap();
    c.send(get("http://h.example/")).await.unwrap();
}

// --- 429 retry (core/net.py:137-145, 203-211) -------------------------------

#[tokio::test(start_paused = true)]
async fn a_429_is_retried_once_after_retry_after() {
    let mock = Mock::new(vec![
        Ok(resp(429, &[("retry-after", "3")])),
        Ok(resp(200, &[])),
    ]);
    let got = client(&mock).send(get("http://h.example/")).await.unwrap();
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(mock.times(), vec![secs(0), secs(3)]);
}

#[tokio::test(start_paused = true)]
async fn a_429_without_retry_after_waits_two_seconds() {
    let mock = Mock::new(vec![Ok(resp(429, &[])), Ok(resp(200, &[]))]);
    client(&mock).send(get("http://h.example/")).await.unwrap();
    assert_eq!(mock.times(), vec![secs(0), secs(2)]);
}

#[tokio::test(start_paused = true)]
async fn head_is_retried_like_get() {
    let mock = Mock::new(vec![
        Ok(resp(429, &[("retry-after", "1")])),
        Ok(resp(200, &[])),
    ]);
    let req = NetRequest::head("http://h.example/", secs(5)).unwrap();
    assert_eq!(
        client(&mock).send(req).await.unwrap().status,
        StatusCode::OK
    );
    assert_eq!(mock.times(), vec![secs(0), secs(1)]);
}

#[tokio::test(start_paused = true)]
async fn only_one_retry_and_the_second_429_comes_back() {
    let mock = Mock::new(vec![
        Ok(resp(429, &[("retry-after", "1")])),
        Ok(resp(429, &[("retry-after", "1")])),
    ]);
    let got = client(&mock).send(get("http://h.example/")).await.unwrap();
    assert_eq!(got.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(mock.count(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_long_or_dated_retry_after_is_not_waited_out() {
    for value in ["11", "10.5", "-1", "Wed, 21 Oct 2026 07:28:00 GMT", "soon"] {
        let mock = Mock::new(vec![Ok(resp(429, &[("retry-after", value)]))]);
        let got = client(&mock).send(get("http://h.example/")).await.unwrap();
        assert_eq!(got.status, StatusCode::TOO_MANY_REQUESTS, "{value}");
        assert_eq!(mock.count(), 1, "{value}");
    }
}

#[tokio::test(start_paused = true)]
async fn post_and_retry_never_are_not_retried() {
    let mock = Mock::new(vec![Ok(resp(429, &[("retry-after", "1")]))]);
    let post = NetRequest::post("http://h.example/", secs(5)).unwrap();
    assert_eq!(
        client(&mock).send(post).await.unwrap().status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(mock.count(), 1);

    let mock = Mock::new(vec![Ok(resp(429, &[("retry-after", "1")]))]);
    let never = get("http://h.example/").with_retry(RetryMode::Never);
    assert_eq!(
        client(&mock).send(never).await.unwrap().status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(mock.count(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_retry_that_does_not_fit_the_deadline_is_skipped() {
    // Retry-After 3 s against a 3 s deadline: no spare time, so no retry.
    let mock = Mock::new(vec![Ok(resp(429, &[("retry-after", "3")]))]);
    let req = get("http://h.example/").with_deadline(secs(3));
    assert_eq!(
        client(&mock).send(req).await.unwrap().status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(mock.count(), 1);
    // 4 s deadline leaves room.
    let mock = Mock::new(vec![
        Ok(resp(429, &[("retry-after", "3")])),
        Ok(resp(200, &[])),
    ]);
    let req = get("http://h.example/").with_deadline(secs(4));
    assert_eq!(
        client(&mock).send(req).await.unwrap().status,
        StatusCode::OK
    );
}

#[tokio::test(start_paused = true)]
async fn the_deadline_clock_starts_after_the_host_wait() {
    let mock = Mock::new(vec![Ok(resp(
        200,
        &[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "20")],
    ))]);
    let c = client(&mock);
    c.send(get("http://h.example/")).await.unwrap();
    let before = Instant::now();
    c.send(get("http://h.example/").with_deadline(secs(5)))
        .await
        .unwrap();
    let calls = mock.calls.lock().unwrap();
    let started = calls[1].1.started.expect("the stack stamps the start");
    assert_eq!(
        started - before,
        secs(20),
        "20 s of waiting is not charged to a 5 s deadline"
    );
}

// --- the Pawchive pattern (core/pawchive_index.py:388-430) -------------------

fn polite(req: NetRequest) -> NetRequest {
    req.with_retry(RetryMode::pawchive()).with_min_gap(secs(1))
}

#[tokio::test(start_paused = true)]
async fn polite_waits_the_named_retry_after_then_the_default() {
    let mock = Mock::new(vec![
        Ok(resp(429, &[("retry-after", "45")])),
        Ok(resp(429, &[])),
        Ok(resp(429, &[("retry-after", "junk")])),
        Ok(resp(200, &[])),
    ]);
    let got = client(&mock)
        .send(polite(get("http://h.example/")))
        .await
        .unwrap();
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(mock.times(), vec![secs(0), secs(45), secs(75), secs(105)]);
}

#[tokio::test(start_paused = true)]
async fn polite_gives_up_after_three_retries_and_returns_the_429() {
    let mock = Mock::new(
        vec![Ok(resp(429, &[("retry-after", "2")])); 5]
            .into_iter()
            .collect(),
    );
    let got = client(&mock)
        .send(polite(get("http://h.example/")))
        .await
        .unwrap();
    assert_eq!(got.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(mock.count(), 4, "one try and MAX_RETRIES = 3 retries");
}

#[tokio::test(start_paused = true)]
async fn polite_retries_a_transport_error_after_the_minimum_gap() {
    let boom = || NetError::new(ErrorKind::Connect, None, "refused");
    let mock = Mock::new(vec![Err(boom()), Err(boom()), Ok(resp(200, &[]))]);
    let got = client(&mock)
        .send(polite(get("http://h.example/")))
        .await
        .unwrap();
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(mock.times(), vec![secs(0), secs(1), secs(2)]);

    // Out of retries: the error is raised.
    let mock = Mock::new(vec![
        Err(boom()),
        Err(boom()),
        Err(boom()),
        Err(boom()),
        Ok(resp(200, &[])),
    ]);
    let err = client(&mock)
        .send(polite(get("http://h.example/")))
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Connect);
    assert_eq!(mock.count(), 4);
}

#[tokio::test(start_paused = true)]
async fn polite_does_not_retry_a_verdict() {
    let mock = Mock::new(vec![Err(NetError::new(
        ErrorKind::DeadlineExceeded,
        None,
        "late",
    ))]);
    assert!(
        client(&mock)
            .send(polite(get("http://h.example/")))
            .await
            .is_err()
    );
    assert_eq!(mock.count(), 1);
    let mock = Mock::new(vec![Ok(resp(500, &[]))]);
    assert_eq!(
        client(&mock)
            .send(polite(get("http://h.example/")))
            .await
            .unwrap()
            .status
            .as_u16(),
        500
    );
    assert_eq!(mock.count(), 1);
}

// --- minimum gap ------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn min_gap_spaces_sequential_requests() {
    let mock = Mock::new(vec![]);
    let c = client(&mock);
    for _ in 0..3 {
        c.send(get("http://h.example/").with_min_gap(secs(1)))
            .await
            .unwrap();
    }
    assert_eq!(mock.times(), vec![secs(0), secs(1), secs(2)]);
}

#[tokio::test(start_paused = true)]
async fn min_gap_spaces_concurrent_requests_too() {
    let mock = Mock::new(vec![]);
    let c = client(&mock);
    let req = || get("http://h.example/").with_min_gap(secs(2));
    let (a, b, d) = tokio::join!(c.send(req()), c.send(req()), c.send(req()));
    a.unwrap();
    b.unwrap();
    d.unwrap();
    assert_eq!(mock.times(), vec![secs(0), secs(2), secs(4)]);
}

#[tokio::test(start_paused = true)]
async fn min_gap_is_per_host_and_measured_from_the_response() {
    let mock = Mock::new(vec![]);
    let c = client(&mock);
    c.send(get("http://a.example/").with_min_gap(secs(5)))
        .await
        .unwrap();
    c.send(get("http://b.example/").with_min_gap(secs(5)))
        .await
        .unwrap();
    tokio::time::sleep(secs(3)).await;
    c.send(get("http://a.example/").with_min_gap(secs(5)))
        .await
        .unwrap();
    // a: 0, b: 0, then a again 5 s after the first response (3 s of that already spent).
    assert_eq!(mock.times(), vec![secs(0), secs(0), secs(5)]);
}
