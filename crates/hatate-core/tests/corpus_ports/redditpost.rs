//! `core.boorus.redditpost.*` to `hatate_core::boorus::redditpost`. A Reddit item's input is the
//! post's RSS feed and `extra_args[0]` is the post URL; it reads no context.
//!
//! `parse_dimensions` reads the remote file in Python. The items that reach that read list it in
//! `skipped_network_callables` and are not run for it; the ones in `callables` never get that
//! far, and the read handed to the Rust function here fails the item if it is ever called.

use hatate_core::boorus::redditpost;

use crate::{Call, Port, ok};

pub const PORT: Port = Port {
    family: "boorus",
    unit: "redditpost",
    calls: &[
        ("core.boorus.redditpost.incomplete_reason", |c: &Call| {
            ok(redditpost::incomplete_reason(c.body(), c.url()))
        }),
        ("core.boorus.redditpost.parse", |c| {
            ok(redditpost::parse(c.body(), c.url()))
        }),
        ("core.boorus.redditpost.parse_dimensions", |c| {
            ok(redditpost::parse_dimensions(
                c.body(),
                c.url(),
                |file, _| panic!("the corpus ran parse_dimensions on {file}, which needs a socket"),
            ))
        }),
        ("core.boorus.redditpost.parse_file_url", |c| {
            ok(redditpost::parse_file_url(c.body(), c.url()))
        }),
        ("core.boorus.redditpost.parse_preview_url", |c| {
            ok(redditpost::parse_preview_url(c.body(), c.url()))
        }),
    ],
};
