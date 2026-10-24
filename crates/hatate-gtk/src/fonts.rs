//! The bundled type, loaded app-local (`gui/fonts.py` at `76c85c4`).
//!
//! Five families: Source Serif 4 (display heads), Space Grotesk (body), JetBrains Mono (mono-caps
//! labels and most of the status glyphs), and two subsets built for the app: Ryoku Kanji (the
//! ghost mark) and Ryoku Status (the six status glyphs `● ○ ◐ × · ▲`). TTF/OTF only, never
//! WOFF2: Pango 1.52 on the CI image does not list WOFF2 faces (DAN-1983).
//!
//! GTK4 has no app-font API of its own. Text goes through Pango, Pango's fontconfig backend asks
//! the process-wide fontconfig configuration, so loading a font means adding a directory to it
//! before the first font lookup. The files are compiled into the binary and written to a
//! directory under the user's cache, then handed to `FcConfigAppFontAddDir`. They are visible to
//! this process only.

use std::ffi::{CString, c_char, c_int, c_void};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One bundled font file.
pub struct FontFile {
    pub file_name: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! font {
    ($name:literal) => {
        FontFile {
            file_name: $name,
            bytes: include_bytes!(concat!("../assets/fonts/", $name)),
        }
    };
}

/// Every bundled face, in the order `gui/fonts.py` sorts them (by family directory, then name).
pub const FILES: [FontFile; 9] = [
    font!("JetBrainsMono-Bold.ttf"),
    font!("JetBrainsMono-Medium.ttf"),
    font!("JetBrainsMono-MediumItalic.ttf"),
    font!("JetBrainsMono-Regular.ttf"),
    font!("RyokuKanji.otf"),
    font!("RyokuStatus.ttf"),
    font!("SourceSerif4-Regular.ttf"),
    font!("SourceSerif4-SemiBold.ttf"),
    font!("SpaceGrotesk-Regular.ttf"),
];

/// The families the type system names (CSS `font-family` names).
pub const FAMILIES: [&str; 5] = [
    "Space Grotesk",
    "JetBrains Mono",
    "Source Serif 4",
    "Ryoku Kanji",
    "Ryoku Status",
];

/// The six status glyphs, in the order Good, Not found, Review, Error, Unsearched, Flagged
/// (`tokens.css`: `unicode-range: U+00B7, U+00D7, U+25B2, U+25CB, U+25CF, U+25D0`).
pub const STATUS_GLYPHS: [char; 6] = [
    '\u{25CF}', '\u{25CB}', '\u{25D0}', '\u{00D7}', '\u{00B7}', '\u{25B2}',
];

/// The four ghost kanji, one per mode: Queue 力, Review 鏡, Activity 動, Empty 空.
pub const KANJI_GLYPHS: [char; 4] = ['\u{529B}', '\u{955C}', '\u{52D5}', '\u{7A7A}'];

#[link(name = "fontconfig")]
unsafe extern "C" {
    /// `FcBool FcConfigAppFontAddDir(FcConfig *config, const FcChar8 *dir)`; NULL = current config.
    fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_char) -> c_int;
}

/// Where the files are written: `$XDG_CACHE_HOME/hatate-rs/fonts`, else `$HOME/.cache/...`.
pub fn default_dir<F>(get: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<String>,
{
    let base = match get("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(get("HOME").filter(|v| !v.is_empty())?).join(".cache"),
    };
    Some(base.join("hatate-rs").join("fonts"))
}

/// Writes every bundled file into `dir` (a file already there with the same bytes is left alone).
pub fn write_files(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for f in &FILES {
        let path = dir.join(f.file_name);
        if fs::read(&path).is_ok_and(|have| have == f.bytes) {
            continue;
        }
        fs::write(&path, f.bytes)?;
    }
    Ok(())
}

/// Make the fonts in `dir` visible to Pango. Call before GTK is initialised (before
/// `Application::run`): fontconfig reads its configuration once, at first use. A failure is
/// returned for the caller to log; the window then runs on the fallback faces, as Python's does
/// when a font fails to register (`register_fonts` logs and carries on).
pub fn load(dir: &Path) -> Result<(), String> {
    write_files(dir).map_err(|e| format!("font dir {}: {e}", dir.display()))?;
    let abs = dir
        .canonicalize()
        .map_err(|e| format!("font dir {}: {e}", dir.display()))?;
    let c = CString::new(abs.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call, and a NULL config
    // means "the current configuration", which fontconfig creates on demand.
    let ok = unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), c.as_ptr()) };
    if ok == 0 {
        return Err(format!("fontconfig refused {}", abs.display()));
    }
    Ok(())
}

/// `register_fonts` for the app: the default directory from the environment.
pub fn load_default() -> Result<PathBuf, String> {
    let dir = default_dir(|k| std::env::var(k).ok())
        .ok_or_else(|| "no HOME or XDG_CACHE_HOME to put the fonts under".to_owned())?;
    load(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_faces_are_bundled_and_none_is_woff2() {
        assert_eq!(FILES.len(), 9);
        for f in &FILES {
            assert!(
                f.file_name.ends_with(".ttf") || f.file_name.ends_with(".otf"),
                "{}",
                f.file_name
            );
            // sfnt magic: TrueType 0x00010000 or CFF 'OTTO'; WOFF2 would be 'wOF2'.
            assert!(
                f.bytes.starts_with(&[0, 1, 0, 0]) || f.bytes.starts_with(b"OTTO"),
                "{} is not an sfnt file",
                f.file_name
            );
        }
    }

    #[test]
    fn files_are_written_once_and_rewritten_when_damaged() {
        let dir = tempfile::tempdir().unwrap();
        write_files(dir.path()).unwrap();
        for f in &FILES {
            assert_eq!(fs::read(dir.path().join(f.file_name)).unwrap(), f.bytes);
        }
        fs::write(dir.path().join("RyokuStatus.ttf"), b"damaged").unwrap();
        write_files(dir.path()).unwrap();
        let fixed = fs::read(dir.path().join("RyokuStatus.ttf")).unwrap();
        assert_eq!(
            fixed,
            FILES
                .iter()
                .find(|f| f.file_name == "RyokuStatus.ttf")
                .unwrap()
                .bytes
        );
    }

    #[test]
    fn the_default_dir_follows_xdg_cache_home_then_home() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            default_dir(env(&[("XDG_CACHE_HOME", "/x"), ("HOME", "/h")])),
            Some(PathBuf::from("/x/hatate-rs/fonts"))
        );
        assert_eq!(
            default_dir(env(&[("HOME", "/h")])),
            Some(PathBuf::from("/h/.cache/hatate-rs/fonts"))
        );
        assert_eq!(default_dir(env(&[])), None);
    }
}
