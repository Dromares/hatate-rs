//! `core.reddit._feed_entries` to `hatate_core::reddit::feed_entries`: the parse half of the call.
//! The item is a `mocked_transport` one: `input.xml` is the body of the last scripted response and
//! `call.json` `responses[-1].status` is its status. The request itself and the session cache are
//! Phase 3; the second test below holds their recorded address and cache key to the Rust helpers
//! that name them.

use hatate_core::reddit;

use crate::{Call, Port, ok};

pub const UNIT: &str = "core.reddit._feed_entries";

pub const PORT: Port = Port {
    family: "engines",
    unit: UNIT,
    calls: &[(UNIT, |c: &Call| {
        ok(reddit::feed_entries(c.response_status(), c.body()))
    })],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_items, repo_root};
    use serde_json::Value;
    use std::fs;

    fn read(dir: &str, name: &str) -> Value {
        let path = repo_root().join(dir).join(name);
        serde_json::from_slice(&fs::read(&path).expect("item file")).expect("item JSON")
    }

    #[test]
    fn the_recorded_requests_and_cache_keys_are_the_ones_the_helpers_name() {
        let items = load_items(&repo_root());
        let items: Vec<_> = items.iter().filter(|i| i.unit == UNIT).collect();
        assert_eq!(items.len(), 4, "the corpus holds four _feed_entries items");
        for item in items {
            let call = read(&item.dir, "call.json");
            let expected = read(&item.dir, "expected.json");
            let subreddit = call["args"]["subreddit"].as_str().unwrap();
            let feed = call["args"]["feed"].as_str().unwrap();

            let requests = call["request"].as_array().unwrap();
            assert_eq!(requests.len(), 1, "{}", item.id);
            assert_eq!(
                requests[0]["url"].as_str().unwrap(),
                reddit::feed_url(subreddit, feed),
                "{}",
                item.id
            );

            // A 200 answer is remembered under the key; anything else is not.
            let result = &expected["results"][UNIT];
            let remembered = result["returns"].is_array();
            let want: Vec<Value> = if remembered {
                vec![Value::from(reddit::feed_cache_key(subreddit, feed))]
            } else {
                vec![]
            };
            assert_eq!(
                result["effects"]["state_after"]["cached_keys"]
                    .as_array()
                    .unwrap(),
                &want,
                "{}",
                item.id
            );
        }
    }
}
