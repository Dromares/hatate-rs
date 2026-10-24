//! The core half of the row thumbnails (DAN-2116 item 3, `phase6-calls` call 4): the state a
//! thumbnail is in, the size contract and the cache key the window's thumbnail cache (6.d) uses.
//! No decoding and no texture here: the cache is generic over what the window stores.
//!
//! Python at `76c85c4`: `gui/main_window.py:113-142` (the sizes), `:2035-2090` (generation and
//! the icon cache), `workers/thumbnail_worker.py:55-95` (the scaled decode).

use crate::lru::LruCache;

/// `THUMB_COLUMN_SIZE`: the row icon, in pixels at scale 1.
pub const THUMB_COLUMN_SIZE: u32 = 40;
/// `QUEUE_ROW_HEIGHT`: the row, hairline included (48 + 1). The thumbnail keeps its rect between
/// states; the frame is a stroke over it (call 4).
pub const QUEUE_ROW_HEIGHT: u32 = 49;
/// `THUMB_DECODE_QUALITY_FACTOR`: the worker decodes at this multiple of the display size and
/// the window filters it down, which is smoother than a direct decode at the final size.
pub const THUMB_DECODE_QUALITY_FACTOR: u32 = 4;
/// `THUMB_ICON_CACHE_ENTRIES`: how many thumbnails the window keeps.
pub const THUMB_ICON_CACHE_ENTRIES: usize = 4000;

/// The largest scale (device pixels per logical pixel) a thumbnail is drawn at. GTK reports an
/// integer scale; a value past this is drawn from the same decode.
pub const MAX_SCALE: u32 = 4;

/// What a row's thumbnail cell shows (`phase6-calls` call 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbState {
    /// Not decoded yet, or not requested yet under lazy thumbnails: the halftone.
    Loading,
    /// The decode failed (missing file, unreadable, not a picture): the `×` glyph and the
    /// path-free tooltip [`FAILED_TOOLTIP`].
    Failed,
    /// The picture.
    Loaded,
}

/// The Failed state's tooltip. No path.
pub const FAILED_TOOLTIP: &str =
    "Thumbnail unavailable \u{2014} the file is missing or could not be read.";

/// The pixel size the worker decodes at: the display size times the quality factor
/// (`THUMB_COLUMN_SIZE * THUMB_DECODE_QUALITY_FACTOR`), whatever the screen's density.
pub fn decode_px() -> u32 {
    THUMB_COLUMN_SIZE * THUMB_DECODE_QUALITY_FACTOR
}

/// The pixel size of the texture drawn into the 40 px cell at an integer `scale`: 40 at scale 1,
/// 80 at 2. A scale of 0 is 1; past [`MAX_SCALE`] it is capped.
pub fn texture_px(scale: u32) -> u32 {
    THUMB_COLUMN_SIZE * scale.clamp(1, MAX_SCALE)
}

/// `QSize.scaled(QSize(limit, limit), KeepAspectRatio)` for a picture of `width` x `height`: the
/// size it is decoded at when either side is past `limit`; unchanged otherwise
/// (`ThumbnailWorker._decode`, `_decode_from_bytes`). Integer arithmetic as Qt does it. A side
/// of 0 is returned as it is.
pub fn fit_within(width: u32, height: u32, limit: u32) -> (u32, u32) {
    if width == 0 || height == 0 || (width <= limit && height <= limit) {
        return (width, height);
    }
    let (w, h, l) = (u64::from(width), u64::from(height), u64::from(limit));
    // Qt: scale to the limit's height, and if that is too wide scale to its width.
    let by_height = l * w / h;
    if by_height <= l {
        (by_height as u32, limit)
    } else {
        (limit, (l * h / w) as u32)
    }
}

/// What a cached thumbnail is filed under: the row's file and the decode size. The window's
/// `id(entry)` of Python is an in-memory identity; the path survives a re-sort, a filter and a
/// reload of the session, and two rows never share one. The decode size is in it so a change of
/// the contract does not serve a thumbnail made at the old one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbKey {
    pub path: String,
    pub decode_px: u32,
}

impl ThumbKey {
    /// The key a row's thumbnail is filed under now.
    pub fn of(path: impl Into<String>) -> ThumbKey {
        ThumbKey {
            path: path.into(),
            decode_px: decode_px(),
        }
    }
}

#[derive(Clone)]
enum Slot<V> {
    Failed,
    Loaded(V),
}

/// The thumbnail cache: [`THUMB_ICON_CACHE_ENTRIES`] rows, least recently used out first. A key
/// that is not in it is [`ThumbState::Loading`]. Generic over what the window keeps (a texture).
#[derive(Clone)]
pub struct ThumbBook<V: Clone> {
    cache: LruCache<ThumbKey, Slot<V>>,
}

impl<V: Clone> Default for ThumbBook<V> {
    fn default() -> Self {
        ThumbBook::new(THUMB_ICON_CACHE_ENTRIES)
    }
}

impl<V: Clone> ThumbBook<V> {
    pub fn new(max_entries: usize) -> Self {
        ThumbBook {
            cache: LruCache::new(max_entries),
        }
    }

    /// A decode finished. Looking at a row counts as a use, as in Python's `LRUCache.get`.
    pub fn loaded(&mut self, key: ThumbKey, value: V) {
        self.cache.set(key, Slot::Loaded(value));
    }

    /// A decode failed.
    pub fn failed(&mut self, key: ThumbKey) {
        self.cache.set(key, Slot::Failed);
    }

    /// The state of a cell, and the picture when there is one.
    pub fn get(&mut self, key: &ThumbKey) -> (ThumbState, Option<V>) {
        match self.cache.get(key) {
            None => (ThumbState::Loading, None),
            Some(Slot::Failed) => (ThumbState::Failed, None),
            Some(Slot::Loaded(v)) => (ThumbState::Loaded, Some(v)),
        }
    }

    /// Whether anything is known of this key (a row to ask the worker for is one for which this
    /// is false).
    pub fn knows(&self, key: &ThumbKey) -> bool {
        self.cache.contains(key)
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sizes_are_pythons() {
        assert_eq!(THUMB_COLUMN_SIZE, 40);
        assert_eq!(QUEUE_ROW_HEIGHT, 49);
        assert_eq!(decode_px(), 160);
        assert_eq!(THUMB_ICON_CACHE_ENTRIES, 4000);
    }

    #[test]
    fn density_sets_the_texture_and_never_the_decode() {
        assert_eq!(
            [0, 1, 2, 3, 4, 9].map(texture_px),
            [40, 40, 80, 120, 160, 160]
        );
        assert_eq!(decode_px(), 160, "the decode is the same at every density");
        assert_eq!(
            texture_px(4),
            decode_px(),
            "and is enough for the densest cell"
        );
    }

    #[test]
    fn a_picture_is_fitted_the_way_qt_scales_it() {
        // `QSize.scaled(QSize(160, 160), KeepAspectRatio)`; unchanged when it already fits.
        assert_eq!(fit_within(100, 50, 160), (100, 50));
        assert_eq!(fit_within(160, 160, 160), (160, 160));
        assert_eq!(fit_within(1600, 800, 160), (160, 80));
        assert_eq!(fit_within(800, 1600, 160), (80, 160));
        assert_eq!(fit_within(3000, 1001, 160), (160, 53));
        assert_eq!(
            fit_within(1000, 3, 160),
            (160, 0),
            "Qt truncates, so a sliver can reach 0"
        );
        assert_eq!(fit_within(0, 500, 160), (0, 500));
    }

    #[test]
    fn a_cell_is_loading_until_a_decode_lands_then_loaded_or_failed() {
        let mut book: ThumbBook<&str> = ThumbBook::new(2);
        let a = ThumbKey::of("/pictures/a.png");
        let b = ThumbKey::of("/pictures/b.png");
        assert_eq!(book.get(&a), (ThumbState::Loading, None));
        assert!(!book.knows(&a));
        book.loaded(a.clone(), "texture-a");
        book.failed(b.clone());
        assert_eq!(book.get(&a), (ThumbState::Loaded, Some("texture-a")));
        assert_eq!(book.get(&b), (ThumbState::Failed, None));
        assert!(book.knows(&b));
    }

    #[test]
    fn the_least_recently_used_thumbnail_goes_first() {
        let mut book: ThumbBook<u8> = ThumbBook::new(2);
        let (a, b, c) = (ThumbKey::of("a"), ThumbKey::of("b"), ThumbKey::of("c"));
        book.loaded(a.clone(), 1);
        book.loaded(b.clone(), 2);
        let _ = book.get(&a); // a is looked at, so b is the one that goes
        book.loaded(c.clone(), 3);
        assert_eq!(
            book.get(&b).0,
            ThumbState::Loading,
            "evicted: asked for again"
        );
        assert_eq!(book.get(&a).0, ThumbState::Loaded);
        assert_eq!(book.len(), 2);
    }

    #[test]
    fn the_key_is_the_file_and_the_decode_size_and_the_tooltip_names_no_path() {
        assert_eq!(
            ThumbKey::of("/p/a.png"),
            ThumbKey {
                path: "/p/a.png".into(),
                decode_px: 160
            }
        );
        assert_ne!(
            ThumbKey::of("/p/a.png"),
            ThumbKey {
                path: "/p/a.png".into(),
                decode_px: 80
            },
            "a thumbnail made at another size is another thumbnail"
        );
        assert!(!FAILED_TOOLTIP.contains('/') && !FAILED_TOOLTIP.contains("os error"));
    }
}
