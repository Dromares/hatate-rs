//! Which Chromium Lens can drive (`core/lens_browser.py`'s dependency half: `playwright_available`,
//! `_browsers_root`, `chromium_installed`, `check_dependencies`, `classify_launch_failure` and the
//! three messages).
//!
//! **What changes from Python (plan decision, DAN-2164).** The Rust app has no Playwright, so it
//! reports only [`LensMissing::Chromium`] as a missing dependency. It looks for a browser in this
//! order: the AppImage's bundled one, Playwright's existing `ms-playwright/chromium-*` download
//! (read exactly as Python reads it, so a current user needs no new download), then
//! `chromium`, `chromium-browser`, `google-chrome-stable` and `google-chrome` on `PATH`. The
//! messages are new copy because Python's name `pip`; `docs/lens-browser.md` has both texts side by
//! side.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::engines::lens_search::LensMissing;
use crate::pyrepr::py_repr;

/// The executables looked for on `PATH`, in order.
pub const PATH_BROWSERS: [&str; 4] = [
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
];

/// Where Playwright puts the executable inside a `chromium-<revision>` directory: the current
/// layout first, then the older one.
const EXECUTABLE_IN_DOWNLOAD: [&str; 2] = ["chrome-linux64/chrome", "chrome-linux/chrome"];

/// The process state `check_dependencies` reads. Read once, so a test (or the differential) gives
/// it a temp `HOME` and a `PATH` of its own instead of touching the process.
#[derive(Debug, Clone, Default)]
pub struct BrowserEnv {
    /// `PLAYWRIGHT_BROWSERS_PATH`.
    pub playwright_browsers_path: Option<OsString>,
    /// `XDG_CACHE_HOME`.
    pub xdg_cache_home: Option<OsString>,
    /// `Path.home()`.
    pub home: Option<PathBuf>,
    /// `PATH`.
    pub path: Option<OsString>,
    /// `running_as_appimage()`.
    pub appimage: bool,
}

impl BrowserEnv {
    pub fn from_process() -> Self {
        BrowserEnv {
            playwright_browsers_path: std::env::var_os("PLAYWRIGHT_BROWSERS_PATH"),
            xdg_cache_home: std::env::var_os("XDG_CACHE_HOME"),
            home: std::env::var_os("HOME").map(PathBuf::from),
            path: std::env::var_os("PATH"),
            // `paths.running_as_appimage()`: the AppImage's `AppRun` sets `HATATE_APPIMAGE=1`.
            // Phase 7 owns the AppImage itself.
            appimage: std::env::var_os("HATATE_APPIMAGE").is_some_and(|v| v == "1"),
        }
    }
}

/// Where a found browser came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserSource {
    /// The AppImage's own `ms-playwright` directory.
    Bundled,
    /// Playwright's download in the user's cache (or `PLAYWRIGHT_BROWSERS_PATH`).
    PlaywrightDownload,
    /// One of [`PATH_BROWSERS`].
    Path(&'static str),
}

/// A browser executable and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundBrowser {
    pub executable: PathBuf,
    pub source: BrowserSource,
}

/// `DependencyStatus`: what Google Lens is short of, if anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DependencyStatus {
    pub missing: Option<LensMissing>,
    pub message: String,
}

impl DependencyStatus {
    pub fn ok(&self) -> bool {
        self.missing.is_none()
    }
}

/// `_browsers_root`: where Playwright keeps its downloaded browsers, or `None` if unknown.
///
/// `PLAYWRIGHT_BROWSERS_PATH` wins when set to a directory; `"0"` means the browsers live inside
/// the installed package, a layout this does not read. With nothing set it is the per-user cache.
pub fn browsers_root(env: &BrowserEnv) -> Option<PathBuf> {
    match &env.playwright_browsers_path {
        Some(value) if value == "0" => return None,
        // Python: `if override:` — an empty value is the same as unset.
        Some(value) if !value.is_empty() => return Some(PathBuf::from(value)),
        _ => {}
    }
    // `sys.platform.startswith("linux")`: this build only runs there.
    let cache = match env.xdg_cache_home.as_ref().filter(|v| !v.is_empty()) {
        Some(cache) => PathBuf::from(cache),
        None => env.home.as_ref()?.join(".cache"),
    };
    Some(cache.join("ms-playwright"))
}

/// `chromium_installed`: whether a headed Chromium has been downloaded. `None` = cannot tell.
///
/// Three-valued on purpose: reporting a missing browser that is there would send the user to fix a
/// problem they do not have, so anything this cannot read for certain answers `None`.
/// `chromium_headless_shell-*` does not count: that build has no window, and Lens is headed.
pub fn chromium_installed(env: &BrowserEnv) -> Option<bool> {
    let Some(root) = browsers_root(env) else {
        return if env.appimage { Some(false) } else { None };
    };
    match fs::metadata(&root) {
        Ok(meta) if !meta.is_dir() => return Some(false),
        Ok(_) => {}
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Some(false);
        }
        Err(_) => return None,
    }
    let entries = fs::read_dir(&root).ok()?;
    for entry in entries {
        let Ok(entry) = entry else { return None };
        let is_download =
            entry.file_name().to_string_lossy().starts_with("chromium-") && entry.path().is_dir();
        if is_download {
            return Some(true);
        }
    }
    Some(false)
}

/// The executable inside the newest `chromium-<revision>` directory of `root` that has one.
fn playwright_executable(root: &Path) -> Option<PathBuf> {
    let mut downloads: Vec<(u64, PathBuf)> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let revision = name.strip_prefix("chromium-")?.parse::<u64>().ok()?;
            entry.path().is_dir().then(|| (revision, entry.path()))
        })
        .collect();
    downloads.sort_by_key(|(revision, _)| std::cmp::Reverse(*revision));
    downloads.into_iter().find_map(|(_, dir)| {
        EXECUTABLE_IN_DOWNLOAD
            .iter()
            .map(|relative| dir.join(relative))
            .find(|candidate| is_executable(candidate))
    })
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn on_path(env: &BrowserEnv, name: &str) -> Option<PathBuf> {
    std::env::split_paths(env.path.as_ref()?)
        // An empty `PATH` entry means the current directory to a shell; a browser is never
        // looked for there.
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// The browser Lens would launch, or `None` when there is none.
///
/// In the AppImage only the bundled browser counts: its `AppRun` points
/// `PLAYWRIGHT_BROWSERS_PATH` at it, and a missing one means the file is damaged, not that the
/// user should install something (`bundled_browser_message`).
pub fn find_chromium(env: &BrowserEnv) -> Option<FoundBrowser> {
    if let Some(root) = browsers_root(env)
        && let Some(executable) = playwright_executable(&root)
    {
        let source = if env.appimage {
            BrowserSource::Bundled
        } else {
            BrowserSource::PlaywrightDownload
        };
        return Some(FoundBrowser { executable, source });
    }
    if env.appimage {
        return None;
    }
    PATH_BROWSERS.iter().find_map(|name| {
        on_path(env, name).map(|executable| FoundBrowser {
            executable,
            source: BrowserSource::Path(name),
        })
    })
}

/// `bundled_browser_message`: Lens is part of the AppImage; if its browser is missing, the file
/// is damaged. Nothing to install.
pub fn bundled_browser_message(env: &BrowserEnv) -> String {
    let shown = env
        .playwright_browsers_path
        .as_ref()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "Google Lens ships inside this AppImage (Chromium), but the bundled browser could not be \
         found (PLAYWRIGHT_BROWSERS_PATH={}). This AppImage is damaged or incomplete. Download it \
         again and check it against SHA256SUMS; there is nothing to install separately.",
        py_repr(&Value::String(shown))
    )
}

/// `missing_chromium_message` (and, in this build, `missing_dependency_message`, since Playwright
/// is not a dependency here).
pub fn missing_chromium_message(env: &BrowserEnv) -> String {
    if env.appimage {
        return bundled_browser_message(env);
    }
    "Google Lens needs a Chromium-based browser and none was found. Install Chromium or Google \
     Chrome with your package manager, so that one of `chromium`, `chromium-browser`, \
     `google-chrome-stable` or `google-chrome` is on your PATH, then restart. Nothing else in this \
     app needs it."
        .to_owned()
}

/// `missing_dependency_message`: Python's names Playwright and `pip`; here there is one missing
/// thing, a browser.
pub fn missing_dependency_message(env: &BrowserEnv) -> String {
    missing_chromium_message(env)
}

/// `check_dependencies`: Lens's optional dependency, before a search is attempted. Cheap enough
/// for a settings checkbox: a directory listing and a `PATH` walk, no browser started.
pub fn check_dependencies(env: &BrowserEnv) -> DependencyStatus {
    if find_chromium(env).is_some() {
        return DependencyStatus::default();
    }
    DependencyStatus {
        missing: Some(LensMissing::Chromium),
        message: missing_chromium_message(env),
    }
}

/// How a launch failed, as far as `classify_launch_failure` and the profile check read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchFailure {
    /// The executable could not be started (`ENOENT`, `EACCES`, not a binary).
    Spawn {
        kind: std::io::ErrorKind,
        text: String,
    },
    /// Chromium exited before it said where its DevTools endpoint is.
    Exited { text: String },
    /// Chromium ran but never said where its DevTools endpoint is.
    NoEndpoint { text: String },
    /// The DevTools connection to a running Chromium failed.
    Connect { text: String },
}

impl LaunchFailure {
    /// `str(error)`: what a launch exception says.
    pub fn text(&self) -> &str {
        match self {
            LaunchFailure::Spawn { text, .. }
            | LaunchFailure::Exited { text }
            | LaunchFailure::NoEndpoint { text }
            | LaunchFailure::Connect { text } => text,
        }
    }
}

/// `classify_launch_failure`: reads a failed launch for the one cause that is the user's to fix.
///
/// Python reads Playwright's own "Executable doesn't exist" sentence. Here the same fact is an
/// executable that is not there (`ENOENT`) or cannot be run (`EACCES`, `ENOEXEC`): the browser
/// that was found has gone since it was found. `None` for every other failure: a profile already
/// in use or a window that would not open is not a missing dependency.
pub fn classify_launch_failure(
    failure: &LaunchFailure,
    env: &BrowserEnv,
) -> Option<DependencyStatus> {
    use std::io::ErrorKind::{NotFound, PermissionDenied};
    match failure {
        LaunchFailure::Spawn {
            kind: NotFound | PermissionDenied,
            ..
        } => Some(DependencyStatus {
            missing: Some(LensMissing::Chromium),
            message: missing_chromium_message(env),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    fn make_exe(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn env_in(home: &Path) -> BrowserEnv {
        BrowserEnv {
            home: Some(home.to_owned()),
            path: Some(OsString::new()),
            ..BrowserEnv::default()
        }
    }

    #[test]
    fn the_root_follows_the_override_then_the_cache() {
        let mut env = env_in(Path::new("/h"));
        assert_eq!(
            browsers_root(&env),
            Some(PathBuf::from("/h/.cache/ms-playwright"))
        );
        env.xdg_cache_home = Some("/x".into());
        assert_eq!(browsers_root(&env), Some(PathBuf::from("/x/ms-playwright")));
        env.playwright_browsers_path = Some("/b".into());
        assert_eq!(browsers_root(&env), Some(PathBuf::from("/b")));
        env.playwright_browsers_path = Some("0".into());
        assert_eq!(browsers_root(&env), None);
        // An empty override is the same as none.
        env.playwright_browsers_path = Some("".into());
        assert_eq!(browsers_root(&env), Some(PathBuf::from("/x/ms-playwright")));
    }

    #[test]
    fn chromium_installed_is_three_valued_and_ignores_the_headless_shell() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env_in(dir.path());
        env.playwright_browsers_path = Some(dir.path().join("pw").into());
        assert_eq!(chromium_installed(&env), Some(false), "no directory yet");
        fs::create_dir_all(dir.path().join("pw/chromium_headless_shell-1")).unwrap();
        assert_eq!(
            chromium_installed(&env),
            Some(false),
            "a headless shell is no window"
        );
        fs::create_dir_all(dir.path().join("pw/chromium-1243")).unwrap();
        assert_eq!(chromium_installed(&env), Some(true));
        env.playwright_browsers_path = Some("0".into());
        assert_eq!(chromium_installed(&env), None, "cannot tell");
        env.appimage = true;
        assert_eq!(chromium_installed(&env), Some(false), "a damaged AppImage");
    }

    #[test]
    fn the_playwright_download_is_found_by_its_newest_revision() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = env_in(dir.path());
        env.playwright_browsers_path = Some(dir.path().join("pw").into());
        make_exe(&dir.path().join("pw/chromium-900/chrome-linux/chrome"));
        make_exe(&dir.path().join("pw/chromium-1243/chrome-linux64/chrome"));
        // A newer directory with no executable in it is skipped, not preferred.
        fs::create_dir_all(dir.path().join("pw/chromium-2000")).unwrap();
        let found = find_chromium(&env).unwrap();
        assert_eq!(found.source, BrowserSource::PlaywrightDownload);
        assert_eq!(
            found.executable,
            dir.path().join("pw/chromium-1243/chrome-linux64/chrome")
        );
        env.appimage = true;
        assert_eq!(find_chromium(&env).unwrap().source, BrowserSource::Bundled);
    }

    #[test]
    fn path_names_are_tried_in_order_and_the_appimage_never_uses_them() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        make_exe(&bin.join("google-chrome"));
        let mut env = env_in(dir.path());
        env.path = Some(bin.clone().into());
        assert_eq!(
            find_chromium(&env).unwrap().source,
            BrowserSource::Path("google-chrome")
        );
        make_exe(&bin.join("chromium-browser"));
        assert_eq!(
            find_chromium(&env).unwrap().source,
            BrowserSource::Path("chromium-browser")
        );
        make_exe(&bin.join("chromium"));
        assert_eq!(
            find_chromium(&env).unwrap().source,
            BrowserSource::Path("chromium")
        );
        env.appimage = true;
        assert_eq!(find_chromium(&env), None);
        let status = check_dependencies(&env);
        assert_eq!(status.missing, Some(LensMissing::Chromium));
        assert!(status.message.contains("AppImage"));
    }

    #[test]
    fn nothing_found_reports_chromium_only_with_copy_that_names_no_pip() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_in(dir.path());
        let status = check_dependencies(&env);
        assert_eq!(status.missing, Some(LensMissing::Chromium));
        assert!(!status.message.contains("pip") && !status.message.contains("playwright"));
        assert!(status.message.contains("google-chrome-stable"));
    }

    #[test]
    fn only_an_executable_that_is_not_there_is_a_missing_dependency() {
        let env = env_in(Path::new("/h"));
        let spawn = |kind| LaunchFailure::Spawn {
            kind,
            text: "x".into(),
        };
        assert!(classify_launch_failure(&spawn(ErrorKind::NotFound), &env).is_some());
        assert!(classify_launch_failure(&spawn(ErrorKind::PermissionDenied), &env).is_some());
        assert!(classify_launch_failure(&spawn(ErrorKind::OutOfMemory), &env).is_none());
        for other in [
            LaunchFailure::Exited {
                text: "Opening in existing browser session.".into(),
            },
            LaunchFailure::NoEndpoint {
                text: "late".into(),
            },
            LaunchFailure::Connect {
                text: "refused".into(),
            },
        ] {
            assert_eq!(classify_launch_failure(&other, &env), None);
        }
    }
}
