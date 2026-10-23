//! The matched-URL log (`core/logger.py`): one line per match, `[stamp] filename -> url`, so a
//! user can review what a run found (Files > Open Matched URLs in the Python app). Rotated like
//! `app.log`: past [`MAX_BYTES`] the file becomes `.1`, `.1` becomes `.2`, up to [`BACKUPS`].

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use tracing::{debug, error};

use crate::paths::rotated;

/// `MAX_BYTES`.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// `BACKUPS`.
pub const BACKUPS: u32 = 3;

fn rotate(path: &Path) {
    match fs::metadata(path) {
        Ok(m) if m.len() >= MAX_BYTES => {}
        _ => return,
    }
    for index in (1..BACKUPS).rev() {
        let older = rotated(path, index);
        if older.exists() {
            let _ = fs::rename(&older, rotated(path, index + 1));
        }
    }
    let _ = fs::rename(path, rotated(path, 1));
}

/// `log_matched_url`: appends one line. Failing to write is logged and otherwise ignored, as in
/// Python (an `OSError` never stops a search). `stamp` is the local `%Y-%m-%d %H:%M:%S`.
pub fn log_matched_url(log_path: &str, stamp: &str, image_filename: &str, url: &str) {
    let path = Path::new(log_path);
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        rotate(path);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(format!("[{stamp}] {image_filename} -> {url}\n").as_bytes())
    })();
    match result {
        Ok(()) => debug!(
            "Logged matched URL for {image_filename} to {}",
            path.display()
        ),
        Err(e) => error!(
            "Could not write matched-URL log entry to {}: {e}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_a_line_and_creates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("deep/er/matched.log");
        let log = log.to_str().unwrap();
        log_matched_url(log, "2026-10-23 10:00:00", "a.png", "https://x/1");
        log_matched_url(log, "2026-10-23 10:00:05", "b.png", "https://x/2");
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            "[2026-10-23 10:00:00] a.png -> https://x/1\n[2026-10-23 10:00:05] b.png -> https://x/2\n"
        );
    }

    #[test]
    fn a_full_file_rotates_to_dot_one_and_the_old_dot_one_to_dot_two() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("m.log");
        let big = fs::File::create(&log).unwrap();
        big.set_len(MAX_BYTES).unwrap();
        fs::write(rotated(&log, 1), "older").unwrap();
        log_matched_url(log.to_str().unwrap(), "s", "a.png", "u");
        assert_eq!(fs::read_to_string(&log).unwrap(), "[s] a.png -> u\n");
        assert_eq!(fs::metadata(rotated(&log, 1)).unwrap().len(), MAX_BYTES);
        assert_eq!(fs::read_to_string(rotated(&log, 2)).unwrap(), "older");
    }

    #[test]
    fn a_log_that_cannot_be_written_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be.
        log_matched_url(dir.path().to_str().unwrap(), "s", "a.png", "u");
    }
}
