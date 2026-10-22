//! `core.boorus.xbooru.*` to `hatate_core::boorus::xbooru`. `context.api_post` is this host's
//! Data-API post for the URL, or `null` for "the API gave nothing".

use hatate_core::boorus::xbooru;

use crate::{Call, Port, api_post, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "xbooru",
    calls: &[
        ("core.boorus.xbooru.parse", |c: &Call| {
            ok(xbooru::parse(c.body(), c.url()))
        }),
        ("core.boorus.xbooru.parse_dimensions", |c| {
            ok(xbooru::parse_dimensions(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.xbooru.parse_file_url", |c| {
            ok(xbooru::parse_file_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.xbooru.parse_preview_url", |c| {
            ok(xbooru::parse_preview_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.xbooru.parse_rating", |c| {
            ok(xbooru::parse_rating(c.body(), c.url()))
        }),
    ],
};
