//! `core.tracemoe.parse_results` to `hatate_core::engines::tracemoe`. The input is the decoded
//! JSON body (`input.json`); `kwargs.min_similarity` is the cut-off.

use hatate_core::engines::tracemoe;
use serde_json::Value;

use crate::{Call, Port, ok};

pub const UNIT: &str = "core.tracemoe.parse_results";

pub const PORT: Port = Port {
    family: "engines",
    unit: UNIT,
    calls: &[(UNIT, |c: &Call| {
        let min_similarity = c
            .kwargs
            .get("min_similarity")
            .and_then(Value::as_f64)
            .unwrap_or(tracemoe::DEFAULT_MIN_SIMILARITY);
        ok(tracemoe::parse_results(&c.json(), min_similarity))
    })],
};
