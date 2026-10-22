//! A countdown around one blocking call (`core/progress_ticker.py`).
//!
//! An HTTP request has no "percent done", so a caller that wants live feedback gets a tick about
//! once a second while the request is in flight: `on_tick(label, seconds_remaining, total)`. When
//! the call ends, however it ends, one last `on_tick("", 0.0, 0.0)` clears whatever display was
//! showing it. With no callback it does nothing at all.

use std::future::Future;
use std::time::Duration;

use tokio::time::{Instant, sleep};

/// `OnTick`: label, seconds remaining, total seconds.
pub type OnTick<'a> = &'a (dyn Fn(&str, f64, f64) + Send + Sync);

/// `ProgressTicker.INTERVAL` default.
pub const TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Runs `call` with the ticker alongside, as `with ProgressTicker(label, timeout, on_tick):`.
///
/// Ticking stops once the countdown reaches zero (the call itself may carry on), and the final
/// clearing tick is sent when `call` finishes.
pub async fn ticked<T>(
    label: &str,
    timeout: f64,
    on_tick: Option<OnTick<'_>>,
    call: impl Future<Output = T>,
) -> T {
    let Some(tick) = on_tick else {
        return call.await;
    };
    tokio::pin!(call);
    let started = Instant::now();
    let mut counting = true;
    let out = loop {
        tokio::select! {
            out = &mut call => break out,
            () = sleep(TICK_INTERVAL), if counting => {
                let remaining = (timeout - started.elapsed().as_secs_f64()).max(0.0);
                tick(label, remaining, timeout);
                if remaining <= 0.0 {
                    counting = false;
                }
            }
        }
    };
    tick("", 0.0, 0.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    type Ticks = Mutex<Vec<(String, f64, f64)>>;

    /// What was ticked, with the remaining seconds rounded.
    fn seen(ticks: &Ticks) -> Vec<(String, i64, f64)> {
        ticks
            .lock()
            .unwrap()
            .iter()
            .map(|(l, r, total)| (l.clone(), r.round() as i64, *total))
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn it_ticks_each_second_then_clears() {
        let ticks = Ticks::default();
        let cb = |l: &str, r: f64, t: f64| ticks.lock().unwrap().push((l.to_string(), r, t));
        let out = ticked("Fetching", 5.0, Some(&cb), async {
            sleep(Duration::from_millis(2500)).await;
            7
        })
        .await;
        assert_eq!(out, 7);
        assert_eq!(
            seen(&ticks),
            vec![
                ("Fetching".to_string(), 4, 5.0),
                ("Fetching".to_string(), 3, 5.0),
                (String::new(), 0, 0.0),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn it_stops_counting_at_zero_but_still_clears() {
        let ticks = Ticks::default();
        let cb = |l: &str, r: f64, t: f64| ticks.lock().unwrap().push((l.to_string(), r, t));
        ticked("x", 2.0, Some(&cb), sleep(Duration::from_secs(10))).await;
        assert_eq!(
            seen(&ticks),
            vec![
                ("x".to_string(), 1, 2.0),
                ("x".to_string(), 0, 2.0),
                (String::new(), 0, 0.0),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_callback_it_is_just_the_call() {
        assert_eq!(ticked("x", 1.0, None, async { 3 }).await, 3);
    }
}
