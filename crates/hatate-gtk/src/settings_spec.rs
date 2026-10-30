//! The Settings dialog's pages as data: every label, tooltip, range, choice and placeholder
//! of `gui/settings_dialog.py` at `76c85c4`, read off the built Python dialog (offscreen, default
//! settings) and written here by a script kept outside the tree (`docs/gtk-6h.md`, "Where the
//! spec comes from"). The Shortcuts and MCP pages are 6.i's.
//!
//! Generated. A change here is a change to what the dialog says: regenerate, do not hand-edit.

use crate::settings_node::{Node, Row};

/// `engine_id`s in `core/engines.ALL_ENGINES` order, for the per-engine delay rows.
pub const ENGINES: &[&str] = &[
    r#"iqdb"#,
    r#"saucenao"#,
    r#"ascii2d"#,
    r#"tracemoe"#,
    r#"iqdb3d"#,
    r#"googleimages"#,
    r#"googlelens"#,
    r#"yandex"#,
    r#"pawchive"#,
    r#"pawchiveindex"#,
];

/// `(tab name, page)`, in the dialog's tab order.
pub const PAGES: &[(&str, Node)] = &[
    (
        r#"General"#,
        Node::Column(&[
            Node::Group {
                title: r#"Performance"#,
                body: &Node::Form(&[
                    Row {
                        label: Some(Node::Label {
                            text: r#"Delay between searches:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"delay_min"#,
                                min: 5.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#""#,
                                tip: r#""#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"delay_max"#,
                                min: 5.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#""#,
                                tip: r#""#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ IQDB delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.iqdb"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to IQDB specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.iqdb"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to IQDB specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ SauceNAO delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.saucenao"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to SauceNAO specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.saucenao"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to SauceNAO specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ ascii2d delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.ascii2d"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to ascii2d specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.ascii2d"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to ascii2d specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ trace.moe delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.tracemoe"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to trace.moe specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.tracemoe"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to trace.moe specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ IQDB 3D delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.iqdb3d"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to IQDB 3D specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.iqdb3d"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to IQDB 3D specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Google Images delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.googleimages"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Google Images specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.googleimages"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Google Images specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Google Lens delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.googlelens"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Google Lens specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.googlelens"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Google Lens specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Yandex delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.yandex"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Yandex specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.yandex"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Yandex specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Pawchive delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.pawchive"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Pawchive specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.pawchive"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Pawchive specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Pawchive index delay:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Label {
                                text: r#"min"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_min.pawchiveindex"#,
                                min: 0.0,
                                max: 600.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Pawchive index specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                            Node::Label {
                                text: r#"max (seconds)"#,
                                wrap: false,
                                hint: false,
                            },
                            Node::Double {
                                id: r#"engine_delay_max.pawchiveindex"#,
                                min: 0.0,
                                max: 900.0,
                                dec: 2,
                                step: 1.0,
                                suffix: r#""#,
                                special: r#"same as above"#,
                                tip: r#"How long to leave between two requests to Pawchive index specifically.

Left at "same as above", this engine uses the general delay. Set it only for a service whose limits you actually know - the general delay is the safe default, and a value that is too low risks getting your IP blocked by that site.

The longer of the two waits always wins, so this can slow a search down but never speeds it past the general delay unless you set it lower here."#,
                            },
                        ]),
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Hash source:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"hash_source"#,
                            items: &[
                                (
                                    r#"Hydrus hashes - read from the filename where possible"#,
                                    r#"hydrus"#,
                                ),
                                (r#"Local hashes - always read every file"#, r#"local"#),
                            ],
                            tip: r#"Where a file's SHA256 comes from when adding a batch.

Hydrus hashes: Hydrus names every file in its store after that file's own SHA256 - the very hash this app would otherwise spend time computing - so it's taken straight from the filename and the file is never opened. Files that aren't named that way are still read normally. On a large batch over a network share this is the difference between minutes and seconds. A few files per batch are read and checked against their names first, and if any disagree the whole batch is read properly instead.

Local hashes: always read and hash every file's bytes. Slower, but makes no assumption about what filenames mean - use this if you have files named like hashes whose contents don't match.

Both produce identical hashes for a genuine Hydrus store."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Row thumbnails:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"thumbnail_source"#,
                            items: &[
                                (
                                    r#"Hydrus thumbnails where available (fastest)"#,
                                    r#"hydrus"#,
                                ),
                                (r#"Decode from the files themselves"#, r#"local"#),
                                (r#"No row thumbnails"#, r#"off"#),
                            ],
                            tip: r#"Where the small preview in each table row comes from.

Hydrus thumbnails: for files Hydrus already has, it serves its own stored thumbnail - a few KB, versus reading the whole multi-megabyte file just to shrink it. Anything Hydrus doesn't have is still decoded from the file as normal. Needs a Hydrus access key.

Decode from the files: always read the file itself. Correct everywhere, but on a large batch over a network share it means pulling every byte across the wire.

No row thumbnails: reads nothing at all. The fastest way to add a very large batch."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"lazy_thumbnails"#,
                            text: r#"Only load thumbnails for rows on screen"#,
                            tip: r#"Generates thumbnails for the rows actually visible (plus a small buffer above and below), refreshing as you scroll, instead of generating one for every file the moment it's added.

A table shows around twenty rows at a time, so for a batch of tens of thousands this is the difference between reading a handful of files and reading the whole library.

Turn off to go back to generating every thumbnail up front."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Parallel file hashing:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Int {
                            id: r#"hash_workers"#,
                            min: 1,
                            max: 16,
                            step: 1,
                            suffix: r#""#,
                            special: r#""#,
                            tip: r#"How many files to hash at once when adding a batch. 1 = sequential (the safe default). Higher values often help a lot on a network share (each read has latency a single stream spends idle), but usually do nothing - or slightly hurt - on a local disk. Whichever you pick, the app logs its own MB/s for each batch (Help > View Logs), so you can compare values and see what your storage actually prefers."#,
                        },
                    },
                ]),
            },
            Node::Group {
                title: r#"Session"#,
                body: &Node::Form(&[
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"restore_session"#,
                            text: r#"Restore the image list on startup"#,
                            tip: r#"Saves the working list when you close the app and reloads it next launch, so a restart doesn't re-hash files it has already seen - which for a large batch on a network share is minutes of work. Thumbnails regenerate in the background."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"autosave_session"#,
                            text: r#"Autosave the image list while running"#,
                            tip: r#"Saves the working list periodically, not just when you close the app.

Searching a large batch runs for hours or days; without this, a crash or power cut loses everything since launch - most importantly which files had already been sent to Hydrus, so you'd have no way to tell what still needs sending.

This is the app's own automatic session. Sessions you save yourself from the File menu are never touched by it."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Autosave every:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Int {
                            id: r#"autosave_interval"#,
                            min: 10,
                            max: 3600,
                            step: 1,
                            suffix: r#" seconds"#,
                            special: r#""#,
                            tip: r#"How often to autosave. The save is cheap - it writes a small JSON file and skips thumbnails entirely - so a short interval costs little, but there's no point going below the time it takes to search a single image."#,
                        },
                    },
                ]),
            },
            Node::Group {
                title: r#"Logging"#,
                body: &Node::Form(&[
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"log_matched"#,
                            text: r#"Log matched URLs to a text file"#,
                            tip: r#""#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Log file:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Line(&[
                            Node::Text {
                                id: r#"log_path"#,
                                password: false,
                                placeholder: r#""#,
                                tip: r#""#,
                            },
                            Node::Button {
                                id: r#"button.browse"#,
                                text: r#"Browse…"#,
                                tip: r#""#,
                            },
                        ]),
                    },
                ]),
            },
            Node::Group {
                title: r#"Tag sources shown in the tags list"#,
                body: &Node::Form(&[
                    Row {
                        label: None,
                        field: Node::Checks {
                            id: r#"tag_sources_list"#,
                            items: &[
                                r#"User"#,
                                r#"Hydrus"#,
                                r#"Search engine"#,
                                r#"Booru"#,
                                r#"Hatate-linux"#,
                            ],
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"filter_sent_tags"#,
                            text: r#"Also apply these sources to tags sent to Hydrus and written to exports"#,
                            tip: r#"Off by default, and left off for existing settings: until now every send carried every tag regardless of what was ticked above, so turning this on for you would have changed what your next send put in your Hydrus library.

On, a send and an export carry only the sources ticked above - including none at all, if you untick everything."#,
                        },
                    },
                ]),
            },
            Node::Group {
                title: r#"Tag files written beside your images (Files > Write Tag Files)"#,
                body: &Node::Form(&[
                    Row {
                        label: Some(Node::Label {
                            text: r#"Name tag files:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"sidecar_style"#,
                            items: &[
                                (
                                    r#"image.jpg.txt - keeps the image's extension"#,
                                    r#"append"#,
                                ),
                                (
                                    r#"image.txt - replaces the image's extension"#,
                                    r#"replace"#,
                                ),
                            ],
                            tip: r#"Both forms are in real use and neither is readable by everything.

image.jpg.txt is what Hydrus's own sidecar importer looks for, and it can't be confused with a text file of yours or collide with the sidecar for a same-named .png.

image.txt is what the stable-diffusion training tooling expects. Pick it if that's what will read these, bearing in mind it's the form that can land on an existing file that has nothing to do with this app."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Existing tag files:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"sidecar_overwrite"#,
                            items: &[
                                (r#"Ask me"#, r#"ask"#),
                                (r#"Keep the existing file"#, r#"skip"#),
                                (r#"Replace it"#, r#"overwrite"#),
                            ],
                            tip: r#"What to do when a text file is already sitting where a tag file would go. This app cannot tell one it wrote on an earlier run from a note you wrote yourself, which is why the default asks.

Either of the other two still shows the count and says which it is about to do before writing anything - it just doesn't make you choose each time."#,
                        },
                    },
                ]),
            },
        ]),
    ),
    (
        r#"Engine"#,
        Node::Column(&[
            Node::Group {
                title: r#"Primary search"#,
                body: &Node::Form(&[
                    Row {
                        label: Some(Node::Label {
                            text: r#"Search/booru request timeout:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Double {
                            id: r#"search_timeout"#,
                            min: 5.0,
                            max: 300.0,
                            dec: 2,
                            step: 1.0,
                            suffix: r#" seconds"#,
                            special: r#""#,
                            tip: r#"How long to wait for IQDB/SauceNAO and booru page requests before giving up. Separate from the Hydrus tab's timeout, which only applies to Hydrus API calls - a slow/unresponsive search request will now fail cleanly after this long instead of hanging indefinitely (or as long as your Hydrus timeout happens to allow)."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Search first with:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"primary_engine"#,
                            items: &[(r#"IQDB"#, r#"iqdb"#), (r#"SauceNAO"#, r#"saucenao"#)],
                            tip: r#""#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Second engine:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Combo {
                            id: r#"secondary_engine_mode"#,
                            items: &[
                                (r#"Always search both, merge every site"#, r#"always"#),
                                (
                                    r#"Only as a fallback, if the first finds nothing"#,
                                    r#"fallback"#,
                                ),
                                (r#"Never use the second engine"#, r#"disabled"#),
                            ],
                            tip: r#"When set to "Always", IQDB and SauceNAO run at the same time - they are different hosts, so waiting on one then the other only lengthens the per-image wait."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Fall back when the best match is under:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Double {
                            id: r#"fallback_below_similarity"#,
                            min: 0.0,
                            max: 100.0,
                            dec: 2,
                            step: 1.0,
                            suffix: r#"%"#,
                            special: r#"off"#,
                            tip: r#"Treat a weak result as no result, and let the fallback engines run anyway.

Normally the fallback only runs when the first engine finds NOTHING, so a 42% "maybe" stops the search just as firmly as a 96% certainty. Set a number here and anything below it counts as not good enough, so the remaining engines get a turn.

Only engines that MEASURE similarity count towards this: IQDB, SauceNAO, IQDB 3D, trace.moe and Pawchive (an exact copy, so 100%). ascii2d, Yandex and the two Google engines score their results by position rather than by likeness, so their numbers cannot satisfy the threshold - Google Lens's first result is always 80%, which would otherwise clear a 75% setting on every image.

Left at "off", nothing changes: any result at all still ends the search."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Current pipeline:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Live {
                            id: r#"engine_pipeline"#,
                            wrap: true,
                            hint: true,
                        },
                    },
                ]),
            },
            Node::Group {
                title: r#"Extra engines"#,
                body: &Node::Form(&[
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"extras_only_as_fallback"#,
                            text: r#"Only run the extra engines when falling back"#,
                            tip: r#"Holds ascii2d, trace.moe, IQDB 3D, Google Images, Google Lens and Yandex back, so they run only on the images IQDB/SauceNAO could not place well - rather than on every image.

Worth turning on once the slow or metered engines are in use: Google Lens spends at least 90 seconds per image and Cloud Vision is billed after its free monthly allowance, and most images never need either.

Pairs with the setting above. With that left "off", the extras run whenever the first engine finds nothing at all."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_ascii2d"#,
                            text: r#"Also search ascii2d (colour / feature, often finds Pixiv)"#,
                            tip: r#"ascii2d.net colour and feature search, run in parallel with IQDB/SauceNAO.

No API key. Strong at exact Pixiv and Twitter reposts IQDB often misses. Does not report a real similarity percentage - results are ordered, not scored, so they will not overturn a high-confidence IQDB match."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_tracemoe"#,
                            text: r#"Also search trace.moe (anime screenshots)"#,
                            tip: r#"Identifies which anime a screenshot is from, which episode, and the timestamp.

Off by default: anonymous use has a monthly quota, and a booru-tagging batch would burn it on images that are not screenshots. Use the right-click "trace.moe only" action for a handful of frames, or turn this on for a screenshot-only list.

A match points at AniList (the series), not a booru post - there is no full-resolution still to download. The value is the series tag."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"trace.moe minimum similarity:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Double {
                            id: r#"tracemoe_min_sim"#,
                            min: 50.0,
                            max: 100.0,
                            dec: 2,
                            step: 1.0,
                            suffix: r#"%"#,
                            special: r#""#,
                            tip: r#"trace.moe false-positives a lot below ~85%. Results under this are dropped."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_iqdb3d"#,
                            text: r#"Also search IQDB 3D (3d.iqdb.org, for 3D/CG)"#,
                            tip: r#"Same protocol as IQDB, against the 3D/CG index. Off by default so a 2D library does not spend a request per image on an index that will not match it."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_google_images"#,
                            text: r#"Also search Google Images (the whole web, no tags)"#,
                            tip: r#"Google's reverse image search, run in parallel with IQDB/SauceNAO.

Searches the whole web instead of a booru index, so it is the one that can still find a source - a personal site, an article, an artist's own portfolio - when every booru engine comes back empty.

Needs the Cloud Vision API key below. Google's public reverse-image page still accepts the upload, but it no longer puts any results in the page it sends back: they are fetched and drawn by Google's own JavaScript, which this app does not run. Without a key the search says so once and skips Google for the rest of the batch.

Vision's index is not the same one the reverse image search in your browser uses, and it is weaker: it will sometimes find nothing for a picture Lens places straight away. It is the best route available without running Google's JavaScript, not an equal substitute.

A hit carries no tags - only the page URL, plus whatever a booru parser can read if the page happens to be one it knows.

Does not report a real similarity percentage - results are ordered by page relevance, not by how much the picture matches, so they are scored below ascii2d and will not overturn an IQDB or SauceNAO match."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"    ↳ Cloud Vision API key:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Text {
                            id: r#"google_images_key"#,
                            password: true,
                            placeholder: r#""#,
                            tip: r#"A Google Cloud API key with the Cloud Vision API enabled, from console.cloud.google.com. Its web-detection feature is what actually answers "which pages is this image on".

Free for the first 1,000 images a month at the time of writing, billed after that - so on a large library, set the engine delay or leave this engine off for the bulk run and use the right-click "Google Images only" action on images nothing else could place.

Stored in the config file alongside your Hydrus and SauceNAO keys, which is kept readable only by you."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_google_lens"#,
                            text: r#"Also search Google Lens (drives a real browser)"#,
                            tip: r#"The reverse image search you get by dropping a picture into images.google.com - a different index from the Cloud Vision engine above. It returns the pages a picture actually appears on: DeviantArt, Reddit, Tumblr, Instagram, personal sites.

Needs Playwright and a Chromium build, which are optional and not installed by default (~150MB, and nothing else here uses them). In this app's virtualenv:
    venv/bin/pip install playwright
    venv/bin/playwright install chromium

A Chromium window opens while a search runs. That is deliberate: headless is challenged on sight, and a visible window is what lets you answer Google's robot check when it appears. The answer is remembered in a profile under ~/.config/hatate-linux/lens_profile, so it should be rare.

Lens's "Exact matches" tab renders its results with no link in the page at all, so those can only be recovered where the title carries the post id (Paheal does). "Visual matches" embeds its URLs and comes through directly. Exact matches sort above visual ones.

Google hides the matches for an explicit image behind a "these results may be explicit" confirmation. The engine clicks through that for you, so a library full of them does not need a click per image.

The slowest engine here - each image is a real page load, so it gets at least 90 seconds whatever the timeout above says. No tags, only the source page, and no real similarity percentage: results are scored below ascii2d and will not overturn an IQDB or SauceNAO match.

Best used through right-click "Google Lens only" on the few images nothing else could place, rather than across a whole library."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"lens_search_whole_image"#,
                            text: r#"    ↳ Search the whole picture, not the part Lens picks out"#,
                            tip: r#"Google Lens chooses a "search area" out of the uploaded picture and searches only that. Usually it takes the whole image, but it can settle on one small detail - a face in a group shot, a single panel of a page - and then the results are about that detail rather than the picture you searched for.

With this on, the crop is dragged back out to the whole image - the same thing you would do by hand - and only on the images Lens actually cropped.

Turn it off to take Lens's own choice, which is occasionally the smarter one - it can pick the character out of a busy collage."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_yandex"#,
                            text: r#"Also search Yandex (finds rule34.us / Danbooru posts the others miss)"#,
                            tip: r#"yandex.com's reverse image search, over plain HTTP - no browser, so it is far quicker than Google Lens.

Tried on 72 images IQDB, SauceNAO and Lens had found nothing or only a poor match for, it placed 7 on a post the app can tag from - mostly rule34.us, which IQDB does not index, plus Danbooru and Xbooru.

Yandex finds a great deal more than that - Pinterest, Telegraph, reposting blogs - but none of it carries tags, so only posts on sites the app can read are kept. Like the Google engines it gives no similarity of its own: each result is measured against your picture from its thumbnail.

If Yandex asks for a captcha, it rests for half an hour and the other engines carry on.

Off by default because each picture is uploaded to Yandex's servers."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_pawchive"#,
                            text: r#"Also look files up on Pawchive (exact copies of Patreon / Fanbox posts)"#,
                            tip: r#"Asks pawchive.pw whether it holds this exact file, by the file's SHA-256 hash. SauceNAO and IQDB don't index pawchive, so without this its posts never come up.

A hit is the very same file, so it's a certain 100% - and the post brings its artist and tags with it. Resized or re-saved copies have a different hash and are not found.

One small request per image and nothing uploaded, so it always runs alongside the first engine, even with "Only run the extra engines when falling back" on.

Off by default because it sends each file's hash (not the picture) to pawchive.pw."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"enable_pawchive_index"#,
                            text: r#"Search my Pawchive index (resized or re-saved copies)"#,
                            tip: r#"Compares each picture against the pawchive artists indexed under Files > Pawchive Index, so copies whose file has changed are found too.

Entirely local: nothing is sent, and until an artist is indexed it does nothing at all."#,
                        },
                    },
                ]),
            },
            Node::Group {
                title: r#"Match handling"#,
                body: &Node::Form(&[
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"use_search_cache"#,
                            text: r#"Reuse previous search results for images searched before"#,
                            tip: r#"Skips a fresh IQDB/SauceNAO request if this exact image (by file content, not filename) has already been searched before, using the saved result instead. "Re-search" and "Search with Opposite Engine" always bypass this and get a fresh result."#,
                        },
                    },
                    Row {
                        label: Some(Node::Label {
                            text: r#"Re-search "not found" after:"#,
                            wrap: false,
                            hint: false,
                        }),
                        field: Node::Double {
                            id: r#"search_cache_ttl"#,
                            min: 0.0,
                            max: 3650.0,
                            dec: 2,
                            step: 1.0,
                            suffix: r#" days"#,
                            special: r#""#,
                            tip: r#"How long a cached "not found" stays trusted before the image is searched again. Sites index new work constantly, so an old negative result says little about today - and once cached it is never rechecked. Set 0 to never expire.

Successful matches are never aged out: they don't go stale the same way, and re-running them would spend rate-limited quota to reach the same answer."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"retrieve_booru_tags"#,
                            text: r#"Retrieve tags from matched booru page"#,
                            tip: r#""#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"rank_by_quality"#,
                            text: r#"Recommend the most useful match, not just the most similar"#,
                            tip: r#"Orders the match dropdown so its first entry - the one selected by default - is the best match you can actually use, rather than whichever scored highest on visual similarity alone.

Similarity still decides: the quality adjustment is capped, so a clearly better match always stays on top. It only reorders matches that are already about equally likely to be the same image, preferring ones on sites this app can read tags from, and ones at a higher resolution than your local file.

Turn off for strict similarity order."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"drop_dead_matches"#,
                            text: r#"Discard matches whose source page is gone (404)"#,
                            tip: r#"While searching, if a match's source page returns a definite 404/410 - the post was deleted or removed - drop it and use the next best match instead, rather than presenting a dead link as a result. Only applies to definite 404/410 responses; a timeout, rate-limit or block leaves the match alone, since those don't mean the content is gone. Requires "Retrieve tags from matched booru page" above."#,
                        },
                    },
                    Row {
                        label: None,
                        field: Node::Check {
                            id: r#"drop_restricted_matches"#,
                            text: r#"Drop matches the site won't show without an account"#,
                            tip: r#"Some sites list a post but hold it back unless you're signed in:

  - Danbooru keeps banned posts (taken down at an artist's request) and posts with censored tags viewable only with a Gold account. They come back looking like normal matches, but the image can't be fetched, so there's nothing to preview, compare, download or send to Hydrus. The tags do still come through.
  - Anime-Pictures answers its API with 403 for account-only posts, which carries nothing at all - no tags, no dimensions, not even a preview.
  - DeviantArt serves adult deviations BLURRED when logged out. The artist still comes through, but the picture can't be compared against your file, which is the whole point of having it.

With this on they're dropped like any other unusable match, and the reason is written to the log. Turn it off to keep them listed.

Signing in is the real fix: cookies under Site Logins make these load normally."#,
                        },
                    },
                ]),
            },
        ]),
    ),
    (
        r#"Import"#,
        Node::Form(&[
            Row {
                label: None,
                field: Node::Check {
                    id: r#"borrow_tags"#,
                    text: r#"Take tags from another match when the best one has none"#,
                    tip: r#"Some sites cannot supply tags at all. MangaDex has no booru-style tags, and Sankaku can only be found by file hash - so a re-encoded or resaved local file can never be traced back to its post. Between them they were 94% of the untagged matches on a real library.

Often another site's copy of the SAME picture is already among the matches, with a full tag list, and only the best match's page is normally read. This reads one more and uses its tags.

The chosen match does not change - it is still the best picture, and the tag list says which match the tags actually came from."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"    …and it may be weaker by up to:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Double {
                    id: r#"borrow_tags_slack"#,
                    min: 0.0,
                    max: 25.0,
                    dec: 2,
                    step: 1.0,
                    suffix: r#"  percentage points"#,
                    special: r#""#,
                    tip: r#"How far BELOW the chosen match's similarity another match may be and still lend its tags.

This is the guard against tagging your image with a different picture's tags. A candidate within a few points is the same picture on another site, which is exactly what makes its tags worth having; a much weaker one may be something else entirely.

0 allows only an equal or better match to lend - the strictest setting, and on a measured library it covered 14% of the untagged matches, because ranking puts the strongest first and the rest sit a point or two behind. 5 covered 76%."#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"retry_failed_searches"#,
                    text: r#"Retry images that failed on a network error, once, at the end of a run"#,
                    tip: r#"An image marked Error failed on a timeout, a 502, or a rate limit - a network fault rather than a verdict about the image. Those results are never cached, precisely so they can be tried again, but until now nothing did: the run ended and the rows sat there until you noticed and re-searched them by hand.

With this on, each one gets exactly one more attempt once the first pass finishes. One, so an image that keeps failing cannot loop - and none are retried if you have stopped the run or SauceNAO's daily allowance has gone, since those would just fail the same way."#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"remove_after_import"#,
                    text: r#"Remove image(s) from the list after importing to Hydrus"#,
                    tip: r#"Applies to any of the Hydrus send actions in the right-click menu. Only removes images that were actually sent successfully - failed ones stay for a retry."#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"auto_import_enabled"#,
                    text: r#"Automatically send confident matches to Hydrus during search"#,
                    tip: r#"As each image in a batch is searched, if the match meets the similarity threshold below, it's sent to Hydrus immediately using the chosen method - before the next image in the batch is searched. Requires a Hydrus access key (Hydrus tab)."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Minimum similarity to auto-import:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Double {
                    id: r#"auto_import_min_similarity"#,
                    min: 0.0,
                    max: 100.0,
                    dec: 2,
                    step: 1.0,
                    suffix: r#"%"#,
                    special: r#""#,
                    tip: r#""#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Auto-import method:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Combo {
                    id: r#"auto_import_method"#,
                    items: &[
                        (r#"Send File + URL + Tags (upload)"#, r#"upload"#),
                        (r#"Send URL to Hydrus's URL Importer"#, r#"url_importer"#),
                        (
                            r#"Download Matched Image + Send with Tags"#,
                            r#"download_send"#,
                        ),
                    ],
                    tip: r#"Same three methods as the right-click menu. If "Remove image(s) from the list after importing" above is also on, the URL Importer method will wait (up to the timeout below) for Hydrus to actually confirm the download finished before removing it - so this batch may pause longer on images sent this way."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Hydrus import confirm timeout:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Double {
                    id: r#"url_import_confirm_timeout"#,
                    min: 5.0,
                    max: 600.0,
                    dec: 2,
                    step: 1.0,
                    suffix: r#" seconds"#,
                    special: r#""#,
                    tip: r#"How long to wait for Hydrus to confirm a URL-importer download actually finished (used by both the right-click action and auto-import) before giving up and leaving the image in the list rather than risk removing it too early. A live countdown shows next to the status bar while this is happening."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Check interval:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Double {
                    id: r#"url_import_confirm_interval"#,
                    min: 0.5,
                    max: 30.0,
                    dec: 2,
                    step: 1.0,
                    suffix: r#" seconds"#,
                    special: r#""#,
                    tip: r#"How often to check Hydrus while waiting."#,
                },
            },
        ]),
    ),
    (
        r#"SauceNAO"#,
        Node::Form(&[
            Row {
                label: Some(Node::Label {
                    text: r#"API key:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"saucenao_key"#,
                    password: true,
                    placeholder: r#""#,
                    tip: r#""#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"saucenao_json"#,
                    text: r#"Use JSON API (needs a key, retrieves more tags)"#,
                    tip: r#""#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"continue_without_saucenao"#,
                    text: r#"...or carry on without SauceNAO instead of stopping"#,
                    tip: r#"IQDB has no daily cap, so the alternative to stopping is an idle machine: a 24,000-image library against a 5,000-a-day allowance is five days, most of them spent waiting for midnight.

Results found this way are weaker than a full search would have given - which is exactly why stopping is the default - so they are marked Provisional in the Cache column and are NOT saved to the result cache. Once the allowance resets, right-click > Select by Cache State > Provisional and re-search them to get the real answer.

Takes precedence over the checkbox above."#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"saucenao_pause_on_quota"#,
                    text: r#"Pause searching when the daily quota runs out"#,
                    tip: r#"Stops the current batch as soon as SauceNAO reports its DAILY allowance is spent, leaving the remaining images unsearched so you can resume once it resets - rather than searching on and recording weaker results for the rest of the list. The ~30-second burst limit does NOT trigger this; that one clears by itself and the app already waits it out. Only applies when SauceNAO is part of your engine order."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Minimum similarity %:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Double {
                    id: r#"saucenao_min_sim"#,
                    min: 0.0,
                    max: 100.0,
                    dec: 2,
                    step: 1.0,
                    suffix: r#""#,
                    special: r#""#,
                    tip: r#""#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Tag namespace:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"saucenao_namespace"#,
                    password: false,
                    placeholder: r#"optional, e.g. "sn""#,
                    tip: r#""#,
                },
            },
            Row {
                label: None,
                field: Node::Label {
                    text: r#"Tag types to retrieve:"#,
                    wrap: false,
                    hint: false,
                },
            },
            Row {
                label: None,
                field: Node::Checks {
                    id: r#"saucenao_tag_types"#,
                    items: &[r#"character"#, r#"material"#, r#"creator"#],
                },
            },
        ]),
    ),
    (
        r#"Hydrus"#,
        Node::Form(&[
            Row {
                label: Some(Node::Label {
                    text: r#"API URL:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"hydrus_url"#,
                    password: false,
                    placeholder: r#""#,
                    tip: r#""#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Access key:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"hydrus_key"#,
                    password: true,
                    placeholder: r#""#,
                    tip: r#""#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Tag service:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Line(&[
                    Node::EditCombo {
                        id: r#"hydrus_service_key"#,
                        placeholder: r#"empty = "my tags" local service"#,
                        tip: r#""#,
                    },
                    Node::Button {
                        id: r#"button.fetch_from_hydrus"#,
                        text: r#"Fetch from Hydrus…"#,
                        tip: r#""#,
                    },
                ]),
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"hydrus_verify_ssl"#,
                    text: r#"Verify SSL certificate"#,
                    tip: r#""#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"set_hydrus_duplicate_relationships"#,
                    text: r#"Mark a downloaded better copy as a duplicate of your existing file"#,
                    tip: r#"When "Download match + send to Hydrus" brings in a better copy of a file Hydrus already holds, Hydrus ends up with two files it believes are unrelated - and nothing points from one to the other.

With this on, the app tells Hydrus how they relate. If the two are the same picture, the new copy becomes the king of the duplicate group - the one Hydrus shows. If it's a different edit or crop, the pair is marked as alternates instead, so neither supersedes the other.

Only ever on a confident comparison of the two actual files: anything the measurement can't settle - including a mirrored copy - is left alone rather than filed as a guess. Nothing is ever deleted.

Off by default, because it writes a relationship into your library and an incorrect pairing is tedious to undo.

Needs the "Edit File Relationships" permission on your Hydrus access key (services → review services → local → client api). Without it the import still succeeds and the app says which permission is missing."#,
                },
            },
            Row {
                label: None,
                field: Node::Check {
                    id: r#"write_hydrus_provenance_note"#,
                    text: r#"Write a note on the file recording where the match came from"#,
                    tip: r#"Everything this app knows about a match - which engine found it, which site it was on, how similar it is, and the link - is on screen and in the exports, and none of it survives in Hydrus. The file and its tags arrive; nothing says an automated tool put them there or what it matched against.

With this on, each send also writes a short note on the file in Hydrus with the engine, the site, the similarity, the matched URL and the date.

The similarity is written with the same marker the Similarity column uses: a "~" and a plain-English note when the number is the engine's own ranking of its results rather than a measurement, which is what ascii2d and the Google engines give. A bare percentage would claim more than this app knows.

The app replaces its own note on every send, so re-sending a file does not stack up copies. Notes under any other name - including your own - are never touched, and this one is deleted from Hydrus's own notes panel in two clicks.

Needs the "Edit File Notes" permission on your Hydrus access key (services → review services → local → client api). Without it the import still succeeds and the app says which permission is missing."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Source note name:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"hydrus_provenance_note_name"#,
                    password: false,
                    placeholder: r#"empty = "hatate: match source""#,
                    tip: r#"Which note the line above goes in. The app overwrites this note every time it sends the file, so give it a name you are not using for notes of your own."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_connection"#,
                    text: r#"Test connection"#,
                    tip: r#""#,
                }),
                field: Node::Live {
                    id: r#"hydrus_test_result"#,
                    wrap: false,
                    hint: false,
                },
            },
        ]),
    ),
    (
        r#"Tag Namespaces"#,
        Node::Column(&[
            Node::Check {
                id: r#"enable_namespace_remap"#,
                text: r#"Rename tag namespaces automatically"#,
                tip: r#"E.g. rewrite booru sites' "artist:" to "creator:" to match Hydrus's PTR tagging convention, or strip a namespace entirely (leave the new side blank). Applies to tags pulled from search engines/booru pages, existing Hydrus tags when imported, and manual inline edits - not to new tags typed via "Add tags"."#,
            },
            Node::Label {
                text: r#"One rule per line, as old:new - e.g. artist:creator rewrites every
"artist:" tag to "creator:" as it's pulled in. Leave the new side
empty (e.g. general:) to strip that namespace entirely - "general:bikini_top" becomes plain "bikini_top"."#,
                wrap: false,
                hint: false,
            },
            Node::Multiline {
                id: r#"namespace_remap_edit"#,
                placeholder: r#"artist:creator"#,
                tip: r#""#,
            },
            Node::Check {
                id: r#"enable_tag_blacklist"#,
                text: r#"Discard blacklisted tags as they're pulled in"#,
                tip: r#"Drops booru housekeeping tags (highres, tagme, bad_id, commentary_request, ...) so they never reach the tag list or get sent to Hydrus. Applies to tags from search engines and booru pages only - NOT to tags Hydrus already has on a file (those stay visible, since this can't remove them from Hydrus anyway), and NOT to tags you type yourself."#,
            },
            Node::Label {
                text: r#"One pattern per line. A pattern without a colon matches the tag name in any
namespace (highres blocks both "highres" and "meta:highres"); with a colon it
matches the full namespaced form (meta:* blocks that whole namespace). * and ?
wildcards work in both. Case-insensitive, and spaces/underscores are equivalent.
Note: namespace renaming above is applied first, so write patterns against the
renamed namespace (creator:*, not artist:*, if you remap artist to creator)."#,
                wrap: false,
                hint: false,
            },
            Node::Multiline {
                id: r#"tag_blacklist_edit"#,
                placeholder: r#"highres
bad_*
meta:*"#,
                tip: r#""#,
            },
            Node::Check {
                id: r#"add_rating_tag"#,
                text: r#"Add the matched post's rating as a tag"#,
                tip: r#"Turns the booru's own age rating for the matched post into a tag - "rating:explicit", "rating:safe", and so on. Only sites that actually state one contribute it (Danbooru, e621, Gelbooru/Safebooru/rule34/Xbooru); it is never guessed from anything else. The tag goes through the renaming and blacklist rules above like any other booru tag, so "rating:*" in the blacklist drops it again. Off by default, since this adds a tag to your library that wasn't there before - including in Hydrus."#,
            },
            Node::Line(&[
                Node::Label {
                    text: r#"Rating namespace:"#,
                    wrap: false,
                    hint: false,
                },
                Node::Text {
                    id: r#"rating_tag_namespace"#,
                    password: false,
                    placeholder: r#"rating"#,
                    tip: r#"The namespace that tag is filed under. Leave it blank to use "rating"."#,
                },
            ]),
            Node::Check {
                id: r#"enable_tag_colors"#,
                text: r#"Color tags to match Hydrus's namespace colors"#,
                tip: r#"Hydrus has no API to report your actual configured colors (and lets you recolor namespaces per-install), so this uses Hydrus's commonly-cited defaults - creator/artist red, character green, series/copyright magenta, studio maroon, meta gray. Override any of them below if yours differ."#,
            },
            Node::Label {
                text: r#"Optional overrides, one per line, as namespace:#RRGGBB - e.g. character:#RRGGBB
(use an empty namespace, i.e. just ":#RRGGBB", to override the unnamespaced color)"#,
                wrap: false,
                hint: false,
            },
            Node::Multiline {
                id: r#"namespace_colors_edit"#,
                placeholder: r#"character:#RRGGBB"#,
                tip: r#""#,
            },
            Node::Group {
                title: r#"Tags applied by rule"#,
                body: &Node::Column(&[
                    Node::Label {
                        text: r#"Tags this program adds itself, by how the search turned out - one per line,
optionally namespace:tag. All three are empty by default and nothing is added
until you fill one in. A search that errored gets nothing either way: it is
retried, so it has no outcome yet."#,
                        wrap: false,
                        hint: false,
                    },
                    Node::Label {
                        text: r#"When a match is found:"#,
                        wrap: false,
                        hint: false,
                    },
                    Node::Multiline {
                        id: r#"tags_for_found_edit"#,
                        placeholder: r#"hatate:found"#,
                        tip: r#"Added to every image a match was found for - both green (good) and yellow (worth reviewing) rows, since both are matches."#,
                    },
                    Node::Label {
                        text: r#"When no match is found:"#,
                        wrap: false,
                        hint: false,
                    },
                    Node::Multiline {
                        id: r#"tags_for_not_found_edit"#,
                        placeholder: r#"hatate:not found"#,
                        tip: r#"Added to every image no match was found for. This is the one worth setting: it turns "everything this program could not place" into a tag you can search your library for later, instead of a red row in a window you have to keep open.

Removed again automatically if a later search does find a match."#,
                    },
                    Node::Label {
                        text: r#"When a match has few tags:"#,
                        wrap: false,
                        hint: false,
                    },
                    Node::Multiline {
                        id: r#"tags_for_low_tag_count_edit"#,
                        placeholder: r#"hatate:few tags"#,
                        tip: r#"Added on top of the found tags when the match came back with fewer tags than the "minimum tags for a good match" setting in Match Conditions - the same number that already makes the row yellow.

Not added to images with no match at all: those have no tags by definition, so it would say nothing the not-found tag above did not."#,
                    },
                    Node::Label {
                        text: r#"These are filed under the "Hatate-linux" tag source. To keep them in this
program and out of Hydrus, untick that source under General and turn on
"also apply these sources to tags sent to Hydrus"."#,
                        wrap: false,
                        hint: false,
                    },
                ]),
            },
        ]),
    ),
    (
        r#"Site Logins"#,
        Node::Form(&[
            Row {
                label: None,
                field: Node::Label {
                    text: r#"Credentials for sites that hide content from logged-out visitors.
Stored in plain text in your config file - treat them like passwords."#,
                    wrap: false,
                    hint: false,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Pixiv session cookie:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"pixiv_session_cookie"#,
                    password: true,
                    placeholder: r#"PHPSESSID value from a logged-in browser session"#,
                    tip: r#"Pixiv no longer supports username/password sign-in for third-party tools, so this uses a session cookie instead. In your browser, log in to Pixiv, open developer tools (F12) > Application/Storage > Cookies > pixiv.net, and copy the value of PHPSESSID.

This lets the app see works that require login and R-18 content, which Pixiv hides from logged-out requests. Treat it like a password: it grants access to your account and is stored in plain text in the config file. It also expires periodically - re-copy it when Pixiv results start coming back empty."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_pixiv"#,
                    text: r#"Test Pixiv…"#,
                    tip: r#"Runs the real parser against a Pixiv artwork URL you provide and reports exactly what came back - HTTP status, whether the page data was found, and how many tags and image URLs were extracted. Uses the cookie currently typed above, so you can check a new one before saving."#,
                }),
                field: Node::Live {
                    id: r#"pixiv_test_result"#,
                    wrap: true,
                    hint: false,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"e621 username:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"e621_username"#,
                    password: false,
                    placeholder: r#"your e621 username"#,
                    tip: r#"e621 keeps some posts under a global blacklist. The API still returns them, but withholds the file - so the match arrives with no picture to preview, download or compare, and often no tags either. Signing in shows them.

Unlike the cookie-based sites here, e621 provides a proper API key for third-party tools, so no browser cookie-hunting is needed."#,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"e621 API key:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"e621_api_key"#,
                    password: true,
                    placeholder: r#"from Account > Manage API Access on e621"#,
                    tip: r#"On e621: Account > Manage API Access. Treat it like a password - it grants access to the account it came from.

Both fields are needed; a username on its own authenticates nothing and is ignored.

A WRONG key is worse than none: e621 answers 401 to every request carrying one, including posts that would have worked anonymously. If that happens the app says so on the match rather than reporting a generic failure - clear both fields to go back to browsing anonymously."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_e621_login"#,
                    text: r#"Test e621 login…"#,
                    tip: r#"Sends one authenticated request to e621 with the username and key typed above, and reports whether they were accepted.

Worth doing before a run: a WRONG key is worse than none, because e621 answers 401 to every request carrying it - including ones that would have worked anonymously."#,
                }),
                field: Node::Live {
                    id: r#"e621_test_result"#,
                    wrap: true,
                    hint: false,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Sankaku cookies:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"sankaku_cookies"#,
                    password: true,
                    placeholder: r#"_sankakuchannel_session=...; locale=en"#,
                    tip: r#"Sankaku hides adult and account-only posts from logged-out requests - it shows a "No Content" page rather than returning an error, so without a login they're indistinguishable from deleted posts and get dropped as dead links.

To get them: log in to Sankaku, press F12 > Storage (Firefox) or Application (Chrome) > Cookies > chan.sankakucomplex.com. Paste them here as name=value pairs separated by semicolons, e.g.
    _sankakuchannel_session=u2uE0z...; locale=en

_sankakuchannel_session is the one that carries your login - the rest are just display preferences. You don't have to work out which is which, though: paste them all and whatever Sankaku currently uses gets sent.

Two things that catch people out:
  - That cookie is HttpOnly, so typing document.cookie in the console will NOT show it. It has to come from the Storage/Application panel.
  - Its value is long (600+ characters) and the panel truncates it on screen. Right-click the row and choose Copy Value to get the whole thing, or you'll paste a cut-off session that silently fails to authenticate.

Treat it like a password: it grants access to your account and is stored in plain text in the config file. It also expires, so if Sankaku matches start disappearing again, re-copy it."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_sankaku"#,
                    text: r#"Test Sankaku…"#,
                    tip: r#"Fetches a Sankaku post URL you provide using the cookies typed above and reports whether it came back as a real post or as the blank "no such post" page."#,
                }),
                field: Node::Live {
                    id: r#"sankaku_test_result"#,
                    wrap: true,
                    hint: false,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"Anime-Pictures cookies:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"animepictures_cookies"#,
                    password: true,
                    placeholder: r#"anime_pictures_jwt=eyJ0eXAiOiJKV1Qi…"#,
                    tip: r#"Anime-Pictures keeps some posts account-only. Its API answers those with HTTP 403 Forbidden, which carries none of the post - no tags, no dimensions, not even a preview - so without a login they simply come back blank.

The cookie that carries your login is anime_pictures_jwt. To get it: log in to anime-pictures.net, press F12 > Storage (Firefox) or Application (Chrome) > Cookies > anime-pictures.net, right-click that row and choose Copy Value.

Paste it here as anime_pictures_jwt=<value>. Pasting just the value on its own also works - the name is filled in for you. Other cookies can be included as name=value pairs separated by semicolons, but none of them are needed.

Two things that catch people out:
  - Use Copy Value, not a text selection. The panel truncates long values on screen, and a cut-off token is accepted as simply invalid: nothing errors, account-only posts just keep coming back blank.
  - It is HttpOnly, so typing document.cookie in the console will NOT show it. It has to come from the Storage/Application panel.

Don't bother with cf_clearance - it is tied to your browser's IP and User-Agent, so it won't validate for this app and can cause a challenge rather than avoid one.

Treat it like a password: it grants access to your account and is stored in plain text in the config file. It expires, so if account-only posts start coming back blank again, re-copy it."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_anime_pictures"#,
                    text: r#"Test Anime-Pictures…"#,
                    tip: r#"Fetches a post through the real API path using the cookies typed above and reports exactly what came back - whether it was readable, how many tags and what dimensions were found, or that it is still account-only."#,
                }),
                field: Node::Live {
                    id: r#"animepictures_test_result"#,
                    wrap: true,
                    hint: false,
                },
            },
            Row {
                label: Some(Node::Label {
                    text: r#"DeviantArt cookies:"#,
                    wrap: false,
                    hint: false,
                }),
                field: Node::Text {
                    id: r#"deviantart_cookies"#,
                    password: true,
                    placeholder: r#"auth=...; auth_secure=...; userinfo=..."#,
                    tip: r#"DeviantArt shows adult deviations BLURRED to logged-out visitors. The blur is baked into the signed image token itself, so nothing this app can do to the URL undoes it - a logged-in session is the only way to see the real picture.

To get them: log in to deviantart.com, press F12 > Storage (Firefox) or Application (Chrome) > Cookies > deviantart.com. Paste them here as name=value pairs separated by semicolons.

Paste all of them rather than picking one - DeviantArt splits a session across several cookies, and which ones matter has changed before.

Use Copy Value rather than selecting text: the panel truncates long values on screen, and a cut-off session is accepted as simply invalid - nothing errors, adult deviations just stay blurred.

DeviantArt renews its session cookies on activity (confirmed: it reissues one with a fresh expiry on every page it serves), which is why a browser tab stays logged in for months (DAN-122). This app now captures that renewal too (DAN-123): whenever a search or availability check actually fetches a DeviantArt page with these cookies attached, whatever comes back is folded in here and saved automatically, so a session kept in regular use should stay valid about as long as it would in a browser.

That only happens on an actual request, though - there's no way to renew a cookie without one. A pasted session that sits unused (the app closed, or no DeviantArt matches turning up) still ages out on its OWN original expiry and will need a fresh paste; that's expected, not a sign it was entered wrong.

Note this only affects the PICTURE. DeviantArt publishes no booru-style tags, so a match here gives you the artist and a preview either way.

Treat it like a password: it grants access to your account and is stored in plain text in the config file."#,
                },
            },
            Row {
                label: Some(Node::Button {
                    id: r#"button.test_deviantart"#,
                    text: r#"Test DeviantArt…"#,
                    tip: r#"Fetches a deviation through the real parser path using the cookies typed above and reports what came back - the artist, and whether the picture arrived properly or is still the blurred logged-out copy."#,
                }),
                field: Node::Live {
                    id: r#"deviantart_test_result"#,
                    wrap: true,
                    hint: false,
                },
            },
        ]),
    ),
];
