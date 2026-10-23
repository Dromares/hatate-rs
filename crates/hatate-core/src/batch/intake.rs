//! Deciding which files enter the working list (`core/file_intake.py`, `core/formats.py`).
//!
//! [`scan_folder`] walks a folder for files with an image or video extension, and
//! [`filter_duplicate_paths`] drops the ones already in the list, by path and then by content
//! hash. Neither touches the content of a file: the hashes it needs were computed before.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::debug;

use crate::model::Entry;

/// `IMAGE_EXTENSIONS`: what the app adds from a folder. Lower case, with the dot. Hydrus
/// libraries also hold AVIF, JPEG XL, TIFF and video, so those are accepted even when a given
/// engine later rejects them: an engine rejecting a file is an error on the row, not a silent
/// skip at add time.
pub const IMAGE_EXTENSIONS: [&str; 15] = [
    ".jpg", ".jpeg", ".jfif", ".png", ".apng", ".gif", ".bmp", ".webp", ".avif", ".jxl", ".tif",
    ".tiff", ".webm", ".mp4", ".mkv",
];

/// `os.path.splitext(name)[1].lower()` is in the set. `splitext` finds the last dot that is not
/// the first character of the name, so `.png` (a hidden file with no extension) has none, and
/// `a.b.png` has `.png`. The lower-casing is Python's `str.lower`, which is full Unicode; a name
/// whose extension is not ASCII is never in the set either way.
pub fn has_image_extension(name: &str) -> bool {
    let ext = match name.rfind('.') {
        Some(i) if i > 0 && !name[..i].chars().all(|c| c == '.') => &name[i..],
        _ => return false,
    };
    let lowered = ext.to_lowercase();
    IMAGE_EXTENSIONS.contains(&lowered.as_str())
}

/// `scan_folder`: every image under `folder`, recursively.
///
/// Extension-based, and deliberately so: the alternative is opening tens of thousands of files to
/// sniff their contents before anything has been asked of them. Like `os.walk` it does not follow
/// symlinks to directories (a link to a directory is not entered), and it lists a symlink to a file
/// as a file, whether or not the target exists.
///
/// **Order.** `os.walk` yields a directory's files in whatever order the file system lists them,
/// root first and then each subdirectory in listing order. Here every directory's names are sorted
/// (by bytes) first, so a re-run lists the same files in the same order, which matters because the
/// first of two identical files is the one that is kept. This is the one deliberate difference
/// from the Python order; the set of paths is the same. An unreadable directory is skipped, as
/// `os.walk` does without an `onerror`.
pub fn scan_folder(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(folder, &mut found);
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<std::ffi::OsString> = Vec::new();
    let mut dirs: Vec<std::ffi::OsString> = Vec::new();
    for item in read.flatten() {
        // `DirEntry::file_type` does not follow a symlink; `is_dir` in Python does, which is
        // what puts a link to a directory in `dirs` (and so out of `files`).
        let is_dir = match item.file_type() {
            Ok(t) if t.is_symlink() => std::fs::metadata(item.path()).is_ok_and(|m| m.is_dir()),
            Ok(t) => t.is_dir(),
            Err(_) => false,
        };
        let is_real_dir = item.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            if is_real_dir {
                dirs.push(item.file_name());
            }
        } else {
            files.push(item.file_name());
        }
    }
    files.sort();
    dirs.sort();
    for name in files {
        if has_image_extension(&name.to_string_lossy()) {
            found.push(dir.join(&name));
        }
    }
    for name in dirs {
        walk(&dir.join(&name), found);
    }
}

/// `os.path.abspath`: absolute and normalised without touching the file system (no symlink
/// resolution): `.` parts are dropped and `..` pops one part.
pub fn abspath(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::from("/");
    for part in joined.components() {
        match part {
            std::path::Component::Normal(p) => out.push(p),
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

/// `filter_duplicate_paths`: drops paths already represented in the list. Two rounds, cheapest
/// first. An exact path match is free. A content-hash match catches the same image arriving under
/// a different name: a copy, a re-download, the same file in two folders.
///
/// Duplicates within the batch count too: the accumulating sets are updated as it goes, so a
/// folder containing a file and its own copy adds one of them, not both. No I/O: a path with no
/// hash falls through to the path check alone. Returns the unique paths and the duplicate count.
pub fn filter_duplicate_paths(
    paths: &[PathBuf],
    path_to_hash: &HashMap<PathBuf, String>,
    existing: &[Entry],
) -> (Vec<PathBuf>, usize) {
    let mut existing_abs: HashSet<PathBuf> = existing
        .iter()
        .map(|e| abspath(Path::new(&e.path)))
        .collect();
    let mut existing_hashes: HashSet<String> = existing
        .iter()
        .filter_map(|e| e.hydrus_hash.as_deref())
        .filter(|h| !h.is_empty())
        .map(str::to_owned)
        .collect();

    let mut unique = Vec::new();
    let mut duplicates = 0;
    for p in paths {
        let abs = abspath(p);
        if existing_abs.contains(&abs) {
            duplicates += 1;
            debug!("Skipping {} - already in the list (same path)", p.display());
            continue;
        }
        let hash = path_to_hash.get(p).filter(|h| !h.is_empty());
        if let Some(h) = hash {
            if existing_hashes.contains(h) {
                duplicates += 1;
                debug!(
                    "Skipping {} - duplicate content of an already-added image",
                    p.display()
                );
                continue;
            }
            existing_hashes.insert(h.clone());
        }
        existing_abs.insert(abs);
        unique.push(p.clone());
    }
    (unique, duplicates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, hash: Option<&str>) -> Entry {
        let mut e = Entry::new(path);
        e.hydrus_hash = hash.map(str::to_owned);
        e
    }

    #[test]
    fn extension_set_is_the_python_one_case_insensitive() {
        for ok in [
            "a.jpg", "a.JPEG", "a.Png", "x.y.webp", "v.MKV", "a.tiff", "a.jxl",
        ] {
            assert!(has_image_extension(ok), "{ok}");
        }
        for no in [
            "a.txt",
            "a",
            ".png",
            "a.png.bak",
            "png",
            "a.",
            "a.jpe",
            "..png",
        ] {
            assert!(!has_image_extension(no), "{no}");
        }
    }

    #[test]
    fn scan_finds_images_recursively_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        for f in [
            "b.png",
            "a.JPG",
            "note.txt",
            "sub/c.gif",
            "sub/deeper/d.webp",
        ] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        let names: Vec<String> = scan_folder(root)
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.JPG", "b.png", "sub/c.gif", "sub/deeper/d.webp"]);
    }

    #[test]
    fn scan_does_not_enter_a_symlinked_directory_but_lists_a_file_link() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::write(root.join("real/a.png"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("real/a.png"), root.join("b.png")).unwrap();
        std::os::unix::fs::symlink(root.join("gone.png"), root.join("dangling.png")).unwrap();
        let names: Vec<String> = scan_folder(root)
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["b.png", "dangling.png", "real/a.png"]);
    }

    #[test]
    fn scan_of_a_missing_folder_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_folder(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn abspath_normalises_without_resolving() {
        assert_eq!(abspath(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(abspath(Path::new("/a//b/")), PathBuf::from("/a/b"));
        assert_eq!(abspath(Path::new("/../..")), PathBuf::from("/"));
        assert!(abspath(Path::new("rel/x")).is_absolute());
    }

    #[test]
    fn an_existing_path_is_a_duplicate_even_spelled_differently() {
        let existing = [entry("/p/a.png", None)];
        let paths = vec![PathBuf::from("/p/./a.png"), PathBuf::from("/p/b.png")];
        let (unique, dups) = filter_duplicate_paths(&paths, &HashMap::new(), &existing);
        assert_eq!(unique, [PathBuf::from("/p/b.png")]);
        assert_eq!(dups, 1);
    }

    #[test]
    fn the_same_content_under_another_name_is_a_duplicate() {
        let existing = [entry("/p/a.png", Some("h1"))];
        let paths = vec![PathBuf::from("/q/copy.png"), PathBuf::from("/q/new.png")];
        let hashes = HashMap::from([
            (PathBuf::from("/q/copy.png"), "h1".to_owned()),
            (PathBuf::from("/q/new.png"), "h2".to_owned()),
        ]);
        let (unique, dups) = filter_duplicate_paths(&paths, &hashes, &existing);
        assert_eq!(unique, [PathBuf::from("/q/new.png")]);
        assert_eq!(dups, 1);
    }

    #[test]
    fn duplicates_inside_the_batch_keep_the_first() {
        let paths = vec![
            PathBuf::from("/q/1.png"),
            PathBuf::from("/q/2.png"),
            PathBuf::from("/q/1.png"),
        ];
        let hashes = HashMap::from([
            (PathBuf::from("/q/1.png"), "h".to_owned()),
            (PathBuf::from("/q/2.png"), "h".to_owned()),
        ]);
        let (unique, dups) = filter_duplicate_paths(&paths, &hashes, &[]);
        assert_eq!(unique, [PathBuf::from("/q/1.png")]);
        assert_eq!(dups, 2);
    }

    #[test]
    fn a_path_with_no_hash_is_judged_on_its_path_alone() {
        let existing = [entry("/p/a.png", Some("h1"))];
        let paths = vec![PathBuf::from("/q/unhashed.png")];
        let (unique, dups) = filter_duplicate_paths(&paths, &HashMap::new(), &existing);
        assert_eq!((unique.len(), dups), (1, 0));
    }

    #[test]
    fn an_empty_hash_never_matches_another_empty_hash() {
        let existing = [entry("/p/a.png", Some(""))];
        let paths = vec![PathBuf::from("/q/b.png")];
        let hashes = HashMap::from([(PathBuf::from("/q/b.png"), String::new())]);
        let (unique, dups) = filter_duplicate_paths(&paths, &hashes, &existing);
        assert_eq!((unique.len(), dups), (1, 0));
    }
}
