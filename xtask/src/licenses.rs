//! The licence step of `xtask appimage`: every component the image bundles gets a licence entry
//! under `usr/share/licenses/`, and the build fails when one has none.
//!
//! ```text
//! usr/share/licenses/INDEX.txt         one line per component: name, version, SPDX/where from, files
//! usr/share/licenses/hatate/           the app's own MIT text
//! usr/share/licenses/chromium/         the Chromium project's BSD-3-Clause text (checked in)
//! usr/share/licenses/fonts/            the app's OFL texts and the DejaVu licence
//! usr/share/licenses/lib/<package>/    the distro package's copyright file for each bundled library
//! usr/share/licenses/crates/<n>-<v>/   each Rust crate's LICENSE/COPYING/NOTICE files
//! ```
//!
//! The Rust crates come from `cargo tree` (normal edges of the three shipped binaries) joined to
//! `cargo metadata` for the licence expression and the source directory, so no extra tool (such as
//! `cargo about`) is needed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Res<T> = Result<T, String>;

/// One line of the index.
pub struct Entry {
    pub name: String,
    pub version: String,
    pub licence: String,
    pub files: Vec<String>,
}

/// Names that make a file a licence text: `LICENSE`, `LICENSE-MIT`, `COPYING`, `UNLICENSE`,
/// `NOTICE`, in any case, with any extension.
pub fn is_licence_file(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "UNLICENSE", "NOTICE"]
        .iter()
        .filter_map(|p| upper.strip_prefix(p))
        .any(|rest| rest.is_empty() || rest.starts_with(['-', '_', '.']))
}

/// `name vX.Y.Z` pairs from `cargo tree --prefix none -f '{p}'` output, registry crates only (a
/// path dependency prints `(/path)` and is the app's own code, covered by its MIT text).
pub fn parse_tree(text: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let (Some(name), Some(version)) = (words.next(), words.next()) else {
            continue;
        };
        let Some(version) = version.strip_prefix('v') else {
            continue;
        };
        if line.contains(" (/") {
            continue;
        }
        out.insert((name.to_owned(), version.to_owned()));
    }
    out
}

fn run(cmd: &mut Command) -> Res<String> {
    let shown = format!("{cmd:?}");
    let out = cmd.output().map_err(|e| format!("{shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{shown} failed: {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn copy_licence_files(from_dir: &Path, to_dir: &Path) -> Res<Vec<String>> {
    let mut copied = Vec::new();
    for entry in fs::read_dir(from_dir).map_err(|e| format!("{}: {e}", from_dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if path.is_file() && is_licence_file(&name) {
            fs::create_dir_all(to_dir).map_err(|e| e.to_string())?;
            fs::copy(&path, to_dir.join(&name)).map_err(|e| format!("{}: {e}", path.display()))?;
            copied.push(name);
        }
    }
    copied.sort();
    Ok(copied)
}

/// The Rust crates of the three shipped binaries, with licence files copied.
fn rust_crates(repo: &Path, licenses: &Path) -> Res<Vec<Entry>> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let tree = run(Command::new(&cargo).current_dir(repo).args([
        "tree",
        "-e",
        "normal",
        "--prefix",
        "none",
        "-f",
        "{p}",
        "-p",
        "hatate-gtk",
        "-p",
        "hatate-cli",
        "-p",
        "hatate-apprun",
    ]))?;
    let wanted = parse_tree(&tree);
    let meta = run(Command::new(&cargo).current_dir(repo).args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
    ]))?;
    let meta: serde_json::Value = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
    let mut by_id: BTreeMap<(String, String), (String, PathBuf)> = BTreeMap::new();
    for p in meta["packages"].as_array().ok_or("no packages")? {
        let (Some(name), Some(version), Some(manifest)) = (
            p["name"].as_str(),
            p["version"].as_str(),
            p["manifest_path"].as_str(),
        ) else {
            continue;
        };
        let licence = p["license"].as_str().unwrap_or("").to_owned();
        let dir = Path::new(manifest)
            .parent()
            .expect("manifest dir")
            .to_owned();
        by_id.insert((name.to_owned(), version.to_owned()), (licence, dir));
    }
    let mut entries = Vec::new();
    for key in wanted {
        let (licence, dir) = by_id.get(&key).ok_or_else(|| {
            format!(
                "{} {} is in `cargo tree` but not in `cargo metadata`",
                key.0, key.1
            )
        })?;
        let to = licenses.join("crates").join(format!("{}-{}", key.0, key.1));
        let files = copy_licence_files(dir, &to)?;
        if licence.is_empty() || files.is_empty() {
            return Err(format!(
                "crate {} {} has no licence entry (SPDX {licence:?}, licence files {files:?})",
                key.0, key.1
            ));
        }
        entries.push(Entry {
            name: key.0,
            version: key.1,
            licence: licence.clone(),
            files,
        });
    }
    Ok(entries)
}

/// The package that owns `path` and its version: `dpkg` on Debian/Ubuntu (the hosted build), then
/// `pacman` on Arch.
fn owner(path: &Path) -> Res<(String, String, Vec<PathBuf>)> {
    let shown = path.display().to_string();
    let candidates = [
        shown.clone(),
        shown.replacen("/usr/lib", "/lib", 1),
        shown.replacen("/lib", "/usr/lib", 1),
    ];
    for c in &candidates {
        if let Ok(out) = run(Command::new("dpkg-query").args(["-S", c])) {
            let package = out.split(':').next().unwrap_or("").trim().to_owned();
            if package.is_empty() {
                continue;
            }
            let version =
                run(Command::new("dpkg-query").args(["-W", "-f", "${Version}", &package]))
                    .unwrap_or_default();
            let doc = PathBuf::from("/usr/share/doc").join(&package);
            let files = ["copyright"]
                .iter()
                .map(|f| doc.join(f))
                .filter(|p| p.is_file())
                .collect();
            return Ok((package, version, files));
        }
    }
    let out = run(Command::new("pacman").args(["-Qo", "-q", &shown]))
        .map_err(|_| format!("no dpkg or pacman package owns {shown}"))?;
    let package = out.trim().to_owned();
    let version = run(Command::new("pacman").args(["-Q", &package]))
        .map(|v| v.split_whitespace().nth(1).unwrap_or("").to_owned())
        .unwrap_or_default();
    let dir = PathBuf::from("/usr/share/licenses").join(&package);
    let mut files = Vec::new();
    if let Ok(read) = fs::read_dir(&dir) {
        for e in read.flatten() {
            if e.path().is_file() {
                files.push(e.path());
            }
        }
    }
    if files.is_empty() {
        // Arch points common licences at /usr/share/licenses/spdx instead of copying them.
        let declared = run(Command::new("pacman").args(["-Qi", &package])).unwrap_or_default();
        let ids = declared
            .lines()
            .find_map(|l| l.strip_prefix("Licenses"))
            .and_then(|l| l.split_once(':'))
            .map_or("", |(_, ids)| ids);
        for id in ids
            .split_whitespace()
            .filter(|w| !matches!(*w, "OR" | "AND" | "WITH"))
        {
            let text = PathBuf::from("/usr/share/licenses/spdx").join(format!("{id}.txt"));
            if text.is_file() {
                files.push(text);
            }
        }
    }
    Ok((package, version, files))
}

/// Licence files of the distro packages that own the bundled libraries and GTK data.
fn bundled_packages(sources: &[(String, PathBuf)], licenses: &Path) -> Res<Vec<Entry>> {
    let mut by_package: BTreeMap<String, (String, Vec<String>, Vec<PathBuf>)> = BTreeMap::new();
    let mut missing = Vec::new();
    for (name, source) in sources {
        // A directory (the icon theme) is owned through the first of its files a package owns:
        // generated ones (`icon-theme.cache`) belong to no package.
        let probes: Vec<PathBuf> = if source.is_dir() {
            let mut files: Vec<PathBuf> = fs::read_dir(source)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            files.sort();
            files
        } else {
            vec![source.clone()]
        };
        let found = probes.iter().find_map(|probe| owner(probe).ok());
        let Some((package, version, files)) = found else {
            missing.push(format!("{name}: no package owns {}", source.display()));
            continue;
        };
        let slot = by_package
            .entry(package)
            .or_insert_with(|| (version, Vec::new(), files));
        slot.1.push(name.clone());
    }
    let mut entries = Vec::new();
    for (package, (version, libs, files)) in by_package {
        if files.is_empty() {
            missing.push(format!("{package} {version} ({})", libs.join(", ")));
            continue;
        }
        let to = licenses.join("lib").join(&package);
        fs::create_dir_all(&to).map_err(|e| e.to_string())?;
        let mut names = Vec::new();
        for f in files {
            let name = f
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("copyright");
            fs::copy(&f, to.join(name)).map_err(|e| format!("{}: {e}", f.display()))?;
            names.push(name.to_owned());
        }
        entries.push(Entry {
            name: package,
            version,
            licence: format!("distro package; bundles {}", libs.join(", ")),
            files: names,
        });
    }
    if !missing.is_empty() {
        return Err(format!(
            "{} bundled components have no owning package or no copyright/licence file on this host: {}",
            missing.len(),
            missing.join("; ")
        ));
    }
    Ok(entries)
}

fn fonts(repo: &Path, appdir: &Path, licenses: &Path) -> Res<Vec<Entry>> {
    let to = licenses.join("fonts");
    fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let assets = repo.join("crates/hatate-gtk/assets/fonts");
    let mut files = Vec::new();
    for entry in fs::read_dir(&assets).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if name.starts_with("OFL-") || name.starts_with("SOURCE-") {
            fs::copy(&path, to.join(&name)).map_err(|e| e.to_string())?;
            files.push(name);
        }
    }
    files.sort();
    if !files.iter().any(|f| f.starts_with("OFL-")) {
        return Err("no OFL text beside the app's fonts".into());
    }
    let mut entries = vec![Entry {
        name: "hatate fonts (JetBrains Mono, Source Serif 4, Space Grotesk, Ryoku Kanji/Status)"
            .into(),
        version: String::new(),
        licence: "OFL-1.1".into(),
        files,
    }];
    // DejaVu, the fallback face: Arch ships it under /usr/share/licenses, Debian under doc/.
    let dejavu = [
        "/usr/share/doc/fonts-dejavu-core/copyright",
        "/usr/share/licenses/ttf-dejavu/LICENSE",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .ok_or("no DejaVu licence text on this host (fonts-dejavu-core, ttf-dejavu)")?;
    fs::copy(&dejavu, to.join("DejaVu-LICENSE.txt")).map_err(|e| e.to_string())?;
    let has_dejavu = appdir
        .join("usr/share/fonts/hatate")
        .read_dir()
        .is_ok_and(|mut d| {
            d.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("DejaVu")))
        });
    if has_dejavu {
        entries.push(Entry {
            name: "DejaVu fonts".into(),
            version: String::new(),
            licence: "Bitstream Vera / public-domain additions".into(),
            files: vec!["DejaVu-LICENSE.txt".into()],
        });
    }
    Ok(entries)
}

/// Writes `usr/share/licenses` and `NOTICE` into `appdir`. `sources` is what `bundle_libs` and
/// `bundle_gtk_data` copied.
pub fn write(repo: &Path, appdir: &Path, sources: &[(String, PathBuf)], chromium: &str) -> Res<()> {
    let licenses = appdir.join("usr/share/licenses");
    fs::create_dir_all(&licenses).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();

    let own = licenses.join("hatate");
    fs::create_dir_all(&own).map_err(|e| e.to_string())?;
    fs::copy(repo.join("LICENSE"), own.join("LICENSE")).map_err(|e| format!("LICENSE: {e}"))?;
    entries.push(Entry {
        name: "hatate (this application's code)".into(),
        version: String::new(),
        licence: "MIT".into(),
        files: vec!["LICENSE".into()],
    });

    let pkg = repo.join("packaging/appimage");
    let browser = licenses.join("chromium");
    fs::create_dir_all(&browser).map_err(|e| e.to_string())?;
    fs::copy(
        pkg.join("licenses/CHROMIUM-BSD-3-Clause.txt"),
        browser.join("LICENSE"),
    )
    .map_err(|e| format!("CHROMIUM-BSD-3-Clause.txt: {e}"))?;
    entries.push(Entry {
        name: "Chromium (Chrome for Testing)".into(),
        version: chromium.to_owned(),
        licence: "BSD-3-Clause for the Chromium project; third-party components are listed in the \
                  browser's own chrome://credits page (offline, from resources.pak)"
            .into(),
        files: vec!["LICENSE".into()],
    });

    entries.extend(fonts(repo, appdir, &licenses)?);
    entries.extend(bundled_packages(sources, &licenses)?);
    entries.extend(rust_crates(repo, &licenses)?);

    let mut index = String::from(
        "Licence index of the Hatate AppImage (work-in-progress Rust rewrite).\n\
         One line per bundled component: name, version, licence, files under this directory.\n\n",
    );
    for e in &entries {
        index.push_str(&format!(
            "{} {} | {} | {}\n",
            e.name,
            e.version,
            e.licence,
            e.files.join(", ")
        ));
    }
    fs::write(licenses.join("INDEX.txt"), index).map_err(|e| e.to_string())?;
    fs::copy(repo.join("NOTICE"), appdir.join("NOTICE")).map_err(|e| format!("NOTICE: {e}"))?;
    eprintln!("licence entries: {}", entries.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_file_names_are_recognised_in_any_case() {
        for yes in [
            "LICENSE",
            "LICENSE-MIT",
            "license-apache",
            "COPYING",
            "UNLICENSE",
            "NOTICE.md",
            "LICENCE.txt",
        ] {
            assert!(is_licence_file(yes), "{yes}");
        }
        for no in ["Cargo.toml", "README.md", "src", "licenses.rs"] {
            assert!(!is_licence_file(no), "{no}");
        }
    }

    #[test]
    fn the_cargo_tree_is_read_into_registry_crates_only() {
        let tree = "adler2 v2.0.1\nadler2 v2.0.1 (*)\nhatate-core v0.0.0 (/w/crates/hatate-core)\nsyn v2.0.1 (proc-macro)\n";
        let set = parse_tree(tree);
        assert_eq!(set.len(), 2);
        assert!(set.contains(&("adler2".into(), "2.0.1".into())));
        assert!(set.contains(&("syn".into(), "2.0.1".into())));
    }

    #[test]
    fn a_crate_with_no_licence_file_stops_the_build() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let to = tempfile::tempdir().unwrap();
        assert!(
            copy_licence_files(dir.path(), to.path())
                .unwrap()
                .is_empty()
        );
        fs::write(dir.path().join("LICENSE-MIT"), "text").unwrap();
        assert_eq!(
            copy_licence_files(dir.path(), to.path()).unwrap(),
            ["LICENSE-MIT"]
        );
    }

    #[test]
    fn the_checked_in_chromium_text_is_the_bsd_licence() {
        let text = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../packaging/appimage/licenses/CHROMIUM-BSD-3-Clause.txt"),
        )
        .unwrap();
        assert!(text.contains("The Chromium Authors"));
        assert!(text.contains("Neither the name of Google LLC"));
    }
}
