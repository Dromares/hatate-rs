//! Review > Shortcuts (`gui/review_view.py` `ReviewViewMixin._show_shortcuts_dialog`, `:619`, at
//! `76c85c4`): a read-only list of every review action and the key it has now.
//!
//! The list is `shortcuts::REVIEW_ACTIONS` in order, each with the binding `resolved_shortcuts`
//! gives, so it shows what the keys do (the Settings overrides included) and `(unbound)` for an
//! action with no key. The one button is Close, which is also the default; the dialog's result is
//! Rejected whichever way it closes (`buttons.rejected.connect(dialog.reject)`).

use crate::widgets;
use crate::window::MainWindow;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use hatate_core::config::OrderedMap;
use hatate_core::shortcuts::REVIEW_ACTIONS;
use std::rc::Rc;

pub const TITLE: &str = "Review Keyboard Shortcuts";
pub const HEADING: &str = "Review Mode Keyboard Shortcuts";
pub const SUBTITLE: &str = "Current effective bindings (from Settings \u{2192} Shortcuts):";
pub const CLOSE: &str = "Close";
/// `dialog.setMinimumWidth(400)`.
pub const MIN_WIDTH: i32 = 400;
/// What an action with no key shows.
pub const UNBOUND: &str = "(unbound)";

/// The rows, in order: `(f"{action.label}:", key or "(unbound)")` for each action.
pub fn rows(bindings: &OrderedMap<String>) -> Vec<(String, String)> {
    REVIEW_ACTIONS
        .iter()
        .map(|a| {
            let key = bindings
                .0
                .iter()
                .find(|(k, _)| k == a.id)
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            let shown = if key.is_empty() { UNBOUND } else { key };
            (format!("{}:", a.label), shown.to_owned())
        })
        .collect()
}

pub struct ShortcutsDialog {
    pub window: gtk::Window,
    pub close_button: gtk::Button,
    labels: Vec<(gtk::Label, gtk::Label)>,
    main: MainWindow,
}

impl ShortcutsDialog {
    pub fn build(main: &MainWindow) -> Rc<ShortcutsDialog> {
        let window = gtk::Window::builder()
            .title(TITLE)
            .transient_for(&main.win)
            .modal(true)
            .default_width(MIN_WIDTH)
            .default_height(520)
            .build();
        window.set_size_request(MIN_WIDTH, 360);
        window.add_css_class("pick-window");
        window.add_css_class("shortcuts-window");

        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let (l, t, r, b) = widgets::PAGE_MARGINS;
        column.set_margin_start(l);
        column.set_margin_top(t);
        column.set_margin_end(r);
        column.set_margin_bottom(b);
        column.append(&widgets::heading(HEADING));
        column.append(&widgets::muted(SUBTITLE));

        let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let mut labels = Vec::new();
        for (text, key) in rows(&main.resolved_shortcuts()) {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let label = widgets::muted(&text);
            label.set_hexpand(true);
            label.set_wrap(true);
            let key_label = gtk::Label::new(Some(&key));
            key_label.add_css_class("hint");
            key_label.set_xalign(1.0);
            key_label.set_valign(gtk::Align::Center);
            row.append(&label);
            row.append(&key_label);
            list.append(&row);
            labels.push((label, key_label));
        }
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        column.append(&scroll);

        let close_button = widgets::pill_button(CLOSE, true, false);
        close_button.set_halign(gtk::Align::End);
        column.append(&close_button);
        window.set_child(Some(&column));
        window.set_default_widget(Some(&close_button));

        let dialog = Rc::new(ShortcutsDialog {
            window: window.clone(),
            close_button: close_button.clone(),
            labels,
            main: main.clone(),
        });
        {
            let w = window.clone();
            close_button.connect_clicked(move |_| w.close());
        }
        {
            let d = Rc::downgrade(&dialog);
            window.connect_close_request(move |_| {
                if let Some(d) = d.upgrade() {
                    d.main.s.shortcuts_dialog.borrow_mut().take();
                }
                glib::Propagation::Proceed
            });
        }
        {
            // Escape is Reject
            let controller = gtk::ShortcutController::new();
            let w = window.clone();
            controller.add_shortcut(gtk::Shortcut::new(
                Some(gtk::KeyvalTrigger::new(
                    gtk::gdk::Key::Escape,
                    gtk::gdk::ModifierType::empty(),
                )),
                Some(gtk::CallbackAction::new(move |_, _| {
                    w.close();
                    glib::Propagation::Stop
                })),
            ));
            window.add_controller(controller);
        }
        dialog
    }

    /// The text of each row: `(action label, key)`. Tests only.
    pub fn texts(&self) -> Vec<(String, String)> {
        self.labels
            .iter()
            .map(|(l, k)| (l.text().to_string(), k.text().to_string()))
            .collect()
    }

    /// The text of every button. Tests only.
    pub fn button_texts(&self) -> Vec<String> {
        vec![
            self.close_button
                .label()
                .map(|l| l.to_string())
                .unwrap_or_default(),
        ]
    }

    /// The button Enter presses. Tests only.
    pub fn default_button_text(&self) -> Option<String> {
        let def = self.window.default_widget()?;
        def.downcast::<gtk::Button>()
            .ok()
            .and_then(|b| b.label().map(|l| l.to_string()))
    }

    pub fn click_close(&self) {
        self.close_button.emit_clicked();
    }
}

impl MainWindow {
    /// `_show_shortcuts_dialog`. A second press while it is open raises it, as a modal dialog
    /// takes the clicks anyway.
    pub fn action_show_shortcuts(&self) {
        if let Some(open) = self.s.shortcuts_dialog.borrow().as_ref() {
            open.window.present();
            return;
        }
        let dialog = ShortcutsDialog::build(self);
        dialog.window.present();
        *self.s.shortcuts_dialog.borrow_mut() = Some(dialog);
    }
}
