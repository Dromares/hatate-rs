//! `core.boorus.e621.*` to `hatate_core::boorus::e621`. A booru item's input is the response body and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::e621;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "e621",
    calls: &[
        ("core.boorus.e621.incomplete_reason", |c: &Call| {
            ok(e621::incomplete_reason(c.body(), c.url()))
        }),
        ("core.boorus.e621.parse", |c| {
            ok(e621::parse(c.body(), c.url()))
        }),
        ("core.boorus.e621.parse_dimensions", |c| {
            ok(e621::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.e621.parse_file_url", |c| {
            ok(e621::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.e621.parse_preview_url", |c| {
            ok(e621::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.e621.parse_rating", |c| {
            ok(e621::parse_rating(c.body(), c.url()))
        }),
    ],
};
