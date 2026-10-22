//! `core.google_images.parse_results`, `parse_lens_results` and `parse_vision_results` to
//! `hatate_core::engines::google_images`. The two page readers take a page (`input.html`;
//! `parse_results` also the page's own URL, `extra_args[0]`); the Vision reader takes the decoded
//! payload (`input.json`). `core.google_images.search` is a `mocked_transport` item, ported below
//! as the two halves of `search` that read an answer.

use hatate_core::engines::google_images as google;
use hatate_core::engines::{Response, UnexpectedInput};

use serde_json::Value;

use crate::{Call, Outcome, Port, ok, replay_unit};

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

// ---- core.google_images.search ----------------------------------------------------------------
//
// A `mocked_transport` item. `args.api_key` picks the route (`vision_key`): a key sends the
// picture to Cloud Vision, whose answer is `input.json`; without one it is uploaded to the public
// endpoint, whose final answer (status, URL after redirects, page) is the last scripted response.
// The requests, the picture and the headers are Phase 3.

const SEARCH_UNIT: &str = "core.google_images.search";

fn search(c: &Call) -> Result<Vec<google::GoogleImagesMatch>, google::GoogleImagesError> {
    let api_key = c.call["args"]["api_key"].as_str().unwrap_or("");
    if google::vision_key(api_key).is_some() {
        google::vision_from_response(c.response_status(), Some(&c.json()))
    } else {
        let last = c.call["responses"].as_array().and_then(|r| r.last());
        google::public_from_response(&Response {
            status: c.response_status(),
            url: last.and_then(|r| r["url"].as_str()).unwrap_or(""),
            text: c.body(),
        })
    }
}

pub const SEARCH: Port = Port {
    family: "engines",
    unit: SEARCH_UNIT,
    calls: &[(SEARCH_UNIT, |c: &Call| match search(c) {
        Ok(matches) => ok(matches),
        Err(google::GoogleImagesError::Failed(_)) => Err("GoogleImagesError::Failed"),
        Err(google::GoogleImagesError::Blocked(_)) => Err("GoogleImagesError::Blocked"),
        Err(google::GoogleImagesError::JavaScript) => Err("GoogleImagesError::JavaScript"),
        Err(google::GoogleImagesError::Unexpected(_)) => Err("UnexpectedInput"),
    })],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A `mocked_transport` item records the message the error carried and whether the call left
    /// the engine latched; the runner compares neither. This holds both to the recorded ones, and
    /// the recorded request to the route the key picks.
    #[test]
    fn the_recorded_messages_latch_and_requests_are_the_ones_the_port_gives() {
        let ran = replay_unit(SEARCH_UNIT, |id, c, expected| {
            let result = search(c);
            let latched = result
                .as_ref()
                .is_err_and(google::GoogleImagesError::latches);
            assert_eq!(
                expected["effects"]["state_after"]["blocked"],
                Value::Bool(latched),
                "{id}"
            );
            assert_eq!(expected["effects"]["sleeps"], serde_json::json!([]), "{id}");
            match (expected["message"].as_str(), result) {
                (Some(message), Err(error)) => assert_eq!(error.to_string(), message, "{id}"),
                (None, Ok(_)) => {}
                (message, result) => panic!("{id}: recorded {message:?}, got {result:?}"),
            }

            let requests = c.call["request"].as_array().expect("request");
            assert_eq!(requests.len(), 1, "{id}");
            let api_key = c.call["args"]["api_key"].as_str().unwrap_or("");
            if google::vision_key(api_key).is_some() {
                assert_eq!(requests[0]["url"], google::VISION_URL, "{id}");
                assert_eq!(requests[0]["params"]["key"], api_key, "{id}");
                assert_eq!(requests[0]["json_body"], Value::Bool(true), "{id}");
            } else {
                assert_eq!(requests[0]["url"], google::SEARCH_BY_IMAGE_URL, "{id}");
                assert_eq!(requests[0]["params"]["hl"], "en", "{id}");
                assert_eq!(requests[0]["allow_redirects"], Value::Bool(true), "{id}");
            }
        });
        assert_eq!(ran, 10);
    }
}
