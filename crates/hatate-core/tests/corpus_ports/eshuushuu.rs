//! `core.boorus.eshuushuu.*` to `hatate_core::boorus::eshuushuu`. A booru item's input is the page and `extra_args[0]` is the post-page URL; the site reads no context.

use hatate_core::boorus::eshuushuu;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "eshuushuu",
    calls: &[
        ("core.boorus.eshuushuu.parse", |c: &Call| {
            ok(eshuushuu::parse(c.body(), c.url()))
        }),
        ("core.boorus.eshuushuu.parse_dimensions", |c| {
            ok(eshuushuu::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.eshuushuu.parse_file_url", |c| {
            ok(eshuushuu::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.eshuushuu.parse_preview_url", |c| {
            ok(eshuushuu::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
