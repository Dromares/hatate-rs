//! How a similarity number says whether it is actually a measurement
//! (`core/similarity_display.py`): the one place that decides how the `similarity_measured` flag
//! is said, so the table, the preview, the dropdown, the export and the Hydrus note agree.
//!
//! Only the label and the tooltip are here (what `provenance_note` and the exports read); the
//! chip key the table paints with belongs to the GUI.

/// Prefixed, not suffixed, so it survives a narrow column eliding the `%` end.
pub const ESTIMATE_MARK: &str = "~";

pub const MEASURED_NOTE: &str = "measured by comparing this file with the match";
pub const UNMEASURED_NOTE: &str = "the engine's ranking of its own results, not a measurement - ascii2d and the Google engines report no similarity at all";

/// `f"{value:.0f}"`: Python rounds the exact binary value half to even and writes `nan`/`inf`
/// in lower case.
fn fixed0(value: f64) -> String {
    if value.is_nan() {
        "nan".to_owned()
    } else if value.is_infinite() {
        if value < 0.0 { "-inf" } else { "inf" }.to_owned()
    } else {
        format!("{value:.0}")
    }
}

/// `similarity_label`: `"92%"` for a real score, `"~92%"` for a ranking, `""` for no match.
pub fn similarity_label(similarity: Option<f64>, measured: bool) -> String {
    match similarity {
        None => String::new(),
        Some(s) => format!(
            "{}{}%",
            if measured { "" } else { ESTIMATE_MARK },
            fixed0(s)
        ),
    }
}

/// `similarity_tooltip`: the long form, for wherever there is room to explain.
pub fn similarity_tooltip(similarity: Option<f64>, measured: bool) -> String {
    match similarity {
        None => String::new(),
        Some(s) => format!(
            "{}% - {}",
            fixed0(s),
            if measured {
                MEASURED_NOTE
            } else {
                UNMEASURED_NOTE
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_to_even_like_python() {
        for (v, want) in [
            (0.5, "0"),
            (1.5, "2"),
            (2.5, "2"),
            (3.5, "4"),
            (84.5, "84"),
            (85.5, "86"),
        ] {
            assert_eq!(fixed0(v), want, "{v}");
        }
    }

    #[test]
    fn non_finite_and_negative_zero() {
        assert_eq!(similarity_label(Some(f64::NAN), true), "nan%");
        assert_eq!(similarity_label(Some(f64::INFINITY), false), "~inf%");
        assert_eq!(similarity_label(Some(-0.0), true), "-0%");
    }

    #[test]
    fn marker_goes_on_the_unmeasured_number() {
        assert_eq!(similarity_label(Some(92.0), true), "92%");
        assert_eq!(similarity_label(Some(92.0), false), "~92%");
        assert_eq!(similarity_label(None, true), "");
        assert!(similarity_tooltip(Some(80.0), false).starts_with("80% - the engine's ranking"));
    }
}
