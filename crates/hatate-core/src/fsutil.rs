//! Atomic file replacement for the files both apps share.
//!
//! The Python app writes the search cache and the quota pause file in place
//! (`Path.write_text`: truncate, then write), so a crash leaves a truncated file. The
//! Rust app writes a temp file in the same directory, fsyncs it and renames it over the
//! target: a reader, and a process killed at any instant, sees the old file or the new
//! one and never a partial one (spec 6.3, "should write atomically").

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How far a simulated kill gets before the process "dies". Only tests ask for these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// Half of the bytes are in the temp file, nothing is synced, the target is untouched.
    PartialTemp,
    /// The temp file is complete and synced; the rename has not happened.
    TempSynced,
    /// The rename has happened (the new file is in place).
    Renamed,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The prefix of every temp file this module makes: `.<name>.<pid>.<n>.tmp`. A leading dot
/// and a `.tmp` suffix keep them out of `*.json` globs, which is how the Python app
/// lists and clears `search_cache/`.
fn temp_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()))
}

/// True for a name `temp_path_for` could have made (leftovers of a killed run).
pub(crate) fn is_temp_leftover(file_name: &str) -> bool {
    file_name.starts_with('.') && file_name.ends_with(".tmp")
}

/// Replace `target` with `bytes`, atomically. The parent directory must exist.
pub(crate) fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_until(target, bytes, None)
}

/// [`write_atomic`] that stops dead after `until`, as a `SIGKILL` at that point would:
/// the files are left exactly as they are and an `Interrupted` error is returned.
pub(crate) fn write_atomic_until(
    target: &Path,
    bytes: &[u8],
    until: Option<Stage>,
) -> io::Result<()> {
    let tmp = temp_path_for(target);
    let result = stage_and_rename(target, &tmp, bytes, until);
    if result.is_err() && !matches!(&result, Err(e) if is_simulated_kill(e)) {
        // A real failure: do not leave our temp file behind. A simulated kill leaves
        // everything where it fell, which is the point of the test.
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn killed() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "simulated kill")
}

fn is_simulated_kill(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted && e.to_string() == "simulated kill"
}

fn stage_and_rename(
    target: &Path,
    tmp: &Path,
    bytes: &[u8],
    until: Option<Stage>,
) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(tmp)?;
    let half = bytes.len() / 2;
    file.write_all(&bytes[..half])?;
    if until == Some(Stage::PartialTemp) {
        file.flush()?;
        return Err(killed());
    }
    file.write_all(&bytes[half..])?;
    file.sync_all()?;
    drop(file);
    if until == Some(Stage::TempSynced) {
        return Err(killed());
    }
    fs::rename(tmp, target)?;
    if until == Some(Stage::Renamed) {
        return Err(killed());
    }
    // Best effort: make the rename itself durable. A filesystem that cannot fsync a
    // directory has already given us the atomic rename.
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("hatate-core-test-")
            .tempdir()
            .unwrap()
    }

    #[test]
    fn replaces_and_creates() {
        let d = dir();
        let f = d.path().join("x.json");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"one");
        write_atomic(&f, b"two!").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two!");
        // No temp file is left behind on success.
        let names: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn a_kill_at_any_stage_leaves_the_old_or_the_new_file() {
        for (stage, expect_new) in [
            (Stage::PartialTemp, false),
            (Stage::TempSynced, false),
            (Stage::Renamed, true),
        ] {
            let d = dir();
            let f = d.path().join("x.json");
            fs::write(&f, b"OLD-CONTENT").unwrap();
            let e = write_atomic_until(&f, b"NEW-CONTENT-LONGER", Some(stage)).unwrap_err();
            assert!(is_simulated_kill(&e));
            let now = fs::read(&f).unwrap();
            assert_eq!(
                now,
                if expect_new {
                    &b"NEW-CONTENT-LONGER"[..]
                } else {
                    &b"OLD-CONTENT"[..]
                },
                "{stage:?}"
            );
            // The simulated kill left its temp file, and it is not a *.json file.
            let leftovers: Vec<String> = fs::read_dir(d.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| is_temp_leftover(n))
                .collect();
            assert_eq!(leftovers.len(), usize::from(!expect_new), "{stage:?}");
            assert!(leftovers.iter().all(|n| !n.ends_with(".json")));
        }
    }

    #[test]
    fn a_real_failure_removes_its_temp_file() {
        let d = dir();
        // The target is a non-empty directory: the rename fails after the temp is written.
        let f = d.path().join("x.json");
        fs::create_dir(&f).unwrap();
        fs::write(f.join("inner"), b"x").unwrap();
        assert!(write_atomic(&f, b"data").is_err());
        let names: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "only the directory remains");
    }
}
