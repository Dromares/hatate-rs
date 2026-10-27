//! The Queue table (`gui/image_table_model.py`, `gui/table_delegates.py` and the table half of
//! `gui/main_window.py` at `76c85c4`; DAN-2217): a `ColumnView` over the window's `QueueModel`.
//!
//! The view owns no entries. Its `gio::ListStore` holds one [`RowObject`] per row **on screen**
//! (the rows that pass the filter, in the queue's order), each carrying the entry's id and a
//! snapshot of its twelve cells. What a cell says, how it is inked and how a column sorts are
//! `hatate_core::table_cells`; this file only draws them. A row number is a position on screen, and
//! every act on a selection resolves it through [`QueueView::selected_ids`], so a row a filter hides
//! can never be reached by one (the safety property `ImageTableModel.entry_at` carries).
//!
//! Sorting reorders the queue itself, as Python's `self.entries.sort(...)` does, so the order is
//! what a saved session keeps. The view's own sorters are no-ops: they exist so the headers are
//! clickable and draw the chevron, and the click is turned into Python's cycle (ascending,
//! descending, original) by [`QueueView::on_header_clicked`].

use crate::tokens::Scheme;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, gio, pango};
use hatate_core::entry_filter::EntryFilter;
use hatate_core::queue::{EntryId, QueueModel};
use hatate_core::status_tier::{GlyphFont, Ink, StatusTier, UpscaleTier};
use hatate_core::table_cells::{self, CellInk, Chip, Column};
use hatate_core::thumb::{QUEUE_ROW_HEIGHT, THUMB_COLUMN_SIZE, ThumbBook, ThumbKey, ThumbState};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// The classes a cell can carry, all removed before a recycled cell is given its own.
const CELL_CLASSES: [&str; 8] = [
    "ink-45",
    "ink-65",
    "ink-100",
    "tier-bold",
    "tier-italic",
    "chip-boxed",
    "cell-mono",
    "cell-mono-bold",
];

/// What one cell of one row shows (`data()` for the display, foreground, font and tooltip roles).
#[derive(Debug, Clone, PartialEq)]
pub struct CellView {
    /// Pango markup; the text is escaped.
    pub markup: String,
    /// The plain text, for the accessible name and the tests.
    pub text: String,
    pub ink: Option<CellInk>,
    pub bold: bool,
    pub italic: bool,
    pub boxed: bool,
    /// Drawn in the mono stack (`Size diff.` and `Upscale`, whose glyph the sans face lacks).
    pub mono: bool,
    pub tooltip: Option<String>,
}

fn ink_of(ink: Ink) -> CellInk {
    match ink {
        Ink::Ink65 => CellInk::Ink65,
        Ink::Ink100 => CellInk::Ink100,
    }
}

/// A glyph, drawn in the face the tier names, then the text.
fn glyph_markup(glyph: char, font: GlyphFont, text: &str) -> String {
    let g = glib::markup_escape_text(&glyph.to_string());
    let t = glib::markup_escape_text(text);
    let glyph = match font {
        GlyphFont::RyokuStatus => format!("<span font_family=\"Ryoku Status\">{g}</span>"),
        GlyphFont::Mono => g.to_string(),
    };
    if text.is_empty() {
        glyph
    } else {
        format!("{glyph} {t}")
    }
}

/// The cell `column` of `entry` as the table draws it.
pub fn cell_view(entry: &hatate_core::model::Entry, column: Column) -> CellView {
    let text = table_cells::cell_text(entry, column).unwrap_or_default();
    let mut view = CellView {
        markup: glib::markup_escape_text(&text).to_string(),
        text: text.clone(),
        ink: table_cells::foreground(entry, column),
        bold: false,
        italic: table_cells::italic(entry, column),
        boxed: false,
        mono: false,
        tooltip: table_cells::tooltip(entry, column),
    };
    match (column, table_cells::chip(entry, column)) {
        (Column::Status, _) => {
            let tier = StatusTier::of(entry);
            view.markup = glyph_markup(tier.glyph(), tier.glyph_font(), tier.chip_text());
            view.text = format!("{} {}", tier.glyph(), tier.chip_text());
            view.ink = Some(ink_of(tier.ink()));
            view.bold = tier.bold();
            view.italic = tier.italic();
            view.boxed = tier.boxed();
        }
        (Column::Sent, Some(Chip::Sent(tier))) => {
            if let Some(glyph) = tier.glyph() {
                view.markup = glyph_markup(glyph, tier.glyph_font(), tier.cell_text());
                view.text = format!("{glyph} {}", tier.cell_text());
            }
            view.ink = Some(CellInk::Ink65);
            view.bold = false;
        }
        (Column::Similarity, Some(Chip::Estimated)) => {
            view.ink = Some(CellInk::Ink65);
        }
        (Column::SizeDelta, _) => {
            view.mono = true;
            view.bold = view.ink == Some(CellInk::Ink100);
        }
        (Column::Upscale, _) => {
            view.mono = true;
            view.bold = UpscaleTier::of(entry) == UpscaleTier::Flagged;
        }
        _ => {}
    }
    view
}

/// All twelve cells of a row.
fn snapshot(entry: &hatate_core::model::Entry) -> Vec<CellView> {
    Column::ALL.iter().map(|&c| cell_view(entry, c)).collect()
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RowObject {
        pub id: Cell<u64>,
        pub cells: RefCell<Vec<CellView>>,
        pub thumb: Cell<Option<ThumbState>>,
        pub texture: RefCell<Option<gdk::Texture>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RowObject {
        const NAME: &'static str = "HatateQueueRow";
        type Type = super::RowObject;
    }

    impl ObjectImpl for RowObject {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::OnceLock<Vec<glib::subclass::Signal>> =
                std::sync::OnceLock::new();
            SIGNALS.get_or_init(|| vec![glib::subclass::Signal::builder("thumb-changed").build()])
        }
    }
}

glib::wrapper! {
    /// One row on screen: the entry's id and what its cells say.
    pub struct RowObject(ObjectSubclass<imp::RowObject>);
}

impl RowObject {
    fn new(
        id: EntryId,
        entry: &hatate_core::model::Entry,
        thumb: (ThumbState, Option<gdk::Texture>),
    ) -> RowObject {
        let row: RowObject = glib::Object::new();
        row.imp().id.set(id.0);
        row.imp().cells.replace(snapshot(entry));
        row.imp().thumb.set(Some(thumb.0));
        row.imp().texture.replace(thumb.1);
        row
    }

    /// The thumbnail cell changed state: whatever draws it repaints.
    fn set_thumb(&self, state: ThumbState, texture: Option<gdk::Texture>) {
        self.imp().thumb.set(Some(state));
        self.imp().texture.replace(texture);
        self.emit_by_name::<()>("thumb-changed", &[]);
    }

    pub fn id(&self) -> EntryId {
        EntryId(self.imp().id.get())
    }

    pub fn cell(&self, column: Column) -> Option<CellView> {
        self.imp().cells.borrow().get(column.index()).cloned()
    }
}

/// Puts `cell` into a label, taking every class a recycled one carried off first.
fn render(label: &gtk::Label, cell: &CellView) {
    for c in CELL_CLASSES {
        label.remove_css_class(c);
    }
    label.set_markup(&cell.markup);
    label.set_tooltip_text(cell.tooltip.as_deref());
    if let Some(ink) = cell.ink {
        label.add_css_class(match ink {
            CellInk::Ink45 => "ink-45",
            CellInk::Ink65 => "ink-65",
            CellInk::Ink100 => "ink-100",
        });
    }
    if cell.bold && !cell.mono {
        label.add_css_class("tier-bold");
    }
    if cell.italic {
        label.add_css_class("tier-italic");
    }
    if cell.boxed {
        label.add_css_class("chip-boxed");
    }
    if cell.mono {
        label.add_css_class(if cell.bold {
            "cell-mono-bold"
        } else {
            "cell-mono"
        });
    }
}

/// Which row each drawn cell widget belongs to (by the widget's address), so a pointer position
/// can be turned back into a row.
type CellRows = Rc<RefCell<HashMap<usize, EntryId>>>;

fn text_factory(column: Column, cells: CellRows) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.add_css_class("queue-cell");
        item.set_child(Some(&label));
    });
    let bound_cells = cells.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(row), Some(label)) = (
            item.item().and_downcast::<RowObject>(),
            item.child().and_downcast::<gtk::Label>(),
        ) else {
            return;
        };
        if let Some(cell) = row.cell(column) {
            render(&label, &cell);
        }
        bound_cells
            .borrow_mut()
            .insert(label.as_ptr() as usize, row.id());
    });
    factory.connect_unbind(move |_, item| {
        if let Some(label) = item.downcast_ref::<gtk::ListItem>().and_then(|i| i.child()) {
            cells.borrow_mut().remove(&(label.as_ptr() as usize));
        }
    });
    factory
}

/// Called with the id of a row that has just been drawn (bound to a cell), so the window can ask
/// for its thumbnail (`_schedule_viewport_thumbnails`).
type BoundCallback = Rc<RefCell<Option<Rc<dyn Fn(EntryId)>>>>;

/// Puts a row's thumbnail state into its cell: the picture, or the loading halftone, or the
/// failed frame with its path-free tooltip. The cell keeps its rect in every state (call 4).
fn paint_thumb(frame: &gtk::Picture, row: &RowObject) {
    frame.remove_css_class("thumb-loading");
    frame.remove_css_class("thumb-failed");
    match row.imp().thumb.get() {
        Some(ThumbState::Loaded) => {
            frame.set_paintable(row.imp().texture.borrow().as_ref());
            frame.set_tooltip_text(None);
        }
        Some(ThumbState::Failed) => {
            frame.set_paintable(None::<&gdk::Paintable>);
            frame.add_css_class("thumb-failed");
            frame.set_tooltip_text(Some(hatate_core::thumb::FAILED_TOOLTIP));
        }
        _ => {
            frame.set_paintable(None::<&gdk::Paintable>);
            frame.add_css_class("thumb-loading");
            frame.set_tooltip_text(None);
        }
    }
}

/// The thumbnail cell: a 40 px frame, loading or failed until the worker's texture lands.
fn thumb_factory(bound: BoundCallback, cells: CellRows) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    // The `thumb-changed` handler each recycled cell holds, so unbinding can drop it.
    let handlers: Rc<RefCell<HashMap<usize, (RowObject, glib::SignalHandlerId)>>> = Rc::default();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let frame = gtk::Picture::new();
        let side = THUMB_COLUMN_SIZE as i32;
        frame.set_size_request(side, side);
        frame.set_can_shrink(true);
        frame.set_halign(gtk::Align::Start);
        frame.set_valign(gtk::Align::Center);
        frame.add_css_class("thumb");
        item.set_child(Some(&frame));
    });
    let held = handlers.clone();
    let unbound_cells = cells.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(row), Some(frame)) = (
            item.item().and_downcast::<RowObject>(),
            item.child().and_downcast::<gtk::Picture>(),
        ) else {
            return;
        };
        paint_thumb(&frame, &row);
        let weak = frame.downgrade();
        let id = row.connect_local("thumb-changed", false, {
            let row = row.downgrade();
            move |_| {
                if let (Some(frame), Some(row)) = (weak.upgrade(), row.upgrade()) {
                    paint_thumb(&frame, &row);
                }
                None
            }
        });
        held.borrow_mut()
            .insert(item.as_ptr() as usize, (row.clone(), id));
        cells.borrow_mut().insert(frame.as_ptr() as usize, row.id());
        let callback = bound.borrow().clone();
        if let Some(callback) = callback {
            callback(row.id());
        }
    });
    factory.connect_unbind(move |_, item| {
        if let Some((row, id)) = handlers.borrow_mut().remove(&(item.as_ptr() as usize)) {
            row.disconnect(id);
        }
        if let Some(frame) = item.downcast_ref::<gtk::ListItem>().and_then(|i| i.child()) {
            unbound_cells
                .borrow_mut()
                .remove(&(frame.as_ptr() as usize));
        }
    });
    factory
}

/// The sorter every column carries: it orders nothing, because the queue is sorted in place.
fn inert_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal)
}

/// Python's header-click cycle: a new column starts ascending, ascending goes to descending,
/// descending goes back to the original (insertion) order.
pub fn next_sort(current: Option<(Column, bool)>, clicked: Column) -> Option<(Column, bool)> {
    match current {
        Some((c, true)) if c == clicked => Some((clicked, false)),
        Some((c, false)) if c == clicked => None,
        _ => Some((clicked, true)),
    }
}

/// What runs when the selection changes.
type SelectionCallback = Rc<dyn Fn(&[EntryId])>;

/// The Queue table and the state `MainWindow` keeps for it.
pub struct QueueView {
    pub root: gtk::Box,
    pub scroller: gtk::ScrolledWindow,
    pub view: gtk::ColumnView,
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    columns: Vec<gtk::ColumnViewColumn>,
    queue: Rc<RefCell<QueueModel>>,
    filter: RefCell<EntryFilter>,
    /// `self._sort_column`, `self._sort_ascending`.
    sort: Cell<Option<(Column, bool)>>,
    /// `self._entry_sequence`: insertion order, so the cycle can restore it.
    sequence: RefCell<HashMap<EntryId, u64>>,
    next_sequence: Cell<u64>,
    /// Set while the view draws a sort indicator itself, so its own `changed` is not a click.
    drawing_indicator: Cell<bool>,
    on_selection: RefCell<Option<SelectionCallback>>,
    scheme: Cell<Scheme>,
    /// `self._thumb_icon_cache`: what each file's thumbnail is, filed by path and decode size.
    thumbs: RefCell<ThumbBook<gdk::Texture>>,
    on_row_bound: BoundCallback,
    /// Called after every `rebuild`, so the filter bar can follow the list.
    on_rebuilt: RefCell<Option<Rc<dyn Fn()>>>,
    cell_rows: CellRows,
}

/// The Thumb column's width: the 40 px picture plus room for the letter-spaced `THUMB` title,
/// which was clipped in a 72 px column at the minimum window size (DAN-1999 F6; 56 px of title in
/// a header cell 24 px narrower than the column).
const THUMB_COLUMN_WIDTH: i32 = THUMB_COLUMN_SIZE as i32 + 44;

/// Least default width of a column whose letter-spaced title is wider than Python's minimum
/// leaves room for: a GTK header cell is 25 px narrower than its column (Qt's is not), so the
/// ported `DEFAULT_COLUMN_MINIMUMS` clip `ENGINE` by a pixel and `SIMILARITY`, `TAGS`, `CACHE` and `REVIEWED`
/// by more. Measured at the minimum window size by the header-title scenario.
const HEADER_TITLE_FLOORS: [(Column, i32); 6] = [
    (Column::Engine, 84),
    (Column::Tags, 68),
    (Column::Cache, 80),
    (Column::Similarity, 116),
    (Column::SizeDelta, 104),
    (Column::Reviewed, 106),
];

impl QueueView {
    pub fn new(queue: Rc<RefCell<QueueModel>>) -> Rc<QueueView> {
        let store = gio::ListStore::new::<RowObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));

        let view = gtk::ColumnView::new(None::<gtk::SelectionModel>);
        view.add_css_class("queue");
        view.set_show_row_separators(false);
        view.set_enable_rubberband(true);
        // One Tab stop for the list; the arrows move between rows (a 5,000-row list otherwise
        // takes thousands of Tab presses to leave).
        view.set_tab_behavior(gtk::ListTabBehavior::Item);
        view.set_vexpand(true);
        view.set_model(Some(&selection));

        let bound: BoundCallback = Rc::default();
        let cell_rows: CellRows = Rc::default();
        let widths = table_cells::default_column_widths(1000, THUMB_COLUMN_WIDTH);
        let mut columns = Vec::new();
        for column in Column::ALL {
            let factory = match column {
                Column::Thumb => thumb_factory(bound.clone(), cell_rows.clone()),
                _ => text_factory(column, cell_rows.clone()),
            };
            let col = gtk::ColumnViewColumn::new(Some(column.title()), Some(factory));
            col.set_resizable(column != Column::Thumb);
            match column {
                Column::Thumb => col.set_fixed_width(THUMB_COLUMN_WIDTH),
                Column::Upscale => col.set_expand(true),
                _ => {
                    if let Some((_, w)) = widths.iter().find(|(c, _)| *c == column) {
                        let floor = HEADER_TITLE_FLOORS
                            .iter()
                            .find(|(c, _)| *c == column)
                            .map_or(0, |(_, f)| *f);
                        col.set_fixed_width((*w).max(floor));
                    }
                }
            }
            if column != Column::Thumb {
                col.set_sorter(Some(&inert_sorter()));
            }
            view.append_column(&col);
            columns.push(col);
        }

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&view));
        scroller.set_vexpand(true);
        // The scrollbar is persistent at 5,000 rows (design gate 2.4): an overlay one is a
        // 2 px line that a pointer has to find.
        scroller.set_overlay_scrolling(false);
        scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scroller.add_css_class("queue-frame");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&scroller);

        let this = Rc::new(QueueView {
            root,
            scroller,
            view,
            store,
            selection,
            columns,
            queue,
            filter: RefCell::new(EntryFilter::default()),
            sort: Cell::new(None),
            sequence: RefCell::new(HashMap::new()),
            next_sequence: Cell::new(0),
            drawing_indicator: Cell::new(false),
            on_selection: RefCell::new(None),
            scheme: Cell::new(Scheme::Dark),
            thumbs: RefCell::new(ThumbBook::default()),
            on_row_bound: bound,
            on_rebuilt: RefCell::new(None),
            cell_rows,
        });

        let weak = Rc::downgrade(&this);
        if let Some(sorter) = this.view.sorter() {
            sorter.connect_changed(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    this.on_view_sorter_changed();
                }
            });
        }
        let weak = Rc::downgrade(&this);
        this.selection.connect_selection_changed(move |_, _, _| {
            if let Some(this) = weak.upgrade() {
                this.notify_selection();
            }
        });
        this
    }

    /// The scheme the window draws in (the cells' inks come from the sheet, so this only tells
    /// a test which one it is looking at).
    pub fn set_scheme(&self, scheme: Scheme) {
        self.scheme.set(scheme);
    }

    /// Called with the selected ids whenever the selection changes (`_on_selection_changed`).
    pub fn connect_selection(&self, f: impl Fn(&[EntryId]) + 'static) {
        *self.on_selection.borrow_mut() = Some(Rc::new(f));
    }

    fn notify_selection(&self) {
        let cb = self.on_selection.borrow().clone();
        if let Some(cb) = cb {
            cb(&self.selected_ids());
        }
    }

    /// `ImageTableModel.set_filter`: changes what is shown, then restores the selection.
    pub fn set_filter(&self, filter: EntryFilter) {
        *self.filter.borrow_mut() = filter;
        self.rebuild();
    }

    pub fn filter(&self) -> EntryFilter {
        self.filter.borrow().clone()
    }

    /// `visible_count()`.
    pub fn visible_count(&self) -> usize {
        self.store.n_items() as usize
    }

    /// `total_count()`.
    pub fn total_count(&self) -> usize {
        self.queue.borrow().len()
    }

    /// The ids on screen, in screen order (`visible_entries`).
    pub fn visible_ids(&self) -> Vec<EntryId> {
        (0..self.store.n_items())
            .filter_map(|i| self.store.item(i).and_downcast::<RowObject>())
            .map(|r| r.id())
            .collect()
    }

    /// `row_of(entry)`: where an entry is on screen, `None` if the filter hides it.
    pub fn row_of(&self, id: EntryId) -> Option<u32> {
        (0..self.store.n_items()).find(|&i| {
            self.store
                .item(i)
                .and_downcast::<RowObject>()
                .is_some_and(|r| r.id() == id)
        })
    }

    /// The text of a cell as drawn, for the differential and the tests.
    pub fn cell(&self, row: u32, column: Column) -> Option<CellView> {
        self.store
            .item(row)
            .and_downcast::<RowObject>()
            .and_then(|r| r.cell(column))
    }

    /// `_selected_entries`: the ids of the selected rows, in screen order.
    pub fn selected_ids(&self) -> Vec<EntryId> {
        let bits = self.selection.selection();
        (0..self.store.n_items())
            .filter(|&i| bits.contains(i))
            .filter_map(|i| self.store.item(i).and_downcast::<RowObject>())
            .map(|r| r.id())
            .collect()
    }

    /// `select_table_row(row)`.
    pub fn select_row(&self, row: u32) {
        if row < self.store.n_items() {
            self.selection.unselect_all();
            self.selection.select_item(row, true);
        }
    }

    /// `_successor_surviving(ids_to_remove)`: the visible entry to land on if the selection is about
    /// to be removed: the next one down that survives, else the nearest above. `None` when the
    /// selection survives or there is nothing selected.
    pub fn successor_surviving(&self, removed: &[EntryId]) -> Option<EntryId> {
        let selected = self.selected_ids();
        let first = selected.iter().filter_map(|id| self.row_of(*id)).min()?;
        if selected.iter().any(|id| !removed.contains(id)) {
            return None;
        }
        let ids = self.visible_ids();
        let total = ids.len() as u32;
        (first + 1..total)
            .chain((0..first).rev())
            .map(|r| ids[r as usize])
            .find(|id| !removed.contains(id))
    }

    /// `_reselect`: selects exactly these ids, those still on screen.
    pub fn select_ids(&self, ids: &[EntryId]) {
        let bits = gtk::Bitset::new_empty();
        for id in ids {
            if let Some(row) = self.row_of(*id) {
                bits.add(row);
            }
        }
        self.selection
            .set_selection(&bits, &gtk::Bitset::new_range(0, self.store.n_items()));
    }

    /// `_refresh_table()`: rebuilds the rows from the queue (the filter applied, the queue's
    /// order kept) and restores the selection, which a reset drops.
    pub fn rebuild(&self) {
        let selected = self.selected_ids();
        let rows: Vec<RowObject> = {
            let queue = self.queue.borrow();
            let entries = queue.entries();
            self.filter
                .borrow()
                .apply_indices(entries)
                .into_iter()
                .filter_map(|i| {
                    let id = queue.id_at(i as i64)?;
                    let key = ThumbKey::of(entries[i].path.clone());
                    let thumb = self.thumbs.borrow_mut().get(&key);
                    Some(RowObject::new(id, &entries[i], thumb))
                })
                .collect()
        };
        self.store.splice(0, self.store.n_items(), &rows);
        self.select_ids(&selected);
        let hook = self.on_rebuilt.borrow().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Called after every `rebuild` (`_refresh_table` ends by rebuilding the filter menus and
    /// the readout).
    pub fn connect_rebuilt(&self, f: impl Fn() + 'static) {
        *self.on_rebuilt.borrow_mut() = Some(Rc::new(f));
    }

    /// `_refresh_entry_row(entry)`: repaints one row; nothing if it is not on screen (it may have
    /// been removed while work was in flight, or hidden by the filter).
    pub fn refresh_row(&self, id: EntryId) {
        let Some(pos) = self.row_of(id) else { return };
        let queue = self.queue.borrow();
        let Some(entry) = queue.entry(id) else { return };
        if let Some(row) = self.store.item(pos).and_downcast::<RowObject>() {
            row.imp().cells.replace(snapshot(entry));
        }
        self.store.items_changed(pos, 1, 1);
    }

    /// The row under a point of the view (`indexAt(pos)`): `None` over the header, the empty
    /// area below the last row or anything else.
    pub fn row_id_at(&self, x: f64, y: f64) -> Option<EntryId> {
        let cells = self.cell_rows.borrow();
        let mut widget = self.view.pick(x, y, gtk::PickFlags::DEFAULT)?;
        loop {
            if let Some(id) = cells.get(&(widget.as_ptr() as usize)) {
                return Some(*id);
            }
            if let Some(id) = widget
                .first_child()
                .and_then(|c| cells.get(&(c.as_ptr() as usize)))
            {
                return Some(*id);
            }
            if widget == *self.view.upcast_ref::<gtk::Widget>() {
                return None;
            }
            widget = widget.parent()?;
        }
    }

    /// Calls `f(row under the pointer, x, y)` on a right click, and `f(None, 0, 0)` from the
    /// keyboard (Menu, Shift+F10), where the menu opens over the first selected row.
    pub fn connect_context_menu(self: &Rc<Self>, f: impl Fn(Option<EntryId>, f64, f64) + 'static) {
        let f = Rc::new(f);
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (weak, g) = (Rc::downgrade(self), f.clone());
        click.connect_pressed(move |gesture, _, x, y| {
            let Some(this) = weak.upgrade() else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let hit = this.row_id_at(x, y);
            // Qt selects the row a right press lands on, unless it is already part of the
            // selection (so a menu over several selected rows keeps them).
            if let Some(id) = hit
                && !this.selected_ids().contains(&id)
            {
                this.select_ids(&[id]);
            }
            g(hit, x, y);
        });
        self.view.add_controller(click);

        let keys = gtk::ShortcutController::new();
        for trigger in ["Menu", "<Shift>F10"] {
            let g = f.clone();
            let shortcut = gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(trigger),
                Some(gtk::CallbackAction::new(move |_, _| {
                    g(None, 0.0, 0.0);
                    glib::Propagation::Stop
                })),
            );
            keys.add_shortcut(shortcut);
        }
        self.view.add_controller(keys);
    }

    /// Called with the id of every row as it is drawn, so its thumbnail can be asked for.
    pub fn connect_row_bound(&self, f: impl Fn(EntryId) + 'static) {
        *self.on_row_bound.borrow_mut() = Some(Rc::new(f));
    }

    /// Where a thumbnail stands: `Loading` for a file nobody has decoded yet.
    pub fn thumb_state(&self, id: EntryId) -> ThumbState {
        self.thumb_key(id)
            .map_or(ThumbState::Loading, |k| self.thumbs.borrow_mut().get(&k).0)
    }

    /// Whether the cache knows anything of this row's file (a decode, or that it failed).
    pub fn thumb_known(&self, id: EntryId) -> bool {
        self.thumb_key(id)
            .is_some_and(|k| self.thumbs.borrow().knows(&k))
    }

    fn thumb_key(&self, id: EntryId) -> Option<ThumbKey> {
        let queue = self.queue.borrow();
        queue.entry(id).map(|e| ThumbKey::of(e.path.clone()))
    }

    /// A decode finished (`Some`) or failed (`None`): files it and repaints the row if it is on
    /// screen.
    pub fn store_thumb(&self, id: EntryId, texture: Option<gdk::Texture>) {
        let Some(key) = self.thumb_key(id) else {
            return;
        };
        let state = match &texture {
            Some(t) => {
                self.thumbs.borrow_mut().loaded(key, t.clone());
                ThumbState::Loaded
            }
            None => {
                self.thumbs.borrow_mut().failed(key);
                ThumbState::Failed
            }
        };
        if let Some(pos) = self.row_of(id)
            && let Some(row) = self.store.item(pos).and_downcast::<RowObject>()
        {
            row.set_thumb(state, texture);
        }
    }

    /// `_thumb_icon_cache.clear()`.
    pub fn clear_thumbs(&self) {
        self.thumbs.borrow_mut().clear();
    }

    /// `_register_new_entries`: records insertion order for entries just added, so "original"
    /// can restore it later.
    pub fn register_new(&self, ids: &[EntryId]) {
        let mut seq = self.sequence.borrow_mut();
        for id in ids {
            seq.insert(*id, self.next_sequence.get());
            self.next_sequence.set(self.next_sequence.get() + 1);
        }
    }

    /// `_save_table_header_state`'s capture: the width and visibility of each column. A column
    /// that was never given a fixed width (the expanding Upscale one) reports the default.
    pub fn header_sections(&self) -> Vec<table_cells::HeaderSection> {
        self.columns
            .iter()
            .map(|c| table_cells::HeaderSection {
                width: match c.fixed_width() {
                    w if w > 0 => w,
                    _ => table_cells::DEFAULT_COLUMN_WIDTH,
                },
                hidden: !c.is_visible(),
            })
            .collect()
    }

    /// Applies a restored (already repaired) layout. Thumb keeps its fixed cell width and the
    /// expanding Upscale column keeps expanding, as in the default layout.
    pub fn apply_header_sections(&self, sections: &[table_cells::HeaderSection]) {
        for ((col, kind), section) in self.columns.iter().zip(Column::ALL).zip(sections) {
            col.set_visible(!section.hidden);
            if !matches!(kind, Column::Thumb | Column::Upscale) {
                col.set_fixed_width(section.width);
            }
        }
    }

    /// `(self._sort_column, self._sort_ascending)`.
    pub fn sort_state(&self) -> Option<(Column, bool)> {
        self.sort.get()
    }

    /// `_on_header_clicked(column)`.
    pub fn on_header_clicked(&self, column: Column) {
        self.sort.set(next_sort(self.sort.get(), column));
        self.apply_sort();
    }

    /// `_apply_sort()`: sorts the queue in place by the current column, redraws, and keeps the
    /// current row selected. As Python does, "the current row" is the top selected one and it is
    /// all that stays selected (`selectRow` replaces the selection the refresh restored).
    pub fn apply_sort(&self) {
        let current = self
            .selected_ids()
            .into_iter()
            .filter_map(|id| Some((self.row_of(id)?, id)))
            .min()
            .map(|(_, id)| id);
        {
            let mut queue = self.queue.borrow_mut();
            let order = match self.sort.get() {
                None => {
                    // The original order: by insertion sequence, unknown entries first (key 0).
                    let seq = self.sequence.borrow();
                    let mut order: Vec<usize> = (0..queue.len()).collect();
                    order.sort_by_key(|&i| {
                        queue
                            .id_at(i as i64)
                            .and_then(|id| seq.get(&id).copied())
                            .unwrap_or(0)
                    });
                    order
                }
                Some((column, ascending)) => {
                    table_cells::sorted_order(queue.entries(), column, !ascending)
                }
            };
            queue.reorder(&order);
        }
        self.draw_indicator();
        self.rebuild();
        if let Some(id) = current {
            self.select_ids(&[id]);
        }
    }

    /// The chevron on the sorted header, or none.
    fn draw_indicator(&self) {
        self.drawing_indicator.set(true);
        match self.sort.get() {
            Some((column, ascending)) => self.view.sort_by_column(
                self.columns.get(column.index()),
                if ascending {
                    gtk::SortType::Ascending
                } else {
                    gtk::SortType::Descending
                },
            ),
            None => self
                .view
                .sort_by_column(None::<&gtk::ColumnViewColumn>, gtk::SortType::Ascending),
        }
        self.drawing_indicator.set(false);
    }

    /// A click on a header makes the view change its own sorter; read which column and
    /// direction it chose and turn that into Python's cycle.
    fn on_view_sorter_changed(&self) {
        if self.drawing_indicator.get() {
            return;
        }
        let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>() else {
            return;
        };
        let Some(col) = sorter.primary_sort_column() else {
            return;
        };
        let Some(index) = self.columns.iter().position(|c| *c == col) else {
            return;
        };
        if let Some(column) = Column::from_index(index) {
            self.on_header_clicked(column);
        }
    }

    /// The row height the table is built to (`QUEUE_ROW_HEIGHT`).
    pub fn row_height() -> u32 {
        QUEUE_ROW_HEIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::model::{Entry, MatchStatus};

    #[test]
    fn the_header_cycle_is_ascending_descending_original() {
        let a = Column::File;
        let b = Column::Status;
        let s1 = next_sort(None, a);
        assert_eq!(s1, Some((a, true)));
        let s2 = next_sort(s1, a);
        assert_eq!(s2, Some((a, false)));
        assert_eq!(next_sort(s2, a), None);
        // a different column always starts ascending, from any state
        assert_eq!(next_sort(Some((a, false)), b), Some((b, true)));
        assert_eq!(next_sort(None, b), Some((b, true)));
    }

    #[test]
    fn a_status_cell_carries_its_tier() {
        let mut e = Entry::new("/x/a.png");
        e.status = MatchStatus::Error;
        let c = cell_view(&e, Column::Status);
        assert_eq!(c.text, "\u{d7} Error");
        assert_eq!(c.ink, Some(CellInk::Ink100));
        assert!(c.bold && c.boxed && !c.italic);
        e.status = MatchStatus::Poor;
        let c = cell_view(&e, Column::Status);
        assert!(c.markup.contains("Ryoku Status"));
        assert!(c.bold);
        e.status = MatchStatus::Searching;
        let c = cell_view(&e, Column::Status);
        assert!(c.italic && !c.bold && c.ink == Some(CellInk::Ink65));
    }
}
