//! `core.boorus.pawchive.*` to `hatate_core::boorus::pawchive`. A booru item's input is the post's
//! API record (or the account-gated page) and `extra_args[0]` is the post-page URL;
//! `context.artist_cache` is the creator names already looked up, keyed `"{service}/{user}"`.
//!
//! The runner never runs a callable listed under `skipped_network_callables`: Python reached the
//! network on those inputs (a creator not in the cache, or a ranged read of the original), and
//! the Rust parser asks the caller for both (`artist_profile_request`, `dimensions_request`).
//! `parse_dimensions` is mapped for the items that ran it, where the post holds no original and
//! no read is needed.

use hatate_core::boorus::{Context, pawchive};
use serde_json::Value;

use crate::{Call, Port, ok};

/// The `artist_cache` of an item's `context`.
fn context(call: &Call) -> Context {
    let cache = call
        .context
        .get("artist_cache")
        .and_then(Value::as_object)
        .map(|names| {
            names
                .iter()
                .map(|(key, name)| {
                    let name = name.as_str().expect("artist_cache values are strings");
                    (key.clone(), name.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    Context::with_artist_cache(cache)
}

pub const PORT: Port = Port {
    family: "boorus",
    unit: "pawchive",
    calls: &[
        ("core.boorus.pawchive.parse", |c: &Call| {
            ok(pawchive::parse(c.body(), c.url(), &context(c)))
        }),
        ("core.boorus.pawchive.parse_dimensions", |c| {
            ok(pawchive::parse_dimensions(c.body(), c.url(), &context(c)))
        }),
        ("core.boorus.pawchive.parse_file_url", |c| {
            ok(pawchive::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.pawchive.parse_page_count", |c| {
            ok(pawchive::parse_page_count(c.body(), c.url()))
        }),
        ("core.boorus.pawchive.parse_pages", |c| {
            ok(pawchive::parse_pages(c.body(), c.url()))
        }),
        ("core.boorus.pawchive.parse_preview_url", |c| {
            ok(pawchive::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
