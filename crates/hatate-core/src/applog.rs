//! The in-memory ring buffer the log viewer reads (`core/applog.py`: `_ring`, `_RingHandler`,
//! `get_recent_logs`). The rotating file is `rotating_log.rs`.
//!
//! Python keeps one process-global `deque(maxlen=4000)` of formatted lines. Here the ring is a
//! cloneable handle, so the window, its log layer and the viewer share one without a global; the
//! lines are formatted exactly as the file's (`RotatingLog::format_line`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::rotating_log::{Level, RotatingLog};

/// `_RING_SIZE`.
pub const RING_SIZE: usize = 4000;

/// A shared ring of formatted log lines; the oldest line goes when it is full.
#[derive(Debug, Clone)]
pub struct LogRing {
    lines: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl Default for LogRing {
    fn default() -> LogRing {
        LogRing::with_capacity(RING_SIZE)
    }
}

impl LogRing {
    pub fn new() -> LogRing {
        LogRing::default()
    }

    /// A ring of another size, for a test that does not want to write 4,000 lines.
    pub fn with_capacity(capacity: usize) -> LogRing {
        LogRing {
            lines: Arc::new(Mutex::new(VecDeque::with_capacity(capacity.min(RING_SIZE)))),
            capacity,
        }
    }

    /// `_RingHandler.emit` with a line already formatted. Logging must never itself crash the
    /// app, so a poisoned lock is read through.
    pub fn push(&self, line: String) {
        if self.capacity == 0 {
            return;
        }
        let mut lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// `_RingHandler.emit` for a record: the line the file would carry for it.
    pub fn record(&self, now: f64, level: Level, name: &str, message: &str) {
        self.push(RotatingLog::format_line(now, level, name, message));
    }

    /// `get_recent_logs`: the lines, oldest first, joined by newlines.
    pub fn recent(&self) -> String {
        let lines = self.lines.lock().unwrap_or_else(|e| e.into_inner());
        lines
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// How many lines are held.
    pub fn len(&self) -> usize {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_the_newest_lines_and_joins_them_with_newlines() {
        let ring = LogRing::with_capacity(3);
        assert!(ring.is_empty());
        assert_eq!(ring.recent(), "");
        for n in 1..=5 {
            ring.push(format!("line {n}"));
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.recent(), "line 3\nline 4\nline 5");
    }

    #[test]
    fn a_clone_shares_the_ring_and_the_default_size_is_the_pythons() {
        let ring = LogRing::new();
        let other = ring.clone();
        for n in 0..RING_SIZE + 10 {
            other.push(n.to_string());
        }
        assert_eq!(ring.len(), RING_SIZE);
        assert!(ring.recent().starts_with("10\n11\n"));
    }

    #[test]
    fn a_record_is_stored_as_the_line_the_file_carries() {
        let ring = LogRing::new();
        ring.record(0.0, Level::Warning, "hatate.x", "hello");
        assert_eq!(
            ring.recent(),
            "1970-01-01 00:00:00 [WARNING] hatate.x: hello"
        );
    }
}
