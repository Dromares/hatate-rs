//! The row thumbnails (`workers/thumbnail_worker.py` and the window's thumbnail scheduling,
//! `gui/main_window.py:2035-2120` and `:2795-2942` at `76c85c4`; DAN-2217).
//!
//! The decode is `hatate_core::thumb_decode`, on the worker pool; this file schedules it and
//! turns each result into a texture on the main thread. Under lazy thumbnails (the default) a
//! row asks for its picture when it is first drawn: the rows drawn since the last pass give the
//! visible range, [`visible_range_with_buffer`] widens it by the buffer, and only the rows of that
//! range nobody has decoded or is decoding are decoded. With lazy thumbnails off, every row is
//! asked for when the list changes, with the progress strip following the pass.
//!
//! **Cut:** `thumbnail_source = "hydrus"` (the default) fetches the picture from Hydrus when it
//! has the file. The Rust Hydrus client has no `get_thumbnail` yet, so every source other than
//! `"off"` decodes the file on disk, which is what Python does without an access key.

use crate::window::MainWindow;
use crate::worker::{Event, Slot};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::queue::EntryId;
use hatate_core::thumb::decode_px;
use hatate_core::thumb_decode::{ThumbPixels, decode_thumbnail};
use hatate_core::viewport::{entries_needing_thumbnails, visible_range_with_buffer};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

/// `THUMB_VIEWPORT_BUFFER_ROWS`: rows decoded beyond each edge of what is on screen.
pub const THUMB_VIEWPORT_BUFFER_ROWS: i64 = 15;

/// What the scheduler keeps between passes (`_thumbs_in_flight`, `thumbnail_worker`,
/// `_thumbnails_quiet`).
#[derive(Default)]
pub struct Scheduler {
    worker: Slot,
    in_flight: RefCell<HashSet<EntryId>>,
    /// Work asked for while a pass was running; started when it finishes.
    backlog: RefCell<Vec<(EntryId, String)>>,
    /// Rows drawn since the last viewport pass.
    drawn: RefCell<Vec<EntryId>>,
    pass_queued: Cell<bool>,
    quiet: Cell<bool>,
    done: Cell<usize>,
    total: Cell<usize>,
}

impl Scheduler {
    /// `retire(self.thumbnail_worker)`, and the results of a list that is gone are not wanted.
    pub fn retire(&self) {
        self.worker.retire();
        self.in_flight.borrow_mut().clear();
        self.backlog.borrow_mut().clear();
        self.drawn.borrow_mut().clear();
    }

    pub fn is_running(&self) -> bool {
        self.worker.is_running()
    }

    /// How many rows are being decoded or waiting to be.
    pub fn in_flight(&self) -> usize {
        self.in_flight.borrow().len()
    }
}

impl MainWindow {
    fn thumbnail_source_off(&self) -> bool {
        self.s.settings.lock().unwrap().thumbnail_source == "off"
    }

    fn lazy_thumbnails(&self) -> bool {
        self.s.settings.lock().unwrap().lazy_thumbnails
    }

    /// A row was drawn: under lazy thumbnails it may need a picture, and a burst of rows is one
    /// debounced pass (`_schedule_viewport_thumbnails`).
    pub fn on_row_drawn(&self, id: EntryId) {
        if self.s.closing.get() || !self.lazy_thumbnails() || self.thumbnail_source_off() {
            return;
        }
        let t = &self.s.thumbs;
        if self.s.queue_view.thumb_known(id) || t.in_flight.borrow().contains(&id) {
            return;
        }
        t.drawn.borrow_mut().push(id);
        if !t.pass_queued.replace(true) {
            let w = self.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(w) = w.upgrade() {
                    w.s.thumbs.pass_queued.set(false);
                    w.schedule_viewport_thumbnails();
                }
            });
        }
    }

    /// One viewport pass: the range the drawn rows span, widened by the buffer, minus what is
    /// decoded or decoding.
    pub fn schedule_viewport_thumbnails(&self) {
        if self.s.closing.get() || !self.lazy_thumbnails() || self.thumbnail_source_off() {
            return;
        }
        let drawn: Vec<EntryId> = self.s.thumbs.drawn.borrow_mut().drain(..).collect();
        let visible = self.s.queue_view.visible_ids();
        let rows: Vec<i64> = drawn
            .iter()
            .filter_map(|id| self.s.queue_view.row_of(*id))
            .map(i64::from)
            .collect();
        let (Some(&first), Some(&last)) = (rows.iter().min(), rows.iter().max()) else {
            return;
        };
        let Some((start, end)) = visible_range_with_buffer(
            first,
            last,
            visible.len() as i64,
            THUMB_VIEWPORT_BUFFER_ROWS,
        ) else {
            return;
        };
        let work = {
            let in_flight = self.s.thumbs.in_flight.borrow();
            entries_needing_thumbnails(
                visible[start as usize..=end as usize].iter().copied(),
                |id| id.0 as usize,
                |raw| self.s.queue_view.thumb_known(EntryId(raw as u64)),
                &in_flight.iter().map(|id| id.0 as usize).collect(),
            )
        };
        if !work.needed.is_empty() {
            self.start_thumbnail_generation(&work.needed, true);
        }
    }

    /// `_start_thumbnail_generation(entries, quiet)`: decodes these rows in the background. A pass
    /// already running is not interrupted (its rows are probably still on screen); the new work
    /// waits behind it.
    pub fn start_thumbnail_generation(&self, ids: &[EntryId], quiet: bool) {
        if ids.is_empty() || self.thumbnail_source_off() {
            return;
        }
        let t = &self.s.thumbs;
        let items: Vec<(EntryId, String)> = {
            let queue = self.s.queue.borrow();
            let mut in_flight = t.in_flight.borrow_mut();
            ids.iter()
                .filter(|id| in_flight.insert(**id))
                .filter_map(|id| queue.entry(*id).map(|e| (*id, e.path.clone())))
                .collect()
        };
        if items.is_empty() {
            return;
        }
        if t.worker.is_running() {
            t.backlog.borrow_mut().extend(items);
            return;
        }
        self.run_thumbnail_pass(items, quiet);
    }

    fn run_thumbnail_pass(&self, items: Vec<(EntryId, String)>, quiet: bool) {
        let t = &self.s.thumbs;
        t.quiet.set(quiet);
        t.done.set(0);
        t.total.set(items.len());
        if !quiet {
            self.s.shell.progress_bar.set_fraction(0.0);
        }
        let limit = decode_px();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx| {
                for (id, path) in items {
                    if ctx.stopped() {
                        break;
                    }
                    ctx.report((id, decode_thumbnail(&path, limit)));
                }
            },
            move |event: Event<(EntryId, Option<ThumbPixels>), ()>| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    Event::Progress((id, pixels)) => w.on_thumbnail_decoded(id, pixels),
                    Event::Done(()) => w.on_thumbnails_finished(),
                }
            },
        );
        t.worker.replace(job);
    }

    /// `_on_thumbnail_ready` (and the failure Python leaves as a blank cell): the picture becomes
    /// a texture, which is built here because only the main thread may.
    fn on_thumbnail_decoded(&self, id: EntryId, pixels: Option<ThumbPixels>) {
        let t = &self.s.thumbs;
        t.in_flight.borrow_mut().remove(&id);
        let texture = pixels.map(|p| {
            gdk::MemoryTexture::new(
                p.width as i32,
                p.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from_owned(p.rgba),
                p.width as usize * 4,
            )
            .upcast::<gdk::Texture>()
        });
        self.s.queue_view.store_thumb(id, texture);
        t.done.set(t.done.get() + 1);
        if !t.quiet.get() {
            let (done, total) = (t.done.get(), t.total.get());
            self.s
                .shell
                .progress_bar
                .set_fraction(done as f64 / total.max(1) as f64);
            self.say(&format!("Loading thumbnails\u{2026} ({done}/{total})"));
        }
    }

    /// `_on_thumbnails_finished`: the next waiting pass starts, or the strip goes quiet.
    fn on_thumbnails_finished(&self) {
        let t = &self.s.thumbs;
        let quiet = t.quiet.get();
        let next: Vec<(EntryId, String)> = t.backlog.borrow_mut().drain(..).collect();
        if !next.is_empty() {
            self.run_thumbnail_pass(next, quiet);
            return;
        }
        if quiet {
            // The user may have kept scrolling while this ran.
            if !t.drawn.borrow().is_empty() {
                self.schedule_viewport_thumbnails();
            }
            return;
        }
        self.s.shell.progress_bar.set_fraction(0.0);
        let total = self.s.queue.borrow().len();
        self.say(&format!("Ready - {total} image(s) in the list"));
    }

    /// With lazy thumbnails off every row is decoded as the list changes; with them on the
    /// rows' own drawing asks (`_refresh_table` -> `_get_row_thumbnail`).
    pub fn thumbnails_for_new_entries(&self) {
        if self.lazy_thumbnails() || self.thumbnail_source_off() {
            return;
        }
        let ids = self.s.queue_view.visible_ids();
        let wanted: Vec<EntryId> = ids
            .into_iter()
            .filter(|id| !self.s.queue_view.thumb_known(*id))
            .collect();
        self.start_thumbnail_generation(&wanted, false);
    }
}
