//! `core/engines.py`: canonical ids and display names for the reverse-image-search engines, and
//! which `Settings` switch turns which engine on.
//!
//! The ids are `config.json` values (`primary_engine`) and the `override_engine` the runner takes,
//! so they must round-trip unchanged ("Stable; do not rename without a config migration").
//! `MatchCandidate.engine` holds a *label*, not an id; the helpers that take one compare it
//! case-insensitively after stripping.

use crate::config::Settings;
use crate::pycompat;

pub const IQDB: &str = "iqdb";
pub const SAUCENAO: &str = "saucenao";
pub const ASCII2D: &str = "ascii2d";
pub const TRACEMOE: &str = "tracemoe";
pub const IQDB3D: &str = "iqdb3d";
pub const GOOGLE_IMAGES: &str = "googleimages";
pub const GOOGLE_LENS: &str = "googlelens";
pub const YANDEX: &str = "yandex";
pub const PAWCHIVE: &str = "pawchive";
pub const PAWCHIVE_INDEX: &str = "pawchiveindex";

/// The two engines that take part in the primary / secondary order. Every other engine is an
/// independent toggle and runs in parallel with that pair when enabled.
pub const PRIMARY_ENGINES: [&str; 2] = [IQDB, SAUCENAO];

pub const ALL_ENGINES: [&str; 10] = [
    IQDB,
    SAUCENAO,
    ASCII2D,
    TRACEMOE,
    IQDB3D,
    GOOGLE_IMAGES,
    GOOGLE_LENS,
    YANDEX,
    PAWCHIVE,
    PAWCHIVE_INDEX,
];

/// What `MatchCandidate.engine` and the Engine column show. Distinct from the id so a row from
/// IQDB 3D does not look like one from regular IQDB. In `ALL_ENGINES` order.
pub const ENGINE_LABELS: [(&str, &str); 10] = [
    (IQDB, "IQDB"),
    (SAUCENAO, "SauceNAO"),
    (ASCII2D, "ascii2d"),
    (TRACEMOE, "trace.moe"),
    (IQDB3D, "IQDB 3D"),
    (GOOGLE_IMAGES, "Google Images"),
    (GOOGLE_LENS, "Google Lens"),
    (YANDEX, "Yandex"),
    (PAWCHIVE, "Pawchive"),
    (PAWCHIVE_INDEX, "Pawchive index"),
];

/// Engines whose similarity is a measurement of how alike two pictures are. The rest score
/// ordinally: a position in a list dressed up as a percentage, because the service returns no
/// similarity at all. Pawchive is the strongest: its lookup is by the file's SHA-256, so a hit is
/// the same bytes, a certain 100%.
pub const MEASURES_SIMILARITY: [&str; 6] =
    [IQDB, SAUCENAO, IQDB3D, TRACEMOE, PAWCHIVE, PAWCHIVE_INDEX];

/// Engines that answer from this machine and make no request, so the search pacing never waits
/// on their account.
pub const LOCAL_ENGINES: [&str; 1] = [PAWCHIVE_INDEX];

/// `label_for`: the display name for an id, or the id itself when it is not one of ours.
pub fn label_for(engine_id: &str) -> &str {
    ENGINE_LABELS
        .iter()
        .find(|(id, _)| *id == engine_id)
        .map_or(engine_id, |(_, label)| label)
}

/// `reports_real_similarity`: whether a candidate's similarity means what it says. Takes the
/// label. An unknown label counts as ordinal: a number that cannot be placed should not be able
/// to satisfy a threshold.
pub fn reports_real_similarity(engine_label: Option<&str>) -> bool {
    let wanted = pycompat::strip(engine_label.unwrap_or("")).to_lowercase();
    MEASURES_SIMILARITY
        .iter()
        .any(|id| label_for(id).to_lowercase() == wanted)
}

/// `effective_primary`: the configured primary engine, or IQDB when it is not one of the two that
/// take part in the order (a hand-edited or older config must not make the opposite-engine action
/// refuse to do anything).
pub fn effective_primary(configured: &str) -> &str {
    if PRIMARY_ENGINES.contains(&configured) {
        configured
    } else {
        IQDB
    }
}

/// `opposite_engine`: whichever of IQDB and SauceNAO did not produce `current`, a *label*
/// ("SauceNAO") that may carry stray whitespace. A row with no match gets the opposite of the
/// configured primary: the engine that has not been tried.
pub fn opposite_engine(current: Option<&str>, configured_primary: &str) -> &'static str {
    let fallback = if effective_primary(configured_primary) == IQDB {
        SAUCENAO
    } else {
        IQDB
    };
    match current {
        Some(label) if !pycompat::strip(label).is_empty() => {
            if pycompat::strip(label).to_lowercase() == IQDB {
                SAUCENAO
            } else {
                IQDB
            }
        }
        _ => fallback,
    }
}

/// `pipeline_summary`: a human-readable description of the engine pipeline, built from the same
/// rules as `_engine_waves` in the runner but described as a sentence rather than executed.
pub fn pipeline_summary(settings: &Settings) -> String {
    let primary = effective_primary(&settings.primary_engine);
    let secondary = if primary == IQDB { SAUCENAO } else { IQDB };
    let mode = match settings.secondary_engine_mode.as_str() {
        m @ ("always" | "fallback" | "disabled") => m,
        _ => "always",
    };

    // Extras, in the order `_engine_waves` builds them.
    let mut extras: Vec<&str> = Vec::new();
    for (on, id) in [
        (settings.enable_ascii2d, ASCII2D),
        (settings.enable_tracemoe, TRACEMOE),
        (settings.enable_iqdb3d, IQDB3D),
        (settings.enable_google_images, GOOGLE_IMAGES),
        (settings.enable_google_lens, GOOGLE_LENS),
        (settings.enable_yandex, YANDEX),
    ] {
        if on {
            extras.push(id);
        }
    }

    let (deferred, mut upfront): (Vec<&str>, Vec<&str>) = if settings.extras_only_as_fallback {
        (extras, Vec::new())
    } else {
        (Vec::new(), extras)
    };

    // Pawchive always runs up front.
    if settings.enable_pawchive_index {
        upfront.insert(0, PAWCHIVE_INDEX);
    }
    if settings.enable_pawchive {
        upfront.insert(0, PAWCHIVE);
    }

    let join = |ids: &[&str], sep: &str| {
        ids.iter()
            .map(|id| label_for(id))
            .collect::<Vec<_>>()
            .join(sep)
    };

    let (wave1, wave2): (Vec<&str>, Vec<&str>) = match mode {
        "always" => (
            [primary, secondary].into_iter().chain(upfront).collect(),
            deferred,
        ),
        "fallback" => (
            std::iter::once(primary).chain(upfront).collect(),
            std::iter::once(secondary).chain(deferred).collect(),
        ),
        // "disabled"
        _ => (std::iter::once(primary).chain(upfront).collect(), deferred),
    };

    let mut parts = vec![join(&wave1, " \u{2192} ")];
    if !wave2.is_empty() {
        let threshold = settings.fallback_below_similarity;
        if threshold > 0.0 {
            parts.push(format!(
                "(if best match < {threshold:.0}%) {}",
                join(&wave2, ", ")
            ));
        } else {
            parts.push(format!("(fallback) {}", join(&wave2, ", ")));
        }
    }
    parts.join(" \u{2192} ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(edit: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = defaults();
        edit(&mut s);
        s
    }

    fn defaults() -> Settings {
        // Pure path arithmetic: nothing is read from or written to this directory.
        let home: std::ffi::OsString = "/nonexistent-hatate-test-home".into();
        let paths = crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| home.clone()))
            .expect("paths");
        Settings::with_defaults(&paths)
    }

    #[test]
    fn ids_are_the_config_values() {
        // These strings are stored in config.json; spec 6.1.
        let ids: Vec<&str> = ALL_ENGINES.to_vec();
        assert_eq!(
            ids,
            [
                "iqdb",
                "saucenao",
                "ascii2d",
                "tracemoe",
                "iqdb3d",
                "googleimages",
                "googlelens",
                "yandex",
                "pawchive",
                "pawchiveindex"
            ]
        );
        // Every id has a label, in the same order.
        let labelled: Vec<&str> = ENGINE_LABELS.iter().map(|(id, _)| *id).collect();
        assert_eq!(labelled, ids);
        assert_eq!(label_for("iqdb3d"), "IQDB 3D");
        assert_eq!(label_for("something-else"), "something-else");
    }

    #[test]
    fn the_primary_id_round_trips_through_the_settings_file() {
        // `Settings.primary_engine` is the one engine id the config holds; it is written and
        // read back as the same string.
        for id in PRIMARY_ENGINES {
            let s = settings(|s| s.primary_engine = id.to_owned());
            let json = s.to_json_string();
            assert!(
                json.contains(&format!("\"primary_engine\": \"{id}\"")),
                "{id}"
            );
        }
    }

    #[test]
    fn real_similarity_is_by_label_however_it_is_written() {
        for label in [
            "IQDB",
            " iqdb ",
            "SauceNAO",
            "IQDB 3D",
            "trace.moe",
            "Pawchive",
            "Pawchive index",
        ] {
            assert!(reports_real_similarity(Some(label)), "{label}");
        }
        for label in [
            "ascii2d",
            "Google Lens",
            "Yandex",
            "Google Images",
            "",
            "nope",
        ] {
            assert!(!reports_real_similarity(Some(label)), "{label}");
        }
        assert!(!reports_real_similarity(None));
    }

    #[test]
    fn the_engine_label_is_matched_however_it_is_written() {
        // test_the_engine_label_is_matched_however_it_is_written
        for written in ["IQDB", "iqdb", "  IQDB  ", "IqDb"] {
            assert_eq!(
                opposite_engine(Some(written), IQDB),
                SAUCENAO,
                "{written:?}"
            );
        }
    }

    #[test]
    fn an_unrecognised_primary_does_not_break_the_action() {
        // test_an_unrecognised_primary_does_not_break_the_action
        assert_eq!(effective_primary("nonsense"), IQDB);
        assert_eq!(opposite_engine(None, "nonsense"), SAUCENAO);
    }

    #[test]
    fn an_unknown_engine_label_flips_to_iqdb() {
        // test_an_unknown_engine_label_flips_to_iqdb
        assert_eq!(opposite_engine(Some("ascii2d"), IQDB), IQDB);
    }

    #[test]
    fn unsearched_rows_use_the_engine_not_yet_tried() {
        // TestEngineOverrideSelection: test_unsearched_rows_use_the_engine_not_yet_tried,
        // test_fallback_follows_the_configured_primary, test_matched_rows_still_flip_per_row.
        assert_eq!(opposite_engine(None, SAUCENAO), IQDB);
        assert_eq!(opposite_engine(None, IQDB), SAUCENAO);
        assert_eq!(opposite_engine(Some("   "), IQDB), SAUCENAO);
        assert_eq!(opposite_engine(Some("SauceNAO"), SAUCENAO), IQDB);
        assert_eq!(opposite_engine(Some("IQDB"), SAUCENAO), SAUCENAO);
    }

    #[test]
    fn default_pipeline() {
        let summary = pipeline_summary(&defaults());
        assert_eq!(
            summary,
            "IQDB \u{2192} SauceNAO \u{2192} Pawchive index \u{2192} ascii2d"
        );
    }

    #[test]
    fn secondary_fallback_mode() {
        let s = settings(|s| {
            s.secondary_engine_mode = "fallback".to_owned();
            s.fallback_below_similarity = 0.0;
        });
        let summary = pipeline_summary(&s);
        assert!(summary.to_lowercase().contains("fallback"), "{summary}");
        assert_eq!(
            summary,
            "IQDB \u{2192} Pawchive index \u{2192} ascii2d \u{2192} (fallback) SauceNAO"
        );
    }

    #[test]
    fn secondary_fallback_with_threshold() {
        let s = settings(|s| {
            s.secondary_engine_mode = "fallback".to_owned();
            s.fallback_below_similarity = 70.0;
        });
        let summary = pipeline_summary(&s);
        assert!(summary.contains("70"), "{summary}");
        assert!(summary.to_lowercase().contains("best match"), "{summary}");
    }

    #[test]
    fn extras_only_as_fallback() {
        let s = settings(|s| {
            s.secondary_engine_mode = "fallback".to_owned();
            s.fallback_below_similarity = 70.0;
            s.extras_only_as_fallback = true;
            s.enable_ascii2d = true;
            s.enable_google_lens = true;
        });
        let summary = pipeline_summary(&s);
        assert!(
            summary.contains("70") && summary.contains("best match"),
            "{summary}"
        );
        assert!(
            summary.contains("ascii2d") && summary.contains("Google Lens"),
            "{summary}"
        );
        assert_eq!(
            summary,
            "IQDB \u{2192} Pawchive index \u{2192} (if best match < 70%) SauceNAO, ascii2d, Google Lens"
        );
    }

    #[test]
    fn pawchive_always_first_wave() {
        let s = settings(|s| {
            s.extras_only_as_fallback = true;
            s.enable_pawchive = true;
            s.enable_pawchive_index = true;
        });
        let summary = pipeline_summary(&s);
        let fallback = summary
            .to_lowercase()
            .find("fallback")
            .unwrap_or(summary.len());
        assert!(summary.find("Pawchive").unwrap() < fallback, "{summary}");
        assert!(
            summary.find("Pawchive index").unwrap() < fallback,
            "{summary}"
        );
        // Pawchive goes in front of the index, and both in front of the extras.
        assert_eq!(
            summary,
            "IQDB \u{2192} SauceNAO \u{2192} Pawchive \u{2192} Pawchive index \u{2192} (fallback) ascii2d"
        );
    }

    #[test]
    fn secondary_disabled() {
        let s = settings(|s| s.secondary_engine_mode = "disabled".to_owned());
        let summary = pipeline_summary(&s);
        assert!(!summary.contains("SauceNAO"), "{summary}");
        assert!(summary.contains("IQDB"), "{summary}");
    }

    #[test]
    fn saucenao_as_primary() {
        let s = settings(|s| s.primary_engine = "saucenao".to_owned());
        let summary = pipeline_summary(&s);
        assert!(summary.find("SauceNAO").unwrap() < summary.find("IQDB").unwrap());
    }

    #[test]
    fn all_extras_disabled() {
        let s = settings(|s| {
            s.enable_ascii2d = false;
            s.enable_tracemoe = false;
            s.enable_iqdb3d = false;
            s.enable_google_images = false;
            s.enable_google_lens = false;
            s.enable_yandex = false;
            s.enable_pawchive = false;
            s.enable_pawchive_index = false;
        });
        let summary = pipeline_summary(&s);
        assert_eq!(summary, "IQDB \u{2192} SauceNAO");
    }

    #[test]
    fn an_unknown_mode_reads_as_always_and_the_threshold_prints_like_python() {
        let s = settings(|s| {
            s.secondary_engine_mode = "bogus".to_owned();
            s.extras_only_as_fallback = true;
            s.fallback_below_similarity = 2.5;
        });
        // Python's `f"{2.5:.0f}"` is "2" (round half to even); so is Rust's.
        assert_eq!(
            pipeline_summary(&s),
            "IQDB \u{2192} SauceNAO \u{2192} Pawchive index \u{2192} (if best match < 2%) ascii2d"
        );
    }
}
