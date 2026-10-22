//! How long to wait between searches: a port of `core/rate_limit.py`.
//!
//! Pure functions on seconds (`f64`), so the caller owns the clock and the
//! tests need none. Times are monotonic seconds from any one origin.

use std::collections::HashMap;
use std::sync::Mutex;

/// Engines that make no network request, so there is nothing to pace
/// (`core/engines.py:73`).
pub const LOCAL_ENGINES: [&str; 1] = ["pawchiveindex"];

/// Seconds still to wait, crediting the time already elapsed since the engines
/// were last called (`core/rate_limit.py:27-52`).
///
/// `engines_finished_at` is `None` when no engine request completed: the full
/// delay applies. A negative elapsed time (two different clocks) also falls
/// back to the full delay, and the result never exceeds it.
pub fn remaining_delay(delay: f64, engines_finished_at: Option<f64>, now: f64) -> f64 {
    if delay <= 0.0 {
        return 0.0;
    }
    let Some(finished) = engines_finished_at else {
        return delay;
    };
    let elapsed = now - finished;
    if elapsed < 0.0 {
        return delay;
    }
    (delay - elapsed).max(0.0)
}

/// The `(min, max)` seconds to leave between two requests to `engine`
/// (`core/rate_limit.py:74-95`).
///
/// `override_pair` is the `engine_delays[engine]` entry. A missing entry, a pair
/// with fewer than two numbers, a non-finite or negative number all fall back to
/// the global pair; a reversed pair is swapped. (Python lets a NaN through; here
/// it is malformed, because a NaN gap is not a gap.)
pub fn engine_interval(
    engine: &str,
    override_pair: Option<&[f64]>,
    global_min: f64,
    global_max: f64,
) -> (f64, f64) {
    if LOCAL_ENGINES.contains(&engine) {
        return (0.0, 0.0);
    }
    let Some(&[low, high, ..]) = override_pair else {
        return (global_min, global_max);
    };
    if !low.is_finite() || !high.is_finite() || low < 0.0 || high < 0.0 {
        return (global_min, global_max);
    }
    if low <= high {
        (low, high)
    } else {
        (high, low)
    }
}

/// `min + (max - min) * unit`, for `unit` in `[0, 1)`: Python's `random.uniform`
/// with the random number supplied by the caller.
pub fn uniform(min: f64, max: f64, unit: f64) -> f64 {
    min + (max - min) * unit
}

/// Remembers when each engine was last queried (`core/rate_limit.py:98-144`).
#[derive(Default)]
pub struct EngineRateLimiter {
    last: Mutex<HashMap<String, f64>>,
}

impl EngineRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, engine: &str, when: f64) {
        self.last.lock().unwrap().insert(engine.to_owned(), when);
    }

    pub fn last_seen(&self, engine: &str) -> Option<f64> {
        self.last.lock().unwrap().get(engine).copied()
    }

    /// Seconds to wait before all of `engines` may be queried: the **maximum** of
    /// each engine's remaining gap, never the minimum, because they are queried
    /// together. An engine never queried imposes no wait; a negative elapsed time
    /// counts as zero elapsed.
    pub fn wait_needed(&self, engines: &[&str], intervals: &HashMap<String, f64>, now: f64) -> f64 {
        let last = self.last.lock().unwrap().clone();
        let mut worst = 0.0_f64;
        for engine in engines {
            let Some(&seen) = last.get(*engine) else {
                continue;
            };
            let elapsed = (now - seen).max(0.0);
            let remaining = intervals.get(*engine).copied().unwrap_or(0.0) - elapsed;
            if remaining > worst {
                worst = remaining;
            }
        }
        worst.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_delay_credits_elapsed_time() {
        assert_eq!(remaining_delay(0.0, Some(1.0), 5.0), 0.0);
        assert_eq!(remaining_delay(-3.0, None, 5.0), 0.0);
        assert_eq!(remaining_delay(60.0, None, 100.0), 60.0);
        assert_eq!(remaining_delay(60.0, Some(100.0), 110.0), 50.0);
        assert_eq!(remaining_delay(60.0, Some(100.0), 200.0), 0.0);
        // Mismatched clocks: the full delay, never more.
        assert_eq!(remaining_delay(60.0, Some(100.0), 90.0), 60.0);
    }

    #[test]
    fn engine_interval_falls_back_to_the_global_pair() {
        let g = (45.0, 75.0);
        assert_eq!(engine_interval("iqdb", None, g.0, g.1), g);
        assert_eq!(engine_interval("iqdb", Some(&[1.0]), g.0, g.1), g);
        assert_eq!(engine_interval("iqdb", Some(&[]), g.0, g.1), g);
        assert_eq!(engine_interval("iqdb", Some(&[-1.0, 5.0]), g.0, g.1), g);
        assert_eq!(engine_interval("iqdb", Some(&[1.0, f64::NAN]), g.0, g.1), g);
        assert_eq!(
            engine_interval("iqdb", Some(&[f64::INFINITY, 5.0]), g.0, g.1),
            g
        );
        assert_eq!(
            engine_interval("iqdb", Some(&[10.0, 20.0]), g.0, g.1),
            (10.0, 20.0)
        );
        assert_eq!(
            engine_interval("iqdb", Some(&[20.0, 10.0]), g.0, g.1),
            (10.0, 20.0)
        );
        assert_eq!(
            engine_interval("iqdb", Some(&[10.0, 20.0, 99.0]), g.0, g.1),
            (10.0, 20.0)
        );
        assert_eq!(
            engine_interval("iqdb", Some(&[0.0, 0.0]), g.0, g.1),
            (0.0, 0.0)
        );
    }

    #[test]
    fn local_engines_are_not_paced_even_with_an_override() {
        assert_eq!(
            engine_interval("pawchiveindex", Some(&[10.0, 20.0]), 45.0, 75.0),
            (0.0, 0.0)
        );
    }

    #[test]
    fn uniform_spans_the_range() {
        assert_eq!(uniform(45.0, 75.0, 0.0), 45.0);
        assert_eq!(uniform(45.0, 75.0, 0.5), 60.0);
    }

    #[test]
    fn wait_needed_is_the_slowest_engine_not_the_fastest() {
        let limiter = EngineRateLimiter::new();
        limiter.record("iqdb", 100.0);
        limiter.record("saucenao", 100.0);
        let intervals: HashMap<String, f64> = [
            ("iqdb".into(), 10.0),
            ("saucenao".into(), 40.0),
            ("ascii2d".into(), 99.0),
        ]
        .into();
        // iqdb has 5 s left, saucenao 35 s; ascii2d was never queried.
        let wait = limiter.wait_needed(&["iqdb", "saucenao", "ascii2d"], &intervals, 105.0);
        assert_eq!(wait, 35.0);
        assert_eq!(limiter.wait_needed(&["iqdb"], &intervals, 105.0), 5.0);
        assert_eq!(limiter.wait_needed(&["ascii2d"], &intervals, 105.0), 0.0);
        assert_eq!(
            limiter.wait_needed(&["iqdb", "saucenao"], &intervals, 500.0),
            0.0
        );
        // A clock that went backwards counts as nothing elapsed.
        assert_eq!(limiter.wait_needed(&["saucenao"], &intervals, 50.0), 40.0);
        assert_eq!(limiter.last_seen("iqdb"), Some(100.0));
        assert_eq!(limiter.last_seen("nope"), None);
    }

    #[test]
    fn an_engine_with_no_interval_imposes_no_wait() {
        let limiter = EngineRateLimiter::new();
        limiter.record("iqdb", 100.0);
        assert_eq!(limiter.wait_needed(&["iqdb"], &HashMap::new(), 101.0), 0.0);
    }
}
