//! `core.boorus.rule34us.*` to `hatate_core::boorus::rule34us`. A booru item's input is the response body and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::rule34us;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "rule34us",
    calls: &[
        ("core.boorus.rule34us.parse", |c: &Call| {
            ok(rule34us::parse(c.body(), c.url()))
        }),
        ("core.boorus.rule34us.parse_dimensions", |c| {
            ok(rule34us::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.rule34us.parse_file_url", |c| {
            ok(rule34us::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.rule34us.parse_preview_url", |c| {
            ok(rule34us::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
