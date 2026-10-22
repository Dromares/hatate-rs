//! `core.google_lens.parse_exact_matches`, `parse_exact_tiles` and `parse_lens_payload` to
//! `hatate_core::engines::google_lens`. The first and third take the response body as text (a
//! `None` payload is `null`, which holds no literal either way); the second takes the decoded tile
//! list.

use hatate_core::engines::google_lens;

use crate::{Port, ok};

pub const PAYLOAD: Port = Port {
    family: "engines",
    unit: "core.google_lens.parse_lens_payload",
    calls: &[("core.google_lens.parse_lens_payload", |c| {
        ok(google_lens::parse_lens_payload(c.body()))
    })],
};

pub const EXACT_MATCHES: Port = Port {
    family: "engines",
    unit: "core.google_lens.parse_exact_matches",
    calls: &[("core.google_lens.parse_exact_matches", |c| {
        ok(google_lens::parse_exact_matches(c.body()))
    })],
};

pub const EXACT_TILES: Port = Port {
    family: "engines",
    unit: "core.google_lens.parse_exact_tiles",
    calls: &[("core.google_lens.parse_exact_tiles", |c| {
        ok(google_lens::parse_exact_tiles(&c.json()))
    })],
};
