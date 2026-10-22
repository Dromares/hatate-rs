//! `core.boorus.paheal.*` to `hatate_core::boorus::paheal`. A booru item's input is the response body and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::paheal;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "paheal",
    calls: &[
        ("core.boorus.paheal.parse", |c: &Call| {
            ok(paheal::parse(c.body(), c.url()))
        }),
        ("core.boorus.paheal.parse_dimensions", |c| {
            ok(paheal::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.paheal.parse_file_info", |c| {
            ok(paheal::parse_file_info(c.body(), c.url()))
        }),
        ("core.boorus.paheal.parse_file_url", |c| {
            ok(paheal::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.paheal.parse_preview_url", |c| {
            ok(paheal::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
