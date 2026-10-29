//! The Queue's right-click menu on screen (`gui/table_context_menu.py` `show`, and
//! `_show_table_context_menu`, `main_window.py` at `76c85c4`; DAN-2217).
//!
//! What the menu says and when an entry is enabled is `hatate_core::table_menu`. This file turns
//! it into a popover, resolves the selection once when the menu is built (a row number can move
//! while a menu is open; an id does not), and runs the picked entry. The Compare and Hydrus
//! entries are in the menu and disabled: 6.f and 6.g give them handlers.

use crate::window::MainWindow;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio};
use hatate_core::model::Entry;
use hatate_core::queue::EntryId;
use hatate_core::table_menu::{self, Action, Item, Line, Menu};

/// Whether this entry has a handler yet.
pub fn is_wired(action: Action) -> bool {
    matches!(
        action,
        Action::SelectByStatus(_)
            | Action::SelectByPredicate(_)
            | Action::Reset
            | Action::SetReviewed(_)
            | Action::Export
            | Action::WriteTagFiles
            | Action::Remove
            | Action::Research
            | Action::ResearchWithEngine(_)
            | Action::ResearchWithOppositeEngine
            | Action::CheckUpscaling
            | Action::CheckAvailability
            | Action::Compare
    )
}

impl MainWindow {
    /// The menu for the current selection (`table_context_menu.build`).
    pub fn table_menu(&self, selected: &[EntryId]) -> Menu {
        let q = self.s.queue.borrow();
        let picked: Vec<&Entry> = selected.iter().filter_map(|id| q.entry(*id)).collect();
        table_menu::build(q.entries(), &picked)
    }

    /// Runs the entry that was picked, on the rows that were selected when the menu opened.
    pub fn run_menu_action(&self, action: Action, ids: &[EntryId]) {
        match action {
            Action::SelectByStatus(s) => self.select_rows_by_status(s),
            Action::SelectByPredicate(p) => {
                self.select_rows_by_predicate(p.description(), |e| p.test(e))
            }
            Action::Reset => self.reset_rows(ids),
            Action::SetReviewed(reviewed) => self.set_rows_reviewed(ids, reviewed),
            Action::Export => self.action_export_results(Some(ids)),
            Action::WriteTagFiles => self.action_write_tag_files(Some(ids)),
            Action::Remove => self.remove_rows(ids),
            Action::Research => self.research_rows(ids),
            Action::ResearchWithEngine(engine) => self.research_rows_with_engine(ids, engine),
            Action::ResearchWithOppositeEngine => self.research_rows_with_opposite_engine(ids),
            Action::CheckUpscaling => self.check_rows_for_upscaling(ids),
            // The menu enables this for exactly one row.
            Action::CheckAvailability => {
                if let Some(id) = ids.first() {
                    self.check_match_availability(*id);
                }
            }
            Action::Compare => {
                if let Some(id) = ids.first() {
                    self.open_compare_dialog(*id);
                }
            }
            // DEFERRED(6.g): the Hydrus entries.
            _ => {}
        }
    }

    /// `_show_table_context_menu(pos)`.
    pub fn show_table_context_menu(&self, x: f64, y: f64) {
        let ids = self.s.queue_view.selected_ids();
        let menu = self.table_menu(&ids);
        let model = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        let mut next = 0usize;
        let mut add = |item: &Item, target: &gio::Menu| {
            let name = format!("a{next}");
            next += 1;
            let act = gio::SimpleAction::new(&name, None);
            act.set_enabled(item.enabled && is_wired(item.action));
            let (w, action, ids) = (self.downgrade(), item.action, ids.clone());
            act.connect_activate(move |_, _| {
                if let Some(w) = w.upgrade() {
                    w.run_menu_action(action, &ids);
                }
            });
            group.add_action(&act);
            let entry = gio::MenuItem::new(Some(&item.label), Some(&format!("rowmenu.{name}")));
            if !item.tooltip.is_empty() {
                entry.set_attribute_value("tooltip", Some(&item.tooltip.to_variant()));
            }
            target.append_item(&entry);
        };
        for (title, items) in &menu.select_by {
            let sub = gio::Menu::new();
            for item in items {
                add(item, &sub);
            }
            model.append_submenu(Some(title), &sub);
        }
        let mut section = |lines: &[Line], target: &gio::Menu| {
            let mut block = gio::Menu::new();
            for line in lines {
                match line {
                    Line::Item(item) => add(item, &block),
                    Line::Separator => {
                        target.append_section(None, &block);
                        block = gio::Menu::new();
                    }
                }
            }
            target.append_section(None, &block);
        };
        if let Some(rows) = &menu.rows {
            let acting = gio::Menu::new();
            section(&rows.head, &acting);
            let engines = gio::Menu::new();
            section(&rows.engines, &engines);
            acting.append_submenu(Some("Search with a specific engine"), &engines);
            section(&rows.tail, &acting);
            model.append_section(None, &acting);
        }
        let popover = gtk::PopoverMenu::from_model(Some(&model));
        popover.insert_action_group("rowmenu", Some(&group));
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let view = self.s.queue_view.view.clone();
        popover.set_parent(&view);
        let (x, y) = if x == 0.0 && y == 0.0 {
            // From the keyboard: over the first selected row, or the top left of the list.
            let row = ids
                .first()
                .and_then(|id| self.s.queue_view.row_of(*id))
                .unwrap_or(0);
            (
                24.0,
                f64::from(hatate_core::thumb::QUEUE_ROW_HEIGHT) * (f64::from(row) + 1.5),
            )
        } else {
            (x, y)
        };
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|p| {
            let p = p.clone();
            gtk::glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    /// Wires the Queue's right click and its keyboard path to the menu.
    pub fn wire_table_context_menu(&self) {
        let w = self.downgrade();
        self.s.queue_view.connect_context_menu(move |_, x, y| {
            if let Some(w) = w.upgrade() {
                w.show_table_context_menu(x, y);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 6.e gives the re-search, upscale-check and availability entries their handlers, 6.f-2 gives
    /// Compare its; the Hydrus entries (6.g) stay off.
    #[test]
    fn the_entries_6e_owns_are_wired_and_the_later_ones_are_not() {
        for a in [
            Action::Research,
            Action::ResearchWithEngine("iqdb"),
            Action::ResearchWithOppositeEngine,
            Action::CheckUpscaling,
            Action::CheckAvailability,
            Action::Compare,
        ] {
            assert!(is_wired(a), "{a:?}");
        }
        for a in [
            Action::SendToHydrus,
            Action::ShowInHydrus,
            Action::DeleteFromHydrus,
        ] {
            assert!(!is_wired(a), "{a:?}");
        }
    }
}
