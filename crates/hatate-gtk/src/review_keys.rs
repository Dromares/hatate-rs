//! A review binding as GTK spells it (`QKeySequence(key)` in `gui/review_shortcuts.py` at
//! `76c85c4`; DAN-2219). The registry holds Qt's spelling (`J`, `Shift+N`, `Return`, `Del`, `]`);
//! GTK's trigger parser wants `<Shift>n`, `Return`, `Delete`, `bracketright`.

/// `"Shift+N"` as the trigger string GTK parses, or `None` for a spelling it has no key for
/// (Python's `sequence.isEmpty()`, which logs and skips the binding).
pub fn gtk_trigger(key: &str) -> Option<String> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = key.split('+').collect();
    // "Ctrl++" is Ctrl and the plus key: the empty piece before the last is the plus.
    let mut last = parts.pop()?.trim();
    if last.is_empty() {
        last = "+";
        if parts.last().is_some_and(|p| p.trim().is_empty()) {
            parts.pop();
        }
    }
    let mut out = String::new();
    for modifier in parts {
        out.push_str(match modifier.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "<Control>",
            "shift" => "<Shift>",
            "alt" => "<Alt>",
            "meta" | "super" => "<Super>",
            _ => return None,
        });
    }
    out.push_str(&key_name(last)?);
    Some(out)
}

/// The keyval name of Qt's name for one key.
fn key_name(name: &str) -> Option<String> {
    let mut chars = name.chars();
    let (first, rest) = (chars.next()?, chars.as_str());
    if rest.is_empty() {
        return Some(match first {
            '[' => "bracketleft".to_owned(),
            ']' => "bracketright".to_owned(),
            '+' => "plus".to_owned(),
            '-' => "minus".to_owned(),
            '=' => "equal".to_owned(),
            ',' => "comma".to_owned(),
            '.' => "period".to_owned(),
            '/' => "slash".to_owned(),
            '\\' => "backslash".to_owned(),
            ';' => "semicolon".to_owned(),
            '\'' => "apostrophe".to_owned(),
            '`' => "grave".to_owned(),
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
            _ => return None,
        });
    }
    let lower = name.to_ascii_lowercase();
    Some(
        match lower.as_str() {
            "return" | "enter" => "Return",
            "space" => "space",
            "del" | "delete" => "Delete",
            "ins" | "insert" => "Insert",
            "esc" | "escape" => "Escape",
            "backspace" => "BackSpace",
            "tab" => "Tab",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "pgup" | "pageup" => "Page_Up",
            "pgdown" | "pagedown" => "Page_Down",
            f if f.starts_with('f')
                && f[1..].parse::<u8>().is_ok_and(|n| (1..=35).contains(&n)) =>
            {
                return Some(format!("F{}", &f[1..]));
            }
            _ => return None,
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_binding_has_a_trigger() {
        for a in hatate_core::shortcuts::REVIEW_ACTIONS {
            assert!(gtk_trigger(a.default).is_some(), "{}: {}", a.id, a.default);
        }
    }

    #[test]
    fn the_defaults_spell_as_gtk_does() {
        let t = |k: &str| gtk_trigger(k).unwrap();
        assert_eq!(t("J"), "j");
        assert_eq!(t("Shift+N"), "<Shift>n");
        assert_eq!(t("Return"), "Return");
        assert_eq!(t("Del"), "Delete");
        assert_eq!(t("F5"), "F5");
        assert_eq!(t("Space"), "space");
        assert_eq!(t("["), "bracketleft");
        assert_eq!(t("]"), "bracketright");
        assert_eq!(t("Ctrl+Shift+K"), "<Control><Shift>k");
        assert_eq!(t("Ctrl++"), "<Control>plus");
    }

    #[test]
    fn a_spelling_with_no_key_is_none() {
        assert_eq!(gtk_trigger(""), None);
        assert_eq!(gtk_trigger("Hyper+X"), None);
        assert_eq!(gtk_trigger("Banana"), None);
        assert_eq!(gtk_trigger("F99"), None);
    }
}
