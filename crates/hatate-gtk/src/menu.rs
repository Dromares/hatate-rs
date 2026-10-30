//! The menubar: every menu Python has (`MainWindow._build_menu`, `_build_hydrus_menu` and
//! `_build_sites_menu`, `gui/main_window.py:571,726,756` at `76c85c4`).
//!
//! The menus are data ([`menus`]) so one table builds the `gio::Menu`, registers the actions and
//! is read by the tests that pin the structure against Python's. An item whose action a later
//! ticket owns is registered as a **disabled** action; the `owner` column names that ticket, and
//! every one of them is also marked `DEFERRED(6.x)` in a comment below so 6.j can find any that
//! are left (`grep -n 'DEFERRED(' crates/hatate-gtk/src/menu.rs`).

use gtk4::gio;
use gtk4::prelude::*;

/// Who implements the action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// This ticket (6.c): the action works.
    Shell,
    /// A later ticket: the action exists and is disabled until that ticket wires it.
    Later(&'static str),
}

/// A menu entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A plain item: label, action name (`win.<name>`), accelerator, owner.
    Item {
        label: &'static str,
        action: &'static str,
        accel: Option<&'static str>,
        owner: Owner,
    },
    /// A checkable item bound to a stateful action `win.<action>` taking a string target.
    Check {
        label: String,
        action: &'static str,
        target: String,
        owner: Owner,
    },
    Separator,
    /// A submenu of radio items (Settings > Theme).
    Radio {
        title: &'static str,
        action: &'static str,
        choices: Vec<(&'static str, &'static str)>,
        owner: Owner,
    },
}

/// One top-level menu. `title` uses `_` for the mnemonic (Qt's `&`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

fn item(label: &'static str, action: &'static str, owner: Owner) -> Entry {
    Entry::Item {
        label,
        action,
        accel: None,
        owner,
    }
}

fn keyed(label: &'static str, action: &'static str, accel: &'static str, owner: Owner) -> Entry {
    Entry::Item {
        label,
        action,
        accel: Some(accel),
        owner,
    }
}

/// `ALL_SITE_OPTIONS` (`core/sites.py:126`): every site, sorted case-insensitively, then `Other`.
/// It is the default of `enabled_sites`.
pub fn all_site_options() -> Vec<String> {
    // The default of `enabled_sites` is every site; the paths only fill `log_file_path`.
    let paths = hatate_core::paths::Paths::from_lookup(|_| Some("/".into()))
        .expect("a lookup that always answers");
    hatate_core::config::Settings::with_defaults(&paths).enabled_sites
}

/// The menubar, in Python's order: Files, Hydrus, Sites, Settings, Help.
pub fn menus() -> Vec<Menu> {
    use Owner::Shell;
    // Preferences opens the Settings window (6.h): its entry below is Owner::Shell.
    let files = Menu {
        title: "_Files",
        entries: vec![
            item("Add Files\u{2026}", "add-files", Shell),
            item("Add Folder\u{2026}", "add-folder", Shell),
            item("Query Hydrus\u{2026}", "query-hydrus", Shell),
            item("Re-check Queued Imports", "reconcile", Shell),
            item("Pawchive Index\u{2026}", "pawchive-index", Shell),
            Entry::Separator,
            keyed(
                "Save Session As\u{2026}",
                "save-session-as",
                "<Ctrl>s",
                Shell,
            ),
            keyed("Open Session\u{2026}", "open-session", "<Ctrl>o", Shell),
            Entry::Separator,
            item("Export Results\u{2026}", "export-results", Shell),
            item("Write Tag Files\u{2026}", "write-tag-files", Shell),
            Entry::Separator,
            item("Open Matched URLs Log", "open-log", Shell),
            Entry::Separator,
            item("Exit", "quit", Shell),
        ],
    };
    let hydrus = Menu {
        title: "_Hydrus",
        entries: vec![
            item(
                "Send File + URL + Tags to Hydrus (upload)",
                "send-to-hydrus",
                Shell,
            ),
            item(
                "Send URL to Hydrus's URL Importer\u{2026}",
                "import-url-to-hydrus",
                Shell,
            ),
            Entry::Separator,
            // The same action the Files menu holds: one action, two homes.
            item("Re-check Queued Imports", "reconcile", Shell),
        ],
    };
    let mut sites_entries = vec![
        item("Select All", "sites-select-all", Shell),
        item("Select None", "sites-select-none", Shell),
        Entry::Separator,
    ];
    for site in all_site_options() {
        sites_entries.push(Entry::Check {
            label: site.clone(),
            action: "site",
            target: site,
            owner: Shell,
        });
    }
    let sites = Menu {
        title: "Si_tes",
        entries: sites_entries,
    };
    let settings = Menu {
        title: "_Settings",
        entries: vec![
            item("Preferences\u{2026}", "preferences", Shell),
            item("Edit Match Conditions\u{2026}", "edit-conditions", Shell),
            Entry::Separator,
            Entry::Radio {
                title: "Theme",
                action: "theme",
                choices: vec![
                    ("system", "Follow the desktop"),
                    ("dark", "Dark"),
                    ("light", "Light"),
                ],
                owner: Shell,
            },
        ],
    };
    let help = Menu {
        title: "_Help",
        entries: vec![
            item("View Logs\u{2026}", "view-logs", Shell),
            item("Parser Health\u{2026}", "parser-health", Shell),
            item("Clear Search Cache\u{2026}", "clear-search-cache", Shell),
            item(
                "Clear List and Saved Session\u{2026}",
                "clear-session",
                Shell,
            ),
        ],
    };
    vec![files, hydrus, sites, settings, help]
}

/// Every `(action, owner)` pair in the menus, once each, in first-appearance order.
pub fn actions() -> Vec<(&'static str, Owner)> {
    let mut seen: Vec<(&'static str, Owner)> = Vec::new();
    for menu in menus() {
        for e in &menu.entries {
            let (name, owner) = match e {
                Entry::Item { action, owner, .. }
                | Entry::Check { action, owner, .. }
                | Entry::Radio { action, owner, .. } => (*action, *owner),
                Entry::Separator => continue,
            };
            if !seen.iter().any(|(n, _)| *n == name) {
                seen.push((name, owner));
            }
        }
    }
    seen
}

/// The actions a later ticket owns, with the ticket: what 6.j must find gone.
pub fn deferred() -> Vec<(&'static str, &'static str)> {
    actions()
        .into_iter()
        .filter_map(|(n, o)| match o {
            Owner::Later(t) => Some((n, t)),
            Owner::Shell => None,
        })
        .collect()
}

/// The accelerators: `(action with target, accelerator)`.
pub fn accelerators() -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    for menu in menus() {
        for e in menu.entries {
            if let Entry::Item {
                action,
                accel: Some(a),
                ..
            } = e
            {
                out.push((format!("win.{action}"), a));
            }
        }
    }
    out
}

/// The boolean action behind the `index`-th site's check item.
pub fn site_action(index: usize) -> String {
    format!("site-{index}")
}

/// Where the Clear Search Cache item sits, so its label (which carries the cache size) can be
/// replaced when the Help menu is about to be shown (`_update_clear_cache_action_label`).
pub struct ClearCacheItem {
    section: gio::Menu,
    position: i32,
}

impl ClearCacheItem {
    /// The label the menu shows now.
    pub fn label(&self) -> String {
        self.section
            .item_attribute_value(self.position, "label", None)
            .and_then(|v| v.str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn set_label(&self, label: &str) {
        self.section.remove(self.position);
        let item = gio::MenuItem::new(Some(label), Some("win.clear-search-cache"));
        self.section.insert_item(self.position, &item);
    }
}

/// The `gio::Menu` for the menubar, and the handle to the item whose label changes.
pub fn model() -> (gio::Menu, ClearCacheItem) {
    let mut clear_cache = None;
    let bar = gio::Menu::new();
    let mut site_index = 0usize;
    for menu in menus() {
        let m = gio::Menu::new();
        let mut section = gio::Menu::new();
        for e in &menu.entries {
            match e {
                Entry::Item { label, action, .. } => {
                    if *action == "clear-search-cache" {
                        clear_cache = Some(ClearCacheItem {
                            section: section.clone(),
                            position: section.n_items(),
                        });
                    }
                    section.append(Some(label), Some(&format!("win.{action}")));
                }
                Entry::Check { label, .. } => {
                    // One boolean action per site: a check item, not a radio on one string.
                    section.append(
                        Some(label),
                        Some(&format!("win.{}", site_action(site_index))),
                    );
                    site_index += 1;
                }
                Entry::Radio {
                    title,
                    action,
                    choices,
                    ..
                } => {
                    let sub = gio::Menu::new();
                    for (value, label) in choices {
                        let it = gio::MenuItem::new(Some(label), None);
                        it.set_action_and_target_value(
                            Some(&format!("win.{action}")),
                            Some(&gtk4::glib::Variant::from(*value)),
                        );
                        sub.append_item(&it);
                    }
                    section.append_submenu(Some(title), &sub);
                }
                Entry::Separator => {
                    m.append_section(None, &section);
                    section = gio::Menu::new();
                }
            }
        }
        m.append_section(None, &section);
        bar.append_submenu(Some(menu.title), &m);
    }
    (
        bar,
        clear_cache.expect("the Help menu has Clear Search Cache"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(menu: &Menu) -> Vec<String> {
        menu.entries
            .iter()
            .map(|e| match e {
                Entry::Item { label, .. } => (*label).to_owned(),
                Entry::Check { label, .. } => format!("[x] {label}"),
                Entry::Radio { title, .. } => format!("{title} >"),
                Entry::Separator => "-".to_owned(),
            })
            .collect()
    }

    /// `_build_menu` at `76c85c4`: the Files menu, entry for entry.
    #[test]
    fn the_files_menu_is_pythons() {
        let m = &menus()[0];
        assert_eq!(m.title, "_Files");
        assert_eq!(
            labels(m),
            [
                "Add Files\u{2026}",
                "Add Folder\u{2026}",
                "Query Hydrus\u{2026}",
                "Re-check Queued Imports",
                "Pawchive Index\u{2026}",
                "-",
                "Save Session As\u{2026}",
                "Open Session\u{2026}",
                "-",
                "Export Results\u{2026}",
                "Write Tag Files\u{2026}",
                "-",
                "Open Matched URLs Log",
                "-",
                "Exit",
            ]
        );
    }

    #[test]
    fn the_other_menus_are_pythons() {
        let all = menus();
        let titles: Vec<_> = all.iter().map(|m| m.title).collect();
        assert_eq!(
            titles,
            ["_Files", "_Hydrus", "Si_tes", "_Settings", "_Help"]
        );
        assert_eq!(
            labels(&all[1]),
            [
                "Send File + URL + Tags to Hydrus (upload)",
                "Send URL to Hydrus's URL Importer\u{2026}",
                "-",
                "Re-check Queued Imports",
            ]
        );
        assert_eq!(
            labels(&all[3]),
            [
                "Preferences\u{2026}",
                "Edit Match Conditions\u{2026}",
                "-",
                "Theme >"
            ]
        );
        assert_eq!(
            labels(&all[4]),
            [
                "View Logs\u{2026}",
                "Parser Health\u{2026}",
                "Clear Search Cache\u{2026}",
                "Clear List and Saved Session\u{2026}",
            ]
        );
        let sites = labels(&all[2]);
        assert_eq!(&sites[..3], ["Select All", "Select None", "-"]);
        assert_eq!(sites.len(), 3 + all_site_options().len());
        assert_eq!(sites.last().unwrap(), "[x] Other");
    }

    #[test]
    fn the_theme_choices_are_pythons_labels() {
        let Entry::Radio { choices, .. } = &menus()[3].entries[3] else {
            panic!("no theme submenu");
        };
        assert_eq!(
            choices,
            &[
                ("system", "Follow the desktop"),
                ("dark", "Dark"),
                ("light", "Light")
            ]
        );
    }

    #[test]
    fn only_two_items_carry_an_accelerator() {
        assert_eq!(
            accelerators(),
            [
                ("win.save-session-as".to_owned(), "<Ctrl>s"),
                ("win.open-session".to_owned(), "<Ctrl>o"),
            ]
        );
    }

    /// `test_both_menus_share_one_recheck_action`: the Files and Hydrus entries name the one
    /// action, so one handler and one enabled state serve both.
    #[test]
    fn the_files_and_hydrus_menus_hold_the_one_recheck_action() {
        let action_of = |menu: &Menu| {
            menu.entries.iter().find_map(|e| match e {
                Entry::Item { label, action, .. } if *label == "Re-check Queued Imports" => {
                    Some(*action)
                }
                _ => None,
            })
        };
        let all = menus();
        assert_eq!(action_of(&all[0]), Some("reconcile"));
        assert_eq!(action_of(&all[1]), Some("reconcile"));
    }

    #[test]
    fn the_same_action_in_two_menus_is_registered_once() {
        let names: Vec<_> = actions().iter().map(|(n, _)| *n).collect();
        assert_eq!(names.iter().filter(|n| **n == "reconcile").count(), 1);
    }

    #[test]
    fn every_deferred_action_names_a_ticket() {
        let d = deferred();
        assert!(d.iter().all(|(_, t)| t.starts_with("6.")));
        // 6.g wired Query Hydrus, Pawchive Index, Re-check Queued Imports and the two Send items;
        // 6.h wired Preferences.
        for working in [
            "preferences",
            "query-hydrus",
            "pawchive-index",
            "reconcile",
            "send-to-hydrus",
            "import-url-to-hydrus",
            "add-files",
            "add-folder",
            "save-session-as",
            "open-session",
            "open-log",
            "quit",
            "view-logs",
            "clear-session",
            "theme",
            "sites-select-all",
            "sites-select-none",
            "site",
            "parser-health",
            "toggle-search",
            "export-results",
            "write-tag-files",
            "clear-search-cache",
            "edit-conditions",
        ] {
            assert!(!d.iter().any(|(n, _)| *n == working), "{working} is wired");
        }
    }
}
