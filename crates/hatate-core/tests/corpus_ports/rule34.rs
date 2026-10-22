//! `core.boorus.rule34.*` to `hatate_core::boorus::rule34`. `context.api_post` is this host's
//! Data-API post for the URL, or `null` for "the API gave nothing".

use hatate_core::boorus::rule34;

use crate::{Call, Port, api_post, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "rule34",
    calls: &[
        ("core.boorus.rule34.parse", |c: &Call| {
            ok(rule34::parse(c.body(), c.url()))
        }),
        ("core.boorus.rule34.parse_dimensions", |c| {
            ok(rule34::parse_dimensions(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.rule34.parse_file_url", |c| {
            ok(rule34::parse_file_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.rule34.parse_preview_url", |c| {
            ok(rule34::parse_preview_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.rule34.parse_rating", |c| {
            ok(rule34::parse_rating(c.body(), c.url()))
        }),
    ],
};
