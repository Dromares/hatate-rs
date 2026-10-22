//! `core.boorus.joyreactor.*` to `hatate_core::boorus::joyreactor`. A JoyReactor item's input is the post page and `extra_args[0]` is the post-page URL; it reads no context.

use hatate_core::boorus::joyreactor;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "joyreactor",
    calls: &[
        ("core.boorus.joyreactor.parse", |c: &Call| {
            ok(joyreactor::parse(c.body(), c.url()))
        }),
        ("core.boorus.joyreactor.parse_file_url", |c| {
            ok(joyreactor::parse_file_url(c.body(), c.url()))
        }),
    ],
};
