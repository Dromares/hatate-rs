//! Tag sidecar files, the planning half (`core/sidecar.py`): the tag list as a text file next to
//! the image, one tag per line.
//!
//! [`plan`] touches no disk except to ask whether each target already exists, and returns exactly
//! what would be written and where, so the window can show it (and the count of files it would
//! clobber) and get an answer first. [`write`] (with `_write_new` and `_write_over`) is the
//! write itself, added by DAN-2217, and the texts the window puts around a write are
//! [`nothing_to_write_text`], [`written_summary`] and [`confirmation`].
//!
//! The file name follows `pathlib` as Python 3.14 implements it (`Path.with_suffix`,
//! `Path.with_name`): `cat.jpg` is `cat.jpg.txt` (append) or `cat.txt` (replace), a path is first
//! normalised (`a//b/./c.jpg` is `a/b/c.jpg`), and a name that is only dots has no suffix.
//!
//! Which tags are written is [`tags_to_send`], the single answer to what leaves the app.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::model::{Entry, tags_to_send};

/// `DEFAULT_EXTENSION`.
pub const DEFAULT_EXTENSION: &str = ".txt";

/// `STYLE_APPEND`: `cat.jpg` -> `cat.jpg.txt`.
pub const STYLE_APPEND: &str = "append";
/// `STYLE_REPLACE`: `cat.jpg` -> `cat.txt`.
pub const STYLE_REPLACE: &str = "replace";
pub const STYLES: [&str; 2] = [STYLE_APPEND, STYLE_REPLACE];

pub const OVERWRITE_ASK: &str = "ask";
pub const OVERWRITE_SKIP: &str = "skip";
pub const OVERWRITE_OVERWRITE: &str = "overwrite";
pub const OVERWRITE_POLICIES: [&str; 3] = [OVERWRITE_ASK, OVERWRITE_SKIP, OVERWRITE_OVERWRITE];

/// `style_of`: the naming style a settings object asks for; an unrecognised value falls back to
/// the default rather than failing.
pub fn style_of(settings: Option<&Settings>) -> &'static str {
    match settings.map(|s| s.sidecar_filename_style.as_str()) {
        Some(STYLE_REPLACE) => STYLE_REPLACE,
        _ => STYLE_APPEND,
    }
}

/// `overwrite_policy_of`: the overwrite policy a settings object asks for, same leniency.
pub fn overwrite_policy_of(settings: Option<&Settings>) -> &'static str {
    match settings.map(|s| s.sidecar_overwrite.as_str()) {
        Some(OVERWRITE_SKIP) => OVERWRITE_SKIP,
        Some(OVERWRITE_OVERWRITE) => OVERWRITE_OVERWRITE,
        _ => OVERWRITE_ASK,
    }
}

/// A `PurePosixPath`: the root (`""`, `/` or `//`) and the parts that are neither empty nor `.`.
struct PurePath<'a> {
    root: &'static str,
    parts: Vec<&'a str>,
}

impl<'a> PurePath<'a> {
    fn parse(text: &'a str) -> PurePath<'a> {
        let slashes = text.bytes().take_while(|b| *b == b'/').count();
        // POSIX keeps exactly two leading slashes; one or three or more are one root.
        let root = match slashes {
            0 => "",
            2 => "//",
            _ => "/",
        };
        let parts = text
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        PurePath { root, parts }
    }

    fn render(root: &str, parts: &[&str]) -> String {
        if root.is_empty() && parts.is_empty() {
            ".".to_owned()
        } else {
            format!("{root}{}", parts.join("/"))
        }
    }
}

/// `PurePath.suffix`: from the last dot, unless nothing but dots comes before it.
fn suffix_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if !name[..i].trim_matches('.').is_empty() => &name[i..],
        _ => "",
    }
}

/// `sidecar_path`: where the sidecar for `image_path` goes. `None` where Python raises
/// `ValueError("has an empty name")`: a path with no name (`""`, `/`, `.`) has no sidecar.
pub fn sidecar_path(image_path: &str, style: &str, extension: &str) -> Option<PathBuf> {
    let path = PurePath::parse(image_path);
    let (name, dir) = path.parts.split_last()?;
    let new_name = if style == STYLE_REPLACE {
        let suffix = suffix_of(name);
        let stem = &name[..name.len() - suffix.len()];
        // `with_suffix` on a name that is only dots: the stem is the whole name, so it is kept.
        format!("{stem}{extension}")
    } else {
        format!("{name}{extension}")
    };
    let mut parts: Vec<&str> = dir.to_vec();
    parts.push(&new_name);
    Some(PathBuf::from(PurePath::render(path.root, &parts)))
}

/// `Path.parent` of a path [`sidecar_path`] returned.
fn parent_of(path: &str) -> PathBuf {
    let p = PurePath::parse(path);
    let keep = p.parts.len().saturating_sub(1);
    PathBuf::from(PurePath::render(p.root, &p.parts[..keep]))
}

/// `render`: the file's contents, one tag per line, newline-terminated. Duplicates are dropped
/// keeping the first occurrence, an empty display is dropped, and the order is the entry's own.
pub fn render(entry: &Entry, settings: Option<&Settings>) -> String {
    let mut seen: HashSet<String> = HashSet::new();
    let mut body = String::new();
    for tag in tags_to_send(entry, settings) {
        let display = tag.display();
        if seen.insert(display.clone()) && !display.is_empty() {
            body.push_str(&display);
            body.push('\n');
        }
    }
    body
}

/// `Planned`: one sidecar that would be written.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned<'a> {
    pub entry: &'a Entry,
    pub path: PathBuf,
    pub body: String,
    pub exists: bool,
}

/// `Plan`: what `write()` would do, and what it would refuse to do. The three "not written" lists
/// are kept apart because they are three different things to tell a user.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan<'a> {
    pub writes: Vec<Planned<'a>>,
    pub without_tags: Vec<&'a Entry>,
    pub missing_files: Vec<&'a Entry>,
}

impl Plan<'_> {
    /// `existing`: the planned writes whose target is already on disk.
    pub fn existing(&self) -> Vec<&Planned<'_>> {
        self.writes.iter().filter(|p| p.exists).collect()
    }

    /// `directories`: every folder this plan would write into, in first-seen order.
    pub fn directories(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for p in &self.writes {
            let dir = parent_of(&p.path.to_string_lossy());
            if seen.insert(dir.clone()) {
                out.push(dir);
            }
        }
        out
    }
}

/// `plan`: work out every sidecar for `entries` without writing anything.
///
/// An entry with no tags to write is reported as skipped, not written as an empty file (an empty
/// file claims the image has no tags). An entry whose file has gone missing is skipped too. An
/// entry whose path has no name (Python raises) is reported with the missing files.
pub fn plan<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    settings: Option<&Settings>,
    extension: &str,
) -> Plan<'a> {
    let mut result = Plan::default();
    let style = style_of(settings);
    for entry in entries {
        if entry.file_missing {
            result.missing_files.push(entry);
            continue;
        }
        let body = render(entry, settings);
        if body.is_empty() {
            result.without_tags.push(entry);
            continue;
        }
        let Some(target) = sidecar_path(&entry.path, style, extension) else {
            result.missing_files.push(entry);
            continue;
        };
        let exists = target.exists();
        result.writes.push(Planned {
            entry,
            path: target,
            body,
            exists,
        });
    }
    result
}

/// `Result`: what [`write`] actually did.
#[derive(Debug, Default)]
pub struct WriteResult {
    pub written: Vec<PathBuf>,
    pub skipped_existing: Vec<PathBuf>,
    pub failures: Vec<WriteFailure>,
}

/// One sidecar that could not be written. Python keeps `str(exc)`; here the side channels the
/// data contract asks for (DAN-2116 item 1) carry it.
#[derive(Debug)]
pub struct WriteFailure {
    pub path: PathBuf,
    error: std::io::Error,
}

impl WriteFailure {
    pub fn kind(&self) -> crate::error::ErrorKind {
        crate::error::ErrorKind::Io
    }

    /// What failed and what to do, with no path and no OS text.
    pub fn user_message(&self) -> String {
        "The tag file could not be written. Check that its folder is writable and has room, then try again."
            .to_owned()
    }

    /// Python's `str(OSError)`: `[Errno 13] Permission denied: '/x/y.txt'`. For the log only.
    pub fn detail(&self) -> String {
        let path = self.path.display();
        match self.error.raw_os_error() {
            Some(code) => {
                let full = std::io::Error::from_raw_os_error(code).to_string();
                let text = full.split(" (os error").next().unwrap_or(&full);
                format!("[Errno {code}] {text}: '{path}'")
            }
            None => format!("{}: '{path}'", self.error),
        }
    }
}

/// `_write_new`: creates `target` only if it does not exist (`O_EXCL`, so a file made in the gap
/// since the plan is not clobbered). `false` if it was there.
fn write_new(target: &Path, body: &str) -> std::io::Result<bool> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(target)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    file.write_all(body.as_bytes())?;
    Ok(true)
}

/// `_write_over`: replaces `target` atomically (a temp file in the same directory, renamed over
/// it), so an error part-way leaves the old sidecar intact.
fn write_over(target: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = target.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(body.as_bytes())?;
        drop(file);
        // The temp file is made 0600; the sidecar is 0644.
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o644))?;
        std::fs::rename(&temp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// `write`: writes the planned sidecars and reports what happened to each. `overwrite == false`
/// leaves an existing file exactly as it was and reports it. One unwritable file does not abandon
/// the rest.
pub fn write(sidecars: &[Planned<'_>], overwrite: bool) -> WriteResult {
    let mut result = WriteResult::default();
    for item in sidecars {
        let outcome = if overwrite {
            write_over(&item.path, &item.body).map(|()| true)
        } else {
            write_new(&item.path, &item.body)
        };
        match outcome {
            Ok(true) => result.written.push(item.path.clone()),
            Ok(false) => result.skipped_existing.push(item.path.clone()),
            Err(error) => {
                let failure = WriteFailure {
                    path: item.path.clone(),
                    error,
                };
                tracing::error!(
                    "Could not write tag sidecar {}: {}",
                    item.path.display(),
                    failure.detail()
                );
                result.failures.push(failure);
            }
        }
    }
    if !result.written.is_empty() {
        tracing::info!("Wrote {} tag sidecar file(s)", result.written.len());
    }
    result
}

/// `_nothing_to_write_text` (`gui/main_window.py:353`): why Write Tag Files has nothing to do.
pub fn nothing_to_write_text(plan: &Plan<'_>) -> String {
    let (no_tags, missing) = (plan.without_tags.len(), plan.missing_files.len());
    if no_tags > 0 && missing > 0 {
        format!(
            "Nothing to write: {no_tags} image(s) have no tags yet and {missing} are missing from disk."
        )
    } else if missing > 0 {
        format!(
            "Nothing to write: all {missing} of those files are missing from disk, so there's nowhere to put a tag file."
        )
    } else {
        "Nothing to write: none of those images have any tags yet.\n\nSearch them, or add tags by hand, and try again."
            .to_owned()
    }
}

/// `_written_summary` (`gui/main_window.py:370`): the status-bar line after a write.
pub fn written_summary(plan: &Plan<'_>, result: &WriteResult) -> String {
    let mut parts = vec![format!("Wrote {} tag file(s)", result.written.len())];
    if !result.skipped_existing.is_empty() {
        parts.push(format!(
            "{} already existed and were kept",
            result.skipped_existing.len()
        ));
    }
    if !plan.without_tags.is_empty() {
        parts.push(format!("{} had no tags", plan.without_tags.len()));
    }
    if !plan.missing_files.is_empty() {
        parts.push(format!("{} missing from disk", plan.missing_files.len()));
    }
    if !result.failures.is_empty() {
        parts.push(format!("{} could not be written", result.failures.len()));
    }
    parts.join(", ")
}

/// How `_ask_about_writing_tag_files` puts the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// A Yes/No question; Yes means "do not overwrite" (`False`) when `yes_overwrites` is false and
    /// "overwrite" (`True`) when it is true. No cancels.
    YesNo { yes_overwrites: bool },
    /// The `ask` policy with files in the way: Keep existing files (the default), Replace them,
    /// Cancel, with this informative text under the body.
    ThreeWay { informative: &'static str },
}

/// The text and shape of `_ask_about_writing_tag_files` (`gui/main_window.py:1757`) for a plan
/// that has writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    pub text: String,
    pub question: Question,
}

/// The informative text of the three-way box.
pub const ASK_INFORMATIVE: &str = "This app cannot tell a tag file it wrote on an earlier run from a text file you wrote yourself.";

/// `_ask_about_writing_tag_files`: what to say and how to ask. `plan.writes` must not be empty.
pub fn confirmation(plan: &Plan<'_>, policy: &str) -> Confirmation {
    let count = plan.writes.len();
    let folders = plan.directories();
    let where_ = if folders.len() == 1 {
        folders[0].display().to_string()
    } else {
        format!(
            "{} folders, the first being {}",
            folders.len(),
            folders[0].display()
        )
    };
    let named = plan.writes[0]
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut detail = vec![
        format!(
            "Write {count} tag file(s), one per image, each holding that image's tags one per line?"
        ),
        String::new(),
        format!("Into: {where_}"),
        format!("Named like: {named}"),
    ];
    for (skipped, why) in [
        (plan.without_tags.len(), "have no tags to write"),
        (plan.missing_files.len(), "are missing from disk"),
    ] {
        if skipped > 0 {
            detail.push(format!("Skipping {skipped} image(s) that {why}."));
        }
    }
    let existing = plan.existing();
    if existing.is_empty() {
        detail.push(String::new());
        detail.push("No text file of yours is in the way.".to_owned());
        return Confirmation {
            text: detail.join("\n"),
            question: Question::YesNo {
                yes_overwrites: false,
            },
        };
    }
    detail.push(String::new());
    detail.push(format!(
        "{} of them already exist, for example:",
        existing.len()
    ));
    detail.push(existing[0].path.display().to_string());
    match policy {
        OVERWRITE_SKIP => {
            detail.push(String::new());
            detail.push(
                "Those will be left exactly as they are (Settings > General: existing tag files)."
                    .to_owned(),
            );
            Confirmation {
                text: detail.join("\n"),
                question: Question::YesNo {
                    yes_overwrites: false,
                },
            }
        }
        OVERWRITE_OVERWRITE => {
            detail.push(String::new());
            detail.push("Those will be REPLACED, and their current contents lost (Settings > General: existing tag files).".to_owned());
            Confirmation {
                text: detail.join("\n"),
                question: Question::YesNo {
                    yes_overwrites: true,
                },
            }
        }
        _ => Confirmation {
            text: detail.join("\n"),
            question: Question::ThreeWay {
                informative: ASK_INFORMATIVE,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Tag, TagSource};

    fn test_settings() -> crate::config::Settings {
        let home = std::ffi::OsString::from("/nonexistent");
        let paths = crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| home.clone()))
            .expect("HOME is set");
        crate::config::Settings::with_defaults(&paths)
    }

    fn path(p: &str, style: &str) -> Option<String> {
        sidecar_path(p, style, DEFAULT_EXTENSION).map(|p| p.to_string_lossy().into_owned())
    }

    fn tagged(path: &str, tags: &[&str]) -> Entry {
        let mut e = Entry::new(path);
        e.tags = tags
            .iter()
            .map(|t| Tag {
                name: (*t).to_owned(),
                namespace: None,
                source: TagSource::User,
            })
            .collect();
        e
    }

    #[test]
    fn append_adds_the_extension_and_replace_swaps_the_suffix() {
        assert_eq!(path("/p/cat.jpg", STYLE_APPEND).unwrap(), "/p/cat.jpg.txt");
        assert_eq!(path("/p/cat.jpg", STYLE_REPLACE).unwrap(), "/p/cat.txt");
        assert_eq!(path("/p/a.tar.gz", STYLE_REPLACE).unwrap(), "/p/a.tar.txt");
        assert_eq!(path("/p/cat", STYLE_REPLACE).unwrap(), "/p/cat.txt");
        assert_eq!(path("/p/.hidden", STYLE_REPLACE).unwrap(), "/p/.hidden.txt");
        assert_eq!(path("/p/..", STYLE_REPLACE).unwrap(), "/p/...txt");
    }

    #[test]
    fn the_path_is_normalised_the_way_pathlib_does() {
        assert_eq!(path("a//b/./c.jpg", STYLE_APPEND).unwrap(), "a/b/c.jpg.txt");
        assert_eq!(path("//a/b.png", STYLE_APPEND).unwrap(), "//a/b.png.txt");
        assert_eq!(path("///a/b.png", STYLE_APPEND).unwrap(), "/a/b.png.txt");
        assert_eq!(path("a/b.jpg/", STYLE_REPLACE).unwrap(), "a/b.txt");
        assert_eq!(path("cat.", STYLE_REPLACE).unwrap(), "cat.txt");
        assert_eq!(path("cat.", STYLE_APPEND).unwrap(), "cat..txt");
    }

    #[test]
    fn a_path_with_no_name_has_no_sidecar() {
        for p in ["", ".", "/", "./"] {
            assert_eq!(path(p, STYLE_APPEND), None, "{p:?}");
        }
    }

    #[test]
    fn an_unrecognised_setting_falls_back_to_the_default() {
        let mut s = test_settings();
        s.sidecar_filename_style = "sideways".to_owned();
        s.sidecar_overwrite = "yes".to_owned();
        assert_eq!(style_of(Some(&s)), STYLE_APPEND);
        assert_eq!(overwrite_policy_of(Some(&s)), OVERWRITE_ASK);
        assert_eq!(style_of(None), STYLE_APPEND);
        s.sidecar_filename_style = STYLE_REPLACE.to_owned();
        s.sidecar_overwrite = OVERWRITE_OVERWRITE.to_owned();
        assert_eq!(style_of(Some(&s)), STYLE_REPLACE);
        assert_eq!(overwrite_policy_of(Some(&s)), OVERWRITE_OVERWRITE);
    }

    #[test]
    fn the_body_is_one_deduplicated_tag_per_line_in_the_entrys_order() {
        let e = tagged("/p/a.png", &["b", "a", "b", "", "c"]);
        assert_eq!(render(&e, None), "b\na\nc\n");
        assert_eq!(render(&tagged("/p/a.png", &[]), None), "");
    }

    #[test]
    fn the_plan_separates_writes_from_the_three_reasons_not_to() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        std::fs::write(dir.path().join("have.png.txt"), "old").unwrap();
        let mut missing = tagged(&format!("{d}/gone.png"), &["x"]);
        missing.file_missing = true;
        let entries = vec![
            tagged(&format!("{d}/new.png"), &["x"]),
            tagged(&format!("{d}/have.png"), &["y"]),
            tagged(&format!("{d}/bare.png"), &[]),
            missing,
        ];
        let plan = plan(&entries, None, DEFAULT_EXTENSION);
        assert_eq!(plan.writes.len(), 2);
        assert!(!plan.writes[0].exists);
        assert!(plan.writes[1].exists);
        assert_eq!(plan.existing().len(), 1);
        assert_eq!(plan.without_tags.len(), 1);
        assert_eq!(plan.missing_files.len(), 1);
        assert_eq!(plan.directories(), vec![std::path::PathBuf::from(d)]);
    }

    #[test]
    fn the_body_uses_display_names_and_the_same_filter_as_a_send() {
        let mut e = Entry::new("/p/a.png");
        e.tags = vec![
            Tag {
                name: "sam".to_owned(),
                namespace: Some("creator".to_owned()),
                source: TagSource::Booru,
            },
            Tag {
                name: "solo".to_owned(),
                namespace: None,
                source: TagSource::User,
            },
        ];
        assert_eq!(render(&e, None), "creator:sam\nsolo\n");
        let mut s = test_settings();
        s.filter_sent_tags_by_source = true;
        s.enabled_tag_sources = vec!["User".to_owned()];
        assert_eq!(render(&e, Some(&s)), "solo\n");
    }

    #[test]
    fn planning_writes_nothing_and_the_shipped_defaults_are_the_safe_ones() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        let e = tagged(&format!("{d}/a.png"), &["x"]);
        let mut s = test_settings();
        assert_eq!(style_of(Some(&s)), STYLE_APPEND);
        assert_eq!(overwrite_policy_of(Some(&s)), OVERWRITE_ASK);
        s.sidecar_filename_style = STYLE_REPLACE.to_owned();
        let planned = plan([&e], Some(&s), DEFAULT_EXTENSION);
        assert_eq!(
            planned.writes[0].path,
            std::path::PathBuf::from(format!("{d}/a.txt"))
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn on_disk(dir: &Path, name: &str, tags: &[&str]) -> Entry {
        tagged(&dir.join(name).to_string_lossy(), tags)
    }

    #[test]
    fn write_never_clobbers_unless_told_to_and_replaces_atomically() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let a = on_disk(dir.path(), "a.png", &["cat"]);
        let b = on_disk(dir.path(), "b.png", &["dog", "red"]);
        std::fs::write(dir.path().join("a.png.txt"), "mine\n").unwrap();
        let settings = test_settings();
        let entries = [a, b];
        let plan = plan(entries.iter(), Some(&settings), DEFAULT_EXTENSION);
        assert_eq!(plan.existing().len(), 1);

        let kept = write(&plan.writes, false);
        assert_eq!(kept.written, vec![dir.path().join("b.png.txt")]);
        assert_eq!(kept.skipped_existing, vec![dir.path().join("a.png.txt")]);
        assert!(kept.failures.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
            "mine\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.png.txt")).unwrap(),
            "dog\nred\n"
        );
        let mode = std::fs::metadata(dir.path().join("b.png.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777 & !0o022, 0o644 & !0o022);

        let replaced = write(&plan.writes, true);
        assert_eq!(replaced.written.len(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
            "cat\n"
        );
        let mode = std::fs::metadata(dir.path().join("a.png.txt"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o644,
            "the temp file's 0600 does not survive the rename"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn one_unwritable_folder_does_not_abandon_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let ok = on_disk(dir.path(), "ok.png", &["cat"]);
        let gone = on_disk(&dir.path().join("no-such-folder"), "x.png", &["dog"]);
        let settings = test_settings();
        let entries = [gone, ok];
        let plan = plan(entries.iter(), Some(&settings), DEFAULT_EXTENSION);
        for overwrite in [false, true] {
            let result = write(&plan.writes, overwrite);
            assert_eq!(result.failures.len(), 1, "overwrite={overwrite}");
            assert_eq!(result.written, vec![dir.path().join("ok.png.txt")]);
            let f = &result.failures[0];
            assert_eq!(f.kind(), crate::error::ErrorKind::Io);
            let shown = f.user_message();
            assert!(
                !shown.contains("no-such-folder") && !shown.contains("os error"),
                "{shown}"
            );
            assert!(
                f.detail()
                    .starts_with("[Errno 2] No such file or directory: '"),
                "{}",
                f.detail()
            );
        }
    }

    #[test]
    fn the_texts_around_a_write_are_pythons() {
        let dir = tempfile::tempdir().unwrap();
        let mut missing = on_disk(dir.path(), "m.png", &["x"]);
        missing.file_missing = true;
        let bare = on_disk(dir.path(), "n.png", &[]);
        let settings = test_settings();
        let entries = [missing, bare];
        let none = plan(entries.iter(), Some(&settings), DEFAULT_EXTENSION);
        assert_eq!(
            nothing_to_write_text(&none),
            "Nothing to write: 1 image(s) have no tags yet and 1 are missing from disk."
        );
        let only_bare = plan(entries[1..].iter(), Some(&settings), DEFAULT_EXTENSION);
        assert!(nothing_to_write_text(&only_bare).ends_with("and try again."));
        let only_missing = plan(entries[..1].iter(), Some(&settings), DEFAULT_EXTENSION);
        assert!(
            nothing_to_write_text(&only_missing).starts_with("Nothing to write: all 1 of those")
        );

        let ok = on_disk(dir.path(), "a.png", &["cat"]);
        std::fs::write(dir.path().join("a.png.txt"), "x").unwrap();
        let list = [ok, entries[0].clone(), entries[1].clone()];
        let p = plan(list.iter(), Some(&settings), DEFAULT_EXTENSION);
        let d = dir.path().display();
        let ask = confirmation(&p, OVERWRITE_ASK);
        assert_eq!(
            ask.text,
            format!(
                "Write 1 tag file(s), one per image, each holding that image's tags one per line?\n\nInto: {d}\nNamed like: a.png.txt\nSkipping 1 image(s) that have no tags to write.\nSkipping 1 image(s) that are missing from disk.\n\n1 of them already exist, for example:\n{d}/a.png.txt"
            )
        );
        assert_eq!(
            ask.question,
            Question::ThreeWay {
                informative: ASK_INFORMATIVE
            }
        );
        assert_eq!(
            confirmation(&p, OVERWRITE_SKIP).question,
            Question::YesNo {
                yes_overwrites: false
            }
        );
        let over = confirmation(&p, OVERWRITE_OVERWRITE);
        assert_eq!(
            over.question,
            Question::YesNo {
                yes_overwrites: true
            }
        );
        assert!(over.text.ends_with(
            "REPLACED, and their current contents lost (Settings > General: existing tag files)."
        ));
        std::fs::remove_file(dir.path().join("a.png.txt")).unwrap();
        let p = plan(list.iter(), Some(&settings), DEFAULT_EXTENSION);
        assert!(
            confirmation(&p, OVERWRITE_ASK)
                .text
                .ends_with("No text file of yours is in the way.")
        );
        let result = write(&p.writes, false);
        assert_eq!(
            written_summary(&p, &result),
            "Wrote 1 tag file(s), 1 had no tags, 1 missing from disk"
        );
    }
}
