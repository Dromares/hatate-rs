//! `core.boorus.sankaku.*` to `hatate_core::boorus::sankaku`. A Sankaku item's input is the API's response body and `extra_args[0]` is the post-page URL; it reads no context.

use hatate_core::boorus::sankaku;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "sankaku",
    calls: &[
        ("core.boorus.sankaku.incomplete_reason", |c: &Call| {
            ok(sankaku::incomplete_reason(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse", |c| {
            ok(sankaku::parse(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_canonical_url", |c| {
            ok(sankaku::parse_canonical_url(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_dimensions", |c| {
            ok(sankaku::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_file_info", |c| {
            ok(sankaku::parse_file_info(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_file_url", |c| {
            ok(sankaku::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_preview_url", |c| {
            ok(sankaku::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.sankaku.parse_rating", |c| {
            ok(sankaku::parse_rating(c.body(), c.url()))
        }),
    ],
};
