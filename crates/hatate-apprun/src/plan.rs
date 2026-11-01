//! What `AppRun` runs and in which environment: a function of the mount path, the arguments and the
//! caller's environment alone, `std` only. A separate file so that `xtask` includes it too (`#[path]`,
//! the way `main.rs` includes the panic hook) and the pins on the image's environment run against
//! the real thing, not a copy.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// First arguments that belong to the command-line binary rather than the window.
pub const COMMAND_ARGS: [&str; 7] = [
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
pub const BROWSERS: &str = "usr/lib/ms-playwright";

/// Where the image keeps the GIO modules, relative to `$APPDIR` (`xtask appimage` copies
/// `libdconfsettings.so` here). Without a module directory the bundled libgio looks in the compiled-in
/// one, the build host's `/usr/lib/x86_64-linux-gnu/gio/modules`, which does not exist on most
/// machines: GSettings then falls back to its keyfile backend and returns schema defaults, never
/// the desktop's settings (DAN-2558).
pub const GIO_MODULES: &str = "usr/lib/gio/modules";

/// The gdk-pixbuf loaders, relative to `$APPDIR` (`xtask appimage` copies them here).
pub const PIXBUF_LOADERS: &str = "usr/lib/gdk-pixbuf-2.0/2.10.0/loaders";

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
    // `GIO_MODULE_DIR` replaces the compiled-in directory, so a host module built against another
    // glib is never loaded into the bundled libgio (`GIO_EXTRA_MODULES` would add to it).
    set.insert("GIO_MODULE_DIR".into(), appdir.join(GIO_MODULES).into());
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
