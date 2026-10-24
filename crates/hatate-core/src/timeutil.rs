//! Calendar arithmetic the startup files need, without a time-zone database.
//!
//! Everything here is UTC. The SauceNAO daily quota resets at 00:00 UTC (spec 5.5), the
//! pause file stores that moment as an ISO-8601 string with a `+00:00` offset, and the
//! Rust log writes UTC stamps (see `rotating_log`).

/// Seconds since the Unix epoch as Python's `time.time()` gives them.
pub fn now_epoch() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Hinnant's algorithm).
pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// `daily_quota_reset_at` (`core/saucenao.py:248-259`): today's UTC midnight plus one day,
/// printed like `datetime.isoformat()` of an aware UTC datetime.
pub fn next_utc_midnight_iso(now: f64) -> String {
    let day = (now / 86_400.0).floor() as i64;
    let (y, m, d) = civil_from_days(day + 1);
    format!("{y:04}-{m:02}-{d:02}T00:00:00+00:00")
}

/// `%Y-%m-%d %H:%M:%S` of an epoch time, in UTC.
pub fn format_stamp(epoch: f64) -> String {
    let secs = epoch.floor() as i64;
    let (day, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(day);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// What `datetime.fromisoformat` takes, reduced to the forms an ISO-8601 reset time comes
/// in: `YYYY-MM-DD`, optionally followed by `T` or a space and `HH[:MM[:SS[.f]]]`, then
/// optionally `Z` or `±HH[:MM[:SS]]`. Returns epoch seconds.
///
/// A value without an offset is read as UTC. Python would raise `TypeError` comparing it
/// with an aware `now` (an uncaught crash at startup); nothing writes such a value, and
/// guessing UTC is the one reading that never deletes a pause the user still needs.
/// Anything else (`None`) is "unparsable", which the startup check treats as stale.
pub fn parse_iso(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    if b.len() < 10 || !text.is_ascii() {
        return None;
    }
    let num = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse().ok())?
    };
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = num(&text[0..4])?;
    let m = num(&text[5..7])? as u32;
    let d = num(&text[8..10])? as u32;
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) || y < 1 {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) as f64 * 86_400.0;
    let rest = &text[10..];
    if rest.is_empty() {
        return Some(secs);
    }
    let rest = rest.strip_prefix(['T', ' '])?;
    // Split the time from the offset at the first `Z`, `+` or `-`.
    let cut = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (time, offset) = rest.split_at(cut);
    let mut parts = time.splitn(3, ':');
    let hh = num(parts.next()?)?;
    let mm = parts.next().map_or(Some(0), num)?;
    let ss_text = parts.next().unwrap_or("0");
    let (ss_int, frac) = match ss_text.split_once('.') {
        Some((i, f)) => (i, f),
        None => (ss_text, ""),
    };
    let ss = num(ss_int)?;
    if hh > 23 || mm > 59 || ss > 59 || frac.len() > 6 {
        return None;
    }
    let frac = if frac.is_empty() {
        0.0
    } else {
        format!("0.{frac}")
            .parse::<f64>()
            .ok()
            .filter(|_| num(frac).is_some())?
    };
    secs += (hh * 3600 + mm * 60 + ss) as f64 + frac;
    let off = match offset {
        "" | "Z" => 0,
        o => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let body = o.strip_prefix(['+', '-'])?;
            let mut p = body.splitn(3, ':');
            let oh = num(p.next()?)?;
            let om = p.next().map_or(Some(0), num)?;
            let os = p.next().map_or(Some(0), num)?;
            if oh > 23 || om > 59 || os > 59 {
                return None;
            }
            sign * (oh * 3600 + om * 60 + os)
        }
    };
    Some(secs - off as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values: Python `datetime.fromtimestamp(t, utc)` / `calendar.timegm`.
    #[test]
    fn epoch_zero_and_leap_days() {
        assert_eq!(format_stamp(0.0), "1970-01-01 00:00:00");
        assert_eq!(format_stamp(951_782_400.0), "2000-02-29 00:00:00");
        assert_eq!(format_stamp(1_792_639_907.558), "2026-10-22 03:31:47");
        assert_eq!(format_stamp(-1.0), "1969-12-31 23:59:59");
    }

    #[test]
    fn next_midnight_matches_python() {
        // 1792639907.558 is 2026-10-22 03:31:47 UTC.
        assert_eq!(
            next_utc_midnight_iso(1_792_639_907.558),
            "2026-10-23T00:00:00+00:00"
        );
        // Exactly at midnight, the *next* one.
        assert_eq!(
            next_utc_midnight_iso(1_792_627_200.0),
            "2026-10-23T00:00:00+00:00"
        );
        // One tick before midnight.
        assert_eq!(
            next_utc_midnight_iso(1_792_627_199.999),
            "2026-10-22T00:00:00+00:00"
        );
        // Month, year and leap-year rollovers.
        assert_eq!(
            next_utc_midnight_iso(1_793_404_800.0 - 1.0),
            "2026-10-31T00:00:00+00:00"
        );
        assert_eq!(
            next_utc_midnight_iso(1_798_675_200.0 - 1.0),
            "2026-12-31T00:00:00+00:00"
        );
        assert_eq!(
            next_utc_midnight_iso(951_782_400.0 + 5.0),
            "2000-03-01T00:00:00+00:00"
        );
        assert_eq!(
            next_utc_midnight_iso(946_684_799.0),
            "2000-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn iso_round_trips_and_handles_offsets() {
        assert_eq!(
            parse_iso("2026-10-23T00:00:00+00:00"),
            Some(1_792_713_600.0)
        );
        assert_eq!(parse_iso("2026-10-23T00:00:00Z"), Some(1_792_713_600.0));
        assert_eq!(parse_iso("2026-10-23 00:00:00"), Some(1_792_713_600.0));
        assert_eq!(parse_iso("2026-10-23"), Some(1_792_713_600.0));
        assert_eq!(
            parse_iso("2026-10-23T02:00:00+02:00"),
            Some(1_792_713_600.0)
        );
        assert_eq!(
            parse_iso("2026-10-22T22:30:00-01:30"),
            Some(1_792_713_600.0)
        );
        assert_eq!(
            parse_iso("2026-10-23T00:00:00.5+00:00"),
            Some(1_792_713_600.5)
        );
        assert_eq!(parse_iso("2026-10-23T00:00"), Some(1_792_713_600.0));
    }

    #[test]
    fn unparsable_is_none() {
        for bad in [
            "",
            "x",
            "tomorrow",
            "2026-13-01",
            "2026-02-30",
            "2026-10-23T25:00:00",
            "2026-10-23T00:00:00+25:00",
            "2026-10-23T",
            "2026-10-23X00:00:00",
            "2026-10-23T00:00:00.1234567",
            "2026/10/23",
            "２０２６-10-23",
            "0000-01-01",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn parse_inverts_next_midnight() {
        for t in [0.0, 86_399.0, 1_792_639_907.558, 4_102_444_799.0] {
            let iso = next_utc_midnight_iso(t);
            let back = parse_iso(&iso).unwrap();
            assert!(back > t && back - t <= 86_400.0, "{iso}");
            assert_eq!(back % 86_400.0, 0.0);
        }
    }
}
