//! `core.pawchive_lookup.parse_response` and `core.pawchive_lookup.lookup` to
//! `hatate_core::engines::pawchive_lookup`. The input is the decoded JSON body (`input.json`) and
//! `extra_args[0]` the SHA-256 for `parse_response`. The `lookup` items are `mocked_transport`
//! ones: `args.sha256` is the hash asked for and `responses[-1]` the answer. Hashing the file and
//! the GET are Phase 3. This is not the booru parser `boorus::pawchive`.

use hatate_core::engines::pawchive_lookup::{self, PawchiveLookupError};
use serde_json::Value;

use crate::{Call, Outcome, Port, ok, replay_unit};

const PARSE_UNIT: &str = "core.pawchive_lookup.parse_response";
const LOOKUP_UNIT: &str = "core.pawchive_lookup.lookup";

pub const PARSE: Port = Port {
    family: "engines",
    unit: PARSE_UNIT,
    calls: &[(PARSE_UNIT, |c: &Call| {
        let sha256 = c.extra_args.first().and_then(Value::as_str).unwrap_or("");
        ok(pawchive_lookup::parse_response(&c.json(), sha256))
    })],
};

fn lookup(c: &Call) -> Result<Vec<pawchive_lookup::PawchiveMatch>, PawchiveLookupError> {
    let sha256 =
        pawchive_lookup::normalise_sha256(c.call["args"]["sha256"].as_str().unwrap_or(""))?;
    pawchive_lookup::lookup_from_response(c.response_status(), c.body(), &sha256)
}

fn outcome(result: Result<Vec<pawchive_lookup::PawchiveMatch>, PawchiveLookupError>) -> Outcome {
    match result {
        Ok(matches) => ok(matches),
        Err(_) => Err("PawchiveLookupError"),
    }
}

pub const LOOKUP: Port = Port {
    family: "engines",
    unit: LOOKUP_UNIT,
    calls: &[(LOOKUP_UNIT, |c: &Call| outcome(lookup(c)))],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded message and the one request the lookup makes, for the hash it was given.
    #[test]
    fn the_recorded_messages_and_addresses_are_the_ones_the_port_gives() {
        let ran = replay_unit(LOOKUP_UNIT, |id, c, expected| {
            let sha256 = c.call["args"]["sha256"].as_str().unwrap();
            let requests = c.call["request"].as_array().unwrap();
            assert_eq!(requests.len(), 1, "{id}");
            assert_eq!(
                requests[0]["url"],
                pawchive_lookup::request_url(sha256),
                "{id}"
            );
            let got = lookup(c);
            if let Some(message) = expected["message"].as_str() {
                assert_eq!(got.unwrap_err().to_string(), message, "{id}");
            } else {
                assert!(got.is_ok(), "{id}");
            }
        });
        assert_eq!(ran, 5);
    }
}
