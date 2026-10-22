//! `core.boorus.moebooru.*` to `hatate_core::boorus::moebooru`. A booru item's input is the response body and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::moebooru;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "moebooru",
    calls: &[
        ("core.boorus.moebooru.parse", |c: &Call| {
            ok(moebooru::parse(c.body(), c.url()))
        }),
        ("core.boorus.moebooru.parse_dimensions", |c| {
            ok(moebooru::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.moebooru.parse_file_url", |c| {
            ok(moebooru::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.moebooru.parse_preview_url", |c| {
            ok(moebooru::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.moebooru.parse_rating", |c| {
            ok(moebooru::parse_rating(c.body(), c.url()))
        }),
    ],
};
