//! Where Hatate keeps its files: `docs/file-formats.md` section 1.
//!
//! The Python app puts configuration, state and data all under one directory,
//! `CONFIG_DIR`, and a cache under `CACHE_DIR`. This module resolves the same two
//! directories the same way (spec 1.1, 1.5; rule 6.1.1) and names every file in them
//! (spec 1.2, 1.3), so a profile written by either app is found by the other.
//!
//! Nothing here reads the process environment except [`Paths::from_env`]; every other
//! entry point takes a lookup function, so a test can resolve against a temp directory
//! without touching (or racing on) the real environment.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The directory name under `$XDG_CONFIG_HOME` / `$XDG_CACHE_HOME` (`core/paths.py:30,43`).
pub const APP_DIR_NAME: &str = "hatate-linux";

/// Name of the Rust-only settings file (DAN-1979, rule 6.1.4). It lives in `CONFIG_DIR`
/// next to `config.json`, and nothing Rust-only is ever written to `config.json`.
/// Section 1.2 has no file of this name (a test checks the whole list).
pub const RUST_SETTINGS_FILE_NAME: &str = "hatate-rs.json";

/// Name of the Rust app's own log (DAN-1982, rule 6.4). It sits in `CONFIG_DIR` next to
/// `app.log`, rotated the same way (`.1` `.2` `.3`), and the Rust app never writes
/// `app.log` itself. Section 1.2 has no file of this name (a test checks the whole list).
pub const RUST_LOG_FILE_NAME: &str = "hatate-rs.log";

/// No home directory could be found, and a path under it was needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoHomeDir;

impl fmt::Display for NoHomeDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "cannot determine the home directory: $HOME is unset and the passwd entry has none",
        )
    }
}

impl std::error::Error for NoHomeDir {}

/// The resolved profile locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    config_dir: PathBuf,
    cache_dir: PathBuf,
}

impl Paths {
    /// Resolve from the process environment.
    pub fn from_env() -> Result<Paths, NoHomeDir> {
        Paths::from_lookup(|name| env::var_os(name))
    }

    /// Resolve from an arbitrary variable lookup.
    ///
    /// A variable that is *set* is used as it is, even when empty or relative
    /// (`XDG_CONFIG_HOME=""` gives `hatate-linux` relative to the working directory),
    /// and `~` is not expanded: spec 1.1 documents that the Python app does exactly
    /// this, and diverging would make a profile vanish for the user who has such a value.
    /// Only an unset variable falls back to `$HOME/.config` or `$HOME/.cache`.
    pub fn from_lookup<F>(get: F) -> Result<Paths, NoHomeDir>
    where
        F: Fn(&str) -> Option<OsString>,
    {
        let config_base = match get("XDG_CONFIG_HOME") {
            Some(v) => PathBuf::from(v),
            None => home_dir(&get)?.join(".config"),
        };
        let cache_base = match get("XDG_CACHE_HOME") {
            Some(v) => PathBuf::from(v),
            None => home_dir(&get)?.join(".cache"),
        };
        Ok(Paths {
            config_dir: config_base.join(APP_DIR_NAME),
            cache_dir: cache_base.join(APP_DIR_NAME),
        })
    }

    /// `CONFIG_DIR`: configuration and all state and data (spec 1.1).
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// `CACHE_DIR`.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// `CACHE_DIR/auto_import_staging` (spec 1.3).
    pub fn auto_import_staging_dir(&self) -> PathBuf {
        self.cache_dir.join("auto_import_staging")
    }

    fn in_config(&self, name: &str) -> PathBuf {
        self.config_dir.join(name)
    }

    /// `config.json`.
    pub fn config_file(&self) -> PathBuf {
        self.in_config("config.json")
    }

    /// `config.json.bak`: the previous config, copied before every save.
    pub fn config_backup_file(&self) -> PathBuf {
        self.in_config("config.json.bak")
    }

    /// `config.json.tmp`: the new config before the atomic rename.
    pub fn config_tmp_file(&self) -> PathBuf {
        self.in_config("config.json.tmp")
    }

    /// `config.json.unreadable`: a config that failed to parse, set aside.
    pub fn config_unreadable_file(&self) -> PathBuf {
        self.in_config("config.json.unreadable")
    }

    /// `hatate-rs.json`: settings only the Rust app has (never in `config.json`).
    pub fn rust_settings_file(&self) -> PathBuf {
        self.in_config(RUST_SETTINGS_FILE_NAME)
    }

    /// `hatate-rs.log`: the Rust app's own log (never `app.log`).
    pub fn rust_log_file(&self) -> PathBuf {
        self.in_config(RUST_LOG_FILE_NAME)
    }

    /// `session.db`.
    pub fn session_db(&self) -> PathBuf {
        self.in_config("session.db")
    }

    /// `session.json`: the legacy automatic session, read once and migrated.
    pub fn session_json(&self) -> PathBuf {
        self.in_config("session.json")
    }

    /// `search_cache/`.
    pub fn search_cache_dir(&self) -> PathBuf {
        self.in_config("search_cache")
    }

    /// `matched_urls.log`, the *default* location; the configured one is `log_file_path`.
    pub fn default_matched_urls_log(&self) -> PathBuf {
        self.in_config("matched_urls.log")
    }

    /// `app.log`.
    pub fn app_log(&self) -> PathBuf {
        self.in_config("app.log")
    }

    /// `crash.log`.
    pub fn crash_log(&self) -> PathBuf {
        self.in_config("crash.log")
    }

    /// `crash.log.running`: "a run is in progress".
    pub fn crash_running_marker(&self) -> PathBuf {
        self.in_config("crash.log.running")
    }

    /// `last_clean_exit`.
    pub fn last_clean_exit(&self) -> PathBuf {
        self.in_config("last_clean_exit")
    }

    /// `mcp_audit.jsonl`.
    pub fn mcp_audit_log(&self) -> PathBuf {
        self.in_config("mcp_audit.jsonl")
    }

    /// `saucenao_quota_pause.json`.
    pub fn saucenao_quota_pause(&self) -> PathBuf {
        self.in_config("saucenao_quota_pause.json")
    }

    /// `pawchive_index.db`.
    pub fn pawchive_index_db(&self) -> PathBuf {
        self.in_config("pawchive_index.db")
    }

    /// `pawchive_creators.json`.
    pub fn pawchive_creators(&self) -> PathBuf {
        self.in_config("pawchive_creators.json")
    }

    /// `lens_profile/`: the Chromium persistent profile of the Google Lens engine.
    pub fn lens_profile_dir(&self) -> PathBuf {
        self.in_config("lens_profile")
    }
}

/// The n-th rotated copy of a rotating log: `matched_urls.log.1`, `app.log.2`, ...
/// (spec 1.2). `n` counts from 1.
pub fn rotated(base: &Path, n: u32) -> PathBuf {
    let mut name = base.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
}

/// True inside the AppImage, whose `AppRun` sets `HATATE_APPIMAGE=1` (spec 1.5, 1.6).
/// Only the exact value `1` counts, as in `core/paths.py:27`.
pub fn running_as_appimage<F>(get: F) -> bool
where
    F: Fn(&str) -> Option<OsString>,
{
    get("HATATE_APPIMAGE").is_some_and(|v| v == OsStr::new("1"))
}

/// Where Playwright keeps its downloaded browsers, for the Google Lens engine, or `None`
/// when that is unknown (`core/lens_browser.py:241-260`, spec 1.5).
///
/// `PLAYWRIGHT_BROWSERS_PATH` wins when non-empty, `"0"` means "inside the package" and
/// yields `None`. Otherwise `$XDG_CACHE_HOME/ms-playwright`, where, unlike
/// [`Paths::from_lookup`], an **empty** `XDG_CACHE_HOME` counts as unset.
pub fn playwright_browsers_root<F>(get: F) -> Result<Option<PathBuf>, NoHomeDir>
where
    F: Fn(&str) -> Option<OsString>,
{
    match get("PLAYWRIGHT_BROWSERS_PATH") {
        Some(v) if v == OsStr::new("0") => return Ok(None),
        Some(v) if !v.is_empty() => return Ok(Some(PathBuf::from(v))),
        _ => {}
    }
    let cache = match get("XDG_CACHE_HOME") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home_dir(&get)?.join(".cache"),
    };
    Ok(Some(cache.join("ms-playwright")))
}

/// Python's `Path.home()`: `$HOME` when set (even empty), else the passwd entry.
fn home_dir<F>(get: &F) -> Result<PathBuf, NoHomeDir>
where
    F: Fn(&str) -> Option<OsString>,
{
    match get("HOME") {
        Some(home) => Ok(normalize_home(&home)),
        None => passwd_home().ok_or(NoHomeDir),
    }
}

/// `os.path.expanduser("~")` strips trailing slashes and turns an empty result into `/`.
fn normalize_home(home: &OsStr) -> PathBuf {
    let bytes = home.as_bytes();
    let end = bytes.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1);
    if end == 0 {
        PathBuf::from("/")
    } else {
        PathBuf::from(OsStr::from_bytes(&bytes[..end]))
    }
}

/// The home directory of the current user from `/etc/passwd`.
///
/// Python asks NSS through `pwd.getpwuid`; reading the file covers a normal desktop
/// without a libc binding. This is only the fallback for a process started without
/// `$HOME`, which a login session never is.
fn passwd_home() -> Option<PathBuf> {
    let uid = fs::metadata("/proc/self").ok()?.uid();
    let passwd = fs::read("/etc/passwd").ok()?;
    passwd_home_for(&passwd, uid)
}

fn passwd_home_for(passwd: &[u8], uid: u32) -> Option<PathBuf> {
    for line in passwd.split(|&b| b == b'\n') {
        let mut fields = line.split(|&b| b == b':');
        let entry_uid = fields
            .nth(2)
            .and_then(|f| std::str::from_utf8(f).ok())
            .and_then(|f| f.parse::<u32>().ok());
        if entry_uid == Some(uid) {
            // After the uid: gid, gecos, home.
            let home = fields.nth(2)?;
            return (!home.is_empty()).then(|| normalize_home(OsStr::from_bytes(home)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<&str, &str> = vars.iter().copied().collect();
        move |name| map.get(name).map(OsString::from)
    }

    #[test]
    fn defaults_come_from_home() {
        let p = Paths::from_lookup(lookup(&[("HOME", "/h/u")])).unwrap();
        assert_eq!(p.config_dir(), Path::new("/h/u/.config/hatate-linux"));
        assert_eq!(p.cache_dir(), Path::new("/h/u/.cache/hatate-linux"));
    }

    #[test]
    fn xdg_variables_win() {
        let p = Paths::from_lookup(lookup(&[
            ("HOME", "/h/u"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CACHE_HOME", "/k"),
        ]))
        .unwrap();
        assert_eq!(p.config_dir(), Path::new("/c/hatate-linux"));
        assert_eq!(p.cache_dir(), Path::new("/k/hatate-linux"));
    }

    // Observed behaviour of the Python app, spec 1.1.
    #[test]
    fn empty_xdg_variable_is_used_as_is() {
        let p = Paths::from_lookup(lookup(&[
            ("HOME", "/h/u"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_CACHE_HOME", ""),
        ]))
        .unwrap();
        assert_eq!(p.config_dir(), Path::new("hatate-linux"));
        assert_eq!(p.cache_dir(), Path::new("hatate-linux"));
    }

    #[test]
    fn relative_xdg_variable_is_used_as_is_and_tilde_is_not_expanded() {
        let p = Paths::from_lookup(lookup(&[
            ("HOME", "/h/u"),
            ("XDG_CONFIG_HOME", "rel/dir"),
            ("XDG_CACHE_HOME", "~/x"),
        ]))
        .unwrap();
        assert_eq!(p.config_dir(), Path::new("rel/dir/hatate-linux"));
        assert_eq!(p.cache_dir(), Path::new("~/x/hatate-linux"));
    }

    #[test]
    fn home_is_normalised_like_expanduser() {
        assert_eq!(normalize_home(OsStr::new("/h/u/")), PathBuf::from("/h/u"));
        assert_eq!(normalize_home(OsStr::new("/h/u//")), PathBuf::from("/h/u"));
        assert_eq!(normalize_home(OsStr::new("")), PathBuf::from("/"));
        assert_eq!(normalize_home(OsStr::new("/")), PathBuf::from("/"));
    }

    #[test]
    fn unset_home_falls_back_to_passwd_not_to_a_guess() {
        // The passwd fallback exists on any machine that runs the tests; what matters is
        // that it is the only source and never silently the current directory.
        if let Ok(p) = Paths::from_lookup(lookup(&[])) {
            assert!(p.config_dir().is_absolute());
        }
        // With both XDG variables set, no home is needed at all.
        assert!(
            Paths::from_lookup(lookup(&[
                ("XDG_CONFIG_HOME", "/c"),
                ("XDG_CACHE_HOME", "/k")
            ]))
            .is_ok()
        );
    }

    #[test]
    fn passwd_parsing() {
        let file =
            b"root:x:0:0:root:/root:/bin/bash\nu:x:1000:1000:U,,,:/home/u:/bin/fish\nbad line\n";
        assert_eq!(passwd_home_for(file, 1000), Some(PathBuf::from("/home/u")));
        assert_eq!(passwd_home_for(file, 0), Some(PathBuf::from("/root")));
        assert_eq!(passwd_home_for(file, 7), None);
    }

    #[test]
    fn file_names_match_the_spec() {
        let p = Paths::from_lookup(lookup(&[
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CACHE_HOME", "/k"),
        ]))
        .unwrap();
        assert_eq!(p.config_file(), Path::new("/c/hatate-linux/config.json"));
        assert_eq!(
            p.config_backup_file(),
            Path::new("/c/hatate-linux/config.json.bak")
        );
        assert_eq!(
            p.config_tmp_file(),
            Path::new("/c/hatate-linux/config.json.tmp")
        );
        assert_eq!(
            p.config_unreadable_file(),
            Path::new("/c/hatate-linux/config.json.unreadable")
        );
        assert_eq!(p.session_db(), Path::new("/c/hatate-linux/session.db"));
        assert_eq!(
            p.search_cache_dir(),
            Path::new("/c/hatate-linux/search_cache")
        );
        assert_eq!(
            p.auto_import_staging_dir(),
            Path::new("/k/hatate-linux/auto_import_staging")
        );
        assert_eq!(
            rotated(&p.app_log(), 2),
            Path::new("/c/hatate-linux/app.log.2")
        );
    }

    #[test]
    fn appimage_flag_is_exactly_one() {
        assert!(running_as_appimage(lookup(&[("HATATE_APPIMAGE", "1")])));
        assert!(!running_as_appimage(lookup(&[("HATATE_APPIMAGE", "true")])));
        assert!(!running_as_appimage(lookup(&[("HATATE_APPIMAGE", "")])));
        assert!(!running_as_appimage(lookup(&[])));
    }

    #[test]
    fn playwright_root_follows_lens_browser() {
        let root =
            |vars: &[(&'static str, &'static str)]| playwright_browsers_root(lookup(vars)).unwrap();
        assert_eq!(
            root(&[("PLAYWRIGHT_BROWSERS_PATH", "0"), ("HOME", "/h")]),
            None
        );
        assert_eq!(
            root(&[("PLAYWRIGHT_BROWSERS_PATH", "/pw"), ("HOME", "/h")]),
            Some("/pw".into())
        );
        assert_eq!(
            root(&[("HOME", "/h")]),
            Some("/h/.cache/ms-playwright".into())
        );
        assert_eq!(
            root(&[("HOME", "/h"), ("XDG_CACHE_HOME", "/k")]),
            Some("/k/ms-playwright".into())
        );
        // Unlike paths.py: empty counts as unset here.
        assert_eq!(
            root(&[
                ("HOME", "/h"),
                ("XDG_CACHE_HOME", ""),
                ("PLAYWRIGHT_BROWSERS_PATH", "")
            ]),
            Some("/h/.cache/ms-playwright".into())
        );
    }
}
