//! Wipe and Differences on the Review page (`gui/review_view.py` `_on_review_view_changed`,
//! `_ensure_comparison_loaded`, `_apply_wipe_mode`, `_show_differences`, `_show_comparison_controls`,
//! `_current_review_key`; `gui/compare_dialog.py` `load_comparison` at `76c85c4`; DAN-2340), on
//! `MainWindow`. The widget is [`crate::wipe_view`], the maths [`hatate_core::compare_view`].
//!
//! Python fetches the match, decodes it and builds the difference overlay on the main thread and
//! calls `processEvents` so the "Loading" line can paint. Here each runs on a worker and the window
//! keeps handling actions while it waits; a result for a pair that is no longer the selected one is
//! dropped.

use crate::review_view::WIPE_HINT;
use crate::window::MainWindow;
use crate::worker::Event;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::compare_view::{
    DifferenceView, PictureError, Rgba, build_difference_overlay, decode_rgba, open_local,
    ramp_picture,
};
use hatate_core::model::Candidate;
use hatate_core::queue::EntryId;
use std::sync::Arc;

/// `(id(entry), candidate url)`: the pair the wipe holds.
pub type ReviewKey = (EntryId, Option<String>);

/// What the wipe holds, and what it is waiting for.
#[derive(Default)]
pub struct ComparisonState {
    /// `_review_loaded_for`.
    pub loaded_for: Option<ReviewKey>,
    /// Bumped by every load, so a late result for an older pair is dropped.
    pub generation: u64,
    /// `_review_loading`.
    pub loading: bool,
    pub local: Option<Arc<Rgba>>,
    pub matched: Option<Arc<Rgba>>,
    /// `_review_diff_for`, and what was built for it: `Some(None)` is "built, nothing to show".
    pub diff_for: Option<ReviewKey>,
    pub diff: Option<Option<Arc<Panes>>>,
    /// `_review_load_notes`.
    pub notes: String,
    /// The local picture's error, while the page is in the error state (design gate 2.8).
    pub error: Option<PictureError>,
}

/// The Differences pane's pictures, off the main thread.
pub struct Panes {
    pub local: Rgba,
    pub matched: Rgba,
    pub overlay: Rgba,
    pub verdict: String,
}

/// `build_difference_overlay` and the ramp, for the Differences pane. Heavy: call it from a worker.
pub fn build_panes(local: &Rgba, matched: &Rgba) -> Option<Arc<Panes>> {
    build_difference_overlay(local, matched).map(|v: DifferenceView| {
        Arc::new(Panes {
            local: ramp_picture(local),
            matched: ramp_picture(matched),
            verdict: v.diff.verdict.clone(),
            overlay: v.overlay,
        })
    })
}

/// `LoadedComparison`.
pub struct Loaded {
    pub local: Result<Rgba, PictureError>,
    pub matched: Option<Rgba>,
    pub matched_bytes: Option<Vec<u8>>,
    pub used_fallback: bool,
    pub candidate: Option<Candidate>,
}

/// `load_comparison`: the match at FULL resolution beside the local file. Deliberately not the
/// sample shown in the preview panel, which is a downscaled copy: comparing against it would
/// misrepresent exactly the quality difference a comparison exists to judge. Blocking; call it from
/// a worker.
pub fn load_comparison(
    handle: &tokio::runtime::Handle,
    engine: &hatate_core::search_engine::SearchEngine,
    settings: &std::sync::Mutex<hatate_core::config::Settings>,
    path: &str,
    mut candidate: Option<Candidate>,
) -> Loaded {
    let local = open_local(path);
    if let Err(e) = &local {
        tracing::warn!("Could not load local image for comparison: {}", e.detail());
    }
    let mut matched = None;
    let mut matched_bytes = None;
    let mut used_fallback = false;
    if let Some(c) = candidate.as_mut() {
        let has = |u: &Option<String>| u.as_deref().is_some_and(|u| !u.is_empty());
        // Only the candidate the user has visited has had its details fetched, and the signed URLs
        // of the others expire: fetch on demand, as the main window does when a candidate is picked.
        if !has(&c.direct_file_url) && !has(&c.preview_url) {
            tracing::info!(
                "Fetching details for the selected match before comparing: {}",
                c.url
            );
            if let Err(e) =
                handle.block_on(engine.fetch_candidate_details(c, settings, None, Some(path)))
            {
                tracing::warn!("Could not fetch match details for {}: {e}", c.url);
            }
        }
        // Prefer the real file; fall back to whatever preview there is rather than nothing, and
        // remember that we did, so the caller can say so.
        let direct = c.direct_file_url.clone().filter(|u| !u.is_empty());
        let url = match &direct {
            Some(u) => Some(u.clone()),
            None => {
                used_fallback = true;
                c.preview_url
                    .clone()
                    .filter(|u| !u.is_empty())
                    .or_else(|| c.thumb_url.clone().filter(|u| !u.is_empty()))
            }
        };
        if let Some(url) = url {
            tracing::info!("Fetching match at full resolution for comparison: {url}");
            let timeout = settings.lock().unwrap().hydrus.timeout;
            let referer = (Some(&url) == direct.as_ref()).then(|| c.url.clone());
            matched_bytes = match handle.block_on(hatate_core::remote::download_bytes(
                &engine.net,
                Some(&url),
                timeout,
                referer.as_deref(),
                None,
                None,
            )) {
                Ok(bytes) => bytes,
                Err(e) => {
                    tracing::warn!("Could not download the match for comparison: {e:?}");
                    None
                }
            };
        }
        if let Some(bytes) = &matched_bytes {
            matched = decode_rgba(bytes);
            if matched.is_none() {
                tracing::warn!(
                    "Fetched {} bytes for the match but could not decode them",
                    bytes.len()
                );
            }
        }
    }
    Loaded {
        local,
        matched,
        matched_bytes,
        used_fallback,
        candidate,
    }
}

/// A decoded picture as a texture.
pub fn texture_of(p: &Rgba) -> gdk::Texture {
    gdk::MemoryTexture::new(
        p.width as i32,
        p.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from_owned(p.rgba.clone()),
        p.width as usize * 4,
    )
    .upcast()
}

impl MainWindow {
    /// `_current_review_key`.
    pub fn current_review_key(&self) -> Option<ReviewKey> {
        let id = self.current_id()?;
        let q = self.s.queue.borrow();
        let url = q.entry(id)?.selected_candidate().map(|c| c.url.clone());
        Some((id, url))
    }

    /// `_show_comparison_controls`.
    fn show_comparison_controls(&self, shown: bool) {
        let r = &self.s.review;
        r.zoom_tools.set_visible(shown);
        r.slider_row.set_visible(shown);
        r.note.set_visible(shown);
        if !shown {
            r.error_detail.set_visible(false);
        }
    }

    fn set_review_note(&self, text: &str) {
        self.s.review.note.set_text(text);
        self.s.review.note.set_visible(true);
    }

    /// `_on_review_view_changed`: switches view, fetching the full-resolution pair if one is needed.
    pub fn on_review_view_changed(&self, key: &str) {
        let r = &self.s.review;
        if key == "pair" {
            r.stack.set_visible_child_name("pair");
            self.show_comparison_controls(false);
            return;
        }
        r.stack.set_visible_child_name("comparison");
        self.show_comparison_controls(true);
        if self.ensure_comparison_loaded() {
            self.apply_wipe_mode(key);
        }
    }

    /// `_ensure_comparison_loaded`: fetches the match at full resolution, once per entry and
    /// candidate. `true` when the pair is already in hand; otherwise the load is under way (or
    /// there is nothing to load) and [`Self::on_comparison_loaded`] applies the view.
    fn ensure_comparison_loaded(&self) -> bool {
        let r = &self.s.review;
        let Some(key) = self.current_review_key() else {
            self.set_review_note("Nothing selected.");
            r.wipe.set_images(None, None);
            return false;
        };
        {
            let c = self.s.comparison.borrow();
            if c.loaded_for.as_ref() == Some(&key) {
                return true;
            }
            if c.loading {
                return false;
            }
        }
        let (path, candidate) = {
            let q = self.s.queue.borrow();
            let Some(e) = q.entry(key.0) else {
                return false;
            };
            (e.path.clone(), e.selected_candidate().cloned())
        };
        let Ok(backend) = self.s.search.backend(&self.s.paths) else {
            self.set_review_note(
                "Could not load the match: the search engines could not be built.",
            );
            return false;
        };
        let Some(handle) = self.s.workers.handle() else {
            return false;
        };
        let generation = {
            let mut c = self.s.comparison.borrow_mut();
            c.loading = true;
            c.generation += 1;
            c.generation
        };
        self.set_review_note("Loading the match at full resolution\u{2026}");
        r.wipe.set_images(None, None);
        let settings = self.s.settings.clone();
        let engine = backend.engine.clone();
        let w = self.downgrade();
        let job = self.s.workers.run(
            move |_ctx: &crate::worker::Ctx<()>| {
                load_comparison(&handle, &engine, &settings, &path, candidate)
            },
            move |event: Event<(), Loaded>| {
                let Some(w) = w.upgrade() else { return };
                if let Event::Done(loaded) = event {
                    w.on_comparison_loaded(key.clone(), generation, loaded);
                }
            },
        );
        self.s.comparison_worker.replace(job);
        false
    }

    fn on_comparison_loaded(&self, key: ReviewKey, generation: u64, loaded: Loaded) {
        if self.s.closing.get() {
            return;
        }
        {
            let mut c = self.s.comparison.borrow_mut();
            if c.generation != generation {
                return;
            }
            c.loading = false;
        }
        // The selection moved on while the match came down: it is not the pair on screen now.
        if self.current_review_key().as_ref() != Some(&key) {
            self.sync_comparison_to_selection();
            return;
        }
        let r = &self.s.review;
        let (local, error) = match loaded.local {
            Ok(p) => (Some(Arc::new(p)), None),
            Err(e) => (None, Some(e)),
        };
        let matched = loaded.matched.map(Arc::new);
        r.wipe.clear_difference_view();
        r.wipe.set_images(local.as_deref(), matched.as_deref());
        if let (Some(m), false) = (&matched, loaded.used_fallback) {
            // The full original is in hand now, so Side by side shows it too instead of the
            // 150-850 px sample it started with.
            r.matched.show_texture(&texture_of(m));
        }
        let mut notes = Vec::new();
        if matched.is_none() {
            notes.push("No match image could be fetched.");
        } else if loaded.used_fallback {
            notes.push(
                "Showing the downscaled preview - the full file was not available, so this \
                 understates the quality difference.",
            );
        }
        let diff_ok = local.is_some() && matched.is_some();
        {
            let mut c = self.s.comparison.borrow_mut();
            c.loaded_for = Some(key);
            c.diff_for = None;
            c.diff = None;
            c.local = local;
            c.matched = matched;
            c.notes = notes.join("  ");
            c.error = error;
        }
        if let Some(d) = r.view_buttons.get("diff") {
            d.set_sensitive(diff_ok);
        }
        self.show_error_state();
        let view = r.current_view();
        if view != "pair" {
            self.apply_wipe_mode(&view);
        }
    }

    /// The error state (design gate 2.8): the plain-language message in the note, the raw text in a
    /// detail line, the slider and the hint insensitive. Leaves it when the local picture is fine.
    fn show_error_state(&self) {
        let r = &self.s.review;
        let error = self.s.comparison.borrow().error.clone();
        match &error {
            Some(e) => {
                tracing::warn!(
                    "The comparison cannot show the local picture: {}",
                    e.detail()
                );
                r.note.set_text(&e.user_message());
                r.error_detail.set_text(&e.detail());
                r.error_detail.set_tooltip_text(Some(&e.detail()));
                r.error_detail.set_visible(true);
            }
            None => r.error_detail.set_visible(false),
        }
        r.wipe_slider.set_sensitive(error.is_none());
    }

    /// `_apply_wipe_mode`.
    fn apply_wipe_mode(&self, key: &str) {
        let r = &self.s.review;
        if self.s.comparison.borrow().error.is_some() {
            // Python's `clear_difference_view` -> `zoom_to_fit` runs for every non-diff view.
            r.wipe.clear_difference_view();
            self.show_error_state();
            return;
        }
        if key == "diff" {
            self.show_differences();
        } else {
            r.wipe.clear_difference_view();
            // A load warning outranks the how-to line: it is the one the user needs to read.
            let notes = self.s.comparison.borrow().notes.clone();
            self.set_review_note(if notes.is_empty() { WIPE_HINT } else { &notes });
        }
    }

    /// `_show_differences`: builds the difference overlay, cached per loaded pair.
    fn show_differences(&self) {
        let (local, matched, key, cached) = {
            let c = self.s.comparison.borrow();
            let cached =
                (c.diff_for == c.loaded_for && c.diff.is_some()).then(|| c.diff.clone().flatten());
            (
                c.local.clone(),
                c.matched.clone(),
                c.loaded_for.clone(),
                cached,
            )
        };
        let (Some(local), Some(matched)) = (local, matched) else {
            self.set_review_note("Both pictures are needed to compare them.");
            return;
        };
        match cached {
            Some(panes) => self.present_differences(panes),
            None => {
                self.set_review_note("Comparing the two\u{2026}");
                let generation = self.s.comparison.borrow().generation;
                let (l, m) = (local.clone(), matched.clone());
                let w = self.downgrade();
                let job = self.s.workers.run(
                    move |_ctx: &crate::worker::Ctx<()>| build_panes(&l, &m),
                    move |event: Event<(), Option<Arc<Panes>>>| {
                        let Some(w) = w.upgrade() else { return };
                        if let Event::Done(panes) = event {
                            w.on_differences_built(key.clone(), generation, panes);
                        }
                    },
                );
                self.s.diff_worker.replace(job);
            }
        }
    }

    fn on_differences_built(
        &self,
        key: Option<ReviewKey>,
        generation: u64,
        panes: Option<Arc<Panes>>,
    ) {
        {
            let mut c = self.s.comparison.borrow_mut();
            if c.generation != generation || c.loaded_for != key {
                return;
            }
            c.diff_for = key;
            c.diff = Some(panes.clone());
        }
        if self.s.review.current_view() == "diff" {
            self.present_differences(panes);
        }
    }

    fn present_differences(&self, panes: Option<Arc<Panes>>) {
        let r = &self.s.review;
        match panes {
            None => {
                self.set_review_note(
                    "Could not compare these two - too much of the frame has moved for a \
                     difference map to mean anything.",
                );
                r.wipe.clear_difference_view();
            }
            Some(p) => {
                r.wipe.set_difference_view(&p.local, &p.matched, &p.overlay);
                r.wipe.set_position(r.wipe_slider.value() / 100.0);
                self.set_review_note(&format!(
                    "{}  \u{b7}  Local on the left, match on the right - drag the wipe to sweep \
                     the highlights across both.",
                    p.verdict
                ));
            }
        }
    }

    /// The top of `refresh_review`: the pair on screen is no longer the pair selected, so it is let go,
    /// and a comparison that is showing fetches the new one.
    pub fn sync_comparison_to_selection(&self) {
        let r = &self.s.review;
        let key = self.current_review_key();
        {
            let c = self.s.comparison.borrow();
            if key == c.loaded_for {
                return;
            }
        }
        {
            let mut c = self.s.comparison.borrow_mut();
            c.loaded_for = None;
            c.diff_for = None;
            c.diff = None;
            c.local = None;
            c.matched = None;
            c.notes.clear();
            c.error = None;
            c.generation += 1;
            c.loading = false;
        }
        self.s.comparison_worker.retire();
        self.s.diff_worker.retire();
        r.wipe.clear_difference_view();
        r.wipe.set_images(None, None);
        r.note.set_text("");
        r.error_detail.set_visible(false);
        r.wipe_slider.set_sensitive(true);
        if let Some(d) = r.view_buttons.get("diff") {
            d.set_sensitive(true);
        }
        if r.stack.visible_child_name().as_deref() == Some("comparison") {
            // Already looking at the comparison, so fetch the new one.
            let view = r.current_view();
            self.on_review_view_changed(if view == "pair" { "wipe" } else { &view });
        }
    }

    /// `full_resolution_match`: the match at full resolution, if the comparison has fetched it for
    /// this entry and its selected candidate.
    pub fn full_resolution_match(&self, id: EntryId) -> Option<Arc<Rgba>> {
        let c = self.s.comparison.borrow();
        let key = self.current_review_key()?;
        (key.0 == id && c.loaded_for.as_ref() == Some(&key))
            .then(|| c.matched.clone())
            .flatten()
    }

    /// The signals of the view switch.
    pub fn wire_review_compare(&self) {
        for (key, button) in self.s.review.view_buttons.iter() {
            let w = self.downgrade();
            let key = key.clone();
            button.connect_toggled(move |b| {
                if b.is_active() {
                    if let Some(w) = w.upgrade() {
                        w.on_review_view_changed(&key);
                    }
                }
            });
        }
    }
}
