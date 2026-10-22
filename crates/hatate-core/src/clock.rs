//! The clock a wait is requested through (DAN-2059).
//!
//! Python reads `time.time()` and `time.monotonic()` and sleeps with `time.sleep()`. A client
//! that waits (SauceNAO's burst window) takes a [`Clock`] instead, so a test can hand it one that
//! answers instantly and records what was asked, and compare the waits rather than the wall time
//! they took. The binary uses [`SystemClock`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
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

/// A clock that never waits: `sleep` advances `wall` and `monotonic` by the amount asked and
/// returns at once, and every amount asked for is kept. A test (or a dry run) reads the sleeps
/// back instead of measuring how long they took.
#[derive(Debug)]
pub struct ManualClock {
    inner: Mutex<ManualState>,
}

#[derive(Debug)]
struct ManualState {
    wall: f64,
    monotonic: f64,
    sleeps: Vec<f64>,
}

impl ManualClock {
    /// Both clocks start at the given readings.
    pub fn new(wall: f64, monotonic: f64) -> Self {
        ManualClock {
            inner: Mutex::new(ManualState {
                wall,
                monotonic,
                sleeps: Vec::new(),
            }),
        }
    }

    /// Moves both clocks on without a sleep being asked for (time passing between calls).
    pub fn advance(&self, seconds: f64) {
        let mut state = self.inner.lock().unwrap();
        state.wall += seconds;
        state.monotonic += seconds;
    }

    /// Every amount `sleep` was asked for, in order.
    pub fn sleeps(&self) -> Vec<f64> {
        self.inner.lock().unwrap().sleeps.clone()
    }

    /// The sum of [`ManualClock::sleeps`].
    pub fn slept(&self) -> f64 {
        self.inner.lock().unwrap().sleeps.iter().sum()
    }
}

impl Clock for ManualClock {
    fn wall(&self) -> f64 {
        self.inner.lock().unwrap().wall
    }

    fn monotonic(&self) -> f64 {
        self.inner.lock().unwrap().monotonic
    }

    fn sleep(&self, seconds: f64) -> BoxFuture<'_, ()> {
        let mut state = self.inner.lock().unwrap();
        state.sleeps.push(seconds);
        if seconds > 0.0 {
            state.wall += seconds;
            state.monotonic += seconds;
        }
        Box::pin(std::future::ready(()))
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

    #[tokio::test]
    async fn the_manual_clock_moves_on_by_what_it_is_asked_to_sleep_and_remembers_it() {
        let clock = ManualClock::new(1000.0, 5.0);
        clock.sleep(0.25).await;
        clock.sleep(1.5).await;
        clock.advance(10.0);
        assert_eq!((clock.wall(), clock.monotonic()), (1011.75, 16.75));
        assert_eq!(clock.sleeps(), vec![0.25, 1.5]);
        assert_eq!(clock.slept(), 1.75);
    }
}
