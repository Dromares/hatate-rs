//! Dark theme as GTK CSS, transcribed from the Ryoku mockup's `tokens.css`
//! (`Dromares/hatate-ryoku-mockup@fff6302`). One ink colour at a handful of
//! opacities, flat, no gradients, no shadows, no rounded corners. Spacing uses
//! the mockup's scale (4, 8, 12, 16, 24, 32 px).

/// Colour tokens (hex or rgba), copied from `tokens.css`.
pub const TOKENS: &str = "
@define-color charcoal #0d0d0d;
@define-color charcoal_raised #141414;
@define-color charcoal_sunken #090909;
@define-color zebra #1f1f1e;
@define-color ink rgb(205,196,186);
@define-color ink_65 rgba(205,196,186,0.65);
@define-color ink_46 rgba(205,196,186,0.46);
@define-color ink_28 rgba(205,196,186,0.28);
@define-color ink_18 rgba(205,196,186,0.18);
@define-color ink_10 rgba(205,196,186,0.10);
@define-color stamp_bg #cdc4ba;
@define-color stamp_fg #0d0d0d;
";

/// Widget rules. Font stacks fall back to what is installed: the mockup's
/// Space Grotesk / JetBrains Mono / Source Serif 4 are not bundled in the spike.
pub const RULES: &str = "
window, window.background { background: @charcoal; color: @ink; border-radius: 0; box-shadow: none; }
window.csd { border-radius: 0; box-shadow: none; }
* { font-family: 'Space Grotesk', 'Inter', 'Helvetica Neue', Arial, sans-serif; }
.mono, .eyebrow, .tab, .mode, .status, columnview > header button, .stat, .hint {
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

/* tabs and mode toggles: hairline frame, the active one is the stamp (fg/bg swap) */
button.tab, button.mode { background: transparent; color: @ink_65; border: 1px solid @ink_28;
  border-radius: 0; box-shadow: none; text-shadow: none; padding: 8px 16px; min-height: 0;
  font-size: 11px; letter-spacing: 0.16em; }
button.tab:hover, button.mode:hover { background: @ink_10; }
button.tab:checked, button.mode:checked { background: @stamp_bg; color: @stamp_fg; border-color: @stamp_bg; }
button.tab:focus-visible, button.mode:focus-visible, scale:focus-visible { outline: 2px solid @ink; outline-offset: 2px; }

/* the queue */
.queue-frame { background: @charcoal_raised; border: 1px solid @ink_18; border-radius: 0; }
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
.thumb { border: 1px solid @ink_46; background: @charcoal_sunken; border-radius: 0; }
columnview.queue > listview > row:selected .thumb { border-color: @stamp_fg; background: @stamp_bg; }
.status { font-size: 12px; }
.status-good { font-weight: 700; color: @ink; }
.status-review { font-weight: 700; color: @ink; }
.status-notfound { color: @ink_65; }
.status-error { font-weight: 700; color: @ink; }
.status-unsearched { color: @ink_46; }
scrollbar slider { background: @ink_28; border-radius: 0; min-width: 6px; min-height: 6px; }
scrollbar, scrollbar trough { background: transparent; }

/* review */
.canvas { background: @charcoal_sunken; border: 1px solid @ink_18; border-radius: 0; }
.canvas:focus-visible { outline: 2px solid @ink; outline-offset: -2px; }
scale trough { background: @ink_18; min-height: 3px; border-radius: 0; }
scale highlight { background: @ink_28; border-radius: 0; }
scale slider { background: @ink; min-width: 4px; min-height: 17px; border-radius: 0; border: none; box-shadow: none; }
.error-text { color: @ink; }
";

/// The whole sheet.
pub fn css() -> String {
    format!("{TOKENS}{RULES}")
}

/// Install the sheet for the default display at application priority, above
/// libadwaita's own stylesheet.
pub fn install() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css());
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
        let css = css();
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
                "undefined colour @{name}"
            );
        }
    }
}
