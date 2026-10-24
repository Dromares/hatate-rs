//! The local clock time of an instant, `time.strftime("%H:%M", time.localtime(t))`, without a
//! time-zone crate (DAN-2061).
//!
//! Yandex's captcha message names when the rest ends "as a local clock time, e.g. 21:41", and
//! Python gets it from the C library's `localtime`. This reads the same zone the C library would:
//! `TZ` (a name under `/usr/share/zoneinfo`, an absolute path, or a POSIX rule such as
//! `CET-1CEST,M3.5.0,M10.5.0/3`; empty means UTC), or `/etc/localtime` when `TZ` is unset. A zone
//! it cannot read is UTC, as in glibc.
//!
//! A zone file is TZif (RFC 8536): the transition table, then for any instant after the last
//! transition the POSIX rule in its footer (`slim` files, the default since tzdata 2020b, carry
//! no explicit transitions past the last rule change). Only `Mm.w.d`, `Jn` and `n` rules are
//! understood, which is every rule tzdata writes.

use std::path::Path;

/// `"%H:%M"` of `epoch` (seconds since the Unix epoch) in the process's local zone.
pub fn local_hhmm(epoch: f64) -> String {
    hhmm(epoch, &Zone::from_environment())
}

/// `"%Y-%m-%d %H:%M:%S"` of `epoch` in the process's local zone (`datetime.now().strftime(...)`).
pub fn local_stamp(epoch: f64) -> String {
    let zone = Zone::from_environment();
    let secs = epoch.floor() as i64;
    crate::timeutil::format_stamp((secs + zone.utc_offset(secs)) as f64)
}

/// A wall-clock moment: the fields `strftime` reads from a naive `datetime` (DAN-2214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallClock {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Monday is 0, as `datetime.weekday()`.
    pub weekday: u32,
}

impl WallClock {
    /// The clock as read off a naive timestamp: seconds since 1970-01-01 00:00 on the wall,
    /// with no zone applied.
    pub fn from_naive_secs(secs: i64) -> WallClock {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = crate::timeutil::civil_from_days(days);
        WallClock {
            year,
            month,
            day,
            hour: (rem / 3600) as u32,
            minute: (rem % 3600 / 60) as u32,
            second: (rem % 60) as u32,
            weekday: (days + 3).rem_euclid(7) as u32,
        }
    }

    /// The calendar day as a count of days, for `(finish.date() - start.date()).days`.
    pub fn day_number(&self) -> i64 {
        days_from_civil(self.year, i64::from(self.month), i64::from(self.day))
    }

    /// `%a`, in the C locale.
    pub fn weekday_abbr(&self) -> &'static str {
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][self.weekday as usize]
    }

    /// `%b`, in the C locale.
    pub fn month_abbr(&self) -> &'static str {
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ][self.month as usize - 1]
    }
}

/// `time.localtime(epoch)` (the fields of `datetime.now()` when `epoch` is the present) in the
/// process's local zone.
pub fn local_wall_clock(epoch: f64) -> WallClock {
    let zone = Zone::from_environment();
    let secs = epoch.floor() as i64;
    WallClock::from_naive_secs(secs + zone.utc_offset(secs))
}

/// `"%H:%M"` of `epoch` in `zone`.
fn hhmm(epoch: f64, zone: &Zone) -> String {
    let local = epoch.floor() as i64 + zone.utc_offset(epoch.floor() as i64);
    let seconds_of_day = local.rem_euclid(86_400);
    format!(
        "{:02}:{:02}",
        seconds_of_day / 3600,
        seconds_of_day % 3600 / 60
    )
}

/// A zone: explicit transitions, then a rule for everything after them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Zone {
    /// `(instant, offset from UTC in seconds from then on)`, ascending.
    transitions: Vec<(i64, i32)>,
    /// The offset before the first transition.
    initial: i32,
    /// What applies after the last transition (or always, for a zone given as a rule).
    rule: Option<Rule>,
}

impl Zone {
    fn from_environment() -> Zone {
        match std::env::var_os("TZ") {
            None => read_file(Path::new("/etc/localtime")).unwrap_or_default(),
            Some(tz) => {
                let tz = tz.to_string_lossy().into_owned();
                let name = tz.strip_prefix(':').unwrap_or(&tz);
                if name.is_empty() {
                    return Zone::default();
                }
                let path = if name.starts_with('/') {
                    Path::new(name).to_owned()
                } else {
                    Path::new("/usr/share/zoneinfo").join(name)
                };
                read_file(&path)
                    .or_else(|| Rule::parse(name).map(Zone::from_rule))
                    .unwrap_or_default()
            }
        }
    }

    fn from_rule(rule: Rule) -> Zone {
        Zone {
            transitions: Vec::new(),
            initial: rule.std_offset,
            rule: Some(rule),
        }
    }

    /// Seconds east of UTC at `t`.
    fn utc_offset(&self, t: i64) -> i64 {
        if let Some(&(last, _)) = self.transitions.last()
            && t >= last
            && let Some(rule) = &self.rule
        {
            return i64::from(rule.offset_at(t));
        }
        if self.transitions.is_empty()
            && let Some(rule) = &self.rule
        {
            return i64::from(rule.offset_at(t));
        }
        match self.transitions.partition_point(|&(at, _)| at <= t) {
            0 => i64::from(self.initial),
            n => i64::from(self.transitions[n - 1].1),
        }
    }
}

fn read_file(path: &Path) -> Option<Zone> {
    parse_tzif(&std::fs::read(path).ok()?)
}

/// A TZif file's 64-bit data block and footer.
fn parse_tzif(data: &[u8]) -> Option<Zone> {
    fn counts(head: &[u8]) -> Option<[usize; 6]> {
        let word = |i: usize| {
            head.get(20 + 4 * i..24 + 4 * i)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
        };
        // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
        Some([word(0)?, word(1)?, word(2)?, word(3)?, word(4)?, word(5)?])
    }
    if data.get(..4)? != b"TZif" {
        return None;
    }
    let version = *data.get(4)?;
    let [isut, isstd, leap, time, typ, chars] = counts(data)?;
    // Version 1 has 4-byte times and no footer: good enough when it is all there is.
    let v1_len = 44 + time * 4 + time + typ * 6 + chars + leap * 8 + isstd + isut;
    let (block, time_size, has_footer) = if version >= b'2' {
        let second = data.get(v1_len..)?;
        if second.get(..4)? != b"TZif" {
            return None;
        }
        (second, 8usize, true)
    } else {
        (data, 4usize, false)
    };
    let [isut, isstd, leap, time, typ, chars] = counts(block)?;
    let mut at = 44;
    let times: Vec<i64> = (0..time)
        .map(|i| {
            let b = block.get(at + i * time_size..at + (i + 1) * time_size)?;
            Some(if time_size == 8 {
                i64::from_be_bytes(b.try_into().ok()?)
            } else {
                i64::from(i32::from_be_bytes(b.try_into().ok()?))
            })
        })
        .collect::<Option<_>>()?;
    at += time * time_size;
    let indexes = block.get(at..at + time)?;
    at += time;
    let types: Vec<i32> = (0..typ)
        .map(|i| {
            let b = block.get(at + i * 6..at + i * 6 + 4)?;
            Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        })
        .collect::<Option<_>>()?;
    at += typ * 6 + chars + leap * (time_size + 4) + isstd + isut;
    let transitions: Vec<(i64, i32)> = times
        .iter()
        .zip(indexes)
        .map(|(&t, &i)| Some((t, *types.get(usize::from(i))?)))
        .collect::<Option<_>>()?;
    let rule = if has_footer {
        let footer = block.get(at..)?.strip_prefix(b"\n")?;
        let end = footer.iter().position(|&b| b == b'\n')?;
        std::str::from_utf8(&footer[..end])
            .ok()
            .filter(|text| !text.is_empty())
            .and_then(Rule::parse)
    } else {
        None
    };
    Some(Zone {
        transitions,
        initial: types.first().copied().unwrap_or(0),
        rule,
    })
}

/// A POSIX TZ rule: a standard offset, and optionally a daylight one with the dates it applies.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    /// Seconds east of UTC (the POSIX string counts west).
    std_offset: i32,
    dst: Option<Dst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Dst {
    offset: i32,
    start: (Day, i32),
    end: (Day, i32),
}

/// The day a rule fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Day {
    /// `Mm.w.d`: weekday `d` (0 = Sunday) of week `w` (5 = last) of month `m`.
    Month(u32, u32, u32),
    /// `Jn`: day 1..=365 of the year, February 29th never counted.
    Julian(u32),
    /// `n`: day 0..=365 of the year, February 29th counted in a leap year.
    Zero(u32),
}

impl Rule {
    fn parse(text: &str) -> Option<Rule> {
        let mut p = Cursor(text);
        p.name()?;
        let std_offset = -p.offset()?;
        if p.0.is_empty() {
            return Some(Rule {
                std_offset,
                dst: None,
            });
        }
        p.name()?;
        let dst_offset = if p.0.starts_with(',') || p.0.is_empty() {
            std_offset + 3600
        } else {
            -p.offset()?
        };
        // tzdata always writes the dates; the C library's default (US rules) is not reproduced.
        p.0 = p.0.strip_prefix(',')?;
        let start = p.date()?;
        p.0 = p.0.strip_prefix(',')?;
        let end = p.date()?;
        if !p.0.is_empty() {
            return None;
        }
        Some(Rule {
            std_offset,
            dst: Some(Dst {
                offset: dst_offset,
                start,
                end,
            }),
        })
    }

    fn offset_at(&self, t: i64) -> i32 {
        let Some(dst) = &self.dst else {
            return self.std_offset;
        };
        let year = year_of(t + i64::from(self.std_offset));
        let instants = |year: i64| {
            (
                day_start(year, dst.start.0) + i64::from(dst.start.1) - i64::from(self.std_offset),
                day_start(year, dst.end.0) + i64::from(dst.end.1) - i64::from(dst.offset),
            )
        };
        let (start, end) = instants(year);
        let in_dst = if start < end {
            start <= t && t < end
        } else {
            !(end <= t && t < start)
        };
        if in_dst { dst.offset } else { self.std_offset }
    }
}

struct Cursor<'a>(&'a str);

impl Cursor<'_> {
    /// A zone abbreviation: `<...>` or letters.
    fn name(&mut self) -> Option<()> {
        if let Some(rest) = self.0.strip_prefix('<') {
            let end = rest.find('>')?;
            self.0 = &rest[end + 1..];
        } else {
            let end = self
                .0
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(self.0.len());
            if end < 3 {
                return None;
            }
            self.0 = &self.0[end..];
        }
        Some(())
    }

    /// `[+-]hh[:mm[:ss]]` in seconds.
    fn offset(&mut self) -> Option<i32> {
        let sign = match self.0.chars().next()? {
            '-' => {
                self.0 = &self.0[1..];
                -1
            }
            '+' => {
                self.0 = &self.0[1..];
                1
            }
            _ => 1,
        };
        let mut seconds = 0;
        for (i, scale) in [3600, 60, 1].into_iter().enumerate() {
            if i > 0 {
                match self.0.strip_prefix(':') {
                    Some(rest) => self.0 = rest,
                    None => break,
                }
            }
            seconds += self.number()? * scale;
        }
        Some(sign * seconds)
    }

    fn number(&mut self) -> Option<i32> {
        let end = self
            .0
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(self.0.len());
        let value = self.0.get(..end)?.parse().ok()?;
        self.0 = &self.0[end..];
        Some(value)
    }

    /// `date[/time]`, the time defaulting to 02:00:00.
    fn date(&mut self) -> Option<(Day, i32)> {
        let day = if let Some(rest) = self.0.strip_prefix('M') {
            self.0 = rest;
            let month = self.number()?;
            self.0 = self.0.strip_prefix('.')?;
            let week = self.number()?;
            self.0 = self.0.strip_prefix('.')?;
            let weekday = self.number()?;
            Day::Month(
                u32::try_from(month).ok()?,
                u32::try_from(week).ok()?,
                u32::try_from(weekday).ok()?,
            )
        } else if let Some(rest) = self.0.strip_prefix('J') {
            self.0 = rest;
            Day::Julian(u32::try_from(self.number()?).ok()?)
        } else {
            Day::Zero(u32::try_from(self.number()?).ok()?)
        };
        let time = match self.0.strip_prefix('/') {
            Some(rest) => {
                self.0 = rest;
                self.offset()?
            }
            None => 7200,
        };
        Some((day, time))
    }
}

/// Days from 1970-01-01 to the civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn year_of(t: i64) -> i64 {
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// 00:00 (local, as an offset the caller applies) of the rule's day in `year`, in seconds from
/// the epoch as if the clock were UTC.
fn day_start(year: i64, day: Day) -> i64 {
    let days = match day {
        Day::Julian(n) => {
            let n = i64::from(n);
            let leap_skip = i64::from(is_leap(year) && n >= 60);
            days_from_civil(year, 1, 1) + n - 1 + leap_skip
        }
        Day::Zero(n) => days_from_civil(year, 1, 1) + i64::from(n),
        Day::Month(month, week, weekday) => {
            let (month, week, weekday) = (i64::from(month), i64::from(week), i64::from(weekday));
            let first = days_from_civil(year, month, 1);
            // 1970-01-01 was a Thursday (4).
            let first_weekday = (first + 4).rem_euclid(7);
            let mut day = first + (weekday - first_weekday).rem_euclid(7) + (week - 1) * 7;
            let next_month = if month == 12 {
                days_from_civil(year + 1, 1, 1)
            } else {
                days_from_civil(year, month + 1, 1)
            };
            while day >= next_month {
                day -= 7;
            }
            day
        }
    };
    days * 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-07-01 12:00:00 UTC
    const SUMMER: f64 = 1_782_907_200.0;
    /// 2026-12-01 12:00:00 UTC
    const WINTER: f64 = 1_796_126_400.0;
    /// 2026-03-29 01:00:00 UTC: when Europe's clocks go forward.
    const EU_SPRING: f64 = 1_774_746_000.0;

    fn rule(text: &str) -> Zone {
        Zone::from_rule(Rule::parse(text).expect(text))
    }

    #[test]
    fn a_rule_without_daylight_time_is_a_fixed_offset() {
        assert_eq!(hhmm(SUMMER, &rule("UTC0")), "12:00");
        assert_eq!(hhmm(SUMMER, &rule("<+0530>-5:30")), "17:30");
        assert_eq!(hhmm(SUMMER, &rule("EST5")), "07:00");
        assert_eq!(hhmm(0.0, &Zone::default()), "00:00");
        assert_eq!(hhmm(86_399.9, &Zone::default()), "23:59");
        assert_eq!(hhmm(-60.0, &Zone::default()), "23:59");
    }

    #[test]
    fn daylight_time_follows_the_month_week_day_rule_in_both_hemispheres() {
        let berlin = rule("CET-1CEST,M3.5.0,M10.5.0/3");
        assert_eq!(hhmm(SUMMER, &berlin), "14:00");
        assert_eq!(hhmm(WINTER, &berlin), "13:00");
        // The switch is at 01:00 UTC: a second before is 01:59 CET, at it 03:00 CEST.
        assert_eq!(hhmm(EU_SPRING - 1.0, &berlin), "01:59");
        assert_eq!(hhmm(EU_SPRING, &berlin), "03:00");
        // Auckland: daylight time from the last Sunday of September to the first of April.
        let auckland = rule("NZST-12NZDT,M9.5.0,M4.1.0/3");
        assert_eq!(hhmm(SUMMER, &auckland), "00:00");
        assert_eq!(hhmm(WINTER, &auckland), "01:00");
        // US Eastern, switching at 02:00 local.
        let new_york = rule("EST5EDT,M3.2.0,M11.1.0");
        assert_eq!(hhmm(SUMMER, &new_york), "08:00");
        assert_eq!(hhmm(WINTER, &new_york), "07:00");
    }

    #[test]
    fn the_julian_and_zero_based_day_forms_skip_or_count_february_29th() {
        // 2024 is a leap year: J60 is 1 March, 59 is 29 February.
        assert_eq!(
            day_start(2024, Day::Julian(60)),
            days_from_civil(2024, 3, 1) * 86_400
        );
        assert_eq!(
            day_start(2024, Day::Zero(59)),
            days_from_civil(2024, 2, 29) * 86_400
        );
        assert_eq!(
            day_start(2025, Day::Zero(59)),
            days_from_civil(2025, 3, 1) * 86_400
        );
        assert_eq!(year_of(SUMMER as i64), 2026);
        assert_eq!(year_of(-1), 1969);
    }

    #[test]
    fn a_bad_rule_is_not_a_zone() {
        for bad in [
            "",
            "A",
            "CET",
            "CET-1CEST",
            "CET-1CEST,M3.5.0",
            "UTC0 extra",
        ] {
            assert_eq!(Rule::parse(bad), None, "{bad:?}");
        }
    }

    /// A TZif v2 file with `transitions` (instant, type index), the types' offsets and a footer.
    fn tzif(transitions: &[(i64, u8)], offsets: &[i32], footer: &str) -> Vec<u8> {
        let head = |times: usize| {
            let mut head = b"TZif2".to_vec();
            head.extend_from_slice(&[0; 15]);
            for count in [0, 0, 0, times, offsets.len(), 2] {
                head.extend_from_slice(&u32::try_from(count).unwrap().to_be_bytes());
            }
            head
        };
        let types = || {
            let mut out = Vec::new();
            for &offset in offsets {
                out.extend_from_slice(&offset.to_be_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            out.extend_from_slice(b"X\0");
            out
        };
        // The 32-bit block holds no transitions; the 64-bit one holds them all.
        let mut file = head(0);
        file.extend(types());
        file.extend(head(transitions.len()));
        for &(t, _) in transitions {
            file.extend_from_slice(&t.to_be_bytes());
        }
        file.extend(transitions.iter().map(|&(_, i)| i));
        file.extend(types());
        file.extend_from_slice(format!("\n{footer}\n").as_bytes());
        file
    }

    #[test]
    fn a_zone_file_gives_its_transitions_then_the_footer_rule() {
        let file = tzif(
            &[(1_000_000_000, 1), (1_500_000_000, 0)],
            &[3600, 7200],
            "CET-1CEST,M3.5.0,M10.5.0/3",
        );
        let zone = parse_tzif(&file).unwrap();
        assert_eq!(zone.transitions.len(), 2);
        // Before the first transition the first type applies (3600), between them the second
        // entry's type (7200), after the last the footer's rule.
        assert_eq!(zone.utc_offset(999_999_999), 3600);
        assert_eq!(zone.utc_offset(1_000_000_000), 7200);
        assert_eq!(zone.utc_offset(1_499_999_999), 7200);
        assert_eq!(hhmm(SUMMER, &zone), "14:00");
        assert_eq!(hhmm(WINTER, &zone), "13:00");
        // No footer: the last transition's offset stays.
        let bare = parse_tzif(&tzif(&[(0, 1)], &[0, 7200], "")).unwrap();
        assert_eq!(hhmm(SUMMER, &bare), "14:00");
    }

    #[test]
    fn a_file_that_is_not_tzif_is_not_a_zone() {
        assert_eq!(parse_tzif(b"not a zone file"), None);
        assert_eq!(parse_tzif(b"TZif2"), None);
        assert_eq!(parse_tzif(b""), None);
    }
}
