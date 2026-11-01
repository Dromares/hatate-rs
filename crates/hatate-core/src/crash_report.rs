//! The panic hook that gives a Rust crash a place in `crash.log` (`core/crashlog.py` at
//! `76c85c4`; DAN-2501, plan rev 12/13 "Carried into Phase 7").
//!
//! Python's `crash.log` holds `faulthandler` output: every run opens its section with
//! [`SESSION_START_MARK`] and a hard crash dumps `Fatal Python error: ...` and the thread stacks
//! under it. This hook writes the same file in the same shape. A run's section opens with the same
//! mark, and each panic appends one entry whose first line starts [`PANIC_HEADER`] (the line the
//! Crashed banner counts, as it counts `Fatal Python error:`), followed by the timestamp, the
//! version, the binary, the thread, the location and a backtrace. Then the default hook runs, so
//! stderr is as it was.
//!
//! **Known gap:** a native signal (SIGSEGV, SIGABRT from C code, an OOM kill) does not unwind and
//! runs no hook, so it leaves nothing in `crash.log`; Python's `faulthandler` covers the first two.
//! The next launch still sees the `crash.log.running` marker and shows the banner, with "no new
//! entry".
//!
//! This file uses `std` and nothing else, and names nothing in `crate::`: the AppImage's `AppRun`
//! has no dependencies (it runs before the image's libraries are on the path), so it includes this
//! source with `#[path]` instead of linking `hatate-core`.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// `SESSION_START_MARK`: written at the top of every run's section of `crash.log`.
pub const SESSION_START_MARK: &str = "--- session started, faulthandler armed ---";

/// What a panic entry starts with, the counterpart of Python's `FAULT_HEADER`
/// (`Fatal Python error:`). The Crashed banner counts lines that start with either.
pub const PANIC_HEADER: &str = "Fatal Rust panic:";

/// The profile directory name under `$XDG_CONFIG_HOME` (`paths::APP_DIR_NAME`), repeated here
/// because this file names nothing else. `paths::tests` compares the two.
pub const APP_DIR_NAME: &str = "hatate-linux";

/// Whether this process has already opened its section of `crash.log`.
static ARMED: AtomicBool = AtomicBool::new(false);

/// `CONFIG_DIR/crash.log` as `paths::Paths` resolves it: a set `$XDG_CONFIG_HOME` is used as it is
/// (even when empty), otherwise `$HOME/.config`. `None` when neither is set.
pub fn crash_log_path(get: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let base = match get("XDG_CONFIG_HOME") {
        Some(v) => PathBuf::from(v),
        None => PathBuf::from(get("HOME").filter(|h| !h.is_empty())?).join(".config"),
    };
    Some(base.join(APP_DIR_NAME).join("crash.log"))
}

/// Opens this run's section: `"\n" + SESSION_START_MARK + "\n"`, as `_install_faulthandler`
/// writes it. Safe to call twice; the second call does nothing.
pub fn arm(crash_log: &Path) {
    if ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = append(crash_log, &format!("\n{SESSION_START_MARK}\n"));
}

/// Installs the hook: a panic appends its entry to `crash_log` and then runs the hook that was
/// installed before. `binary` names the process (`hatate-gtk`), `version` is the workspace version.
/// A run that has not called [`arm`] gets its section opened by its first panic, so a command that
/// never panics (`hatate --version`) creates no file.
///
/// Best effort throughout, like Python's `install()`: a diagnostic that can itself fail during a
/// crash is worse than none, so every write error is dropped.
pub fn install(crash_log: PathBuf, binary: &'static str, version: &'static str) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let mut text = String::new();
        if !ARMED.swap(true, Ordering::SeqCst) {
            text.push_str(&format!("\n{SESSION_START_MARK}\n"));
        }
        text.push_str(&entry(info, binary, version, now_secs()));
        let _ = append(&crash_log, &text);
        previous(info);
    }));
}

/// One panic's entry. One buffer, written with one `write`, so two threads panicking together do
/// not interleave their lines.
fn entry(info: &PanicHookInfo<'_>, binary: &str, version: &str, now: u64) -> String {
    let thread = std::thread::current();
    let message = payload_text(info);
    let location = match info.location() {
        Some(l) => format!("{}:{}:{}", l.file(), l.line(), l.column()),
        None => "unknown".to_owned(),
    };
    render(
        &message,
        &Fields {
            time: &utc_stamp(now),
            version,
            binary,
            thread: thread.name().unwrap_or("<unnamed>"),
            location: &location,
        },
        &Backtrace::force_capture().to_string(),
    )
}

/// The lines of an entry that are not the message or the backtrace.
struct Fields<'a> {
    time: &'a str,
    version: &'a str,
    binary: &'a str,
    thread: &'a str,
    location: &'a str,
}

/// An entry's text. The message's first line goes on the header line; any further lines are
/// indented so none of them starts a line the banner would count by accident.
fn render(message: &str, f: &Fields<'_>, backtrace: &str) -> String {
    let mut lines = message.lines();
    let first = lines.next().unwrap_or("");
    let mut out = format!("{PANIC_HEADER} {first}\n");
    for more in lines {
        out.push_str(&format!("  {more}\n"));
    }
    out.push_str(&format!(
        "  time: {}\n  version: {}\n  binary: {}\n  thread: {}\n  location: {}\nBacktrace:\n",
        f.time, f.version, f.binary, f.thread, f.location
    ));
    for line in backtrace.lines() {
        out.push_str(&format!("  {line}\n"));
    }
    out
}

fn payload_text(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Box<dyn Any>".to_owned()
    }
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(text.as_bytes())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for epoch seconds (civil-from-days, proleptic Gregorian).
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn the_stamp_is_utc_iso() {
        assert_eq!(utc_stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_stamp(1_793_494_585), "2026-11-01T00:56:25Z");
    }

    #[test]
    fn an_entry_starts_with_the_header_the_banner_counts_and_no_other_line_does() {
        let text = render(
            "first\nsecond line",
            &Fields {
                time: "T",
                version: "0.0.0",
                binary: "hatate-gtk",
                thread: "main",
                location: "x.rs:1:2",
            },
            "0: a\n1: b\n",
        );
        let headers: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with(PANIC_HEADER))
            .collect();
        assert_eq!(headers, ["Fatal Rust panic: first"]);
        assert!(text.contains("  second line\n"), "{text}");
        for want in [
            "  time: T\n",
            "  version: 0.0.0\n",
            "  binary: hatate-gtk\n",
            "  thread: main\n",
            "  location: x.rs:1:2\n",
            "Backtrace:\n  0: a\n",
        ] {
            assert!(text.contains(want), "{want:?} in {text}");
        }
    }

    #[test]
    fn the_path_is_the_one_paths_resolves() {
        let none = |_: &str| None;
        assert_eq!(crash_log_path(&none), None);
        let home = |n: &str| (n == "HOME").then(|| OsString::from("/h"));
        assert_eq!(
            crash_log_path(&home),
            Some(PathBuf::from("/h/.config/hatate-linux/crash.log"))
        );
        // A set variable is used as it is, even when empty.
        let empty = |n: &str| (n == "XDG_CONFIG_HOME").then(OsString::new);
        assert_eq!(
            crash_log_path(&empty),
            Some(PathBuf::from("hatate-linux/crash.log"))
        );
    }
}
