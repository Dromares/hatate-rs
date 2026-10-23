//! The auto-import staging directory (`workers/search_worker.py`, Phase 4.g, DAN-2132): the
//! per-run directory the download paths write into, the `owner.pid` that says whose it is, the
//! sweep of directories an earlier run left behind, and the cleanup however a run ends.
//!
//! **What is ported.** `_write_pid_file`, `_read_pid_file`, `_pid_is_alive`,
//! `_cleanup_stale_staging_dirs`, the `mkdtemp` + pid-file step at the top of `SearchWorker._run`,
//! `SearchWorker._cleanup_auto_import_tmp`, and the module-level `_current_staging_dir` that the
//! atexit/signal handlers of `register_cleanup_handlers` clean up ([`cleanup_current`]).
//!
//! **What reaches the caller as an error.** Python lets two things escape the worker thread: the
//! second `mkdir` of the root and `mkdtemp` (`OSError`), and `os.kill(pid, 0)` for an `owner.pid`
//! that holds a number larger than a C `int` (`OverflowError`, which the sweep's
//! `except OSError` does not catch). Both end the run before any image is searched, so they are
//! [`StagingError`]s here.

use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::{debug, info, warn};

use crate::error::ErrorKind;
use crate::pycompat::strip;

/// `_PID_FILE_NAME`.
pub const PID_FILE_NAME: &str = "owner.pid";
/// The `prefix` the staging directory is made with, and the only names the sweep looks at.
pub const STAGING_PREFIX: &str = "hatate-linux-auto-";

/// `_current_staging_dir`: the directory an atexit/signal handler cleans up.
static CURRENT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Why the staging directory could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagingError {
    /// `mkdir` of the staging root or `mkdtemp` failed (`OSError`).
    Create(String),
    /// An `owner.pid` names a process id too large for `os.kill` (`OverflowError`).
    PidOutOfRange(String),
}

impl StagingError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            StagingError::Create(_) => ErrorKind::Io,
            StagingError::PidOutOfRange(_) => ErrorKind::InvalidInput,
        }
    }

    /// The text of the exception Python would have raised.
    pub fn message(&self) -> String {
        match self {
            StagingError::Create(text) | StagingError::PidOutOfRange(text) => text.clone(),
        }
    }

    /// What to tell a person: no path, no OS text.
    pub fn user_message(&self) -> String {
        match self {
            StagingError::Create(_) => "Hatate could not make its temporary folder for \
                automatic imports. Check that the cache folder exists and can be written to."
                .to_owned(),
            StagingError::PidOutOfRange(_) => "A leftover marker in Hatate's temporary folder for \
                automatic imports is not readable. Delete the old folders under the cache \
                folder's auto_import_staging and try again."
                .to_owned(),
        }
    }

    /// The raw text for a log (may hold a path or an OS error).
    pub fn detail(&self) -> String {
        self.message()
    }
}

impl std::fmt::Display for StagingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for StagingError {}

/// `shutil.rmtree(path, ignore_errors=True)`: a symbolic link is left alone (rmtree refuses it),
/// links inside are unlinked and never followed, and whatever cannot be removed stays.
pub fn rmtree_ignore_errors(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    if let Ok(children) = std::fs::read_dir(path) {
        for child in children.flatten() {
            let p = child.path();
            match child.file_type() {
                Ok(t) if t.is_dir() => rmtree_ignore_errors(&p),
                _ => {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
    let _ = std::fs::remove_dir(path);
}

/// `_write_pid_file`: this process's id, no newline. A failure is a warning.
pub fn write_pid_file(staging_dir: &Path) {
    let file = staging_dir.join(PID_FILE_NAME);
    if let Err(e) = std::fs::write(&file, std::process::id().to_string()) {
        warn!("Could not write PID file to {}: {e}", staging_dir.display());
    }
}

/// What an `owner.pid` holds, as far as the sweep can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerPid {
    /// `_read_pid_file` returned `None`: missing, unreadable, not UTF-8, or not a decimal number.
    Unknown,
    /// A number, saturated at `u64::MAX` (Python's int has no ceiling; anything past a C `int`
    /// is an `OverflowError` for `os.kill` either way).
    Pid(u64),
}

/// The first code point of each block of ten decimal digits (`unicodedata.category == "Nd"`),
/// Unicode 16.0, which is what CPython 3.14 carries. `str.isdigit()` and `int()` accept all of
/// them; `int()` refuses the other digits `isdigit()` accepts (the superscripts, circled digits).
const DECIMAL_ZEROS: [u32; 76] = [
    0x30, 0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66,
    0xDE6, 0xE50, 0xED0, 0xF20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90,
    0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
    0x104A0, 0x10D30, 0x10D40, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450, 0x114D0,
    0x11650, 0x116C0, 0x116D0, 0x116DA, 0x11730, 0x118E0, 0x11950, 0x11BF0, 0x11C50, 0x11D50,
    0x11DA0, 0x11F50, 0x16130, 0x16A60, 0x16AC0, 0x16B50, 0x16D70, 0x1CCF0, 0x1D7CE, 0x1D7D8,
    0x1D7E2, 0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E4F0, 0x1E5F1, 0x1E950, 0x1FBF0,
];

/// The value of a decimal digit, whatever script it is in.
fn decimal_value(c: char) -> Option<u32> {
    let cp = c as u32;
    DECIMAL_ZEROS
        .iter()
        .find(|&&zero| (zero..zero + 10).contains(&cp))
        .map(|&zero| cp - zero)
}

/// `int(content)` for a stripped `content` that `isdigit()` accepted, as far as `_read_pid_file`
/// goes: every character a decimal digit.
fn parse_decimal(content: &str) -> Option<u64> {
    if content.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for c in content.chars() {
        let digit = decimal_value(c)?;
        value = value.saturating_mul(10).saturating_add(u64::from(digit));
    }
    Some(value)
}

/// `_read_pid_file`.
pub fn read_pid_file(staging_dir: &Path) -> OwnerPid {
    let Ok(bytes) = std::fs::read(staging_dir.join(PID_FILE_NAME)) else {
        return OwnerPid::Unknown;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return OwnerPid::Unknown;
    };
    match parse_decimal(strip(&text)) {
        Some(pid) => OwnerPid::Pid(pid),
        None => OwnerPid::Unknown,
    }
}

/// `_pid_is_alive`: `os.kill(pid, 0)`. No such process is "not alive"; any other answer (a
/// process of another user, say) is "alive". A pid of zero is "not alive" without asking.
pub fn pid_is_alive(pid: u64) -> Result<bool, StagingError> {
    if pid == 0 {
        return Ok(false);
    }
    let Ok(pid) = i32::try_from(pid) else {
        return Err(StagingError::PidOutOfRange(
            "signed integer is greater than maximum".to_owned(),
        ));
    };
    // SAFETY: `kill` with signal 0 sends nothing; it only asks whether the process exists.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return Ok(true);
    }
    Ok(std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH))
}

/// `_cleanup_stale_staging_dirs`: removes the staging directories earlier runs left, except the
/// ones whose owner is provably still running. Errors of the filesystem are a warning; only an
/// `owner.pid` Python's `os.kill` would choke on ends the run.
pub fn cleanup_stale(root: &Path) -> Result<(), StagingError> {
    if let Err(e) = std::fs::create_dir_all(root) {
        warn!("Could not clean up stale staging directories: {e}");
        return Ok(());
    }
    let children = match std::fs::read_dir(root) {
        Ok(c) => c,
        Err(e) => {
            warn!("Could not clean up stale staging directories: {e}");
            return Ok(());
        }
    };
    for child in children {
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                warn!("Could not clean up stale staging directories: {e}");
                return Ok(());
            }
        };
        let path = child.path();
        // `child.is_dir()` follows symbolic links.
        if !(path.is_dir()
            && child
                .file_name()
                .to_string_lossy()
                .starts_with(STAGING_PREFIX))
        {
            continue;
        }
        let pid = read_pid_file(&path);
        if let OwnerPid::Pid(n) = pid {
            if pid_is_alive(n)? {
                debug!(
                    "Skipping live staging directory {} (owner PID {n})",
                    path.display()
                );
                continue;
            }
        }
        rmtree_ignore_errors(&path);
        info!(
            "Cleaned up stale auto-import staging directory {} (owner PID {})",
            path.display(),
            match pid {
                OwnerPid::Pid(n) if n != 0 => n.to_string(),
                _ => "unknown".to_owned(),
            }
        );
    }
    Ok(())
}

/// This run's staging directory. It is removed when the value is dropped, which is however the
/// run ends (`run()`'s `finally`).
#[derive(Debug)]
pub struct StagingDir {
    path: PathBuf,
}

const NAME_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";

/// Eight characters, as `tempfile`'s `_RandomNameSequence` makes them.
fn random_suffix(attempt: u64) -> String {
    use std::hash::{BuildHasher, RandomState};
    let mut bits = RandomState::new().hash_one((attempt, std::process::id()));
    let mut out = String::with_capacity(8);
    for _ in 0..8 {
        out.push(NAME_CHARS[(bits % NAME_CHARS.len() as u64) as usize] as char);
        bits /= NAME_CHARS.len() as u64;
        if bits == 0 {
            bits = RandomState::new().hash_one(attempt ^ 0x9E37_79B9);
        }
    }
    out
}

impl StagingDir {
    /// `AUTO_IMPORT_STAGING_DIR.mkdir(parents=True, exist_ok=True)`, `tempfile.mkdtemp(prefix=...)`
    /// (mode 0700), `_write_pid_file`, and the bookkeeping of `_current_staging_dir`.
    pub fn create(root: &Path) -> Result<StagingDir, StagingError> {
        std::fs::create_dir_all(root).map_err(|e| StagingError::Create(e.to_string()))?;
        let mut attempt = 0u64;
        let path = loop {
            let candidate = root.join(format!("{STAGING_PREFIX}{}", random_suffix(attempt)));
            match std::fs::DirBuilder::new().mode(0o700).create(&candidate) {
                Ok(()) => break candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 10_000 => {
                    attempt += 1;
                }
                Err(e) => return Err(StagingError::Create(e.to_string())),
            }
        };
        write_pid_file(&path);
        *CURRENT.lock().unwrap() = Some(path.clone());
        Ok(StagingDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingDir {
    /// `_cleanup_auto_import_tmp`.
    fn drop(&mut self) {
        rmtree_ignore_errors(&self.path);
        debug!(
            "Cleaned up auto-import staging directory {}",
            self.path.display()
        );
        let mut current = CURRENT.lock().unwrap();
        if current.as_deref() == Some(self.path.as_path()) {
            *current = None;
        }
    }
}

/// The atexit/signal handler's `cleanup()`: removes the staging directory of a run that is still
/// under way. For a process that is about to exit without unwinding (a second signal).
pub fn cleanup_current() {
    let taken = CURRENT.lock().unwrap().take();
    if let Some(dir) = taken {
        rmtree_ignore_errors(&dir);
        debug!(
            "Cleaned up auto-import staging directory {} (atexit/signal)",
            dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_digits_of_any_script_are_a_number_and_other_digits_are_not() {
        assert_eq!(parse_decimal("4242"), Some(4242));
        assert_eq!(parse_decimal("\u{663}\u{664}"), Some(34), "Arabic-Indic");
        assert_eq!(parse_decimal("\u{ff11}\u{ff12}"), Some(12), "full-width");
        assert_eq!(parse_decimal("\u{1d7d9}"), Some(1), "mathematical bold");
        assert_eq!(
            parse_decimal("\u{b2}"),
            None,
            "superscript two is a digit but not decimal"
        );
        assert_eq!(parse_decimal("\u{2460}"), None, "circled one");
        assert_eq!(parse_decimal("-5"), None);
        assert_eq!(parse_decimal("12x"), None);
        assert_eq!(parse_decimal(""), None);
    }

    #[test]
    fn the_table_is_ten_wide_blocks_in_order() {
        assert!(DECIMAL_ZEROS.windows(2).all(|w| w[0] + 10 <= w[1]));
        assert_eq!(DECIMAL_ZEROS.len() * 10, 760);
    }

    #[test]
    fn the_pid_file_is_trimmed_and_anything_else_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(PID_FILE_NAME);
        for (content, want) in [
            (&b"123\n"[..], OwnerPid::Pid(123)),
            (b"  7  ", OwnerPid::Pid(7)),
            (b"0", OwnerPid::Pid(0)),
            (b"", OwnerPid::Unknown),
            (b"abc", OwnerPid::Unknown),
            (b"12 34", OwnerPid::Unknown),
            (b"\xff\xfe", OwnerPid::Unknown),
        ] {
            std::fs::write(&file, content).unwrap();
            assert_eq!(read_pid_file(dir.path()), want, "{content:?}");
        }
        std::fs::remove_file(&file).unwrap();
        assert_eq!(read_pid_file(dir.path()), OwnerPid::Unknown, "missing");
        std::fs::create_dir(&file).unwrap();
        assert_eq!(read_pid_file(dir.path()), OwnerPid::Unknown, "a directory");
    }

    #[test]
    fn zero_is_not_alive_this_process_is_and_a_huge_number_is_an_error() {
        assert_eq!(pid_is_alive(0), Ok(false));
        assert_eq!(pid_is_alive(u64::from(std::process::id())), Ok(true));
        assert_eq!(
            pid_is_alive(i32::MAX as u64 + 1).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_dead_pid_is_not_alive() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = u64::from(child.id());
        child.wait().unwrap();
        assert_eq!(pid_is_alive(pid), Ok(false));
    }

    #[test]
    fn create_makes_a_private_directory_with_this_pid_and_drop_removes_it() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join("cache").join("auto_import_staging");
        let made = StagingDir::create(&staging).unwrap();
        let path = made.path().to_path_buf();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(STAGING_PREFIX));
        assert_eq!(name.len(), STAGING_PREFIX.len() + 8);
        assert_eq!(
            std::fs::read_to_string(path.join(PID_FILE_NAME)).unwrap(),
            std::process::id().to_string()
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::write(path.join("download.png"), b"x").unwrap();
        drop(made);
        assert!(!path.exists());
        assert!(staging.is_dir(), "the root stays");
    }

    #[test]
    fn rmtree_leaves_a_symlink_to_a_directory_alone_and_unlinks_links_inside() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("keep"), b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        rmtree_ignore_errors(&link);
        assert!(link.exists() && real.join("keep").exists());

        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("sub").join("f"), b"x").unwrap();
        std::os::unix::fs::symlink(&real, tree.join("sub").join("to_real")).unwrap();
        rmtree_ignore_errors(&tree);
        assert!(!tree.exists());
        assert!(real.join("keep").exists(), "a link inside is not followed");
    }

    #[test]
    fn a_staging_error_says_neither_the_path_nor_the_os_error() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("a_file");
        std::fs::write(&blocker, b"x").unwrap();
        let err = StagingDir::create(&blocker.join("auto_import_staging")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Io);
        let shown = err.user_message();
        assert!(!shown.contains(dir.path().to_str().unwrap()), "{shown}");
        assert!(
            !shown.contains("os error") && !shown.contains("Errno"),
            "{shown}"
        );
        assert!(
            err.detail().contains("os error"),
            "the detail keeps the raw text"
        );
    }
}
