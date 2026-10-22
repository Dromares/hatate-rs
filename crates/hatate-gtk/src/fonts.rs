//! App-font loading from a directory given at run time (flip condition (a), DAN-1983).
//!
//! GTK4 has no app-font API of its own: text goes through Pango, Pango's fontconfig backend asks
//! the process-wide fontconfig configuration, so loading a font means changing what fontconfig
//! sees *before* the first font lookup. Three routes are implemented and measured:
//!
//! * [`Route::FcAddDir`]: `FcConfigAppFontAddDir` on the current config, one `unsafe` FFI call.
//!   Needs the directory at run time and nothing else; fonts are private to the process.
//! * [`Route::ConfFile`]: write a small `fonts.conf` that includes the system one and adds
//!   `<dir>` with the **absolute** path, point `FONTCONFIG_FILE` at it before fontconfig starts.
//!   No unsafe FFI, but it needs `set_var` (unsafe in edition 2024) or a re-exec, and the
//!   absolute path (a relative `<dir>` is ignored by old fontconfig, see DAN-1937).
//! * [`Route::XdgDataHome`]: point `XDG_DATA_HOME` at a directory whose `fonts/` holds the
//!   files. Fontconfig scans `$XDG_DATA_HOME/fonts` by default. It moves the app's whole XDG
//!   data home, so it is a test route, not a design.
//!
//! Pango's own `pango_font_map_add_font_file` is not an option on the target: it needs
//! Pango >= 1.56 and Ubuntu 24.04 ships 1.52.1 (the symbol is absent from its libpango).

use gtk4::pango;
use gtk4::prelude::*;
use std::ffi::{CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

/// The families the Ryoku type system names.
pub const FAMILIES: [&str; 4] = [
    "Space Grotesk",
    "JetBrains Mono",
    "Source Serif 4",
    "Ryoku Status",
];

/// The six status glyphs, in the order Good, Not found, Review, Error, Unsearched, Flagged.
pub const STATUS_GLYPHS: [char; 6] = [
    '\u{25CF}', '\u{25CB}', '\u{25D0}', '\u{00D7}', '\u{00B7}', '\u{25B2}',
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Load nothing: the control run (fallback faces only).
    None,
    FcAddDir,
    ConfFile,
    XdgDataHome,
}

impl Route {
    pub fn parse(s: &str) -> Option<Route> {
        match s {
            "none" => Some(Route::None),
            "fc-add-dir" => Some(Route::FcAddDir),
            "conf-file" => Some(Route::ConfFile),
            "xdg-data-home" => Some(Route::XdgDataHome),
            _ => None,
        }
    }
}

#[link(name = "fontconfig")]
unsafe extern "C" {
    /// `FcBool FcConfigAppFontAddDir(FcConfig *config, const FcChar8 *dir)`; NULL = current config.
    fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_char) -> c_int;
}

/// Make the fonts in `dir` visible to Pango via `route`. Call before GTK is initialised (before
/// `Application::run`): fontconfig reads its configuration once, at first use.
pub fn load(dir: &Path, route: Route) -> Result<(), String> {
    if route == Route::None {
        return Ok(());
    }
    let abs = dir
        .canonicalize()
        .map_err(|e| format!("font dir {}: {e}", dir.display()))?;
    match route {
        Route::None => Ok(()),
        Route::FcAddDir => {
            let c = CString::new(abs.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            // SAFETY: `c` is a valid NUL-terminated string that outlives the call, and a NULL
            // config means "the current configuration", which fontconfig creates on demand.
            let ok = unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), c.as_ptr()) };
            if ok == 0 {
                Err(format!(
                    "FcConfigAppFontAddDir({}) returned FcFalse",
                    abs.display()
                ))
            } else {
                Ok(())
            }
        }
        Route::ConfFile => {
            let conf_dir =
                std::env::temp_dir().join(format!("hatate-fonts-{}", std::process::id()));
            std::fs::create_dir_all(&conf_dir).map_err(|e| e.to_string())?;
            let conf = conf_dir.join("fonts.conf");
            std::fs::write(&conf, conf_xml(&abs, &conf_dir.join("cache")))
                .map_err(|e| e.to_string())?;
            // SAFETY: called from `main` before GTK or any other thread exists.
            unsafe { std::env::set_var("FONTCONFIG_FILE", &conf) };
            Ok(())
        }
        Route::XdgDataHome => {
            let home = std::env::temp_dir().join(format!("hatate-xdg-{}", std::process::id()));
            std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
            let link = home.join("fonts");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&abs, &link).map_err(|e| e.to_string())?;
            // SAFETY: called from `main` before GTK or any other thread exists.
            unsafe { std::env::set_var("XDG_DATA_HOME", &home) };
            Ok(())
        }
    }
}

/// The `fonts.conf` of [`Route::ConfFile`].
pub fn conf_xml(dir: &Path, cache: &Path) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n<fontconfig>\n  \
         <include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>\n  \
         <dir>{}</dir>\n  <cachedir>{}</cachedir>\n</fontconfig>\n",
        dir.display(),
        cache.display()
    )
}

/// What Pango resolved for one family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyReport {
    pub family: &'static str,
    pub listed: bool,
    pub resolved: String,
}

impl FamilyReport {
    /// The face Pango actually picked is the requested one.
    pub fn loaded(&self) -> bool {
        self.listed && self.resolved.eq_ignore_ascii_case(self.family)
    }
}

/// Which families Pango lists, what `family` resolves to, and the glyph coverage of the
/// resolved `Ryoku Status`. Needs GTK initialised.
pub struct Report {
    pub families: Vec<FamilyReport>,
    pub n_families: usize,
    /// Per status glyph: does the face Pango picked for "Ryoku Status" have it, and is that
    /// face really Ryoku Status.
    pub glyphs: Vec<(char, bool)>,
    pub status_face: String,
}

pub fn report() -> Report {
    let ctx = gtk4::Label::new(None).pango_context();
    let map = ctx.font_map();
    let listed: Vec<String> = map
        .as_ref()
        .map(|m| {
            m.list_families()
                .iter()
                .map(|f| f.name().to_string())
                .collect()
        })
        .unwrap_or_default();
    let resolve = |family: &str, weight: pango::Weight| -> (String, Option<pango::Font>) {
        let mut desc = pango::FontDescription::new();
        desc.set_family(family);
        desc.set_weight(weight);
        desc.set_size(12 * pango::SCALE);
        match ctx.load_font(&desc) {
            Some(font) => (
                font.describe()
                    .family()
                    .map(|f| f.to_string())
                    .unwrap_or_default(),
                Some(font),
            ),
            None => (String::new(), None),
        }
    };
    let families = FAMILIES
        .iter()
        .map(|f| FamilyReport {
            family: f,
            listed: listed.iter().any(|l| l.eq_ignore_ascii_case(f)),
            resolved: resolve(f, pango::Weight::Normal).0,
        })
        .collect();
    let (status_face, font) = resolve("Ryoku Status", pango::Weight::Normal);
    let glyphs = STATUS_GLYPHS
        .iter()
        .map(|g| (*g, font.as_ref().is_some_and(|f| f.has_char(*g))))
        .collect();
    Report {
        families,
        n_families: listed.len(),
        glyphs,
        status_face,
    }
}

impl Report {
    /// `FONT ...` lines for the run log.
    pub fn lines(&self, route: Route) -> Vec<String> {
        let mut out = vec![format!(
            "FONT route={route:?} pango_families_listed={}",
            self.n_families
        )];
        for f in &self.families {
            out.push(format!(
                "FONT family=\"{}\" listed={} resolved=\"{}\" loaded={}",
                f.family,
                f.listed,
                f.resolved,
                f.loaded()
            ));
        }
        let have: String = self
            .glyphs
            .iter()
            .filter(|(_, ok)| *ok)
            .map(|(g, _)| *g)
            .collect();
        out.push(format!(
            "FONT status_face=\"{}\" glyphs_present={}/{} [{have}]",
            self.status_face,
            self.glyphs.iter().filter(|(_, ok)| *ok).count(),
            self.glyphs.len()
        ));
        out
    }

    /// All four families resolved to themselves and all six glyphs are in the status face.
    pub fn all_loaded(&self) -> bool {
        self.families.iter().all(FamilyReport::loaded) && self.glyphs.iter().all(|(_, ok)| *ok)
    }
}

/// The family files the spike expects under a font directory, for the run log.
pub fn list_font_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(list_font_files(&p));
            } else if p
                .extension()
                .is_some_and(|x| matches!(x.to_str(), Some("ttf" | "otf" | "woff2" | "woff")))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_xml_names_the_absolute_dir_and_includes_the_system_config() {
        let x = conf_xml(Path::new("/opt/fonts"), Path::new("/tmp/c"));
        assert!(x.contains("<dir>/opt/fonts</dir>"));
        assert!(x.contains("/etc/fonts/fonts.conf"));
        assert!(x.contains("<cachedir>/tmp/c</cachedir>"));
    }

    #[test]
    fn route_names_round_trip() {
        assert_eq!(Route::parse("fc-add-dir"), Some(Route::FcAddDir));
        assert_eq!(Route::parse("conf-file"), Some(Route::ConfFile));
        assert_eq!(Route::parse("xdg-data-home"), Some(Route::XdgDataHome));
        assert_eq!(Route::parse("none"), Some(Route::None));
        assert_eq!(Route::parse("nope"), None);
    }

    #[test]
    fn a_missing_directory_is_an_error_not_a_panic() {
        let e = load(Path::new("/nonexistent/hatate-fonts"), Route::FcAddDir).unwrap_err();
        assert!(e.contains("/nonexistent/hatate-fonts"));
    }

    #[test]
    fn glyph_list_is_the_six_status_glyphs() {
        let s: String = STATUS_GLYPHS.iter().collect();
        assert_eq!(s, "●○◐×·▲");
    }
}
