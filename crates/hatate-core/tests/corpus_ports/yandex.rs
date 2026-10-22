//! `core.yandex.parse_sites` and `core.yandex.keep_post_pages` to `hatate_core::engines::yandex`.
//! `parse_sites` reads a results page (`input.html`); `keep_post_pages` is handed the list it
//! returned (`input.json`). `core.yandex.search` is a `mocked_transport` item, ported below as the
//! two halves of `search` that read an answer.

use hatate_core::engines::{Response, yandex};
use serde_json::Value;

use crate::{Call, Outcome, Port, ok, replay_unit};

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

// ---- core.yandex.search ---------------------------------------------------------------------
//
// A `mocked_transport` item. `input.html` is the body of the last scripted response. An item whose
// upload was answered with a search id has two responses (the upload's JSON, then the "sites"
// page); one whose upload was a captcha, or carried no id, has only the upload. The two requests
// themselves, the picture and the headers are Phase 3.

const SEARCH_UNIT: &str = "core.yandex.search";

/// The clock Python's message read, as the corpus writes it (`README.md`, "Wall-clock text").
const CLOCK: yandex::RestClock<'static> = yandex::RestClock {
    now: 1000.0,
    rest_ends_at: "<HH:MM>",
};

/// What a replay of the item leaves behind: the result and the state after it.
fn search(
    c: &Call,
) -> (
    Result<Vec<yandex::YandexMatch>, yandex::YandexError>,
    yandex::YandexState,
) {
    let responses = c.call["responses"].as_array().expect("responses");
    let mut state = yandex::YandexState::default();
    let at = |i: usize| &responses[i];
    let upload_is_last = responses.len() == 1;
    let upload_text = if upload_is_last {
        c.body()
    } else {
        at(0)["text"].as_str().expect("upload text")
    };
    let upload = Response {
        status: u16::try_from(at(0)["status"].as_u64().expect("status")).expect("status"),
        url: at(0)["url"].as_str().unwrap_or(""),
        text: upload_text,
    };
    // `json_unset`: the mock gave `.json()` nothing to decode, so the replay must make it fail.
    let upload_json = at(0).get("json");
    let result =
        yandex::upload_from_response(&mut state, CLOCK, &upload, upload_json).and_then(|_| {
            let last = responses.last().expect("responses");
            let page = Response {
                status: c.response_status(),
                url: last["url"].as_str().unwrap_or(""),
                text: c.body(),
            };
            yandex::results_from_response(&mut state, CLOCK, &page)
        });
    (result, state)
}

pub const SEARCH: Port = Port {
    family: "engines",
    unit: SEARCH_UNIT,
    calls: &[(SEARCH_UNIT, |c: &Call| match search(c).0 {
        Ok(matches) => ok(matches),
        Err(yandex::YandexError::Failed(_)) => Err("YandexError::Failed"),
        Err(yandex::YandexError::Blocked(_)) => Err("YandexError::Blocked"),
        Err(yandex::YandexError::Unexpected(_)) => Err("UnexpectedInput"),
    })],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A `mocked_transport` item records the message the error carried and the module state the
    /// call left behind; the runner compares neither. This holds both to the recorded ones, and
    /// the recorded requests to what the two halves are the answers to.
    #[test]
    fn the_recorded_messages_state_and_requests_are_the_ones_the_port_gives() {
        let ran = replay_unit(SEARCH_UNIT, |id, c, expected| {
            let (result, state) = search(c);
            let after = &expected["effects"]["state_after"];
            assert_eq!(after["blocked"], state.blocked, "{id}");
            assert_eq!(
                after["consecutive_failures"], state.consecutive_failures,
                "{id}"
            );
            assert_eq!(after["resting"], state.is_resting(CLOCK.now), "{id}");
            assert_eq!(expected["effects"]["sleeps"], serde_json::json!([]), "{id}");
            match (expected["message"].as_str(), result) {
                (Some(message), Err(error)) => assert_eq!(error.to_string(), message, "{id}"),
                (None, Ok(_)) => {}
                (message, result) => panic!("{id}: recorded {message:?}, got {result:?}"),
            }

            let requests = c.call["request"].as_array().expect("request");
            assert_eq!(
                requests.len(),
                c.call["responses"].as_array().unwrap().len(),
                "{id}"
            );
            assert!(
                requests.iter().all(|r| r["url"] == yandex::SEARCH_URL),
                "{id}"
            );
            assert_eq!(
                requests[0]["params"]["request"],
                yandex::UPLOAD_REQUEST,
                "{id}"
            );
            if let Some(sites) = requests.get(1) {
                let cbir_id = c.call["responses"][0]["json"]["blocks"][0]["params"]["cbirId"]
                    .as_str()
                    .unwrap();
                assert_eq!(sites["params"]["cbir_id"], cbir_id, "{id}");
                assert_eq!(
                    sites["params"]["url"],
                    format!("https://avatars.mds.yandex.net/get-images-cbir/{cbir_id}/orig"),
                    "{id}"
                );
            }
        });
        assert_eq!(ran, 5);
    }
}
