//! The application's look: GTK CSS generated from the mockup's tokens (`gui/theme.py`,
//! `stylesheet` and `resolve_mode` at `76c85c4`).
//!
//! One ink colour at a handful of opacities, flat: no gradients, no shadows, no rounded corners.
//! The colours and lengths come from [`crate::tokens`] (the vendored `tokens.css`); the widget
//! rules below name them as `@colour` references and `%length%` placeholders, so a token changed
//! in the sheet changes the window.
//!
//! Three modes, as Python has: dark, light and follow-system. A pinned mode draws that scheme; the
//! system mode draws what the desktop asks for, and dark when it asks for nothing (Python's
//! fallback, `resolve_mode`).

use crate::tokens::{Palette, Scheme, TOKENS_CSS, Tokens};
use std::cell::RefCell;

/// `gui/theme.py: MODES`.
pub const MODES: [&str; 3] = ["system", "dark", "light"];

/// The stored `theme` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    System,
    Dark,
    Light,
}

impl Mode {
    /// Any other string is not a mode (`action_set_theme` ignores it).
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "system" => Some(Mode::System),
            "dark" => Some(Mode::Dark),
            "light" => Some(Mode::Light),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::System => "system",
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }
}

/// `resolve_mode(setting)`: a pinned mode draws itself; `system` draws what the desktop reports
/// (`Some(true)` dark, `Some(false)` light) and dark when it expresses no preference. A setting
/// that is not a mode counts as `system`, as in Python.
pub fn resolve_scheme(setting: &str, system_is_dark: Option<bool>) -> Scheme {
    match Mode::parse(setting) {
        Some(Mode::Dark) => Scheme::Dark,
        Some(Mode::Light) => Scheme::Light,
        _ => match system_is_dark {
            Some(false) => Scheme::Light,
            _ => Scheme::Dark,
        },
    }
}

/// The lengths the rules use, from the tokens.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub display: f64,
    pub title: f64,
    pub body: f64,
    pub label: f64,
    pub micro: f64,
    pub space_1: f64,
    pub space_2: f64,
    pub space_3: f64,
    pub space_4: f64,
    pub space_5: f64,
    pub space_6: f64,
    /// `--tracking-label` (0.16em) in px at the label size, for `letter-spacing`.
    pub tracking_label: f64,
}

impl Metrics {
    pub fn from_tokens(t: &Tokens) -> Metrics {
        let label = t.px("text-label");
        let tracking = t
            .raw("tracking-label")
            .and_then(|v| v.strip_suffix("em"))
            .and_then(|v| v.parse::<f64>().ok())
            .expect("--tracking-label in em");
        Metrics {
            display: t.px("text-display"),
            title: t.px("text-title"),
            body: t.px("text-body"),
            label,
            micro: t.px("text-micro"),
            space_1: t.px("space-1"),
            space_2: t.px("space-2"),
            space_3: t.px("space-3"),
            space_4: t.px("space-4"),
            space_5: t.px("space-5"),
            space_6: t.px("space-6"),
            tracking_label: tracking * label,
        }
    }
}

/// The families the Ryoku type system names, in the stacks of `tokens.css`.
pub const SERIF: &str = "'Source Serif 4', 'Iowan Old Style', 'Palatino Linotype', Georgia, serif";
pub const SANS: &str = "'Space Grotesk', 'Inter', 'Helvetica Neue', Arial, sans-serif";
pub const MONO: &str =
    "'JetBrains Mono', 'IBM Plex Mono', 'DejaVu Sans Mono', 'Courier New', monospace";
pub const KANJI: &str = "'Ryoku Kanji', 'Source Serif 4', 'Noto Serif CJK JP', serif";
pub const STATUS: &str = "'Ryoku Status', 'JetBrains Mono', monospace";

/// Widget rules. Colours are `@names` from [`Palette::define_colors`]; `%name%` is a length from
/// [`Metrics`]; `%mono%`, `%sans%`, `%serif%`, `%kanji%` and `%status%` are the font stacks.
const RULES: &str = r#"
window, window.background { background: @page; color: @ink; border-radius: 0; box-shadow: none; }
window.csd { border-radius: 0; box-shadow: none; }
/* no animation: libadwaita fades outlines and backgrounds over 200 ms, which blurs a state change
   (a focus ring caught mid-fade is half strength) and a shot taken a few frames later */
* { transition: none; animation: none; }
* { font-family: %sans%; font-size: %body%px; }
.mono, .section-label, .readout, .figure, .meta-label, .meta-figure, .keycap, .frame-caption,
.statusbar label, .banner-head, .mode-switch button, button.pill, menubar > item {
  font-family: %mono%;
}

/* an uppercase voice: the string stays sentence case, the sheet draws it in capitals */
.caps, .caps label { text-transform: uppercase; }

/* ---- the one focus ring (design gate 1.1): 2px ink; on a stamp-filled surface the ring is the
   stamp's text colour, because ink on ink is 1:1 ---- */
*:focus:focus-visible { outline: 2px solid @ink; outline-offset: 2px; }
button.pill:focus:focus-visible, .mode-switch button:focus:focus-visible { outline-offset: -4px; }
button.pill.primary:focus:focus-visible, .mode-switch button:checked:focus:focus-visible { outline-color: @stamp_fg; }
popover modelbutton:focus:focus-visible, popover row:focus:focus-visible, menubar > item:focus:focus-visible,
dialog button:focus:focus-visible { outline-offset: -2px; }
window:focus:focus-visible, popover:focus:focus-visible, popover > contents:focus:focus-visible,
dialog:focus:focus-visible, floating-sheet:focus:focus-visible, sheet:focus:focus-visible,
dialog widget:focus:focus-visible { outline: none; }

/* ---- cards: regions of a flat page, set off by a 1px hairline, not a fill (DAN-1160 / G-08) ---- */
/* libadwaita's own `.card` draws a radius-12 outline and a shadow falloff; this one is a plain region */
.card { background: transparent; border-radius: 0; box-shadow: none; }
.card.framed { border: 1px solid @ink_18; }
.section-label { font-size: %label%px; letter-spacing: %tracking_label%px; color: @ink_65; font-weight: 700; }
.section-rule { background: @ink_18; min-height: 1px; }
.heading { font-family: %serif%; font-size: %title%px; font-weight: 600; color: @ink; }
.screen-title, .display-title { font-family: %serif%; font-size: %display%px; font-weight: 600; color: @ink; }
.muted { color: @ink_65; }
.hint { color: @ink_65; font-size: %body%px; }

/* ---- the page chrome: top bar, mode switch, run strip ---- */
.mark-glyph { font-family: %kanji%; font-size: 22px; color: @ink; }
.mark-word { font-family: %mono%; font-size: %label%px; letter-spacing: %tracking_label%px; font-weight: 700; color: @ink; }
/* a control whose border is its shape takes the border tier (`ink_46`, 3:1 on the page in both
   schemes; `ink_45` is 2.68 in light) and `ink_58` on hover, as gui/theme.py does (DAN-224); cards,
   the run strip, the frame hairline and rules stay `ink_18` on purpose */
.mode-switch { border: 1px solid @ink_46; }
.mode-switch button { background: transparent; color: @ink_65; border: none; border-right: 1px solid @ink_46;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: %space_3%px %space_5%px; min-height: 0; min-width: 0;
  font-size: %label%px; letter-spacing: %tracking_label%px; font-weight: 700; }
.mode-switch button:last-child { border-right: none; }
.mode-switch button:hover { background: @ink_10; color: @ink; }
.mode-switch button:checked { background: @stamp_bg; color: @stamp_fg; }
.run-strip { background: @page; border: 1px solid @ink_18; padding: %space_3%px %space_5%px; }
.readout { font-size: %label%px; letter-spacing: 0.08em; color: @ink_65; }
.figure { font-size: %label%px; letter-spacing: 0.08em; color: @ink; font-weight: 700; }
.run-gauge trough { background: @ink_18; min-height: 2px; min-width: 220px; border-radius: 0; border: none; }
.run-gauge progress { background: @ink; min-height: 2px; border-radius: 0; border: none; }

/* ---- buttons: square, one outline weight, inversion for primary ---- */
button.pill { background: transparent; color: @ink; border: 1px solid @ink_46; border-radius: 0; box-shadow: none;
  text-shadow: none; padding: %space_3%px %space_5%px; min-height: 0; font-size: %label%px;
  letter-spacing: %tracking_label%px; font-weight: 700; }
button.pill:hover { background: @ink_10; border-color: @ink_58; }
button.pill:disabled { color: @ink_45; border-color: @ink_46; }
button.pill.primary { background: @stamp_bg; color: @stamp_fg; border-color: @ink; }
button.pill.primary:hover { background: @ink; border-color: @ink; }
button.pill.plain { text-transform: none; letter-spacing: 0; }
button.icon-boxed { background: transparent; color: @ink; border: 1px solid @ink_46; border-radius: 0; box-shadow: none;
  padding: %space_2%px %space_3%px; min-height: 0; min-width: 0; font-size: 15px; }
button.icon-boxed:hover { background: @ink_10; border-color: @ink_58; }
button.icon-boxed:disabled { color: @ink_45; border-color: @ink_46; }
button.link { background: none; border: none; box-shadow: none; padding: 0; min-height: 0; min-width: 0;
  color: @ink_65; font-family: %mono%; font-size: %label%px; letter-spacing: 0.06em; text-decoration: underline; }
button.link:hover { color: @ink; }

/* ---- keycaps (the mockup's .kbd) and the key-hint bar ---- */
.keycap { border: 1px solid @ink_28; color: @ink_65; padding: 0 5px; font-size: %micro%px; letter-spacing: 0.08em; }
button.pill.primary .keycap { border-color: @stamp_fg; color: @stamp_fg; }
.hint-bar label { font-family: %mono%; font-size: %label%px; letter-spacing: 0.06em; color: @ink_65; }
.hint-bar .strong { color: @ink; font-weight: 700; }

/* ---- chips and disclosure ---- */
.chip { font-family: %mono%; font-size: %micro%px; letter-spacing: 0.08em; padding: 1px 6px; border: 1px solid @ink_18; color: @ink_45; }
.chip.on { border-color: @ink_28; color: @ink_65; }
button.disclosure { background: none; border: none; box-shadow: none; padding: 0; min-height: 0; color: @ink_65;
  font-family: %mono%; font-size: %label%px; letter-spacing: %tracking_label%px; font-weight: 700; }

/* ---- frames: hairline box with four crop marks, and the caption under it ---- */
.frame { background: @raised; border: 1px solid @ink_18; border-radius: 0; }
.corner { min-width: 12px; min-height: 12px; margin: -1px; }
.corner.tl { border-top: 2px solid @ink_65; border-left: 2px solid @ink_65; }
.corner.tr { border-top: 2px solid @ink_65; border-right: 2px solid @ink_65; }
.corner.bl { border-bottom: 2px solid @ink_65; border-left: 2px solid @ink_65; }
.corner.br { border-bottom: 2px solid @ink_65; border-right: 2px solid @ink_65; }
.frame-caption { font-size: %label%px; letter-spacing: 0.08em; color: @ink_65; }

/* ---- the ghost kanji: ink-05, 420px, bleeding off the top-right (design gate 2.9) ---- */
.ghost-kanji { font-family: %kanji%; font-size: 420px; line-height: 1; color: @ink_05; margin: -40px 40px 0 0; }

/* ---- banners (Crashed, quota pause) and the empty state ---- */
.banner { background: @raised; border: 1px solid @ink_18; padding: %space_5%px; }
.banner-glyph { font-family: %status%; font-size: 17px; color: @ink; }
.banner-head { font-size: %label%px; letter-spacing: %tracking_label%px; font-weight: 700; color: @ink; }
.banner-body { color: @ink_65; font-size: %body%px; }
.meta-label { font-size: %label%px; letter-spacing: 0.08em; color: @ink_65; }
.meta-figure { font-size: %label%px; letter-spacing: 0.08em; color: @ink; font-weight: 700; }
.meta-figure.quiet { color: @ink_65; font-weight: 400; }
.drop-zone { background: @raised; border: 1px solid @ink_18; }
.drop-copy { color: @ink_65; font-size: %body%px; }
.survived-glyph { border: 1px solid @ink_28; min-width: 42px; min-height: 42px; font-size: 32px; color: @ink; }

/* ---- menubar and status bar ---- */
menubar { background: @page; color: @ink; border-bottom: 1px solid @ink_18; padding: 0 %space_3%px; min-height: 27px; }
menubar > item { border-radius: 0; padding: 4px %space_3%px; min-height: 0; color: @ink_65;
  font-size: %label%px; letter-spacing: 0.08em; }
menubar > item:hover, menubar > item:checked, menubar > item:selected { background: @stamp_bg; color: @stamp_fg; }
.statusbar { background: @page; border-top: 1px solid @ink_18; min-height: 23px; padding: 0 %space_6%px; }
.statusbar label { font-size: %label%px; letter-spacing: 0.06em; color: @ink_65; }

/* ---- popovers: square, shadowless, hairline edge ---- */
popover, popover.background, popover.menu { background: transparent; box-shadow: none; border-radius: 0;
  margin: 0; padding: 0; border: none; }
popover > contents, popover.background > contents, popover.menu > contents {
  background: @raised; color: @ink; border: 1px solid @ink_28; border-radius: 0; box-shadow: none; padding: 0; }
popover > arrow { background: transparent; border: none; box-shadow: none; min-width: 0; min-height: 0; }
popover modelbutton, popover.menu modelbutton { border-radius: 0; min-height: 0; padding: %space_2%px %space_3%px; color: @ink; }
popover modelbutton:hover, popover modelbutton:focus { background: @ink_10; }
popover modelbutton:disabled { color: @ink_45; }
popover modelbutton check, popover modelbutton radio { border-radius: 0; box-shadow: none; }
popover separator { background: @ink_18; min-height: 1px; margin: 0; }
popover label, popover modelbutton label { font-family: %mono%; font-size: 12px; }
popover modelbutton accelerator, popover modelbutton label.accelerator { color: @ink_65; }
/* ink_65 on the hover wash is 4.25:1 in dark; a hovered or focused row reads its hint in ink */
popover modelbutton:hover accelerator, popover modelbutton:hover label.accelerator,
popover modelbutton:focus accelerator, popover modelbutton:focus label.accelerator { color: @ink; }

/* ---- dialogs (libadwaita AlertDialog) flattened; the destructive response is the stamp, and the
   default response is Cancel (design gate 2.6) ---- */
dialog.floating, dialog.floating.alert, window.dialog, window.messagedialog, .alert { border-radius: 0; box-shadow: none; }
dialog.alert sheet, dialog.floating sheet, floating-sheet > sheet, sheet { background: @raised; color: @ink; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; }
dialog.alert .title-2, dialog.alert .heading, .alert .heading { font-family: %serif%; font-weight: 600; color: @ink; }
dialog.alert .body { color: @ink_65; }
dialog.alert button, dialog.alert button.suggested-action, dialog.alert button.destructive,
dialog.alert .response-area button { background: transparent; color: @ink; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: %space_2%px %space_4%px; min-height: 0;
  font-family: %mono%; font-size: %label%px; letter-spacing: %tracking_label%px; }
dialog.alert button.destructive { background: @stamp_bg; color: @stamp_fg; border-color: @stamp_bg; }
dialog.alert button.destructive label { color: @stamp_fg; }
dialog.alert button:hover { background: @ink_10; }
dialog.alert button.destructive:hover { background: @ink; }
dialog.alert separator { background: @ink_18; min-width: 1px; min-height: 1px; }

/* ---- the log viewer ---- */
textview.log, textview.log text { background: @sunken; color: @ink; font-family: %mono%; font-size: 12px; }
.log-path { font-family: %mono%; font-size: %label%px; color: @ink_65; }

/* scrolled windows draw soft edge gradients where content continues; flat has none */
undershoot, overshoot, undershoot.top, undershoot.bottom, undershoot.left, undershoot.right {
  background: none; background-image: none; box-shadow: none; }
scrollbar slider { background: @ink_28; border-radius: 0; min-width: 6px; min-height: 6px; }
scrollbar, scrollbar trough { background: transparent; }
"#;

/// The rules with `%name%` replaced by the token values.
fn rules(p: &Palette, m: &Metrics) -> String {
    let _ = p;
    let num = |v: f64| {
        if v.fract() == 0.0 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        }
    };
    let subs: [(&str, String); 16] = [
        ("%display%", num(m.display)),
        ("%title%", num(m.title)),
        ("%body%", num(m.body)),
        ("%label%", num(m.label)),
        ("%micro%", num(m.micro)),
        ("%space_1%", num(m.space_1)),
        ("%space_2%", num(m.space_2)),
        ("%space_3%", num(m.space_3)),
        ("%space_4%", num(m.space_4)),
        ("%space_5%", num(m.space_5)),
        ("%space_6%", num(m.space_6)),
        ("%tracking_label%", format!("{:.2}", m.tracking_label)),
        ("%mono%", MONO.to_owned()),
        ("%sans%", SANS.to_owned()),
        ("%serif%", SERIF.to_owned()),
        ("%kanji%", KANJI.to_owned()),
    ];
    let mut out = RULES.to_owned();
    for (k, v) in subs {
        out = out.replace(k, &v);
    }
    out.replace("%status%", STATUS)
}

/// The whole sheet for a scheme, from `tokens_css` (the vendored sheet in production).
pub fn css_from(tokens_css: &str, scheme: Scheme) -> String {
    let t = Tokens::parse(tokens_css, scheme);
    let p = Palette::from_tokens(scheme, &t);
    let m = Metrics::from_tokens(&t);
    format!("{}{}", p.define_colors(), rules(&p, &m))
}

/// The whole sheet for a scheme.
pub fn css(scheme: Scheme) -> String {
    css_from(TOKENS_CSS, scheme)
}

thread_local! {
    static INSTALLED: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Install the sheet for the default display at application priority, above libadwaita's own
/// stylesheet, replacing the one installed before, and tell libadwaita which scheme to base its
/// own rules on. `action_set_theme`: nothing is rebuilt; every widget comes back in the new
/// palette.
pub fn install(scheme: Scheme) {
    libadwaita::StyleManager::default().set_color_scheme(match scheme {
        Scheme::Dark => libadwaita::ColorScheme::ForceDark,
        Scheme::Light => libadwaita::ColorScheme::ForceLight,
    });
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    INSTALLED.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            gtk4::style_context_remove_provider_for_display(&display, &old);
        }
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(&css(scheme));
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        *slot.borrow_mut() = Some(provider);
    });
}

/// What the desktop asks for, or `None` when it asks for nothing. Reads libadwaita's own
/// colour-scheme state with the app's override lifted for the duration of the read.
pub fn system_prefers_dark() -> Option<bool> {
    let manager = libadwaita::StyleManager::default();
    if !manager.system_supports_color_schemes() {
        return None;
    }
    let before = manager.color_scheme();
    manager.set_color_scheme(libadwaita::ColorScheme::Default);
    let dark = manager.is_dark();
    manager.set_color_scheme(before);
    Some(dark)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::contrast;

    #[test]
    fn every_named_colour_used_is_defined() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            for token in css.split('@').skip(1) {
                let name: String = token
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name == "define" {
                    continue;
                }
                assert!(
                    css.contains(&format!("@define-color {name} ")),
                    "undefined colour @{name} in {scheme:?}"
                );
            }
        }
    }

    #[test]
    fn no_placeholder_is_left_in_the_sheet() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            assert!(!css.contains('%'), "unreplaced placeholder in {scheme:?}");
        }
    }

    #[test]
    fn the_sheet_follows_the_tokens() {
        let edited = TOKENS_CSS.replacen("--space-5: 24px;", "--space-5: 30px;", 1);
        assert!(css_from(&edited, Scheme::Dark).contains("padding: 12px 30px"));
        assert!(css(Scheme::Dark).contains("padding: 12px 24px"));
    }

    /// `resolve_mode` (`gui/theme.py:303`): pinned modes pin; `system` follows the desktop and
    /// falls back to dark; anything else is `system`.
    #[test]
    fn resolve_scheme_matches_resolve_mode() {
        for (setting, system, want) in [
            ("dark", Some(false), Scheme::Dark),
            ("light", Some(true), Scheme::Light),
            ("system", Some(true), Scheme::Dark),
            ("system", Some(false), Scheme::Light),
            ("system", None, Scheme::Dark),
            ("bogus", Some(false), Scheme::Light),
            ("", None, Scheme::Dark),
        ] {
            assert_eq!(
                resolve_scheme(setting, system),
                want,
                "{setting} {system:?}"
            );
        }
    }

    #[test]
    fn modes_are_the_three_python_has() {
        assert_eq!(MODES, ["system", "dark", "light"]);
        for m in MODES {
            assert_eq!(Mode::parse(m).unwrap().name(), m);
        }
        assert_eq!(Mode::parse("sepia"), None);
    }

    #[test]
    fn the_ring_clears_three_to_one_on_every_surface_it_sits_on() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            for (name, surface) in [
                ("page", p.page),
                ("raised", p.raised),
                ("sunken", p.sunken),
                ("zebra", p.zebra()),
                ("hover", p.ink_10.over(p.raised)),
                ("stamp", p.stamp_bg),
            ] {
                let c = contrast(p.ring_on(surface), surface);
                assert!(c >= 3.0, "{scheme:?} ring on {name}: {c:.2}:1");
            }
        }
    }

    #[test]
    fn a_plain_ink_ring_would_vanish_on_the_stamp() {
        let p = Palette::of(Scheme::Dark);
        assert!(contrast(p.ink, p.stamp_bg) < 1.01);
    }

    #[test]
    fn body_text_tiers_stay_above_four_and_a_half() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            for surface in [p.page, p.raised, p.zebra()] {
                let c = contrast(p.ink_65.over(surface), surface);
                assert!(c >= 4.5, "{scheme:?} ink-65 on {}: {c:.2}", surface.hex());
            }
        }
    }
}
