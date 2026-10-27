//! The window's wording: the pure functions `gui/main_window.py` (`:148-256` at `76c85c4`) and
//! `core/saucenao.py:330` build their banner and status text with. Nothing here touches GTK, so
//! the same inputs can be run through Python and compared (`docs/gtk-shell.md`).

use hatate_core::model::{Entry, MatchStatus};
use hatate_core::queue::needs_review;
use hatate_core::quota_pause::QuotaPause;
use hatate_core::timeutil;

/// `_format_duration(seconds, pad_minutes)`: compact human-readable duration: `45s`, `2m 30s`,
/// `1h 5m`, `6d 4h`. `pad_minutes` zero-pads the minutes that follow hours (`2h 09m`), as the
/// mockup's banner meta line does.
pub fn format_duration(seconds: f64, pad_minutes: bool) -> String {
    // `max(int(round(seconds)), 0)`: Python rounds half to even.
    let rounded = round_half_even(seconds);
    let seconds = rounded.max(0) as u64;
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

/// Python's `round(x)` for a float: nearest, ties to even. NaN and infinities have no `int()`;
/// Python raises there, so they are not inputs (the caller never passes them).
pub fn round_half_even(x: f64) -> i64 {
    let floor = x.floor();
    let diff = x - floor;
    let f = floor as i64;
    if diff < 0.5 {
        f
    } else if diff > 0.5 {
        f + 1
    } else if f % 2 == 0 {
        f
    } else {
        f + 1
    }
}

/// `_plural(count, singular, plural)`.
pub fn plural(count: i64, singular: &str, plural: Option<&str>) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        match plural {
            Some(p) => format!("{count} {p}"),
            None => format!("{count} {singular}s"),
        }
    }
}

fn is_are(n: i64) -> &'static str {
    if n == 1 { "is" } else { "are" }
}

fn was_were(n: i64) -> &'static str {
    if n == 1 { "was" } else { "were" }
}

/// `_is_waiting_review(entry)`: a match was found and the row is neither sent nor reviewed.
pub fn is_waiting_review(e: &Entry) -> bool {
    needs_review(e) && matches!(e.status, MatchStatus::Good | MatchStatus::Poor)
}

/// `_waiting_review_count(entries)`.
pub fn waiting_review_count(entries: &[Entry]) -> usize {
    entries.iter().filter(|e| is_waiting_review(e)).count()
}

/// `_interrupted_prose(entries)`: the banner's reassurance, with the counts that earn it (S-01).
pub fn interrupted_prose(entries: &[Entry]) -> String {
    let total = entries.len() as i64;
    let sent = entries
        .iter()
        .filter(|e| e.sent_to_hydrus && e.hydrus_import_confirmed)
        .count() as i64;
    let queued = entries
        .iter()
        .filter(|e| e.sent_to_hydrus && !e.hydrus_import_confirmed)
        .count() as i64;
    let waiting = waiting_review_count(entries) as i64;
    let unreached = entries
        .iter()
        .filter(|e| e.status == MatchStatus::NotSearched)
        .count() as i64;
    let mut parts = vec![format!(
        "{sent} of {} {} already sent to Hydrus",
        plural(total, "image", None),
        if total == 1 { "is" } else { "are" }
    )];
    if queued != 0 {
        parts.push(format!(
            "{queued} {} still queued with Hydrus",
            is_are(queued)
        ));
    }
    parts.push(format!(
        "{waiting} {} decided and waiting for your review",
        is_are(waiting)
    ));
    parts.push(format!("{unreached} {} never reached", was_were(unreached)));
    let last = parts.pop().expect("at least three parts");
    let counts = format!("{}, and {last}", parts.join(", "));
    format!(
        "Hatate did not exit cleanly during this run. Everything up to the last autosave is \
         safe. {counts}."
    )
}

/// `_quota_pause_prose(entries, remaining)`: why the run stopped, with the counts that say
/// nothing was lost (S-03). `remaining` is the worker's own figure and stands in when there is no
/// list to count.
pub fn quota_pause_prose(entries: &[Entry], remaining: i64) -> String {
    let why = "Hatate stopped here on purpose, rather than finish the batch with weaker matches \
               from the other engines.";
    if entries.is_empty() {
        return format!(
            "{why} {} {} untouched until you resume.",
            plural(remaining, "image", None),
            is_are(remaining)
        );
    }
    let total = entries.len() as i64;
    let sent = entries.iter().filter(|e| e.sent_to_hydrus).count() as i64;
    let waiting = waiting_review_count(entries) as i64;
    let untouched = entries
        .iter()
        .filter(|e| e.status == MatchStatus::NotSearched)
        .count() as i64;
    format!(
        "{why} {sent} of {} {} already sent, {waiting} {} waiting on your review, and \
         {untouched} {} untouched until you resume. Nothing was lost.",
        plural(total, "image", None),
        is_are(total),
        is_are(waiting),
        is_are(untouched)
    )
}

/// `describe_quota_pause(state, now)` (`core/saucenao.py:330`): the real, known reset time.
pub fn describe_quota_pause(state: &QuotaPause, now: f64) -> String {
    let Some(reset) = state.reset_epoch() else {
        return format!(
            "SauceNAO paused after {} searched, {} left unsearched",
            state.searched, state.remaining
        );
    };
    let remaining_seconds = (reset - now).max(0.0);
    let whole = remaining_seconds as i64;
    let (hours, rem) = (whole / 3600, whole % 3600);
    let minutes = rem / 60;
    let eta = if hours != 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes}m")
    };
    format!(
        "Paused: SauceNAO daily quota exhausted - {} searched, {} left unsearched. Resets {} \
         (in {eta})",
        state.searched,
        state.remaining,
        utc_hhmm(&state.reset_at, reset)
    )
}

/// `reset_at.strftime('%H:%M UTC')` as Python prints it for the stored offset: the clock time in
/// the offset the string carries (the file writes `+00:00`), then the word UTC.
pub fn utc_hhmm(reset_at: &str, epoch: f64) -> String {
    // The string's own wall clock, not a conversion: `datetime.fromisoformat` keeps the offset
    // it was given, and `strftime` prints that wall time.
    if let Some(time) = reset_at.get(11..16).filter(|t| t.as_bytes()[2] == b':') {
        return format!("{time} UTC");
    }
    let stamp = timeutil::format_stamp(epoch);
    format!("{} UTC", &stamp[11..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::model::Entry;

    fn entry(status: MatchStatus) -> Entry {
        let mut e = Entry::new("/x.png");
        e.status = status;
        e
    }

    #[test]
    fn durations_round_to_even_and_pad_on_request() {
        for (secs, pad, want) in [
            (0.0, false, "0s"),
            (44.5, false, "44s"),
            (45.5, false, "46s"),
            (59.6, false, "1m"),
            (90.0, false, "1m 30s"),
            (3600.0, false, "1h"),
            (3660.0, false, "1h 1m"),
            (3660.0, true, "1h 01m"),
            (7740.0, true, "2h 09m"),
            (86400.0, false, "1d"),
            (90000.0, false, "1d 1h"),
            (-5.0, false, "0s"),
            (536400.0, false, "6d 5h"),
        ] {
            assert_eq!(format_duration(secs, pad), want, "{secs} {pad}");
        }
    }

    #[test]
    fn plural_follows_pythons_default_s() {
        assert_eq!(plural(1, "image", None), "1 image");
        assert_eq!(plural(0, "image", None), "0 images");
        assert_eq!(plural(2, "new entry", Some("new entries")), "2 new entries");
        assert_eq!(plural(1, "new entry", Some("new entries")), "1 new entry");
    }

    #[test]
    fn the_interrupted_prose_leaves_out_a_zero_queued_count() {
        let mut sent = entry(MatchStatus::Good);
        sent.sent_to_hydrus = true;
        sent.hydrus_import_confirmed = true;
        let waiting = entry(MatchStatus::Good);
        let untouched = entry(MatchStatus::NotSearched);
        let text = interrupted_prose(&[sent.clone(), waiting.clone(), untouched.clone()]);
        assert_eq!(
            text,
            "Hatate did not exit cleanly during this run. Everything up to the last autosave is \
             safe. 1 of 3 images are already sent to Hydrus, 1 is decided and waiting for your \
             review, and 1 was never reached."
        );
        let mut queued = sent;
        queued.hydrus_import_confirmed = false;
        let text = interrupted_prose(&[queued, waiting, untouched]);
        assert!(text.contains(
            "0 of 3 images are already sent to Hydrus, 1 is still queued with Hydrus, 1 is decided"
        ));
    }

    #[test]
    fn the_quota_prose_counts_or_falls_back_to_the_workers_figure() {
        assert!(quota_pause_prose(&[], 1).ends_with("1 image is untouched until you resume."));
        assert!(quota_pause_prose(&[], 7).ends_with("7 images are untouched until you resume."));
        let mut sent = entry(MatchStatus::Good);
        sent.sent_to_hydrus = true;
        let text = quota_pause_prose(&[sent, entry(MatchStatus::NotSearched)], 1);
        assert!(text.contains(
            "1 of 2 images are already sent, 0 are waiting on your review, and 1 is untouched \
             until you resume. Nothing was lost."
        ));
    }

    #[test]
    fn a_pause_is_described_with_its_reset_time_and_distance() {
        let state = QuotaPause {
            paused_at: 0.0,
            searched: 12,
            remaining: 30,
            reset_at: "2026-10-23T00:00:00+00:00".into(),
        };
        let reset = state.reset_epoch().unwrap();
        assert_eq!(
            describe_quota_pause(&state, reset - 3.0 * 3600.0 - 7.0 * 60.0 - 5.0),
            "Paused: SauceNAO daily quota exhausted - 12 searched, 30 left unsearched. Resets \
             00:00 UTC (in 3h07m)"
        );
        assert!(describe_quota_pause(&state, reset - 90.0).ends_with("(in 1m)"));
        assert!(describe_quota_pause(&state, reset + 10.0).ends_with("(in 0m)"));
        let bad = QuotaPause {
            reset_at: "soon".into(),
            ..state
        };
        assert_eq!(
            describe_quota_pause(&bad, 0.0),
            "SauceNAO paused after 12 searched, 30 left unsearched"
        );
    }
}
