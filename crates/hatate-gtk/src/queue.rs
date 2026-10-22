//! The Queue: a `ColumnView` over a `gio::ListStore` of synthetic rows, sorted
//! through the view's own sorter and selected through a `MultiSelection`.

use crate::thumbs::ThumbCache;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Match status of a queue row. The discriminant is the sort key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Good = 0,
    Review = 1,
    NotFound = 2,
    Error = 3,
    Unsearched = 4,
    Flagged = 5,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Good,
        Status::Review,
        Status::NotFound,
        Status::Error,
        Status::Unsearched,
        Status::Flagged,
    ];

    pub fn from_u32(n: u32) -> Status {
        Self::ALL[(n as usize).min(5)]
    }

    /// The glyph, drawn in the `Ryoku Status` face (see `fonts`).
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Good => "\u{25CF}",
            Status::Review => "\u{25D0}",
            Status::NotFound => "\u{25CB}",
            Status::Error => "\u{00D7}",
            Status::Unsearched => "\u{00B7}",
            Status::Flagged => "\u{25B2}",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Status::Good => "Good",
            Status::Review => "Review",
            Status::NotFound => "Not found",
            Status::Error => "Error",
            Status::Unsearched => "Unsearched",
            Status::Flagged => "Flagged",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Status::Good => "status-good",
            Status::Review => "status-review",
            Status::NotFound => "status-notfound",
            Status::Error => "status-error",
            Status::Unsearched => "status-unsearched",
            Status::Flagged => "status-flagged",
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
        /// The row's number: the thumbnail file it shows.
        #[property(get, set)]
        id: Cell<u32>,
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
    pub fn new(id: u32, filename: &str, status: Status, matches: u32) -> Self {
        glib::Object::builder()
            .property("id", id)
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
            let status = Status::from_u32((seed >> 24) % 6);
            let matches = match status {
                Status::Good | Status::Review | Status::Flagged => 1 + (seed >> 16) % 14,
                _ => 0,
            };
            // 7919 is prime, so for n = 5,000 this is a shuffled permutation of the
            // names and the first sort click visibly reorders.
            let name = (i * 7919) % n.max(1);
            RowObject::new(i, &format!("sample_{name:04}.png"), status, matches)
        })
        .collect();
    store.extend_from_slice(&rows);
    store
}

fn plain_column(
    title: &str,
    width: i32,
    expand: bool,
    sorter: Option<gtk4::Sorter>,
    text: impl Fn(&RowObject) -> String + 'static,
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
        let (Some(row), Some(label)) = (
            item.item().and_downcast::<RowObject>(),
            item.child().and_downcast::<gtk4::Label>(),
        ) else {
            return;
        };
        label.set_text(&text(&row));
    });
    let col = gtk4::ColumnViewColumn::new(Some(title), Some(factory));
    col.set_fixed_width(width);
    col.set_expand(expand);
    col.set_resizable(true);
    col.set_sorter(sorter.as_ref());
    col
}

/// The status column: the glyph in its own label (drawn in the `Ryoku Status` face) next to the
/// word in the mono face, one weight/ink tier per status via a class on the cell box.
fn status_column(sorter: gtk4::Sorter) -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let cell = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let glyph = gtk4::Label::new(None);
        glyph.add_css_class("status-glyph");
        let word = gtk4::Label::new(None);
        word.add_css_class("status-word");
        word.set_xalign(0.0);
        cell.append(&glyph);
        cell.append(&word);
        item.set_child(Some(&cell));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let (Some(row), Some(cell)) = (
            item.item().and_downcast::<RowObject>(),
            item.child().and_downcast::<gtk4::Box>(),
        ) else {
            return;
        };
        let status = Status::from_u32(row.status());
        if let (Some(g), Some(w)) = (
            cell.first_child().and_downcast::<gtk4::Label>(),
            cell.last_child().and_downcast::<gtk4::Label>(),
        ) {
            g.set_text(status.glyph());
            w.set_text(status.word());
        }
        // Rows are recycled: drop the previous status class before adding one.
        for s in Status::ALL {
            cell.remove_css_class(s.css_class());
        }
        cell.add_css_class(status.css_class());
    });
    let col = gtk4::ColumnViewColumn::new(Some("STATUS"), Some(factory));
    col.set_fixed_width(180);
    col.set_resizable(true);
    col.set_sorter(Some(&sorter));
    col
}

/// The thumbnail column. With a cache, each bound row shows a decoded image (or its empty frame
/// until the decode lands); without, the frame stays empty, which is the DAN-1967 baseline.
fn thumb_column(thumbs: Option<ThumbCache>) -> gtk4::ColumnViewColumn {
    let factory = gtk4::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
        let pic = gtk4::Picture::new();
        pic.add_css_class("thumb");
        pic.set_size_request(32, 32);
        pic.set_content_fit(gtk4::ContentFit::Cover);
        pic.set_halign(gtk4::Align::Start);
        pic.set_valign(gtk4::Align::Center);
        pic.set_can_shrink(false);
        pic.set_focusable(false);
        item.set_child(Some(&pic));
    });
    if let Some(cache) = thumbs {
        let c = cache.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
            if let (Some(row), Some(pic)) = (
                item.item().and_downcast::<RowObject>(),
                item.child().and_downcast::<gtk4::Picture>(),
            ) {
                c.bind(row.id(), &pic);
            }
        });
        factory.connect_unbind(move |_, item| {
            let item = item.downcast_ref::<gtk4::ListItem>().expect("ListItem");
            if let Some(row) = item.item().and_downcast::<RowObject>() {
                cache.unbind(row.id());
            }
        });
    }
    let col = gtk4::ColumnViewColumn::new(Some("THUMB"), Some(factory));
    col.set_fixed_width(72);
    col
}

fn prop_expr(name: &str) -> gtk4::PropertyExpression {
    gtk4::PropertyExpression::new(RowObject::static_type(), None::<gtk4::Expression>, name)
}

/// The Queue view and the handles the spike drives from the outside.
#[derive(Clone)]
pub struct Queue {
    pub root: gtk4::Box,
    pub view: gtk4::ColumnView,
    pub scroller: gtk4::ScrolledWindow,
    pub selection: gtk4::MultiSelection,
    filter: gtk4::CustomFilter,
    status_filter: Rc<Cell<Option<u32>>>,
    columns: Vec<gtk4::ColumnViewColumn>,
}

impl Queue {
    pub fn new(store: &gio::ListStore, thumbs: Option<ThumbCache>) -> Self {
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
            thumb_column(thumbs),
            plain_column("FILE", 260, true, Some(filename_sorter.upcast()), |r| {
                r.filename()
            }),
            status_column(status_sorter.upcast()),
            plain_column("MATCHES", 110, false, Some(matches_sorter.upcast()), |r| {
                r.matches().to_string()
            }),
        ];
        for c in &columns {
            view.append_column(c);
        }

        // Status filter (the `Status: All` dropdown): None = all.
        let status_filter: Rc<Cell<Option<u32>>> = Rc::new(Cell::new(None));
        let sf = status_filter.clone();
        let filter = gtk4::CustomFilter::new(move |o| {
            let want = sf.get();
            want.is_none_or(|w| {
                o.downcast_ref::<RowObject>()
                    .is_some_and(|r| r.status() == w)
            })
        });
        let filtered = gtk4::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        // The ColumnView exposes the combined sorter of the clicked header.
        let sorted = gtk4::SortListModel::new(Some(filtered), view.sorter());
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
            filter,
            status_filter,
            columns,
        }
    }

    /// Show only rows of `status` (None = all).
    pub fn set_status_filter(&self, status: Option<Status>) {
        self.status_filter.set(status.map(|s| s as u32));
        self.filter.changed(gtk4::FilterChange::Different);
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
        // GTK keeps the focused item in view through a re-sort, which can leave the
        // viewport a thousand rows down. A sort click should show the top of the result.
        self.view
            .scroll_to(0, None, gtk4::ListScrollFlags::NONE, None);
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
            let matched = matches!(
                Status::from_u32(r.status()),
                Status::Good | Status::Review | Status::Flagged
            );
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
