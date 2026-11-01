//! The one place the Rust build's version lives (`core/version.py` at `76c85c4`; DAN-2501).
//!
//! It is the workspace version in the root `Cargo.toml` (`[workspace.package] version`), which
//! every crate inherits with `version.workspace = true`. The three surfaces that print a version
//! read this constant: `hatate --version`, the MCP `serverInfo`, and the `crash.log` entry. There
//! is no About dialog in either app (Python has none: its version is printed by `main.py
//! --version` and stamped into the AppImage). Step 7.d moved it to `0.2.0` and moved the status
//! claim in `README.md` with it, under the status-claim sweep of `CLAUDE.md`.

/// The workspace version, `CARGO_PKG_VERSION` of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    /// The root manifest, read from this crate's source directory.
    fn workspace_manifest() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        std::fs::read_to_string(path).expect("the root Cargo.toml")
    }

    #[test]
    fn the_version_is_the_workspace_package_version() {
        let manifest = workspace_manifest();
        let package = manifest
            .split("[workspace.package]")
            .nth(1)
            .expect("[workspace.package]");
        let line = package
            .lines()
            .find(|l| l.starts_with("version"))
            .expect("version line");
        assert_eq!(line.trim(), format!("version = \"{VERSION}\""));
    }

    #[test]
    fn every_crate_inherits_it() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut seen = 0;
        for dir in std::fs::read_dir(crates).unwrap() {
            let manifest = dir.unwrap().path().join("Cargo.toml");
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            assert!(
                text.lines().any(|l| l.trim() == "version.workspace = true"),
                "{} sets its own version",
                manifest.display()
            );
            seen += 1;
        }
        assert!(seen >= 6, "{seen} crates");
    }
}
