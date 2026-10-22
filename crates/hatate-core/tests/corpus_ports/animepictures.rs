//! `core.boorus.animepictures.*` to `hatate_core::boorus::animepictures`. An Anime-Pictures item's input is the v3 API's response body and `extra_args[0]` is the post-page URL; it reads no context.

use hatate_core::boorus::animepictures;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "animepictures",
    calls: &[
        ("core.boorus.animepictures.parse", |c: &Call| {
            ok(animepictures::parse(c.body(), c.url()))
        }),
        ("core.boorus.animepictures.parse_dimensions", |c| {
            ok(animepictures::parse_dimensions(c.body(), c.url()))
        }),
        ("core.boorus.animepictures.parse_file_info", |c| {
            ok(animepictures::parse_file_info(c.body(), c.url()))
        }),
        ("core.boorus.animepictures.parse_file_url", |c| {
            ok(animepictures::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.animepictures.parse_preview_url", |c| {
            ok(animepictures::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
