//! `core.google_images.parse_results`, `parse_lens_results` and `parse_vision_results` to
//! `hatate_core::engines::google_images`. The two page readers take a page (`input.html`;
//! `parse_results` also the page's own URL, `extra_args[0]`); the Vision reader takes the decoded
//! payload (`input.json`). `core.google_images.search` is a `mocked_transport` item and Phase 3.

use hatate_core::engines::UnexpectedInput;
use hatate_core::engines::google_images as google;

use crate::{Call, Outcome, Port, ok};

const PARSE_RESULTS_UNIT: &str = "core.google_images.parse_results";
const PARSE_LENS_RESULTS_UNIT: &str = "core.google_images.parse_lens_results";
const PARSE_VISION_RESULTS_UNIT: &str = "core.google_images.parse_vision_results";

fn outcome<T: serde::Serialize>(result: Result<T, UnexpectedInput>) -> Outcome {
    match result {
        Ok(value) => ok(value),
        Err(_) => Err("UnexpectedInput"),
    }
}

pub const PARSE_RESULTS: Port = Port {
    family: "engines",
    unit: PARSE_RESULTS_UNIT,
    calls: &[(PARSE_RESULTS_UNIT, |c: &Call| {
        outcome(google::parse_results(c.body(), c.url()))
    })],
};

pub const PARSE_LENS_RESULTS: Port = Port {
    family: "engines",
    unit: PARSE_LENS_RESULTS_UNIT,
    calls: &[(PARSE_LENS_RESULTS_UNIT, |c: &Call| {
        ok(google::parse_lens_results(c.body()))
    })],
};

pub const PARSE_VISION_RESULTS: Port = Port {
    family: "engines",
    unit: PARSE_VISION_RESULTS_UNIT,
    calls: &[(PARSE_VISION_RESULTS_UNIT, |c: &Call| {
        outcome(google::parse_vision_results(&c.json()))
    })],
};
