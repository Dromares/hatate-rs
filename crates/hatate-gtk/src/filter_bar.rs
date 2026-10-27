//! The row of controls above the list (`gui/filter_bar.py`): filename text and multi-select
//! Status, Site and Upscale menus over [`EntryFilter`], a Clear link and the hidden-count reading.
//!
//! A filter only changes what is displayed. The bar owns what is ticked and reports when that
//! moves; the window decides what to do about it (`_apply_filter`).

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;
use hatate_core::entry_filter::{
    EntryFilter, describe_hidden, hidden_count_text, sites_present, statuses_present,
    summarise_selection, upscale_verdicts_present,
};
use hatate_core::model::MatchStatus;
use hatate_core::queue::QueueModel;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use crate::widgets;

/// `FILTER_BAND_MARGINS`, `FILTER_BAND_SPACING`.
const BAND_MARGINS: (i32, i32) = (16, 10);
const BAND_SPACING: i32 = 12;

const STATUS_TIP: &str = "Show only rows with the statuses you tick. Only the statuses actually present in the list are offered.";
const SITE_TIP: &str = "Show only rows whose match came from the sites you tick, by the site shown in the Booru column.\n\n\"(no match)\" collects rows with no match at all - not the same as \"Other\", which means a match from a site with no name of its own.";
const UPSCALE_TIP: &str = "Show only rows with the Check for Upscaling verdict you tick.";
const ALL_TIP: &str = "Tick everything - the same as not filtering on this at all.";
const NONE_TIP: &str = "Untick everything. The list goes empty until you tick something back on - which is the quick way to isolate one value out of many.";

/// One of the three multi-select menus. `chosen` of `None` is "not filtering".
struct Dim {
    prefix: &'static str,
    button: gtk::MenuButton,
    group: gio::SimpleActionGroup,
    name: &'static str,
    chosen: RefCell<Option<HashSet<String>>>,
    /// What the menu offers now, in menu order.
    present: RefCell<Vec<String>>,
}

pub struct FilterBar {
    pub root: gtk::Box,
    pub text: gtk::Entry,
    pub clear: gtk::Button,
    pub count: gtk::Label,
    status: Dim,
    site: Dim,
    upscale: Dim,
    queue: Rc<RefCell<QueueModel>>,
    changed: RefCell<Option<Rc<dyn Fn()>>>,
    quiet: Cell<bool>,
}

impl Dim {
    fn new(prefix: &'static str, name: &'static str, tip: &str) -> Dim {
        let button = gtk::MenuButton::new();
        button.set_label(&format!("{prefix}: All"));
        button.add_css_class("field");
        button.set_tooltip_text(Some(tip));
        button.set_always_show_arrow(true);
        let group = gio::SimpleActionGroup::new();
        button.insert_action_group(name, Some(&group));
        Dim {
            prefix,
            button,
            group,
            name,
            chosen: RefCell::new(None),
            present: RefCell::new(Vec::new()),
        }
    }
}

impl FilterBar {
    pub fn new(queue: Rc<RefCell<QueueModel>>) -> Rc<FilterBar> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, BAND_SPACING);
        root.add_css_class("filter-band");
        root.set_margin_start(BAND_MARGINS.0);
        root.set_margin_end(BAND_MARGINS.0);
        root.set_margin_top(BAND_MARGINS.1);
        root.set_margin_bottom(BAND_MARGINS.1);
        root.append(&widgets::section_label("01 // Filter"));

        let text = gtk::Entry::new();
        text.set_placeholder_text(Some("Filename contains…"));
        text.add_css_class("field");
        text.set_hexpand(true);
        text.update_property(&[gtk::accessible::Property::Label("Filename contains")]);
        root.append(&text);

        let status = Dim::new("Status", "fstatus", STATUS_TIP);
        let site = Dim::new("Site", "fsite", SITE_TIP);
        let upscale = Dim::new("Upscale", "fupscale", UPSCALE_TIP);
        root.append(&status.button);
        root.append(&site.button);
        root.append(&upscale.button);

        let clear = widgets::link_button("Clear");
        clear.set_tooltip_text(Some("Show everything again."));
        clear.set_sensitive(false);
        root.append(&clear);

        let spacer = gtk::Label::new(None);
        spacer.set_hexpand(true);
        root.append(&spacer);

        let count = gtk::Label::new(None);
        count.add_css_class("filter-count");
        count.set_xalign(1.0);
        root.append(&count);

        let this = Rc::new(FilterBar {
            root,
            text,
            clear,
            count,
            status,
            site,
            upscale,
            queue,
            changed: RefCell::new(None),
            quiet: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.text.connect_changed(move |_| {
            if let Some(this) = weak.upgrade() {
                if !this.quiet.get() {
                    this.emit();
                }
            }
        });
        let weak = Rc::downgrade(&this);
        this.clear.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.clear_all();
            }
        });
        this.rebuild_menus();
        this
    }

    /// The Status, Site or Upscale button (`filter_status_button` and its siblings), by prefix.
    pub fn button(&self, prefix: &str) -> &gtk::MenuButton {
        match prefix {
            "Status" => &self.status.button,
            "Site" => &self.site.button,
            _ => &self.upscale.button,
        }
    }

    /// What a menu offers now, in menu order (the checkable actions of `filter_*_menu`).
    pub fn present(&self, prefix: &str) -> Vec<String> {
        let which = match prefix {
            "Status" => 0,
            "Site" => 1,
            _ => 2,
        };
        self.dim(which).present.borrow().clone()
    }

    /// Whether the nth entry of a menu is ticked.
    pub fn ticked(&self, prefix: &str, n: usize) -> bool {
        let which = match prefix {
            "Status" => 0,
            "Site" => 1,
            _ => 2,
        };
        let dim = self.dim(which);
        dim.group
            .lookup_action(&format!("v{n}"))
            .and_then(|a| a.state())
            .and_then(|s| s.get::<bool>())
            .unwrap_or(false)
    }

    /// `changed.connect`.
    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(f));
    }

    fn emit(&self) {
        let hook = self.changed.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// `current_filter()`.
    pub fn current_filter(&self) -> EntryFilter {
        let statuses = self.status.chosen.borrow().as_ref().map(|labels| {
            MatchStatus::ALL
                .into_iter()
                .filter(|s| labels.contains(s.label()))
                .collect()
        });
        EntryFilter {
            statuses,
            sites: self.site.chosen.borrow().clone(),
            upscale_verdicts: self.upscale.chosen.borrow().clone(),
            text: self.text.text().to_string(),
        }
    }

    fn dim(&self, which: u8) -> &Dim {
        match which {
            0 => &self.status,
            1 => &self.site,
            _ => &self.upscale,
        }
    }

    /// What each menu would offer for the list as it is now.
    fn offered(&self, which: u8) -> Vec<String> {
        let q = self.queue.borrow();
        let entries = q.entries();
        match which {
            0 => statuses_present(entries)
                .into_iter()
                .map(|s| s.label().to_owned())
                .collect(),
            1 => sites_present(entries),
            _ => upscale_verdicts_present(entries)
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    }

    /// `rebuild_menus()`: repopulates the menus from what the list now holds. Offering only what
    /// is present keeps the menus short, and means a site cannot be ticked into showing nothing.
    /// A menu that would offer what it already offers is left alone, so a popover that is open
    /// does not change under the pointer.
    pub fn rebuild_menus(self: &Rc<Self>) {
        for which in 0..3u8 {
            let offered = self.offered(which);
            let dim = self.dim(which);
            if *dim.present.borrow() == offered && dim.button.menu_model().is_some() {
                self.sync_checks(which);
                continue;
            }
            for name in dim.group.list_actions() {
                dim.group.remove_action(&name);
            }
            let menu = gio::Menu::new();
            if !offered.is_empty() {
                let head = gio::Menu::new();
                for (label, action, tip) in [("All", "all", ALL_TIP), ("None", "none", NONE_TIP)] {
                    let item =
                        gio::MenuItem::new(Some(label), Some(&format!("{}.{action}", dim.name)));
                    item.set_attribute_value("tooltip", Some(&tip.to_variant()));
                    head.append_item(&item);
                    let act = gio::SimpleAction::new(action, None);
                    let weak = Rc::downgrade(self);
                    let checked = action == "all";
                    act.connect_activate(move |_, _| {
                        if let Some(this) = weak.upgrade() {
                            this.set_all(which, checked);
                        }
                    });
                    dim.group.add_action(&act);
                }
                menu.append_section(None, &head);
            }
            let items = gio::Menu::new();
            for (i, label) in offered.iter().enumerate() {
                let name = format!("v{i}");
                items.append(Some(label), Some(&format!("{}.{name}", dim.name)));
                let on = dim
                    .chosen
                    .borrow()
                    .as_ref()
                    .is_none_or(|c| c.contains(label));
                let act = gio::SimpleAction::new_stateful(&name, None, &on.to_variant());
                let weak = Rc::downgrade(self);
                let label = label.clone();
                act.connect_activate(move |act, _| {
                    let now = act.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
                    act.set_state(&(!now).to_variant());
                    if let Some(this) = weak.upgrade() {
                        this.toggled(which, &label, !now);
                    }
                });
                dim.group.add_action(&act);
            }
            menu.append_section(None, &items);
            dim.button.set_menu_model(Some(&menu));
            *dim.present.borrow_mut() = offered;
        }
    }

    /// Puts the ticks back in step with `chosen` without firing a handler.
    fn sync_checks(&self, which: u8) {
        let dim = self.dim(which);
        let chosen = dim.chosen.borrow();
        for (i, label) in dim.present.borrow().iter().enumerate() {
            if let Some(act) = dim
                .group
                .lookup_action(&format!("v{i}"))
                .and_downcast::<gio::SimpleAction>()
            {
                act.set_state(
                    &chosen
                        .as_ref()
                        .is_none_or(|c| c.contains(label))
                        .to_variant(),
                );
            }
        }
    }

    /// `_set_all_statuses(checked)` and its siblings, by prefix.
    pub fn set_all_named(&self, prefix: &str, checked: bool) {
        let which = match prefix {
            "Status" => 0,
            "Site" => 1,
            _ => 2,
        };
        self.set_all(which, checked);
    }

    /// `set_all_checked` + `_set_all_*`: everything ticked is stored as `None` ("not filtering"),
    /// nothing ticked is a real empty set, which matches no row.
    fn set_all(&self, which: u8, checked: bool) {
        *self.dim(which).chosen.borrow_mut() = if checked { None } else { Some(HashSet::new()) };
        self.sync_checks(which);
        self.emit();
    }

    /// `_on_*_toggled`.
    fn toggled(&self, which: u8, label: &str, checked: bool) {
        let dim = self.dim(which);
        let present: HashSet<String> = self.offered(which).into_iter().collect();
        let mut current: HashSet<String> = match dim.chosen.borrow().as_ref() {
            None => present.clone(),
            Some(c) => c.clone(),
        };
        if checked {
            current.insert(label.to_owned());
        } else {
            current.remove(label);
        }
        *dim.chosen.borrow_mut() = if current.is_superset(&present) {
            None
        } else {
            Some(current)
        };
        self.emit();
    }

    /// `refresh(shown, total, active)`: keeps the buttons and the count in step with the filter.
    pub fn refresh(&self, shown: usize, total: usize, active: bool) {
        for which in 0..3u8 {
            let dim = self.dim(which);
            let present: HashSet<String> = self.offered(which).into_iter().collect();
            dim.button.set_label(&summarise_selection(
                dim.prefix,
                dim.chosen.borrow().as_ref(),
                &present,
            ));
        }
        self.clear.set_sensitive(active);
        if active {
            self.count.set_label(&hidden_count_text(shown, total));
            self.count
                .set_tooltip_text(Some(&describe_hidden(shown, total)));
        } else {
            self.count.set_label("");
            self.count.set_tooltip_text(None);
        }
    }

    /// `clear()`: back to showing everything, as one change rather than two (the text field is
    /// cleared without firing).
    pub fn clear_all(&self) {
        for which in 0..3u8 {
            *self.dim(which).chosen.borrow_mut() = None;
            self.sync_checks(which);
        }
        self.quiet.set(true);
        self.text.set_text("");
        self.quiet.set(false);
        self.emit();
    }
}
