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

/* ---- selected text: an ink-18 wash under ink text, not libadwaita's accent blue (the only chroma in a
   warm-monochrome app, and 3.4:1 under the log viewer's path line). The `:focus-within` and `:backdrop`
   forms are libadwaita's own, which would otherwise win on specificity ---- */
selection, label > selection, label > selection:focus-within, label > selection:backdrop,
text > selection, text > selection:focus-within, text > selection:backdrop,
textview text selection, textview text selection:focus-within, textview text selection:backdrop
{ background: @ink_18; color: @ink; }

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
/* the ink ramp's two ends on a label that changes weight with its state (Settings > MCP: a line that
   needs the user is `ink_100`, a routine one `ink_65`) */
label.ink-100, .hint.ink-100 { color: @ink; }
label.ink-65 { color: @ink_65; }

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
/* a disabled hollow control is ink-45 text in an ink-46 border, as gui/theme.py draws it, not libadwaita's
   50% fade of the whole control (which would take the border to 1.4:1); a disabled stamp keeps the fade */
button.pill:disabled, button.icon-boxed:disabled { color: @ink_45; border-color: @ink_46; opacity: 1; filter: none; }
button.pill.primary:disabled { opacity: 0.5; }
button.pill.primary { background: @stamp_bg; color: @stamp_fg; border-color: @ink; }
button.pill.primary:hover { background: @ink; border-color: @ink; }
button.pill.plain { text-transform: none; letter-spacing: 0; }
button.icon-boxed { background: transparent; color: @ink; border: 1px solid @ink_46; border-radius: 0; box-shadow: none;
  padding: %space_2%px %space_3%px; min-height: 0; min-width: 0; font-size: 15px; }
button.icon-boxed:hover { background: @ink_10; border-color: @ink_58; }
button.link { background: none; border: none; box-shadow: none; padding: 0; min-height: 0; min-width: 0;
  color: @ink_65; font-family: %mono%; font-size: %label%px; letter-spacing: 0.06em; text-decoration: underline; }
button.link:hover { color: @ink; }

/* ---- the filter band over the Queue (gui/filter_bar.py; the mockup's .field): a hairline band, mono
   fields on the border tier, the hidden count on the label tier (the mockup's ink_45 is under 4.5:1) ---- */
.filter-band { border: 1px solid @ink_18; }
entry.field, menubutton.field > button { background: transparent; color: @ink; border: 1px solid @ink_46;
  border-radius: 0; box-shadow: none; font-family: %mono%; font-size: %label%px; letter-spacing: 0.04em;
  min-height: 0; min-width: 0; }
entry.field { padding: 7px 10px; }
menubutton.field > button { padding: 6px 14px; }
entry.field:hover, menubutton.field > button:hover { border-color: @ink_58; }
entry.field:focus-within { border-color: @ink; }
entry.field text > placeholder { color: @ink_65; opacity: 1; }
.filter-count { font-family: %mono%; font-size: %label%px; color: @ink_65; }

/* ---- keycaps (the mockup's .kbd) and the key-hint bar ---- */
.keycap { border: 1px solid @ink_28; color: @ink_65; padding: 0 5px; font-size: %micro%px; letter-spacing: 0.08em; }
button.pill.primary .keycap { border-color: @stamp_fg; color: @stamp_fg; }
.hint-bar label { font-family: %mono%; font-size: %label%px; letter-spacing: 0.06em; color: @ink_65; }
.hint-bar .strong { color: @ink; font-weight: 700; }

/* ---- chips and disclosure ---- */
/* DAN-2297: a chip is text, so it clears 4.5:1 in both schemes: `ink_65` off (the mockup's `ink-45` measured
   2.71 to 3.46:1), `ink` at weight 700 on, so the two states differ by weight and not by contrast alone. */
.chip { font-family: %mono%; font-size: %micro%px; letter-spacing: 0.08em; padding: 1px 6px; border: 1px solid @ink_18; color: @ink_65; }
.chip.on { border-color: @ink_28; color: @ink; font-weight: 700; }
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

/* ---- Review (gui/review_view.py; mockup review.html): the readout in the compare header, the match
   stepper, the candidate picker, the ruled tag list and the footer rule ---- */
.preview-text { color: @ink_65; font-size: %body%px; padding: %space_5%px; }
.readout-glyph, .readout-value { font-family: %serif%; font-size: 22px; font-weight: 600; color: @ink; }
.readout-glyph { font-family: %status%; font-size: 17px; }
.readout-label { font-family: %mono%; font-size: %label%px; letter-spacing: 0.1em; color: @ink_65; }
.readout-diff { font-family: %mono%; font-size: %label%px; letter-spacing: 0.06em; color: @ink_65; }
.readout-diff.tier-ink_100 { color: @ink; font-weight: 700; }
.readout-diff.tier-ink_65, .readout-diff.tier-ink_45 { color: @ink_65; }
.match-nav { border: 1px solid @ink_28; padding: %space_2%px %space_3%px %space_2%px %space_4%px; }
.match-label { font-family: %mono%; font-size: %label%px; letter-spacing: 0.1em; color: @ink; }
dropdown.candidate-picker > button { background: transparent; color: @ink; border: 1px solid @ink_46; border-radius: 0;
  box-shadow: none; padding: %space_2%px %space_3%px; min-height: 0; font-family: %mono%; font-size: %label%px; }
dropdown.candidate-picker > button:hover { border-color: @ink_58; }
.tag-scroller { background: @sunken; border: 1px solid @ink_28; }
.tag-list, .tag-list > row { background: transparent; color: @ink; }
.tag-list > row { border-bottom: 1px solid @ink_18; min-height: 0; padding: 6px %space_4%px; }
.tag-list > row:selected { background: @stamp_bg; color: @stamp_fg; }
.pick-list, .pick-list > row { background: transparent; color: @ink; }
.pick-list > row { border-bottom: 1px solid @ink_18; min-height: 0; }
.pick-list > row:selected { background: @stamp_bg; color: @stamp_fg; }
.pick-list > row:selected label { color: @stamp_fg; }
.tag-row { font-family: %mono%; font-size: 13px; color: @ink; }
.review-switch button { padding: 4px 6px; }
.review-rail button.pill { padding: 5px %space_2%px; }

/* ---- Wipe, Differences and the compare dialog (DAN-2340): the zoom tools float in the frame, the
   slider is a hairline with a stamp handle, the summary's tones are the Status column's ---- */
.wipe-view { color: @ink; }
.zoom-tools { background: @raised; border: 1px solid @ink_18; padding: 4px %space_2%px; }
.zoom-percent { font-family: %mono%; font-size: %label%px; color: @ink_65; }
.wipe-end-label { font-family: %mono%; font-size: %label%px; letter-spacing: 0.1em; color: @ink_65; }
scale.wipe-slider trough { min-height: 1px; background: @ink_46; border-radius: 0; }
scale.wipe-slider slider { min-width: 14px; min-height: 14px; background: @stamp_bg; border: 1px solid @ink;
  border-radius: 0; box-shadow: none; }
scale.wipe-slider highlight { background: transparent; }
window.compare-window radio { background: transparent; border: 1px solid @ink_46; border-radius: 50%;
  box-shadow: none; color: @stamp_fg; }
window.compare-window radio:checked { background: @stamp_bg; border-color: @stamp_bg; color: @stamp_fg; }
.summary-line { color: @ink; }
.summary-good { color: @ink_65; }
.summary-strong { font-weight: 700; }
.summary-italic { font-style: italic; }
.commit-caption { font-family: %mono%; font-size: %micro%px; letter-spacing: 0.06em; color: @ink_65; }
separator.footer-rule { background: @ink_18; min-height: 1px; }

/* ---- the ghost kanji: ink-05, 420px, bleeding off the top-right (design gate 2.9). `line-height: 100%`
   makes the label box 420 px tall, as `tokens.css` `.kanji-ghost` (`line-height: 1`) does in a browser;
   GTK reads a bare `1` as a factor of the font's own line height, 604 px for this face ---- */
.ghost-kanji { font-family: %kanji%; font-size: 420px; line-height: 100%; color: @ink_05; margin: -40px 40px 0 0; }

/* ---- banners (Crashed, quota pause) and the empty state ---- */
.banner { background: @raised; border: 1px solid @ink_18; padding: %space_5%px; }
.banner-glyph { font-family: %status%; font-size: 17px; color: @ink; }
.banner-head { font-size: %label%px; letter-spacing: %tracking_label%px; font-weight: 700; color: @ink; }
.banner-body { color: @ink_65; font-size: %body%px; }
/* Activity: the pipeline sentence, the "Show in Queue" hint, the log footer. */
.pipeline-line { font-family: %mono%; font-size: %label%px; color: @ink_65; }
.pipeline-names { font-family: %mono%; font-size: %label%px; font-weight: 700; color: @ink; }
.button-hint { font-family: %mono%; font-size: %micro%px; color: @ink_65; }
.log-footer { font-family: %mono%; font-size: %micro%px; color: @ink_65; }
/* Parser Health: working is ink_65, broken is ink_100 (the tiers Status uses). */
.health-ok { color: @ink_65; }
.health-bad { color: @ink; font-weight: 700; }
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
/* the fields a dialog holds: a sunken well, an ink tick on the stamp, a hairline on the border tier. Not
   libadwaita's white view or its accent blue (the only chroma in a warm-monochrome app: the tick and the
   spin button's edge were #3584e4, 4.89:1 dark and 3.23:1 light, and the Add tags view was #ffffff on
   the light sheet, DAN-2219) */
dialog.alert scrolledwindow.field { border: 1px solid @ink_46; }
dialog.alert textview, dialog.alert textview text { background: @sunken; color: @ink; }
/* the Query Hydrus and Pawchive Index windows are not alert dialogs, so the rules above never reached them:
   their tag box and lists were stock libadwaita (a white view, and a list with no edge at all, DAN-2395).
   Same sunken well, border on the @ink_58 tier (@ink_46 is 2.75:1 on the light page, under the 3:1
   non-text floor), and a border that goes to ink when the field holds focus */
window.pick-window scrolledwindow.field { border: 1px solid @ink_58; }
window.pick-window scrolledwindow.field:focus-within { border-color: @ink; }
window.pick-window textview, window.pick-window textview text { background: @sunken; color: @ink; }
window.pick-window entry.field:focus-within { border-color: @ink; outline: 2px solid @ink; outline-offset: 2px; }
/* a disabled primary button in these windows is the hollow, dimmed look Stop has when disabled, not the
   stamp fill at half opacity (DAN-2398 P3-2) */
window.pick-window button.pill.primary:disabled { background: transparent; color: @ink_45; border-color: @ink_46; opacity: 1; }
window.pick-window button.pill.primary:disabled label { color: @ink_45; }
window.pick-window progressbar trough { background: @ink_18; min-height: 4px; border-radius: 0; border: none; }
window.pick-window progressbar progress { background: @ink; min-height: 4px; border-radius: 0; border: none; }
dialog.alert check { background: transparent; color: @stamp_fg; border: 1px solid @ink_46; border-radius: 0;
  box-shadow: none; }
dialog.alert check:checked { background: @stamp_bg; border-color: @stamp_bg; color: @stamp_fg; }
dialog.alert spinbutton { background: @sunken; color: @ink; border: 1px solid @ink_46; border-radius: 0;
  box-shadow: none; }
dialog.alert spinbutton:focus-within { border-color: @ink; box-shadow: none; outline: none; }
dialog.alert spinbutton > text { background: transparent; color: @ink; box-shadow: none; }

/* ---- the Settings window (DAN-2221). A pick-window, so the sunken well, the field border and the ring
   above already reach its entries and lists; these are the controls only it holds. The tab strip is
   gui/theme.py's QTabBar: transparent at rest, the raised sheet when selected, ink on hover. `.found` is
   QSS `#Found` (stamp fill, 1px 5px) on the label or tick a Find jump has just landed on ---- */
window.settings-window notebook > header { background: @page; border: none; box-shadow: none; }
window.settings-window notebook > header > tabs > tab { background: transparent; color: @ink_65; border: none;
  border-radius: 0; box-shadow: none; padding: 9px 20px; margin: 0 6px 0 0; min-height: 0; min-width: 0; }
window.settings-window notebook > header > tabs > tab:hover { color: @ink; }
window.settings-window notebook > header > tabs > tab:checked { background: @raised; color: @ink; }
window.settings-window notebook > header > tabs > tab label { color: inherit; }
window.settings-window notebook:focus:focus-visible > header > tabs > tab:checked { outline: 2px solid @ink;
  outline-offset: -4px; }
window.settings-window notebook > stack { background: @page; border: none; }
window.settings-window check { background: transparent; color: @stamp_fg; border: 1px solid @ink_46;
  border-radius: 0; box-shadow: none; }
window.settings-window check:checked { background: @stamp_bg; border-color: @stamp_bg; color: @stamp_fg; }
window.settings-window spinbutton { background: @sunken; color: @ink; border: 1px solid @ink_58; border-radius: 0;
  box-shadow: none; }
window.settings-window spinbutton:focus-within { border-color: @ink; box-shadow: none; outline: 2px solid @ink;
  outline-offset: 2px; }
window.settings-window spinbutton > text { background: transparent; color: @ink; box-shadow: none; outline: none; }
window.settings-window spinbutton > button { background: transparent; color: @ink; border: none; border-radius: 0;
  box-shadow: none; }
window.settings-window dropdown > button { background: @sunken; color: @ink; border: 1px solid @ink_58;
  border-radius: 0; box-shadow: none; padding: %space_1%px %space_3%px; min-height: 0; }
window.settings-window dropdown > button:hover { border-color: @ink; }
window.settings-window entry.field { background: @sunken; color: @ink; border: 1px solid @ink_58; border-radius: 0;
  box-shadow: none; }
window.settings-window label.found, window.settings-window checkbutton.found {
  background: @stamp_bg; color: @stamp_fg; padding: 1px 5px; }
window.settings-window checkbutton.found label { color: @stamp_fg; }
window.settings-window checkbutton.found check { border-color: @stamp_fg; }

/* ---- the log viewer ---- */
textview.log, textview.log text { background: @sunken; color: @ink; font-family: %mono%; font-size: 12px; }
.log-path { font-family: %mono%; font-size: %label%px; color: @ink_65; }

/* ---- the Queue table (DAN-2217). Ink tiers are classes the cell renderer sets; the status glyphs
   take the bundled face through Pango markup, so no rule names it here ---- */
.queue-frame { background: @raised; border: 1px solid @ink_18; }
.queue { background: @raised; color: @ink; }
.queue > header { background: @raised; border-bottom: 1px solid @ink_28; }
.queue > header > button { border-radius: 0; background: transparent; color: @ink_65; padding: 0 %space_3%px;
  min-height: 28px; font-family: %mono%; font-size: %micro%px; font-weight: 700; letter-spacing: 0.1em;
  text-transform: uppercase; }
.queue > header > button:hover { background: @ink_10; color: @ink; }
.queue > header > button label { opacity: 1; color: @ink_65; }
.queue > header > button:hover label { color: @ink; }
.queue > header > button sort-indicator { color: @ink; min-width: 10px; min-height: 10px; }
.queue > listview > row { min-height: 48px; border-bottom: 1px solid @ink_18; background: @raised; padding: 0; }
.queue > listview > row:selected { background: @stamp_bg; color: @stamp_fg; }
.queue > listview > row:selected label { color: @stamp_fg; }
/* the row is clipped by the list, so the global 2px ring at offset 2 falls outside it and never shows;
   draw it inside, in ink on the row and in the stamp's own ink on a selected one (design gate 1.1) */
.queue > listview > row:focus:focus-visible { outline-offset: -4px; }
.queue > listview > row:selected:focus:focus-visible { outline-color: @stamp_fg; }
.queue-cell { padding: 0 %space_3%px; font-size: %body%px; color: @ink; }
.queue-cell.ink-45 { color: @ink_45; }
.queue-cell.ink-65 { color: @ink_65; }
.queue-cell.ink-100 { color: @ink; }
.queue-cell.tier-bold { font-weight: 700; }
.queue-cell.tier-italic { font-style: italic; }
.queue-cell.chip-boxed { border: 1px solid @ink_46; padding: 1px %space_2%px; margin: 0 %space_3%px; }
.queue-cell.cell-mono, .queue-cell.cell-mono-bold { font-family: %mono%; font-size: 11px; }
.queue-cell.cell-mono { font-weight: 500; }
.queue-cell.cell-mono-bold { font-weight: 700; }
.queue .thumb { margin: 0 %space_3%px; border: 1px solid @ink_58; background: @sunken; }
.queue .thumb.thumb-failed { border-color: @ink_58; }

/* scrolled windows draw soft edge gradients where content continues; flat has none */
undershoot, overshoot, undershoot.top, undershoot.bottom, undershoot.left, undershoot.right {
  background: none; background-image: none; box-shadow: none; }
scrollbar slider { background: @ink_58; border-radius: 0; min-width: 6px; min-height: 6px; }
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

    /// DAN-2395 (design read, F2/F6): the Query Hydrus and Pawchive windows are not `dialog.alert`, so
    /// their tag box and lists need their own well, an @ink_58 border (@ink_46 is 2.75:1 on the
    /// light page) that goes to ink on focus, and a hairline between list rows.
    #[test]
    fn pick_windows_get_the_well_the_border_and_row_dividers() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            let rule = |start: &str| {
                css.lines()
                    .find(|l| l.starts_with(start))
                    .unwrap_or_else(|| panic!("{scheme:?}: no rule starting {start}"))
                    .to_owned()
            };
            assert!(
                rule("window.pick-window textview,").contains("background: @sunken"),
                "{scheme:?}: the tag box is not the sunken well"
            );
            assert!(
                rule("window.pick-window scrolledwindow.field {").contains("@ink_58"),
                "{scheme:?}: the list border is not on the @ink_58 tier"
            );
            assert!(
                rule("window.pick-window scrolledwindow.field:focus-within")
                    .contains("border-color: @ink"),
                "{scheme:?}: a focused field does not show it"
            );
            assert!(
                rule("window.pick-window entry.field:focus-within")
                    .contains("outline: 2px solid @ink"),
                "{scheme:?}: the Pawchive entry's focus ring is libadwaita's accent blue"
            );
            assert!(
                rule("window.pick-window progressbar progress").contains("background: @ink;"),
                "{scheme:?}: the Pawchive progress bar is the toolkit's accent blue"
            );
            assert!(
                rule(".pick-list > row {").contains("border-bottom: 1px solid @ink_18"),
                "{scheme:?}: pick-list rows have no divider"
            );
            let off = rule("window.pick-window button.pill.primary:disabled {");
            assert!(
                off.contains("background: transparent")
                    && off.contains("border-color: @ink_46")
                    && off.contains("opacity: 1"),
                "{scheme:?}: a disabled primary is not Stop's hollow dim look (DAN-2398 P3-2): {off}"
            );
        }
    }

    /// DAN-2219 (design read): the fields a dialog holds take the palette, not libadwaita's accent
    /// blue (#3584e4: the tick and the spin button's edge) or its white view.
    #[test]
    fn dialog_fields_take_the_palette_not_the_accent() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            let rule = |start: &str| {
                css.lines()
                    .find(|l| l.starts_with(start))
                    .unwrap_or_else(|| panic!("{scheme:?}: no rule starting {start}"))
                    .to_owned()
            };
            assert!(
                rule("dialog.alert textview,").contains("background: @sunken"),
                "{scheme:?}: the Add tags view is not the sunken well"
            );
            assert!(
                rule("dialog.alert check:checked").contains("background: @stamp_bg"),
                "{scheme:?}: the tick is not the stamp"
            );
            assert!(
                rule("dialog.alert spinbutton:focus-within").contains("box-shadow: none"),
                "{scheme:?}: the spin button keeps libadwaita's focus edge"
            );
        }
    }

    /// DAN-2221: the Settings window's own controls take the palette, and a Find jump lands on the stamp.
    #[test]
    fn the_settings_window_controls_take_the_palette_and_found_is_the_stamp() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            let rule = |start: &str| {
                css.lines()
                    .find(|l| l.starts_with(start))
                    .unwrap_or_else(|| panic!("{scheme:?}: no rule starting {start}"))
                    .to_owned()
            };
            assert!(
                rule("window.settings-window check:checked").contains("background: @stamp_bg"),
                "{scheme:?}: the tick is not the stamp"
            );
            assert!(
                rule("window.settings-window notebook > header > tabs > tab:checked")
                    .contains("background: @raised"),
                "{scheme:?}: the selected tab is not the raised sheet"
            );
            assert!(
                rule("window.settings-window notebook:focus:focus-visible > header > tabs > tab:checked")
                    .contains("outline: 2px solid @ink"),
                "{scheme:?}: the tab's own focus ring is libadwaita's accent blue"
            );
            assert!(
                rule("window.settings-window spinbutton:focus-within").contains("box-shadow: none"),
                "{scheme:?}: the spin button keeps libadwaita's focus edge"
            );
            assert!(
                css.contains("window.settings-window label.found, window.settings-window checkbutton.found {\n  background: @stamp_bg"),
                "{scheme:?}: .found is not the stamp fill"
            );
            assert!(
                css.contains("padding: 1px 5px;"),
                "{scheme:?}: .found lost its 1px 5px padding"
            );
        }
    }

    /// DAN-2245: selected text is ink on the ink-18 wash, not libadwaita's accent blue (3.48:1 dark,
    /// 3.40:1 light under the log viewer's path line), for every selectable widget the app has, and
    /// the pair clears 4.5:1 over the surfaces a selection sits on (the page, the log's sunken fill).
    #[test]
    fn selected_text_is_ink_on_the_ink_18_wash() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            let rule = css
                .lines()
                .skip_while(|l| !l.starts_with("selection, label > selection,"))
                .take_while(|l| !l.starts_with("/* ----"))
                .collect::<Vec<_>>()
                .join(" ");
            for selector in [
                "label > selection:focus-within",
                "label > selection:backdrop",
                "text > selection:focus-within",
                "textview text selection:backdrop",
            ] {
                assert!(
                    rule.contains(selector),
                    "{scheme:?}: no selection rule for {selector}"
                );
            }
            assert!(
                rule.contains("{ background: @ink_18; color: @ink; }"),
                "{scheme:?}: selection is not ink on the ink-18 wash: {rule}"
            );
            let pal = Palette::of(scheme);
            for (surface, name) in [
                (pal.page, "page"),
                (pal.raised, "raised"),
                (pal.sunken, "sunken"),
            ] {
                let ratio = contrast(
                    pal.ink.over(pal.ink_18.over(surface)),
                    pal.ink_18.over(surface),
                );
                assert!(
                    ratio >= 4.5,
                    "{scheme:?}: ink on the wash over {name} is {ratio:.2}:1"
                );
            }
        }
    }

    #[test]
    fn no_placeholder_is_left_in_the_sheet() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            // `100%` is a CSS value; `%name%` is a placeholder.
            let mut parts = css.split('%').peekable();
            while let (Some(_), Some(between)) = (parts.next(), parts.peek()) {
                let is_name = !between.is_empty()
                    && between.chars().all(|c| c.is_ascii_lowercase() || c == '_');
                assert!(!is_name, "unreplaced placeholder %{between}% in {scheme:?}");
            }
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

    /// DAN-2297 ruling 1: the Activity chips are text, so off reads in `ink_65` (`ink_45` and `ink_58`
    /// measured 2.7 to 4.4:1) and on in `ink` at weight 700; both clear 4.5:1 in both schemes.
    #[test]
    fn the_engine_chips_are_text_and_clear_four_and_a_half() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let css = css(scheme);
            let rule = |sel: &str| {
                css.lines()
                    .find(|l| l.starts_with(sel))
                    .unwrap_or_else(|| panic!("no {sel} rule"))
                    .to_owned()
            };
            assert!(rule(".chip {").contains("color: @ink_65;"), "{scheme:?}");
            let on = rule(".chip.on {");
            assert!(on.contains("color: @ink;") && on.contains("font-weight: 700"));
            let p = Palette::of(scheme);
            for surface in [p.page, p.raised] {
                assert!(contrast(p.ink_65.over(surface), surface) >= 4.5);
                assert!(contrast(p.ink, surface) >= 4.5);
            }
        }
    }

    /// DAN-2242 fix 4: a control whose border is its shape clears 3:1 on the page and on the raised
    /// fill in both schemes, at rest (`ink-46`) and under the pointer (`ink-58`); `ink-45` would
    /// not (2.68:1 on the light page). Containers stay on `ink-18`, deliberately under 3:1.
    #[test]
    fn the_border_tier_clears_three_to_one_where_a_control_sits() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            for (tier, c) in [("ink-46", p.ink_46), ("ink-58", p.ink_58)] {
                for (name, surface) in [("page", p.page), ("raised", p.raised)] {
                    let r = contrast(c.over(surface), surface);
                    assert!(r >= 3.0, "{scheme:?} {tier} on {name}: {r:.2}:1");
                }
            }
            assert!(
                contrast(p.ink_46.over(p.page), p.page) > contrast(p.ink_45.over(p.page), p.page)
            );
            assert!(
                contrast(p.ink_18.over(p.page), p.page) < 3.0,
                "ink-18 is the quiet tier"
            );
        }
    }

    /// DAN-2217: the Queue's non-text boundaries (thumbnail frame, scrollbar thumb) clear 3:1 on the
    /// row and on the page, and the two libadwaita rules that dim text below 4.5:1 are overridden.
    #[test]
    fn queue_boundaries_clear_three_to_one_and_header_and_placeholder_are_not_dimmed() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            for ground in [p.raised, p.page] {
                let r = contrast(p.ink_58.over(ground), ground);
                assert!(r >= 3.0, "{scheme:?} ink-58 on {ground:?}: {r:.2}:1");
            }
            // the thumb frame is a 1px border painted over the thumb's own sunken fill, and it
            // sits next to the row, the zebra stripe and the page (DAN-2325: ink-46 gave 2.91:1);
            // the tier is read from the failed rule itself, so a weaker token goes red here
            let rule = RULES
                .lines()
                .find(|l| l.starts_with(".queue .thumb.thumb-failed"))
                .expect("the failed thumb rule");
            let tier = match rule
                .split("border-color: @")
                .nth(1)
                .and_then(|t| t.split(';').next())
            {
                Some("ink_58") => p.ink_58,
                Some("ink_46") => p.ink_46,
                other => panic!("the failed thumb frame uses {other:?}"),
            };
            let frame = tier.over(p.sunken);
            for ground in [p.raised, p.zebra(), p.page] {
                let r = contrast(frame, ground);
                assert!(
                    r >= 3.0,
                    "{scheme:?} failed thumb frame on {ground:?}: {r:.2}:1"
                );
            }
            let sheet = css(scheme);
            assert!(
                sheet.contains(".queue > header > button label { opacity: 1;"),
                "the header label is dimmed by the toolkit again"
            );
            assert!(
                sheet.contains("entry.field text > placeholder { color:")
                    && sheet.contains("opacity: 1; }\n.filter-count"),
                "the placeholder is dimmed by the toolkit again"
            );
        }
    }

    /// DAN-2242 fix 5: the shortcut hint on a hovered or focused menu row reads in `ink`, which
    /// clears 4.5:1 on the hover wash; `ink-65` did not (4.25:1 in dark).
    #[test]
    fn a_hovered_menu_hint_in_ink_clears_four_and_a_half() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            let wash = p.ink_10.over(p.raised);
            let r = contrast(p.ink, wash);
            assert!(r >= 4.5, "{scheme:?} ink on the hover wash: {r:.2}:1");
        }
        let sheet = css(Scheme::Dark);
        assert!(
            sheet.contains("popover modelbutton:hover label.accelerator")
                && sheet.contains("popover modelbutton:focus label.accelerator"),
            "the hint rule names hover and focus"
        );
    }

    /// `test_the_zebra_stripe_is_quiet_but_visible` and
    /// `test_ink_tiers_resolve_to_distinct_colours_per_theme`.
    #[test]
    fn the_zebra_is_quiet_but_visible_and_the_ink_tiers_are_distinct() {
        for scheme in [Scheme::Dark, Scheme::Light] {
            let p = Palette::of(scheme);
            assert_ne!(p.zebra(), p.raised, "{scheme:?}: the stripe is invisible");
            let ratio = contrast(p.zebra(), p.raised);
            assert!(
                ratio > 1.0 && ratio < 1.2,
                "{scheme:?}: a zebra of {ratio:.3}:1 is noise or nothing"
            );
            let tiers = [p.ink, p.ink_65, p.ink_58, p.ink_46].map(|c| c.over(p.raised));
            for (i, a) in tiers.iter().enumerate() {
                for b in &tiers[i + 1..] {
                    assert_ne!(a, b, "{scheme:?}: two ink tiers paint the same colour");
                }
            }
        }
    }
}
