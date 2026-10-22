//! One-time migrations for `schema_version` 1-12 (spec 2.3, `core/config.py:572-691`).

use serde_json::{Map, Value};

use super::model::Settings;

const OTHER_SITE: &str = "Other";

/// Python truthiness of a JSON value, for the two legacy booleans.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn append_missing(list: &mut Vec<String>, names: &[&str]) {
    for name in names {
        if !list.iter().any(|s| s == name) {
            list.push((*name).to_owned());
        }
    }
}

/// Apply every migration above `stored_version` to `s`, which already holds the values
/// read from `raw`. The caller sets `schema_version` and saves afterwards.
pub(crate) fn run(s: &mut Settings, raw: &Map<String, Value>, stored_version: i64) {
    if stored_version < 2 {
        append_missing(&mut s.enabled_tag_sources, &["Hydrus"]);
    }
    if stored_version < 3 {
        append_missing(&mut s.enabled_sites, &["e621", "Safebooru"]);
    }
    if stored_version < 4 {
        // `use_saucenao_fallback`: read, never written back (spec 2.3).
        if let Some(old) = raw.get("use_saucenao_fallback").filter(|v| !v.is_null()) {
            s.secondary_engine_mode = if truthy(old) { "always" } else { "disabled" }.to_owned();
        }
    }
    if stored_version < 5 {
        // `use_filename_hashes`: same shape.
        if let Some(old) = raw.get("use_filename_hashes").filter(|v| !v.is_null()) {
            s.hash_source = if truthy(old) { "hydrus" } else { "local" }.to_owned();
        }
    }
    if stored_version < 6 {
        append_missing(
            &mut s.enabled_sites,
            &["Rule34", "Xbooru", "Twitter", "AniList"],
        );
    }
    if stored_version < 7 {
        append_missing(&mut s.enabled_sites, &["Paheal"]);
    }
    if stored_version < 8 {
        append_missing(&mut s.enabled_sites, &["MangaUpdates"]);
    }
    if stored_version < 9 {
        append_missing(&mut s.enabled_sites, &["MyAnimeList"]);
    }
    // 10-12: the site was "Other" until now, so it starts wherever "Other" was.
    let other_enabled = |s: &Settings| s.enabled_sites.iter().any(|x| x == OTHER_SITE);
    if stored_version < 10 && other_enabled(s) {
        append_missing(&mut s.enabled_sites, &["Rule34.us"]);
    }
    if stored_version < 11 && other_enabled(s) {
        append_missing(&mut s.enabled_sites, &["Reddit"]);
    }
    if stored_version < 12 && other_enabled(s) {
        append_missing(&mut s.enabled_sites, &["Toon34"]);
    }
    if stored_version < 13 {
        s.filter_sent_tags_by_source = false;
    }
}
