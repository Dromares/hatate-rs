//! A Settings page as widgets: turns a `settings_node::Node` tree into GTK, and reads the controls
//! back as [`Values`] (`settings_values.rs`). The page itself is data; nothing here knows what a
//! setting means.
//!
//! The spin boxes keep Qt's behaviour where the values are concerned: a `QDoubleSpinBox` shows
//! its `suffix`, shows its `specialValueText` at the minimum, and holds the typed number rounded
//! to its decimals and bounded ([`settings_values::qt_double`]).

use crate::settings_node::Node;
use crate::settings_values::{Value, Values, qt_double, qt_int};
use crate::widgets;
use gtk4 as gtk;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub enum Control {
    Check(gtk::CheckButton),
    Num {
        spin: gtk::SpinButton,
        min: f64,
        max: f64,
        dec: u32,
    },
    Choice {
        drop: gtk::DropDown,
        data: Vec<&'static str>,
    },
    Text(gtk::Entry),
    Multi(gtk::TextView),
    Checks {
        boxes: Vec<gtk::CheckButton>,
        items: Vec<&'static str>,
    },
    Button(gtk::Button),
    Live(gtk::Label),
}

/// One built page.
pub struct Form {
    pub root: gtk::Box,
    pub controls: BTreeMap<&'static str, Control>,
    /// What the Find box can name, in tree order: the text of every label, check box and group
    /// title (`settings_search.index_tabs` walks `QLabel`, `QCheckBox`, `QRadioButton`, `QGroupBox`).
    pub named: Vec<(Option<String>, gtk::Widget)>,
    /// The drop-down list of each editable combo (`QComboBox` with `setEditable(True)`).
    pub combos: BTreeMap<&'static str, Combo>,
}

/// The list under an editable combo's line edit. It has no items until something fills it in
/// (`Fetch from Hydrus`), and its arrow is hidden until then.
pub struct Combo {
    pub arrow: gtk::MenuButton,
    pub list: gtk::ListBox,
    popover: gtk::Popover,
    /// `itemData` of each row, in order: what picking the row writes into the line edit.
    keys: Rc<RefCell<Vec<String>>>,
}

impl Combo {
    /// `clear()` and `addItem(label, userData=key)` for each: the list now holds `items`.
    pub fn set_items(&self, items: &[(String, String)]) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        *self.keys.borrow_mut() = items.iter().map(|(_, k)| k.clone()).collect();
        for (label, _) in items {
            let l = gtk::Label::new(Some(label));
            l.set_xalign(0.0);
            l.set_margin_top(4);
            l.set_margin_bottom(4);
            l.set_margin_start(8);
            l.set_margin_end(8);
            self.list.append(&l);
        }
        self.arrow.set_visible(!items.is_empty());
    }

    /// The text of each row.
    pub fn labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(row) = self.list.row_at_index(i) {
            if let Some(l) = row.child().and_downcast::<gtk::Label>() {
                out.push(l.text().to_string());
            }
            i += 1;
        }
        out
    }

    /// `itemData` of each row.
    pub fn keys(&self) -> Vec<String> {
        self.keys.borrow().clone()
    }

    /// Picks row `i` the way a click does (`_on_service_selected`).
    pub fn pick(&self, i: usize) {
        if let Some(row) = self.list.row_at_index(i as i32) {
            self.list.emit_by_name::<()>("row-activated", &[&row]);
        }
    }

    pub fn popover(&self) -> &gtk::Popover {
        &self.popover
    }
}

impl Form {
    pub fn build(page: &Node) -> Form {
        let mut form = Form {
            root: gtk::Box::new(gtk::Orientation::Vertical, 0),
            controls: BTreeMap::new(),
            named: Vec::new(),
            combos: BTreeMap::new(),
        };
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        form.root.set_margin_start(l);
        form.root.set_margin_top(t);
        form.root.set_margin_end(r);
        form.root.set_margin_bottom(b);
        let body = form.node(page);
        form.root.append(&body);
        form
    }

    fn name(&mut self, text: &str, w: &impl IsA<gtk::Widget>) {
        self.named.push((Some(text.to_owned()), w.clone().upcast()));
    }

    fn node(&mut self, n: &Node) -> gtk::Widget {
        match *n {
            Node::Group { title, body } => {
                let frame = widgets::card(Some(title), true);
                // margins belong to the page; a card nested in a page sits flush
                frame.set_margin_start(0);
                frame.set_margin_end(0);
                self.name(title, &frame);
                let inner = self.node(body);
                frame.append(&inner);
                frame.upcast()
            }
            Node::Form(rows) => {
                let grid = gtk::Grid::new();
                grid.set_column_spacing(14);
                grid.set_row_spacing(8);
                for (r, row) in rows.iter().enumerate() {
                    let r = r as i32;
                    let field = self.node(&row.field);
                    match &row.label {
                        Some(label) => {
                            let l = self.node(label);
                            l.set_valign(gtk::Align::Start);
                            grid.attach(&l, 0, r, 1, 1);
                            field.set_hexpand(true);
                            grid.attach(&field, 1, r, 1, 1);
                        }
                        None => {
                            field.set_hexpand(true);
                            grid.attach(&field, 0, r, 2, 1);
                        }
                    }
                }
                grid.upcast()
            }
            Node::Column(items) => {
                let col = gtk::Box::new(gtk::Orientation::Vertical, widgets::CARD_SPACING);
                for item in items {
                    let w = self.node(item);
                    col.append(&w);
                }
                col.upcast()
            }
            Node::Line(items) => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                for item in items {
                    let w = self.node(item);
                    row.append(&w);
                }
                row.upcast()
            }
            Node::Label { text, wrap, hint } => {
                let l = gtk::Label::new(Some(text));
                l.set_xalign(0.0);
                if hint {
                    l.add_css_class("hint");
                }
                // A sentence-length caption wraps to the page even when the spec hard-wrapped it:
                // it clipped mid-sentence at the 560 px minimum (DAN-2319, 6.h F3).
                if wrap || is_prose(text) {
                    l.set_wrap(true);
                    l.set_max_width_chars(72);
                }
                self.name(text, &l);
                l.upcast()
            }
            Node::Check { id, text, tip } => {
                let c = gtk::CheckButton::with_label(text);
                tooltip(&c, tip);
                self.name(text, &c);
                self.controls.insert(id, Control::Check(c.clone()));
                c.upcast()
            }
            Node::Double {
                id,
                min,
                max,
                dec,
                step,
                suffix,
                special,
                tip,
            } => {
                let spin = spin_button(min, max, step, dec, suffix, special);
                tooltip(&spin, tip);
                self.controls.insert(
                    id,
                    Control::Num {
                        spin: spin.clone(),
                        min,
                        max,
                        dec,
                    },
                );
                spin.upcast()
            }
            Node::Int {
                id,
                min,
                max,
                step,
                suffix,
                special,
                tip,
            } => {
                let spin = spin_button(min as f64, max as f64, step as f64, 0, suffix, special);
                tooltip(&spin, tip);
                self.controls.insert(
                    id,
                    Control::Num {
                        spin: spin.clone(),
                        min: min as f64,
                        max: max as f64,
                        dec: 0,
                    },
                );
                spin.upcast()
            }
            Node::Combo { id, items, tip } => {
                let texts: Vec<&str> = items.iter().map(|(t, _)| *t).collect();
                let drop = gtk::DropDown::from_strings(&texts);
                tooltip(&drop, tip);
                self.controls.insert(
                    id,
                    Control::Choice {
                        drop: drop.clone(),
                        data: items.iter().map(|(_, d)| *d).collect(),
                    },
                );
                drop.upcast()
            }
            Node::EditCombo {
                id,
                placeholder,
                tip,
            } => self.edit_combo(id, placeholder, tip),
            Node::Text {
                id,
                password,
                placeholder,
                tip,
            } => self.entry(id, password, placeholder, tip),
            Node::Multiline {
                id,
                placeholder: _,
                tip,
            } => {
                let view = gtk::TextView::new();
                view.set_accepts_tab(false);
                view.set_monospace(true);
                tooltip(&view, tip);
                let scroll = gtk::ScrolledWindow::new();
                scroll.set_child(Some(&view));
                scroll.set_min_content_height(88);
                scroll.set_hscrollbar_policy(gtk::PolicyType::Automatic);
                scroll.add_css_class("field");
                self.controls.insert(id, Control::Multi(view));
                scroll.upcast()
            }
            Node::Checks { id, items } => {
                let list = gtk::ListBox::new();
                list.set_selection_mode(gtk::SelectionMode::None);
                list.add_css_class("pick-list");
                let mut boxes = Vec::new();
                for text in items {
                    let c = gtk::CheckButton::with_label(text);
                    list.append(&c);
                    boxes.push(c);
                }
                // each row is a check button; the list takes no focus of its own
                list.set_focusable(false);
                self.controls.insert(
                    id,
                    Control::Checks {
                        boxes,
                        items: items.to_vec(),
                    },
                );
                list.upcast()
            }
            Node::Button { id, text, tip } => {
                let b = gtk::Button::with_label(text);
                b.set_halign(gtk::Align::Start);
                tooltip(&b, tip);
                self.controls.insert(id, Control::Button(b.clone()));
                b.upcast()
            }
            Node::Pill { id, text, tip } => {
                let b = widgets::pill_button(text, false, false);
                tooltip(&b, tip);
                self.controls.insert(id, Control::Button(b.clone()));
                b.upcast()
            }
            Node::Icon { id, glyph, tip } => {
                let b = widgets::icon_button(glyph, tip, true);
                self.controls.insert(id, Control::Button(b.clone()));
                b.upcast()
            }
            Node::Stretch => {
                let s = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                s.set_hexpand(true);
                s.upcast()
            }
            Node::Live { id, wrap, hint } => {
                let l = gtk::Label::new(None);
                l.set_xalign(0.0);
                if hint {
                    l.add_css_class("hint");
                }
                l.set_wrap(wrap);
                if wrap {
                    l.set_max_width_chars(60);
                }
                self.named.push((None, l.clone().upcast()));
                self.controls.insert(id, Control::Live(l.clone()));
                l.upcast()
            }
        }
    }

    /// An editable combo: the line edit and an arrow that opens the list. Picking a row writes its
    /// key into the line edit (`_on_service_selected`).
    fn edit_combo(&mut self, id: &'static str, placeholder: &str, tip: &str) -> gtk::Widget {
        let entry = self.entry(id, false, placeholder, tip);
        entry.set_hexpand(true);
        let line = match self.controls.get(id) {
            Some(Control::Text(e)) => e.clone(),
            _ => unreachable!("entry() just inserted it"),
        };
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_activate_on_single_click(true);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_propagate_natural_height(true);
        // wide enough for the longest service name, as Qt's combo popup is, not a list that clips it
        scroll.set_propagate_natural_width(true);
        scroll.set_max_content_height(240);
        scroll.set_min_content_width(260);
        let popover = gtk::Popover::new();
        popover.set_child(Some(&scroll));
        let arrow = gtk::MenuButton::new();
        arrow.set_icon_name("pan-down-symbolic");
        arrow.set_popover(Some(&popover));
        arrow.add_css_class("field");
        arrow.set_tooltip_text(Some("The tag services Hydrus reported"));
        arrow.set_visible(false);
        let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let (k, p) = (keys.clone(), popover.clone());
        list.connect_row_activated(move |_, row| {
            let key = k.borrow().get(row.index() as usize).cloned();
            if let Some(key) = key.filter(|k| !k.is_empty()) {
                line.set_text(&key);
            }
            p.popdown();
        });
        let line_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        line_box.append(&entry);
        line_box.append(&arrow);
        self.combos.insert(
            id,
            Combo {
                arrow,
                list,
                popover,
                keys,
            },
        );
        line_box.upcast()
    }

    fn entry(
        &mut self,
        id: &'static str,
        password: bool,
        placeholder: &str,
        tip: &str,
    ) -> gtk::Widget {
        let e = gtk::Entry::new();
        e.add_css_class("field");
        e.set_visibility(!password);
        if !placeholder.is_empty() {
            e.set_placeholder_text(Some(placeholder));
        }
        e.set_width_chars(24);
        tooltip(&e, tip);
        self.controls.insert(id, Control::Text(e.clone()));
        e.upcast()
    }

    /// Puts every control this page has a value for into its widget.
    pub fn write(&self, values: &Values) {
        for (id, control) in &self.controls {
            let Some(v) = values.0.get(*id) else { continue };
            match (control, v) {
                (Control::Check(c), Value::Bool(b)) => c.set_active(*b),
                (
                    Control::Num {
                        spin,
                        min,
                        max,
                        dec,
                    },
                    Value::Num(n),
                ) => spin.set_value(if *dec == 0 {
                    qt_int(*n as i64, *min as i64, *max as i64) as f64
                } else {
                    qt_double(*n, *min, *max, *dec)
                }),
                (Control::Choice { drop, data }, Value::Choice(d)) => {
                    drop.set_selected(data.iter().position(|x| x == d).unwrap_or(0) as u32)
                }
                (Control::Text(e), Value::Text(t)) => e.set_text(t),
                (Control::Multi(view), Value::Text(t)) => view
                    .buffer()
                    .set_text(&crate::settings_values::qt_plain_text(t)),
                (Control::Checks { boxes, items }, Value::Checked(on)) => {
                    for (b, name) in boxes.iter().zip(items) {
                        b.set_active(on.iter().any(|o| o == name));
                    }
                }
                _ => {}
            }
        }
    }

    /// Every control's value as it reads now.
    pub fn read(&self) -> Values {
        let mut out = Values::default();
        for (id, control) in &self.controls {
            let v = match control {
                Control::Check(c) => Value::Bool(c.is_active()),
                Control::Num {
                    spin,
                    min,
                    max,
                    dec,
                } => {
                    // commit what is typed, as Qt's value() already reflects it
                    spin.update();
                    let n = spin.value();
                    Value::Num(if *dec == 0 {
                        qt_int(n.round() as i64, *min as i64, *max as i64) as f64
                    } else {
                        qt_double(n, *min, *max, *dec)
                    })
                }
                Control::Choice { drop, data } => Value::Choice(
                    data.get(drop.selected() as usize)
                        .copied()
                        .unwrap_or_default()
                        .to_owned(),
                ),
                Control::Text(e) => Value::Text(e.text().to_string()),
                Control::Multi(view) => {
                    let b = view.buffer();
                    Value::Text(b.text(&b.start_iter(), &b.end_iter(), false).to_string())
                }
                Control::Checks { boxes, items } => Value::Checked(
                    boxes
                        .iter()
                        .zip(items)
                        .filter(|(b, _)| b.is_active())
                        .map(|(_, n)| (*n).to_owned())
                        .collect(),
                ),
                Control::Button(_) | Control::Live(_) => continue,
            };
            out.set(id, v);
        }
        out
    }
}

fn tooltip(w: &impl IsA<gtk::Widget>, tip: &str) {
    if !tip.is_empty() {
        w.set_tooltip_text(Some(tip));
    }
}

/// A `QDoubleSpinBox` / `QSpinBox`: the suffix after the number, the special text at the minimum.
pub(crate) fn spin_button(
    min: f64,
    max: f64,
    step: f64,
    dec: u32,
    suffix: &'static str,
    special: &'static str,
) -> gtk::SpinButton {
    let adj = gtk::Adjustment::new(min, min, max, step, step * 10.0, 0.0);
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, dec);
    spin.set_numeric(false);
    spin.set_width_chars(10);
    spin.connect_output(move |s| {
        let v = s.value();
        let text = if !special.is_empty() && v <= min {
            special.to_owned()
        } else {
            format!("{v:.*}{suffix}", dec as usize)
        };
        s.set_text(&text);
        gtk::glib::Propagation::Stop
    });
    spin.connect_input(move |s| {
        let text = s.text().to_string();
        let text = text.trim();
        if !special.is_empty() && text == special.trim() {
            return Some(Ok(min));
        }
        let number = text.strip_suffix(suffix.trim()).unwrap_or(text).trim();
        Some(number.parse::<f64>().map_err(|_| ()))
    });
    spin
}

impl Form {
    /// The Find box's names: fixed texts as built, a live label's as it reads now.
    pub fn names(&self) -> Vec<(String, gtk::Widget)> {
        self.named
            .iter()
            .map(|(fixed, w)| {
                let text = match fixed {
                    Some(t) => t.clone(),
                    None => w
                        .downcast_ref::<gtk::Label>()
                        .map(|l| l.text().to_string())
                        .unwrap_or_default(),
                };
                (text, w.clone())
            })
            .collect()
    }

    pub fn live(&self, id: &str) -> Option<&gtk::Label> {
        match self.controls.get(id) {
            Some(Control::Live(l)) => Some(l),
            _ => None,
        }
    }

    /// Runs `f` whenever the control `id` changes by hand or by `write`.
    pub fn on_change(&self, id: &str, f: impl Fn() + Clone + 'static) {
        match self.controls.get(id) {
            Some(Control::Check(c)) => {
                c.connect_toggled(move |_| f());
            }
            Some(Control::Num { spin, .. }) => {
                spin.connect_value_changed(move |_| f());
            }
            Some(Control::Choice { drop, .. }) => {
                drop.connect_selected_notify(move |_| f());
            }
            _ => {}
        }
    }

    pub fn button(&self, id: &str) -> Option<&gtk::Button> {
        match self.controls.get(id) {
            Some(Control::Button(b)) => Some(b),
            _ => None,
        }
    }

    pub fn combo(&self, id: &str) -> Option<&Combo> {
        self.combos.get(id)
    }

    pub fn entry_of(&self, id: &str) -> Option<&gtk::Entry> {
        match self.controls.get(id) {
            Some(Control::Text(e)) => Some(e),
            _ => None,
        }
    }
}

impl Form {
    pub fn check(&self, id: &str) -> Option<&gtk::CheckButton> {
        match self.controls.get(id) {
            Some(Control::Check(c)) => Some(c),
            _ => None,
        }
    }
}

/// A label that is a paragraph, not the caption of a control: it has a line break of its own or
/// runs past what a caption beside a control does.
fn is_prose(text: &str) -> bool {
    text.contains('\n') || text.chars().count() > PROSE_CHARS
}

/// Longest caption (characters) that stays on one line beside its control.
const PROSE_CHARS: usize = 48;
