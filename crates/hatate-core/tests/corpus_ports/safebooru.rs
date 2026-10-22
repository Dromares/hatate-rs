//! `core.boorus.safebooru.*` to `hatate_core::boorus::safebooru`. Only `parse_file_url` reads the
//! Data-API post (it delegates to Gelbooru's API-first hook); the rest are page-only.

use hatate_core::boorus::safebooru;

use crate::{Call, Port, api_post, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "safebooru",
    calls: &[
        ("core.boorus.safebooru.parse", |c: &Call| {
            ok(safebooru::parse(c.body(), c.url()))
        }),
        ("core.boorus.safebooru.parse_dimensions", |c| {
            ok(safebooru::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.safebooru.parse_file_url", |c| {
            ok(safebooru::parse_file_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.safebooru.parse_preview_url", |c| {
            ok(safebooru::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.safebooru.parse_rating", |c| {
            ok(safebooru::parse_rating(c.body(), c.url()))
        }),
    ],
};
