//! The clock a wait is requested through (DAN-2059).
//!
//! Python reads `time.time()` and `time.monotonic()` and sleeps with `time.sleep()`. A client
//! that waits (SauceNAO's burst window) takes a [`Clock`] instead, so a test can hand it one that
//! answers instantly and records what was asked, and compare the waits rather than the wall time
//! they took. The binary uses [`SystemClock`].

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::timeutil;

/// A boxed future, so a [`Clock`] can be used as `dyn Clock`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Clock: Send + Sync {
    /// `time.time()`: seconds since the Unix epoch.
    fn wall(&self) -> f64;
    /// `time.monotonic()`: seconds from any one origin, never going back.
    fn monotonic(&self) -> f64;
    /// `time.sleep(seconds)`. A value no wait can express (negative, NaN) is no wait.
    fn sleep(&self, seconds: f64) -> BoxFuture<'_, ()>;
}

/// The real clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn wall(&self) -> f64 {
        timeutil::now_epoch()
    }

    fn monotonic(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    fn sleep(&self, seconds: f64) -> BoxFuture<'_, ()> {
        let wait = if seconds.is_nan() || seconds <= 0.0 {
            Duration::ZERO
        } else {
            Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
        };
        Box::pin(tokio::time::sleep(wait))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn the_system_clock_sleeps_on_tokio_time_and_never_panics() {
        let clock = SystemClock::default();
        let before = tokio::time::Instant::now();
        clock.sleep(2.5).await;
        assert_eq!(before.elapsed(), Duration::from_millis(2500));
        clock.sleep(-1.0).await;
        clock.sleep(f64::NAN).await;
        assert!(clock.wall() > 1.0e9);
        assert!(clock.monotonic() >= 0.0);
    }
}
