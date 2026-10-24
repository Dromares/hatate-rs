//! Pure viewport arithmetic for lazy thumbnail loading (`core/viewport.py`): which rows get work
//! done for them, decided without a window.

use std::collections::HashSet;

/// `visible_range_with_buffer`: expands a visible row range by a buffer, clamped to the list.
///
/// `first` and `last` come from the view's row-at-position query, which answers `-1` when the
/// coordinate falls past the last row; that is a normal case, handled here: `first < 0` is the
/// top of the list, `last < 0` the final row. A swapped pair is swapped back rather than read as
/// an empty range. `None` for an empty list. Rows are `i64` because `-1` is a value.
pub fn visible_range_with_buffer(
    first: i64,
    last: i64,
    row_count: i64,
    buffer_rows: i64,
) -> Option<(i64, i64)> {
    if row_count <= 0 {
        return None;
    }
    let mut first = if first < 0 { 0 } else { first };
    let mut last = if last < 0 { row_count - 1 } else { last };
    if last < first {
        std::mem::swap(&mut first, &mut last);
    }
    let start = (first - buffer_rows).max(0);
    let end = (row_count - 1).min(last + buffer_rows);
    Some((start, end))
}

/// `ThumbnailWork`: what a viewport pass found in the rows it looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailWork<T> {
    /// Entries with no thumbnail and none being made.
    pub needed: Vec<T>,
    /// Had one already.
    pub already_cached: usize,
    /// A worker is making one right now.
    pub in_flight: usize,
}

impl<T> ThumbnailWork<T> {
    /// `examined`.
    pub fn examined(&self) -> usize {
        self.needed.len() + self.already_cached + self.in_flight
    }
}

/// `entries_needing_thumbnails`: splits the rows in view into work to do and work already done.
/// A row that is mid-generation is not work to do, or a scroll would queue a second worker for it
/// on every debounce tick. Rows are keyed by `id` (an entry's identity, not its path: the same
/// file added twice is two rows and each needs its own thumbnail).
pub fn entries_needing_thumbnails<T>(
    candidates: impl IntoIterator<Item = T>,
    id_of: impl Fn(&T) -> usize,
    is_cached: impl Fn(usize) -> bool,
    in_flight_ids: &HashSet<usize>,
) -> ThumbnailWork<T> {
    let (mut needed, mut already_cached, mut in_flight) = (Vec::new(), 0, 0);
    for entry in candidates {
        let id = id_of(&entry);
        if is_cached(id) {
            already_cached += 1;
        } else if in_flight_ids.contains(&id) {
            in_flight += 1;
        } else {
            needed.push(entry);
        }
    }
    ThumbnailWork {
        needed,
        already_cached,
        in_flight,
    }
}
