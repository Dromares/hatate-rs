//! Moves the image out of the FUSE mount before the app starts (DAN-2564).
//!
//! The AppImage runtime keeps the mount alive with a helper process that lives outside the app's
//! process group. On the board's desktop that helper ended while `hatate-gtk` was running, the
//! mount went with it, and the next page fault in a library that was not yet resident raised
//! SIGBUS. Nothing in the app signals that helper, and the sender was never found. So the app
//! stops depending on the mount: `AppRun` copies `$APPDIR` to the user's cache once per build
//! and starts the app from the copy, where no helper can pull the files away.
//!
//! `std` only, like the rest of this crate. Every function takes its paths explicitly, so a test
//! covers it in a temporary directory.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Directory under the cache root that holds the copies.
const CACHE_SUBDIR: &str = "hatate";
/// Prefix of a finished copy: `appdir-<version>-<id>`.
const COPY_PREFIX: &str = "appdir-";
/// Prefix of a copy being built: `tmp-appdir-<version>-<id>-<pid>`.
const TEMP_PREFIX: &str = "tmp-appdir-";
/// Written last inside a copy; a directory without it is never used.
const MARKER: &str = ".complete";
/// A sibling copy whose marker is younger than this is left alone: its launch may not have
/// reached `exec` yet.
const FRESH: Duration = Duration::from_secs(60);

/// `${XDG_CACHE_HOME:-$HOME/.cache}/hatate`. A relative `XDG_CACHE_HOME` is ignored, as the
/// XDG spec says.
pub fn cache_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let absolute = |name: &str| env(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let root = absolute("XDG_CACHE_HOME").or_else(|| absolute("HOME").map(|h| h.join(".cache")))?;
    Some(root.join(CACHE_SUBDIR))
}

/// Names one build of the image: FNV-1a (64 bit) over the relative path, size, mode and mtime of
/// every entry, as eight hex digits. A rebuilt image changes it; reading the image's bytes
/// would cost seconds on every launch.
pub fn build_id(appdir: &Path) -> io::Result<String> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash = (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    let mut stack = vec![appdir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&dir)?.collect::<io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let meta = fs::symlink_metadata(entry.path())?;
            let rel = entry.path();
            let rel = rel.strip_prefix(appdir).unwrap_or(&rel);
            feed(rel.as_os_str().as_encoded_bytes());
            feed(&meta.len().to_le_bytes());
            feed(&meta.mode().to_le_bytes());
            feed(&meta.mtime().to_le_bytes());
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(format!("{:08x}", (hash >> 32) ^ (hash & 0xffff_ffff)))
}

/// Copies the tree at `src` to `dst` (directories, regular files, symlinks; anything else is
/// skipped). File permissions are kept; a copy is never writable for others than its owner.
fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&from, &to)?;
        } else if kind.is_symlink() {
            symlink(fs::read_link(&from)?, &to)?;
        } else if kind.is_file() {
            fs::copy(&from, &to)?;
            // The mount is read-only, so its files are not writable; the cache copy must stay
            // deletable by the sweep (that needs the directory, not the file) and owner-writable
            // is harmless.
            let mode = fs::metadata(&from)?.permissions().mode() | 0o200;
            fs::set_permissions(&to, fs::Permissions::from_mode(mode & 0o755))?;
        }
    }
    Ok(())
}

/// True if `pid` is a live process (its `/proc` entry exists).
fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// True if a process of this user runs a binary from inside `dir`.
fn in_use(dir: &Path) -> bool {
    let Ok(procs) = fs::read_dir("/proc") else {
        return false;
    };
    procs
        .flatten()
        .filter(|p| {
            p.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .any(|p| fs::read_link(p.path().join("exe")).is_ok_and(|exe| exe.starts_with(dir)))
}

/// Removes the other `appdir-*` copies and the temporaries of dead builds. A copy that a running
/// process executes from, or whose marker is younger than `FRESH`, stays: the app loads
/// Chromium from its copy on demand, so unlinking under a running older version would break it.
fn sweep(cache: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix(TEMP_PREFIX) {
            let pid = rest.rsplit('-').next().and_then(|p| p.parse::<u32>().ok());
            if pid.is_some_and(|p| !alive(p)) {
                let _ = fs::remove_dir_all(&path);
            }
        } else if name.starts_with(COPY_PREFIX) {
            let fresh = fs::metadata(path.join(MARKER))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < FRESH);
            if !fresh && !in_use(&path) {
                let _ = fs::remove_dir_all(&path);
            }
        }
    }
}

/// The copy of `appdir` under `cache`, built if it is not there yet: in a temporary sibling,
/// with the marker written last, then renamed into place, so an interrupted copy is never used.
pub fn ensure_copy(appdir: &Path, cache: &Path, version: &str) -> io::Result<PathBuf> {
    let name = format!("{COPY_PREFIX}{version}-{}", build_id(appdir)?);
    let target = cache.join(&name);
    if target.join(MARKER).is_file() {
        sweep(cache, &target);
        return Ok(target);
    }
    fs::create_dir_all(cache)?;
    // The rename below is atomic, so a target without its marker is debris, not a launch in progress.
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    let temp = cache.join(format!(
        "{TEMP_PREFIX}{}-{}",
        &name[COPY_PREFIX.len()..],
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&temp);
    let built = copy_tree(appdir, &temp)
        .and_then(|()| fs::write(temp.join(MARKER), b""))
        .and_then(|()| match fs::rename(&temp, &target) {
            Ok(()) => Ok(()),
            // A second launch finished first: its copy is complete, use it.
            Err(_) if target.join(MARKER).is_file() => Ok(()),
            Err(err) => Err(err),
        });
    if let Err(err) = built {
        let _ = fs::remove_dir_all(&temp);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&temp);
    sweep(cache, &target);
    Ok(target)
}

/// Closes every inherited descriptor that points into `appdir`. The runtime leaves a directory
/// descriptor on the mount open (fd 1023 on the board's desktop); the app has no use for it, and
/// while it is open the process still holds the mount. Returns how many it closed.
pub fn close_fds_into(appdir: &Path) -> usize {
    use std::os::fd::{FromRawFd, OwnedFd};
    let Ok(dir) = fs::read_dir("/proc/self/fd") else {
        return 0;
    };
    // Collected first: closing while `read_dir` iterates would disturb the listing.
    let fds: Vec<i32> = dir
        .flatten()
        .filter_map(|e| {
            let fd = e.file_name().to_str()?.parse::<i32>().ok()?;
            let target = fs::read_link(e.path()).ok()?;
            (fd > 2 && target.starts_with(appdir)).then_some(fd)
        })
        .collect();
    for fd in &fds {
        // SAFETY: the descriptor was inherited, nothing in this process owns it, and dropping the
        // `OwnedFd` closes it once.
        drop(unsafe { OwnedFd::from_raw_fd(*fd) });
    }
    fds.len()
}

/// What `AppRun` runs from: the cache copy, or `appdir` itself with the reason when the copy
/// cannot be made. It never fails to start.
pub fn resolve(
    appdir: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    version: &str,
) -> (PathBuf, Option<String>) {
    let Some(cache) = cache_dir(env) else {
        return (
            appdir.to_path_buf(),
            Some("neither XDG_CACHE_HOME nor HOME is an absolute path".into()),
        );
    };
    match ensure_copy(appdir, &cache, version) {
        Ok(copy) => (copy, None),
        Err(err) => (
            appdir.to_path_buf(),
            Some(format!(
                "cannot copy the image to {}: {err}",
                cache.display()
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PIXBUF_LOADERS, fontconfig_text, plan};

    /// A small image: a launcher target, a library, a data file and a symlink.
    fn fake_image(root: &Path) -> PathBuf {
        let appdir = root.join(".mount_Hatate1234");
        fs::create_dir_all(appdir.join("usr/bin")).unwrap();
        fs::create_dir_all(appdir.join("usr/lib")).unwrap();
        fs::create_dir_all(appdir.join(PIXBUF_LOADERS)).unwrap();
        fs::write(appdir.join("usr/bin/hatate-gtk"), b"#!gtk").unwrap();
        fs::set_permissions(
            appdir.join("usr/bin/hatate-gtk"),
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        fs::write(appdir.join("usr/lib/libx.so.1.2"), b"lib").unwrap();
        symlink("libx.so.1.2", appdir.join("usr/lib/libx.so.1")).unwrap();
        appdir
    }

    fn env_of(cache: &Path) -> impl Fn(&str) -> Option<OsString> + use<> {
        let cache = cache.to_owned();
        move |name| (name == "XDG_CACHE_HOME").then(|| cache.clone().into())
    }

    /// Every path the launch hands the app, as strings: the program, each variable value.
    fn launch_paths(appdir: &Path, conf: Option<&Path>) -> Vec<String> {
        let launch = plan(appdir, &[], &|_| None, conf);
        let mut out = vec![launch.program.to_string_lossy().into_owned()];
        out.extend(
            launch
                .set
                .values()
                .map(|v| v.to_string_lossy().into_owned()),
        );
        out
    }

    /// The DAN-2564 pin. The app must start from a directory that is not the mount, and nothing it
    /// is handed (program, `APPDIR`, `LD_LIBRARY_PATH`, `GIO_MODULE_DIR`, `GSETTINGS_SCHEMA_DIR`,
    /// `XDG_DATA_DIRS`, `PLAYWRIGHT_BROWSERS_PATH`, the fontconfig text) may name the mount.
    /// Reverting `resolve` to return `appdir` turns it red.
    #[test]
    fn the_app_starts_from_a_copy_and_nothing_it_is_given_names_the_mount() {
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let cache = tmp.path().join("cache");
        let (root, warning) = resolve(&appdir, &env_of(&cache), "0.2.0");
        assert_eq!(warning, None);
        assert!(root.starts_with(cache.join("hatate")), "{root:?}");
        assert!(!root.starts_with(&appdir), "{root:?}");
        let mount = appdir.to_string_lossy().into_owned();
        for value in launch_paths(&root, None) {
            assert!(!value.contains(&mount), "{value} names the mount");
        }
        assert!(!fontconfig_text(&root).contains(&mount));
        // The exec target exists in the copy, and so do the library and its symlink.
        assert_eq!(fs::read(root.join("usr/bin/hatate-gtk")).unwrap(), b"#!gtk");
        assert_eq!(
            fs::read_link(root.join("usr/lib/libx.so.1")).unwrap(),
            Path::new("libx.so.1.2")
        );
        assert!(root.join(PIXBUF_LOADERS).is_dir());
    }

    #[test]
    fn descriptors_into_the_mount_are_closed_and_others_are_kept() {
        use std::os::fd::{AsRawFd, IntoRawFd};
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let elsewhere = tmp.path().join("elsewhere");
        fs::write(&elsewhere, b"x").unwrap();
        let held = fs::File::open(appdir.join("usr/bin/hatate-gtk"))
            .unwrap()
            .into_raw_fd();
        let dir = fs::File::open(&appdir).unwrap().into_raw_fd();
        let kept = fs::File::open(&elsewhere).unwrap();
        // Other tests open files too, so a closed number may be reused: ask where it points.
        let into_mount = |fd: i32| {
            fs::read_link(format!("/proc/self/fd/{fd}")).is_ok_and(|t| t.starts_with(&appdir))
        };
        assert!(into_mount(held) && into_mount(dir));
        assert!(close_fds_into(&appdir) >= 2);
        assert!(!into_mount(held) && !into_mount(dir));
        assert!(fs::read_link(format!("/proc/self/fd/{}", kept.as_raw_fd())).is_ok());
    }

    #[test]
    fn a_second_launch_reuses_the_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let env = env_of(&tmp.path().join("cache"));
        let (first, _) = resolve(&appdir, &env, "0.2.0");
        // A file added to the copy survives only if the second launch does not rebuild it.
        fs::write(first.join("usr/lib/sentinel"), b"x").unwrap();
        let (second, _) = resolve(&appdir, &env, "0.2.0");
        assert_eq!(first, second);
        assert!(second.join("usr/lib/sentinel").is_file());
    }

    #[test]
    fn a_rebuilt_image_gets_its_own_copy_and_the_old_one_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let env = env_of(&tmp.path().join("cache"));
        let (old, _) = resolve(&appdir, &env, "0.2.0");
        // Age the old marker past the freshness guard.
        let marker = fs::File::options()
            .write(true)
            .open(old.join(MARKER))
            .unwrap();
        marker
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        fs::write(appdir.join("usr/lib/libx.so.1.2"), b"a longer rebuilt lib").unwrap();
        let (new, _) = resolve(&appdir, &env, "0.2.0");
        assert_ne!(old, new);
        assert!(!old.exists(), "the old copy should be swept");
        assert!(new.join(MARKER).is_file());
    }

    #[test]
    fn a_half_built_copy_is_never_used_and_a_dead_temporary_is_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let cache = tmp.path().join("cache");
        let env = env_of(&cache);
        // The target name exists but has no marker (debris; never produced by the rename).
        let id = build_id(&appdir).unwrap();
        let half = cache.join("hatate").join(format!("appdir-0.2.0-{id}"));
        fs::create_dir_all(half.join("usr/bin")).unwrap();
        let dead = cache.join("hatate/tmp-appdir-0.2.0-deadbeef-4000000000");
        fs::create_dir_all(&dead).unwrap();
        let (root, warning) = resolve(&appdir, &env, "0.2.0");
        assert_eq!(warning, None);
        assert_eq!(root, half);
        assert!(
            root.join(MARKER).is_file(),
            "the debris is rebuilt, not used"
        );
        assert!(root.join("usr/bin/hatate-gtk").is_file());
        assert!(!dead.exists());
    }

    #[test]
    fn an_unwritable_cache_falls_back_to_the_mount_with_a_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let appdir = fake_image(tmp.path());
        let cache = tmp.path().join("cache");
        fs::create_dir(&cache).unwrap();
        fs::set_permissions(&cache, fs::Permissions::from_mode(0o500)).unwrap();
        let (root, warning) = resolve(&appdir, &env_of(&cache), "0.2.0");
        fs::set_permissions(&cache, fs::Permissions::from_mode(0o700)).unwrap();
        if fs::create_dir(cache.join("probe")).is_ok() {
            return; // root ignores the mode, so there is nothing to observe
        }
        assert_eq!(root, appdir);
        assert!(warning.unwrap().contains("cannot copy the image"));
    }

    #[test]
    fn the_cache_dir_follows_xdg_then_home_and_ignores_relative_values() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        let both = env(&[("XDG_CACHE_HOME", "/c"), ("HOME", "/h")]);
        assert_eq!(cache_dir(&both), Some(PathBuf::from("/c/hatate")));
        let home = env(&[("HOME", "/h")]);
        assert_eq!(cache_dir(&home), Some(PathBuf::from("/h/.cache/hatate")));
        let relative = env(&[("XDG_CACHE_HOME", "c"), ("HOME", "/h")]);
        assert_eq!(
            cache_dir(&relative),
            Some(PathBuf::from("/h/.cache/hatate"))
        );
        assert_eq!(cache_dir(&env(&[])), None);
    }
}
