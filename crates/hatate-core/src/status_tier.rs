//! The chips of the queue table and the order their columns sort in (`phase6-calls` call 1,
//! DAN-2213; Python at `76c85c4`: `core/models.py:47-58`, `gui/theme.py:155-231`,
//! `gui/image_table_model.py:111-131, 237-244, 413-419, 644-654`).
//!
//! The Status column has exactly six tiers, one per [`MatchStatus`], and the tier is a function of
//! `status` alone. The upscale verdict and the reviewed flag do not enter it: Flagged is a cell of
//! the Upscale column ([`UpscaleTier`]), Reviewed a cell of the Reviewed column. The three columns
//! are independent, so a Good row can be Flagged and Reviewed at once. There is no `Flagged`
//! tier, and the rank is Python's (Good first), not the order the plan's table first gave.
//!
//! Nothing here is stored. `MatchStatus` and the entry fields are unchanged on disk.

use crate::model::{Entry, MatchStatus};

/// Which font a chip's glyph must be drawn in. Only `◐` and `○` need the bundled subset; the
/// others stay on the font the caller already uses (`STATUS_GLYPH_FONT`, `_NEEDS_GLYPH_FONT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphFont {
    /// Whatever font the caller already has (JetBrains Mono).
    Mono,
    /// The bundled `Ryoku Status` face.
    RyokuStatus,
}

/// The ink tier a chip's glyph and word draw in (`STATUS_WEIGHTS`). Python also has `ink_45`,
/// for a key it does not know; no tier here is that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Routine or done.
    Ink65,
    /// Needs a decision.
    Ink100,
}

impl Ink {
    /// The token name in `gui/theme.py`.
    pub fn token(self) -> &'static str {
        match self {
            Ink::Ink65 => "ink_65",
            Ink::Ink100 => "ink_100",
        }
    }
}

/// The six tiers of the Status column. The discriminant is the rank: Ascending puts Good first and
/// Unsearched last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum StatusTier {
    /// `MatchStatus::Good`.
    Good = 0,
    /// `MatchStatus::Poor`, which Python's chip calls Review.
    Review = 1,
    /// `MatchStatus::NotFound`.
    NotFound = 2,
    /// `MatchStatus::Error`.
    Error = 3,
    /// `MatchStatus::Searching`.
    Searching = 4,
    /// `MatchStatus::NotSearched`.
    Unsearched = 5,
}

impl StatusTier {
    /// In rank order.
    pub const ALL: [StatusTier; 6] = [
        StatusTier::Good,
        StatusTier::Review,
        StatusTier::NotFound,
        StatusTier::Error,
        StatusTier::Searching,
        StatusTier::Unsearched,
    ];

    /// The tier of a status.
    pub fn of_status(status: MatchStatus) -> StatusTier {
        match status {
            MatchStatus::Good => StatusTier::Good,
            MatchStatus::Poor => StatusTier::Review,
            MatchStatus::NotFound => StatusTier::NotFound,
            MatchStatus::Error => StatusTier::Error,
            MatchStatus::Searching => StatusTier::Searching,
            MatchStatus::NotSearched => StatusTier::Unsearched,
        }
    }

    /// The tier of a row: its status, and nothing else.
    pub fn of(entry: &Entry) -> StatusTier {
        StatusTier::of_status(entry.status)
    }

    /// `MatchStatus.sort_rank`: 0 to 5. A sort by Status must be stable (`list.sort` is), in both
    /// directions, so rows of one tier keep their previous order.
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// The status this tier is the chip of.
    pub fn status(self) -> MatchStatus {
        match self {
            StatusTier::Good => MatchStatus::Good,
            StatusTier::Review => MatchStatus::Poor,
            StatusTier::NotFound => MatchStatus::NotFound,
            StatusTier::Error => MatchStatus::Error,
            StatusTier::Searching => MatchStatus::Searching,
            StatusTier::Unsearched => MatchStatus::NotSearched,
        }
    }

    /// `CHIP_LABELS`: what the chip says.
    pub fn chip_text(self) -> &'static str {
        match self {
            StatusTier::Good => "Good",
            StatusTier::Review => "Review",
            StatusTier::NotFound => "Not found",
            StatusTier::Error => "Error",
            StatusTier::Searching => "Searching",
            StatusTier::Unsearched => "Unsearched",
        }
    }

    /// `STATUS_GLYPHS`.
    pub fn glyph(self) -> char {
        match self {
            StatusTier::Good => '\u{25cf}',
            StatusTier::Review | StatusTier::Searching => '\u{25d0}',
            StatusTier::NotFound => '\u{25cb}',
            StatusTier::Error => '\u{d7}',
            StatusTier::Unsearched => '\u{b7}',
        }
    }

    /// `status_glyph_font`.
    pub fn glyph_font(self) -> GlyphFont {
        match self {
            StatusTier::Review | StatusTier::NotFound | StatusTier::Searching => {
                GlyphFont::RyokuStatus
            }
            _ => GlyphFont::Mono,
        }
    }

    /// `STATUS_WEIGHTS`.
    pub fn ink(self) -> Ink {
        match self {
            StatusTier::Review | StatusTier::NotFound | StatusTier::Error => Ink::Ink100,
            _ => Ink::Ink65,
        }
    }

    /// Whether the chip is bold. Python bolds only Error (`BOXED_CHIPS`); Review and Not found are
    /// bold here as the design call decides (`phase6-calls` 1d: bold is a third non-hue channel
    /// for the tiers the user hunts for). Mono advance is the same in bold, so no column moves.
    pub fn bold(self) -> bool {
        matches!(
            self,
            StatusTier::Review | StatusTier::NotFound | StatusTier::Error
        )
    }

    /// Only Searching is italic.
    pub fn italic(self) -> bool {
        self == StatusTier::Searching
    }

    /// Only Error carries a hairline box (`BOXED_CHIPS`), in the `ink_46` border tier.
    pub fn boxed(self) -> bool {
        self == StatusTier::Error
    }

    /// The filter menu's and the tooltip's wording (`MatchStatus.label`).
    pub fn label(self) -> &'static str {
        self.status().label()
    }
}

/// The Upscale column: `upscale_verdict`, three states. `None`, `""` and any string that is not
/// `flagged` or `clear` are `NotChecked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum UpscaleTier {
    /// A bigger copy is likely out there. Draws `▲ Flagged`, bold, `ink_100`.
    Flagged = 0,
    /// Checked, nothing found. Draws `● Clear`, `ink_65`.
    Clear = 1,
    /// Never checked, or an unknown verdict.
    NotChecked = 2,
}

impl UpscaleTier {
    pub fn of_verdict(verdict: Option<&str>) -> UpscaleTier {
        match verdict {
            Some("flagged") => UpscaleTier::Flagged,
            Some("clear") => UpscaleTier::Clear,
            _ => UpscaleTier::NotChecked,
        }
    }

    pub fn of(entry: &Entry) -> UpscaleTier {
        UpscaleTier::of_verdict(entry.upscale_verdict.as_deref())
    }

    /// The sort key of the Upscale column: flagged first, then clear, then never checked.
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// `_upscale_glyph`: none for a verdict that was not run.
    pub fn glyph(self) -> Option<char> {
        match self {
            UpscaleTier::Flagged => Some('\u{25b2}'),
            UpscaleTier::Clear => Some('\u{25cf}'),
            UpscaleTier::NotChecked => None,
        }
    }

    /// `_upscale_weight`.
    pub fn ink(self) -> Ink {
        match self {
            UpscaleTier::Flagged => Ink::Ink100,
            _ => Ink::Ink65,
        }
    }

    /// The word after the glyph (`upscale_verdict_label`).
    pub fn label(self) -> &'static str {
        match self {
            UpscaleTier::Flagged => "Flagged",
            UpscaleTier::Clear => "Clear",
            UpscaleTier::NotChecked => crate::entry_filter::UPSCALE_NOT_CHECKED,
        }
    }

    /// `COL_UPSCALE`'s cell text (`data`): the dash for an empty verdict, `"{glyph} {label}"` for
    /// one with a glyph, the label alone for an unknown string.
    pub fn cell_text(entry: &Entry) -> String {
        if entry.upscale_verdict.as_deref().is_none_or(str::is_empty) {
            return EMPTY_CELL.to_owned();
        }
        let tier = UpscaleTier::of(entry);
        match tier.glyph() {
            Some(glyph) => format!("{glyph} {}", tier.label()),
            None => tier.label().to_owned(),
        }
    }
}

/// `EMPTY_CELL` (`gui/image_table_model.py:52`): a cell with nothing to say.
pub const EMPTY_CELL: &str = "\u{2014}";

/// The Sent column: unsent, handed to Hydrus's downloader, confirmed held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum SentTier {
    NotSent = 0,
    /// Sent, nothing has confirmed it landed.
    Queued = 1,
    /// Sent and Hydrus acknowledged holding the file.
    Sent = 2,
}

impl SentTier {
    pub fn of(entry: &Entry) -> SentTier {
        if !entry.sent_to_hydrus {
            SentTier::NotSent
        } else if entry.hydrus_import_confirmed {
            SentTier::Sent
        } else {
            SentTier::Queued
        }
    }

    /// The sort key of the Sent column: unsent first, then queued, then confirmed.
    pub fn rank(self) -> u8 {
        self as u8
    }

    /// `_entry_sent_label`, with the dash for unsent as the cell shows it.
    pub fn cell_text(self) -> &'static str {
        match self {
            SentTier::NotSent => EMPTY_CELL,
            SentTier::Queued => "Queued",
            SentTier::Sent => "Sent",
        }
    }

    /// `STATUS_GLYPHS['queued']` and `['sent']`; none for unsent.
    pub fn glyph(self) -> Option<char> {
        match self {
            SentTier::NotSent => None,
            SentTier::Queued => Some('\u{25d0}'),
            SentTier::Sent => Some('\u{25cf}'),
        }
    }

    /// `status_glyph_font`: `◐` needs the bundled face.
    pub fn glyph_font(self) -> GlyphFont {
        match self {
            SentTier::Queued => GlyphFont::RyokuStatus,
            _ => GlyphFont::Mono,
        }
    }
}

/// The Reviewed column's sort key: rows still wanting a decision first.
pub fn reviewed_rank(entry: &Entry) -> u8 {
    u8::from(entry.reviewed)
}

/// The Reviewed column's cell: the mark alone, or the dash.
pub fn reviewed_cell_text(entry: &Entry) -> &'static str {
    if entry.reviewed {
        "\u{25cf}"
    } else {
        EMPTY_CELL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: MatchStatus, verdict: Option<&str>, reviewed: bool) -> Entry {
        let mut e = Entry::new("/pictures/a.png");
        e.status = status;
        e.upscale_verdict = verdict.map(str::to_owned);
        e.reviewed = reviewed;
        e
    }

    #[test]
    fn the_rank_is_pythons_good_first_unsearched_last() {
        // `MatchStatus.sort_rank`, `core/models.py:47-58`; call 1a. Not the plan's
        // `Error < Flagged < Not found < Review < Good < Unsearched`.
        let ranks: Vec<(MatchStatus, u8)> = [
            MatchStatus::Good,
            MatchStatus::Poor,
            MatchStatus::NotFound,
            MatchStatus::Error,
            MatchStatus::Searching,
            MatchStatus::NotSearched,
        ]
        .into_iter()
        .map(|s| (s, StatusTier::of_status(s).rank()))
        .collect();
        assert_eq!(
            ranks.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(StatusTier::Good as u8, 0);
        assert_eq!(StatusTier::Unsearched as u8, 5);
    }

    #[test]
    fn the_tier_depends_on_status_only_for_all_36_cells_and_an_unknown_verdict() {
        let verdicts = [None, Some("flagged"), Some("clear")];
        let mut cells = 0;
        for status in MatchStatus::ALL {
            let expected = StatusTier::of_status(status);
            for verdict in verdicts {
                for reviewed in [false, true] {
                    let e = entry(status, verdict, reviewed);
                    assert_eq!(StatusTier::of(&e), expected, "{status:?} {verdict:?} {reviewed}");
                    cells += 1;
                }
            }
            for odd in [Some(""), Some("FLAGGED"), Some("maybe")] {
                assert_eq!(StatusTier::of(&entry(status, odd, true)), expected);
            }
        }
        assert_eq!(cells, 36);
    }

    #[test]
    fn each_tier_is_one_status_and_round_trips() {
        for (i, tier) in StatusTier::ALL.into_iter().enumerate() {
            assert_eq!(tier.rank() as usize, i);
            assert_eq!(StatusTier::of_status(tier.status()), tier);
        }
        assert_eq!(StatusTier::Review.status(), MatchStatus::Poor);
    }

    #[test]
    fn the_chip_of_each_tier_matches_the_table_of_call_1a() {
        use GlyphFont::{Mono, RyokuStatus};
        use Ink::{Ink100, Ink65};
        // tier, chip, glyph, font, ink, bold, italic, boxed, label
        let table = [
            (StatusTier::Good, "Good", '\u{25cf}', Mono, Ink65, false, false, false, "Found (good)"),
            (StatusTier::Review, "Review", '\u{25d0}', RyokuStatus, Ink100, true, false, false, "Found (review)"),
            (StatusTier::NotFound, "Not found", '\u{25cb}', RyokuStatus, Ink100, true, false, false, "Not found"),
            (StatusTier::Error, "Error", '\u{d7}', Mono, Ink100, true, false, true, "Error"),
            (StatusTier::Searching, "Searching", '\u{25d0}', RyokuStatus, Ink65, false, true, false, "Searching..."),
            (StatusTier::Unsearched, "Unsearched", '\u{b7}', Mono, Ink65, false, false, false, "Not searched"),
        ];
        for (tier, chip, glyph, font, ink, bold, italic, boxed, label) in table {
            assert_eq!(tier.chip_text(), chip);
            assert_eq!(tier.glyph(), glyph, "{tier:?}");
            assert_eq!(tier.glyph_font(), font, "{tier:?}");
            assert_eq!(tier.ink(), ink, "{tier:?}");
            assert_eq!(tier.bold(), bold, "{tier:?}");
            assert_eq!(tier.italic(), italic, "{tier:?}");
            assert_eq!(tier.boxed(), boxed, "{tier:?}");
            assert_eq!(tier.label(), label);
        }
        assert_eq!(Ink::Ink65.token(), "ink_65");
        assert_eq!(Ink::Ink100.token(), "ink_100");
    }

    #[test]
    fn the_upscale_column_ranks_flagged_clear_not_checked() {
        for (verdict, tier, rank) in [
            (Some("flagged"), UpscaleTier::Flagged, 0),
            (Some("clear"), UpscaleTier::Clear, 1),
            (None, UpscaleTier::NotChecked, 2),
            (Some(""), UpscaleTier::NotChecked, 2),
            (Some("other"), UpscaleTier::NotChecked, 2),
        ] {
            assert_eq!(UpscaleTier::of_verdict(verdict), tier);
            assert_eq!(tier.rank(), rank);
        }
        let mut e = entry(MatchStatus::Good, Some("flagged"), false);
        assert_eq!(UpscaleTier::cell_text(&e), "\u{25b2} Flagged");
        e.upscale_verdict = Some("clear".into());
        assert_eq!(UpscaleTier::cell_text(&e), "\u{25cf} Clear");
        e.upscale_verdict = None;
        assert_eq!(UpscaleTier::cell_text(&e), EMPTY_CELL);
        e.upscale_verdict = Some("other".into());
        assert_eq!(UpscaleTier::cell_text(&e), "Not checked");
        assert_eq!(UpscaleTier::Flagged.ink(), Ink::Ink100);
        assert_eq!(UpscaleTier::Clear.ink(), Ink::Ink65);
    }

    #[test]
    fn the_sent_and_reviewed_columns_rank_as_python_sorts_them() {
        let mut e = entry(MatchStatus::Good, None, false);
        assert_eq!(SentTier::of(&e).rank(), 0);
        assert_eq!(SentTier::of(&e).cell_text(), EMPTY_CELL);
        e.sent_to_hydrus = true;
        assert_eq!(SentTier::of(&e), SentTier::Queued);
        assert_eq!(SentTier::Queued.cell_text(), "Queued");
        assert_eq!(SentTier::Queued.glyph_font(), GlyphFont::RyokuStatus);
        e.hydrus_import_confirmed = true;
        assert_eq!(SentTier::of(&e), SentTier::Sent);
        assert_eq!(SentTier::of(&e).rank(), 2);
        assert_eq!(SentTier::Sent.glyph(), Some('\u{25cf}'));
        // A confirmation on a row that was never sent changes nothing.
        let mut unsent = entry(MatchStatus::Good, None, false);
        unsent.hydrus_import_confirmed = true;
        assert_eq!(SentTier::of(&unsent), SentTier::NotSent);
        assert_eq!(reviewed_rank(&entry(MatchStatus::Good, None, false)), 0);
        assert_eq!(reviewed_rank(&entry(MatchStatus::Good, None, true)), 1);
        assert_eq!(reviewed_cell_text(&entry(MatchStatus::Good, None, true)), "\u{25cf}");
        assert_eq!(reviewed_cell_text(&entry(MatchStatus::Good, None, false)), EMPTY_CELL);
    }
}
