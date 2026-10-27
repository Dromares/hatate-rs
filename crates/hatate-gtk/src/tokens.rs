//! The design tokens, read from the mockup's own `tokens.css` (vendored as `assets/tokens.css`,
//! `Dromares/hatate-ryoku-mockup@fff6302`). The stylesheet the window uses is generated from what
//! this module parses, so a token changed in the vendored sheet changes the window, and a test
//! pins the values the design gate measured (`docs/gtk-shell.md`).
//!
//! Only the two blocks that carry colours are read: `:root { ... }` (dark) and
//! `:root[data-theme="light"] { ... }` (the overrides). Values are `#rrggbb`, `rgba(r,g,b,a)` or
//! `var(--other)`.

use std::collections::BTreeMap;

/// The vendored sheet.
pub const TOKENS_CSS: &str = include_str!("../assets/tokens.css");

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

    /// A GTK CSS colour. Alpha is written with four decimals so a token round-trips.
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

    /// Parses `#rrggbb` and `rgba(r,g,b,a)` / `rgb(r,g,b)`.
    pub fn parse(s: &str) -> Option<Rgba> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 {
                return None;
            }
            let v = u32::from_str_radix(hex, 16).ok()?;
            return Some(Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
        }
        let inner = s
            .strip_prefix("rgba(")
            .or_else(|| s.strip_prefix("rgb("))?
            .strip_suffix(')')?;
        let parts: Vec<f64> = inner
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .ok()?;
        match parts.as_slice() {
            [r, g, b] => Some(Rgba(*r, *g, *b, 1.0)),
            [r, g, b, a] => Some(Rgba(*r, *g, *b, *a)),
            _ => None,
        }
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
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Dark => "dark",
            Scheme::Light => "light",
        }
    }
}

/// The custom properties of one scheme, `var()` references resolved.
#[derive(Clone, Debug)]
pub struct Tokens {
    vars: BTreeMap<String, String>,
}

/// The body of the first block whose selector is exactly `selector`.
fn block<'a>(css: &'a str, selector: &str) -> Option<&'a str> {
    let at = css.find(&format!("{selector} {{"))?;
    let open = at + selector.len() + 2;
    let close = css[open..].find('}')? + open;
    Some(&css[open..close])
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i..].find("*/") {
            Some(j) => rest = &rest[i + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn declarations(body: &str) -> Vec<(String, String)> {
    strip_comments(body)
        .split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let k = k.trim();
            k.strip_prefix("--")
                .map(|name| (name.to_owned(), v.trim().to_owned()))
        })
        .collect()
}

impl Tokens {
    /// The tokens of `scheme` from `css` (the vendored sheet in production).
    pub fn parse(css: &str, scheme: Scheme) -> Tokens {
        let mut vars = BTreeMap::new();
        let mut absorb = |body: &str| {
            for (k, v) in declarations(body) {
                vars.insert(k, v);
            }
        };
        if let Some(root) = block(css, ":root") {
            absorb(root);
        }
        if scheme == Scheme::Light {
            if let Some(light) = block(css, ":root[data-theme=\"light\"]") {
                absorb(light);
            }
        }
        // `var(--x)` references, resolved against the final map (one level is all the sheet uses).
        let snapshot = vars.clone();
        for v in vars.values_mut() {
            while let Some(i) = v.find("var(--") {
                let end = v[i..].find(')').map_or(v.len(), |j| i + j + 1);
                let name = &v[i + 6..end - 1];
                let Some(target) = snapshot.get(name) else {
                    break;
                };
                *v = format!("{}{}{}", &v[..i], target, &v[end..]);
            }
        }
        Tokens { vars }
    }

    pub fn raw(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    pub fn color(&self, name: &str) -> Rgba {
        self.raw(name)
            .and_then(Rgba::parse)
            .unwrap_or_else(|| panic!("token --{name} is missing or not a colour"))
    }

    /// A length in px (`24px`) or rem (`2.25rem`, 16 px to the rem).
    pub fn px(&self, name: &str) -> f64 {
        let v = self
            .raw(name)
            .unwrap_or_else(|| panic!("token --{name} is missing"));
        if let Some(px) = v.strip_suffix("px") {
            px.trim().parse().expect("px length")
        } else if let Some(rem) = v.strip_suffix("rem") {
            rem.trim().parse::<f64>().expect("rem length") * 16.0
        } else {
            panic!("token --{name} is not a length: {v}")
        }
    }
}

/// The surfaces and ink tiers of one scheme, as the window uses them.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub scheme: Scheme,
    pub page: Rgba,
    pub raised: Rgba,
    pub sunken: Rgba,
    pub ink: Rgba,
    pub ink_65: Rgba,
    pub ink_45: Rgba,
    pub ink_28: Rgba,
    pub ink_18: Rgba,
    pub ink_10: Rgba,
    pub ink_06: Rgba,
    pub ink_05: Rgba,
    pub stamp_bg: Rgba,
    pub stamp_fg: Rgba,
}

impl Palette {
    pub fn from_tokens(scheme: Scheme, t: &Tokens) -> Palette {
        Palette {
            scheme,
            page: t.color("charcoal"),
            raised: t.color("charcoal-raised"),
            sunken: t.color("charcoal-sunken"),
            ink: t.color("ink-100"),
            ink_65: t.color("ink-65"),
            ink_45: t.color("ink-45"),
            ink_28: t.color("ink-28"),
            ink_18: t.color("ink-18"),
            ink_10: t.color("ink-10"),
            ink_06: t.color("ink-06"),
            ink_05: t.color("ink-05"),
            stamp_bg: t.color("stamp-bg"),
            stamp_fg: t.color("stamp-fg"),
        }
    }

    /// The palette of `scheme` from the vendored sheet.
    pub fn of(scheme: Scheme) -> Palette {
        Palette::from_tokens(scheme, &Tokens::parse(TOKENS_CSS, scheme))
    }

    /// Zebra row: `ink-06` over the raised list surface.
    pub fn zebra(&self) -> Rgba {
        self.ink_06.over(self.raised)
    }

    /// The focus ring on a given surface: ink, except on the stamp-filled surface, where ink is
    /// the fill itself and the ring takes the stamp's text colour.
    pub fn ring_on(&self, surface: Rgba) -> Rgba {
        if surface == self.stamp_bg {
            self.stamp_fg
        } else {
            self.ink
        }
    }

    /// The `@define-color` block.
    pub fn define_colors(&self) -> String {
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
            ("ink_06", self.ink_06),
            ("ink_05", self.ink_05),
            ("stamp_bg", self.stamp_bg),
            ("stamp_fg", self.stamp_fg),
        ];
        defs.iter()
            .map(|(n, c)| format!("@define-color {n} {};\n", c.css()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_tokens_are_the_sheets_values() {
        let p = Palette::of(Scheme::Dark);
        assert_eq!(p.page.hex(), "#0d0d0d");
        assert_eq!(p.raised.hex(), "#141414");
        assert_eq!(p.sunken.hex(), "#090909");
        assert_eq!(p.ink.hex(), "#cdc4ba");
        assert_eq!(p.stamp_bg, p.ink);
        assert_eq!(p.stamp_fg.hex(), "#0d0d0d");
        assert_eq!(p.ink_65.3, 0.65);
        assert_eq!(p.ink_05.3, 0.05);
    }

    #[test]
    fn light_tokens_override_only_what_the_light_block_sets() {
        let p = Palette::of(Scheme::Light);
        assert_eq!(p.page.hex(), "#e8e1d2");
        assert_eq!(p.raised.hex(), "#f2ede1");
        assert_eq!(p.sunken.hex(), "#ddd2bd");
        assert_eq!(p.ink.hex(), "#211d17");
        assert_eq!(p.stamp_fg.hex(), "#f6f1e6");
        assert_eq!(p.ink_65.3, 0.70);
        assert_eq!(p.ink_28.3, 0.45);
        assert_eq!(p.ink_05.3, 0.08);
    }

    #[test]
    fn var_references_resolve() {
        let t = Tokens::parse(TOKENS_CSS, Scheme::Dark);
        assert_eq!(t.raw("stamp-bg"), t.raw("ink"));
        assert!(!t.raw("font-status").unwrap().contains("var("));
        assert_eq!(t.px("space-5"), 24.0);
        assert_eq!(t.px("text-display"), 36.0);
    }

    #[test]
    fn a_changed_token_changes_the_palette() {
        let css = TOKENS_CSS.replacen(
            "--charcoal:        #0d0d0d;",
            "--charcoal:        #101010;",
            1,
        );
        let p = Palette::from_tokens(Scheme::Dark, &Tokens::parse(&css, Scheme::Dark));
        assert_eq!(p.page.hex(), "#101010");
    }

    #[test]
    fn contrast_matches_the_published_ratios() {
        let d = Palette::of(Scheme::Dark);
        // tokens.css: ink-100 is 11.3:1 on the page, ink-65 is 5.25:1.
        assert!((contrast(d.ink, d.page) - 11.3).abs() < 0.1);
        assert!((contrast(d.ink_65.over(d.page), d.page) - 5.25).abs() < 0.15);
    }

    #[test]
    fn rgba_parse_round_trips() {
        assert_eq!(Rgba::parse("#0d0d0d"), Some(Rgba::rgb(13, 13, 13)));
        assert_eq!(
            Rgba::parse("rgba(205,196,186,0.65)"),
            Some(Rgba(205.0, 196.0, 186.0, 0.65))
        );
        assert_eq!(Rgba::parse("nonsense"), None);
    }
}
