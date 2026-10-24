//! Finding entries whose file no longer exists on disk (`core/missing_files.py`, and the logic of
//! `workers/missing_file_worker.py`).
//!
//! A saved session stores paths, and paths go stale, most often because the file was deleted from
//! Hydrus. The check lists each *directory* once rather than stat-ing each file, because the cost
//! that matters over a network share is round trips: a Hydrus store shards its files over 256
//! directories, so tens of thousands of files are a couple of hundred requests.
//!
//! The thread that runs the check and the dialog that reports it are the window's (6.d); the
//! decisions in them are here: [`check_session_files`], [`flag_missing`] and the two messages.

use std::collections::{HashMap, HashSet};
use std::fs;

use tracing::{debug, info};

use crate::model::Entry;

/// `os.path.dirname(p)`.
fn dirname(p: &str) -> &str {
    let head = &p[..p.rfind('/').map_or(0, |i| i + 1)];
    if !head.is_empty() && head.bytes().any(|b| b != b'/') {
        head.trim_end_matches('/')
    } else {
        head
    }
}

/// `os.path.basename(p)`.
fn basename(p: &str) -> &str {
    &p[p.rfind('/').map_or(0, |i| i + 1)..]
}

/// The directories of `paths` in first-seen order, each with its paths: `group_by_directory`.
pub fn group_by_directory<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Vec<(String, Vec<&'a str>)> {
    let mut order: Vec<(String, Vec<&'a str>)> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for path in paths {
        let dir = match dirname(path) {
            "" => ".",
            d => d,
        };
        match at.get(dir) {
            Some(&i) => order[i].1.push(path),
            None => {
                at.insert(dir.to_owned(), order.len());
                order.push((dir.to_owned(), vec![path]));
            }
        }
    }
    order
}

/// `find_missing_paths`: the subset of `paths` that no longer exist.
///
/// `stop_check` is polled between directories so a long scan can be abandoned when the app is
/// closing. When it asks to stop, what has been confirmed missing so far is returned; a caller
/// that cares checks the stop condition itself, as [`check_session_files`] does. A directory that
/// cannot be listed (gone, unmounted, unreadable) counts all of its files as missing.
pub fn find_missing_paths<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    stop_check: Option<&dyn Fn() -> bool>,
) -> HashSet<String> {
    let mut missing: HashSet<String> = HashSet::new();
    let grouped = group_by_directory(paths);
    let total: usize = grouped.iter().map(|(_, g)| g.len()).sum();

    for (directory, group) in &grouped {
        if stop_check.is_some_and(|stop| stop()) {
            info!("Missing-file scan stopped early");
            break;
        }
        let listing: std::io::Result<HashSet<String>> = fs::read_dir(directory).and_then(|rd| {
            rd.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect()
        });
        let present = match listing {
            Ok(names) => names,
            Err(exc) => {
                debug!(
                    "Could not list {directory} ({exc}) - treating its {} file(s) as missing",
                    group.len()
                );
                missing.extend(group.iter().map(|p| (*p).to_owned()));
                continue;
            }
        };
        for path in group {
            if !present.contains(basename(path)) {
                missing.insert((*path).to_owned());
            }
        }
    }

    if !missing.is_empty() {
        info!(
            "{} of {total} file(s) are missing from disk, across {} director{}",
            missing.len(),
            grouped.len(),
            if grouped.len() == 1 { "y" } else { "ies" }
        );
    }
    missing
}

/// `MissingFileWorker.run`: the paths of a restored session that no longer exist. A check that
/// was asked to stop reports nothing, as the worker emits an empty set when cancelled.
pub fn check_session_files(paths: &[String], stop_requested: &dyn Fn() -> bool) -> HashSet<String> {
    info!(
        "Checking whether {} session file(s) still exist",
        paths.len()
    );
    let missing = find_missing_paths(paths.iter().map(String::as_str), Some(stop_requested));
    if stop_requested() {
        info!("Missing-file check cancelled");
        return HashSet::new();
    }
    info!("Missing-file check complete: {} missing", missing.len());
    missing
}

/// The loop of `_on_missing_files_checked`: flags every entry whose path is in `missing` and
/// returns how many it flagged.
pub fn flag_missing(entries: &mut [Entry], missing: &HashSet<String>) -> usize {
    let mut affected = 0;
    for entry in entries {
        if missing.contains(&entry.path) {
            entry.file_missing = true;
            affected += 1;
        }
    }
    info!("Flagged {affected} entry(s) whose file is no longer on disk");
    affected
}

/// The status-bar line of `_on_missing_files_checked`.
pub fn missing_status_text(affected: usize) -> String {
    format!("{affected} image(s) in this list are no longer on disk - see the File column")
}

/// The body of the "Missing files" dialog of `_on_missing_files_checked`.
pub fn missing_dialog_text(affected: usize, total: usize) -> String {
    format!(
        "{affected} of {total} image(s) in this session are no longer on disk - most likely \
         deleted from Hydrus since the session was saved.\n\n\
         They're still listed, marked \"missing\" in the File column, and their tags and \
         matched URLs are intact. Anything needing the file itself (searching, uploading, \
         the upscale check) will fail for them, but \"Send URL to Hydrus's URL Importer\" \
         still works - Hydrus re-downloads from the source and applies the tags."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn dirname_and_basename_follow_posixpath() {
        assert_eq!(dirname("/a/b/c.png"), "/a/b");
        assert_eq!(dirname("c.png"), "");
        assert_eq!(dirname("/c.png"), "/");
        assert_eq!(dirname("a//b"), "a");
        assert_eq!(dirname("a/b/"), "a/b");
        assert_eq!(basename("a/b/"), "");
        assert_eq!(basename("a/b.png"), "b.png");
    }

    #[test]
    fn paths_are_grouped_by_directory_in_first_seen_order() {
        let g = group_by_directory(["/b/1", "/a/2", "/b/3", "x"]);
        let dirs: Vec<&str> = g.iter().map(|(d, _)| d.as_str()).collect();
        assert_eq!(dirs, vec!["/b", "/a", "."]);
        assert_eq!(g[0].1, vec!["/b/1", "/b/3"]);
    }

    #[test]
    fn a_missing_file_a_missing_directory_and_a_file_for_a_directory_are_all_missing() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("here.png"), b"x").unwrap();
        std::fs::write(dir.path().join("plain"), b"x").unwrap();
        let paths = [
            format!("{d}/here.png"),
            format!("{d}/gone.png"),
            format!("{d}/nodir/a.png"),
            format!("{d}/plain/a.png"),
        ];
        let missing = find_missing_paths(paths.iter().map(String::as_str), None);
        assert_eq!(missing.len(), 3);
        assert!(!missing.contains(&paths[0]));
    }

    #[test]
    fn a_stop_returns_what_was_found_so_far_and_the_worker_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        for sub in ["one", "two"] {
            std::fs::create_dir(dir.path().join(sub)).unwrap();
        }
        let paths = vec![format!("{d}/one/a.png"), format!("{d}/two/b.png")];
        let polls = Cell::new(0);
        let stop_second = || {
            polls.set(polls.get() + 1);
            polls.get() > 1
        };
        let partial = find_missing_paths(paths.iter().map(String::as_str), Some(&stop_second));
        assert_eq!(partial.len(), 1, "only the first directory was scanned");
        assert!(check_session_files(&paths, &|| true).is_empty());
        assert_eq!(check_session_files(&paths, &|| false).len(), 2);
    }

    #[test]
    fn flagging_marks_the_entries_in_the_set_and_counts_them() {
        let mut entries = vec![Entry::new("/a"), Entry::new("/b"), Entry::new("/a")];
        let missing: HashSet<String> = ["/a".to_owned()].into();
        assert_eq!(flag_missing(&mut entries, &missing), 2);
        assert!(entries[0].file_missing && !entries[1].file_missing && entries[2].file_missing);
        assert_eq!(
            missing_status_text(2),
            "2 image(s) in this list are no longer on disk - see the File column"
        );
        assert!(missing_dialog_text(2, 3).starts_with("2 of 3 image(s) in this session"));
    }

    #[test]
    fn an_unreadable_directory_counts_all_its_files_and_nothing_is_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("a.png"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let paths = [
            locked.join("a.png").to_string_lossy().into_owned(),
            locked.join("b.png").to_string_lossy().into_owned(),
        ];
        let missing = find_missing_paths(paths.iter().map(String::as_str), None);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        if std::fs::read_dir(&locked).is_ok() && missing.is_empty() {
            return; // running as a user the mode does not bind (root): nothing to prove
        }
        assert_eq!(missing.len(), 2);
        assert!(find_missing_paths(std::iter::empty::<&str>(), None).is_empty());
    }
}
