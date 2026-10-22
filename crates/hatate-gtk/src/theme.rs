//! Ryoku theme as GTK CSS, transcribed from the mockup's `tokens.css`
//! (`Dromares/hatate-ryoku-mockup@fff6302`, dark and `[data-theme="light"]`). One ink colour at
//! a handful of opacities, flat, no gradients, no shadows, no rounded corners. Spacing uses the
//! mockup's scale (4, 8, 12, 16, 24, 32 px).
//!
//! The palette is data, not a CSS string, so the contrast numbers in the tests and in the
//! `--audit` run are computed from the same values the stylesheet is generated from.

/// One colour: sRGB channels 0..=255 and an alpha 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba(r as f64, g as f64, b as f64, 1.0)
    }

    /// `self` composited over an opaque `bg`.
    pub fn over(self, bg: Rgba) -> Rgba {
        let a = self.3;
        Rgba(
            self.0 * a + bg.0 * (1.0 - a),
            self.1 * a + bg.1 * (1.0 - a),
            self.2 * a + bg.2 * (1.0 - a),
            1.0,
        )
    }

    pub fn css(self) -> String {
        if self.3 >= 1.0 {
            format!(
                "rgb({},{},{})",
                self.0.round(),
                self.1.round(),
                self.2.round()
            )
        } else {
            format!(
                "rgba({},{},{},{})",
                self.0.round(),
                self.1.round(),
                self.2.round(),
                self.3
            )
        }
    }

    pub fn hex(self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            self.0.round() as u8,
            self.1.round() as u8,
            self.2.round() as u8
        )
    }
}

/// WCAG 2.x relative luminance of an opaque colour.
fn luminance(c: Rgba) -> f64 {
    let lin = |v: f64| {
        let v = v / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG 2.x contrast ratio between two opaque colours.
pub fn contrast(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    Light,
}

impl Scheme {
    pub fn parse(s: &str) -> Option<Scheme> {
        match s {
            "dark" => Some(Scheme::Dark),
            "light" => Some(Scheme::Light),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scheme::Dark => "dark",
            Scheme::Light => "light",
        }
    }
}

/// The surfaces and ink tiers of one scheme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub page: Rgba,
    pub raised: Rgba,
    pub sunken: Rgba,
    /// Ink at full strength (`--ink`).
    pub ink: Rgba,
    pub ink_65: Rgba,
    pub ink_45: Rgba,
    pub ink_28: Rgba,
    pub ink_18: Rgba,
    pub ink_10: Rgba,
    pub ink_06: Rgba,
    pub stamp_bg: Rgba,
    pub stamp_fg: Rgba,
}

fn with_alpha(c: Rgba, a: f64) -> Rgba {
    Rgba(c.0, c.1, c.2, a)
}

pub fn palette(scheme: Scheme) -> Palette {
    match scheme {
        Scheme::Dark => {
            let ink = Rgba::rgb(205, 196, 186);
            Palette {
                page: Rgba::rgb(0x0d, 0x0d, 0x0d),
                raised: Rgba::rgb(0x14, 0x14, 0x14),
                sunken: Rgba::rgb(0x09, 0x09, 0x09),
                ink,
                ink_65: with_alpha(ink, 0.65),
                ink_45: with_alpha(ink, 0.45),
                ink_28: with_alpha(ink, 0.28),
                ink_18: with_alpha(ink, 0.18),
                ink_10: with_alpha(ink, 0.10),
                ink_06: with_alpha(ink, 0.06),
                stamp_bg: ink,
                stamp_fg: Rgba::rgb(0x0d, 0x0d, 0x0d),
            }
        }
        Scheme::Light => {
            let ink = Rgba::rgb(0x21, 0x1d, 0x17);
            Palette {
                page: Rgba::rgb(0xe8, 0xe1, 0xd2),
                raised: Rgba::rgb(0xf2, 0xed, 0xe1),
                sunken: Rgba::rgb(0xdd, 0xd2, 0xbd),
                ink,
                ink_65: with_alpha(ink, 0.70),
                ink_45: with_alpha(ink, 0.50),
                ink_28: with_alpha(ink, 0.45),
                ink_18: with_alpha(ink, 0.32),
                ink_10: with_alpha(ink, 0.12),
                ink_06: with_alpha(ink, 0.07),
                stamp_bg: ink,
                stamp_fg: Rgba::rgb(0xf6, 0xf1, 0xe6),
            }
        }
    }
}

impl Palette {
    /// Zebra row: `ink-06` over the raised list surface.
    pub fn zebra(&self) -> Rgba {
        self.ink_06.over(self.raised)
    }

    /// The colour of the focus ring on a given surface: ink, except on the stamp-filled
    /// (selected) row, where ink is the fill itself and the ring takes the stamp's text colour.
    pub fn ring_on(&self, surface: Rgba) -> Rgba {
        if surface == self.stamp_bg {
            self.stamp_fg
        } else {
            self.ink
        }
    }

    /// `@define-color` block.
    pub fn tokens(&self) -> String {
        let defs = [
            ("page", self.page),
            ("raised", self.raised),
            ("sunken", self.sunken),
            ("zebra", self.zebra()),
            ("ink", self.ink),
            ("ink_65", self.ink_65),
            ("ink_45", self.ink_45),
            ("ink_28", self.ink_28),
            ("ink_18", self.ink_18),
            ("ink_10", self.ink_10),
            ("stamp_bg", self.stamp_bg),
            ("stamp_fg", self.stamp_fg),
        ];
        defs.iter()
            .map(|(n, c)| format!("@define-color {n} {};\n", c.css()))
            .collect()
    }
}

/// Widget rules. Colours are `@names` from [`Palette::tokens`].
///
/// Fonts: the stacks name the mockup's families first. They only resolve when `fonts::load`
/// has added them; otherwise the fallbacks are what is installed.
pub const RULES: &str = "
window, window.background { background: @page; color: @ink; border-radius: 0; box-shadow: none; }
window.csd { border-radius: 0; box-shadow: none; }
* { font-family: 'Space Grotesk', 'Inter', 'Helvetica Neue', Arial, sans-serif; }
.mono, .eyebrow, .tab, .mode, .status-word, columnview > header button, .stat, .hint, .filter-label {
  font-family: 'JetBrains Mono', 'IBM Plex Mono', 'DejaVu Sans Mono', monospace;
}
.page { padding: 0 32px 16px 32px; }
.topbar { padding: 16px 32px 12px 32px; }
.wordmark { font-weight: 700; letter-spacing: 0.16em; font-size: 11px; color: @ink; }
.title { font-family: 'Source Serif 4', 'Iowan Old Style', 'Palatino Linotype', Georgia, serif;
         font-size: 36px; font-weight: 600; color: @ink; margin: 16px 0 16px 0; }
.eyebrow { font-size: 11px; letter-spacing: 0.16em; color: @ink_65; }
.stat { font-size: 11px; color: @ink_65; }
.stat.strong { color: @ink; font-weight: 700; }
.hint { font-size: 11px; color: @ink_65; }
.note { font-size: 13px; color: @ink_65; }
.filter-label { font-size: 11px; letter-spacing: 0.16em; color: @ink_65; }

/* the one focus ring: 2px ink, inset on rows (a row's neighbours would paint over an outer
   ring), outset elsewhere. On the stamp-filled selected row the ring is the stamp's text colour. */
*:focus-visible { outline: 2px solid @ink; outline-offset: 2px; }
columnview.queue > listview > row:focus-visible,
columnview.queue > listview > row:focus-within:focus-visible { outline-offset: -2px; }
columnview.queue > listview > row:selected:focus-visible { outline-color: @stamp_fg; }
.canvas:focus-visible { outline-offset: -2px; }
popover modelbutton:focus-visible, popover row:focus-visible, popovermenubar > item:focus-visible,
popover listview > row:focus-visible, popover button:focus-visible { outline-offset: -2px; }

/* tabs and mode toggles: hairline frame, the active one is the stamp (fg/bg swap) */
button.tab, button.mode { background: transparent; color: @ink_65; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: 8px 16px; min-height: 0;
  font-size: 11px; letter-spacing: 0.16em; }
button.tab:hover, button.mode:hover { background: @ink_10; }
button.tab:checked, button.mode:checked { background: @stamp_bg; color: @stamp_fg; border-color: @stamp_bg; }

/* the queue */
.queue-frame { background: @raised; border: 1px solid @ink_18; border-radius: 0; }
columnview.queue, columnview.queue > listview { background: transparent; color: @ink; }
columnview.queue > header { background: transparent; border-bottom: 1px solid @ink_28; }
columnview.queue > header > button { background: transparent; color: @ink_65; border: none;
  border-radius: 0; box-shadow: none; padding: 8px 12px; font-size: 11px; letter-spacing: 0.16em; }
columnview.queue > header > button:hover { background: @ink_10; }
columnview.queue > header > button label { color: @ink_65; opacity: 1; }
columnview.queue > listview > row { background: transparent; border-radius: 0; min-height: 48px; }
columnview.queue > listview > row:nth-child(even) { background: @zebra; }
columnview.queue > listview > row:hover { background: @ink_10; }
columnview.queue > listview > row:selected { background: @stamp_bg; color: @stamp_fg; }
columnview.queue > listview > row > cell { padding: 4px 12px; }
columnview.queue > listview > row:selected label { color: @stamp_fg; }
.thumb { border: 1px solid @ink_45; background: @sunken; border-radius: 0; }
columnview.queue > listview > row:selected .thumb { border-color: @stamp_fg; background: @stamp_bg; }

/* status column: the glyph from the 'Ryoku Status' subset, the word in the mono face. Weight
   tiers per tokens.css: good and unsearched recede (ink-65), the rest are ink-100 bold. */
.status-glyph { font-family: 'Ryoku Status', 'JetBrains Mono', monospace; font-size: 14px; min-width: 18px; }
.status-word { font-size: 12px; }
.status-good, .status-unsearched { color: @ink_65; font-weight: 500; }
.status-review, .status-notfound, .status-flagged, .status-error { color: @ink; font-weight: 700; }
.status-error .status-word { border: 1px solid @ink_28; padding: 1px 6px; }
columnview.queue > listview > row:selected .status-glyph,
columnview.queue > listview > row:selected .status-word { color: @stamp_fg; }
columnview.queue > listview > row:selected .status-error .status-word { border-color: @stamp_fg; }
scrollbar slider { background: @ink_28; border-radius: 0; min-width: 6px; min-height: 6px; }
scrollbar, scrollbar trough { background: transparent; }

/* review */
.canvas { background: @sunken; border: 1px solid @ink_18; border-radius: 0; }
scale trough { background: @ink_18; min-height: 3px; border-radius: 0; }
scale highlight { background: @ink_28; border-radius: 0; }
scale slider { background: @ink; margin: -7px 0; min-width: 4px; min-height: 17px; border-radius: 0; border: none; box-shadow: none; }
.error-text { color: @ink; }

/* filter bar: the dropdown button is a hairline box like the tabs */
dropdown > button, dropdown > button.toggle { background: @sunken; color: @ink; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: 6px 12px; min-height: 0;
  font-family: 'JetBrains Mono', monospace; font-size: 12px; }
dropdown > button:hover { background: @ink_10; }

/* popovers (dropdown list, menubar menus, context menus): square, shadowless, hairline edge */
popover, popover.background, popover.menu { background: transparent; box-shadow: none; border-radius: 0; }
popover > contents, popover.background > contents, popover.menu > contents {
  background: @raised; color: @ink; border: 1px solid @ink_28; border-radius: 0; box-shadow: none; padding: 0; }
popover > arrow { background: transparent; border: none; box-shadow: none; min-width: 0; min-height: 0; }
popover modelbutton, popover.menu modelbutton { border-radius: 0; min-height: 0; padding: 8px 12px; color: @ink; }
popover modelbutton:hover, popover modelbutton:focus { background: @ink_10; }
popover modelbutton check, popover modelbutton radio { border-radius: 0; box-shadow: none; }
popover separator { background: @ink_18; min-height: 1px; margin: 0; }
popover listview, popover listview > row, popover scrolledwindow { background: transparent; border-radius: 0; color: @ink; }
popover listview > row { padding: 8px 12px; min-height: 0; }
popover listview > row:hover { background: @ink_10; }
popover listview > row:selected { background: @stamp_bg; color: @stamp_fg; }
popover listview > row:selected label { color: @stamp_fg; }
popover label, popover modelbutton label { font-family: 'JetBrains Mono', monospace; font-size: 12px; }

/* menubar */
popovermenubar { background: @page; color: @ink; border-bottom: 1px solid @ink_18; padding: 0 24px; }
popovermenubar > item { border-radius: 0; padding: 6px 12px; min-height: 0; color: @ink_65;
  font-family: 'JetBrains Mono', monospace; font-size: 11px; letter-spacing: 0.16em; }
popovermenubar > item:hover, popovermenubar > item:checked, popovermenubar > item:selected { background: @stamp_bg; color: @stamp_fg; }

/* dialog (libadwaita AlertDialog): floating card flattened, buttons as hairline boxes */
dialog.floating, dialog.floating.alert, window.dialog, window.messagedialog, .alert { border-radius: 0; box-shadow: none; }
dialog.alert sheet, dialog.floating sheet, floating-sheet > sheet, sheet { background: @raised; color: @ink; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; }
dialog.alert .heading, dialog.alert .title-2, .alert .heading { font-family: 'Source Serif 4', Georgia, serif; font-weight: 600; color: @ink; }
dialog.alert .body { color: @ink_65; }
dialog.alert button, dialog.alert button.suggested-action, dialog.alert button.destructive-action,
dialog.alert .response-area button { background: transparent; color: @ink; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: 8px 16px; min-height: 0;
  font-family: 'JetBrains Mono', monospace; font-size: 11px; letter-spacing: 0.16em; }
dialog.alert button.destructive-action { background: @stamp_bg; color: @stamp_fg; border-color: @stamp_bg; }
dialog.alert button:hover { background: @ink_10; }
dialog.alert button.destructive-action:hover { background: @ink; }
";

/// The whole sheet for a scheme.
pub fn css(scheme: Scheme) -> String {
    format!("{}{RULES}", palette(scheme).tokens())
}

/// Install the sheet for the default display at application priority, above libadwaita's own
/// stylesheet, and tell libadwaita which scheme to base its own rules on.
pub fn install(scheme: Scheme) {
    libadwaita::StyleManager::default().set_color_scheme(match scheme {
        Scheme::Dark => libadwaita::ColorScheme::ForceDark,
        Scheme::Light => libadwaita::ColorScheme::ForceLight,
    });
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css(scheme));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn contrast_matches_the_published_ratios() {
        let d = palette(Scheme::Dark);
        // tokens.css: ink-100 is 11.3:1 on the page, ink-65 is 5.25:1.
        assert!((contrast(d.ink, d.page) - 11.3).abs() < 0.1);
        assert!((contrast(d.ink_65.over(d.page), d.page) - 5.25).abs() < 0.15);
    }

    #[test]
    fn the_ring_clears_three_to_one_on_every_surface_it_sits_on() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = palette(scheme);
            for (name, surface) in [
                ("page", p.page),
                ("raised", p.raised),
                ("sunken", p.sunken),
                ("zebra", p.zebra()),
                ("hover", p.ink_10.over(p.raised)),
                ("selected", p.stamp_bg),
            ] {
                let ring = p.ring_on(surface);
                let c = contrast(ring, surface);
                assert!(c >= 3.0, "{scheme:?} ring on {name}: {c:.2}:1");
            }
        }
    }

    #[test]
    fn a_plain_ink_ring_would_vanish_on_the_selected_row() {
        // The reason `ring_on` exists: ink on the ink-filled stamp is 1:1.
        let p = palette(Scheme::Dark);
        assert!(contrast(p.ink, p.stamp_bg) < 1.01);
    }

    #[test]
    fn body_text_tiers_stay_above_four_and_a_half() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = palette(scheme);
            for surface in [p.page, p.raised, p.zebra()] {
                let c = contrast(p.ink_65.over(surface), surface);
                assert!(c >= 4.5, "{scheme:?} ink-65 on {}: {c:.2}", surface.hex());
            }
        }
    }
}
