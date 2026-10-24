//! `core/hard_timeout.py`: a genuine wall-clock deadline around a call, and the error every
//! client reports when it passes.
//!
//! A per-read timeout resets every time the server sends a little data, so a server that
//! trickles bytes keeps a "timed out" request alive for as long as it likes. A deadline counts
//! wall-clock time from the start of the exchange, body included.
//!
//! Python runs a blocking call in a daemon thread and abandons it. Rust has two shapes:
//!
//! - [`within`] is what the network clients use. The exchange is a future; when the deadline
//!   passes it is dropped, which closes the connection, so unlike Python nothing is left running.
//!   `NetRequest::with_deadline` does the same job inside the net layer and is what the engine
//!   requests set; `within` bounds the work around it (reading and downscaling the upload).
//! - [`run_with_hard_timeout`] is Python's function for blocking code: a detached thread that is
//!   abandoned at the deadline (a thread can't be killed). A detached thread never keeps the
//!   process from exiting, which is what `daemon=True` bought in Python.

use std::fmt;
use std::future::Future;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// `HardTimeoutError`: a call didn't finish within its hard wall-clock deadline. The message is
/// user-facing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardTimeoutError(pub String);

impl fmt::Display for HardTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HardTimeoutError {}

impl HardTimeoutError {
    pub fn kind(&self) -> crate::error::ErrorKind {
        crate::error::ErrorKind::Timeout
    }

    /// What failed and what to do. Python's text names the call and its address, so none of it is
    /// shown.
    pub fn user_message(&self) -> String {
        "A request took too long and was given up. Try again.".to_owned()
    }

    /// Python's message.
    pub fn detail(&self) -> String {
        self.0.clone()
    }
}

/// Python's `f"{seconds:.0f}"`: round half to even, no point. Rust's float formatting rounds the
/// exact decimal value the same way.
fn whole_seconds(timeout: Duration) -> String {
    format!("{:.0}", timeout.as_secs_f64())
}

impl HardTimeoutError {
    /// The error `run_with_hard_timeout` raises, and the one `core/net.py` raises when a body is
    /// still arriving at the deadline.
    pub fn trickling(timeout: Duration) -> Self {
        HardTimeoutError(format!(
            "Timed out after {}s (the server may be trickling data, keeping the connection alive \
             without actually finishing)",
            whole_seconds(timeout)
        ))
    }

    /// The error `core/net.py` raises when the deadline has already passed before a request is
    /// sent (after a 429 wait that did not leave room).
    pub fn already_passed(timeout: Duration) -> Self {
        HardTimeoutError(format!("Timed out after {}s", whole_seconds(timeout)))
    }
}

/// `run_with_hard_timeout`: runs `func` with a genuine wall-clock deadline. `Err` if it does not
/// complete in time, however the call itself reckons its own timeout. Python re-raises the
/// function's exception on the caller's side; here a panic in `func` is re-raised the same way.
pub fn run_with_hard_timeout<T, F>(func: F, timeout: Duration) -> Result<T, HardTimeoutError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    // Detached on purpose: an abandoned, permanently-hung call must never block the process
    // from exiting.
    thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(func));
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => Err(HardTimeoutError::trickling(timeout)),
    }
}

/// Runs `work` with a wall-clock deadline. Past it, `work` is dropped (which stops it at its
/// next await point and closes whatever it held) and the error is returned.
pub async fn within<T>(
    timeout: Duration,
    work: impl Future<Output = T>,
) -> Result<T, HardTimeoutError> {
    tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| HardTimeoutError::trickling(timeout))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn returns_value_when_fast() {
        assert_eq!(run_with_hard_timeout(|| 42, Duration::from_secs(2)), Ok(42));
    }

    #[test]
    #[should_panic(expected = "boom")]
    fn propagates_a_panic() {
        // Python: `test_propagates_exception`. The function's failure reaches the caller.
        let _ = run_with_hard_timeout(|| panic!("boom"), Duration::from_secs(2));
    }

    #[test]
    fn enforces_deadline_on_a_hanging_call() {
        // Python: `test_enforces_deadline_on_a_hanging_call`.
        let start = Instant::now();
        let got = run_with_hard_timeout(
            || thread::sleep(Duration::from_secs(30)),
            Duration::from_millis(400),
        );
        assert!(got.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_abandoned_call_is_a_detached_thread_so_it_cannot_block_exit() {
        // Python: `test_uses_daemon_thread_so_a_hung_call_cannot_block_exit`. A `JoinHandle`
        // dropped without `join` detaches the thread, and the process exits at the end of
        // `main` whatever is left running. There is nothing to wait for: the call returned
        // while its thread was still asleep.
        let start = Instant::now();
        let got = run_with_hard_timeout(
            || thread::sleep(Duration::from_secs(5)),
            Duration::from_millis(200),
        );
        assert!(got.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_message_matches_python() {
        let e = HardTimeoutError::trickling(Duration::from_secs_f64(45.0));
        assert_eq!(
            e.to_string(),
            "Timed out after 45s (the server may be trickling data, keeping the connection alive \
             without actually finishing)"
        );
        // `f"{timeout:.0f}"`: half to even.
        for (secs, shown) in [
            (0.4, "0"),
            (0.5, "0"),
            (1.5, "2"),
            (2.5, "2"),
            (44.99, "45"),
        ] {
            assert_eq!(
                HardTimeoutError::already_passed(Duration::from_secs_f64(secs)).to_string(),
                format!("Timed out after {shown}s"),
                "{secs}"
            );
        }
    }

    #[tokio::test]
    async fn within_gives_up_at_the_deadline_and_drops_the_work() {
        let got = within(
            Duration::from_millis(100),
            tokio::time::sleep(Duration::from_secs(30)),
        )
        .await;
        assert!(got.is_err());
        assert_eq!(within(Duration::from_secs(2), async { 7 }).await, Ok(7));
    }
}
