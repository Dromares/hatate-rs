//! `core.ascii2d.parse_results` and `core.ascii2d.search` to `hatate_core::engines::ascii2d`.
//! The parse items hold a results page (`input.html`), the page URL as `extra_args[0]` and the
//! kind as `kwargs.kind` (colour when absent). The search items are `mocked_transport` ones:
//! `input.html` is the body of the last scripted response, which is the colour page; the
//! feature-search request and the `_blocked` latch are Phase 3.

use hatate_core::engines::Response;
use hatate_core::engines::ascii2d::{self, Ascii2dError};
use serde_json::Value;

use crate::{Call, Port, ok, replay_unit};

const PARSE_UNIT: &str = "core.ascii2d.parse_results";
const SEARCH_UNIT: &str = "core.ascii2d.search";

pub const PARSE: Port = Port {
    family: "engines",
    unit: PARSE_UNIT,
    calls: &[(PARSE_UNIT, |c: &Call| {
        let page_url = c.extra_args.first().and_then(Value::as_str).unwrap_or("");
        let kind = c
            .kwargs
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("color");
        ok(ascii2d::parse_results(c.body(), page_url, kind))
    })],
};

fn search(c: &Call) -> Result<Vec<ascii2d::Ascii2dMatch>, Ascii2dError> {
    // The scripted response has no URL of its own; the colour page's hash is not needed by an
    // item that never reaches the feature search.
    let color = Response {
        status: c.response_status(),
        url: "",
        text: c.body(),
    };
    ascii2d::search_from_responses(&color, None)
}

pub const SEARCH: Port = Port {
    family: "engines",
    unit: SEARCH_UNIT,
    calls: &[(SEARCH_UNIT, |c: &Call| match search(c) {
        Ok(matches) => ok(matches),
        Err(Ascii2dError::Failed(_)) => Err("Ascii2dError::Failed"),
        Err(Ascii2dError::Blocked) => Err("Ascii2dError::Blocked"),
    })],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded message, the `_blocked` latch the client must set, and the one request made.
    #[test]
    fn the_recorded_messages_and_the_latch_are_the_ones_the_port_gives() {
        let ran = replay_unit(SEARCH_UNIT, |id, c, expected| {
            let requests = c.call["request"].as_array().unwrap();
            assert_eq!(requests.len(), 1, "{id}");
            assert_eq!(requests[0]["url"], ascii2d::SEARCH_FILE_URL, "{id}");
            let got = search(c);
            let blocked = matches!(got, Err(Ascii2dError::Blocked));
            assert_eq!(
                expected["effects"]["state_after"]["blocked"],
                Value::Bool(blocked),
                "{id}"
            );
            if let Some(message) = expected["message"].as_str() {
                assert_eq!(got.unwrap_err().to_string(), message, "{id}");
            }
        });
        assert_eq!(ran, 3);
    }
}
