//! `core.saucenao._tags_from_result` and `core.saucenao.search` to
//! `hatate_core::engines::saucenao`. A tag item's input is one result's `data` (`input.json`) and
//! `extra_args[0]` the settings. The search items are `mocked_transport` ones: `args.settings` is
//! the settings the test passed, `input.json` the body of the last scripted response and
//! `responses[-1].status` its status. That is the answer to the retry when the first answer was
//! a 429: the wait and the second request are Phase 3, and the second test below holds the
//! recorded quota and daily-limit latch to what the parse hands the client.

use hatate_core::config::SauceNaoSettings;
use hatate_core::engines::saucenao::{self, SauceNaoError};
use serde_json::{Map, Value};

use crate::{Call, Outcome, Port, ok, replay_unit};

const TAGS_UNIT: &str = "core.saucenao._tags_from_result";
const SEARCH_UNIT: &str = "core.saucenao.search";

/// The `SauceNaoSettings` a `call.json` records, as `{"__dataclass__": ..., <field>: <value>}`.
fn settings_of(recorded: &Value) -> SauceNaoSettings {
    let mut settings = SauceNaoSettings::default();
    let fields = recorded
        .as_object()
        .expect("settings is a dataclass object");
    for (key, value) in fields {
        match key.as_str() {
            "__dataclass__" => assert_eq!(value, "SauceNaoSettings"),
            "api_key" => settings.api_key = value.as_str().expect("api_key").into(),
            "use_json_api" => settings.use_json_api = value.as_bool().expect("use_json_api"),
            "min_similarity" => settings.min_similarity = value.as_f64().expect("min_similarity"),
            "tag_types" => {
                settings.tag_types = value
                    .as_array()
                    .expect("tag_types")
                    .iter()
                    .map(|t| t.as_str().expect("tag type").to_owned())
                    .collect();
            }
            "namespace" => settings.namespace = value.as_str().expect("namespace").to_owned(),
            "dbmask" => settings.dbmask = value.as_i64().expect("dbmask"),
            "pause_search_on_quota_exhausted" => {
                settings.pause_search_on_quota_exhausted = value.as_bool().expect("pause");
            }
            other => panic!("SauceNaoSettings has no field {other:?} the port knows"),
        }
    }
    settings
}

pub const TAGS: Port = Port {
    family: "engines",
    unit: TAGS_UNIT,
    calls: &[(TAGS_UNIT, |c: &Call| {
        let settings = settings_of(&c.extra_args[0]);
        let empty = Map::new();
        let data = c.json();
        ok(saucenao::tags_from_result(
            data.as_object().unwrap_or(&empty),
            &settings,
        ))
    })],
};

fn search(c: &Call) -> saucenao::JsonAnswer {
    let settings = settings_of(&c.call["args"]["settings"]);
    assert!(
        saucenao::uses_json_api(&settings),
        "every recorded search item uses the JSON API"
    );
    saucenao::parse_json_response(&settings, c.response_status(), c.body())
}

pub const SEARCH: Port = Port {
    family: "engines",
    unit: SEARCH_UNIT,
    calls: &[(SEARCH_UNIT, |c: &Call| -> Outcome {
        match search(c).result {
            Ok(matches) => ok(matches),
            Err(SauceNaoError(_)) => Err("SauceNaoError"),
        }
    })],
};

#[cfg(test)]
mod tests {
    use super::*;

    /// What the runner does not compare: the error text, the quota reading and the daily-limit
    /// latch, each against the `effects` of the recorded call, and the request addresses.
    #[test]
    fn the_recorded_messages_quota_and_latch_are_the_ones_the_port_gives() {
        let ran = replay_unit(SEARCH_UNIT, |id, c, expected| {
            for request in c.call["request"].as_array().unwrap() {
                assert_eq!(request["url"], saucenao::API_URL, "{id}");
            }
            let answer = search(c);
            let state = &expected["effects"]["state_after"];
            assert_eq!(
                state["daily_limit_reported"], answer.daily_limit_reported,
                "{id}"
            );
            // `last_quota` after a fresh start: null until a header was read.
            let quota = answer
                .quota
                .map_or(Value::Null, |q| serde_json::to_value(q).unwrap());
            assert_eq!(state["last_quota"], quota, "{id}");
            // One wait of the burst window per 429 the client retried.
            let retried = c.call["responses"].as_array().unwrap().len() - 1;
            assert_eq!(
                expected["effects"]["sleeps"].as_array().unwrap().len(),
                retried,
                "{id}"
            );
            for sleep in expected["effects"]["sleeps"].as_array().unwrap() {
                assert_eq!(sleep.as_f64(), Some(saucenao::SHORT_WINDOW_SECONDS), "{id}");
            }
            if let Some(message) = expected["message"].as_str() {
                assert_eq!(answer.result.unwrap_err().to_string(), message, "{id}");
            } else {
                assert!(answer.result.is_ok(), "{id}");
            }
        });
        assert_eq!(ran, 14);
    }
}
