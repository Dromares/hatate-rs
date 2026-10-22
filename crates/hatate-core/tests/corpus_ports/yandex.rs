//! `core.yandex.parse_sites` and `core.yandex.keep_post_pages` to `hatate_core::engines::yandex`.
//! `parse_sites` reads a results page (`input.html`); `keep_post_pages` is handed the list it
//! returned (`input.json`). `core.yandex.search` is a `mocked_transport` item and Phase 3.

use hatate_core::engines::yandex;
use serde_json::Value;

use crate::{Call, Outcome, Port, ok};

const PARSE_SITES_UNIT: &str = "core.yandex.parse_sites";
const KEEP_POST_PAGES_UNIT: &str = "core.yandex.keep_post_pages";

fn outcome<T: serde::Serialize>(
    result: Result<T, hatate_core::engines::UnexpectedInput>,
) -> Outcome {
    match result {
        Ok(value) => ok(value),
        Err(_) => Err("UnexpectedInput"),
    }
}

pub const PARSE_SITES: Port = Port {
    family: "engines",
    unit: PARSE_SITES_UNIT,
    calls: &[(PARSE_SITES_UNIT, |c: &Call| {
        outcome(yandex::parse_sites(c.body()))
    })],
};

pub const KEEP_POST_PAGES: Port = Port {
    family: "engines",
    unit: KEEP_POST_PAGES_UNIT,
    calls: &[(KEEP_POST_PAGES_UNIT, |c: &Call| {
        let Value::Array(sites) = c.json() else {
            panic!("input.json is not a list of sites");
        };
        outcome(yandex::keep_post_pages(&sites))
    })],
};
