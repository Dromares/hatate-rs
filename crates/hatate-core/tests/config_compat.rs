//! `config.json` compatibility with the Python app: `docs/file-formats.md` section 2 and
//! rules 6.1.1-6.1.11 (DAN-1979). Test names carry the rule they prove (`ruleN_`).
//! Every test runs in its own temp profile (`common::Sandbox`).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{Sandbox, corpus, mode, normalise_log_path};
use hatate_core::config::{LoadReport, Notice, Settings, Unreadable, key_counts};
use serde_json::{Value, json};

const FRESH: &str = "fresh-profile/xdg-config/hatate-linux/config.json";

fn fresh_bytes() -> Vec<u8> {
    fs::read(corpus(FRESH)).unwrap()
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn put_json(sb: &Sandbox, v: &Value) {
    sb.put_config(v.to_string().as_bytes());
}

fn has(report: &LoadReport, f: impl Fn(&Notice) -> bool) -> bool {
    report.notices.iter().any(f)
}

// ---- the key set (rule 6.1.4) -------------------------------------------------------

/// Keys of the spec 2.5 tables, in table order: top-level, then `section.key` for nested.
fn spec_keys() -> (Vec<String>, Vec<String>) {
    let doc = common::doc();
    let top_start = doc.find("#### Top-level keys (79)").unwrap();
    let nested_start = doc.find("#### Nested keys (24").unwrap();
    let end = doc.find("**Secrets.**").unwrap();
    let rows = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("| `"))
            .map(|l| l.split('`').nth(1).unwrap().to_owned())
            .collect()
    };
    (
        rows(&doc[top_start..nested_start]),
        rows(&doc[nested_start..end]),
    )
}

#[test]
fn rule4_settings_has_exactly_the_103_keys_of_the_spec_in_order() {
    let (top, nested) = spec_keys();
    assert_eq!((top.len(), nested.len()), (79, 24));
    assert_eq!(key_counts(), (79, 24));
    assert_eq!(
        Settings::KEYS,
        top.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let ours: Vec<String> = [
        ("saucenao", hatate_core::config::SauceNaoSettings::KEYS),
        ("hydrus", hatate_core::config::HydrusSettings::KEYS),
        (
            "match_conditions",
            hatate_core::config::MatchConditions::KEYS,
        ),
        ("mcp", hatate_core::config::McpSettings::KEYS),
    ]
    .iter()
    .flat_map(|(s, keys)| keys.iter().map(move |k| format!("{s}.{k}")))
    .collect();
    let mut a = ours.clone();
    let mut b = nested.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn rule4_written_file_has_all_103_keys_and_no_others() {
    let sb = Sandbox::new();
    let s = Settings::with_defaults(&sb.paths);
    let v = json_of(s.to_json_string().as_bytes());
    let obj = v.as_object().unwrap();
    let (top, nested) = spec_keys();
    assert_eq!(obj.keys().cloned().collect::<Vec<_>>(), top);
    for name in ["saucenao", "hydrus", "match_conditions", "mcp"] {
        let want: Vec<&String> = nested
            .iter()
            .filter(|k| k.starts_with(&format!("{name}.")))
            .collect();
        let got: Vec<String> = obj[name]
            .as_object()
            .unwrap()
            .keys()
            .map(|k| format!("{name}.{k}"))
            .collect();
        assert_eq!(got.iter().collect::<Vec<_>>(), want, "{name}");
    }
    assert!(obj.contains_key("window_geometry"));
}

#[test]
fn rule4_nested_unknown_keys_never_reach_the_file() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 13, "hydrus": {"api_url": "http://h:1", "bogus": 1}, "mcp": {"port": 9, "x": {"y": 1}}}),
    );
    let loaded = Settings::load(&sb.paths);
    assert!(has(
        &loaded.report,
        |n| matches!(n, Notice::UnknownNestedKey { section } if section == "hydrus")
    ));
    assert_eq!(loaded.settings.hydrus.api_url, "http://h:1");
    assert_eq!(loaded.settings.mcp.port, 9);
    loaded.settings.save(&sb.paths).unwrap();
    let v = json_of(&sb.config_bytes());
    assert_eq!(v["hydrus"].as_object().unwrap().len(), 5);
    assert_eq!(v["mcp"].as_object().unwrap().len(), 8);
    assert!(
        !String::from_utf8(sb.config_bytes())
            .unwrap()
            .contains("bogus")
    );
}

#[test]
fn rule4_nothing_rust_only_is_written_to_config_json() {
    let sb = Sandbox::new();
    let loaded = Settings::load(&sb.paths);
    let text = String::from_utf8(sb.config_bytes()).unwrap();
    assert!(!text.contains("hatate-rs") && !text.contains("hatate_rs"));
    assert!(!sb.paths.rust_settings_file().exists());
    drop(loaded);
}

// ---- reading (rules 6.1.2, 6.1.9) ---------------------------------------------------

#[test]
fn rule2_missing_keys_use_defaults_and_unknown_top_level_keys_are_dropped() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 13, "delay_min_seconds": 12.5, "from_the_future": [1, 2]}),
    );
    let loaded = Settings::load(&sb.paths);
    let defaults = Settings::with_defaults(&sb.paths);
    assert_eq!(loaded.settings.delay_min_seconds, 12.5);
    assert_eq!(
        loaded.settings.delay_max_seconds,
        defaults.delay_max_seconds
    );
    assert_eq!(loaded.settings.enabled_sites, defaults.enabled_sites);
    assert!(has(&loaded.report, |n| matches!(
        n,
        Notice::UnknownKeysIgnored { count: 1 }
    )));
    // Schema 13 and nothing wrong: no write happened.
    assert!(!sb.paths.config_backup_file().exists());
    loaded.settings.save(&sb.paths).unwrap();
    assert!(
        !String::from_utf8(sb.config_bytes())
            .unwrap()
            .contains("from_the_future")
    );
}

#[test]
fn rule2_newer_schema_version_loads_and_is_kept() {
    let sb = Sandbox::new();
    put_json(&sb, &json!({"schema_version": 99, "theme": "dark"}));
    let before = sb.config_bytes();
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings.schema_version, 99);
    assert_eq!(loaded.settings.theme, "dark");
    assert!(!has(&loaded.report, |n| matches!(
        n,
        Notice::Migrated { .. }
    )));
    assert_eq!(
        sb.config_bytes(),
        before,
        "loading a newer config must not rewrite it"
    );
    loaded.settings.save(&sb.paths).unwrap();
    assert_eq!(json_of(&sb.config_bytes())["schema_version"], 99);
}

#[test]
fn rule5_schema_version_is_13_whenever_a_migration_wrote_it() {
    for stored in [
        json!(null),
        json!(1),
        json!(12),
        json!(0),
        json!(-4),
        json!("abc"),
        json!(7.5),
        json!(true),
    ] {
        let sb = Sandbox::new();
        let mut cfg = json!({"theme": "light"});
        if !stored.is_null() {
            cfg["schema_version"] = stored.clone();
        }
        put_json(&sb, &cfg);
        let loaded = Settings::load(&sb.paths);
        assert_eq!(loaded.settings.schema_version, 13, "stored {stored}");
        assert_eq!(
            json_of(&sb.config_bytes())["schema_version"],
            13,
            "stored {stored}"
        );
    }
}

#[test]
fn rule9_existing_log_file_path_is_never_replaced() {
    let sb = Sandbox::new();
    sb.put_config(&fresh_bytes());
    let loaded = Settings::load(&sb.paths);
    assert!(
        loaded
            .settings
            .log_file_path
            .starts_with("/tmp/hatate-fresh-profile-")
    );
    assert!(
        !loaded
            .settings
            .log_file_path
            .contains(sb.dir.path().to_str().unwrap())
    );
    // And a migration save keeps it too.
    let sb = Sandbox::new();
    put_json(&sb, &json!({"log_file_path": "/elsewhere/urls.log"}));
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings.log_file_path, "/elsewhere/urls.log");
    assert_eq!(
        json_of(&sb.config_bytes())["log_file_path"],
        "/elsewhere/urls.log"
    );
}

#[test]
fn rule9_a_new_config_gets_a_log_path_inside_config_dir() {
    let sb = Sandbox::new();
    let loaded = Settings::load(&sb.paths);
    assert!(has(&loaded.report, |n| matches!(n, Notice::FirstRun)));
    assert_eq!(
        loaded.settings.log_file_path,
        sb.paths.default_matched_urls_log().to_str().unwrap()
    );
    assert!(sb.paths.config_file().exists(), "first run saves at once");
}

#[test]
fn rule6_qt_header_blobs_are_kept_verbatim() {
    let sb = Sandbox::new();
    let blob = "AAAA/wAAAAAAAQAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA+/=\u{e9}\u{1f600}";
    for version in [json!(5), json!(13)] {
        put_json(
            &sb,
            &json!({"schema_version": version, "table_header_state": blob, "table_header_columns": 11}),
        );
        let loaded = Settings::load(&sb.paths);
        assert_eq!(loaded.settings.table_header_state, blob);
        assert_eq!(loaded.settings.table_header_columns, 11);
        loaded.settings.save(&sb.paths).unwrap();
        let v = json_of(&sb.config_bytes());
        assert_eq!(v["table_header_state"], blob);
        assert_eq!(v["table_header_columns"], 11);
    }
}

#[test]
fn rule11_known_keys_survive_a_save_and_reload() {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    s.theme = "dark".into();
    s.hash_workers = 4;
    s.enabled_sites = vec!["Danbooru".into(), "Other".into()];
    s.engine_delays.0.push(("iqdb".into(), vec![1.5, 2.0]));
    s.tag_namespace_colors.0.push(("".into(), "#ffffff".into()));
    s.hydrus.timeout = 7.25;
    s.save(&sb.paths).unwrap();
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings, s);
    assert!(loaded.report.notices.is_empty(), "{}", loaded.report);
}

// ---- migrations (rule 6.1.2, spec 2.3) ----------------------------------------------

/// The sites each migration adds, written out from spec 2.3 (not read from the code).
fn expected_sites(from: i64, other: bool) -> Vec<&'static str> {
    let mut v = vec!["Danbooru"];
    if other {
        v.push("Other");
    }
    let steps: [(i64, &[&str], bool); 8] = [
        (3, &["e621", "Safebooru"], false),
        (6, &["Rule34", "Xbooru", "Twitter", "AniList"], false),
        (7, &["Paheal"], false),
        (8, &["MangaUpdates"], false),
        (9, &["MyAnimeList"], false),
        (10, &["Rule34.us"], true),
        (11, &["Reddit"], true),
        (12, &["Toon34"], true),
    ];
    for (below, names, needs_other) in steps {
        if from < below && (!needs_other || other) {
            v.extend(names);
        }
    }
    v
}

#[test]
fn rule2_every_schema_version_migrates_the_site_list() {
    for from in 1..=13i64 {
        for other in [true, false] {
            let sb = Sandbox::new();
            let sites: Vec<&str> = if other {
                vec!["Danbooru", "Other"]
            } else {
                vec!["Danbooru"]
            };
            let mut cfg = json!({"enabled_sites": sites});
            if from > 1 {
                cfg["schema_version"] = json!(from);
            }
            put_json(&sb, &cfg);
            let loaded = Settings::load(&sb.paths);
            assert_eq!(
                loaded.settings.enabled_sites,
                expected_sites(from, other),
                "from {from}, other {other}"
            );
            assert_eq!(loaded.settings.schema_version, 13);
            assert_eq!(
                has(
                    &loaded.report,
                    |n| matches!(n, Notice::Migrated { from: f } if *f == from)
                ),
                from < 13
            );
            // What is on disk is what is in memory: schema 13, same sites.
            let on_disk = json_of(&sb.config_bytes());
            assert_eq!(on_disk["schema_version"], 13);
            assert_eq!(on_disk["enabled_sites"], json!(expected_sites(from, other)));
        }
    }
}

#[test]
fn rule2_migration_is_idempotent_for_sites_already_present() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 2, "enabled_sites": ["Safebooru", "Other", "Reddit"]}),
    );
    let loaded = Settings::load(&sb.paths);
    let got = &loaded.settings.enabled_sites;
    assert_eq!(got.iter().filter(|s| *s == "Safebooru").count(), 1);
    assert_eq!(got.iter().filter(|s| *s == "Reddit").count(), 1);
    assert_eq!(got.first().map(String::as_str), Some("Safebooru"));
}

#[test]
fn rule2_migration_2_adds_the_hydrus_tag_source_once() {
    for (version, expect) in [
        (1, vec!["User", "Hydrus"]),
        (2, vec!["User"]),
        (13, vec!["User"]),
    ] {
        let sb = Sandbox::new();
        put_json(
            &sb,
            &json!({"schema_version": version, "enabled_tag_sources": ["User"]}),
        );
        assert_eq!(
            Settings::load(&sb.paths).settings.enabled_tag_sources,
            expect,
            "v{version}"
        );
    }
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 1, "enabled_tag_sources": ["Hydrus", "User"]}),
    );
    assert_eq!(
        Settings::load(&sb.paths).settings.enabled_tag_sources,
        ["Hydrus", "User"]
    );
}

#[test]
fn rule2_legacy_use_saucenao_fallback_maps_to_secondary_engine_mode() {
    let cases = [
        (json!(true), Some("always")),
        (json!(false), Some("disabled")),
        (json!(0), Some("disabled")),
        (json!("no"), Some("always")), // any non-empty string is truthy in Python
        (json!(""), Some("disabled")),
        (json!(null), None),
    ];
    for (old, want) in cases {
        let sb = Sandbox::new();
        put_json(
            &sb,
            &json!({"schema_version": 3, "use_saucenao_fallback": old, "secondary_engine_mode": "fallback"}),
        );
        let got = Settings::load(&sb.paths).settings.secondary_engine_mode;
        assert_eq!(got, want.unwrap_or("fallback"), "{old}");
    }
    // From schema 4 on the legacy key is ignored.
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 4, "use_saucenao_fallback": false}),
    );
    assert_eq!(
        Settings::load(&sb.paths).settings.secondary_engine_mode,
        "always"
    );
}

#[test]
fn rule2_legacy_use_filename_hashes_maps_to_hash_source() {
    for (old, want) in [
        (json!(true), "hydrus"),
        (json!(false), "local"),
        (json!(null), "hydrus"),
    ] {
        let sb = Sandbox::new();
        put_json(
            &sb,
            &json!({"schema_version": 4, "use_filename_hashes": old}),
        );
        assert_eq!(
            Settings::load(&sb.paths).settings.hash_source,
            want,
            "{old}"
        );
    }
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 5, "use_filename_hashes": false}),
    );
    assert_eq!(Settings::load(&sb.paths).settings.hash_source, "hydrus");
}

#[test]
fn rule2_legacy_keys_are_read_but_never_written() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"use_saucenao_fallback": false, "use_filename_hashes": false}),
    );
    Settings::load(&sb.paths);
    let text = String::from_utf8(sb.config_bytes()).unwrap();
    assert!(!text.contains("use_saucenao_fallback") && !text.contains("use_filename_hashes"));
}

#[test]
fn rule2_migration_13_pins_filter_sent_tags_off_only_below_13() {
    for (version, want) in [(12, false), (1, false), (13, true)] {
        let sb = Sandbox::new();
        put_json(
            &sb,
            &json!({"schema_version": version, "filter_sent_tags_by_source": true}),
        );
        assert_eq!(
            Settings::load(&sb.paths)
                .settings
                .filter_sent_tags_by_source,
            want,
            "v{version}"
        );
    }
}

#[test]
fn rule2_legacy_migration_sample_matches_what_python_made_of_it() {
    let sb = Sandbox::new();
    let input = fs::read(corpus("legacy-migration/input/config.json")).unwrap();
    sb.put_config(&input);
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings.schema_version, 13);
    assert_eq!(loaded.settings.hash_source, "local");
    assert_eq!(loaded.settings.secondary_engine_mode, "disabled");
    // The sample's own `hydrus` block names only two of five keys: the rest are defaults.
    assert_eq!(loaded.settings.hydrus.timeout, 30.0);
    let python = fs::read(corpus("legacy-migration/after-load/config.json")).unwrap();
    assert_eq!(
        normalise_log_path(&sb.config_bytes()),
        normalise_log_path(&python)
    );
    // `.bak` holds the input bytes, as in Python (spec 2.3).
    assert_eq!(sb.read(sb.paths.config_backup_file()), input);
    assert_eq!(
        fs::read(corpus("legacy-migration/after-load/config.json.bak")).unwrap(),
        input
    );
}

// ---- bytes (rule 6.1.8) -------------------------------------------------------------

#[test]
fn rule8_fresh_profile_round_trips_byte_for_byte() {
    let sb = Sandbox::new();
    let original = fresh_bytes();
    sb.put_config(&original);
    let loaded = Settings::load(&sb.paths);
    assert!(loaded.report.notices.is_empty(), "{}", loaded.report);
    loaded.settings.save(&sb.paths).unwrap();
    // Only log_file_path may differ, and it does not: it is preserved (rule 6.1.9).
    assert_eq!(sb.config_bytes(), original);
}

#[test]
fn rule8_fresh_defaults_equal_the_python_default_file() {
    // A new profile: only log_file_path is machine-specific.
    let sb = Sandbox::new();
    Settings::load(&sb.paths);
    assert_eq!(
        normalise_log_path(&sb.config_bytes()),
        normalise_log_path(&fresh_bytes())
    );
    let own = String::from_utf8(sb.config_bytes()).unwrap();
    assert!(own.contains(&format!(
        "\"log_file_path\": \"{}\"",
        sb.paths.default_matched_urls_log().display()
    )));
}

#[test]
fn rule8_after_load_sample_round_trips_byte_for_byte() {
    let sb = Sandbox::new();
    let python = fs::read(corpus("legacy-migration/after-load/config.json")).unwrap();
    sb.put_config(&python);
    Settings::load(&sb.paths).settings.save(&sb.paths).unwrap();
    assert_eq!(sb.config_bytes(), python);
}

#[test]
fn rule8_python_format_details() {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    s.hydrus_provenance_note_name = "caf\u{e9} \u{1f600} \"q\" \\ \n".into();
    s.search_timeout = 0.00001;
    s.delay_min_seconds = 1e16;
    s.fallback_below_similarity = 2.5e-7;
    s.engine_delays.0.push(("iqdb".into(), vec![1.0, 2.5]));
    s.engine_delays.0.push(("yandex".into(), vec![]));
    let text = s.to_json_string();
    assert!(!text.ends_with('\n'), "no trailing newline");
    assert!(text.starts_with("{\n  \"schema_version\": 13,\n"));
    assert!(
        text.contains(r#""hydrus_provenance_note_name": "caf\u00e9 \ud83d\ude00 \"q\" \\ \n","#),
        "{text}"
    );
    assert!(text.contains("\"search_timeout\": 1e-05,"));
    assert!(text.contains("\"delay_min_seconds\": 1e+16,"));
    assert!(text.contains("\"fallback_below_similarity\": 2.5e-07,"));
    assert!(text.contains("\"engine_delays\": {\n    \"iqdb\": [\n      1.0,\n      2.5\n    ],\n    \"yandex\": []\n  },"));
    assert!(text.contains("\"tags_for_found\": [],"));
    assert!(text.ends_with("  \"table_header_columns\": 0\n}"));
    // And the Python reader's view of it is the same document.
    let v = json_of(text.as_bytes());
    assert_eq!(
        v["hydrus_provenance_note_name"],
        "caf\u{e9} \u{1f600} \"q\" \\ \n"
    );
}

#[test]
fn rule8_dict_fields_keep_file_order() {
    let sb = Sandbox::new();
    sb.put_config(br##"{"schema_version":13,"tag_namespace_colors":{"z":"#000000","a":"#111111","":"#222222"},"review_shortcuts":{"b":"X","a":""}}"##);
    let loaded = Settings::load(&sb.paths);
    loaded.settings.save(&sb.paths).unwrap();
    let text = String::from_utf8(sb.config_bytes()).unwrap();
    let z = text.find("\"z\"").unwrap();
    let a = text.find("\"a\": \"#111111\"").unwrap();
    let empty = text.find("\"\": \"#222222\"").unwrap();
    assert!(z < a && a < empty);
    assert!(text.find("\"b\": \"X\"").unwrap() < text.find("\"a\": \"\"").unwrap());
}

// ---- bad input (rule 6.1.3) ---------------------------------------------------------

/// Run a load against `bytes` as `config.json`; assert it did not panic, the file was
/// kept as `config.json.unreadable` byte for byte, and defaults are in effect.
fn assert_set_aside(label: &str, bytes: &[u8]) -> LoadReport {
    let sb = Sandbox::new();
    sb.put_config(bytes);
    let loaded = Settings::load(&sb.paths);
    assert_eq!(
        loaded.settings,
        Settings::with_defaults(&sb.paths),
        "{label}"
    );
    assert!(
        has(&loaded.report, |n| matches!(n, Notice::NothingReadable)),
        "{label}"
    );
    assert_eq!(
        sb.read(sb.paths.config_unreadable_file()),
        bytes,
        "{label}: the bad file must be kept as it was"
    );
    assert!(
        !sb.paths.config_file().exists(),
        "{label}: main moved aside, nothing written over it"
    );
    assert!(
        has(&loaded.report, |n| matches!(
            n,
            Notice::SetAsideUnreadable { .. }
        )),
        "{label}"
    );
    // A later save must not touch the kept copy.
    loaded.settings.save(&sb.paths).unwrap();
    assert_eq!(
        sb.read(sb.paths.config_unreadable_file()),
        bytes,
        "{label}: still intact after a save"
    );
    loaded.report
}

#[test]
fn rule3_top_level_array_is_unreadable_not_a_crash() {
    let r = assert_set_aside("array", b"[1, 2, 3]");
    assert!(has(&r, |n| matches!(
        n,
        Notice::CouldNotRead {
            why: Unreadable::NotAnObject,
            ..
        }
    )));
    for other in [&b"null"[..], b"\"str\"", b"42", b"true"] {
        assert_set_aside("non-object", other);
    }
}

#[test]
fn rule3_bom_is_unreadable_like_python() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(br#"{"schema_version": 13, "theme": "dark"}"#);
    let r = assert_set_aside("bom", &bytes);
    assert!(has(&r, |n| matches!(
        n,
        Notice::CouldNotRead {
            why: Unreadable::Bom,
            ..
        }
    )));
}

#[test]
fn rule3_truncated_empty_and_garbage_files_never_panic() {
    let full = fresh_bytes();
    for cut in [0, 1, 2, 17, full.len() / 2, full.len() - 1] {
        assert_set_aside(&format!("truncated at {cut}"), &full[..cut]);
    }
    assert_set_aside("garbage", &[0, 1, 2, 3, 255, 254, 253]);
    assert_set_aside("invalid utf-8", b"{\"theme\": \"\xff\xfe\"}");
    assert_set_aside("trailing garbage", br#"{"theme": "dark"} trailing"#);
    assert_set_aside("deeply nested", "[".repeat(100_000).as_bytes());
    assert_set_aside(
        "nested object depth",
        format!("{}1{}", "{\"a\":".repeat(5000), "}".repeat(5000)).as_bytes(),
    );
}

#[test]
fn rule3_wrong_typed_values_fall_back_to_the_default_and_are_reported() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({
            "schema_version": 13,
            "search_timeout": "abc",
            "delay_min_seconds": null,
            "hash_workers": 2.5,
            "lazy_thumbnails": "yes",
            "enabled_sites": [1, 2],
            "tag_blacklist": "nope",
            "engine_delays": {"iqdb": [1, "x"]},
            "tag_namespace_remap": {"artist": 3},
            "hydrus": "not an object",
            "saucenao": {"min_similarity": "high", "use_json_api": false},
            "theme": 5,
            "hash_source": "local"
        }),
    );
    let loaded = Settings::load(&sb.paths);
    let d = Settings::with_defaults(&sb.paths);
    let s = &loaded.settings;
    assert_eq!(s.search_timeout, d.search_timeout);
    assert_eq!(s.delay_min_seconds, d.delay_min_seconds);
    assert_eq!(s.hash_workers, d.hash_workers);
    assert_eq!(s.lazy_thumbnails, d.lazy_thumbnails);
    assert_eq!(s.enabled_sites, d.enabled_sites);
    assert_eq!(s.tag_blacklist, d.tag_blacklist);
    assert_eq!(s.engine_delays, d.engine_delays);
    assert_eq!(s.tag_namespace_remap, d.tag_namespace_remap);
    assert_eq!(s.hydrus, d.hydrus);
    assert_eq!(s.theme, d.theme);
    // The good values next to the bad ones are kept.
    assert_eq!(s.saucenao.min_similarity, d.saucenao.min_similarity);
    assert!(!s.saucenao.use_json_api);
    assert_eq!(s.hash_source, "local");
    let wrong: Vec<&str> = loaded
        .report
        .notices
        .iter()
        .filter_map(|n| match n {
            Notice::WrongType { key } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    for k in [
        "search_timeout",
        "delay_min_seconds",
        "hash_workers",
        "lazy_thumbnails",
        "enabled_sites",
        "tag_blacklist",
        "engine_delays",
        "tag_namespace_remap",
        "hydrus",
        "saucenao.min_similarity",
        "theme",
    ] {
        assert!(wrong.contains(&k), "{k} not reported: {wrong:?}");
    }
}

#[test]
fn rule3_integers_in_float_fields_and_integral_floats_in_int_fields_are_accepted() {
    let sb = Sandbox::new();
    put_json(
        &sb,
        &json!({"schema_version": 13, "search_timeout": 30, "hash_workers": 3.0}),
    );
    let loaded = Settings::load(&sb.paths);
    assert_eq!(
        (loaded.settings.search_timeout, loaded.settings.hash_workers),
        (30.0, 3)
    );
    assert!(loaded.report.notices.is_empty());
}

#[test]
fn rule3_unreadable_main_with_good_backup_recovers_and_keeps_the_bad_file() {
    let sb = Sandbox::new();
    put_json(&sb, &json!({"schema_version": 13, "theme": "light"}));
    Settings::load(&sb.paths).settings.save(&sb.paths).unwrap(); // makes .bak == the good file
    let good = sb.config_bytes();
    // A second save so the backup is a faithful copy of a good main, then corrupt main.
    Settings::load(&sb.paths).settings.save(&sb.paths).unwrap();
    assert_eq!(sb.read(sb.paths.config_backup_file()), good);
    sb.put_config(b"{ this is not json");
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings.theme, "light");
    assert!(has(&loaded.report, |n| matches!(
        n,
        Notice::RecoveredFromBackup
    )));
    assert_eq!(
        sb.read(sb.paths.config_unreadable_file()),
        b"{ this is not json"
    );
    assert!(!sb.paths.config_file().exists());
}

#[test]
fn rule3_an_existing_unreadable_file_is_never_overwritten() {
    let sb = Sandbox::new();
    sb.put_config(b"first broken");
    Settings::load(&sb.paths);
    sb.put_config(b"second broken");
    Settings::load(&sb.paths);
    assert_eq!(sb.read(sb.paths.config_unreadable_file()), b"first broken");
    assert_eq!(
        sb.read(sb.config_dir().join("config.json.unreadable.1")),
        b"second broken"
    );
}

#[test]
fn rule3_missing_config_with_only_a_backup_starts_from_defaults_like_python() {
    let sb = Sandbox::new();
    fs::create_dir_all(sb.config_dir()).unwrap();
    fs::write(
        sb.paths.config_backup_file(),
        br#"{"schema_version": 13, "theme": "dark"}"#,
    )
    .unwrap();
    let loaded = Settings::load(&sb.paths);
    assert_eq!(loaded.settings.theme, "system");
    assert!(has(&loaded.report, |n| matches!(n, Notice::FirstRun)));
}

// ---- writing (rule 6.1.7) -----------------------------------------------------------

#[test]
fn rule7_save_is_private_and_leaves_no_tmp_file() {
    let sb = Sandbox::new();
    let s = Settings::with_defaults(&sb.paths);
    s.save(&sb.paths).unwrap();
    assert_eq!(mode(sb.config_dir()), 0o700);
    assert_eq!(mode(&sb.paths.config_file()), 0o600);
    assert!(!sb.paths.config_tmp_file().exists());
    s.save(&sb.paths).unwrap();
    assert_eq!(mode(&sb.paths.config_backup_file()), 0o600);
    assert_eq!(sb.listing(), ["config.json", "config.json.bak"]);
}

#[test]
fn rule7_save_tightens_loose_permissions() {
    let sb = Sandbox::new();
    sb.put_config(&fresh_bytes());
    fs::set_permissions(sb.config_dir(), fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(sb.paths.config_file(), fs::Permissions::from_mode(0o644)).unwrap();
    // A stale tmp file from an older run, world-readable.
    fs::write(sb.paths.config_tmp_file(), b"stale").unwrap();
    fs::set_permissions(
        sb.paths.config_tmp_file(),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    // Loading alone tightens (spec 2.2.4).
    let loaded = Settings::load(&sb.paths);
    assert_eq!(mode(sb.config_dir()), 0o700);
    assert_eq!(mode(&sb.paths.config_file()), 0o600);
    loaded.settings.save(&sb.paths).unwrap();
    assert_eq!(mode(&sb.paths.config_file()), 0o600);
    assert_eq!(mode(&sb.paths.config_backup_file()), 0o600);
    assert!(!sb.paths.config_tmp_file().exists());
}

#[test]
fn rule7_previous_config_becomes_the_backup_before_the_new_one_lands() {
    let sb = Sandbox::new();
    let original = fresh_bytes();
    sb.put_config(&original);
    let mut s = Settings::load(&sb.paths).settings;
    s.theme = "dark".into();
    s.save(&sb.paths).unwrap();
    assert_eq!(sb.read(sb.paths.config_backup_file()), original);
    assert_ne!(sb.config_bytes(), original);
}

#[test]
fn rule7_a_failed_write_leaves_the_old_config_untouched() {
    let sb = Sandbox::new();
    let original = fresh_bytes();
    sb.put_config(&original);
    // The tmp name is taken by a directory: opening it for writing must fail.
    fs::create_dir(sb.paths.config_tmp_file()).unwrap();
    let mut s = Settings::load(&sb.paths).settings;
    s.theme = "dark".into();
    let err = s.save(&sb.paths).unwrap_err();
    assert_eq!(err.step, "write");
    assert_eq!(
        sb.config_bytes(),
        original,
        "the live config is either the old or the new file, never half of one"
    );
}

#[test]
fn rule7_save_creates_the_directory_private() {
    let sb = Sandbox::new();
    assert!(!sb.config_dir().exists());
    Settings::with_defaults(&sb.paths).save(&sb.paths).unwrap();
    assert_eq!(mode(sb.config_dir()), 0o700);
}

#[test]
fn rule7_save_reports_a_backup_failure_but_still_saves() {
    let sb = Sandbox::new();
    sb.put_config(&fresh_bytes());
    fs::create_dir(sb.paths.config_backup_file()).unwrap();
    let notices = Settings::with_defaults(&sb.paths).save(&sb.paths).unwrap();
    assert!(
        notices
            .iter()
            .any(|n| matches!(n, Notice::CouldNotBackUp { .. }))
    );
    assert!(json_of(&sb.config_bytes()).is_object());
}

// ---- what the Python reader needs (spec 6.5) ----------------------------------------

#[test]
fn written_file_is_a_json_object_in_utf8_without_a_bom() {
    let sb = Sandbox::new();
    let mut s = Settings::with_defaults(&sb.paths);
    s.theme = "d\u{e9}j\u{e0}".into();
    s.save(&sb.paths).unwrap();
    let bytes = sb.config_bytes();
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(bytes.is_ascii(), "ensure_ascii output");
    assert!(json_of(&bytes).is_object());
}

// ---- secrets (rule 6.1.10) ----------------------------------------------------------

const SECRET_KEYS: [(&str, Option<&str>); 10] = [
    ("google_images_api_key", None),
    ("e621_api_key", None),
    ("e621_username", None),
    ("pixiv_session_cookie", None),
    ("sankaku_cookies", None),
    ("animepictures_cookies", None),
    ("deviantart_cookies", None),
    ("api_key", Some("saucenao")),
    ("access_key", Some("hydrus")),
    ("token", Some("mcp")),
];

#[test]
fn rule10_no_secret_reaches_debug_display_or_a_report() {
    let sb = Sandbox::new();
    let mut cfg = json!({"schema_version": 5, "saucenao": {}, "hydrus": {}, "mcp": {"bogus_next_to_token": 1}});
    let mut canaries = Vec::new();
    for (i, (key, section)) in SECRET_KEYS.iter().enumerate() {
        let canary = format!("CANARY-{i}-s3cr3t-\u{e9}-{key}");
        match section {
            None => cfg[key] = json!(canary),
            Some(s) => cfg[s][key] = json!(canary),
        }
        canaries.push(canary);
    }
    // Wrong-typed neighbours, so the report has entries that sit next to secrets.
    cfg["search_timeout"] = json!("CANARY-wrong-typed");
    put_json(&sb, &cfg);
    let loaded = Settings::load(&sb.paths);

    // The values did load: they are in the settings, reachable only through `expose`.
    assert_eq!(loaded.settings.e621_api_key.expose(), canaries[1]);
    assert_eq!(loaded.settings.mcp.token.expose(), canaries[9]);

    let mut surfaces = vec![
        format!("{:?}", loaded.settings),
        format!("{:#?}", loaded.settings),
        format!("{:?}", loaded),
        format!("{:#?}", loaded),
        format!("{}", loaded.report),
        format!("{:?}", loaded.report),
        format!("{:?}", loaded.settings.saucenao),
        format!("{:?}", loaded.settings.hydrus.access_key),
    ];
    for n in &loaded.report.notices {
        surfaces.push(format!("{n} | {n:?}"));
    }
    // The error type of a failing save.
    fs::create_dir(sb.paths.config_tmp_file()).unwrap();
    let err = loaded.settings.save(&sb.paths).unwrap_err();
    surfaces.push(format!("{err} | {err:?}"));
    for surface in &surfaces {
        for c in canaries.iter().chain([&"CANARY-wrong-typed".to_owned()]) {
            assert!(
                !surface.contains(c.as_str()),
                "secret leaked: {c} in {surface}"
            );
        }
    }
    assert!(surfaces[0].contains("<redacted>"));
    fs::remove_dir(sb.paths.config_tmp_file()).unwrap();

    // Bad input must not echo content in its error text either.
    for (label, bytes) in [
        ("syntax", &b"{\"api_key\": \"CANARY-bad\" \"x\": 1}"[..]),
        ("bom", b"\xEF\xBB\xBF{\"token\": \"CANARY-bad\"}"),
        ("utf8", b"{\"token\": \"CANARY-bad\xff\"}"),
        ("array", b"[\"CANARY-bad\"]"),
    ] {
        let report = assert_set_aside(label, bytes);
        let text = format!("{report} {report:?}");
        assert!(!text.contains("CANARY"), "{label}: {text}");
    }
}

#[test]
fn rule10_every_secret_key_of_the_spec_is_a_secret_in_the_model() {
    // The list in spec 2.5 "Secrets" and the Debug output agree: each prints redacted.
    let doc = common::doc();
    let at = doc.find("**Secrets.**").unwrap();
    let para = &doc[at..at + doc[at..].find("\n\n").unwrap()];
    let spec: Vec<&str> = para
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|k| !k.contains('/') && !k.contains(':'))
        .collect();
    assert!(spec.len() >= 10, "{spec:?}");
    let sb = Sandbox::new();
    let dbg = format!("{:#?}", Settings::with_defaults(&sb.paths));
    for key in spec {
        let leaf = key.rsplit('.').next().unwrap();
        // A derived Debug prints `field: Secret(<redacted>)`; find that field's line.
        let line = dbg
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{leaf}: ")))
            .unwrap_or_else(|| panic!("{key} not in Debug"));
        assert!(line.contains("Secret(<redacted>)"), "{key}: {line}");
    }
}
