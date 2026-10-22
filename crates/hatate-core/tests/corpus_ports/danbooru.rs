//! `core.boorus.danbooru.*` to `hatate_core::boorus::danbooru`. A booru item's input is the
//! response body and `extra_args[0]` is the post-page URL; danbooru reads no context.

use hatate_core::boorus::danbooru;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "danbooru",
    calls: &[
        ("core.boorus.danbooru.parse", |c: &Call| {
            ok(danbooru::parse(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_dimensions", |c| {
            ok(danbooru::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_file_size", |c| {
            ok(danbooru::parse_file_size(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_file_url", |c| {
            ok(danbooru::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_preview_url", |c| {
            ok(danbooru::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_rating", |c| {
            ok(danbooru::parse_rating(c.body(), c.url()))
        }),
        ("core.boorus.danbooru.parse_restriction", |c| {
            ok(danbooru::parse_restriction(c.body(), c.url()))
        }),
    ],
};
