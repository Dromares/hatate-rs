//! Keyboard bindings for the review pass, and the user's overrides (`core/shortcuts.py`).
//!
//! The registry of what can be bound, what it is bound to by default, and how a saved override is
//! merged over that. Key sequences are strings, which is what the toolkit's accelerator parser
//! takes and what `config.json` stores (`review_shortcuts`: an `id -> key` object), so none of
//! this needs a display.
//!
//! Two rules the window depends on. An override of `""` is not the same as no override: absent
//! means the default, empty means the user cleared the binding. And an id no longer in the
//! registry is dropped on [`resolve`], so a config written by another build cannot keep claiming a
//! key nothing fires.
//!
//! The ids are the ones `mcp::tools::REVIEW_ACTION_IDS` lists (a test holds the two together).

use crate::config::OrderedMap;
use crate::pycase;
use crate::pycompat;

/// `ReviewAction`: one bindable thing. `needs_single_row` marks the actions that only make sense
/// on exactly one image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewAction {
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub description: &'static str,
    pub needs_single_row: bool,
}

/// `REVIEW_ACTIONS`: ordered as they appear in the Shortcuts tab, grouped by what they do. Every
/// default is a bare key, which is only safe because the window binds them to the image table and
/// not to itself (`gui/review_shortcuts.py`).
pub const REVIEW_ACTIONS: [ReviewAction; 16] = [
    ReviewAction {
        id: "next_row",
        label: "Next image",
        default: "J",
        description: "Moves the selection down one row. The arrow keys still work too.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "prev_row",
        label: "Previous image",
        default: "K",
        description: "Moves the selection up one row.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "next_unreviewed",
        label: "Next image needing review",
        default: "N",
        description: "Skips ahead to the next row you haven't decided about - neither sent to Hydrus nor marked reviewed. This is the one that makes a long review pass resumable: you never have to find your place again by eye.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "prev_unreviewed",
        label: "Previous image needing review",
        default: "Shift+N",
        description: "The same search, backwards.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "next_candidate",
        label: "Next match candidate",
        default: "]",
        description: "Steps forward through the match dropdown, so alternatives can be read without reaching for the mouse.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "prev_candidate",
        label: "Previous match candidate",
        default: "[",
        description: "Steps back through the match dropdown.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "compare",
        label: "Compare with match",
        default: "C",
        description: "Opens the compare window on the selected image.",
        needs_single_row: true,
    },
    ReviewAction {
        id: "open_match",
        label: "Open match in browser",
        default: "O",
        description: "Opens the matched page in your browser.",
        needs_single_row: true,
    },
    ReviewAction {
        id: "show_in_hydrus",
        label: "Show in Hydrus",
        default: "H",
        description: "Opens a Hydrus page holding the selected files and switches Hydrus to it. Pairs with O: that opens the match in a browser, this opens your own copy in Hydrus. Needs the \"manage pages\" permission on your access key.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "send_upload",
        label: "Send file + URL + tags to Hydrus",
        default: "Return",
        description: "The main send: uploads your local file with the matched URL and the tag list attached. Bound to Return because on most images it is the whole decision.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "send_url",
        label: "Send URL to Hydrus's URL importer",
        default: "U",
        description: "Hands the matched URL to Hydrus's own downloader instead of uploading your local copy.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "download_send",
        label: "Download match + send to Hydrus",
        default: "D",
        description: "Downloads the full-resolution match and sends that, rather than your local file.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "research",
        label: "Re-search",
        default: "F5",
        description: "Searches the selected images again, ignoring the cached result.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "reset_result",
        label: "Reset result",
        default: "R",
        description: "Puts the selected images back to Not searched. Asks first, as the menu entry does.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "toggle_reviewed",
        label: "Mark / unmark as reviewed",
        default: "Space",
        description: "Records that you looked at this one and decided against sending it. Nothing is sent or changed on disk - it just takes the row out of \"needs review\", so N skips past it from now on. Press again to undo.",
        needs_single_row: false,
    },
    ReviewAction {
        id: "remove_row",
        label: "Remove from list",
        default: "Del",
        description: "Takes the selected images out of this list. The files, and Hydrus, are untouched.",
        needs_single_row: false,
    },
];

/// `ACTIONS_BY_ID[id]`.
pub fn action(id: &str) -> Option<&'static ReviewAction> {
    REVIEW_ACTIONS.iter().find(|a| a.id == id)
}

/// `default_bindings`: the out-of-the-box key for every action, in registry order.
pub fn default_bindings() -> OrderedMap<String> {
    OrderedMap(
        REVIEW_ACTIONS
            .iter()
            .map(|a| (a.id.to_owned(), a.default.to_owned()))
            .collect(),
    )
}

/// `resolve`: the effective binding for every action, the user's override where there is one and
/// the default otherwise. Overrides for ids this build does not have are dropped, values are
/// stripped, and the result has exactly one entry per registered action, in registry order.
///
/// (Python also skips an override whose value is not a string. A non-string cannot reach here:
/// `review_shortcuts` is an [`OrderedMap<String>`], and the config reader keeps the default for a
/// whole object that holds one.)
pub fn resolve(overrides: Option<&OrderedMap<String>>) -> OrderedMap<String> {
    let mut bindings = default_bindings();
    for (id, key) in overrides.map_or(&[][..], |o| &o.0[..]) {
        if action(id).is_none() {
            continue;
        }
        if let Some((_, slot)) = bindings.0.iter_mut().find(|(k, _)| k == id) {
            *slot = pycompat::strip(key).to_owned();
        }
    }
    bindings
}

/// `prune`: what is worth writing to the config, only the bindings that differ from the default.
/// Saving all sixteen would freeze today's defaults into every config.
pub fn prune(bindings: &OrderedMap<String>) -> OrderedMap<String> {
    OrderedMap(
        bindings
            .0
            .iter()
            .filter(|(id, key)| action(id).is_some_and(|a| key != a.default))
            .cloned()
            .collect(),
    )
}

/// `normalize`: a key in the form used for comparing two bindings (surrounding space and case
/// dropped, since `j` and `J` are one key).
pub fn normalize(key: &str) -> String {
    pycase::casefold(pycompat::strip(key))
}

/// `conflicts`: keys bound to more than one action, as `(key, the action ids on it)` in the order
/// each key was first seen. Unbound (`""`) is never a conflict. Worth blocking rather than
/// warning about: an ambiguous shortcut fires neither action.
pub fn conflicts(bindings: &OrderedMap<String>) -> Vec<(String, Vec<String>)> {
    let mut seen: Vec<(String, Vec<String>)> = Vec::new();
    for (id, key) in &bindings.0 {
        let canonical = normalize(key);
        if canonical.is_empty() {
            continue;
        }
        match seen.iter_mut().find(|(k, _)| *k == canonical) {
            Some((_, ids)) => ids.push(id.clone()),
            None => seen.push((canonical, vec![id.clone()])),
        }
    }
    seen.into_iter().filter(|(_, ids)| ids.len() > 1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::REVIEW_ACTION_IDS;

    fn map(pairs: &[(&str, &str)]) -> OrderedMap<String> {
        OrderedMap(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        )
    }

    fn test_paths(home: &std::path::Path) -> crate::paths::Paths {
        let home = home.as_os_str().to_owned();
        crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap()
    }

    #[test]
    fn the_registry_is_the_one_the_tool_surface_checks_against() {
        let ids: Vec<&str> = REVIEW_ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(ids, REVIEW_ACTION_IDS.to_vec());
        assert_eq!(default_bindings().0.len(), 16);
        assert_eq!(action("compare").map(|a| a.default), Some("C"));
        assert!(action("compare").unwrap().needs_single_row);
        assert!(!action("next_row").unwrap().needs_single_row);
        assert_eq!(action("nope"), None);
    }

    #[test]
    fn an_empty_override_clears_the_binding_and_is_not_the_default() {
        let resolved = resolve(Some(&map(&[("next_row", "")])));
        assert_eq!(resolved.get("next_row").map(String::as_str), Some(""));
        assert_eq!(resolved.get("prev_row").map(String::as_str), Some("K"));
    }

    #[test]
    fn resolve_drops_unknown_ids_strips_values_and_keeps_registry_order() {
        let resolved = resolve(Some(&map(&[
            ("zzz", "X"),
            ("remove_row", "  Ctrl+Del \t"),
            ("next_row", "Down"),
        ])));
        assert_eq!(resolved.0.len(), 16);
        assert_eq!(resolved.get("zzz"), None);
        assert_eq!(
            resolved.get("remove_row").map(String::as_str),
            Some("Ctrl+Del")
        );
        assert_eq!(resolved.0[0], ("next_row".to_owned(), "Down".to_owned()));
        assert_eq!(resolve(None), default_bindings());
    }

    #[test]
    fn prune_keeps_only_what_differs_from_the_default() {
        let bound = resolve(Some(&map(&[
            ("next_row", "Down"),
            ("prev_row", "K"),
            ("compare", ""),
        ])));
        assert_eq!(prune(&bound), map(&[("next_row", "Down"), ("compare", "")]));
        assert_eq!(prune(&default_bindings()), OrderedMap::default());
    }

    #[test]
    fn conflicts_compare_case_and_space_insensitively_and_ignore_unbound() {
        let bound = map(&[
            ("a", "J"),
            ("b", " j "),
            ("c", ""),
            ("d", ""),
            ("e", "x"),
            ("f", "X"),
            ("g", "y"),
        ]);
        assert_eq!(
            conflicts(&bound),
            vec![
                ("j".to_owned(), vec!["a".to_owned(), "b".to_owned()]),
                ("x".to_owned(), vec!["e".to_owned(), "f".to_owned()]),
            ]
        );
        assert_eq!(normalize("  Shift+N "), "shift+n");
        assert_eq!(normalize("\u{df}"), "ss", "casefold, not lowercase");
        assert!(conflicts(&default_bindings()).is_empty());
    }

    #[test]
    fn an_override_round_trips_through_config_json() {
        let home = tempfile::tempdir().unwrap();
        let paths = test_paths(home.path());
        let mut settings = crate::config::Settings::with_defaults(&paths);
        settings.review_shortcuts = prune(&resolve(Some(&map(&[
            ("send_upload", "Ctrl+Return"),
            ("compare", ""),
        ]))));
        settings.save(&paths).expect("saves");
        let loaded = crate::config::Settings::load(&paths).settings;
        assert_eq!(loaded.review_shortcuts.len(), 2);
        assert_eq!(
            loaded.review_shortcuts.get("compare").map(String::as_str),
            Some("")
        );
        let bindings = resolve(Some(&loaded.review_shortcuts));
        assert_eq!(
            bindings.get("send_upload").map(String::as_str),
            Some("Ctrl+Return")
        );
        assert_eq!(bindings.get("compare").map(String::as_str), Some(""));
    }
}
