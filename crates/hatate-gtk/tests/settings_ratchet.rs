//! The settings ratchet (DAN-2602, decided on DAN-2594).
//!
//! 0.2.0 shipped `lazy_thumbnails` with only its default path tested: the setting promised a bound and
//! nothing ever turned it off or checked the bound (DAN-2587). The parity map and the presubmit walk check
//! that every Python test has a Rust counterpart; neither checks that a setting's non-default branch runs.
//!
//! This test closes that for the settings that promise a limit, an exclusion or a switch: a key of
//! `Settings` (or of its four nested sections) whose name
//!
//! - starts `min_`, `max_`, `drop_`, `allow_`, `enable_`, `use_` or `lazy_`, or
//! - ends `_workers`, `_ttl_days`, `_bytes`, `_timeout` or `_thumbnails` (or is `timeout` itself), or
//! - has `_min_` or `_max_` inside it (`tracemoe_min_similarity`, `delay_min_seconds`)
//!
//! (plus the keys in [`INCLUDE`], which are switches of the same class that no name rule catches) needs at least one test that gives it a value other than its default, **or** a one-line waiver in
//! [`WAIVERS`] with a reason. "A test" is an `x.key = value;` or a `Section { key: value }` in test code
//! ([`hatate_doccite::assigns`]) whose value is not the key's default literal (a value that is not a
//! literal counts: the test chose it). Two kinds of site do not count, because they only prove the value
//! survives a save: `config_compat.rs`, and the Settings-dialog round trips in [`ROUND_TRIPS`].
//!
//! The scan finds an assignment, not an assertion: it cannot tell a pin from a fixture that sets
//! `delay_min_seconds = 0.0` to run fast. So a hit is the floor, not a proof; the verifier reads the
//! sites this test prints (`--nocapture`) and the revert-and-red of the PR that adds a key.
//!
//! Ratchet direction: a waived key that gains a test fails until its waiver is removed, so the waiver
//! list only shrinks. It needs no display, so it runs the same with and without `HATATE_GTK_SMOKE`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hatate_core::config::{
    HydrusSettings, MatchConditions, McpSettings, SauceNaoSettings, Settings,
};
use hatate_core::paths::Paths;
use hatate_doccite::Tree;
use hatate_doccite::assigns::{Assign, Rhs, key_assignments};

/// A key with no non-default test yet, and why. The key is `key` for a top-level setting and
/// `section.key` for a nested one. A key found here that has a test fails the ratchet: delete its line.
const WAIVERS: &[(&str, &str)] = &[
    // DAN-2595 q1 rows 3-7. `hash_workers = 99` in the smoke test is a Settings-dialog round trip.
    ("hash_workers", OWED),
    ("search_cache_ttl_days", OWED),
    ("match_conditions.min_width_gain_percent", OWED),
    ("match_conditions.min_height_gain_percent", OWED),
    ("mcp.audit_log_max_bytes", OWED),
    ("lens_search_whole_image", OWED),
    ("rank_matches_by_quality", OWED),
    // Found by this ratchet, not in q1 (which called it covered): the one boundary test in
    // `search_engine.rs` derives its tag count from the setting, so it passes at the default 5 with the
    // number hard-coded.
    (
        "match_conditions.min_tags_for_good",
        "Phase 7: non-default test owed (DAN-2594); found by the ratchet, its only test derives the boundary from the setting",
    ),
];

const OWED: &str = "Phase 7: non-default test owed (DAN-2594)";

/// The Settings-dialog round trips: they hand a value to the dialog and read it back, which proves the
/// value survives a save and says nothing about what the setting does. `(file, fn)`.
const ROUND_TRIPS: &[(&str, &str)] = &[
    (
        "crates/hatate-gtk/tests/smoke.rs",
        "settings_dialog_widgets_hand_back_the_values_they_were_given",
    ),
    (
        "crates/hatate-gtk/tests/smoke.rs",
        "edit_conditions_stores_and_saves_on_ok_and_nothing_on_cancel",
    ),
];

/// Files whose every assignment is a round trip of the whole file format.
const ROUND_TRIP_FILES: &[&str] = &["crates/hatate-core/tests/config_compat.rs"];

/// Switches the name rule does not catch: `lens_search_whole_image` turns the whole-image Lens search on or
/// off (DAN-2595 q1 row 7); `rank_matches_by_quality` switches the quality ordering of matches (DAN-2610).
/// A key is `key` or `section.key`.
const INCLUDE: &[&str] = &["lens_search_whole_image", "rank_matches_by_quality"];

/// Field names too common to count without the section: `x.timeout = ..` is not always Hydrus's.
const GENERIC_LEAVES: &[&str] = &["min_similarity", "timeout"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Whether the rule above names this field.
fn in_scope(leaf: &str) -> bool {
    if INCLUDE.contains(&leaf) {
        return true;
    }
    const START: &[&str] = &[
        "min_", "max_", "drop_", "allow_", "enable_", "use_", "lazy_",
    ];
    const END: &[&str] = &["_workers", "_ttl_days", "_bytes", "_timeout", "_thumbnails"];
    START.iter().any(|p| leaf.starts_with(p))
        || END.iter().any(|s| leaf.ends_with(s))
        || leaf == "timeout"
        || leaf.contains("_min_")
        || leaf.contains("_max_")
}

/// One key of the file format: `section` is `None` at top level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    section: Option<&'static str>,
    ty: &'static str,
    leaf: &'static str,
}

impl Key {
    fn path(&self) -> String {
        match self.section {
            Some(s) => format!("{s}.{}", self.leaf),
            None => self.leaf.to_string(),
        }
    }
}

/// Every in-scope key of `Settings` and its four sections.
fn scoped_keys() -> Vec<Key> {
    let mut out = Vec::new();
    let mut add =
        |section: Option<&'static str>, ty: &'static str, keys: &'static [&'static str]| {
            for leaf in keys {
                if in_scope(leaf)
                    && !(section.is_none()
                        && matches!(*leaf, "saucenao" | "hydrus" | "match_conditions" | "mcp"))
                {
                    out.push(Key { section, ty, leaf });
                }
            }
        };
    add(None, "Settings", Settings::KEYS);
    add(Some("saucenao"), "SauceNaoSettings", SauceNaoSettings::KEYS);
    add(Some("hydrus"), "HydrusSettings", HydrusSettings::KEYS);
    add(
        Some("match_conditions"),
        "MatchConditions",
        MatchConditions::KEYS,
    );
    add(Some("mcp"), "McpSettings", McpSettings::KEYS);
    out
}

/// The default of every key, as the JSON the app writes for a fresh config.
fn defaults() -> serde_json::Value {
    let paths =
        Paths::from_lookup(|k| (k == "HOME").then(|| "/nonexistent-ratchet-home".into())).unwrap();
    serde_json::from_str(&Settings::with_defaults(&paths).to_json_string()).unwrap()
}

fn default_of<'a>(d: &'a serde_json::Value, k: &Key) -> &'a serde_json::Value {
    match k.section {
        Some(s) => &d[s][k.leaf],
        None => &d[k.leaf],
    }
}

/// Whether the value on the right is something other than the key's default. A value that is not a
/// literal is the test's own choice, so it counts.
fn is_non_default(rhs: &Rhs, default: &serde_json::Value) -> bool {
    match rhs {
        Rhs::Bool(b) => default.as_bool() != Some(*b),
        Rhs::Num(n) => default.as_f64() != Some(*n),
        Rhs::Str(s) => default.as_str() != Some(s.as_str()),
        Rhs::Other => true,
    }
}

/// Whether an assignment found by the scan is an assignment of this key.
fn names_key(a: &Assign, k: &Key) -> bool {
    if a.key != k.leaf {
        return false;
    }
    let Some(section) = k.section else {
        return true;
    };
    match a.section.as_deref() {
        Some(s) => s == section || s == k.ty,
        None => !GENERIC_LEAVES.contains(&k.leaf),
    }
}

/// `(file, first line of test code)` for a file that holds test code: all of an integration test or a
/// `tests.rs`, or the part of a source file from its first `#[cfg(test)]`.
fn test_code_start(rel: &str, src: &str) -> Option<usize> {
    if rel.contains("/tests/") || rel.ends_with("/tests.rs") {
        return Some(1);
    }
    if rel.contains("/examples/") || rel.contains("/benches/") {
        return None;
    }
    src.lines()
        .position(|l| l.trim_start().starts_with("#[cfg(test)]"))
        .map(|i| i + 1)
}

/// One counted assignment: where, and what it is.
#[derive(Debug, Clone)]
struct Site {
    file: String,
    line: usize,
    in_fn: String,
    rhs: Rhs,
}

/// `key path -> sites that give it a non-default value in test code`, scanning the whole tree.
fn evidence(tree: &Tree) -> BTreeMap<String, Vec<Site>> {
    let d = defaults();
    let keys = scoped_keys();
    let leaves: Vec<&str> = keys.iter().map(|k| k.leaf).collect();
    let mut out: BTreeMap<String, Vec<Site>> = BTreeMap::new();
    for rel in tree.files() {
        if ROUND_TRIP_FILES.contains(&rel.as_str()) {
            continue;
        }
        let src = tree.read(rel);
        let Some(start) = test_code_start(rel, &src) else {
            continue;
        };
        let found = key_assignments(&src, &leaves).unwrap_or_else(|e| panic!("{rel} {e}"));
        for a in found.into_iter().filter(|a| a.line >= start) {
            let in_fn = a.in_fn.clone().unwrap_or_default();
            if ROUND_TRIPS.iter().any(|(f, n)| *f == rel && *n == in_fn) {
                continue;
            }
            for k in keys.iter().filter(|k| names_key(&a, k)) {
                if is_non_default(&a.rhs, default_of(&d, k)) {
                    out.entry(k.path()).or_default().push(Site {
                        file: rel.clone(),
                        line: a.line,
                        in_fn: in_fn.clone(),
                        rhs: a.rhs.clone(),
                    });
                }
            }
        }
    }
    out
}

/// What the ratchet finds wrong, one line each; empty is green.
fn problems(tree: &Tree, waivers: &[(&str, &str)]) -> Vec<String> {
    let keys = scoped_keys();
    let found = evidence(tree);
    let mut out = Vec::new();
    for (file, name) in ROUND_TRIPS {
        let ok = tree
            .parsed(file)
            .is_ok_and(|p| p.spans.iter().any(|s| s.kind == "fn" && s.name == *name));
        if !ok {
            out.push(format!(
                "ROUND_TRIPS names `{name}` in {file}, which no longer exists: update the entry"
            ));
        }
    }
    for k in &keys {
        let path = k.path();
        let waiver = waivers.iter().find(|(w, _)| *w == path);
        match (found.contains_key(&path), waiver) {
            (false, None) => out.push(format!(
                "{path}: no test gives it a non-default value. Add one that asserts what the setting changes, \
                 or a waiver in WAIVERS with the reason"
            )),
            (true, Some(_)) => {
                let s = &found[&path][0];
                out.push(format!(
                    "{path}: waived, but {}:{} (fn {}) now gives it a non-default value. Delete its waiver",
                    s.file, s.line, s.in_fn
                ));
            }
            _ => {}
        }
    }
    for (w, reason) in waivers {
        if !keys.iter().any(|k| k.path() == *w) {
            out.push(format!(
                "WAIVERS names `{w}`, which is not an in-scope setting"
            ));
        }
        if reason.trim().is_empty() {
            out.push(format!("WAIVERS `{w}` has no reason"));
        }
    }
    out
}

#[test]
fn every_bound_or_switch_setting_has_a_non_default_test_or_a_waiver() {
    let tree = Tree::new(&root());
    let found = evidence(&tree);
    eprintln!("settings ratchet: {} in-scope keys", scoped_keys().len());
    for k in scoped_keys() {
        let path = k.path();
        match found.get(&path) {
            Some(sites) => {
                let s = &sites[0];
                eprintln!(
                    "  {path:<42} {} site(s); first {}:{} fn {} = {:?}",
                    sites.len(),
                    s.file,
                    s.line,
                    s.in_fn,
                    s.rhs
                );
            }
            None => eprintln!("  {path:<42} no test; waiver in WAIVERS"),
        }
    }
    let p = problems(&tree, WAIVERS);
    assert!(
        p.is_empty(),
        "the settings ratchet found {} problem(s):\n  {}",
        p.len(),
        p.join("\n  ")
    );
}

#[test]
fn the_rule_names_the_keys_the_ticket_lists() {
    let paths: Vec<String> = scoped_keys().iter().map(Key::path).collect();
    for want in [
        "lazy_thumbnails",
        "hash_workers",
        "search_cache_ttl_days",
        "match_conditions.min_width_gain_percent",
        "match_conditions.min_height_gain_percent",
        "mcp.audit_log_max_bytes",
        "search_timeout",
        "hydrus.timeout",
        "drop_restricted_matches",
        "tracemoe_min_similarity",
        "lens_search_whole_image",
    ] {
        assert!(
            paths.iter().any(|p| p == want),
            "{want} is not in scope: {paths:?}"
        );
    }
    // not matched by the rule: no bound, exclusion or switch by name
    for not in ["mcp.port", "sidecar_overwrite", "tags_for_low_tag_count"] {
        assert!(
            !paths.iter().any(|p| p == not),
            "{not} unexpectedly in scope"
        );
    }
}

#[test]
fn a_literal_equal_to_the_default_is_no_evidence() {
    let d = serde_json::json!({"a": true, "n": 30.0, "s": "x"});
    assert!(!is_non_default(&Rhs::Bool(true), &d["a"]));
    assert!(is_non_default(&Rhs::Bool(false), &d["a"]));
    assert!(!is_non_default(&Rhs::Num(30.0), &d["n"]));
    assert!(is_non_default(&Rhs::Num(0.0), &d["n"]));
    assert!(!is_non_default(&Rhs::Str("x".into()), &d["s"]));
    assert!(is_non_default(&Rhs::Other, &d["a"]));
}
