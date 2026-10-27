//! The menubar's keyboard path (DAN-2246).
//!
//! `GtkPopoverMenuBar` handles some keys itself: Alt plus a mnemonic opens that menu, Left and
//! Right move between open menus, Escape closes one. Measured on the real window under Xvfb with
//! real X key events (`tests/smoke.rs` `menubar_keyboard_path`), two things are missing:
//!
//! - **F10 does not open a menu.** GTK 4 only moves focus onto the first menubar item, and a bar
//!   that is not the application's own `GMenu` bar gets nothing more. [`install`] makes F10 open
//!   the first menu, and F10 again close it.
//! - **Escape leaves focus on the menubar.** The widget that had focus before the menu was entered
//!   never gets it back, so a keyboard user is dropped at the top of the window. [`install`]
//!   remembers the last focus outside the bar and returns to it once no menu is open.
//!
//! Alt mnemonics, Left and Right are left to GTK: a second handler on top of them would open a menu
//! twice. The smoke pins that GTK still does them.

use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

/// The popover of each top-level menu of `bar`, in order.
pub fn menu_popovers(bar: &gtk::PopoverMenuBar) -> Vec<gtk::PopoverMenu> {
    let mut out = Vec::new();
    let mut item = bar.first_child();
    while let Some(i) = item {
        let mut c = i.first_child();
        while let Some(child) = c {
            if let Ok(p) = child.clone().downcast::<gtk::PopoverMenu>() {
                out.push(p);
                break;
            }
            c = child.next_sibling();
        }
        item = i.next_sibling();
    }
    out
}

fn any_open(bar: &gtk::PopoverMenuBar) -> bool {
    menu_popovers(bar).iter().any(|p| p.is_visible())
}

/// Wires F10 and the focus return onto `win` for `bar`. Everything held across the window's life
/// is a weak reference: the window owns the bar and its controllers.
pub fn install(win: &adw::ApplicationWindow, bar: &gtk::PopoverMenuBar) {
    // The last widget with focus that is not inside the bar.
    let before: Rc<RefCell<Option<glib::WeakRef<gtk::Widget>>>> = Rc::new(RefCell::new(None));

    let window: &gtk::Window = win.upcast_ref();
    {
        let before = before.clone();
        let bar = bar.downgrade();
        window.connect_focus_widget_notify(move |w| {
            let (Some(focus), Some(bar)) = (gtk::prelude::GtkWindowExt::focus(w), bar.upgrade())
            else {
                return;
            };
            if !focus.is_ancestor(&bar) && focus != bar.clone().upcast::<gtk::Widget>() {
                *before.borrow_mut() = Some(focus.downgrade());
            }
        });
    }

    // Once the last menu is closed, hand focus back (Escape, or a chosen item). Left and Right
    // close one popover and open the next, so look after the loop has settled.
    for p in menu_popovers(bar) {
        let bar = bar.downgrade();
        let win = win.downgrade();
        let before = before.clone();
        p.connect_closed(move |_| {
            let (bar, win, before) = (bar.clone(), win.clone(), before.clone());
            glib::idle_add_local_once(move || {
                let (Some(bar), Some(win)) = (bar.upgrade(), win.upgrade()) else {
                    return;
                };
                if any_open(&bar) {
                    return;
                }
                let window: &gtk::Window = win.upcast_ref();
                // Only if focus is still on the bar: a click elsewhere has moved it on already.
                let on_bar = gtk::prelude::GtkWindowExt::focus(window).is_none_or(|f| {
                    f.is_ancestor(&bar) || f == bar.clone().upcast::<gtk::Widget>()
                });
                if !on_bar {
                    return;
                }
                // The borrow ends first: `grab_focus` re-enters the focus handler above.
                let target = before.borrow().as_ref().and_then(glib::WeakRef::upgrade);
                if let Some(w) = target {
                    w.grab_focus();
                }
            });
        });
    }

    // The window sees F10 while focus is in the page; an open menu holds the keyboard, so each
    // popover has the same controller to close itself.
    win.add_controller(f10(bar));
    for p in menu_popovers(bar) {
        p.add_controller(f10(bar));
    }
}

/// F10, no modifiers: open the first menu, or close the open one.
fn f10(bar: &gtk::PopoverMenuBar) -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    // Capture: an entry or a list that has focus must not swallow the key first.
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let bar = bar.downgrade();
    keys.connect_key_pressed(move |_, key, _, state| {
        let plain = state
            .intersection(gtk::accelerator_get_default_mod_mask())
            .is_empty();
        if key != gdk::Key::F10 || !plain {
            return glib::Propagation::Proceed;
        }
        let Some(bar) = bar.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let menus = menu_popovers(&bar);
        if let Some(open) = menus.iter().find(|p| p.is_visible()) {
            open.popdown();
        } else if let Some(item) = menus.first().and_then(|p| p.parent()) {
            // The route an Alt mnemonic takes, so the bar knows a menu is open and Left and Right
            // keep working. `popup()` on the popover alone opens it but not that state.
            item.mnemonic_activate(false);
        }
        glib::Propagation::Stop
    });
    keys
}
