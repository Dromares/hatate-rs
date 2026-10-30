//! Settings > MCP (`gui/settings_dialog.py` `_build_mcp_tab` and what hangs off it at `76c85c4`,
//! against `hatate-ryoku-mockup/mcp-settings-spec.md`).
//!
//! The controls are data like every other page ([`PAGE`]); the Recent tool calls card is the one
//! part with a widget of its own ([`AuditCard`]). What each cell says is `mcp_audit_view.rs`, and
//! what the Connection card says is the same module's `connection`.

use crate::mcp_audit_view::{self, AuditRow, Weight};
use crate::settings_node::{Node, Row};
use crate::widgets;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// The Notebook tab's name.
pub const TAB: &str = "MCP";

/// `QTimer.setInterval(2000)`: how often the table reads the log while the tab is open.
pub const AUDIT_INTERVAL_MS: u64 = 2000;

pub const INTRO: &str = "Lets a connected AI read the queue, see the pictures, and record decisions while you're away. Off by default.";
pub const ENABLE: &str = "Enable the MCP server";
pub const PORT_HINT: &str =
    "Loopback only (127.0.0.1) \u{2014} never reachable from another machine on your network.";
pub const DRY_RUN: &str = "Dry run \u{2014} practice mode";
pub const DRY_RUN_ON: &str = "Dry run is on. The AI can look at everything and tell you what it would pick, but every action \u{2014} picking a candidate, marking reviewed, re-searching, sending to Hydrus, removing a row \u{2014} is logged as recorded-not-run and never actually happens.";
pub const DRY_RUN_OFF: &str =
    "Dry run is off. Allowed actions happen for real, including anything enabled above.";
pub const ALWAYS_TITLE: &str = "Always available when MCP is on";
pub const ALWAYS_READ: &str = "Read everything \u{2014} the queue, each entry, candidates, scores, upscale verdicts, tags, the images themselves, and the diff between two candidates.";
pub const ALWAYS_LOCAL: &str = "Make reversible local decisions \u{2014} pick a candidate, mark an entry reviewed. Undo by opening the entry again and choosing differently.";
pub const HYDRUS_TITLE: &str = "On by default \u{2014} sends to your live Hydrus";
pub const HYDRUS_LABEL: &str = "Let it send files and tags to Hydrus";
pub const HYDRUS_HINT: &str = "This is on, because you asked for it. Every send lands as a row below in Recent tool calls \u{2014} check there for exactly what went out and when. Turn this off here to stop it.";
pub const MARKER_TIP: &str = "Live - this permission is already on.";
pub const RESEARCH_TITLE: &str = "Off by default \u{2014} spends a limited allowance";
pub const RESEARCH_LABEL: &str = "Let it re-search a stuck entry";
pub const RESEARCH_HINT: &str = "Spends the same daily allowance a manual re-search would \u{2014} SauceNAO's is capped per day. Leave this off to keep that allowance for your own searching.";
pub const DESTRUCTIVE_TITLE: &str = "Off by default \u{2014} hard to undo by hand";
pub const DESTRUCTIVE_LABEL: &str = "Let it remove rows and reset results";
pub const DESTRUCTIVE_HINT: &str = "Throws away work this app already did. There's no confirmation prompt from the AI side \u{2014} only this switch.";
pub const TOKEN_GENERATED: &str = "New token generated and copied to clipboard.";
pub const COPY_TIP: &str = "Copy token to clipboard";
pub const FOLLOW_TIP: &str = "Keep this up to date while the tab is open.";
pub const FOLDER_TIP: &str = "Open the folder the audit log is in.";
pub const SUMMARY_TITLE: &str = "Enabling the MCP server";

/// The page, minus the Recent tool calls card ([`AuditCard`]), which the dialog appends.
pub const PAGE: Node = Node::Column(&[
    Node::Label {
        text: INTRO,
        wrap: true,
        hint: true,
    },
    Node::Check {
        id: "mcp_enabled",
        text: ENABLE,
        tip: "",
    },
    Node::Group {
        title: "Connection",
        body: &Node::Column(&[
            Node::Live {
                id: "mcp_connection_primary",
                wrap: true,
                hint: false,
            },
            Node::Live {
                id: "mcp_connection_remedy",
                wrap: true,
                hint: true,
            },
            Node::Line(&[
                Node::Stretch,
                Node::Pill {
                    id: "mcp_open_log",
                    text: "Open the log",
                    tip: "",
                },
                Node::Pill {
                    id: "mcp_restart",
                    text: "Restart server",
                    tip: "",
                },
            ]),
        ]),
    },
    Node::Form(&[
        Row {
            label: Some(Node::Label {
                text: "Port:",
                wrap: false,
                hint: false,
            }),
            field: Node::Int {
                id: "mcp_port",
                min: 1024,
                max: 65535,
                step: 1,
                suffix: "",
                special: "",
                tip: "",
            },
        },
        Row {
            label: Some(Node::Label {
                text: "",
                wrap: false,
                hint: false,
            }),
            field: Node::Label {
                text: PORT_HINT,
                wrap: true,
                hint: true,
            },
        },
        Row {
            label: Some(Node::Label {
                text: "Token:",
                wrap: false,
                hint: false,
            }),
            field: Node::Line(&[
                Node::Text {
                    id: "mcp_token",
                    password: true,
                    placeholder: "",
                    tip: "",
                },
                Node::Icon {
                    id: "mcp_copy_token",
                    glyph: "\u{29c9}",
                    tip: COPY_TIP,
                },
                Node::Pill {
                    id: "mcp_generate_token",
                    text: "Generate new token",
                    tip: "",
                },
            ]),
        },
        Row {
            label: Some(Node::Label {
                text: "",
                wrap: false,
                hint: false,
            }),
            field: Node::Live {
                id: "mcp_token_hint",
                wrap: true,
                hint: true,
            },
        },
    ]),
    Node::Check {
        id: "mcp_dry_run",
        text: DRY_RUN,
        tip: "",
    },
    Node::Live {
        id: "mcp_dry_run_hint",
        wrap: true,
        hint: true,
    },
    Node::Group {
        title: ALWAYS_TITLE,
        body: &Node::Column(&[
            Node::Label {
                text: ALWAYS_READ,
                wrap: true,
                hint: false,
            },
            Node::Label {
                text: ALWAYS_LOCAL,
                wrap: true,
                hint: false,
            },
        ]),
    },
    Node::Group {
        title: HYDRUS_TITLE,
        body: &Node::Column(&[
            Node::Line(&[
                Node::Check {
                    id: "mcp_allow_hydrus_writes",
                    text: HYDRUS_LABEL,
                    tip: "",
                },
                Node::Live {
                    id: "mcp_live_marker",
                    wrap: false,
                    hint: false,
                },
            ]),
            Node::Label {
                text: HYDRUS_HINT,
                wrap: true,
                hint: true,
            },
        ]),
    },
    Node::Group {
        title: RESEARCH_TITLE,
        body: &Node::Column(&[
            Node::Check {
                id: "mcp_allow_research",
                text: RESEARCH_LABEL,
                tip: "",
            },
            Node::Label {
                text: RESEARCH_HINT,
                wrap: true,
                hint: true,
            },
        ]),
    },
    Node::Group {
        title: DESTRUCTIVE_TITLE,
        body: &Node::Column(&[
            Node::Check {
                id: "mcp_allow_destructive",
                text: DESTRUCTIVE_LABEL,
                tip: "",
            },
            // the one hint drawn at `ink_100`: turning this on is a decision still in front of
            // the user
            Node::Live {
                id: "mcp_destructive_hint",
                wrap: true,
                hint: true,
            },
        ]),
    },
]);

/// `_refresh_mcp_dry_run_hint`: the sentence under the Dry run box, and the ink it is drawn at.
pub fn dry_run_text(on: bool) -> (&'static str, Weight) {
    if on {
        (DRY_RUN_ON, Weight::Ink65)
    } else {
        (DRY_RUN_OFF, Weight::Ink100)
    }
}

/// `_show_mcp_enable_summary`: what the box lists, from the tiers ticked in the dialog now.
pub fn enable_summary(hydrus_writes: bool, research: bool, destructive: bool) -> String {
    let mut lines: Vec<&str> = vec![
        "Starting now, a connected AI with the token below can:",
        "",
        " \u{2022}  Read everything \u{2014} the queue, entries, candidates, scores, tags,",
        "    the images themselves, and diffs.",
        " \u{2022}  Pick a candidate and mark entries reviewed. Undo by opening the",
        "    entry again.",
    ];
    if hydrus_writes {
        lines.push(
            " \u{2022}  Send files and tags to your live Hydrus library \u{2014} on by default.",
        );
    }
    if research {
        lines.push(" \u{2022}  Re-search a stuck entry, spending your SauceNAO allowance.");
    }
    if destructive {
        lines.push(" \u{2022}  Remove rows and reset results, with no confirmation on its side.");
    }
    lines.push("");
    lines.push("Recent tool calls below records exactly what it does. Turn any of");
    lines.push("this off in the groups above, any time.");
    lines.join("\n")
}

/// Sets the ink weight of a label (`theme.ink_color(mode, 'ink_65' | 'ink_100')`).
pub fn set_weight(label: &impl IsA<gtk::Widget>, weight: Weight) {
    label.remove_css_class("ink-65");
    label.remove_css_class("ink-100");
    label.add_css_class(match weight {
        Weight::Ink65 => "ink-65",
        Weight::Ink100 => "ink-100",
    });
}

/// The columns of the table, in order.
pub const COLUMNS: [&str; 5] = ["Time", "Tool", "Tier", "Target", "Outcome"];

/// The Recent tool calls card.
pub struct AuditCard {
    pub root: gtk::Box,
    pub actions: gtk::ToggleButton,
    pub all: gtk::ToggleButton,
    pub follow: gtk::ToggleButton,
    pub max_mb: gtk::SpinButton,
    pub caption: gtk::Label,
    /// The heading, the spin box's two labels and the path caption: the labels the Find box can
    /// name.
    pub labels: Vec<gtk::Label>,
    /// The table's scroller and its grid, for a test that measures the columns.
    pub scroll: gtk::ScrolledWindow,
    pub grid: gtk::Grid,
    path: PathBuf,
    now: Rc<dyn Fn() -> i64>,
    timer: RefCell<Option<glib::SourceId>>,
    /// Whether the MCP tab is the one on show (`self.tabs.currentIndex() == self._mcp_tab_index`).
    on_show: Cell<bool>,
    /// The rows last drawn, for a test.
    last: RefCell<Vec<AuditRow>>,
    /// Itself, for the timer.
    me: RefCell<Weak<AuditCard>>,
}

impl AuditCard {
    /// `_build_mcp_audit_card`. `open_folder` is `QDesktopServices.openUrl` on the log's folder.
    pub fn build(
        path: PathBuf,
        max_mb: i64,
        now: Rc<dyn Fn() -> i64>,
        open_folder: Rc<dyn Fn()>,
    ) -> Rc<AuditCard> {
        let root = widgets::card(None, true);
        root.set_margin_start(0);
        root.set_margin_end(0);

        let heading = widgets::heading("Recent tool calls");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.append(&heading);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        head.append(&spacer);
        // the filter's own weak handle, filled once the card exists
        let card_cell: Rc<RefCell<Weak<AuditCard>>> = Rc::new(RefCell::new(Weak::new()));
        let filter_cell = card_cell.clone();
        let (filter_row, buttons) = widgets::segmented(
            &[("actions", "Actions", ""), ("all", "All calls", "")],
            "actions",
            move |_| {
                if let Some(card) = filter_cell.borrow().upgrade() {
                    card.refresh();
                }
            },
        );
        head.append(&filter_row);
        root.append(&head);

        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let follow = gtk::ToggleButton::with_label("\u{27f3}");
        follow.add_css_class("icon-boxed");
        follow.set_size_request(widgets::ICON_BOX, widgets::ICON_BOX);
        follow.set_tooltip_text(Some(FOLLOW_TIP));
        follow.update_property(&[gtk::accessible::Property::Label(FOLLOW_TIP)]);
        follow.set_active(true);
        controls.append(&follow);
        let folder = widgets::icon_button("\u{2197}", FOLDER_TIP, true);
        {
            let open = open_folder.clone();
            folder.connect_clicked(move |_| open());
        }
        controls.append(&folder);
        let gap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        gap.set_hexpand(true);
        controls.append(&gap);
        let keep = gtk::Label::new(Some("Keep up to"));
        set_weight(&keep, Weight::Ink100);
        controls.append(&keep);
        let spin = crate::settings_form::spin_button(1.0, 500.0, 1.0, 0, "", "");
        spin.set_value(max_mb as f64);
        controls.append(&spin);
        let before = gtk::Label::new(Some("MB before trimming the oldest entries."));
        set_weight(&before, Weight::Ink100);
        before.set_ellipsize(gtk::pango::EllipsizeMode::End);
        controls.append(&before);
        root.append(&controls);

        let grid = gtk::Grid::new();
        grid.set_column_spacing(18);
        grid.set_row_spacing(6);
        grid.add_css_class("mcp-audit");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&grid));
        scroll.set_min_content_height(220);
        // The table fits the page at the minimum window: Target and Outcome ellipsize (the full text is
        // in the tooltip), so the column the card exists for is never cut off the side with no cue.
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.add_css_class("field");
        root.append(&scroll);

        let caption = widgets::muted("");
        root.append(&caption);
        let writing = widgets::muted(&format!("Writing to {}", path.display()));
        // a path has no break to wrap at: it gives way in the middle, the full text in the tooltip
        writing.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        writing.set_tooltip_text(Some(&writing.text()));
        root.append(&writing);

        let card = Rc::new(AuditCard {
            root,
            actions: buttons["actions"].clone(),
            all: buttons["all"].clone(),
            follow: follow.clone(),
            max_mb: spin,
            caption,
            labels: vec![heading, keep, before, writing],
            scroll,
            grid,
            path,
            now,
            timer: RefCell::new(None),
            on_show: Cell::new(false),
            last: RefCell::new(Vec::new()),
            me: RefCell::new(Weak::new()),
        });
        *card_cell.borrow_mut() = Rc::downgrade(&card);
        *card.me.borrow_mut() = Rc::downgrade(&card);
        {
            let weak = Rc::downgrade(&card);
            follow.connect_toggled(move |b| {
                let Some(card) = weak.upgrade() else { return };
                if b.is_active() {
                    card.refresh();
                    card.sync_timer();
                } else {
                    card.stop_timer();
                }
            });
        }
        card.refresh();
        card
    }

    /// `_refresh_mcp_audit_table`.
    pub fn refresh(&self) {
        let entries = mcp_audit_view::read_entries(&self.path);
        let view = mcp_audit_view::view(&entries, self.actions.is_active(), (self.now)());
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        for (c, name) in COLUMNS.iter().enumerate() {
            let head = widgets::caps_label(name, "section-label");
            self.grid.attach(&head, c as i32, 0, 1, 1);
        }
        for (r, row) in view.rows.iter().enumerate() {
            let r = r as i32 + 1;
            let cells = [&row.time, &row.tool, &row.tier, &row.target, &row.outcome];
            for (c, text) in cells.iter().enumerate() {
                let label = gtk::Label::new(Some(text));
                label.set_xalign(0.0);
                if c < 2 {
                    label.add_css_class("mono");
                }
                if (1..=3).contains(&c) {
                    // Tool, Tier and Target give way before Outcome does
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    label.set_tooltip_text(Some(text));
                }
                if c == 3 {
                    label.set_max_width_chars(14);
                }
                if c == 4 {
                    label.set_hexpand(true);
                    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    // at the minimum window the other columns give way first
                    label.set_width_chars(11);
                    label.set_tooltip_text(Some(&row.tooltip));
                    set_weight(&label, row.weight);
                } else {
                    set_weight(&label, Weight::Ink100);
                }
                self.grid.attach(&label, c as i32, r, 1, 1);
            }
        }
        self.caption.set_text(&view.caption);
        *self.last.borrow_mut() = view.rows;
    }

    /// The rows as last drawn.
    pub fn rows(&self) -> Vec<AuditRow> {
        self.last.borrow().clone()
    }

    /// The tab came to the front or left it (`_on_tabs_current_changed`).
    pub fn set_on_show(&self, on_show: bool) {
        self.on_show.set(on_show);
        if on_show {
            self.refresh();
        }
        self.sync_timer();
    }

    /// The timer runs while the tab is on show and the follow button is down.
    fn sync_timer(&self) {
        let want = self.on_show.get() && self.follow.is_active();
        let running = self.timer.borrow().is_some();
        if want && !running {
            let weak = self.me.borrow().clone();
            let id =
                glib::timeout_add_local(
                    Duration::from_millis(AUDIT_INTERVAL_MS),
                    move || match weak.upgrade() {
                        Some(card) => {
                            card.refresh();
                            glib::ControlFlow::Continue
                        }
                        None => glib::ControlFlow::Break,
                    },
                );
            *self.timer.borrow_mut() = Some(id);
        } else if !want {
            self.stop_timer();
        }
    }

    pub fn stop_timer(&self) {
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
    }

    /// Whether the refresh timer is armed.
    pub fn following(&self) -> bool {
        self.timer.borrow().is_some()
    }
}
