//! `core.boorus.deviantart.*` to `hatate_core::boorus::deviantart`. A booru item's input is the
//! deviation page and `extra_args[0]` is its URL; the site reads no context.

use hatate_core::boorus::deviantart;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "deviantart",
    calls: &[
        ("core.boorus.deviantart.parse", |c: &Call| {
            ok(deviantart::parse(c.body(), c.url()))
        }),
        ("core.boorus.deviantart.parse_dimensions", |c| {
            ok(deviantart::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.deviantart.parse_file_info", |c| {
            ok(deviantart::parse_file_info(c.body(), c.url()))
        }),
        ("core.boorus.deviantart.parse_file_url", |c| {
            ok(deviantart::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.deviantart.parse_preview_url", |c| {
            ok(deviantart::parse_preview_url(c.body(), c.url()))
        }),
        ("core.boorus.deviantart.parse_restriction", |c| {
            ok(deviantart::parse_restriction(c.body(), c.url()))
        }),
    ],
};
