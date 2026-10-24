//! What the Crashed banner reads off disk about the run before this one (`core/crashlog.py:150-210`
//! at `76c85c4`): how many faults it left in `crash.log`, and when it last wrote to its log.
//!
//! The Rust app writes no faulthandler output (`docs/cache-and-startup-files.md`), so the fault
//! count is whatever a Python run left in `crash.log`, and zero otherwise. Its own log is
//! `hatate-rs.log` with UTC stamps (`rotating_log`), which is the one difference from Python's
//! local-time `app.log` here.

use hatate_core::timeutil;

/// `SESSION_START_MARK`: written at the top of every run's section of `crash.log`.
pub const SESSION_START_MARK: &str = "--- session started, faulthandler armed ---";
/// `FAULT_HEADER`: what faulthandler prints first for each fault it catches.
pub const FAULT_HEADER: &str = "Fatal Python error:";
/// `setup_logging()`'s first line of every run: the boundary between the previous run's lines
/// and this one's.
pub const LOG_RUN_START: &str = "Logging started, writing to";
/// `_LOG_TAIL_BYTES`: how much of the log is read from the end.
pub const LOG_TAIL_BYTES: usize = 64_000;
/// `CRASH_LOG_TAIL_CHARS`: how much of `crash.log` the viewer shows.
pub const CRASH_LOG_TAIL_CHARS: usize = 20_000;

/// `faults_in_previous_run(path)` over the file's text: the faults in the section before the
/// last one (the last is this run's, already opened). Fewer than two marks means none. Zero is a
/// real answer: a SIGKILL or a power cut gives faulthandler no chance to write anything.
pub fn faults_in_previous_run(crash_log: &str) -> usize {
    let sections: Vec<&str> = crash_log.split(SESSION_START_MARK).collect();
    if sections.len() < 3 {
        return 0;
    }
    sections[sections.len() - 2]
        .lines()
        .filter(|l| l.starts_with(FAULT_HEADER))
        .count()
}

/// `^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d) \[` at the start of a line: the stamp, or `None`.
fn stamp_of(line: &str) -> Option<&str> {
    let b = line.as_bytes();
    if b.len() < 21 {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let ok = digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b' '
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b' '
        && b[20] == b'[';
    ok.then(|| &line[..19])
}

/// `previous_run_stopped_at(path)` over the tail of the log: the stamp of the last line before
/// this run's own "Logging started" line, as epoch seconds (the Rust log's stamps are UTC), or
/// `None` when the log cannot say. A SIGKILL or a power cut records nothing at the instant, so
/// this is a lower bound.
pub fn previous_run_stopped_at(log_tail: &str) -> Option<f64> {
    let lines: Vec<&str> = log_tail.lines().collect();
    let this_run = lines.iter().rposition(|l| l.contains(LOG_RUN_START))?;
    for line in lines[..this_run].iter().rev() {
        if let Some(stamp) = stamp_of(line) {
            return timeutil::parse_iso(stamp);
        }
    }
    None
}

/// The last `max_bytes` of `bytes`, as lossy UTF-8 (`fh.seek(max(size - n, 0))` then decode with
/// `errors="replace"`).
pub fn tail_lossy(bytes: &[u8], max_bytes: usize) -> String {
    let start = bytes.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(faults: usize) -> String {
        let mut s = format!("\n{SESSION_START_MARK}\n");
        for _ in 0..faults {
            s.push_str(&format!("{FAULT_HEADER} Segmentation fault\n  File x\n"));
        }
        s
    }

    #[test]
    fn faults_are_counted_in_the_section_before_the_last() {
        assert_eq!(faults_in_previous_run(""), 0);
        assert_eq!(
            faults_in_previous_run(&section(3)),
            0,
            "one mark: only this run"
        );
        let two = format!("{}{}", section(2), section(0));
        assert_eq!(faults_in_previous_run(&two), 2);
        let three = format!("{}{}{}", section(5), section(1), section(0));
        assert_eq!(faults_in_previous_run(&three), 1);
    }

    #[test]
    fn only_a_line_that_starts_with_the_header_is_a_fault() {
        let log = format!(
            "{SESSION_START_MARK}\n  {FAULT_HEADER} indented\n{FAULT_HEADER} real\n{SESSION_START_MARK}\n"
        );
        assert_eq!(faults_in_previous_run(&log), 1);
    }

    #[test]
    fn the_stop_time_is_the_last_stamped_line_before_this_runs_start() {
        let log = "2026-10-22 10:00:00 [INFO] hatate: Logging started, writing to /x\n\
                   2026-10-22 10:05:30 [INFO] hatate.gui: working\n\
                   a continuation line without a stamp\n\
                   2026-10-23 08:00:00 [INFO] hatate: Logging started, writing to /x\n\
                   2026-10-23 08:00:01 [INFO] hatate.gui: this run\n";
        let t = previous_run_stopped_at(log).unwrap();
        assert_eq!(timeutil::format_stamp(t), "2026-10-22 10:05:30");
    }

    #[test]
    fn no_run_start_or_no_earlier_stamp_is_unknown() {
        assert_eq!(previous_run_stopped_at(""), None);
        assert_eq!(
            previous_run_stopped_at("2026-10-22 10:00:00 [INFO] a: no start line\n"),
            None
        );
        let only_this_run = "2026-10-23 08:00:00 [INFO] hatate: Logging started, writing to /x\n";
        assert_eq!(previous_run_stopped_at(only_this_run), None);
    }

    #[test]
    fn a_stamp_needs_the_bracket() {
        assert_eq!(
            stamp_of("2026-10-22 10:00:00 [INFO] x"),
            Some("2026-10-22 10:00:00")
        );
        assert_eq!(stamp_of("2026-10-22 10:00:00 INFO x"), None);
        assert_eq!(stamp_of("26-10-22 10:00:00 [INFO]"), None);
    }

    #[test]
    fn the_tail_keeps_the_end() {
        assert_eq!(tail_lossy(b"abcdef", 3), "def");
        assert_eq!(tail_lossy(b"ab", 10), "ab");
    }
}
