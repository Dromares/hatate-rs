//! Test scaffolding: every test runs in its own temp profile and never resolves to the
//! real one (DAN-1979 acceptance: "never resolves to the real profile").

#![allow(dead_code)]

pub mod art;
mod net_server;
#[allow(unused_imports)]
pub use net_server::*;
mod replay;
#[allow(unused_imports)]
pub use replay::*;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hatate_core::paths::Paths;
use tempfile::TempDir;

/// A throw-away profile: `HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` all inside a temp dir.
pub struct Sandbox {
    pub dir: TempDir,
    pub paths: Paths,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let dir = tempfile::Builder::new()
            .prefix("hatate-core-test-")
            .tempdir()
            .expect("temp dir");
        let root = dir.path().to_path_buf();
        let vars: HashMap<&str, OsString> = HashMap::from([
            ("HOME", root.join("home").into()),
            ("XDG_CONFIG_HOME", root.join("xdg/config").into()),
            ("XDG_CACHE_HOME", root.join("xdg/cache").into()),
        ]);
        let paths = Paths::from_lookup(|k| vars.get(k).cloned()).expect("paths");
        let sb = Sandbox { dir, paths };
        sb.assert_sandboxed();
        sb
    }

    /// A sandbox that resolves through `HOME` alone (no XDG variables), like a plain login.
    pub fn home_only() -> Sandbox {
        let dir = tempfile::Builder::new()
            .prefix("hatate-core-test-")
            .tempdir()
            .expect("temp dir");
        let home: OsString = dir.path().join("home").into();
        let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).expect("paths");
        let sb = Sandbox { dir, paths };
        sb.assert_sandboxed();
        sb
    }

    /// Fails when the resolved directories are not inside the temp dir, or are the real
    /// profile's. "Real" is what the process environment resolves to; the check holds even
    /// when the temp dir itself is under the real `$HOME`.
    pub fn assert_sandboxed(&self) {
        let root = self.dir.path();
        for p in [self.paths.config_dir(), self.paths.cache_dir()] {
            assert!(
                p.starts_with(root),
                "{} escapes the sandbox {}",
                p.display(),
                root.display()
            );
        }
        if let Ok(real) = Paths::from_env() {
            for (mine, theirs) in [
                (self.paths.config_dir(), real.config_dir()),
                (self.paths.cache_dir(), real.cache_dir()),
            ] {
                assert_ne!(mine, theirs, "sandbox resolved to the real profile");
                assert!(
                    !mine.starts_with(theirs),
                    "sandbox is inside the real profile"
                );
            }
        }
    }

    pub fn config_dir(&self) -> &Path {
        self.paths.config_dir()
    }

    /// Write `bytes` as `config.json`, creating the directory `0755` like a hand-made one.
    pub fn put_config(&self, bytes: &[u8]) {
        fs::create_dir_all(self.config_dir()).unwrap();
        fs::write(self.paths.config_file(), bytes).unwrap();
    }

    pub fn read(&self, path: impl AsRef<Path>) -> Vec<u8> {
        fs::read(path).unwrap()
    }

    pub fn config_bytes(&self) -> Vec<u8> {
        self.read(self.paths.config_file())
    }

    /// Names in `CONFIG_DIR`, sorted.
    pub fn listing(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.config_dir())
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

pub fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

pub fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/formats")
        .join(rel)
}

pub fn doc() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/file-formats.md"))
        .unwrap()
}

/// Replace the value of `log_file_path`, the one machine-specific line (rules 6.1.8, 6.1.9).
pub fn normalise_log_path(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|l| {
            if l.starts_with("  \"log_file_path\": ") {
                "  \"log_file_path\": \"<LOG>\",".to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
