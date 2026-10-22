//! `core.boorus.mangadex.*` to `hatate_core::boorus::mangadex`. A booru item's input is the at-home server response and `extra_args[0]` is the chapter URL; the site reads no context.

use hatate_core::boorus::mangadex;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "mangadex",
    calls: &[
        ("core.boorus.mangadex.parse", |c: &Call| {
            ok(mangadex::parse(c.body(), c.url()))
        }),
        ("core.boorus.mangadex.parse_file_url", |c| {
            ok(mangadex::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.mangadex.parse_page_count", |c| {
            ok(mangadex::parse_page_count(c.body(), c.url()))
        }),
        ("core.boorus.mangadex.parse_pages", |c| {
            ok(mangadex::parse_pages(c.body(), c.url()))
        }),
        ("core.boorus.mangadex.parse_preview_url", |c| {
            ok(mangadex::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
