//! What each cell of the queue table says, how it is inked, and how each column sorts
//! (`gui/image_table_model.py` at `76c85c4`, the model half; DAN-2217). No widget here: the window
//! (`hatate-gtk`) renders these answers, and the differential compares them with Python's.
//!
//! The tier tables of 6.a ([`crate::status_tier`]) own the Status, Sent, Reviewed and Upscale glyphs
//! and ranks; this module adds the cell text of the other columns, the foreground tier of each
//! cell, the tooltips, the sort keys, the default column widths and the saved-layout check.

use crate::image_compare::{SAME_SIZE_TOLERANCE, compare_sizes, size_delta_sort_key};
use crate::model::{Entry, MatchStatus};
use crate::similarity_display::{similarity_label, similarity_tooltip};
use crate::status_tier::{
    EMPTY_CELL, SentTier, StatusTier, UpscaleTier, reviewed_cell_text, reviewed_rank,
};
use std::cmp::Ordering;

/// `COLUMNS` (`image_table_model.py:41`): the header titles, in order.
pub const COLUMNS: [&str; 12] = [
    "Thumb",
    "File",
    "Status",
    "Engine",
    "Similarity",
    "Size diff.",
    "Booru",
    "Tags",
    "Cache",
    "Sent",
    "Reviewed",
    "Upscale",
];

/// `COL_THUMB` .. `COL_UPSCALE` (`image_table_model.py:48-50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Column {
    Thumb = 0,
    File = 1,
    Status = 2,
    Engine = 3,
    Similarity = 4,
    SizeDelta = 5,
    Booru = 6,
    Tags = 7,
    Cache = 8,
    Sent = 9,
    Reviewed = 10,
    Upscale = 11,
}

impl Column {
    pub const ALL: [Column; 12] = [
        Column::Thumb,
        Column::File,
        Column::Status,
        Column::Engine,
        Column::Similarity,
        Column::SizeDelta,
        Column::Booru,
        Column::Tags,
        Column::Cache,
        Column::Sent,
        Column::Reviewed,
        Column::Upscale,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Column> {
        Column::ALL.get(i).copied()
    }

    /// The header title (`COLUMNS[section]`).
    pub fn title(self) -> &'static str {
        COLUMNS[self.index()]
    }
}

/// `HEADER_TRACKING`: percent, `tokens.css`'s 0.1em on the table headers.
pub const HEADER_TRACKING: i32 = 110;
/// `CELL_TRACKING`: percent, `tokens.css`'s 0.06em on `.upscale--*`.
pub const CELL_TRACKING: i32 = 106;

/// The cell text of an entry that was in flight when the previous run died (S-02).
pub const INTERRUPTED_MARKER: &str = "\u{2014} interrupted mid-search";

/// `_MISSING_TOOLTIP`.
pub const MISSING_TOOLTIP: &str = "This file is no longer on disk - most likely deleted from Hydrus.\nIts tags and matched URL are still intact, and \"Send URL to Hydrus's URL Importer\" still works for it.";
/// `_REVIEWED_TOOLTIP`.
pub const REVIEWED_TOOLTIP: &str = "You looked at this one and decided against sending it - the local copy is better, or the match is wrong.\n\nSeparate from Sent because both are decisions, and a row with neither mark is one nobody has got to yet. That is what makes a long review pass resumable: right-click > Select by Review State > Needs review, or press N to jump to the next one.";
/// `_SIZE_DELTA_TOOLTIP`.
pub const SIZE_DELTA_TOOLTIP: &str = "How the matched image's dimensions compare with your local file, on the long edge. \"\u{25b2} 4\u{d7}\" means the match is four times the size; \"0.25\u{d7}\" means it's a quarter of it; \"=\" means they're the same. A dash when the match's size isn't known.";
/// `_UPSCALE_NOT_CHECKED_TOOLTIP`.
pub const UPSCALE_NOT_CHECKED_TOOLTIP: &str =
    "Right-click > Check for Upscaling has not been run on this image yet.";

/// The ink tiers a cell can be drawn in: `ink_45` is under the text floor and only the "same size"
/// and the missing-file cells use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellInk {
    Ink45,
    Ink65,
    Ink100,
}

impl CellInk {
    /// The token name in `gui/theme.py`.
    pub fn token(self) -> &'static str {
        match self {
            CellInk::Ink45 => "ink_45",
            CellInk::Ink65 => "ink_65",
            CellInk::Ink100 => "ink_100",
        }
    }
}

/// The chip a cell is drawn as (`CHIP_ROLE`, `_chip`): `None` is ordinary text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Status(MatchStatus),
    Sent(SentTier),
    Reviewed,
    /// An unmeasured similarity: the chip keeps the cell's own text (`ESTIMATE_CHIP`).
    Estimated,
}

impl Chip {
    /// The key `_chip` returns in Python (`status.value`, `"sent"`, `"queued"`, `"reviewed"`,
    /// `"estimated"`).
    pub fn key(self) -> &'static str {
        match self {
            Chip::Status(s) => s.as_str(),
            Chip::Sent(SentTier::Sent) => "sent",
            Chip::Sent(_) => "queued",
            Chip::Reviewed => "reviewed",
            Chip::Estimated => "estimated",
        }
    }
}

/// `_entry_engine_label`: which engine found the selected match; empty when none.
pub fn engine_label(entry: &Entry) -> String {
    entry
        .selected_candidate()
        .map(|c| c.engine.clone())
        .unwrap_or_default()
}

/// `_is_interrupted`: the restore's "was in flight" note, only while the row is unsearched.
pub fn is_interrupted(entry: &Entry) -> bool {
    entry.interrupted_mid_search && entry.status == MatchStatus::NotSearched
}

/// `_entry_cache_label`: whether the result came from the cache or a fresh search.
pub fn cache_label(entry: &Entry) -> &'static str {
    if entry.searched_without_saucenao {
        // Distinct from "Fresh": searched without SauceNAO on a spent allowance, not cached.
        return "Provisional";
    }
    match entry.result_source.as_deref() {
        Some("cached") => "Cached",
        Some("fresh") => "Fresh",
        _ => "",
    }
}

/// `_entry_size_scale`: the match's long edge over the local file's, `None` when either is unknown.
pub fn size_scale(entry: &Entry) -> Option<f64> {
    let candidate = entry.selected_candidate()?;
    // Python passes `x or 0`; a zero side is "unknown", which `compare_sizes` already answers.
    compare_sizes(
        Some(entry.local_width.unwrap_or(0)),
        Some(entry.local_height.unwrap_or(0)),
        Some(candidate.width.unwrap_or(0)),
        Some(candidate.height.unwrap_or(0)),
    )
    .scale
}

/// `_scale_text`: one decimal for ordinary ratios, none from 100 up (with a thousands comma),
/// `<0.1` below a tenth.
pub fn scale_text(scale: f64) -> String {
    if scale >= 100.0 {
        return group_thousands(scale.round_ties_even() as i64);
    }
    if scale < 0.1 {
        return "<0.1".to_owned();
    }
    format!("{scale:.1}")
}

/// `f"{n:,}"`.
fn group_thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

/// `_entry_size_delta_label`: `2.5×`, `0.4×`, `=`, or the dash.
pub fn size_delta_label(entry: &Entry) -> String {
    match size_scale(entry) {
        None => EMPTY_CELL.to_owned(),
        Some(scale) if (scale - 1.0).abs() <= SAME_SIZE_TOLERANCE => "=".to_owned(),
        Some(scale) => format!("{}\u{d7}", scale_text(scale)),
    }
}

/// `_size_delta_glyph`: `▲` for an upgrade, nothing otherwise.
pub fn size_delta_glyph(entry: &Entry) -> Option<char> {
    match size_scale(entry) {
        Some(scale) if scale > 1.0 + SAME_SIZE_TOLERANCE => Some('\u{25b2}'),
        _ => None,
    }
}

/// `_size_delta_weight`: `ink_100` for an upgrade, `ink_65` for a downgrade or an unknown size,
/// `ink_45` for "=".
pub fn size_delta_ink(entry: &Entry) -> CellInk {
    match size_scale(entry) {
        None => CellInk::Ink65,
        Some(s) if s > 1.0 + SAME_SIZE_TOLERANCE => CellInk::Ink100,
        Some(s) if s < 1.0 - SAME_SIZE_TOLERANCE => CellInk::Ink65,
        Some(_) => CellInk::Ink45,
    }
}

/// The text `data(DisplayRole)` returns for a cell; `None` for the Thumb column (an icon).
pub fn cell_text(entry: &Entry, column: Column) -> Option<String> {
    Some(match column {
        Column::Thumb => return None,
        // Marked in the File column rather than by changing Status, so the result the row
        // still holds stays useful.
        Column::File if entry.file_missing => format!("{}  (missing)", entry.filename()),
        Column::File => entry.filename().to_owned(),
        Column::Status => entry.status.label().to_owned(),
        Column::Engine => {
            let label = engine_label(entry);
            if !label.is_empty() {
                label
            } else if is_interrupted(entry) {
                INTERRUPTED_MARKER.to_owned()
            } else {
                EMPTY_CELL.to_owned()
            }
        }
        Column::Similarity => dash_if_empty(similarity_label(
            entry.similarity(),
            entry.similarity_measured(),
        )),
        Column::SizeDelta => {
            let label = size_delta_label(entry);
            match size_delta_glyph(entry) {
                Some(g) => format!("{g} {label}"),
                None => label,
            }
        }
        Column::Booru => dash_if_empty(entry.booru_name().unwrap_or("").to_owned()),
        Column::Tags => entry.tags.len().to_string(),
        Column::Cache => dash_if_empty(cache_label(entry).to_owned()),
        Column::Sent => SentTier::of(entry).cell_text().to_owned(),
        Column::Reviewed => reviewed_cell_text(entry).to_owned(),
        Column::Upscale => UpscaleTier::cell_text(entry),
    })
}

fn dash_if_empty(s: String) -> String {
    if s.is_empty() {
        EMPTY_CELL.to_owned()
    } else {
        s
    }
}

/// `_chip`: the chip a cell is drawn as. Status and Sent are the two columns read by the dozen
/// down a long list; only an unmeasured similarity is chipped.
pub fn chip(entry: &Entry, column: Column) -> Option<Chip> {
    match column {
        Column::Status => Some(Chip::Status(entry.status)),
        Column::Sent if entry.sent_to_hydrus => Some(Chip::Sent(SentTier::of(entry))),
        Column::Reviewed if entry.reviewed => Some(Chip::Reviewed),
        Column::Similarity if entry.similarity().is_some() && !entry.similarity_measured() => {
            Some(Chip::Estimated)
        }
        _ => None,
    }
}

/// `_foreground`: the ink a cell overrides the table's own text colour with, `None` for the
/// table's default.
pub fn foreground(entry: &Entry, column: Column) -> Option<CellInk> {
    if column == Column::File && entry.file_missing {
        // A missing file fades like a disabled control: ink_100 would match the unstyled text.
        return Some(CellInk::Ink45);
    }
    if column == Column::Engine && engine_label(entry).is_empty() && is_interrupted(entry) {
        return Some(CellInk::Ink65);
    }
    if column == Column::Reviewed && entry.reviewed {
        return Some(CellInk::Ink65);
    }
    let dashed = matches!(
        column,
        Column::Engine
            | Column::Similarity
            | Column::Booru
            | Column::Cache
            | Column::Sent
            | Column::Reviewed
    );
    if dashed && cell_text(entry, column).as_deref() == Some(EMPTY_CELL) {
        // A placeholder is text, so it sits on the tier that clears 4.5:1 (DAN-211).
        return Some(CellInk::Ink65);
    }
    match column {
        Column::SizeDelta => Some(size_delta_ink(entry)),
        Column::Upscale => Some(match UpscaleTier::of(entry) {
            UpscaleTier::Flagged => CellInk::Ink100,
            _ => CellInk::Ink65,
        }),
        _ => None,
    }
}

/// Whether the cell is drawn in italics (`FontRole` on the Engine column of an interrupted row).
pub fn italic(entry: &Entry, column: Column) -> bool {
    column == Column::Engine && engine_label(entry).is_empty() && is_interrupted(entry)
}

/// `_tooltip`.
pub fn tooltip(entry: &Entry, column: Column) -> Option<String> {
    match column {
        Column::File if entry.file_missing => Some(MISSING_TOOLTIP.to_owned()),
        Column::SizeDelta => Some(SIZE_DELTA_TOOLTIP.to_owned()),
        Column::Reviewed => Some(REVIEWED_TOOLTIP.to_owned()),
        Column::Similarity => {
            let t = similarity_tooltip(entry.similarity(), entry.similarity_measured());
            (!t.is_empty()).then_some(t)
        }
        Column::Status => Some(entry.status.label().to_owned()),
        Column::Upscale => Some(
            entry
                .upscale_check_detail
                .clone()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| UPSCALE_NOT_CHECKED_TOOLTIP.to_owned()),
        ),
        _ => None,
    }
}

/// A sort key: Python sorts a column by one kind of value, so a comparison across kinds never
/// happens; [`SortKey::cmp`] orders the kinds only so it is total.
#[derive(Debug, Clone, PartialEq)]
pub enum SortKey {
    Int(i64),
    Float(f64),
    Text(String),
}

impl SortKey {
    pub fn compare(&self, other: &SortKey) -> Ordering {
        match (self, other) {
            (SortKey::Int(a), SortKey::Int(b)) => a.cmp(b),
            (SortKey::Float(a), SortKey::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (SortKey::Text(a), SortKey::Text(b)) => compare_code_points(a, b),
            _ => Ordering::Equal,
        }
    }
}

/// Python compares `str`s by code point; Rust's `str::cmp` compares UTF-8 bytes, which orders the
/// same way.
fn compare_code_points(a: &str, b: &str) -> Ordering {
    a.cmp(b)
}

/// `sort_key_for_column(column)(entry)`.
pub fn sort_key(entry: &Entry, column: Column) -> SortKey {
    match column {
        Column::Thumb => SortKey::Int(0),
        Column::File => SortKey::Text(entry.filename().to_lowercase()),
        Column::Status => SortKey::Int(i64::from(StatusTier::of(entry).rank())),
        Column::Engine => SortKey::Text(engine_label(entry).to_lowercase()),
        Column::Similarity => SortKey::Float(entry.similarity().unwrap_or(-1.0)),
        Column::SizeDelta => {
            let c = entry.selected_candidate();
            SortKey::Float(size_delta_sort_key(
                Some(entry.local_width.unwrap_or(0)),
                Some(entry.local_height.unwrap_or(0)),
                Some(c.and_then(|c| c.width).unwrap_or(0)),
                Some(c.and_then(|c| c.height).unwrap_or(0)),
            ))
        }
        Column::Booru => SortKey::Text(entry.booru_name().unwrap_or("").to_lowercase()),
        Column::Tags => SortKey::Int(entry.tags.len() as i64),
        Column::Cache => SortKey::Text(entry.result_source.as_deref().unwrap_or("").to_lowercase()),
        Column::Sent => SortKey::Int(i64::from(SentTier::of(entry).rank())),
        Column::Reviewed => SortKey::Int(i64::from(reviewed_rank(entry))),
        Column::Upscale => SortKey::Int(i64::from(UpscaleTier::of(entry).rank())),
    }
}

/// `sorted(entries, key=sort_key_for_column(column), reverse=descending)`: stable, and a reversed
/// sort keeps equal rows in their original order (Python's `reverse=True` does).
pub fn sorted_order(entries: &[Entry], column: Column, descending: bool) -> Vec<usize> {
    let keys: Vec<SortKey> = entries.iter().map(|e| sort_key(e, column)).collect();
    let mut order: Vec<usize> = (0..entries.len()).collect();
    if descending {
        order.sort_by(|&a, &b| keys[b].compare(&keys[a]));
    } else {
        order.sort_by(|&a, &b| keys[a].compare(&keys[b]));
    }
    order
}

/// `DEFAULT_COLUMN_SHARES`: first-run widths as shares of the width left after the thumbnail.
/// The last column (Upscale) stretches and has no entry.
pub const DEFAULT_COLUMN_SHARES: [(Column, f64); 10] = [
    (Column::File, 0.18),
    (Column::Status, 0.10),
    (Column::Engine, 0.08),
    (Column::Similarity, 0.09),
    (Column::SizeDelta, 0.09),
    (Column::Booru, 0.09),
    (Column::Tags, 0.05),
    (Column::Cache, 0.06),
    (Column::Sent, 0.07),
    (Column::Reviewed, 0.08),
];

/// `DEFAULT_COLUMN_MINIMUMS`: below these the tracked header or the longest chip elides.
pub const DEFAULT_COLUMN_MINIMUMS: [(Column, i32); 10] = [
    (Column::File, 140),
    (Column::Status, 120),
    (Column::Engine, 80),
    (Column::Similarity, 100),
    (Column::SizeDelta, 100),
    (Column::Booru, 80),
    (Column::Tags, 60),
    (Column::Cache, 70),
    (Column::Sent, 80),
    (Column::Reviewed, 100),
];

/// `default_column_widths(viewport_width, thumb_width)`: `{column: width}` for a first run.
pub fn default_column_widths(viewport_width: i32, thumb_width: i32) -> Vec<(Column, i32)> {
    let room = (viewport_width - thumb_width).max(0);
    DEFAULT_COLUMN_SHARES
        .iter()
        .zip(DEFAULT_COLUMN_MINIMUMS.iter())
        .map(|(&(col, share), &(_, min))| (col, min.max((f64::from(room) * share) as i32)))
        .collect()
}

/// `header_layout_is_usable(saved_state, saved_columns, current_columns)`: the count must match
/// exactly, and zero (a layout from before the count was recorded) is not an exemption.
pub fn header_layout_is_usable(
    saved_state: &str,
    saved_columns: i64,
    current_columns: Option<i64>,
) -> bool {
    if saved_state.is_empty() {
        return false;
    }
    saved_columns == current_columns.unwrap_or(COLUMNS.len() as i64)
}

/// `MIN_COLUMN_WIDTH` (`gui/main_window.py:135`): below this a header cell shows no title.
pub const MIN_COLUMN_WIDTH: i32 = 24;
/// `DEFAULT_COLUMN_WIDTH` (`gui/main_window.py:136`): what a too-narrow restored column becomes.
pub const DEFAULT_COLUMN_WIDTH: i32 = 90;
/// What starts a `table_header_state` this build wrote (ruling on DAN-2293: Rust writes its own
/// format into the same key; Python's value there is a Qt `saveState()` blob, base64, which is
/// never parsed, not even partially).
pub const HEADER_STATE_PREFIX: &str = "hatate-rs:1:";

/// One column of a saved layout: its width and whether it was hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderSection {
    pub width: i32,
    pub hidden: bool,
}

/// `_save_table_header_state`: the layout as the string stored in `table_header_state`
/// (`hatate-rs:1:` and one `<width>` or `<width>h` per column, in logical column order).
pub fn encode_header_state(sections: &[HeaderSection]) -> String {
    let body: Vec<String> = sections
        .iter()
        .map(|s| format!("{}{}", s.width, if s.hidden { "h" } else { "" }))
        .collect();
    format!("{HEADER_STATE_PREFIX}{}", body.join(","))
}

/// `_restore_table_header_state`'s outcomes, so the window applies the right settings change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderRestore {
    /// Nothing was saved: first run, the default widths apply.
    NothingSaved,
    /// Saved with another column count: `table_header_state` and `table_header_columns` are reset
    /// (Python sets the state to `""` and the count to `0`).
    WrongColumnCount,
    /// The value is not one this build can read (a Qt blob, or damaged): `table_header_state` is
    /// reset to `""`, the column count stays (Python's `restoreState() == False` path).
    Unreadable,
    /// The layout to apply, already repaired; `repaired` is the log line's parts.
    Restored {
        sections: Vec<HeaderSection>,
        repaired: Vec<String>,
    },
}

fn decode_header_sections(state: &str) -> Option<Vec<HeaderSection>> {
    let body = state.strip_prefix(HEADER_STATE_PREFIX)?;
    let sections: Option<Vec<HeaderSection>> = body
        .split(',')
        .map(|tok| {
            let (digits, hidden) = match tok.strip_suffix('h') {
                Some(d) => (d, true),
                None => (tok, false),
            };
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some(HeaderSection {
                width: digits.parse().ok()?,
                hidden,
            })
        })
        .collect();
    sections.filter(|s| s.len() == COLUMNS.len())
}

/// `_repair_header_after_restore`: a hidden column is shown again and one narrower than
/// [`MIN_COLUMN_WIDTH`] gets [`DEFAULT_COLUMN_WIDTH`]. Returns the log line's parts.
pub fn repair_header_sections(sections: &mut [HeaderSection]) -> Vec<String> {
    let mut repaired = Vec::new();
    for (index, section) in sections.iter_mut().enumerate() {
        let name = COLUMNS.get(index).copied().unwrap_or("?");
        if section.hidden {
            section.hidden = false;
            repaired.push(format!("{name} (hidden)"));
        }
        if section.width < MIN_COLUMN_WIDTH {
            section.width = DEFAULT_COLUMN_WIDTH;
            repaired.push(format!("{name} (too narrow to show its title)"));
        }
    }
    repaired
}

/// `_restore_table_header_state(saved_state, saved_columns)`: what to apply and what to reset.
pub fn restore_header_state(saved_state: &str, saved_columns: i64) -> HeaderRestore {
    if saved_state.is_empty() {
        return HeaderRestore::NothingSaved;
    }
    if !header_layout_is_usable(saved_state, saved_columns, None) {
        return HeaderRestore::WrongColumnCount;
    }
    match decode_header_sections(saved_state) {
        None => HeaderRestore::Unreadable,
        Some(mut sections) => {
            let repaired = repair_header_sections(&mut sections);
            HeaderRestore::Restored { sections, repaired }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Candidate, Tag, TagSource};

    fn entry(path: &str) -> Entry {
        Entry::new(path)
    }

    fn with_match(path: &str, w: i64, h: i64) -> Entry {
        let mut e = entry(path);
        e.local_width = Some(100);
        e.local_height = Some(100);
        let mut c = Candidate::new("https://example.test/1");
        c.width = Some(w);
        c.height = Some(h);
        c.engine = "iqdb".into();
        c.similarity = 92.0;
        e.candidates = vec![c];
        e
    }

    #[test]
    fn scale_text_is_pythons() {
        assert_eq!(scale_text(2.5), "2.5");
        assert_eq!(scale_text(0.04), "<0.1");
        assert_eq!(scale_text(0.1), "0.1");
        assert_eq!(scale_text(100.0), "100");
        assert_eq!(scale_text(1200.4), "1,200");
        assert_eq!(scale_text(1234567.5), "1,234,568");
        // round() sends an exact half to the even neighbour.
        assert_eq!(scale_text(102.5), "102");
        assert_eq!(scale_text(103.5), "104");
    }

    #[test]
    fn size_delta_cells_follow_the_tolerance() {
        assert_eq!(
            cell_text(&with_match("a.png", 400, 400), Column::SizeDelta).unwrap(),
            "\u{25b2} 4.0\u{d7}"
        );
        assert_eq!(
            cell_text(&with_match("a.png", 25, 25), Column::SizeDelta).unwrap(),
            "0.2\u{d7}"
        );
        assert_eq!(
            cell_text(&with_match("a.png", 101, 101), Column::SizeDelta).unwrap(),
            "="
        );
        assert_eq!(
            cell_text(&entry("a.png"), Column::SizeDelta).unwrap(),
            EMPTY_CELL
        );
        assert_eq!(
            size_delta_ink(&with_match("a.png", 400, 400)),
            CellInk::Ink100
        );
        assert_eq!(size_delta_ink(&with_match("a.png", 25, 25)), CellInk::Ink65);
        assert_eq!(
            size_delta_ink(&with_match("a.png", 100, 100)),
            CellInk::Ink45
        );
        assert_eq!(size_delta_ink(&entry("a.png")), CellInk::Ink65);
    }

    #[test]
    fn a_missing_file_is_marked_in_the_file_column_and_fades() {
        let mut e = entry("/x/pic.png");
        assert_eq!(cell_text(&e, Column::File).unwrap(), "pic.png");
        e.file_missing = true;
        assert_eq!(cell_text(&e, Column::File).unwrap(), "pic.png  (missing)");
        assert_eq!(foreground(&e, Column::File), Some(CellInk::Ink45));
        assert_eq!(tooltip(&e, Column::File).as_deref(), Some(MISSING_TOOLTIP));
    }

    #[test]
    fn an_interrupted_row_says_so_only_while_unsearched() {
        let mut e = entry("a.png");
        e.interrupted_mid_search = true;
        assert_eq!(cell_text(&e, Column::Engine).unwrap(), INTERRUPTED_MARKER);
        assert!(italic(&e, Column::Engine));
        assert_eq!(foreground(&e, Column::Engine), Some(CellInk::Ink65));
        e.status = MatchStatus::Good;
        assert_eq!(cell_text(&e, Column::Engine).unwrap(), EMPTY_CELL);
        assert!(!italic(&e, Column::Engine));
    }

    #[test]
    fn dashed_columns_take_the_body_tier_and_chips_follow_the_entry() {
        let e = entry("a.png");
        for col in [
            Column::Engine,
            Column::Similarity,
            Column::Booru,
            Column::Cache,
            Column::Sent,
            Column::Reviewed,
        ] {
            assert_eq!(cell_text(&e, col).unwrap(), EMPTY_CELL, "{col:?}");
            assert_eq!(foreground(&e, col), Some(CellInk::Ink65), "{col:?}");
        }
        assert_eq!(
            chip(&e, Column::Status),
            Some(Chip::Status(MatchStatus::NotSearched))
        );
        assert_eq!(chip(&e, Column::Sent), None);
        let mut sent = entry("a.png");
        sent.sent_to_hydrus = true;
        assert_eq!(chip(&sent, Column::Sent).map(Chip::key), Some("queued"));
        sent.hydrus_import_confirmed = true;
        assert_eq!(chip(&sent, Column::Sent).map(Chip::key), Some("sent"));
        let mut est = with_match("a.png", 100, 100);
        est.candidates[0].engine = "ascii2d".into();
        est.candidates[0].similarity_measured = false;
        assert_eq!(cell_text(&est, Column::Similarity).unwrap(), "~92%");
        assert_eq!(chip(&est, Column::Similarity), Some(Chip::Estimated));
    }

    #[test]
    fn cache_labels_and_sort_keys() {
        let mut e = entry("a.png");
        assert_eq!(cache_label(&e), "");
        e.result_source = Some("cached".into());
        assert_eq!(cache_label(&e), "Cached");
        e.searched_without_saucenao = true;
        assert_eq!(cache_label(&e), "Provisional");
        assert_eq!(sort_key(&e, Column::Cache), SortKey::Text("cached".into()));
        assert_eq!(
            sort_key(&entry("B.png"), Column::File),
            SortKey::Text("b.png".into())
        );
        assert_eq!(
            sort_key(&entry("a.png"), Column::Similarity),
            SortKey::Float(-1.0)
        );
        assert_eq!(sort_key(&entry("a.png"), Column::Thumb), SortKey::Int(0));
    }

    /// `lower` sorts before `higher` in `column`, ascending and (reversed) descending.
    fn assert_sorts_before(column: Column, lower: Entry, higher: Entry) {
        assert_eq!(
            sort_key(&lower, column).compare(&sort_key(&higher, column)),
            Ordering::Less,
            "{column:?}: {:?} against {:?}",
            sort_key(&lower, column),
            sort_key(&higher, column)
        );
        let rows = [higher, lower];
        assert_eq!(sorted_order(&rows, column, false), vec![1, 0], "{column:?}");
        assert_eq!(sorted_order(&rows, column, true), vec![0, 1], "{column:?}");
    }

    fn named_tag(name: &str) -> Tag {
        Tag {
            name: name.to_owned(),
            namespace: None,
            source: TagSource::Booru,
        }
    }

    #[test]
    fn every_column_sorts_by_the_value_python_sorts_it_by() {
        // Status: the tier rank, good first and unsearched last.
        let mut good = entry("a.png");
        good.status = MatchStatus::Good;
        let mut error = entry("a.png");
        error.status = MatchStatus::Error;
        assert_eq!(sort_key(&good, Column::Status), SortKey::Int(0));
        assert_sorts_before(Column::Status, good, entry("a.png"));
        assert_sorts_before(Column::Status, error, entry("a.png"));

        // Engine and Booru: the selected match's text, lower-cased; no match is the empty text.
        let mut iqdb = with_match("a.png", 100, 100);
        iqdb.candidates[0].engine = "IQDB".into();
        iqdb.candidates[0].source_name = Some("Gelbooru".into());
        let mut sauce = with_match("a.png", 100, 100);
        sauce.candidates[0].engine = "saucenao".into();
        sauce.candidates[0].source_name = Some("danbooru".into());
        assert_eq!(
            sort_key(&iqdb, Column::Engine),
            SortKey::Text("iqdb".into())
        );
        assert_eq!(
            sort_key(&iqdb, Column::Booru),
            SortKey::Text("gelbooru".into())
        );
        assert_eq!(
            sort_key(&entry("a.png"), Column::Booru),
            SortKey::Text(String::new())
        );
        assert_sorts_before(Column::Engine, iqdb.clone(), sauce.clone());
        assert_sorts_before(Column::Booru, sauce, iqdb);
        assert_sorts_before(
            Column::Engine,
            entry("a.png"),
            with_match("a.png", 100, 100),
        );

        // Size diff.: the scale of the selected match against the local file.
        assert_sorts_before(
            Column::SizeDelta,
            with_match("a.png", 25, 25),
            with_match("a.png", 400, 400),
        );
        assert_eq!(
            sort_key(&entry("a.png"), Column::SizeDelta),
            SortKey::Float(-1.0)
        );
        assert_sorts_before(
            Column::SizeDelta,
            entry("a.png"),
            with_match("a.png", 25, 25),
        );

        // Tags: how many.
        let mut one = entry("a.png");
        one.tags = vec![named_tag("red")];
        let mut two = entry("a.png");
        two.tags = vec![named_tag("red"), named_tag("blue")];
        assert_eq!(sort_key(&two, Column::Tags), SortKey::Int(2));
        assert_sorts_before(Column::Tags, entry("a.png"), one.clone());
        assert_sorts_before(Column::Tags, one, two);

        // Sent: unsent, then queued, then confirmed.
        let mut queued = entry("a.png");
        queued.sent_to_hydrus = true;
        let mut sent = queued.clone();
        sent.hydrus_import_confirmed = true;
        assert_eq!(sort_key(&sent, Column::Sent), SortKey::Int(2));
        assert_sorts_before(Column::Sent, entry("a.png"), queued.clone());
        assert_sorts_before(Column::Sent, queued, sent);

        // Upscale: flagged, then clear, then never checked.
        let mut flagged = entry("a.png");
        flagged.upscale_verdict = Some("flagged".into());
        let mut clear = entry("a.png");
        clear.upscale_verdict = Some("clear".into());
        assert_eq!(sort_key(&entry("a.png"), Column::Upscale), SortKey::Int(2));
        assert_sorts_before(Column::Upscale, flagged, clear.clone());
        assert_sorts_before(Column::Upscale, clear, entry("a.png"));
    }

    #[test]
    fn a_descending_sort_keeps_equal_rows_in_their_order() {
        let mut a = entry("a.png");
        a.reviewed = true;
        let b = entry("b.png");
        let c = entry("c.png");
        let rows = [a, b, c];
        assert_eq!(sorted_order(&rows, Column::Reviewed, false), vec![1, 2, 0]);
        assert_eq!(sorted_order(&rows, Column::Reviewed, true), vec![0, 1, 2]);
    }

    #[test]
    fn column_widths_and_the_saved_layout_check() {
        let w = default_column_widths(1360, 40);
        assert_eq!(w[0], (Column::File, 237)); // int(1320 * 0.18)
        assert_eq!(w[6], (Column::Tags, 66));
        assert_eq!(default_column_widths(0, 40)[0], (Column::File, 140));
        assert!(!header_layout_is_usable("", 12, None));
        assert!(header_layout_is_usable("state", 12, None));
        assert!(!header_layout_is_usable("state", 0, None));
        assert!(!header_layout_is_usable("state", 10, None));
        assert!(header_layout_is_usable("state", 9, Some(9)));
        assert_eq!(COLUMNS.len(), Column::ALL.len());
    }

    fn sections(width: i32) -> Vec<HeaderSection> {
        vec![
            HeaderSection {
                width,
                hidden: false
            };
            COLUMNS.len()
        ]
    }

    #[test]
    fn a_header_state_round_trips_and_is_repaired() {
        let mut s = sections(120);
        s[2].hidden = true;
        s[5].width = 10;
        let text = encode_header_state(&s);
        assert!(text.starts_with(HEADER_STATE_PREFIX));
        match restore_header_state(&text, 12) {
            HeaderRestore::Restored { sections, repaired } => {
                assert!(!sections[2].hidden);
                assert_eq!(sections[5].width, DEFAULT_COLUMN_WIDTH);
                assert_eq!(sections[0].width, 120);
                assert_eq!(
                    repaired,
                    vec![
                        format!("{} (hidden)", COLUMNS[2]),
                        format!("{} (too narrow to show its title)", COLUMNS[5]),
                    ]
                );
            }
            other => panic!("expected a restored layout, got {other:?}"),
        }
        let clean = encode_header_state(&sections(MIN_COLUMN_WIDTH));
        assert!(matches!(
            restore_header_state(&clean, 12),
            HeaderRestore::Restored { ref repaired, .. } if repaired.is_empty()
        ));
    }

    #[test]
    fn a_header_state_this_build_cannot_read_is_never_partly_parsed() {
        assert_eq!(restore_header_state("", 0), HeaderRestore::NothingSaved);
        let good = encode_header_state(&sections(100));
        assert_eq!(
            restore_header_state(&good, 11),
            HeaderRestore::WrongColumnCount
        );
        assert_eq!(
            restore_header_state(&good, 0),
            HeaderRestore::WrongColumnCount
        );
        // A Qt saveState() blob (base64) with the right column count.
        for bad in [
            "AAAA/wAAAAAAAAABAAAAAQEAAAAAAAAAAAAAAAAAAAAAAADIAAA=",
            "hatate-rs:2:1,2,3,4,5,6,7,8,9,10,11,12",
            "hatate-rs:1:1,2,3",
            "hatate-rs:1:100,100,100,100,100,100,100,100,100,100,100,x",
            "hatate-rs:1:100,100,100,100,100,100,100,100,100,100,100,100,100",
            "hatate-rs:1:100,100,100,100,100,100,100,100,100,100,100,-5",
            "hatate-rs:1:100,100,100,100,100,100,100,100,100,100,100,",
        ] {
            assert_eq!(
                restore_header_state(bad, 12),
                HeaderRestore::Unreadable,
                "{bad}"
            );
        }
    }
}
