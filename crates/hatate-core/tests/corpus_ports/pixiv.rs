//! `core.boorus.pixiv.*` to `hatate_core::boorus::pixiv`. A Pixiv item's input is the AJAX
//! response (or the legacy page) and `extra_args[0]` is the URL; it reads no context.

use hatate_core::boorus::pixiv;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "pixiv",
    calls: &[
        ("core.boorus.pixiv.parse", |c: &Call| {
            ok(pixiv::parse(c.body(), c.url()))
        }),
        ("core.boorus.pixiv.parse_dimensions", |c| {
            ok(pixiv::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.pixiv.parse_file_url", |c| {
            ok(pixiv::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.pixiv.parse_page_count", |c| {
            ok(pixiv::parse_page_count(c.body(), c.url()))
        }),
        ("core.boorus.pixiv.parse_pages", |c| {
            ok(pixiv::parse_pages(c.body(), c.url()))
        }),
        ("core.boorus.pixiv.parse_preview_url", |c| {
            ok(pixiv::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
