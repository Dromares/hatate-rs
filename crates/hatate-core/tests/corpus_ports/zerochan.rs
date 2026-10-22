//! `core.boorus.zerochan.*` to `hatate_core::boorus::zerochan`. A booru item's input is the API response and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::zerochan;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "zerochan",
    calls: &[
        ("core.boorus.zerochan.parse", |c: &Call| {
            ok(zerochan::parse(c.body(), c.url()))
        }),
        ("core.boorus.zerochan.parse_dimensions", |c| {
            ok(zerochan::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.zerochan.parse_file_url", |c| {
            ok(zerochan::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.zerochan.parse_preview_url", |c| {
            ok(zerochan::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
