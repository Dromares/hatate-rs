//! What the Settings dialog's controls hold and what Save writes: `SettingsDialog.__init__`'s
//! `setValue`/`setChecked`/`setText` half ([`initial`]) and `apply_to_settings` ([`apply`]) of
//! `gui/settings_dialog.py` at `76c85c4`, with no widget in between.
//!
//! The controls are named by the `id`s of `settings_spec.rs`. A spin box's value is what a Qt
//! `QDoubleSpinBox` would hold after `setValue` ([`qt_double`]: rounded to its decimals, then
//! bounded), so the pure round trip `apply(initial(s))` is the same function as Python's
//! `SettingsDialog(s).apply_to_settings()` on an untouched dialog. The widgets must hand back
//! exactly these values, which the dialog's tests check.
//!
//! Not here (6.i): the Shortcuts and MCP pages. Their keys are left as found.

use crate::add_tags::splitlines;
use crate::settings_node::{Node, find};
use crate::settings_spec::{ENGINES, PAGES};
use hatate_core::config::{OrderedMap, Secret, Settings};
use hatate_core::pycompat::strip;
use hatate_core::sidecar;
use std::collections::BTreeMap;

/// What one control holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Num(f64),
    /// A line edit's text, an editable combo's text, a plain-text edit's text.
    Text(String),
    /// A combo's `currentData()`.
    Choice(String),
    /// The texts of the checked items of a check list, in list order.
    Checked(Vec<String>),
}

/// Every control's value, by id.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Values(pub BTreeMap<String, Value>);

impl Values {
    pub fn set(&mut self, id: &str, v: Value) {
        self.0.insert(id.to_owned(), v);
    }
    pub fn b(&self, id: &str) -> bool {
        matches!(self.0.get(id), Some(Value::Bool(true)))
    }
    pub fn n(&self, id: &str) -> f64 {
        match self.0.get(id) {
            Some(Value::Num(n)) => *n,
            _ => 0.0,
        }
    }
    pub fn i(&self, id: &str) -> i64 {
        self.n(id) as i64
    }
    pub fn t(&self, id: &str) -> &str {
        match self.0.get(id) {
            Some(Value::Text(s) | Value::Choice(s)) => s,
            _ => "",
        }
    }
    fn checked(&self, id: &str) -> Vec<String> {
        match self.0.get(id) {
            Some(Value::Checked(v)) => v.clone(),
            _ => Vec::new(),
        }
    }
}

/// `QDoubleSpinBox.setValue`: round to `dec` decimals, then bound to `[min, max]`.
pub fn qt_double(v: f64, min: f64, max: f64, dec: u32) -> f64 {
    let rounded = format!("{:.*}", dec as usize, v)
        .parse::<f64>()
        .unwrap_or(v);
    rounded.clamp(min, max)
}

/// What a `QPlainTextEdit` holds after `setPlainText(text)`, read back by `toPlainText()`: every
/// line ending (`\r\n`, `\r`, and the Unicode line and paragraph separators) is one `\n`.
pub fn qt_plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{2028}' | '\u{2029}' => out.push('\n'),
            c => out.push(c),
        }
    }
    out
}

/// `QSpinBox.setValue`: bound to `[min, max]`.
pub fn qt_int(v: i64, min: i64, max: i64) -> i64 {
    v.clamp(min, max)
}

/// `findData(value)` then `idx if idx >= 0 else 0`: a stored value the combo does not offer
/// shows (and saves) as its first item.
pub fn combo_choice(items: &[(&str, &str)], stored: &str) -> String {
    items
        .iter()
        .find(|(_, d)| *d == stored)
        .or(items.first())
        .map(|(_, d)| (*d).to_owned())
        .unwrap_or_default()
}

fn nonblank_lines(text: &str) -> Vec<String> {
    splitlines(text)
        .into_iter()
        .map(strip)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `dict[key] = value`: replaces in place, appends when new.
fn put<V>(map: &mut Vec<(String, V)>, key: String, value: V) {
    match map.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => map.push((key, value)),
    }
}

/// `__init__`: the value each control starts at for `s`.
pub fn initial(s: &Settings) -> Values {
    let engines = ENGINES;
    let mut v = Values::default();
    let dbl = |x: f64, lo: f64, hi: f64| Value::Num(qt_double(x, lo, hi, 2));
    let choice = |id: &str, stored: &str| match find(PAGES, id) {
        Some(Node::Combo { items, .. }) => Value::Choice(combo_choice(items, stored)),
        other => panic!("{id} is not a combo in the spec: {other:?}"),
    };
    v.set("delay_min", dbl(s.delay_min_seconds, 5.0, 600.0));
    v.set("delay_max", dbl(s.delay_max_seconds, 5.0, 900.0));
    for e in engines {
        let (low, high) = match s.engine_delays.get(e) {
            Some(pair) if pair.len() >= 2 => (pair[0], pair[1]),
            _ => (0.0, 0.0),
        };
        v.set(&format!("engine_delay_min.{e}"), dbl(low, 0.0, 600.0));
        v.set(&format!("engine_delay_max.{e}"), dbl(high, 0.0, 900.0));
    }
    v.set("search_timeout", dbl(s.search_timeout, 5.0, 300.0));
    v.set("lazy_thumbnails", Value::Bool(s.lazy_thumbnails));
    v.set(
        "hash_workers",
        Value::Num(qt_int(s.hash_workers, 1, 16) as f64),
    );
    v.set("restore_session", Value::Bool(s.restore_session_on_start));
    v.set("autosave_session", Value::Bool(s.autosave_session));
    v.set(
        "autosave_interval",
        Value::Num(qt_int(s.autosave_interval_seconds, 10, 3600) as f64),
    );
    v.set("log_matched", Value::Bool(s.log_matched_urls));
    v.set("log_path", Value::Text(s.log_file_path.clone()));
    v.set(
        "filter_sent_tags",
        Value::Bool(s.filter_sent_tags_by_source),
    );
    v.set(
        "tag_sources_list",
        Value::Checked(checked_of("tag_sources_list", &s.enabled_tag_sources)),
    );
    v.set(
        "saucenao_tag_types",
        Value::Checked(checked_of("saucenao_tag_types", &s.saucenao.tag_types)),
    );
    v.set(
        "extras_only_as_fallback",
        Value::Bool(s.extras_only_as_fallback),
    );
    v.set("enable_ascii2d", Value::Bool(s.enable_ascii2d));
    v.set("enable_tracemoe", Value::Bool(s.enable_tracemoe));
    v.set("enable_iqdb3d", Value::Bool(s.enable_iqdb3d));
    v.set("enable_google_images", Value::Bool(s.enable_google_images));
    v.set("enable_google_lens", Value::Bool(s.enable_google_lens));
    v.set(
        "lens_search_whole_image",
        Value::Bool(s.lens_search_whole_image),
    );
    v.set("enable_yandex", Value::Bool(s.enable_yandex));
    v.set("enable_pawchive", Value::Bool(s.enable_pawchive));
    v.set(
        "enable_pawchive_index",
        Value::Bool(s.enable_pawchive_index),
    );
    v.set(
        "google_images_key",
        Value::Text(s.google_images_api_key.expose().to_owned()),
    );
    v.set(
        "tracemoe_min_sim",
        dbl(s.tracemoe_min_similarity, 50.0, 100.0),
    );
    v.set(
        "fallback_below_similarity",
        dbl(s.fallback_below_similarity, 0.0, 100.0),
    );
    v.set("use_search_cache", Value::Bool(s.use_search_cache));
    v.set(
        "search_cache_ttl",
        dbl(s.search_cache_ttl_days, 0.0, 3650.0),
    );
    v.set(
        "retrieve_booru_tags",
        Value::Bool(s.retrieve_tags_from_booru),
    );
    v.set("rank_by_quality", Value::Bool(s.rank_matches_by_quality));
    v.set("drop_dead_matches", Value::Bool(s.drop_dead_matches));
    v.set(
        "drop_restricted_matches",
        Value::Bool(s.drop_restricted_matches),
    );
    v.set("borrow_tags", Value::Bool(s.borrow_tags_from_other_matches));
    v.set(
        "borrow_tags_slack",
        dbl(s.borrow_tags_similarity_slack, 0.0, 25.0),
    );
    v.set(
        "retry_failed_searches",
        Value::Bool(s.retry_failed_searches),
    );
    v.set("remove_after_import", Value::Bool(s.remove_after_import));
    v.set("auto_import_enabled", Value::Bool(s.auto_import_enabled));
    v.set(
        "auto_import_min_similarity",
        dbl(s.auto_import_min_similarity, 0.0, 100.0),
    );
    v.set(
        "url_import_confirm_timeout",
        dbl(s.url_import_confirm_timeout, 5.0, 600.0),
    );
    v.set(
        "url_import_confirm_interval",
        dbl(s.url_import_confirm_interval, 0.5, 30.0),
    );
    v.set(
        "saucenao_key",
        Value::Text(s.saucenao.api_key.expose().to_owned()),
    );
    v.set("saucenao_json", Value::Bool(s.saucenao.use_json_api));
    v.set(
        "saucenao_pause_on_quota",
        Value::Bool(s.saucenao.pause_search_on_quota_exhausted),
    );
    v.set(
        "continue_without_saucenao",
        Value::Bool(s.continue_without_saucenao_on_quota),
    );
    v.set(
        "saucenao_min_sim",
        dbl(s.saucenao.min_similarity, 0.0, 100.0),
    );
    v.set(
        "saucenao_namespace",
        Value::Text(s.saucenao.namespace.clone()),
    );
    v.set("hydrus_url", Value::Text(s.hydrus.api_url.clone()));
    v.set(
        "hydrus_key",
        Value::Text(s.hydrus.access_key.expose().to_owned()),
    );
    v.set(
        "hydrus_service_key",
        Value::Text(s.hydrus.tag_service_key.clone()),
    );
    v.set("hydrus_verify_ssl", Value::Bool(s.hydrus.verify_ssl));
    // the MCP page (6.i): `mcp_port` and the audit-log size are spin boxes, which hold what Qt's
    // `setValue` would (a stored port under 1024 shows as 1024 and saves as 1024)
    v.set("mcp_enabled", Value::Bool(s.mcp.enabled));
    v.set(
        "mcp_port",
        Value::Num(qt_int(s.mcp.port, 1024, 65535) as f64),
    );
    v.set("mcp_token", Value::Text(s.mcp.token.expose().to_owned()));
    v.set("mcp_dry_run", Value::Bool(s.mcp.dry_run));
    v.set("mcp_allow_research", Value::Bool(s.mcp.allow_research));
    v.set(
        "mcp_allow_hydrus_writes",
        Value::Bool(s.mcp.allow_hydrus_writes),
    );
    v.set(
        "mcp_allow_destructive",
        Value::Bool(s.mcp.allow_destructive),
    );
    v.set(
        "mcp_audit_max_mb",
        Value::Num(qt_int(
            s.mcp.audit_log_max_bytes.div_euclid(1_000_000).max(1),
            1,
            500,
        ) as f64),
    );
    v.set(
        "set_hydrus_duplicate_relationships",
        Value::Bool(s.set_hydrus_duplicate_relationships),
    );
    v.set(
        "write_hydrus_provenance_note",
        Value::Bool(s.write_hydrus_provenance_note),
    );
    v.set(
        "hydrus_provenance_note_name",
        Value::Text(s.hydrus_provenance_note_name.clone()),
    );
    v.set(
        "enable_namespace_remap",
        Value::Bool(s.enable_tag_namespace_remap),
    );
    v.set(
        "namespace_remap_edit",
        Value::Text(pairs_text(&s.tag_namespace_remap)),
    );
    v.set("enable_tag_blacklist", Value::Bool(s.enable_tag_blacklist));
    v.set(
        "tag_blacklist_edit",
        Value::Text(s.tag_blacklist.join("\n")),
    );
    v.set("add_rating_tag", Value::Bool(s.add_rating_tag));
    v.set(
        "rating_tag_namespace",
        Value::Text(s.rating_tag_namespace.clone()),
    );
    v.set("enable_tag_colors", Value::Bool(s.enable_hydrus_tag_colors));
    v.set(
        "namespace_colors_edit",
        Value::Text(pairs_text(&s.tag_namespace_colors)),
    );
    v.set(
        "tags_for_found_edit",
        Value::Text(s.tags_for_found.join("\n")),
    );
    v.set(
        "tags_for_not_found_edit",
        Value::Text(s.tags_for_not_found.join("\n")),
    );
    v.set(
        "tags_for_low_tag_count_edit",
        Value::Text(s.tags_for_low_tag_count.join("\n")),
    );
    v.set(
        "pixiv_session_cookie",
        Value::Text(s.pixiv_session_cookie.expose().to_owned()),
    );
    v.set(
        "e621_username",
        Value::Text(s.e621_username.expose().to_owned()),
    );
    v.set(
        "e621_api_key",
        Value::Text(s.e621_api_key.expose().to_owned()),
    );
    v.set(
        "sankaku_cookies",
        Value::Text(s.sankaku_cookies.expose().to_owned()),
    );
    v.set(
        "animepictures_cookies",
        Value::Text(s.animepictures_cookies.expose().to_owned()),
    );
    v.set(
        "deviantart_cookies",
        Value::Text(s.deviantart_cookies.expose().to_owned()),
    );
    // the combos' stored values
    v.set("hash_source", choice("hash_source", &s.hash_source));
    v.set(
        "thumbnail_source",
        choice("thumbnail_source", &s.thumbnail_source),
    );
    v.set(
        "sidecar_style",
        Value::Choice(sidecar::style_of(Some(s)).to_owned()),
    );
    v.set(
        "sidecar_overwrite",
        Value::Choice(sidecar::overwrite_policy_of(Some(s)).to_owned()),
    );
    v.set(
        "primary_engine",
        choice("primary_engine", &s.primary_engine),
    );
    v.set(
        "secondary_engine_mode",
        choice("secondary_engine_mode", &s.secondary_engine_mode),
    );
    v.set(
        "auto_import_method",
        choice("auto_import_method", &s.auto_import_method),
    );
    v
}

/// A check list's checked items: the list's own items that `stored` names, in list order.
fn checked_of(id: &str, stored: &[String]) -> Vec<String> {
    match find(PAGES, id) {
        Some(Node::Checks { items, .. }) => items
            .iter()
            .filter(|name| stored.iter().any(|s| s == *name))
            .map(|n| (*n).to_owned())
            .collect(),
        other => panic!("{id} is not a check list in the spec: {other:?}"),
    }
}

fn pairs_text(map: &OrderedMap<String>) -> String {
    map.0
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `apply_to_settings`, minus the Shortcuts lines. `da_snapshot` is
/// `_deviantart_cookies_snapshot`: what the box held when the dialog opened.
pub fn apply(v: &Values, da_snapshot: &str, s: &mut Settings) {
    let engines = ENGINES;
    s.delay_min_seconds = v.n("delay_min");
    s.delay_max_seconds = v.n("delay_max").max(v.n("delay_min"));

    // 0 in either box means "same as above": no override for that engine
    let mut overrides: Vec<(String, Vec<f64>)> = Vec::new();
    for e in engines {
        let (mut low, mut high) = (
            v.n(&format!("engine_delay_min.{e}")),
            v.n(&format!("engine_delay_max.{e}")),
        );
        if low <= 0.0 && high <= 0.0 {
            continue;
        }
        if low <= 0.0 {
            low = high;
        }
        if high <= 0.0 {
            high = low;
        }
        overrides.push(((*e).to_owned(), vec![low.min(high), low.max(high)]));
    }
    s.engine_delays = OrderedMap(overrides);
    s.search_timeout = v.n("search_timeout");
    s.hash_source = v.t("hash_source").to_owned();
    s.thumbnail_source = v.t("thumbnail_source").to_owned();
    s.lazy_thumbnails = v.b("lazy_thumbnails");
    s.hash_workers = v.i("hash_workers");
    s.retrieve_tags_from_booru = v.b("retrieve_booru_tags");
    s.rank_matches_by_quality = v.b("rank_by_quality");
    s.drop_dead_matches = v.b("drop_dead_matches");
    s.drop_restricted_matches = v.b("drop_restricted_matches");
    s.borrow_tags_from_other_matches = v.b("borrow_tags");
    s.borrow_tags_similarity_slack = v.n("borrow_tags_slack");
    s.retry_failed_searches = v.b("retry_failed_searches");
    s.remove_after_import = v.b("remove_after_import");
    s.restore_session_on_start = v.b("restore_session");
    s.autosave_session = v.b("autosave_session");
    s.autosave_interval_seconds = v.i("autosave_interval");
    s.search_cache_ttl_days = v.n("search_cache_ttl");
    s.use_search_cache = v.b("use_search_cache");
    s.auto_import_enabled = v.b("auto_import_enabled");
    s.auto_import_min_similarity = v.n("auto_import_min_similarity");
    s.auto_import_method = v.t("auto_import_method").to_owned();
    s.url_import_confirm_timeout = v.n("url_import_confirm_timeout");
    s.url_import_confirm_interval = v.n("url_import_confirm_interval");
    s.pixiv_session_cookie = Secret::new(strip(v.t("pixiv_session_cookie")));
    s.e621_username = Secret::new(strip(v.t("e621_username")));
    s.e621_api_key = Secret::new(strip(v.t("e621_api_key")));
    s.sankaku_cookies = Secret::new(strip(v.t("sankaku_cookies")));
    s.animepictures_cookies = Secret::new(strip(v.t("animepictures_cookies")));
    // Only written when the user changed the box: a search running while the dialog was open may
    // have rotated the stored cookie to a newer session (DAN-123).
    let typed = strip(v.t("deviantart_cookies"));
    if typed != strip(da_snapshot) {
        s.deviantart_cookies = Secret::new(typed);
    }
    s.enable_tag_namespace_remap = v.b("enable_namespace_remap");

    let mut remap: Vec<(String, String)> = Vec::new();
    for line in splitlines(v.t("namespace_remap_edit")) {
        let line = strip(line);
        let Some((old, new)) = line.split_once(':') else {
            continue;
        };
        let (old, new) = (strip(old), strip(new));
        if !old.is_empty() {
            put(&mut remap, old.to_owned(), new.to_owned());
        }
    }
    s.tag_namespace_remap = OrderedMap(remap);

    s.enable_tag_blacklist = v.b("enable_tag_blacklist");
    s.tag_blacklist = nonblank_lines(v.t("tag_blacklist_edit"));

    s.add_rating_tag = v.b("add_rating_tag");
    s.rating_tag_namespace = strip(v.t("rating_tag_namespace")).to_owned();
    s.tags_for_found = nonblank_lines(v.t("tags_for_found_edit"));
    s.tags_for_not_found = nonblank_lines(v.t("tags_for_not_found_edit"));
    s.tags_for_low_tag_count = nonblank_lines(v.t("tags_for_low_tag_count_edit"));

    s.enable_hydrus_tag_colors = v.b("enable_tag_colors");
    let mut colors: Vec<(String, String)> = Vec::new();
    for line in splitlines(v.t("namespace_colors_edit")) {
        let line = strip(line);
        let Some((ns, color)) = line.split_once(':') else {
            continue;
        };
        let (ns, color) = (strip(ns), strip(color));
        // the namespace may be empty ("unnamespaced"); a colour is required
        if !color.is_empty() {
            put(&mut colors, ns.to_owned(), color.to_owned());
        }
    }
    s.tag_namespace_colors = OrderedMap(colors);

    s.primary_engine = v.t("primary_engine").to_owned();
    s.secondary_engine_mode = v.t("secondary_engine_mode").to_owned();
    s.fallback_below_similarity = v.n("fallback_below_similarity");
    s.extras_only_as_fallback = v.b("extras_only_as_fallback");
    s.enable_ascii2d = v.b("enable_ascii2d");
    s.enable_tracemoe = v.b("enable_tracemoe");
    s.enable_iqdb3d = v.b("enable_iqdb3d");
    s.enable_google_images = v.b("enable_google_images");
    s.google_images_api_key = Secret::new(v.t("google_images_key"));
    s.enable_google_lens = v.b("enable_google_lens");
    s.lens_search_whole_image = v.b("lens_search_whole_image");
    s.enable_yandex = v.b("enable_yandex");
    s.enable_pawchive = v.b("enable_pawchive");
    s.enable_pawchive_index = v.b("enable_pawchive_index");
    s.tracemoe_min_similarity = v.n("tracemoe_min_sim");
    s.log_matched_urls = v.b("log_matched");
    s.log_file_path = v.t("log_path").to_owned();
    s.enabled_tag_sources = v.checked("tag_sources_list");
    s.filter_sent_tags_by_source = v.b("filter_sent_tags");
    s.sidecar_filename_style = v.t("sidecar_style").to_owned();
    s.sidecar_overwrite = v.t("sidecar_overwrite").to_owned();

    s.saucenao.api_key = Secret::new(v.t("saucenao_key"));
    s.saucenao.use_json_api = v.b("saucenao_json");
    s.saucenao.pause_search_on_quota_exhausted = v.b("saucenao_pause_on_quota");
    s.continue_without_saucenao_on_quota = v.b("continue_without_saucenao");
    s.saucenao.min_similarity = v.n("saucenao_min_sim");
    s.saucenao.namespace = v.t("saucenao_namespace").to_owned();
    s.saucenao.tag_types = v.checked("saucenao_tag_types");

    s.hydrus.api_url = v.t("hydrus_url").trim_end_matches('/').to_owned();
    s.hydrus.access_key = Secret::new(v.t("hydrus_key"));
    s.hydrus.tag_service_key = strip(v.t("hydrus_service_key")).to_owned();
    s.hydrus.verify_ssl = v.b("hydrus_verify_ssl");
    s.set_hydrus_duplicate_relationships = v.b("set_hydrus_duplicate_relationships");
    s.write_hydrus_provenance_note = v.b("write_hydrus_provenance_note");
    s.hydrus_provenance_note_name = strip(v.t("hydrus_provenance_note_name")).to_owned();

    s.mcp.enabled = v.b("mcp_enabled");
    s.mcp.port = v.i("mcp_port");
    s.mcp.token = Secret::new(v.t("mcp_token"));
    s.mcp.dry_run = v.b("mcp_dry_run");
    s.mcp.allow_research = v.b("mcp_allow_research");
    s.mcp.allow_hydrus_writes = v.b("mcp_allow_hydrus_writes");
    s.mcp.allow_destructive = v.b("mcp_allow_destructive");
    s.mcp.audit_log_max_bytes = v.i("mcp_audit_max_mb") * 1_000_000;
}

/// The controls whose change restates the Engine page's "Current pipeline" line
/// (`_update_engine_pipeline`'s widget list).
pub const PIPELINE_CONTROLS: &[&str] = &[
    "primary_engine",
    "secondary_engine_mode",
    "fallback_below_similarity",
    "extras_only_as_fallback",
    "enable_ascii2d",
    "enable_tracemoe",
    "enable_iqdb3d",
    "enable_google_images",
    "enable_google_lens",
    "enable_yandex",
    "enable_pawchive",
    "enable_pawchive_index",
];

/// `_update_engine_pipeline`: the sentence for the engine controls as they read now. Python
/// built it on a fresh `Settings()`; `pipeline_summary` reads only these twelve fields, so `base`
/// stands in for it.
pub fn pipeline_text(base: &Settings, v: &Values) -> String {
    let mut t = base.clone();
    t.primary_engine = v.t("primary_engine").to_owned();
    t.secondary_engine_mode = v.t("secondary_engine_mode").to_owned();
    t.fallback_below_similarity = v.n("fallback_below_similarity");
    t.extras_only_as_fallback = v.b("extras_only_as_fallback");
    t.enable_ascii2d = v.b("enable_ascii2d");
    t.enable_tracemoe = v.b("enable_tracemoe");
    t.enable_iqdb3d = v.b("enable_iqdb3d");
    t.enable_google_images = v.b("enable_google_images");
    t.enable_google_lens = v.b("enable_google_lens");
    t.enable_yandex = v.b("enable_yandex");
    t.enable_pawchive = v.b("enable_pawchive");
    t.enable_pawchive_index = v.b("enable_pawchive_index");
    hatate_core::engine_ids::pipeline_summary(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Settings {
        let paths = hatate_core::paths::Paths::from_lookup(|k| match k {
            "HOME" => Some("/tmp/x".into()),
            _ => None,
        })
        .unwrap();
        Settings::with_defaults(&paths)
    }

    /// `apply(initial(s))` with the controls edited by `edit`.
    fn saved(edit: impl FnOnce(&mut Values)) -> Settings {
        let s = defaults();
        let mut v = initial(&s);
        edit(&mut v);
        let mut out = s.clone();
        apply(&v, s.deviantart_cookies.expose(), &mut out);
        out
    }

    #[test]
    fn a_double_spin_rounds_to_its_decimals_then_bounds() {
        assert_eq!(qt_double(1.23456, 0.0, 10.0, 2), 1.23);
        assert_eq!(qt_double(0.005, 0.0, 10.0, 2), 0.01);
        assert_eq!(qt_double(3.0, 5.0, 600.0, 2), 5.0);
        assert_eq!(qt_double(1000.0, 5.0, 300.0, 2), 300.0);
        assert_eq!(qt_double(12.3456, 5.0, 900.0, 2), 12.35);
    }

    #[test]
    fn plain_text_has_one_kind_of_line_ending() {
        assert_eq!(
            qt_plain_text("a\r\nb\rc\u{2028}d\u{2029}e\nf"),
            "a\nb\nc\nd\ne\nf"
        );
    }

    #[test]
    fn an_unopened_dialog_saves_what_python_saves() {
        // untouched controls: the values come back, bounded to the spin ranges
        let s = saved(|_| {});
        assert_eq!(s.delay_min_seconds, 45.0);
        assert_eq!(
            s.tag_namespace_remap.0,
            vec![("artist".to_owned(), "creator".to_owned())]
        );
        assert_eq!(
            s.enabled_tag_sources,
            ["User", "Hydrus", "Booru", "Hatate-linux"]
        );
    }

    #[test]
    fn the_max_delay_never_sits_under_the_min() {
        let s = saved(|v| {
            v.set("delay_min", Value::Num(100.0));
            v.set("delay_max", Value::Num(60.0));
        });
        assert_eq!((s.delay_min_seconds, s.delay_max_seconds), (100.0, 100.0));
    }

    #[test]
    fn an_engine_delay_of_zero_is_no_override_and_one_zero_copies_the_other() {
        let s = saved(|v| {
            v.set("engine_delay_min.iqdb", Value::Num(0.0));
            v.set("engine_delay_max.iqdb", Value::Num(0.0));
            v.set("engine_delay_min.saucenao", Value::Num(0.0));
            v.set("engine_delay_max.saucenao", Value::Num(40.0));
            v.set("engine_delay_min.ascii2d", Value::Num(30.0));
            v.set("engine_delay_max.ascii2d", Value::Num(20.0));
        });
        assert_eq!(
            s.engine_delays.0,
            vec![
                ("saucenao".to_owned(), vec![40.0, 40.0]),
                ("ascii2d".to_owned(), vec![20.0, 30.0]),
            ]
        );
    }

    #[test]
    fn the_deviantart_cookie_is_written_only_when_the_box_was_changed() {
        let mut s = defaults();
        s.deviantart_cookies = Secret::new("old");
        let mut v = initial(&s);
        // a search rotated the stored cookie while the dialog was open
        let mut now = s.clone();
        now.deviantart_cookies = Secret::new("rotated");
        apply(&v, "old", &mut now);
        assert_eq!(now.deviantart_cookies.expose(), "rotated");
        // an edit wins, clearing the box included
        v.set("deviantart_cookies", Value::Text("  ".into()));
        apply(&v, "old", &mut now);
        assert_eq!(now.deviantart_cookies.expose(), "");
    }

    #[test]
    fn remap_and_colour_lines_parse_the_way_python_splits_them() {
        let s = saved(|v| {
            v.set(
                "namespace_remap_edit",
                Value::Text("a:b\n c : d \n\nnocolon\n:x\nk:\na:z".into()),
            );
            v.set(
                "namespace_colors_edit",
                Value::Text("character:#f00\n:#0f0\nns:\nns2 : #123 \ncharacter:#abc".into()),
            );
        });
        // a later line replaces the value and keeps the first place; an empty new name is kept
        assert_eq!(
            s.tag_namespace_remap.0,
            vec![
                ("a".to_owned(), "z".to_owned()),
                ("c".to_owned(), "d".to_owned()),
                ("k".to_owned(), String::new()),
            ]
        );
        // an empty namespace is "unnamespaced"; an empty colour is no entry
        assert_eq!(
            s.tag_namespace_colors.0,
            vec![
                ("character".to_owned(), "#abc".to_owned()),
                (String::new(), "#0f0".to_owned()),
                ("ns2".to_owned(), "#123".to_owned()),
            ]
        );
    }

    #[test]
    fn rule_tags_and_the_blacklist_keep_their_lines_trimmed_without_the_blank_ones() {
        let s = saved(|v| {
            v.set(
                "tag_blacklist_edit",
                Value::Text("  bad_* \n\n meta:*\r\n".into()),
            );
            v.set(
                "tags_for_found_edit",
                Value::Text("hatate:found\n   \nx".into()),
            );
        });
        assert_eq!(s.tag_blacklist, ["bad_*", "meta:*"]);
        assert_eq!(s.tags_for_found, ["hatate:found", "x"]);
    }

    #[test]
    fn the_hydrus_fields_lose_their_trailing_slashes_and_blanks() {
        let s = saved(|v| {
            v.set("hydrus_url", Value::Text("http://h:1///".into()));
            v.set("hydrus_service_key", Value::Text("  abc ".into()));
            v.set("hydrus_provenance_note_name", Value::Text(" n ".into()));
            v.set("hydrus_key", Value::Text(" kept as typed ".into()));
        });
        assert_eq!(s.hydrus.api_url, "http://h:1");
        assert_eq!(s.hydrus.tag_service_key, "abc");
        assert_eq!(s.hydrus_provenance_note_name, "n");
        assert_eq!(s.hydrus.access_key.expose(), " kept as typed ");
    }

    #[test]
    fn a_choice_the_combo_does_not_offer_saves_as_its_first_item() {
        let mut s = defaults();
        s.hash_source = "nonsense".into();
        s.sidecar_overwrite = "nonsense".into();
        let v = initial(&s);
        assert_eq!(v.t("hash_source"), "hydrus");
        assert_eq!(v.t("sidecar_overwrite"), "ask");
    }

    #[test]
    fn checked_lists_keep_the_list_order_and_drop_names_the_list_lacks() {
        let mut s = defaults();
        s.enabled_tag_sources = vec!["Booru".into(), "Gone".into(), "User".into()];
        let v = initial(&s);
        assert_eq!(v.checked("tag_sources_list"), ["User", "Booru"]);
    }
}
