//! The window's `tracing` events into `hatate-rs.log` (`core/applog.py`'s `setup_logging`): one
//! line per event in Python's format, size-rotated, written next to (never into) `app.log`. The
//! log viewer (`logview.rs`) reads this file, so without this layer a production run would show
//! it nothing.

use hatate_core::rotating_log::{Level as FileLevel, RotatingLog};
use std::fmt::Write as _;
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// A `tracing` layer that writes every event it is given to a [`RotatingLog`].
pub struct FileLayer {
    log: Mutex<RotatingLog>,
    now: fn() -> f64,
}

impl FileLayer {
    pub fn new(log: RotatingLog) -> FileLayer {
        FileLayer {
            log: Mutex::new(log),
            now: hatate_core::session::now_epoch,
        }
    }

    /// The same layer with a fixed clock, for a test.
    pub fn with_clock(log: RotatingLog, now: fn() -> f64) -> FileLayer {
        FileLayer {
            log: Mutex::new(log),
            now,
        }
    }
}

/// Collects an event's `message` and, after it, its other fields as `key=value`.
#[derive(Default)]
struct Message {
    text: String,
}

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.text, "{value:?}");
        } else {
            if !self.text.is_empty() {
                self.text.push(' ');
            }
            let _ = write!(self.text, "{}={value:?}", field.name());
        }
    }
}

fn level(l: &Level) -> FileLevel {
    match *l {
        Level::ERROR => FileLevel::Error,
        Level::WARN => FileLevel::Warning,
        Level::INFO => FileLevel::Info,
        _ => FileLevel::Debug,
    }
}

impl<S: Subscriber> Layer<S> for FileLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut m = Message::default();
        event.record(&mut m);
        let meta = event.metadata();
        if let Ok(mut log) = self.log.lock() {
            // A log that cannot be written must not take the app down with it.
            let _ = log.record(
                (self.now)(),
                level(meta.level()),
                meta.target(),
                &m.text.replace('\n', " | "),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn events_become_python_format_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = RotatingLog::open(dir.path().join("hatate-rs.log"), 2_000_000, 3).unwrap();
        let layer = FileLayer::with_clock(log, || 1_792_000_000.0);
        let sub = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(sub, || {
            tracing::info!(target: "hatate_gtk::window", "Added {} file(s)", 2);
            tracing::warn!(target: "hatate_gtk::fonts", "fonts not loaded");
            tracing::info!(target: "hatate_gtk::window", count = 3, "two\nlines");
        });
        let text = std::fs::read_to_string(dir.path().join("hatate-rs.log")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert!(lines[0].ends_with("[INFO] hatate_gtk::window: Added 2 file(s)"));
        assert!(lines[1].ends_with("[WARNING] hatate_gtk::fonts: fonts not loaded"));
        assert!(lines[2].ends_with("[INFO] hatate_gtk::window: two | lines count=3"));
    }
}
