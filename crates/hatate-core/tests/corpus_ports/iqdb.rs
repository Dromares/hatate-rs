//! `core.iqdb._parse_results`, `core.iqdb.search` and `core.iqdb.search_3d` to
//! `hatate_core::engines::iqdb`. The parse items hold IQDB's results page (`input.html`) and the
//! instance as `kwargs.base_url`. The two search items are `mocked_transport` ones: `input.html`
//! is the body of the last scripted response and `responses[-1].status` its status. The upload is
//! Phase 3; `search_3d` is `search` against the 3D instance.

use hatate_core::engines::iqdb::{self, IQDB_3D_URL, IQDB_URL, IqdbError};
use serde_json::Value;

use crate::{Call, Outcome, Port, ok, replay_unit};

const PARSE_UNIT: &str = "core.iqdb._parse_results";
const SEARCH_UNIT: &str = "core.iqdb.search";
const SEARCH_3D_UNIT: &str = "core.iqdb.search_3d";

fn outcome(result: Result<Vec<iqdb::IqdbMatch>, IqdbError>) -> Outcome {
    match result {
        Ok(matches) => ok(matches),
        Err(_) => Err("IqdbError"),
    }
}

fn parse(c: &Call) -> Outcome {
    let base_url = c
        .kwargs
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or(IQDB_URL);
    outcome(iqdb::parse_results(c.body(), base_url))
}

pub const PARSE: Port = Port {
    family: "engines",
    unit: PARSE_UNIT,
    calls: &[(PARSE_UNIT, parse)],
};

pub const SEARCH: Port = Port {
    family: "engines",
    unit: SEARCH_UNIT,
    calls: &[(SEARCH_UNIT, |c: &Call| {
        outcome(iqdb::search_from_response(
            IQDB_URL,
            c.response_status(),
            c.body(),
        ))
    })],
};

pub const SEARCH_3D: Port = Port {
    family: "engines",
    unit: SEARCH_3D_UNIT,
    calls: &[(SEARCH_3D_UNIT, |c: &Call| {
        outcome(iqdb::search_from_response(
            IQDB_3D_URL,
            c.response_status(),
            c.body(),
        ))
    })],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A `mocked_transport` item records the message the error carried; the runner compares only
    /// the class. This holds the messages (they are user-facing text) to the recorded ones, and
    /// the recorded request to the instance address.
    #[test]
    fn the_recorded_messages_and_addresses_are_the_ones_the_port_gives() {
        for (unit, base_url) in [(SEARCH_UNIT, IQDB_URL), (SEARCH_3D_UNIT, IQDB_3D_URL)] {
            let ran = replay_unit(unit, |id, c, expected| {
                assert_eq!(c.call["request"][0]["url"], base_url, "{id}");
                assert_eq!(c.call["request"].as_array().unwrap().len(), 1, "{id}");
                let got = iqdb::search_from_response(base_url, c.response_status(), c.body());
                if let Some(message) = expected["message"].as_str() {
                    assert_eq!(got.unwrap_err().to_string(), message, "{id}");
                } else {
                    assert!(got.is_ok(), "{id}");
                }
            });
            assert!(ran > 0, "{unit} has no corpus item");
        }
    }
}
