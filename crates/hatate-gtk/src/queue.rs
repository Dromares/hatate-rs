//! The Queue: a `ColumnView` over a `gio::ListStore` of synthetic rows, sorted
//! through the view's own sorter and selected through a `MultiSelection`.

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gio, glib};
use std::cell::{Cell, RefCell};

/// Match status of a queue row. The discriminant is the sort key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Good = 0,
    Review = 1,
    NotFound = 2,
    Error = 3,
    Unsearched = 4,
}

impl Status {
    pub const ALL: [Status; 5] = [
        Status::Good,
        Status::Review,
        Status::NotFound,
        Status::Error,
        Status::Unsearched,
    ];

    pub fn from_u32(n: u32) -> Status {
        Self::ALL[(n as usize).min(4)]
    }

    /// Glyph and word, as in the Ryoku mockup's status column.
    pub fn text(self) -> &'static str {
        match self {
            Status::Good => "\u{25CF} Good",
            Status::Review => "\u{25D0} Review",
            Status::NotFound => "\u{25CB} Not found",
            Status::Error => "\u{00D7} Error",
            Status::Unsearched => "\u{00B7} Unsearched",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Status::Good => "status-good",
            Status::Review => "status-review",
            Status::NotFound => "status-notfound",
            Status::Error => "status-error",
            Status::Unsearched => "status-unsearched",
        }
    }
}

mod imp {
    use super::*;

    #[derive(glib::Properties, Default)]
    #[properties(wrapper_type = super::RowObject)]
    pub struct RowObject {
        #[property(get, set)]
        filename: RefCell<String>,
        #[property(get, set)]
        status: Cell<u32>,
        #[property(get, set)]
        matches: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RowObject {
        const NAME: &'static str = "HatateRowObject";
        type Type = super::RowObject;
    }

    #[glib::derived_properties]
    impl ObjectImpl for RowObject {}
}

glib::wrapper! {
    /// One queue row.
    pub struct RowObject(ObjectSubclass<imp::RowObject>);
}

impl RowObject {
    pub fn new(filename: &str, status: Status, matches: u32) -> Self {
        glib::Object::builder()
            .property("filename", filename)
            .property("status", status as u32)
            .property("matches", matches)
            .build()
    }
}

/// `n` deterministic synthetic rows (no randomness, so runs are comparable).
pub fn build_store(n: u32) -> gio::ListStore {
    let store = gio::ListStore::new::<RowObject>();
    let mut seed = 0x9E37_79B9_u32;
    let rows: Vec<RowObject> = (0..n)
        .map(|i| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let status = Status::from_u32((seed >> 24) % 5);
            let matches = match status {
                Status::Good | Status::Review => 1 + (seed >> 16) % 14,
                _ => 0,
            };
            // 7919 is prime, so for n = 5,000 this is a shuffled permutation of the
            // names and the first sort click visibly reorders.
            RowObject::new(
                &format!("sample_{:04}.png", (i * 7919) % n.max(1)),
                status,
                matches,
            )
        })
        .collect();
    store.extend_from_slice(&rows);
    store
}

fn text_column(
    title: &str,
    width: i32,
    expand: bool,
    sorter: Option<gtk4::Sorter>,
    text: impl Fn(&RowObject) -> (String, Option<&'static str>) + 'static,
) -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let label = gtk4::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let Some(row) = item.item().and_downcast::<RowObject>() else {
            return;
        };
        let Some(label) = item.child().and_downcast::<gtk4::Label>() else {
            return;
        };
        let (s, class) = text(&row);
        label.set_text(&s);
        // Rows are recycled: drop the previous status class before adding one.
        for c in StatusClasses::ALL {
            label.remove_css_class(c);
        }
        if let Some(c) = class {
            label.add_css_class(c);
        }
    });
    let col = gtk4::ColumnViewColumn::new(Some(title), Some(factory));
    col.set_fixed_width(width);
    col.set_expand(expand);
    col.set_resizable(true);
    col.set_sorter(sorter.as_ref());
    col
}

struct StatusClasses;
impl StatusClasses {
    const ALL: [&'static str; 5] = [
        "status-good",
        "status-review",
        "status-notfound",
        "status-error",
        "status-unsearched",
    ];
}

fn thumb_column() -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        // Placeholder: the real Queue will bind a decoded thumbnail here.
        let thumb = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        thumb.add_css_class("thumb");
        thumb.set_size_request(32, 32);
        thumb.set_halign(gtk4::Align::Start);
        item.set_child(Some(&thumb));
    });
    let col = gtk4::ColumnViewColumn::new(Some("THUMB"), Some(factory));
    col.set_fixed_width(72);
    col
}

fn prop_expr(name: &str) -> gtk4::PropertyExpression {
    gtk4::PropertyExpression::new(RowObject::static_type(), None::<gtk4::Expression>, name)
}

/// The Queue view and the handles the spike drives from the outside.
pub struct Queue {
    pub root: gtk4::Box,
    pub view: gtk4::ColumnView,
    pub scroller: gtk4::ScrolledWindow,
    pub selection: gtk4::MultiSelection,
    columns: Vec<gtk4::ColumnViewColumn>,
}

impl Queue {
    pub fn new(store: &gio::ListStore) -> Self {
        let view = gtk4::ColumnView::new(None::<gtk4::SelectionModel>);
        view.add_css_class("queue");
        view.set_show_row_separators(false);
        view.set_enable_rubberband(true);
        // Default `All` makes Tab walk every row (measured: 60 presses, never leaves a
        // 5,000-row list). `Item` makes the list one Tab stop; arrows move between rows.
        view.set_tab_behavior(gtk4::ListTabBehavior::Item);
        view.set_vexpand(true);

        let filename_sorter = gtk4::StringSorter::new(Some(prop_expr("filename")));
        let status_sorter = gtk4::NumericSorter::new(Some(prop_expr("status")));
        let matches_sorter = gtk4::NumericSorter::new(Some(prop_expr("matches")));

        let columns = vec![
            thumb_column(),
            text_column("FILE", 260, true, Some(filename_sorter.upcast()), |r| {
                (r.filename(), None)
            }),
            text_column("STATUS", 180, false, Some(status_sorter.upcast()), |r| {
                let s = Status::from_u32(r.status());
                (s.text().to_string(), Some(s.css_class()))
            }),
            text_column("MATCHES", 110, false, Some(matches_sorter.upcast()), |r| {
                (r.matches().to_string(), None)
            }),
        ];
        for c in &columns {
            view.append_column(c);
        }

        // The ColumnView exposes the combined sorter of the clicked header.
        let sorted = gtk4::SortListModel::new(Some(store.clone()), view.sorter());
        let selection = gtk4::MultiSelection::new(Some(sorted));
        view.set_model(Some(&selection));

        let scroller = gtk4::ScrolledWindow::new();
        scroller.set_child(Some(&view));
        scroller.set_vexpand(true);
        scroller.add_css_class("queue-frame");

        let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        root.append(&scroller);
        Queue {
            root,
            view,
            scroller,
            selection,
            columns,
        }
    }

    /// Select rows `a..=b` of the sorted model, as a Shift-click range would.
    pub fn select_range(&self, a: u32, b: u32) {
        self.selection.unselect_all();
        self.selection.select_range(a, b - a + 1, true);
    }

    /// Sort by a column title ("FILE", "STATUS", "MATCHES").
    pub fn sort_by(&self, title: &str, descending: bool) -> bool {
        let Some(col) = self
            .columns
            .iter()
            .find(|c| c.title().as_deref() == Some(title))
        else {
            return false;
        };
        let order = if descending {
            gtk4::SortType::Descending
        } else {
            gtk4::SortType::Ascending
        };
        self.view.sort_by_column(Some(col), order);
        true
    }

    pub fn selected_count(&self) -> u64 {
        self.selection.selection().size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_has_5000_unique_rows() {
        use std::collections::HashSet;
        let store = build_store(5000);
        assert_eq!(store.n_items(), 5000);
        let names: HashSet<String> = (0..5000)
            .filter_map(|i| store.item(i).and_downcast::<RowObject>())
            .map(|r| r.filename())
            .collect();
        assert_eq!(names.len(), 5000);
    }

    #[test]
    fn only_matched_statuses_have_matches() {
        let store = build_store(500);
        for i in 0..500 {
            let r = store.item(i).and_downcast::<RowObject>().unwrap();
            let matched = matches!(Status::from_u32(r.status()), Status::Good | Status::Review);
            assert_eq!(r.matches() > 0, matched);
        }
    }

    #[test]
    fn status_sort_key_matches_declaration_order() {
        for (i, s) in Status::ALL.iter().enumerate() {
            assert_eq!(*s as usize, i);
            assert_eq!(Status::from_u32(i as u32), *s);
        }
    }
}
