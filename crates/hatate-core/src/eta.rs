//! How much longer a search run has to go (`core/eta.py`), and the wording the status strip puts
//! on it (`gui/main_window.py`: `_format_duration`, `_format_finish_time`,
//! `_refresh_run_progress_label`, `_idle_run_text`, `_on_wait_countdown`, `_on_worker_waiting`).
//!
//! The estimate is the mean interval between progress signals over the whole run, not a trailing
//! window and not elapsed time over images done: the worker reports progress after an image's
//! search and before its rate-limit wait, and cache hits arrive in clusters (see the Python
//! module's docstring, which is the reasoning).
//!
//! Every number that is written is rounded the way Python rounds it: `round()` and `{:.0f}` both
//! send an exact half to the even neighbour.

use std::sync::OnceLock;
use std::time::Instant;

use crate::localtime::WallClock;

/// `MIN_INTERVALS_BEFORE_ESTIMATING`: two intervals, three progress signals, before saying
/// anything.
pub const MIN_INTERVALS_BEFORE_ESTIMATING: i64 = 2;

/// `IDLE_PROGRESS` (`gui/shell.py:50`).
pub const IDLE_PROGRESS: &str = "nothing queued";
/// `IDLE_ETA` (`gui/shell.py:51`).
pub const IDLE_ETA: &str = "ETA \u{2014}";
/// `QUOTA_PAUSED_ETA` (`gui/shell.py:56`).
pub const QUOTA_PAUSED_ETA: &str = "ETA paused";

/// `time.monotonic()`: seconds from a fixed point in this process.
pub fn monotonic() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// `RunEstimate`: tracks a run's pace and answers how much of it is left. Reused across runs:
/// [`RunEstimate::reset`] at every start, [`RunEstimate::record`] on every progress signal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunEstimate {
    first_at: Option<f64>,
    first_done: i64,
    last_at: f64,
    done: i64,
    total: i64,
}

impl RunEstimate {
    pub fn new() -> RunEstimate {
        RunEstimate::default()
    }

    /// `reset`: forgets the previous run.
    pub fn reset(&mut self) {
        *self = RunEstimate::default();
    }

    /// `record`: notes one progress signal. `now` is `time.monotonic()` seconds, `None` for the
    /// present. `total` is re-read every time because it grows once mid-run, when images that
    /// failed on a network fault are queued for their retry.
    pub fn record(&mut self, done: i64, total: i64, now: Option<f64>) {
        let moment = now.unwrap_or_else(monotonic);
        self.done = done;
        self.total = total;
        if self.first_at.is_none() {
            // The baseline is this signal, not the run's start: the time before it is one search
            // with no wait attached.
            self.first_at = Some(moment);
            self.first_done = done;
        }
        self.last_at = moment;
    }

    /// `intervals`: completed image-to-image cycles observed so far.
    pub fn intervals(&self) -> i64 {
        (self.done - self.first_done).max(0)
    }

    /// `seconds_per_image`: the mean over the run so far, or `-1.0` when there is not enough of
    /// the run yet to say.
    pub fn seconds_per_image(&self) -> f64 {
        let Some(first_at) = self.first_at else {
            return -1.0;
        };
        if self.intervals() < MIN_INTERVALS_BEFORE_ESTIMATING {
            return -1.0;
        }
        let elapsed = self.last_at - first_at;
        if elapsed <= 0.0 {
            return -1.0;
        }
        elapsed / self.intervals() as f64
    }

    /// `seconds_remaining`: the estimate, or `-1.0` when unknown. Unknown is a real answer.
    pub fn seconds_remaining(&self) -> f64 {
        let pace = self.seconds_per_image();
        if pace < 0.0 {
            return -1.0;
        }
        let remaining = (self.total - self.done).max(0);
        remaining as f64 * pace
    }
}

/// Python's `int(round(x))` clamped at 0 (`max(int(round(seconds)), 0)`). Python raises for a NaN
/// or an infinity; here NaN is 0 and an infinity saturates, and nothing panics.
fn rounded_seconds(seconds: f64) -> i64 {
    if seconds.is_nan() {
        return 0;
    }
    (seconds.round_ties_even() as i64).max(0)
}

/// `_format_duration`: `45s`, `2m 30s`, `1h 5m`, `6d 4h`. `pad_minutes` zero-pads the minutes
/// after hours (`2h 09m`) for the banner's meta line.
pub fn format_duration(seconds: f64, pad_minutes: bool) -> String {
    let seconds = rounded_seconds(seconds);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let (minutes, secs) = (seconds / 60, seconds % 60);
    if minutes < 60 {
        return if secs != 0 {
            format!("{minutes}m {secs}s")
        } else {
            format!("{minutes}m")
        };
    }
    let (hours, mins) = (minutes / 60, minutes % 60);
    if hours < 24 {
        if mins == 0 {
            return format!("{hours}h");
        }
        return if pad_minutes {
            format!("{hours}h {mins:02}m")
        } else {
            format!("{hours}h {mins}m")
        };
    }
    let (days, hrs) = (hours / 24, hours % 24);
    if hrs != 0 {
        format!("{days}d {hrs}h")
    } else {
        format!("{days}d")
    }
}

/// The largest `timedelta` Python builds (`999999999` days) and the first instant it cannot add
/// to a `datetime` (`10000-01-01`), as seconds.
const MAX_TIMEDELTA_SECONDS: f64 = 999_999_999.0 * 86_400.0;
const DATETIME_LIMIT_SECS: f64 = 253_402_300_800.0;

/// `_format_finish_time`: when a run will end, phrased the way the distance warrants. `start` is
/// the present as naive wall-clock seconds since 1970-01-01 00:00 (`datetime.now()`); the
/// weekday and month names are the C locale's. An absurd horizon is `""`, as Python's
/// `OverflowError` branch.
pub fn format_finish_time(seconds_from_now: f64, start_naive_secs: i64) -> String {
    let ahead = seconds_from_now.max(0.0);
    if !ahead.is_finite() || ahead > MAX_TIMEDELTA_SECONDS {
        return String::new();
    }
    let finish_secs = start_naive_secs as f64 + ahead;
    if finish_secs >= DATETIME_LIMIT_SECS {
        return String::new();
    }
    let start = WallClock::from_naive_secs(start_naive_secs);
    let finish = WallClock::from_naive_secs(finish_secs.floor() as i64);
    let days_ahead = finish.day_number() - start.day_number();
    let hhmm = format!("{:02}:{:02}", finish.hour, finish.minute);
    if days_ahead <= 0 {
        format!("ends {hhmm}")
    } else if days_ahead == 1 {
        format!("ends tomorrow {hhmm}")
    } else if days_ahead < 7 {
        format!("ends {} {hhmm}", finish.weekday_abbr())
    } else {
        format!("ends {} {}", finish.day, finish.month_abbr())
    }
}

/// `f"{n:,}"`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `f"{value:.0f}"`: the exact binary value, an exact half to the even neighbour.
pub fn fixed0(value: f64) -> String {
    if value.is_nan() {
        "nan".to_owned()
    } else if value.is_infinite() {
        if value < 0.0 { "-inf" } else { "inf" }.to_owned()
    } else {
        // A negative that rounds to zero keeps its sign ("-0") in Python and in Rust.
        format!("{value:.0}")
    }
}

/// The two labels of the run strip (`run_progress_label`, `run_eta_label`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStrip {
    pub progress: String,
    pub eta: String,
}

/// `_refresh_run_progress_label` and `_idle_run_text`: the count, and what the pace says about
/// the rest.
///
/// `done` and `total` are the progress bar's value and maximum, `unsearched` the number of rows
/// still `not_searched`, `remaining` `RunEstimate::seconds_remaining()` (ignored unless a run is
/// active), `start_naive_secs` the present as naive wall-clock seconds.
pub fn run_strip(
    done: i64,
    total: i64,
    run_active: bool,
    quota_paused: bool,
    remaining: f64,
    unsearched: usize,
    start_naive_secs: i64,
) -> RunStrip {
    let paused_eta = if quota_paused {
        QUOTA_PAUSED_ETA
    } else {
        IDLE_ETA
    };
    if total <= 0 || (done <= 0 && !run_active) {
        let progress = if unsearched > 0 {
            format!("{} not searched", thousands(unsearched as i64))
        } else {
            IDLE_PROGRESS.to_owned()
        };
        return RunStrip {
            progress,
            eta: paused_eta.to_owned(),
        };
    }
    let remaining = if run_active { remaining } else { -1.0 };
    let eta = if remaining >= 0.0 {
        let mut eta = format!("ETA ~{}", format_duration(remaining, false));
        let finish = format_finish_time(remaining, start_naive_secs);
        if !finish.is_empty() {
            eta.push_str(" \u{b7} ");
            eta.push_str(&finish);
        }
        eta
    } else {
        paused_eta.to_owned()
    };
    RunStrip {
        progress: format!("{}/{} searched", thousands(done), thousands(total)),
        eta,
    }
}

/// `_on_wait_countdown`: `waiting 38s`, or `""` when there is nothing to count down.
pub fn wait_countdown_text(label: &str, remaining: f64) -> String {
    if label.is_empty() || remaining <= 0.0 {
        return String::new();
    }
    let what = if label == "Rate limit" {
        "waiting".to_owned()
    } else {
        label.to_lowercase()
    };
    format!("{what} {}s", fixed0(remaining))
}

/// `_on_worker_waiting`: the status bar's line while the rate limit holds the next search.
pub fn worker_waiting_text(seconds: f64) -> String {
    format!(
        "Waiting {}s before next search (rate limiting)\u{2026}",
        fixed0(seconds)
    )
}
