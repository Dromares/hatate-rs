//! Settings > Shortcuts (`gui/settings_dialog.py` `_build_shortcuts_tab`, `_current_shortcut_bindings`,
//! `_reset_shortcuts_to_defaults` and `_refresh_shortcut_conflicts` at `76c85c4`).
//!
//! The registry is `hatate_core::shortcuts` (6.b); the keys are held in Qt's spelling, as
//! `QKeySequence.toString()` writes them (`J`, `Shift+N`, `Return`, `Del`, `]`), because that is
//! what `config.json` holds. [`canonical`] is `QKeySequence(text).toString()`, and
//! [`from_key_event`] is what a `QKeySequenceEdit` records from one key press.

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::OrderedMap;
use hatate_core::shortcuts::{self, REVIEW_ACTIONS};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const TAB: &str = "Shortcuts";
pub const INTRO: &str = "Keys for reviewing results without the mouse. They work while the image list has focus, so the filter box and the tag editor keep every character you type into them.\n\nClick a shortcut to record a new key. Each does exactly what the right-click menu entry of the same name does, confirmation prompts included.";
pub const HEADERS: [&str; 4] = ["Action", "Shortcut", "Default", ""];
pub const RECORDING: &str = "Press a key\u{2026}";
pub const CONFLICT_HEAD: &str =
    "Same key on more than one action - Qt fires neither, so these would both stop working:\n";
pub const CONFLICT_TAIL: &str = "\n\nGive one of them a different key, or clear it.";
pub const CONFLICT_TITLE: &str = "Shortcut conflict";

/// The name Qt writes for a key, from the names a person types or the registry holds.
fn key_alias(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "return" => "Return",
        "enter" => "Enter",
        "space" => "Space",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "del" | "delete" => "Del",
        "ins" | "insert" => "Ins",
        "esc" | "escape" => "Esc",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "home" => "Home",
        "end" => "End",
        "pgup" | "pageup" => "PgUp",
        "pgdown" | "pagedown" => "PgDown",
        _ => return None,
    })
}

/// `QKeySequence(text).toString()` for one key: modifiers in Qt's order (`Meta+Ctrl+Alt+Shift`),
/// the key as Qt names it. Text that names no key is the empty sequence.
pub fn canonical(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let mut parts: Vec<&str> = text.split('+').collect();
    let mut last = parts.pop().unwrap_or("").trim();
    if last.is_empty() {
        // "Ctrl++": the plus key itself
        last = "+";
        if parts.last().is_some_and(|p| p.trim().is_empty()) {
            parts.pop();
        }
    }
    let (mut meta, mut ctrl, mut alt, mut shift) = (false, false, false, false);
    for m in parts {
        match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            "meta" | "super" => meta = true,
            _ => return String::new(),
        }
    }
    let lower = last.to_ascii_lowercase();
    let key = if let Some(name) = key_alias(&lower) {
        name.to_owned()
    } else if lower.starts_with('f')
        && lower[1..]
            .parse::<u8>()
            .is_ok_and(|n| (1..=35).contains(&n))
    {
        format!("F{}", &lower[1..])
    } else if last.chars().count() == 1 {
        last.to_uppercase()
    } else {
        return String::new();
    };
    let mut out = String::new();
    for (on, name) in [
        (meta, "Meta"),
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
    ] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&key);
    out
}

/// What a `QKeySequenceEdit` with a one-key limit records from a key press, or `None` while only
/// modifiers are down.
pub fn from_key_event(keyval: gdk::Key, state: gdk::ModifierType) -> Option<String> {
    use gdk::Key;
    let named = match keyval {
        Key::Shift_L
        | Key::Shift_R
        | Key::Control_L
        | Key::Control_R
        | Key::Alt_L
        | Key::Alt_R
        | Key::Meta_L
        | Key::Meta_R
        | Key::Super_L
        | Key::Super_R
        | Key::Caps_Lock
        | Key::ISO_Level3_Shift
        | Key::Num_Lock => return None,
        Key::Return => "Return",
        Key::KP_Enter => "Enter",
        Key::space => "Space",
        Key::Tab | Key::ISO_Left_Tab => "Tab",
        Key::BackSpace => "Backspace",
        Key::Delete => "Del",
        Key::Insert => "Ins",
        Key::Escape => "Esc",
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Home => "Home",
        Key::End => "End",
        Key::Page_Up => "PgUp",
        Key::Page_Down => "PgDown",
        _ => "",
    };
    let key = if !named.is_empty() {
        named.to_owned()
    } else if let Some(name) = keyval.name().filter(|n| {
        n.len() >= 2
            && n.starts_with('F')
            && n[1..].parse::<u8>().is_ok_and(|v| (1..=35).contains(&v))
    }) {
        name.to_string()
    } else {
        keyval.to_unicode()?.to_uppercase().to_string()
    };
    let mut out = String::new();
    for (flag, name) in [
        (gdk::ModifierType::SUPER_MASK, "Meta"),
        (gdk::ModifierType::CONTROL_MASK, "Ctrl"),
        (gdk::ModifierType::ALT_MASK, "Alt"),
        (gdk::ModifierType::SHIFT_MASK, "Shift"),
    ] {
        if state.contains(flag) {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&key);
    Some(out)
}

/// A `QKeySequenceEdit` with `setMaximumSequenceLength(1)`: shows the key; a click starts
/// recording, and the next key press (not a modifier on its own) is the new value.
pub struct KeyEdit {
    pub button: gtk::Button,
    value: RefCell<String>,
    recording: Cell<bool>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
}

impl KeyEdit {
    fn new(initial: &str, tooltip: &str) -> Rc<KeyEdit> {
        let button = gtk::Button::new();
        button.add_css_class("key-edit");
        button.set_halign(gtk::Align::Start);
        button.set_tooltip_text(Some(tooltip));
        let edit = Rc::new(KeyEdit {
            button,
            value: RefCell::new(canonical(initial)),
            recording: Cell::new(false),
            on_change: RefCell::new(None),
        });
        edit.show();
        {
            let w = Rc::downgrade(&edit);
            edit.button.connect_clicked(move |_| {
                if let Some(e) = w.upgrade() {
                    e.recording.set(true);
                    e.show();
                }
            });
        }
        {
            let w = Rc::downgrade(&edit);
            let keys = gtk::EventControllerKey::new();
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(move |_, keyval, _, state| {
                let Some(e) = w.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if !e.recording.get() {
                    return glib::Propagation::Proceed;
                }
                match from_key_event(keyval, state) {
                    Some(key) => {
                        e.recording.set(false);
                        e.set(&key);
                        glib::Propagation::Stop
                    }
                    None => glib::Propagation::Stop,
                }
            });
            edit.button.add_controller(keys);
        }
        {
            let w = Rc::downgrade(&edit);
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(move |_| {
                if let Some(e) = w.upgrade() {
                    if e.recording.replace(false) {
                        e.show();
                    }
                }
            });
            edit.button.add_controller(focus);
        }
        edit
    }

    fn show(&self) {
        let text = if self.recording.get() {
            RECORDING.to_owned()
        } else {
            self.value.borrow().clone()
        };
        self.button.set_label(&text);
    }

    /// `keySequence().toString()`.
    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    /// `setKeySequence(QKeySequence(text))`; tells the page when the key changed
    /// (`keySequenceChanged`).
    pub fn set(&self, text: &str) {
        let key = canonical(text);
        let changed = *self.value.borrow() != key;
        *self.value.borrow_mut() = key;
        self.recording.set(false);
        self.show();
        if changed {
            let cb = self.on_change.borrow().clone();
            if let Some(cb) = cb {
                cb();
            }
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.get()
    }

    /// Starts recording as a click does. Tests only.
    pub fn start_recording(&self) {
        self.button.emit_clicked();
    }

    /// Feeds a key press to the recorder as the window would deliver it. Tests only.
    pub fn press(&self, keyval: gdk::Key, state: gdk::ModifierType) {
        if !self.recording.get() {
            return;
        }
        if let Some(key) = from_key_event(keyval, state) {
            self.recording.set(false);
            self.set(&key);
        }
    }
}

/// The tab.
pub struct ShortcutsPage {
    pub root: gtk::Box,
    pub edits: Vec<(&'static str, Rc<KeyEdit>)>,
    pub clears: Vec<gtk::Button>,
    pub reset: gtk::Button,
    pub conflict: gtk::Label,
    pub intro: gtk::Label,
}

impl ShortcutsPage {
    /// `_build_shortcuts_tab` over the stored overrides.
    pub fn build(overrides: &OrderedMap<String>) -> Rc<ShortcutsPage> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, crate::widgets::CARD_SPACING);
        let (l, t, r, b) = crate::widgets::PAGE_MARGINS;
        root.set_margin_start(l);
        root.set_margin_top(t);
        root.set_margin_end(r);
        root.set_margin_bottom(b);

        let intro = gtk::Label::new(Some(INTRO));
        intro.set_xalign(0.0);
        intro.set_wrap(true);
        intro.set_max_width_chars(72);
        root.append(&intro);

        let bindings = shortcuts::resolve(Some(overrides));
        let grid = gtk::Grid::new();
        grid.set_column_spacing(18);
        grid.set_row_spacing(6);
        grid.set_hexpand(true);
        for (c, head) in HEADERS.iter().enumerate() {
            let label = crate::widgets::caps_label(head, "section-label");
            grid.attach(&label, c as i32, 0, 1, 1);
        }
        let mut edits = Vec::new();
        let mut clears = Vec::new();
        for (i, action) in REVIEW_ACTIONS.iter().enumerate() {
            let row = i as i32 + 1;
            let name = gtk::Label::new(Some(action.label));
            name.set_xalign(0.0);
            name.set_hexpand(true);
            if !action.description.is_empty() {
                name.set_tooltip_text(Some(action.description));
            }
            grid.attach(&name, 0, row, 1, 1);
            let key = bindings
                .0
                .iter()
                .find(|(id, _)| id == action.id)
                .map_or("", |(_, k)| k.as_str());
            let edit = KeyEdit::new(key, "");
            grid.attach(&edit.button, 1, row, 1, 1);
            let default = gtk::Label::new(Some(if action.default.is_empty() {
                "\u{2014}"
            } else {
                action.default
            }));
            default.set_xalign(0.0);
            grid.attach(&default, 2, row, 1, 1);
            let clear = crate::widgets::pill_button("Clear", false, false);
            clear.set_tooltip_text(Some(&format!(
                "Leave \"{}\" with no key at all.",
                action.label
            )));
            {
                let e = edit.clone();
                clear.connect_clicked(move |_| e.set(""));
            }
            grid.attach(&clear, 3, row, 1, 1);
            clears.push(clear);
            edits.push((action.id, edit));
        }
        let frame = gtk::ScrolledWindow::new();
        frame.set_child(Some(&grid));
        frame.set_min_content_height(320);
        frame.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        frame.add_css_class("field");
        root.append(&frame);

        let conflict = gtk::Label::new(None);
        conflict.set_xalign(0.0);
        conflict.set_wrap(true);
        conflict.set_max_width_chars(72);
        conflict.add_css_class("ink-100");
        root.append(&conflict);

        let reset = crate::widgets::pill_button("Reset all to defaults", false, false);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row.append(&spacer);
        row.append(&reset);
        root.append(&row);

        let page = Rc::new(ShortcutsPage {
            root,
            edits,
            clears,
            reset,
            conflict,
            intro,
        });
        for (_, edit) in &page.edits {
            let w = Rc::downgrade(&page);
            *edit.on_change.borrow_mut() = Some(Rc::new(move || {
                if let Some(p) = w.upgrade() {
                    p.refresh_conflicts();
                }
            }));
        }
        {
            let w = Rc::downgrade(&page);
            page.reset.connect_clicked(move |_| {
                if let Some(p) = w.upgrade() {
                    p.reset_to_defaults();
                }
            });
        }
        page.refresh_conflicts();
        page
    }

    /// `_current_shortcut_bindings`: every action, in the registry's order.
    pub fn bindings(&self) -> OrderedMap<String> {
        OrderedMap(
            self.edits
                .iter()
                .map(|(id, e)| ((*id).to_owned(), e.value()))
                .collect(),
        )
    }

    pub fn edit(&self, id: &str) -> Option<&Rc<KeyEdit>> {
        self.edits.iter().find(|(i, _)| *i == id).map(|(_, e)| e)
    }

    /// `_reset_shortcuts_to_defaults`.
    pub fn reset_to_defaults(&self) {
        for (id, key) in shortcuts::default_bindings().0 {
            if let Some(edit) = self.edit(&id) {
                edit.set(&key);
            }
        }
        self.refresh_conflicts();
    }

    /// `_refresh_shortcut_conflicts`: draws any key bound twice, and says whether there is one.
    pub fn refresh_conflicts(&self) -> bool {
        let clashes = shortcuts::conflicts(&self.bindings());
        if clashes.is_empty() {
            self.conflict.set_text("");
            return false;
        }
        let mut lines = Vec::new();
        for (_, ids) in &clashes {
            let labels: Vec<&str> = ids
                .iter()
                .filter_map(|i| shortcuts::action(i).map(|a| a.label))
                .collect();
            let shown = ids
                .first()
                .and_then(|i| self.edit(i))
                .map(|e| e.value())
                .unwrap_or_default();
            lines.push(format!("{shown} is bound to {}", labels.join(" and ")));
        }
        self.conflict
            .set_text(&format!("{CONFLICT_HEAD}{}", lines.join("\n")));
        true
    }

    /// `QLabel`s the Find box can name: the intro (too long to be listed) and the conflict line.
    pub fn names(&self) -> Vec<(Option<String>, gtk::Widget)> {
        vec![
            (Some(INTRO.to_owned()), self.intro.clone().upcast()),
            (None, self.conflict.clone().upcast()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_defaults_are_already_qts_spelling() {
        for a in REVIEW_ACTIONS {
            assert_eq!(canonical(a.default), a.default, "{}", a.id);
        }
    }

    #[test]
    fn text_is_written_the_way_a_key_sequence_writes_it() {
        assert_eq!(canonical("j"), "J");
        assert_eq!(canonical(" shift+n "), "Shift+N");
        assert_eq!(canonical("shift+ctrl+a"), "Ctrl+Shift+A");
        assert_eq!(canonical("alt+ctrl+meta+shift+x"), "Meta+Ctrl+Alt+Shift+X");
        assert_eq!(canonical("delete"), "Del");
        assert_eq!(canonical("Escape"), "Esc");
        assert_eq!(canonical("pageup"), "PgUp");
        assert_eq!(canonical("f12"), "F12");
        assert_eq!(canonical("Ctrl++"), "Ctrl++");
        assert_eq!(canonical("]"), "]");
        assert_eq!(canonical(""), "");
        assert_eq!(canonical("not a key"), "");
        assert_eq!(canonical("hyper+a"), "");
    }
}
