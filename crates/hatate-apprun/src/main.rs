//! `AppRun` of the Hatate AppImage (a Rust binary, never a script: CLAUDE.md).
//!
//! The AppImage runtime mounts the image, sets `APPDIR` and executes `$APPDIR/AppRun`. This binary
//! points the process at what the image carries (its own libraries, GLib schemas, Chromium and
//! fonts) and then `exec`s the window (`usr/bin/hatate-gtk`) or, for a command-line argument, the
//! command (`usr/bin/hatate`). Nothing is ever written inside `$APPDIR`: the mount is read-only.
//!
//! The environment is worked out by [`plan`], a function of the mount path, the arguments and
//! the caller's environment alone, so a test covers it without starting anything.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// First arguments that belong to the command-line binary rather than the window.
const COMMAND_ARGS: [&str; 7] = [
    "batch",
    "mcp-stdio-bridge",
    "-h",
    "--help",
    "help",
    "-V",
    "--version",
];

/// Where the image keeps Chromium, relative to `$APPDIR`. Must equal
/// `hatate_core::lens_browser::deps::BUNDLED_BROWSERS`, which `hatate-core`'s test
/// `the_launcher_and_the_lookup_name_the_same_bundled_path` reads this source to check.
const BROWSERS: &str = "usr/lib/ms-playwright";

/// The gdk-pixbuf loaders, relative to `$APPDIR` (`xtask appimage` copies them here).
const PIXBUF_LOADERS: &str = "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders";

/// What to run and in which environment.
#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Variables to set, over the caller's environment.
    pub set: BTreeMap<String, OsString>,
}

fn prepend(first: &Path, existing: Option<&OsString>, default: &str) -> OsString {
    let mut out = OsString::from(first);
    out.push(":");
    match existing {
        Some(v) if !v.is_empty() => out.push(v),
        _ => out.push(default),
    }
    out
}

/// The program, arguments and environment for a start from `appdir`.
pub fn plan(
    appdir: &Path,
    args: &[OsString],
    env: &dyn Fn(&str) -> Option<OsString>,
    fontconfig: Option<&Path>,
) -> Launch {
    let cli = args
        .first()
        .and_then(|a| a.to_str())
        .is_some_and(|a| COMMAND_ARGS.contains(&a));
    let program = appdir
        .join("usr/bin")
        .join(if cli { "hatate" } else { "hatate-gtk" });
    let mut set: BTreeMap<String, OsString> = BTreeMap::new();
    set.insert("APPDIR".into(), appdir.into());
    // `paths.running_as_appimage()`: Lens then looks for the bundled browser and nothing else.
    set.insert("HATATE_APPIMAGE".into(), "1".into());
    set.insert(
        "PLAYWRIGHT_BROWSERS_PATH".into(),
        appdir.join(BROWSERS).into(),
    );
    // The bundled libraries first; whatever the host exports follows.
    set.insert(
        "LD_LIBRARY_PATH".into(),
        prepend(&appdir.join("usr/lib"), env("LD_LIBRARY_PATH").as_ref(), "")
            .to_string_lossy()
            .trim_end_matches(':')
            .into(),
    );
    set.insert(
        "XDG_DATA_DIRS".into(),
        prepend(
            &appdir.join("usr/share"),
            env("XDG_DATA_DIRS").as_ref(),
            "/usr/local/share:/usr/share",
        ),
    );
    set.insert(
        "GSETTINGS_SCHEMA_DIR".into(),
        appdir.join("usr/share/glib-2.0/schemas").into(),
    );
    if let Some(conf) = fontconfig {
        set.insert("FONTCONFIG_FILE".into(), conf.into());
    }
    Launch {
        program,
        args: args.to_vec(),
        set,
    }
}

/// The fontconfig file text: the image's fonts, the host's, then the user's. Written with the
/// absolute mount path because the image is read-only and a relative `<dir>` would be read
/// against the config file, which lives elsewhere.
pub fn fontconfig_text(appdir: &Path) -> String {
    let fonts = appdir.join("usr/share/fonts/hatate");
    let escaped = fonts
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n\
         <fontconfig>\n  \
         <dir>{escaped}</dir> <!-- bundled -->\n  \
         <dir>/usr/share/fonts</dir>\n  \
         <dir>/usr/local/share/fonts</dir>\n  \
         <dir prefix=\"xdg\">fonts</dir>\n  \
         <dir>~/.fonts</dir>\n  \
         <cachedir prefix=\"xdg\">fontconfig</cachedir>\n  \
         <alias><family>sans-serif</family><prefer><family>DejaVu Sans</family></prefer></alias>\n  \
         <alias><family>serif</family><prefer><family>DejaVu Serif</family></prefer></alias>\n  \
         <alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family></prefer></alias>\n\
         </fontconfig>\n"
    )
}

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
    let Some(appdir) = appdir_of(&env) else {
        eprintln!("AppRun: cannot tell where the AppImage is mounted");
        std::process::exit(1);
    };
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
