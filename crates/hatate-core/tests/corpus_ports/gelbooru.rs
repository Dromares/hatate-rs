//! `core.boorus.gelbooru.*` to `hatate_core::boorus::gelbooru`. A booru item's input is the page
//! and `extra_args[0]` is the post-page URL; `context.api_post` is the Data-API post, or `null`
//! for "the API gave nothing".

use hatate_core::boorus::gelbooru;

use crate::{Call, Port, api_post, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "gelbooru",
    calls: &[
        ("core.boorus.gelbooru.parse", |c: &Call| {
            ok(gelbooru::parse(c.body(), c.url()))
        }),
        ("core.boorus.gelbooru.parse_dimensions", |c| {
            ok(gelbooru::parse_dimensions(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.gelbooru.parse_dimensions_from_html", |c| {
            ok(gelbooru::parse_dimensions_from_html(c.body(), c.url()))
        }),
        ("core.boorus.gelbooru.parse_file_url", |c| {
            ok(gelbooru::parse_file_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.gelbooru.parse_file_url_from_html", |c| {
            ok(gelbooru::parse_file_url_from_html(c.body(), c.url()))
        }),
        ("core.boorus.gelbooru.parse_preview_url", |c| {
            ok(gelbooru::parse_preview_url(c.body(), c.url(), &api_post(c)))
        }),
        ("core.boorus.gelbooru.parse_preview_url_from_html", |c| {
            ok(gelbooru::parse_preview_url_from_html(c.body(), c.url()))
        }),
        ("core.boorus.gelbooru.parse_rating", |c| {
            ok(gelbooru::parse_rating(c.body(), c.url()))
        }),
        ("core.boorus.gelbooru.parse_rating_from_html", |c| {
            ok(gelbooru::parse_rating_from_html(c.body(), c.url()))
        }),
    ],
};
