//! What the Review page does (`gui/review_view.py` `refresh_review`, `_refresh_review_strip`,
//! `_step_review_candidate`; `gui/main_window.py` `_update_preview`, `_populate_candidate_combo`,
//! `_on_candidate_combo_changed`, `_ensure_candidate_loaded`, `_on_candidate_fetched`,
//! `_refresh_tag_list`, `_on_edit_tags_toggled`, `_on_tag_item_edited`, `_remove_selected_tags`,
//! `_add_tags_to_current`, `_on_selection_changed`, `action_show_in_queue`,
//! `action_open_matched_url`; `gui/review_shortcuts.py` at `76c85c4`; DAN-2219), on `MainWindow`.
//! The widgets are [`crate::review_view`].

use crate::add_tags;
use crate::dialogs;
use crate::review_keys;
use crate::review_view::ReviewPage;
use crate::widgets::Piece;
use crate::window::MainWindow;
use crate::worker::Event;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use hatate_core::config::OrderedMap;
use hatate_core::engine_ids::reports_real_similarity;
use hatate_core::model::{Candidate, Entry, Tag, TagSource};
use hatate_core::preview_text::{
    ComparisonReadout, comparison_readout, local_frame_meta, matched_caption, matched_frame_label,
    matched_frame_meta, matched_image_info_text, no_candidate_text,
};
use hatate_core::queue::{EntryId, needs_review};
use hatate_core::shortcuts;
use hatate_core::similarity_display::similarity_label;
use hatate_core::tag_colors::get_tag_color;
use hatate_core::tag_rules::{InlineEditAction, apply_inline_edit};
use hatate_core::thumb_decode::{decode_thumbnail, image_header_info};
use std::collections::HashSet;

/// `PREVIEW_DECODE_SIZE`: the longest side the Side by side view decodes the local file at.
pub const PREVIEW_DECODE: u32 = 2048;

/// The ids with a handler (the three send ids are 6.g's). An action with no handler is not bound ("never bind a dead key", `install`).
pub const WIRED: [&str; 15] = [
    "next_row",
    "prev_row",
    "next_unreviewed",
    "prev_unreviewed",
    "next_candidate",
    "prev_candidate",
    "open_match",
    "research",
    "reset_result",
    "remove_row",
    "toggle_reviewed",
    "compare",
    "send_upload",
    "send_url",
    "download_send",
];

/// How many decoded local pictures are kept (`_local_pixmap_cache`, which Python never trims).
const LOCAL_PICTURES_KEPT: usize = 48;

/// The decoded local pictures, newest last. `Some(None)` is "tried and failed", distinct from
/// absent ("not yet attempted"), so a broken file is not retried on every click.
#[derive(Default)]
pub struct LocalPictures {
    map: std::collections::HashMap<EntryId, Option<gdk::Texture>>,
    order: std::collections::VecDeque<EntryId>,
}

impl LocalPictures {
    pub fn get(&self, id: EntryId) -> Option<Option<gdk::Texture>> {
        self.map.get(&id).cloned()
    }

    pub fn put(&mut self, id: EntryId, texture: Option<gdk::Texture>) {
        if self.map.insert(id, texture).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > LOCAL_PICTURES_KEPT {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    /// A row's file changed or went: forget what was decoded from it.
    pub fn forget(&mut self, id: EntryId) {
        self.map.remove(&id);
        self.order.retain(|i| *i != id);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// `_host_from_url`.
fn host_from_url(url: &str) -> Option<String> {
    hatate_core::pyurl::try_urlparse(url, "")
        .ok()
        .map(|p| p.netloc)
        .filter(|n| !n.is_empty())
}

/// The text of one dropdown item (`_populate_candidate_combo`).
pub fn candidate_item_text(c: &Candidate) -> String {
    let site = c
        .source_name
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| host_from_url(&c.url))
        .unwrap_or_else(|| c.engine.clone());
    // The dropdown is where candidates are compared against one another, so a ranking sitting
    // next to a real score is most likely to be read as the same kind of number: same "~" as the
    // table.
    // `MatchCandidate.score_is_measured`: the flag is set only by the local re-measure, which skips
    // IQDB and SauceNAO because their own score is already a measurement.
    let measured = c.similarity_measured || reports_real_similarity(Some(&c.engine));
    let score = similarity_label(Some(c.similarity), measured);
    format!("{site} \u{2014} {score} ({}) \u{2014} {}", c.engine, c.url)
}

/// An item that elides instead of setting the picker's width.
fn eliding_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.set_width_chars(0);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(label), Some(text)) = (
            item.child().and_downcast::<gtk::Label>(),
            item.item().and_downcast::<gtk::StringObject>(),
        ) {
            label.set_text(&text.string());
            label.set_tooltip_text(Some(&text.string()));
        }
    });
    factory
}

fn rgb_of(hex: &str) -> Option<(u16, u16, u16)> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let part = |i: usize| u16::from_str_radix(&h[i..i + 2], 16).ok().map(|v| v * 257);
    Some((part(0)?, part(2)?, part(4)?))
}

/// A tag's label in the read-only list: `[Source] ns:name`, the marker dim and the tag in its
/// namespace colour (`TagRowDelegate`).
fn tag_row_label(tag: &Tag, colour: Option<&str>) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("tag-row");
    label.set_xalign(0.0);
    label.set_ellipsize(pango::EllipsizeMode::End);
    label.set_width_chars(0);
    let marker = glib::markup_escape_text(&format!("[{}] ", tag.source.as_str()));
    let text = glib::markup_escape_text(&tag.display());
    let body = match colour.and_then(rgb_of) {
        Some((r, g, b)) => format!(
            "<span foreground=\"#{:02x}{:02x}{:02x}\">{text}</span>",
            r >> 8,
            g >> 8,
            b >> 8
        ),
        None => text.to_string(),
    };
    label.set_markup(&format!("<span alpha=\"60%\">{marker}</span>{body}"));
    label
}

/// Paints the label inside an `EditableLabel` in `colour`.
fn colour_editable(row: &gtk::EditableLabel, colour: &str) {
    let Some((r, g, b)) = rgb_of(colour) else {
        return;
    };
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrColor::new_foreground(r, g, b));
    let mut stack = vec![row.clone().upcast::<gtk::Widget>()];
    while let Some(w) = stack.pop() {
        if let Some(label) = w.downcast_ref::<gtk::Label>() {
            label.set_attributes(Some(&attrs));
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            stack.push(c);
        }
    }
}

/// An unbound action's key, as `resolve` gives it.
fn key_of(bindings: &OrderedMap<String>, id: &str) -> String {
    bindings
        .0
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

impl MainWindow {
    /// `_current_entry`: the first selected row's id.
    pub fn current_id(&self) -> Option<EntryId> {
        self.s.queue_view.selected_ids().first().copied()
    }

    /// `_resolved_shortcuts`.
    pub fn resolved_shortcuts(&self) -> OrderedMap<String> {
        let s = self.s.settings.lock().unwrap();
        shortcuts::resolve(Some(&s.review_shortcuts))
    }

    /// `_key_for(action_id)`.
    pub fn key_for(&self, action_id: &str) -> String {
        key_of(&self.resolved_shortcuts(), action_id)
    }

    /// The page, built once with the keys as they are now.
    pub fn build_review_page(
        settings: &std::sync::Mutex<hatate_core::config::Settings>,
    ) -> std::rc::Rc<ReviewPage> {
        let bindings = shortcuts::resolve(Some(&settings.lock().unwrap().review_shortcuts));
        let page = ReviewPage::new(&|id| key_of(&bindings, id));
        page.candidate_combo.set_factory(Some(&eliding_factory()));
        page.candidate_combo
            .set_list_factory(Some(&eliding_factory()));
        page
    }

    /// The signals of the page's buttons (`_build_*`'s `connect`s).
    pub fn wire_review_page(&self) {
        let r = self.s.review.clone();
        let conn = |button: &gtk::Button, f: fn(&MainWindow)| {
            let w = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    f(&w);
                }
            });
        };
        let step = |button: &gtk::Button, delta: i32| {
            let w = self.downgrade();
            button.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    w.step_review_candidate(delta);
                }
            });
        };
        step(&r.prev_match_btn, -1);
        step(&r.next_match_btn, 1);
        conn(&r.add_tags_btn, MainWindow::add_tags_to_current);
        conn(&r.remove_tags_btn, MainWindow::remove_selected_tags);
        conn(&r.show_in_queue_btn, MainWindow::action_show_in_queue);
        conn(&r.open_btn.button, MainWindow::action_open_matched_url);
        conn(&r.mark_btn.button, MainWindow::toggle_reviewed_from_review);
        for (button, id) in [
            (&r.prev_btn, "prev_row"),
            (&r.next_unreviewed_btn, "next_unreviewed"),
            (&r.next_btn, "next_row"),
        ] {
            let w = self.downgrade();
            button.button.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    w.run_review_action(id);
                }
            });
        }
        let w = self.downgrade();
        r.edit_tags_btn.connect_toggled(move |b| {
            if let Some(w) = w.upgrade() {
                w.on_edit_tags_toggled(b.is_active());
            }
        });
        let w = self.downgrade();
        r.candidate_combo.connect_selected_notify(move |combo| {
            if let Some(w) = w.upgrade() {
                if !w.s.review.combo_blocked.get() {
                    w.on_candidate_combo_changed(combo.selected());
                }
            }
        });
        // The picture of the match is a link: a click opens the page, as the context menu does.
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let w = self.downgrade();
        click.connect_pressed(move |g, _, x, y| {
            if let Some(w) = w.upgrade() {
                g.set_state(gtk::EventSequenceState::Claimed);
                w.show_matched_preview_menu(x, y);
            }
        });
        r.matched.root.add_controller(click);
        // Pop out opens the comparison window (6.f-2). The shortcuts list is not on a ticket yet, and
        // the Hydrus buttons are 6.g's: present, and insensitive with their reason.
        {
            let w = self.downgrade();
            r.pop_out_btn.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    w.run_review_action("compare");
                }
            });
        }
        r.shortcuts_btn.set_sensitive(false);
        r.shortcuts_btn
            .set_tooltip_text(Some("The shortcuts list arrives in a later step."));
        for (b, id) in [
            (&r.send_btn, "send_upload"),
            (&r.url_btn, "send_url"),
            (&r.download_btn, "download_send"),
        ] {
            let w = self.downgrade();
            b.button.connect_clicked(move |_| {
                if let Some(w) = w.upgrade() {
                    w.run_review_action(id);
                }
            });
        }
        self.wire_review_compare();
        self.install_review_shortcuts();
    }

    // ------------------------------------------------------------------
    // The selection
    // ------------------------------------------------------------------

    /// `_on_selection_changed`.
    pub fn on_selection_changed(&self) {
        let id = self.current_id();
        self.update_preview(id);
        self.refresh_tag_list(id);
        let target = id.and_then(|id| {
            let q = self.s.queue.borrow();
            let e = q.entry(id)?;
            e.selected_candidate()?;
            Some((id, e.selected_candidate_index))
        });
        if let Some((id, index)) = target {
            // Covers a restored session, where the match's picture was never on disk.
            self.ensure_candidate_loaded(id, index);
        }
    }

    /// `_refresh_entry_row(entry)`: the row follows the entry, in place, and the sent count.
    pub fn refresh_entry_row(&self, id: EntryId) {
        self.s.queue.borrow_mut().take_events();
        self.s.queue_view.refresh_row(id);
        self.refresh_sent_count_label();
    }

    // ------------------------------------------------------------------
    // The pictures and captions
    // ------------------------------------------------------------------

    /// `_update_preview(entry)`.
    pub fn update_preview(&self, id: Option<EntryId>) {
        let r = &self.s.review;
        let Some(entry) = id.and_then(|id| self.s.queue.borrow().entry(id).cloned()) else {
            r.local.show_text("No image selected");
            r.matched.show_text("No match yet");
            r.set_readout(&ComparisonReadout::default());
            self.set_review_captions(None);
            self.populate_candidate_combo(None);
            // Python returns here without `refresh_review`, which leaves "Match 1 of 3" and the
            // steppers of the row that was just deselected; the page follows the empty selection.
            self.refresh_review();
            return;
        };
        let id = id.unwrap();
        self.show_local_picture(id, &entry.path);
        self.populate_candidate_combo(Some(&entry));

        let candidate = entry.selected_candidate();
        // `theme.status_glyph`: the two keys the readout asks for.
        let glyph = |key: &str| match key {
            "good" => Some("\u{25cf}".to_owned()),
            "estimated" => Some("\u{b7}".to_owned()),
            _ => None,
        };
        r.set_readout(&comparison_readout(&entry, candidate, &glyph));
        self.set_review_captions(Some(&entry));
        let thumb = candidate.and_then(|c| c.thumb_bytes.as_ref());
        if let Some(bytes) = thumb {
            match gdk::Texture::from_bytes(&glib::Bytes::from(bytes)) {
                Ok(texture) => r.matched.show_texture(&texture),
                Err(_) => r
                    .matched
                    .show_text("Match found but thumbnail failed to load"),
            }
        } else if !entry.candidates.is_empty() {
            r.matched.show_text("Loading picture\u{2026}");
        } else {
            r.matched.show_text(&no_candidate_text(&entry));
        }
        // Review shows the same entry from the other side of the window.
        self.refresh_review();
    }

    /// `_get_local_pixmap` into `local_preview`: decoded off the main thread, once per entry.
    fn show_local_picture(&self, id: EntryId, path: &str) {
        let cached = self.s.local_pictures.borrow().get(id);
        match cached {
            Some(Some(texture)) => self.s.review.local.show_texture(&texture),
            Some(None) => self.s.review.local.show_text("Could not load image"),
            None => {
                self.s.review.local.show_text("Loading image\u{2026}");
                if !self.s.local_pending.borrow_mut().insert(id) {
                    return;
                }
                let path = path.to_owned();
                let w = self.downgrade();
                let _job = self.s.workers.run(
                    move |_ctx: &crate::worker::Ctx<()>| decode_thumbnail(&path, PREVIEW_DECODE),
                    move |event: Event<(), Option<hatate_core::thumb_decode::ThumbPixels>>| {
                        let Some(w) = w.upgrade() else { return };
                        if let Event::Done(pixels) = event {
                            w.on_local_decoded(id, pixels);
                        }
                    },
                );
            }
        }
    }

    fn on_local_decoded(
        &self,
        id: EntryId,
        pixels: Option<hatate_core::thumb_decode::ThumbPixels>,
    ) {
        self.s.local_pending.borrow_mut().remove(&id);
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
        self.s.local_pictures.borrow_mut().put(id, texture.clone());
        if self.current_id() == Some(id) {
            match texture {
                Some(t) => self.s.review.local.show_texture(&t),
                None => self.s.review.local.show_text("Could not load image"),
            }
        }
    }

    /// `_set_review_captions(entry, candidate)`.
    fn set_review_captions(&self, entry: Option<&Entry>) {
        let r = &self.s.review;
        let (local, label, meta, note, tip);
        match entry {
            None => {
                local = String::new();
                label = "Match".to_owned();
                meta = String::new();
                note = String::new();
                tip = String::new();
            }
            Some(e) => {
                local = local_frame_meta(&e.path, &|p: &str| image_header_info(p));
                if e.candidates.is_empty() {
                    label = "Match \u{2014} none".to_owned();
                    meta = "\u{2014}".to_owned();
                    note = String::new();
                    tip = String::new();
                } else {
                    let c = e.selected_candidate();
                    label = matched_frame_label(e);
                    let m = matched_frame_meta(c);
                    meta = if m.is_empty() {
                        "Image info not available".to_owned()
                    } else {
                        m
                    };
                    note = matched_image_info_text(c)
                        .split_once('\n')
                        .map(|(_, rest)| rest.to_owned())
                        .unwrap_or_default();
                    tip = matched_caption(e);
                }
            }
        }
        r.local_caption.right.set_text(&local);
        r.matched_caption.left.set_text(&label);
        r.matched_caption
            .left
            .set_tooltip_text(if tip.is_empty() { None } else { Some(&tip) });
        r.matched_caption.right.set_text(&meta);
        r.matched_note.set_text(&note);
        r.matched_note.set_visible(!note.is_empty());
    }

    // ------------------------------------------------------------------
    // The candidate picker
    // ------------------------------------------------------------------

    /// `_populate_candidate_combo(entry)`: rebuilt without reading as a choice.
    pub fn populate_candidate_combo(&self, entry: Option<&Entry>) {
        let r = &self.s.review;
        r.combo_blocked.set(true);
        let n = r.candidate_model.n_items();
        let items: Vec<String> = entry
            .map(|e| e.candidates.iter().map(candidate_item_text).collect())
            .unwrap_or_default();
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        r.candidate_model.splice(0, n, &refs);
        match entry.filter(|e| !e.candidates.is_empty()) {
            None => {
                r.candidate_combo.set_sensitive(false);
                r.candidate_combo.set_tooltip_text(None);
                r.candidate_combo.set_selected(gtk::INVALID_LIST_POSITION);
            }
            Some(e) => {
                r.candidate_combo.set_sensitive(true);
                r.candidate_combo
                    .set_selected(e.selected_candidate_index as u32);
                self.sync_candidate_combo_tooltip(Some(e));
            }
        }
        r.combo_blocked.set(false);
    }

    /// `_sync_candidate_combo_tooltip(entry)`.
    fn sync_candidate_combo_tooltip(&self, entry: Option<&Entry>) {
        let url = entry
            .and_then(|e| e.selected_candidate())
            .map(|c| c.url.clone())
            .unwrap_or_default();
        self.s
            .review
            .candidate_combo
            .set_tooltip_text(if url.is_empty() { None } else { Some(&url) });
    }

    /// `_on_candidate_combo_changed(index)`.
    fn on_candidate_combo_changed(&self, index: u32) {
        let Some(id) = self.current_id() else { return };
        {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            if index == gtk::INVALID_LIST_POSITION || index as usize >= entry.candidates.len() {
                return;
            }
            entry.select_candidate(index as usize);
        }
        let entry = self.s.queue.borrow().entry(id).cloned();
        self.sync_candidate_combo_tooltip(entry.as_ref());
        self.refresh_entry_row(id);
        self.update_preview(Some(id));
        self.refresh_tag_list(Some(id));
        self.ensure_candidate_loaded(id, index as usize);
    }

    /// `_step_review_candidate(delta)`.
    pub fn step_review_candidate(&self, delta: i32) {
        let combo = &self.s.review.candidate_combo;
        let count = self.s.review.candidate_model.n_items() as i32;
        if !combo.is_sensitive() || count == 0 {
            return;
        }
        let at = combo.selected() as i32;
        combo.set_selected((at + delta).clamp(0, count - 1) as u32);
    }

    /// `_ensure_candidate_loaded(entry, index)`: the selected candidate's picture and tags, from
    /// the engine, off the main thread, when they are not in memory.
    pub fn ensure_candidate_loaded(&self, id: EntryId, index: usize) {
        let (mut candidate, path) = {
            let q = self.s.queue.borrow();
            let Some(e) = q.entry(id) else { return };
            let Some(c) = e.candidates.get(index) else {
                return;
            };
            if c.thumb_bytes.is_some() && c.booru_tags_fetched {
                return;
            }
            (c.clone(), e.path.clone())
        };
        tracing::debug!(
            "Fetching details for the selected candidate {index} of {path} (thumb={}, tags_fetched={})",
            candidate.thumb_bytes.is_some(),
            candidate.booru_tags_fetched
        );
        self.s.candidate_worker.retire();
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            tracing::warn!("Could not fetch candidate details: the engines could not be built");
            return;
        };
        let Some(handle) = self.s.workers.handle() else {
            return;
        };
        self.say("Fetching picture and tags for the selected site\u{2026}");
        let settings = self.s.settings.clone();
        let engine = backend.engine.clone();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |ctx: &crate::worker::Ctx<(String, f64, f64)>| {
                let reports = ctx.reports();
                let tick = move |label: &str, remaining: f64, total: f64| {
                    reports.send((label.to_owned(), remaining, total));
                };
                if let Err(e) = handle.block_on(engine.fetch_candidate_details(
                    &mut candidate,
                    &settings,
                    Some(&tick),
                    Some(&path),
                )) {
                    tracing::warn!("Unexpected error fetching candidate details: {e}");
                }
                candidate
            },
            move |event| {
                let Some(w) = w.upgrade() else { return };
                match event {
                    Event::Progress((label, remaining, total)) => {
                        w.on_wait_countdown(&label, remaining, total)
                    }
                    Event::Done(fetched) => w.on_candidate_fetched(id, index, fetched),
                }
            },
        );
        self.s.candidate_worker.replace(job);
    }

    /// `_on_candidate_fetched(entry, index)`.
    fn on_candidate_fetched(&self, id: EntryId, index: usize, fetched: Candidate) {
        if self.s.closing.get() {
            return;
        }
        let drop_dead = self.s.settings.lock().unwrap().drop_dead_matches;
        {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            // The fetch ran on a copy; it fills the candidate in place on the entry's own.
            match entry.candidates.get_mut(index) {
                Some(slot) if slot.url == fetched.url => *slot = fetched,
                _ => return,
            }
        }
        // The lazy fetch may have just discovered the source is gone (a 404 or 410); the link is
        // not left on screen once it is known to be dead.
        let dead = drop_dead
            && self
                .s
                .queue
                .borrow()
                .entry(id)
                .and_then(|e| e.candidates.get(index))
                .is_some_and(|c| c.remote_available == Some(false));
        if dead {
            tracing::info!("Selected match is gone - dropping it and moving to the next");
            self.remove_dead_candidates(id);
            self.say("That match is no longer available - it's been removed");
            return;
        }
        {
            let mut q = self.s.queue.borrow_mut();
            if let Some(e) = q.entry_mut(id) {
                if e.selected_candidate_index == index {
                    // Re-sync now that the thumbnail and the booru tags are filled in.
                    e.select_candidate(index);
                }
            }
        }
        if self.current_id() == Some(id) {
            self.update_preview(Some(id));
            self.refresh_tag_list(Some(id));
        }
        self.refresh_entry_row(id);
        self.say("Ready");
    }

    // ------------------------------------------------------------------
    // The tag panel
    // ------------------------------------------------------------------

    /// `_refresh_tag_list(entry)`.
    pub fn refresh_tag_list(&self, id: Option<EntryId>) {
        let r = &self.s.review;
        while let Some(row) = r.tag_list.row_at_index(0) {
            r.tag_list.remove(&row);
        }
        self.s.tag_rows.borrow_mut().clear();
        let Some(id) = id else { return };
        let editable = self.s.tags_editable.get();
        let (entry, settings) = {
            let q = self.s.queue.borrow();
            let Some(e) = q.entry(id) else { return };
            (e.clone(), self.s.settings.lock().unwrap().clone())
        };
        let enabled: Vec<TagSource> = settings
            .enabled_tag_sources
            .iter()
            .filter_map(|n| TagSource::parse(n))
            .collect();
        for (at, tag) in entry.tags.iter().enumerate() {
            if !enabled.is_empty() && !enabled.contains(&tag.source) {
                continue;
            }
            let colour = get_tag_color(tag, &settings);
            let row = gtk::ListBoxRow::new();
            if editable {
                // Plain "namespace:name" while editable, so there is nothing to mangle and what
                // is shown is exactly what gets parsed back.
                let field = gtk::EditableLabel::new(&tag.display());
                field.add_css_class("tag-row");
                if let Some(c) = &colour {
                    colour_editable(&field, c);
                }
                let (w, original) = (self.downgrade(), tag.display());
                let row_widget = row.clone();
                field.connect_editing_notify(move |f| {
                    if f.is_editing() {
                        return;
                    }
                    let text = f.text().to_string();
                    if text == original {
                        return;
                    }
                    if let Some(w) = w.upgrade() {
                        let at = row_widget.index();
                        w.on_tag_item_edited(at, &text);
                    }
                });
                row.set_child(Some(&field));
            } else {
                row.set_child(Some(&tag_row_label(tag, colour.as_deref())));
            }
            r.tag_list.append(&row);
            self.s.tag_rows.borrow_mut().push(at);
        }
    }

    /// `_on_edit_tags_toggled(checked)`.
    fn on_edit_tags_toggled(&self, checked: bool) {
        self.s.tags_editable.set(checked);
        self.s.review.edit_tags_btn.set_label(if checked {
            "Editing Tags (click to stop)"
        } else {
            "Edit Tags"
        });
        self.refresh_tag_list(self.current_id());
    }

    /// `_on_tag_item_edited(item)`: clearing a tag's text deletes it; otherwise it is renamed in
    /// place with the namespace remap, keeping its source.
    pub fn on_tag_item_edited(&self, row: i32, text: &str) {
        let Some(id) = self.current_id() else { return };
        let Some(target) = usize::try_from(row)
            .ok()
            .and_then(|r| self.s.tag_rows.borrow().get(r).copied())
        else {
            return;
        };
        let settings = self.s.settings.lock().unwrap().clone();
        {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            let Some(old) = entry.tags.get(target).map(|t| t.display()) else {
                return;
            };
            let result =
                apply_inline_edit(std::mem::take(&mut entry.tags), target, text, &settings);
            entry.tags = result.tags;
            if result.action == InlineEditAction::Removed {
                tracing::info!(
                    "Removed tag {old} from {} (cleared via inline edit)",
                    entry.filename()
                );
            } else {
                if result.dropped_duplicate {
                    tracing::debug!(
                        "Inline edit of {} created a duplicate of an existing tag; removed the older one",
                        entry.filename()
                    );
                }
                tracing::info!("Renamed tag {old} -> {text} on {}", entry.filename());
            }
        }
        self.refresh_tag_list(Some(id));
        self.list_changed();
    }

    /// `_remove_selected_tags`.
    pub fn remove_selected_tags(&self) {
        let Some(id) = self.current_id() else { return };
        let picked: Vec<usize> = self
            .s
            .review
            .tag_list
            .selected_rows()
            .iter()
            .filter_map(|r| usize::try_from(r.index()).ok())
            .filter_map(|r| self.s.tag_rows.borrow().get(r).copied())
            .collect();
        {
            let mut q = self.s.queue.borrow_mut();
            let Some(entry) = q.entry_mut(id) else { return };
            let keys: HashSet<(Option<String>, String)> = picked
                .iter()
                .filter_map(|i| entry.tags.get(*i))
                .map(|t| (t.namespace.clone(), t.name.clone()))
                .collect();
            entry
                .tags
                .retain(|t| !keys.contains(&(t.namespace.clone(), t.name.clone())));
        }
        self.on_selection_changed();
        self.list_changed();
    }

    /// `action_edit_conditions`: the dialog opens on the settings' conditions, and OK stores them
    /// and saves.
    pub fn action_edit_conditions(&self) {
        let current = self.s.settings.lock().unwrap().match_conditions.clone();
        let w = self.downgrade();
        self.s.prompter.match_conditions(
            &current,
            Box::new(move |answer| {
                let (Some(w), Some(conditions)) = (w.upgrade(), answer) else {
                    return;
                };
                let mut s = w.s.settings.lock().unwrap();
                s.match_conditions = conditions;
                if let Err(e) = s.save(&w.s.paths) {
                    tracing::warn!("could not save the settings: {e}");
                }
            }),
        );
    }

    /// `_add_tags_to_current`.
    pub fn add_tags_to_current(&self) {
        let ids = self.s.queue_view.selected_ids();
        if ids.is_empty() {
            dialogs::information(
                &*self.s.prompter,
                "Add tags",
                "Select at least one image first.",
            );
            return;
        }
        let description = if ids.len() == 1 {
            self.s
                .queue
                .borrow()
                .entry(ids[0])
                .map(|e| e.filename().to_owned())
                .unwrap_or_default()
        } else {
            format!("{} selected images", ids.len())
        };
        let w = self.downgrade();
        self.s.prompter.add_tags(
            &add_tags::prompt(&description),
            Box::new(move |text| {
                let (Some(w), Some(text)) = (w.upgrade(), text) else {
                    return;
                };
                let new_tags = add_tags::parse_added_tags(&text);
                if new_tags.is_empty() {
                    return;
                }
                {
                    let mut q = w.s.queue.borrow_mut();
                    for id in &ids {
                        if let Some(e) = q.entry_mut(*id) {
                            e.add_tags(new_tags.clone(), None);
                        }
                    }
                }
                tracing::info!("Added {} tag(s) to {} image(s)", new_tags.len(), ids.len());
                if let Some(current) = w.current_id() {
                    w.refresh_tag_list(Some(current));
                }
                w.list_changed();
            }),
        );
    }

    // ------------------------------------------------------------------
    // The match menu and links
    // ------------------------------------------------------------------

    /// `_show_matched_preview_menu(pos)`.
    fn show_matched_preview_menu(&self, x: f64, y: f64) {
        let Some(id) = self.current_id() else { return };
        let Some(url) = self
            .s
            .queue
            .borrow()
            .entry(id)
            .and_then(|e| e.matched_url().map(str::to_owned))
            .filter(|u| !u.is_empty())
        else {
            return;
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            dialogs::warning(
                &*self.s.prompter,
                "Open image URL",
                "This match doesn't have a valid web address to open.",
            );
            return;
        }
        let model = gio::Menu::new();
        model.append(Some("Open image URL in browser"), Some("matchmenu.open"));
        model.append(Some("Copy image URL"), Some("matchmenu.copy"));
        let group = gio::SimpleActionGroup::new();
        let open = gio::SimpleAction::new("open", None);
        let w = self.downgrade();
        open.connect_activate(move |_, _| {
            if let Some(w) = w.upgrade() {
                w.action_open_matched_url();
            }
        });
        let copy = gio::SimpleAction::new("copy", None);
        let w = self.downgrade();
        copy.connect_activate(move |_, _| {
            if let Some(w) = w.upgrade() {
                w.s.review.matched.root.clipboard().set_text(&url);
            }
        });
        group.add_action(&open);
        group.add_action(&copy);
        let pane = &self.s.review.matched.root;
        pane.insert_action_group("matchmenu", Some(&group));
        let popover = gtk::PopoverMenu::from_model(Some(&model));
        popover.set_parent(pane);
        popover.set_has_arrow(false);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    /// `action_open_matched_url` -> `review_shortcuts.open_matched_url(window, entry)`.
    pub fn action_open_matched_url(&self) {
        let url = self.current_id().and_then(|id| {
            self.s
                .queue
                .borrow()
                .entry(id)
                .map(|e| e.matched_url().map(str::to_owned))
        });
        let Some(Some(url)) = url.filter(|u| u.as_deref().is_some_and(|s| !s.is_empty())) else {
            self.say("Open match: this image has no match");
            return;
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            dialogs::warning(
                &*self.s.prompter,
                "Open match in browser",
                "This match doesn't have a valid web address to open.",
            );
            return;
        }
        (self.s.open_url)(&url);
    }

    /// `action_show_in_queue`: Queue mode, with the current image's row selected.
    pub fn action_show_in_queue(&self) {
        let row = self
            .current_id()
            .and_then(|id| self.s.queue_view.row_of(id));
        self.set_mode("queue");
        if let Some(row) = row {
            self.s.queue_view.select_row(row);
        }
    }

    // ------------------------------------------------------------------
    // The page's state
    // ------------------------------------------------------------------

    /// `refresh_review`: brings the page up to date with the selection.
    pub fn refresh_review(&self) {
        let r = &self.s.review;
        self.sync_comparison_to_selection();
        let id = self.current_id();
        let (count, index) = (
            r.candidate_model.n_items() as usize,
            r.candidate_combo.selected(),
        );
        let index = if index == gtk::INVALID_LIST_POSITION {
            0
        } else {
            index as usize
        };
        if id.is_some() && count > 0 {
            r.match_label
                .set_text(&format!("Match {} of {count}", index + 1));
        } else {
            r.match_label.set_text("No match");
        }
        let has = id.is_some() && count > 0;
        let (prev_on, next_on) = (has && index > 0, has && index + 1 < count);
        r.prev_match_btn.set_sensitive(prev_on);
        r.next_match_btn.set_sensitive(next_on);
        r.prev_match_key.set_sensitive(prev_on);
        r.next_match_key.set_sensitive(next_on);
        if let Some(entry) = id.and_then(|id| self.s.queue.borrow().entry(id).cloned()) {
            r.mark_btn.set_text(if entry.reviewed {
                "Unmark reviewed"
            } else {
                "Mark reviewed"
            });
        }
        self.refresh_review_strip();
    }

    /// `_refresh_review_strip`: where you are, and how much is left.
    pub fn refresh_review_strip(&self) {
        let r = &self.s.review;
        let total = self.s.queue_view.visible_count();
        let id = self.current_id();
        let row = id.and_then(|id| self.s.queue_view.row_of(id));
        let position = row.map_or(String::new(), |row| format!("{} of {total}", row + 1));
        r.prev_btn.button.set_sensitive(total > 0 && row != Some(0));
        r.next_btn
            .button
            .set_sensitive(total > 0 && row.is_none_or(|row| (row as usize) + 1 < total));
        self.refresh_review_keycaps();
        let left = self
            .s
            .queue
            .borrow()
            .entries()
            .iter()
            .filter(|e| needs_review(e))
            .count();
        r.set_hint_groups(&self.hint_bar_groups(&position, left));
    }

    /// `_refresh_review_keycaps`.
    pub fn refresh_review_keycaps(&self) {
        let r = &self.s.review;
        for (button, id) in [
            (&r.send_btn, "send_upload"),
            (&r.url_btn, "send_url"),
            (&r.download_btn, "download_send"),
            (&r.mark_btn, "toggle_reviewed"),
            (&r.open_btn, "open_match"),
            (&r.prev_btn, "prev_row"),
            (&r.next_unreviewed_btn, "next_unreviewed"),
            (&r.next_btn, "next_row"),
        ] {
            button.set_key(&self.key_for(id));
        }
        r.prev_match_key.set_text(&self.key_for("prev_candidate"));
        r.next_match_key.set_text(&self.key_for("next_candidate"));
    }

    /// `_hint_bar_groups(position, left)`: built from the resolved bindings, so a key the user
    /// rebound (or unbound) is what the bar shows.
    pub fn hint_bar_groups(&self, position: &str, left: usize) -> Vec<Vec<Piece>> {
        let bindings = self.resolved_shortcuts();
        let mut groups: Vec<Vec<Piece>> = Vec::new();
        if !position.is_empty() {
            groups.push(vec![Piece::Strong(position.to_owned())]);
        }
        if left > 0 {
            groups.push(vec![
                Piece::Strong(left.to_string()),
                Piece::Text("still to review".to_owned()),
            ]);
        }
        let move_keys: Vec<String> = [key_of(&bindings, "next_row"), key_of(&bindings, "prev_row")]
            .into_iter()
            .filter(|k| !k.is_empty())
            .collect();
        if !move_keys.is_empty() {
            let mut pieces = Vec::new();
            for (n, key) in move_keys.into_iter().enumerate() {
                if n > 0 {
                    pieces.push(Piece::Text("/".to_owned()));
                }
                pieces.push(Piece::Key(key));
            }
            pieces.push(Piece::Text("move".to_owned()));
            groups.push(pieces);
        }
        for (id, words) in [
            ("next_unreviewed", "next unreviewed"),
            ("toggle_reviewed", "marks"),
            ("compare", "compares"),
        ] {
            let key = key_of(&bindings, id);
            if !key.is_empty() {
                groups.push(vec![Piece::Key(key), Piece::Text(words.to_owned())]);
            }
        }
        groups
    }

    /// `_on_mode_changed("review")`: the page is brought up to date, and takes focus so its keys
    /// have somewhere to land.
    pub fn enter_review(&self) {
        self.refresh_review();
        self.s.review.root.grab_focus();
    }

    // ------------------------------------------------------------------
    // The keys
    // ------------------------------------------------------------------

    /// `review_shortcuts._move_selection`.
    fn move_selection(&self, delta: i64) {
        let total = self.s.queue_view.visible_count() as i64;
        if total <= 0 {
            return;
        }
        let rows = self.selected_rows();
        let target = match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => (if delta > 0 { *last } else { *first }) as i64 + delta,
            _ => {
                if delta > 0 {
                    0
                } else {
                    total - 1
                }
            }
        };
        if (0..total).contains(&target) {
            self.s.queue_view.select_row(target as u32);
        }
    }

    fn selected_rows(&self) -> Vec<u32> {
        let mut rows: Vec<u32> = self
            .s
            .queue_view
            .selected_ids()
            .into_iter()
            .filter_map(|id| self.s.queue_view.row_of(id))
            .collect();
        rows.sort_unstable();
        rows
    }

    /// `review_shortcuts._move_to_unreviewed`.
    fn move_to_unreviewed(&self, delta: i64) {
        let ids = self.s.queue_view.visible_ids();
        let total = ids.len() as i64;
        if total <= 0 {
            return;
        }
        let rows = self.selected_rows();
        let start = match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => (if delta > 0 { *last } else { *first }) as i64,
            _ => {
                if delta > 0 {
                    -1
                } else {
                    total
                }
            }
        };
        let mut row = start + delta;
        while (0..total).contains(&row) {
            let wants = self
                .s
                .queue
                .borrow()
                .entry(ids[row as usize])
                .is_some_and(needs_review);
            if wants {
                self.s.queue_view.select_row(row as u32);
                return;
            }
            row += delta;
        }
        let direction = if delta > 0 { "after" } else { "before" };
        self.say(&format!("Nothing left to review {direction} this one"));
    }

    /// `review_shortcuts._toggle_reviewed`: a mixed selection marks.
    fn toggle_reviewed(&self) {
        let ids = self.s.queue_view.selected_ids();
        let already = !ids.is_empty()
            && ids
                .iter()
                .all(|id| self.s.queue.borrow().entry(*id).is_some_and(|e| e.reviewed));
        self.set_rows_reviewed(&ids, !already);
        // Python flips the button's label only when the selection next changes; here it follows
        // the mark at once.
        self.refresh_review();
    }

    /// `_toggle_reviewed_from_review`.
    pub fn toggle_reviewed_from_review(&self) {
        self.toggle_reviewed();
        self.refresh_review_strip();
    }

    /// `_run_review_action(id)` -> `review_shortcuts.handlers(window)[id]()`.
    pub fn run_review_action(&self, action_id: &str) {
        match action_id {
            "next_row" => self.move_selection(1),
            "prev_row" => self.move_selection(-1),
            "next_unreviewed" => self.move_to_unreviewed(1),
            "prev_unreviewed" => self.move_to_unreviewed(-1),
            "next_candidate" => self.step_candidate(1),
            "prev_candidate" => self.step_candidate(-1),
            "open_match" => self.action_open_matched_url(),
            "research" => self.research_rows(&self.s.queue_view.selected_ids()),
            "reset_result" => self.reset_rows(&self.s.queue_view.selected_ids()),
            "remove_row" => self.remove_rows(&self.s.queue_view.selected_ids()),
            "toggle_reviewed" => self.toggle_reviewed(),
            "compare" => {
                if let Some(id) = self.current_id() {
                    self.open_compare_dialog(id);
                }
            }
            // The Review page's three buttons act on the selected rows, as the menu entries do.
            "send_upload" => self.action_send_to_hydrus(&self.s.queue_view.selected_ids()),
            "send_url" => self.action_import_url_to_hydrus(&self.s.queue_view.selected_ids()),
            "download_send" => {
                self.action_download_and_send_to_hydrus(&self.s.queue_view.selected_ids())
            }
            other => tracing::warn!("No handler for review shortcut {other:?}"),
        }
    }

    /// `review_shortcuts._step_candidate`: the dropdown's own change does the fetching and the
    /// preview.
    fn step_candidate(&self, delta: i32) {
        let combo = &self.s.review.candidate_combo;
        let count = self.s.review.candidate_model.n_items() as i32;
        if !combo.is_sensitive() || count <= 1 {
            return;
        }
        let target = combo.selected() as i32 + delta;
        if (0..count).contains(&target) {
            combo.set_selected(target as u32);
        }
    }

    /// `review_shortcuts._guarded(window, id, handler)`: the checks the menu entry gets for free,
    /// answered in the status bar.
    fn guarded_review_action(&self, action_id: &'static str) {
        let action = shortcuts::action(action_id);
        let navigational = matches!(
            action_id,
            "next_row"
                | "prev_row"
                | "next_unreviewed"
                | "prev_unreviewed"
                | "next_candidate"
                | "prev_candidate"
        );
        if !navigational {
            let rows = self.selected_rows();
            let label = action.map_or(action_id, |a| a.label);
            if rows.is_empty() {
                self.say(&format!("{label}: select an image first"));
                return;
            }
            if action.is_some_and(|a| a.needs_single_row) && rows.len() > 1 {
                self.say(&format!(
                    "{label}: works on one image at a time ({} selected)",
                    rows.len()
                ));
                return;
            }
        }
        self.run_review_action(action_id);
    }

    /// `review_shortcuts.install(window)`: every binding from the current settings, on the table
    /// and on the Review page, and on nothing else. A text field keeps its characters because the
    /// focused widget sees a key before the shortcut controller above it does.
    pub fn install_review_shortcuts(&self) {
        for (widget, controller) in self.s.review_controllers.borrow_mut().drain(..) {
            widget.remove_controller(&controller);
        }
        let bindings = self.resolved_shortcuts();
        let hosts: [gtk::Widget; 2] = [
            self.s.queue_view.root.clone().upcast(),
            self.s.review.root.clone().upcast(),
        ];
        let mut installed = 0;
        for host in hosts {
            let controller = gtk::ShortcutController::new();
            controller.set_scope(gtk::ShortcutScope::Local);
            controller.set_propagation_phase(gtk::PropagationPhase::Bubble);
            for action_id in WIRED {
                let key = key_of(&bindings, action_id);
                if key.is_empty() {
                    continue; // deliberately unbound
                }
                let Some(trigger) = review_keys::gtk_trigger(&key)
                    .and_then(|t| gtk::ShortcutTrigger::parse_string(&t))
                else {
                    tracing::warn!("Ignoring unparseable shortcut {key:?} for {action_id:?}");
                    continue;
                };
                let w = self.downgrade();
                let action = gtk::CallbackAction::new(move |_, _| {
                    if let Some(w) = w.upgrade() {
                        w.guarded_review_action(action_id);
                    }
                    glib::Propagation::Stop
                });
                controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
                installed += 1;
            }
            host.add_controller(controller.clone());
            self.s
                .review_controllers
                .borrow_mut()
                .push((host, controller));
        }
        tracing::debug!(
            "Installed {installed} review shortcut(s) on the image table and the Review page"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(engine: &str, measured: bool) -> Candidate {
        let mut c = Candidate::new("https://example.invalid/post/3");
        c.engine = engine.to_owned();
        c.similarity = 95.0;
        c.similarity_measured = measured;
        c.source_name = Some("Other".to_owned());
        c
    }

    /// `_populate_candidate_combo` marks a ranking with the `~` and a real score without it, and
    /// IQDB and SauceNAO scores are real even though the flag reads False for them.
    #[test]
    fn picker_item_marks_only_estimated_scores() {
        let text = |c| candidate_item_text(&c);
        assert_eq!(
            text(candidate("IQDB", false)),
            "Other \u{2014} 95% (IQDB) \u{2014} https://example.invalid/post/3"
        );
        assert!(text(candidate("Google Lens", false)).contains("~95%"));
        assert!(text(candidate("Google Lens", true)).contains(" 95% "));
    }
}
