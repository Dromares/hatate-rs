//! The shape of a Settings page as data (`settings_spec.rs` holds the pages themselves). One
//! `Node` per Qt widget or layout the Python dialog built; the GTK side (`settings_form.rs`) turns
//! a tree into widgets and the pure side (`settings_values.rs`) reads and writes `Settings` by
//! the `id` on each control. The `id` is the attribute name `SettingsDialog` gave the widget
//! (`self.delay_min`); the per-engine delay spinners, which Python kept in a dict, are
//! `engine_delay_min.<engine id>` and `engine_delay_max.<engine id>`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Node {
    /// `QGroupBox(title)` around a layout.
    Group {
        title: &'static str,
        body: &'static Node,
    },
    /// `QFormLayout`: rows of (label, field), or a field spanning both columns.
    Form(&'static [Row]),
    /// `QVBoxLayout`.
    Column(&'static [Node]),
    /// `QHBoxLayout`.
    Line(&'static [Node]),
    /// `QLabel`. `hint` is the `Hint` object name (muted, small).
    Label {
        text: &'static str,
        wrap: bool,
        hint: bool,
    },
    Check {
        id: &'static str,
        text: &'static str,
        tip: &'static str,
    },
    /// `QDoubleSpinBox`. `special` is shown instead of the number at `min`.
    Double {
        id: &'static str,
        min: f64,
        max: f64,
        dec: u32,
        step: f64,
        suffix: &'static str,
        special: &'static str,
        tip: &'static str,
    },
    /// `QSpinBox`.
    Int {
        id: &'static str,
        min: i64,
        max: i64,
        step: i64,
        suffix: &'static str,
        special: &'static str,
        tip: &'static str,
    },
    /// `QComboBox` of `(text, userData)`.
    Combo {
        id: &'static str,
        items: &'static [(&'static str, &'static str)],
        tip: &'static str,
    },
    /// An editable `QComboBox` (the Hydrus tag-service field): free text with a drop-down of
    /// fetched services.
    EditCombo {
        id: &'static str,
        placeholder: &'static str,
        tip: &'static str,
    },
    /// `QLineEdit`.
    Text {
        id: &'static str,
        password: bool,
        placeholder: &'static str,
        tip: &'static str,
    },
    /// `QPlainTextEdit`.
    Multiline {
        id: &'static str,
        placeholder: &'static str,
        tip: &'static str,
    },
    /// `QListWidget` of user-checkable items.
    Checks {
        id: &'static str,
        items: &'static [&'static str],
    },
    Button {
        id: &'static str,
        text: &'static str,
        tip: &'static str,
    },
    /// `widgets.pill_button(text, ..., uppercase=False)` (the MCP page's buttons).
    Pill {
        id: &'static str,
        text: &'static str,
        tip: &'static str,
    },
    /// `widgets.icon_button(glyph, tooltip, ...)`.
    Icon {
        id: &'static str,
        glyph: &'static str,
        tip: &'static str,
    },
    /// `layout.addStretch(1)` in a line.
    Stretch,
    /// A `QLabel` the dialog writes into as it runs: the pipeline sentence, and the answer a Test
    /// button gives.
    Live {
        id: &'static str,
        wrap: bool,
        hint: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// `None` is `addRow(widget)`: the field spans both columns.
    pub label: Option<Node>,
    pub field: Node,
}

impl Node {
    /// Calls `f` on this node and every node under it, in tree order.
    pub fn walk(&self, f: &mut impl FnMut(&Node)) {
        f(self);
        match self {
            Node::Group { body, .. } => body.walk(f),
            Node::Form(rows) => {
                for r in *rows {
                    if let Some(l) = &r.label {
                        l.walk(f);
                    }
                    r.field.walk(f);
                }
            }
            Node::Column(items) | Node::Line(items) => {
                for n in *items {
                    n.walk(f);
                }
            }
            _ => {}
        }
    }

    /// The id of a control (`None` for layouts and plain labels).
    pub fn id(&self) -> Option<&'static str> {
        match self {
            Node::Check { id, .. }
            | Node::Double { id, .. }
            | Node::Int { id, .. }
            | Node::Combo { id, .. }
            | Node::EditCombo { id, .. }
            | Node::Text { id, .. }
            | Node::Multiline { id, .. }
            | Node::Checks { id, .. }
            | Node::Button { id, .. }
            | Node::Pill { id, .. }
            | Node::Icon { id, .. }
            | Node::Live { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// The control with `id` among `pages`.
pub fn find(pages: &[(&'static str, Node)], id: &str) -> Option<Node> {
    let mut found = None;
    for (_, page) in pages {
        page.walk(&mut |n| {
            if found.is_none() && n.id() == Some(id) {
                found = Some(*n);
            }
        });
    }
    found
}
