//! Tag sidecar files, the planning half (`core/sidecar.py`): the tag list as a text file next to
//! the image, one tag per line.
//!
//! [`plan`] touches no disk except to ask whether each target already exists, and returns exactly
//! what would be written and where, so the window can show it (and the count of files it would
//! clobber) and get an answer first. `write`, `_write_new` and `_write_over` are not here: the
//! action that writes them is Phase 6.d's.
//!
//! The file name follows `pathlib` as Python 3.14 implements it (`Path.with_suffix`,
//! `Path.with_name`): `cat.jpg` is `cat.jpg.txt` (append) or `cat.txt` (replace), a path is first
//! normalised (`a//b/./c.jpg` is `a/b/c.jpg`), and a name that is only dots has no suffix.
//!
//! Which tags are written is [`tags_to_send`], the single answer to what leaves the app.

use std::collections::HashSet;
use std::path::PathBuf;

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
}
