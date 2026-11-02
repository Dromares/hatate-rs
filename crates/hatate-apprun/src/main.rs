//! `AppRun` of the Hatate AppImage (a Rust binary, never a script: CLAUDE.md).
//!
//! The AppImage runtime mounts the image, sets `APPDIR` and executes `$APPDIR/AppRun`. This binary
//! points the process at what the image carries (its own libraries, GLib schemas, Chromium and
//! fonts) and then `exec`s the window (`usr/bin/hatate-gtk`) or, for a command-line argument, the
//! command (`usr/bin/hatate`). Nothing is ever written inside `$APPDIR`: the mount is read-only.
//! It first copies the image to the user's cache and runs from the copy (`relocate.rs`), so the
//! app no longer depends on the mount staying up.
//!
//! The environment is worked out by `plan` (`plan.rs`), a function of the mount path, the arguments and
//! the caller's environment alone, so a test covers it without starting anything.

// The panic hook (DAN-2501). `hatate-core` is not a dependency of this crate (the launcher uses only
// `std`), so the one std-only source file is included instead of linked.
#[allow(dead_code)] // `arm` and the entry text are for the binaries that open a section
#[path = "../../hatate-core/src/crash_report.rs"]
mod crash_report;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

mod plan;
mod relocate;
use plan::{PIXBUF_LOADERS, fontconfig_text, plan};

/// Writes `contents` to a file named for this mount under the runtime directory (`ext` tells the
/// files apart); `None` when it cannot be written.
fn runtime_file(
    appdir: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    ext: &str,
    contents: &[u8],
) -> Option<PathBuf> {
    let base = env("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .or_else(|| env("TMPDIR").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let uid = fs::metadata("/proc/self").ok()?.uid();
    let dir = base.join(format!("hatate-fonts-{uid}"));
    fs::create_dir_all(&dir).ok()?;
    // FNV-1a of the mount path names the file, so two mounts never share one.
    let hash = appdir
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
    let path = dir.join(format!("{hash:016x}.{ext}"));
    let tmp = dir.join(format!("{hash:016x}.{ext}.{}", std::process::id()));
    fs::write(&tmp, contents).ok()?;
    if fs::rename(&tmp, &path).is_err() {
        let _ = fs::remove_file(&tmp);
        return None;
    }
    Some(path)
}

/// One config file per mount; `None` leaves the host's own fontconfig in charge.
fn write_fontconfig(appdir: &Path, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    runtime_file(appdir, env, "conf", fontconfig_text(appdir).as_bytes())
}

/// The gdk-pixbuf loader cache. The cache lists loaders by absolute path, and the mount path
/// changes on every run, so the bundled `gdk-pixbuf-query-loaders` writes it at launch. `None`
/// when the image carries no loaders; GTK then has no SVG loader and its symbolic icons are blank.
fn write_pixbuf_cache(
    appdir: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    set: &BTreeMap<String, OsString>,
) -> Option<PathBuf> {
    let query = appdir.join("usr/bin/gdk-pixbuf-query-loaders");
    let loaders = appdir.join(PIXBUF_LOADERS);
    if !query.is_file() || !loaders.is_dir() {
        return None;
    }
    let out = Command::new(&query)
        .env("GDK_PIXBUF_MODULEDIR", &loaders)
        .env("LD_LIBRARY_PATH", set.get("LD_LIBRARY_PATH")?)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    runtime_file(appdir, env, "pixbuf.cache", &out.stdout)
}

/// `$APPDIR` if the runtime set it, else the directory this binary sits in.
fn appdir_of(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = env("APPDIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let exe = fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    exe.parent().map(Path::to_path_buf)
}

fn main() {
    let env = |name: &str| std::env::var_os(name);
    if let Some(crash_log) = crash_report::crash_log_path(&env) {
        crash_report::install(crash_log, "AppRun", env!("CARGO_PKG_VERSION"));
    }
    let Some(appdir) = appdir_of(&env) else {
        eprintln!("AppRun: cannot tell where the AppImage is mounted");
        std::process::exit(1);
    };
    // Run from a copy in the user's cache, never from the FUSE mount (DAN-2564): see `relocate`.
    let mount = appdir;
    let (appdir, warning) = relocate::resolve(&mount, &env, env!("CARGO_PKG_VERSION"));
    if appdir != mount {
        relocate::close_fds_into(&mount);
    }
    if let Some(reason) = warning {
        eprintln!("AppRun: WARNING: {reason}; running from the mount");
    }
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let conf = write_fontconfig(&appdir, &env);
    let mut launch = plan(&appdir, &args, &env, conf.as_deref());
    if let Some(cache) = write_pixbuf_cache(&appdir, &env, &launch.set) {
        launch
            .set
            .insert("GDK_PIXBUF_MODULE_FILE".into(), cache.into());
        launch.set.insert(
            "GDK_PIXBUF_MODULEDIR".into(),
            appdir.join(PIXBUF_LOADERS).into(),
        );
    }
    let mut command = Command::new(&launch.program);
    command.args(&launch.args);
    for (name, value) in &launch.set {
        command.env(name, value);
    }
    // A host Python or Qt environment is no business of a Rust GTK app, but a stale GTK one is:
    // the image carries its own GTK, so the host's module and theme paths must not mix in.
    for stale in [
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        // The bundled libgio reads `GIO_MODULE_DIR` (set above) and nothing the host adds.
        "GIO_EXTRA_MODULES",
    ] {
        command.env_remove(stale);
    }
    let err = command.exec();
    eprintln!("AppRun: cannot start {}: {err}", launch.program.display());
    std::process::exit(127);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn none(_: &str) -> Option<OsString> {
        None
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_command_argument_runs_the_command_and_anything_else_the_window() {
        let appdir = Path::new("/mnt/hatate");
        for first in ["batch", "mcp-stdio-bridge", "--help", "--version", "-V"] {
            let launch = plan(appdir, &os(&[first, "x"]), &none, None);
            assert_eq!(launch.program, Path::new("/mnt/hatate/usr/bin/hatate"));
            assert_eq!(launch.args, os(&[first, "x"]));
        }
        for args in [
            &[][..],
            &["--gapplication-service"][..],
            &["/pics/a.png"][..],
        ] {
            let launch = plan(appdir, &os(args), &none, None);
            assert_eq!(launch.program, Path::new("/mnt/hatate/usr/bin/hatate-gtk"));
        }
    }

    #[test]
    fn the_environment_points_at_the_image_and_keeps_the_hosts_after_it() {
        let host = |name: &str| match name {
            "LD_LIBRARY_PATH" => Some(OsString::from("/opt/lib")),
            "XDG_DATA_DIRS" => Some(OsString::from("/home/u/.local/share:/usr/share")),
            _ => None,
        };
        let launch = plan(
            Path::new("/mnt/h"),
            &[],
            &host,
            Some(Path::new("/run/f.conf")),
        );
        let get = |k: &str| launch.set[k].to_str().unwrap().to_owned();
        assert_eq!(get("APPDIR"), "/mnt/h");
        assert_eq!(get("HATATE_APPIMAGE"), "1");
        assert_eq!(
            get("PLAYWRIGHT_BROWSERS_PATH"),
            "/mnt/h/usr/lib/ms-playwright"
        );
        assert_eq!(get("LD_LIBRARY_PATH"), "/mnt/h/usr/lib:/opt/lib");
        assert_eq!(
            get("XDG_DATA_DIRS"),
            "/mnt/h/usr/share:/home/u/.local/share:/usr/share"
        );
        assert_eq!(
            get("GSETTINGS_SCHEMA_DIR"),
            "/mnt/h/usr/share/glib-2.0/schemas"
        );
        assert_eq!(get("GIO_MODULE_DIR"), "/mnt/h/usr/lib/gio/modules");
        assert_eq!(get("FONTCONFIG_FILE"), "/run/f.conf");
    }

    #[test]
    fn with_no_host_values_the_lists_have_no_empty_entry() {
        let launch = plan(Path::new("/mnt/h"), &[], &none, None);
        // An empty `LD_LIBRARY_PATH` entry means the current directory to the loader.
        assert_eq!(launch.set["LD_LIBRARY_PATH"], OsStr::new("/mnt/h/usr/lib"));
        assert_eq!(
            launch.set["XDG_DATA_DIRS"],
            OsStr::new("/mnt/h/usr/share:/usr/local/share:/usr/share")
        );
        assert!(!launch.set.contains_key("FONTCONFIG_FILE"));
    }

    #[test]
    fn the_fontconfig_lists_the_bundled_fonts_by_absolute_path_and_escapes_it() {
        let text = fontconfig_text(Path::new("/tmp/.mount_a&b<c"));
        assert!(text.contains("<dir>/tmp/.mount_a&amp;b&lt;c/usr/share/fonts/hatate</dir>"));
        assert!(text.contains("<dir>/usr/share/fonts</dir>"));
    }

    #[test]
    fn the_fontconfig_is_written_once_per_mount_under_the_runtime_dir() {
        let run = tempfile::tempdir().unwrap();
        let run_path = run.path().to_owned();
        let env = move |name: &str| (name == "XDG_RUNTIME_DIR").then(|| run_path.clone().into());
        let a = write_fontconfig(Path::new("/mnt/a"), &env).unwrap();
        let b = write_fontconfig(Path::new("/mnt/b"), &env).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(run.path()));
        assert!(
            fs::read_to_string(&a)
                .unwrap()
                .contains("/mnt/a/usr/share/fonts/hatate")
        );
        assert_eq!(write_fontconfig(Path::new("/mnt/a"), &env).unwrap(), a);
    }
}
