//! `cargo run -p xtask -- appimage`: builds `Hatate-x86_64.AppImage` from a release build.
//!
//! Rust only (CLAUDE.md): the recipe is this file. It shells out to tools the way any build does
//! (`cargo`, `curl`, `sha256sum`, `unzip`, `ldd`, and the pinned `appimagetool`), and every
//! external input is pinned by URL **and** sha256 in `packaging/appimage/pins.toml`, checked
//! before use. `AppRun` is the `hatate-apprun` Rust binary, not a script.
//!
//! What goes in the image (`<out>/AppDir`):
//!
//! ```text
//! AppRun                      crates/hatate-apprun (sets the environment, then execs below)
//! usr/bin/hatate-gtk, hatate  the window and the command line, from `cargo build --release`
//! usr/lib/                    the shared libraries those and Chromium need (the AppImage
//!                             excludelist, less FORCE_BUNDLE, stays on the host)
//! usr/lib/ms-playwright/      the pinned Chromium (Chrome for Testing), headed
//! usr/lib/gdk-pixbuf-2.0/     the image loaders GTK's symbolic icons go through
//! usr/share/glib-2.0/schemas  the compiled GSettings schemas GTK and libadwaita read
//! usr/share/icons/Adwaita     the symbolic icon theme
//! usr/share/fonts/hatate/     the app's TTF/OTF faces (never WOFF2) and a fallback face for a
//!                             machine with no fonts (Chromium draws blank text without one)
//! usr/share/licenses/         every bundled component's licence text, and INDEX.txt
//! ```
//!
//! **GTK 4 and libadwaita are bundled, not taken from the host** (design call, DAN-2499): the
//! window needs GTK 4.12 and libadwaita 1.5 (`crates/hatate-gtk/Cargo.toml`), and the images the
//! AppImage must start on, a clean Arch rootfs with only Xvfb, carry neither. The cost is a glibc
//! floor equal to the build host's (hosted `ubuntu-latest`), which the build prints.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Res<T> = Result<T, String>;

const TARGET_NAME: &str = "Hatate-x86_64.AppImage";

/// Excludelist entries that are bundled anyway: DT_NEEDED of Chromium or of GTK/libadwaita that a
/// machine with only a base system and Xvfb does not have. GL stays out on purpose: the GL front
/// ends are the host's (libepoxy opens them with `dlopen`), and without a driver GTK uses its
/// Cairo renderer.
const FORCE_BUNDLE: [&str; 13] = [
    "libgbm.so.1",
    "libdrm.so.2",
    "libasound.so.2",
    "libxcb.so.1",
    "libX11.so.6",
    "libX11-xcb.so.1",
    "libfontconfig.so.1",
    "libfreetype.so.6",
    "libharfbuzz.so.0",
    "libfribidi.so.0",
    "libexpat.so.1",
    "libwayland-client.so.0",
    "libwayland-cursor.so.0",
];

/// Libraries that are `dlopen`ed by path, so `ldd` cannot see them: package library -> files
/// taken from its directory.
const DLOPENED: [(&str, &[&str]); 1] = [(
    "libnss3.so",
    &[
        "libsoftokn3.so",
        "libfreeblpriv3.so",
        "libnssckbi.so",
        "libnssdbm3.so",
        "libfreebl3.so",
    ],
)];

/// One section of `pins.toml`: `[name]` then `key = "value"` lines.
pub type Pins = BTreeMap<String, BTreeMap<String, String>>;

/// Reads the manifest. Only what `pins.toml` uses: comments, `[section]`, `key = "string"`.
pub fn parse_pins(text: &str) -> Res<Pins> {
    let mut pins: Pins = BTreeMap::new();
    let mut section: Option<String> = None;
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = Some(name.trim().to_owned());
            pins.entry(name.trim().to_owned()).or_default();
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("pins.toml line {}: not `key = \"value\"`", number + 1))?;
        let value = value
            .trim()
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(|| format!("pins.toml line {}: value is not a string", number + 1))?;
        let section = section
            .as_ref()
            .ok_or_else(|| format!("pins.toml line {}: key outside a section", number + 1))?;
        pins.get_mut(section)
            .expect("section inserted above")
            .insert(key.trim().to_owned(), value.to_owned());
    }
    Ok(pins)
}

/// A pinned input: its URL and sha256, checked for shape (https, 64 hex digits, no `latest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub url: String,
    pub sha256: String,
}

/// The pin of `name`, or why it is unusable.
pub fn pin(pins: &Pins, name: &str) -> Res<Pin> {
    let section = pins
        .get(name)
        .ok_or_else(|| format!("pins.toml has no [{name}]"))?;
    let get = |key: &str| {
        section
            .get(key)
            .cloned()
            .ok_or_else(|| format!("pins.toml [{name}] has no `{key}`"))
    };
    let (url, sha256) = (get("url")?, get("sha256")?);
    if !url.starts_with("https://") {
        return Err(format!("pins.toml [{name}] url is not https: {url}"));
    }
    for floating in ["latest", "continuous", "nightly"] {
        if url.contains(floating) {
            return Err(format!("pins.toml [{name}] url floats ({floating}): {url}"));
        }
    }
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("pins.toml [{name}] sha256 is not 64 hex digits"));
    }
    Ok(Pin { url, sha256 })
}

fn run(cmd: &mut Command) -> Res<String> {
    let shown = format!("{cmd:?}");
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{shown} failed: {}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn sha256_of(path: &Path) -> Res<String> {
    let out = run(Command::new("sha256sum").arg(path))?;
    out.split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "sha256sum printed nothing".to_owned())
}

/// Downloads `pin` into `dl/<file>` unless it is there, then refuses anything whose hash is not
/// the pinned one (a cached file is re-checked every time).
fn fetch(dl: &Path, file: &str, pin: &Pin) -> Res<PathBuf> {
    let dest = dl.join(file);
    if !dest.exists() {
        eprintln!("fetching {}", pin.url);
        let part = dl.join(format!("{file}.part"));
        run(Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(&part)
            .arg(&pin.url))?;
        fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    }
    let got = sha256_of(&dest)?;
    if got != pin.sha256 {
        let _ = fs::remove_file(&dest);
        return Err(format!(
            "SHA-256 mismatch for {file}: pinned {}, got {got}",
            pin.sha256
        ));
    }
    eprintln!("verified {file} sha256={got}");
    Ok(dest)
}

/// The names in the excludelist: one per line, `#` starts a comment.
pub fn parse_excludelist(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|l| l.split('#').next())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `ldd` output to (name, resolved path); a `not found` library is an error.
pub fn parse_ldd(output: &str) -> Res<Vec<(String, PathBuf)>> {
    let mut libs = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        let Some((name, rest)) = line.split_once("=>") else {
            continue; // the loader and `linux-vdso` have no `=>` path
        };
        let rest = rest.trim();
        if rest.starts_with("not found") {
            return Err(format!("ldd: {} not found", name.trim()));
        }
        if let Some(path) = rest.split_whitespace().next()
            && path.starts_with('/')
        {
            libs.push((name.trim().to_owned(), PathBuf::from(path)));
        }
    }
    Ok(libs)
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && &magic == b"\x7fELF"
}

fn files_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            files_under(&path, out);
        } else if kind.is_file() {
            out.push(path);
        }
    }
}

/// Copies every shared library the ELF files of `appdir` need into `appdir/usr/lib`, closing over
/// the copies. Returns the sources it copied (for the licence step).
fn bundle_libs(appdir: &Path, exclude: &BTreeSet<String>) -> Res<Vec<(String, PathBuf)>> {
    let lib = appdir.join("usr/lib");
    fs::create_dir_all(&lib).map_err(|e| e.to_string())?;
    let mut pending: VecDeque<PathBuf> = VecDeque::new();
    let mut all = Vec::new();
    files_under(appdir, &mut all);
    pending.extend(all.into_iter().filter(|p| is_elf(p)));
    let mut done: BTreeSet<String> = BTreeSet::new();
    let mut sources: Vec<(String, PathBuf)> = Vec::new();
    let mut dlopen_dirs: Vec<(PathBuf, &[&str])> = Vec::new();
    while let Some(file) = pending.pop_front() {
        let out = run(Command::new("ldd").arg(&file))?;
        for (name, resolved) in parse_ldd(&out)? {
            if resolved.starts_with(appdir)
                || done.contains(&name)
                || (exclude.contains(&name) && !FORCE_BUNDLE.contains(&name.as_str()))
            {
                continue;
            }
            let real = fs::canonicalize(&resolved).map_err(|e| e.to_string())?;
            let dest = lib.join(&name);
            fs::copy(&real, &dest).map_err(|e| format!("{}: {e}", real.display()))?;
            done.insert(name.clone());
            sources.push((name.clone(), real.clone()));
            for (marker, files) in DLOPENED {
                if name.starts_with(marker)
                    && let Some(dir) = real.parent()
                {
                    dlopen_dirs.push((dir.to_owned(), files));
                }
            }
            pending.push_back(dest);
        }
    }
    for (dir, files) in dlopen_dirs {
        for file in files {
            // Debian puts these in the library directory or its `nss/` sub-directory.
            let found = [dir.join(file), dir.join("nss").join(file)]
                .into_iter()
                .find(|p| p.exists());
            let Some(found) = found else {
                return Err(format!("dlopened {file} not found under {}", dir.display()));
            };
            let real = fs::canonicalize(&found).map_err(|e| e.to_string())?;
            fs::copy(&real, lib.join(file)).map_err(|e| e.to_string())?;
            sources.push(((*file).to_owned(), real));
        }
    }
    eprintln!("bundled {} shared libraries", sources.len());
    Ok(sources)
}

fn copy_tree(from: &Path, to: &Path) -> Res<()> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = fs::symlink_metadata(&src)
            .map_err(|e| e.to_string())?
            .file_type();
        if kind.is_symlink() {
            let target = fs::read_link(&src).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(&dst);
            std::os::unix::fs::symlink(target, &dst).map_err(|e| e.to_string())?;
        } else if kind.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

fn first_existing(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// A WOFF2 face anywhere in the image is a bug: fontconfig 2.15 and Pango 1.52 do not list it
/// (Phase 1, DAN-1983). Returns the offenders.
pub fn woff2_files(appdir: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    files_under(appdir, &mut all);
    all.into_iter()
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("woff2") || e.eq_ignore_ascii_case("woff"))
        })
        .collect()
}

/// The pixbuf loaders, the schemas, the icon theme: GTK's own data, taken from the build host.
fn bundle_gtk_data(appdir: &Path, sources: &mut Vec<(String, PathBuf)>) -> Res<()> {
    let loaders = first_existing(&[
        "/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders",
        "/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders",
    ]);
    if let Some(loaders) = loaders {
        let to = appdir.join("usr/lib/gdk-pixbuf-2.0/2.10.0/loaders");
        copy_tree(&loaders, &to)?;
        sources.push(("gdk-pixbuf loaders".to_owned(), loaders));
    } else {
        return Err(
            "no gdk-pixbuf loaders directory on this host (install librsvg2-common)".into(),
        );
    }
    let query = first_existing(&[
        "/usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders",
        "/usr/bin/gdk-pixbuf-query-loaders",
    ]);
    if let Some(query) = query {
        let bin = appdir.join("usr/bin/gdk-pixbuf-query-loaders");
        fs::copy(&query, &bin).map_err(|e| e.to_string())?;
    }
    let schemas = Path::new("/usr/share/glib-2.0/schemas");
    if schemas.is_dir() {
        let to = appdir.join("usr/share/glib-2.0/schemas");
        copy_tree(schemas, &to)?;
        // The host compiled these, but a compiled file next to a different set is stale.
        run(Command::new("glib-compile-schemas").arg(&to))?;
    } else {
        return Err("no /usr/share/glib-2.0/schemas on this host".into());
    }
    let adwaita = Path::new("/usr/share/icons/Adwaita");
    if adwaita.is_dir() {
        let to = appdir.join("usr/share/icons/Adwaita");
        copy_tree(adwaita, &to)?;
        let _ = fs::remove_dir_all(to.join("cursors"));
    } else {
        eprintln!("warning: no Adwaita icon theme on this host; symbolic icons will be blank");
    }
    Ok(())
}

fn copy_fonts(repo: &Path, appdir: &Path) -> Res<()> {
    let to = appdir.join("usr/share/fonts/hatate");
    fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    // The app's own faces are also compiled into `hatate-gtk` (`fonts.rs`); shipping the files
    // lets Chromium and the licence texts sit next to them.
    let assets = repo.join("crates/hatate-gtk/assets/fonts");
    for entry in fs::read_dir(&assets).map_err(|e| format!("{}: {e}", assets.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "ttf" | "otf") {
            fs::copy(&path, to.join(path.file_name().expect("file"))).map_err(|e| e.to_string())?;
        }
    }
    // A fallback family for a machine with no fonts at all: Chromium draws blank text, Pango
    // draws tofu, without one. DejaVu (free licence, `fonts-dejavu-core` on the build host).
    let dejavu = first_existing(&["/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/TTF"])
        .ok_or("no DejaVu fonts on this host (install fonts-dejavu-core)")?;
    let mut copied = 0;
    for entry in fs::read_dir(&dejavu).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if name.starts_with("DejaVu") && name.ends_with(".ttf") {
            fs::copy(&path, to.join(&name)).map_err(|e| e.to_string())?;
            copied += 1;
        }
    }
    if copied == 0 {
        return Err(format!("no DejaVu*.ttf in {}", dejavu.display()));
    }
    Ok(())
}

fn set_exec(path: &Path) -> Res<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

/// The workspace root: the directory above `xtask/`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits inside the workspace")
        .to_owned()
}

/// `xtask appimage [--out DIR] [--dl DIR] [--no-build]`.
pub fn build(args: &[&str]) -> i32 {
    match build_inner(args) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(e) => {
            eprintln!("xtask appimage: {e}");
            1
        }
    }
}

fn arg_value(args: &[&str], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| *a == flag)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

fn build_inner(args: &[&str]) -> Res<PathBuf> {
    let repo = repo_root();
    let out = arg_value(args, "--out").unwrap_or_else(|| repo.join("target/appimage"));
    let dl = arg_value(args, "--dl").unwrap_or_else(|| out.join("dl"));
    let appdir = out.join("AppDir");
    fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&appdir);
    fs::create_dir_all(appdir.join("usr/bin")).map_err(|e| e.to_string())?;

    let pins = parse_pins(
        &fs::read_to_string(repo.join("packaging/appimage/pins.toml"))
            .map_err(|e| format!("pins.toml: {e}"))?,
    )?;
    let chromium = fetch(&dl, "chrome-linux64.zip", &pin(&pins, "chromium")?)?;
    let tool = fetch(
        &dl,
        "appimagetool-x86_64.AppImage",
        &pin(&pins, "appimagetool")?,
    )?;
    let runtime = fetch(&dl, "runtime-x86_64", &pin(&pins, "runtime")?)?;
    let excludelist = fetch(&dl, "excludelist", &pin(&pins, "excludelist")?)?;
    set_exec(&tool)?;
    let revision = pins["chromium"]
        .get("revision")
        .ok_or("pins.toml [chromium] has no revision")?;

    if !args.contains(&"--no-build") {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&repo)
            .args([
                "build",
                "--release",
                "-p",
                "hatate-gtk",
                "-p",
                "hatate-cli",
                "-p",
                "hatate-apprun",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("cargo build --release failed".into());
        }
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target"))
        .join("release");
    for (from, to) in [
        ("AppRun", "AppRun"),
        ("hatate-gtk", "usr/bin/hatate-gtk"),
        ("hatate", "usr/bin/hatate"),
    ] {
        let dest = appdir.join(to);
        fs::copy(target.join(from), &dest).map_err(|e| format!("{from}: {e}"))?;
        set_exec(&dest)?;
    }

    // Chromium, in Playwright's layout: usr/lib/ms-playwright/chromium-<rev>/chrome-linux64.
    let browsers = appdir
        .join("usr/lib/ms-playwright")
        .join(format!("chromium-{revision}"));
    fs::create_dir_all(&browsers).map_err(|e| e.to_string())?;
    run(Command::new("unzip")
        .arg("-q")
        .arg(&chromium)
        .arg("-d")
        .arg(&browsers))?;
    let chrome = browsers.join("chrome-linux64/chrome");
    if !chrome.is_file() {
        return Err(format!("the Chromium archive has no {}", chrome.display()));
    }
    set_exec(&chrome)?;

    copy_fonts(&repo, &appdir)?;
    let exclude = parse_excludelist(&fs::read_to_string(&excludelist).map_err(|e| e.to_string())?);
    let mut sources = bundle_libs(&appdir, &exclude)?;
    bundle_gtk_data(&appdir, &mut sources)?;

    // appimagetool wants the desktop file and the icon at the root.
    let pkg = repo.join("packaging/appimage");
    fs::copy(pkg.join("hatate.desktop"), appdir.join("hatate.desktop"))
        .map_err(|e| e.to_string())?;
    fs::copy(pkg.join("hatate.svg"), appdir.join("hatate.svg")).map_err(|e| e.to_string())?;
    let _ = std::os::unix::fs::symlink("hatate.svg", appdir.join(".DirIcon"));

    let offenders = woff2_files(&appdir);
    if !offenders.is_empty() {
        return Err(format!("WOFF2/WOFF faces in the image: {offenders:?}"));
    }
    let glibc = run(Command::new("ldd").arg("--version"))
        .ok()
        .and_then(|v| v.lines().next().map(str::to_owned))
        .unwrap_or_default();
    eprintln!("build host libc (the image's glibc floor): {glibc}");

    let image = out.join(TARGET_NAME);
    let _ = fs::remove_file(&image);
    let status = Command::new(&tool)
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .env("ARCH", "x86_64")
        .arg("--runtime-file")
        .arg(&runtime)
        .arg("--no-appstream")
        .arg(&appdir)
        .arg(&image)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("appimagetool failed: {status}"));
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "43fd31766c7533a02dda2ded5ad50c2f861cd5400be7509c0abad71129f79d79";

    #[test]
    fn the_checked_in_manifest_pins_every_input_by_url_and_sha256() {
        let text = fs::read_to_string(repo_root().join("packaging/appimage/pins.toml")).unwrap();
        let pins = parse_pins(&text).unwrap();
        for name in ["chromium", "appimagetool", "runtime", "excludelist"] {
            pin(&pins, name).unwrap_or_else(|e| panic!("{e}"));
        }
        assert_eq!(pins["chromium"]["revision"], "1248");
        assert!(
            pins["chromium"]["url"].contains(&pins["chromium"]["version"]),
            "the Chromium URL names the pinned version"
        );
    }

    #[test]
    fn a_floating_or_unhashed_pin_is_refused() {
        let p = |body: &str| pin(&parse_pins(body).unwrap(), "x");
        assert!(p(&format!("[x]\nurl = \"https://h/a\"\nsha256 = \"{SHA}\"")).is_ok());
        assert!(
            p(&format!(
                "[x]\nurl = \"https://h/latest/a\"\nsha256 = \"{SHA}\""
            ))
            .is_err()
        );
        assert!(
            p(&format!(
                "[x]\nurl = \"https://h/continuous/a\"\nsha256 = \"{SHA}\""
            ))
            .is_err()
        );
        assert!(p(&format!("[x]\nurl = \"http://h/a\"\nsha256 = \"{SHA}\"")).is_err());
        assert!(p("[x]\nurl = \"https://h/a\"").is_err());
        assert!(p("[x]\nurl = \"https://h/a\"\nsha256 = \"abc\"").is_err());
    }

    #[test]
    fn the_manifest_parser_reads_sections_comments_and_rejects_the_rest() {
        let pins = parse_pins("# c\n[a]\nk = \"v\" \n\n[b]\nk = \"w\"\n").unwrap();
        assert_eq!(pins["a"]["k"], "v");
        assert_eq!(pins["b"]["k"], "w");
        assert!(parse_pins("k = \"v\"").is_err(), "key outside a section");
        assert!(parse_pins("[a]\nk = v").is_err(), "unquoted value");
        assert!(parse_pins("[a]\nnonsense").is_err());
    }

    #[test]
    fn ldd_output_is_read_and_a_missing_library_is_an_error() {
        let ok = "\tlinux-vdso.so.1 (0x00007ffd)\n\tlibm.so.6 => /usr/lib/libm.so.6 (0x1)\n\t/lib64/ld-linux-x86-64.so.2 (0x2)\n";
        assert_eq!(
            parse_ldd(ok).unwrap(),
            vec![("libm.so.6".to_owned(), PathBuf::from("/usr/lib/libm.so.6"))]
        );
        assert!(parse_ldd("\tlibfoo.so.1 => not found\n").is_err());
    }

    #[test]
    fn the_excludelist_drops_comments_and_blank_lines() {
        let set = parse_excludelist("# header\nlibc.so.6\n\nlibm.so.6 # trailing\n");
        assert_eq!(set.len(), 2);
        assert!(set.contains("libc.so.6") && set.contains("libm.so.6"));
    }

    #[test]
    fn a_woff2_face_in_the_image_is_found() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("usr/share/fonts")).unwrap();
        fs::write(dir.path().join("usr/share/fonts/a.ttf"), b"").unwrap();
        assert!(woff2_files(dir.path()).is_empty());
        fs::write(dir.path().join("usr/share/fonts/b.WOFF2"), b"").unwrap();
        assert_eq!(woff2_files(dir.path()).len(), 1);
    }

    #[test]
    fn the_apprun_the_image_ships_is_a_rust_binary_named_apprun() {
        // `AppRun` is a Rust binary (never a script): the launcher crate's only binary is named
        // so, which is the file `build_inner` copies from the release directory.
        let manifest =
            fs::read_to_string(repo_root().join("crates/hatate-apprun/Cargo.toml")).unwrap();
        assert!(manifest.contains("name = \"AppRun\""));
    }
}
