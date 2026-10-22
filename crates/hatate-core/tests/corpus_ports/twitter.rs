//! `core.boorus.twitter.*` to `hatate_core::boorus::twitter`. A Twitter item's input is the
//! FxTwitter API record (or Twitter's own HTML) and `extra_args[0]` is the URL; it reads no
//! context.

use hatate_core::boorus::twitter;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "twitter",
    calls: &[
        ("core.boorus.twitter.parse", |c: &Call| {
            ok(twitter::parse(c.body(), c.url()))
        }),
        ("core.boorus.twitter.parse_dimensions", |c| {
            ok(twitter::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.twitter.parse_file_url", |c| {
            ok(twitter::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.twitter.parse_page_count", |c| {
            ok(twitter::parse_page_count(c.body(), c.url()))
        }),
        ("core.boorus.twitter.parse_pages", |c| {
            ok(twitter::parse_pages(c.body(), c.url()))
        }),
        ("core.boorus.twitter.parse_preview_url", |c| {
            ok(twitter::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
