//! What the preview panel says (`gui/preview_text.py`): every function turns an entry or a
//! candidate into a piece of text (or a colour tier) and touches no widget.
//!
//! The placeholder wording is worth pinning: "No match found" and "Not searched yet" mean opposite
//! things to someone deciding whether to re-run a search.
//!
//! The Python module's `image_opener` (PIL's `Image.open`) is the [`ImageOpener`] trait. Two
//! things the module reads from `gui.theme` are not here: `comparison_banner_colour` (the palette
//! lookup, with the theme) and the glyph table behind [`comparison_readout`], which takes the
//! lookup as an argument.

use std::fs;

use crate::image_compare::{compare_file_sizes, compare_sizes};
use crate::model::{Candidate, Entry, MatchStatus};
use crate::similarity_display::similarity_label;

/// `NOT_SEARCHED_TEXT`.
pub const NOT_SEARCHED_TEXT: &str = "Not searched yet";

/// `BANNER_GOOD`: ink tier for a confirmed upgrade.
pub const BANNER_GOOD: &str = "ink_65";
/// `BANNER_UNCERTAIN`: ink tier for "worth a second look".
pub const BANNER_UNCERTAIN: &str = "ink_100";
/// `BANNER_NEUTRAL`: ink tier for "nothing to compare".
pub const BANNER_NEUTRAL: &str = "ink_45";

/// What PIL's `Image.open(path)` gives these functions: a size and, if it knows, a format name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: i64,
    pub height: i64,
    /// `im.format`: `"PNG"`, `"JPEG"`, ... or `None` / empty when unknown.
    pub format: Option<String>,
}

/// `image_opener`: reads a picture's header. `None` is Python's `OSError` or `ValueError`.
pub trait ImageOpener {
    fn open(&self, path: &str) -> Option<ImageInfo>;
}

impl<F: Fn(&str) -> Option<ImageInfo>> ImageOpener for F {
    fn open(&self, path: &str) -> Option<ImageInfo> {
        self(path)
    }
}

/// `human_size`: `512 B`, `1.5 KB`, `2.0 MB`, `3.1 GB`; `None` for no size. Never past GB.
pub fn human_size(num_bytes: Option<i64>) -> Option<String> {
    let mut size = num_bytes? as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if size < 1024.0 || unit == "GB" {
            return Some(if unit == "B" {
                format!("{size:.0} {unit}")
            } else {
                format!("{size:.1} {unit}")
            });
        }
        size /= 1024.0;
    }
    unreachable!("the GB arm always returns")
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.is_empty())
}

/// `matched_caption`: `Matched image: Danbooru (95% similar)`, with the site and the score
/// omitted when absent; an unmeasured score says so in words.
pub fn matched_caption(entry: &Entry) -> String {
    let mut caption = "Matched image:".to_owned();
    if let Some(name) = non_empty(entry.booru_name()) {
        caption.push(' ');
        caption.push_str(name);
    }
    if let Some(similarity) = entry.similarity() {
        let measured = entry.similarity_measured();
        let score = similarity_label(Some(similarity), measured);
        let qualifier = if measured {
            ""
        } else {
            " - ranking, not measured"
        };
        caption.push_str(&format!(" ({score} similar{qualifier})"));
    }
    caption
}

/// `no_candidate_text`: why this row shows no match.
pub fn no_candidate_text(entry: &Entry) -> String {
    let message = non_empty(entry.error_message.as_deref());
    match (entry.status, message) {
        (MatchStatus::Error, _) => format!("Search error: {}", message.unwrap_or("")),
        (MatchStatus::NotFound, Some(m)) => {
            format!("No match found (not every engine could search: {m})")
        }
        (MatchStatus::Searching, _) => "Searching\u{2026}".to_owned(),
        (MatchStatus::NotFound, None) => "No match found".to_owned(),
        _ => NOT_SEARCHED_TEXT.to_owned(),
    }
}

fn dimensions(width: i64, height: i64, spaced: bool) -> String {
    if spaced {
        format!("{width} \u{d7} {height}")
    } else {
        format!("{width}\u{d7}{height}")
    }
}

fn local_info_parts(path: &str, opener: &dyn ImageOpener, spaced: bool) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(info) = opener.open(path) {
        parts.push(dimensions(info.width, info.height, spaced));
        if let Some(format) = non_empty(info.format.as_deref()) {
            parts.push(format.to_owned());
        }
    }
    if let Ok(meta) = fs::metadata(path) {
        if let Some(size) = human_size(i64::try_from(meta.len()).ok()) {
            parts.push(size);
        }
    }
    parts
}

/// `local_image_info_text`: dimensions, file size and format of the local file, all read from
/// disk.
pub fn local_image_info_text(path: &str, image_opener: &dyn ImageOpener) -> String {
    let parts = local_info_parts(path, image_opener, false);
    if parts.is_empty() {
        "Could not read image info".to_owned()
    } else {
        parts.join(" \u{2022} ")
    }
}

/// `local_frame_meta`: the same facts in the caption under the picture's frame
/// (`560 × 900 · PNG · 10.1 KB`), and nothing at all when the file cannot be read.
pub fn local_frame_meta(path: &str, image_opener: &dyn ImageOpener) -> String {
    local_info_parts(path, image_opener, true).join(" \u{b7} ")
}

/// `matched_frame_label`: `Match — Danbooru`, or `Match — Yande.re · ~77% · ranking, not
/// measured` when the score is only a rank. A measured score is the header's readout, not
/// repeated here.
pub fn matched_frame_label(entry: &Entry) -> String {
    let mut label = "Match".to_owned();
    if let Some(name) = non_empty(entry.booru_name()) {
        label.push_str(&format!(" \u{2014} {name}"));
    }
    if let Some(similarity) = entry.similarity() {
        if !entry.similarity_measured() {
            label.push_str(&format!(
                " \u{b7} {} \u{b7} ranking, not measured",
                similarity_label(Some(similarity), false)
            ));
        }
    }
    label
}

fn candidate_parts(candidate: &Candidate, spaced: bool) -> Vec<String> {
    let mut parts = Vec::new();
    if let (Some(w), Some(h)) = (candidate.width, candidate.height) {
        if w != 0 && h != 0 {
            parts.push(dimensions(w, h, spaced));
        }
    }
    if let Some(format) = non_empty(candidate.remote_format.as_deref()) {
        parts.push(format.to_owned());
    }
    if let Some(size) = human_size(candidate.remote_size_bytes) {
        parts.push(size);
    }
    parts
}

/// `matched_frame_meta`: the match's dimensions, format and size for its caption. Empty when
/// there is no candidate (the caller shows a dash then).
pub fn matched_frame_meta(candidate: Option<&Candidate>) -> String {
    match candidate {
        None => String::new(),
        Some(c) => candidate_parts(c, true).join(" \u{b7} "),
    }
}

/// `matched_image_info_text`: dimensions, format and size of the selected match, plus a line for
/// borrowed tags and one for why the match has no tags. A piece that is not available is omitted.
pub fn matched_image_info_text(candidate: Option<&Candidate>) -> String {
    let Some(candidate) = candidate else {
        return String::new();
    };
    let parts = candidate_parts(candidate, false);
    let mut info = if parts.is_empty() {
        "Image info not available".to_owned()
    } else {
        parts.join(" \u{2022} ")
    };
    if let Some(lender) = non_empty(candidate.tags_borrowed_from.as_deref()) {
        info.push_str(&format!(
            "\ntags from another match of the same image: {lender}"
        ));
    }
    if let Some(reason) = non_empty(candidate.incomplete_reason.as_deref()) {
        return format!("{info}\n{reason}");
    }
    info
}

fn compare_entry_with(
    entry: &Entry,
    candidate: &Candidate,
) -> crate::image_compare::SizeComparison {
    compare_sizes(
        Some(entry.local_width.unwrap_or(0)),
        Some(entry.local_height.unwrap_or(0)),
        Some(candidate.width.unwrap_or(0)),
        Some(candidate.height.unwrap_or(0)),
    )
}

/// `comparison_banner_text`: the one line that answers "is this match better than what I have?".
pub fn comparison_banner_text(entry: &Entry, candidate: Option<&Candidate>) -> String {
    let Some(candidate) = candidate else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(similarity) = entry.similarity() {
        let measured = entry.similarity_measured();
        let score = similarity_label(Some(similarity), measured);
        parts.push(if measured {
            format!("{score} similar")
        } else {
            format!("{score} similar (ranking)")
        });
    }
    let comparison = compare_entry_with(entry, candidate);
    if !comparison.verdict.is_empty() {
        parts.push(format!("match is {}", comparison.verdict));
    }
    let local_bytes = fs::metadata(&entry.path)
        .ok()
        .and_then(|m| i64::try_from(m.len()).ok());
    let file_text = compare_file_sizes(local_bytes, candidate.remote_size_bytes);
    if !file_text.is_empty() {
        parts.push(file_text);
    }
    parts.join("  \u{b7}  ")
}

/// `comparison_banner_tier`: `ink_65` when the match is worth taking, `ink_100` when it is not
/// clearly an upgrade, `ink_45` with no candidate to compare against.
pub fn comparison_banner_tier(entry: &Entry, candidate: Option<&Candidate>) -> &'static str {
    let Some(candidate) = candidate else {
        return BANNER_NEUTRAL;
    };
    let comparison = compare_entry_with(entry, candidate);
    if comparison.aspect_differs {
        return BANNER_UNCERTAIN;
    }
    if comparison.remote_is_bigger() {
        BANNER_GOOD
    } else {
        BANNER_UNCERTAIN
    }
}

/// `ComparisonReadout`: the compare header's readout as parts the widget can style apart
/// (`● 96% SIMILARITY ▲ 2.5× larger`). Every part is `""` when it has nothing to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonReadout {
    pub glyph: String,
    pub value: String,
    pub label: String,
    pub diff: String,
    pub tier: &'static str,
    pub tooltip: String,
}

impl Default for ComparisonReadout {
    fn default() -> Self {
        ComparisonReadout {
            glyph: String::new(),
            value: String::new(),
            label: String::new(),
            diff: String::new(),
            tier: BANNER_NEUTRAL,
            tooltip: String::new(),
        }
    }
}

/// `comparison_readout`. `status_glyph` is `theme.status_glyph`: the glyph of a chip key
/// (`"good"`, `"estimated"`), or `None` for a key the table does not draw.
pub fn comparison_readout(
    entry: &Entry,
    candidate: Option<&Candidate>,
    status_glyph: &dyn Fn(&str) -> Option<String>,
) -> ComparisonReadout {
    let Some(cand) = candidate else {
        return ComparisonReadout::default();
    };
    let (mut glyph, mut value, mut label) = (String::new(), String::new(), String::new());
    if let Some(similarity) = entry.similarity() {
        let measured = entry.similarity_measured();
        value = similarity_label(Some(similarity), measured);
        label = if measured { "Similarity" } else { "Ranking" }.to_owned();
        glyph = status_glyph(if measured { "good" } else { "estimated" }).unwrap_or_default();
    }
    let comparison = compare_entry_with(entry, cand);
    let mut diff = comparison.verdict.clone();
    if !diff.is_empty() && comparison.remote_is_bigger() {
        diff = format!("\u{25b2} {diff}");
    }
    ComparisonReadout {
        glyph,
        value,
        label,
        diff,
        tier: comparison_banner_tier(entry, candidate),
        tooltip: comparison_banner_text(entry, candidate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Candidate;

    fn entry(status: MatchStatus) -> Entry {
        let mut e = Entry::new("/nonexistent/pic.png");
        e.status = status;
        e
    }

    fn matched(similarity: f64, measured: bool, w: i64, h: i64) -> (Entry, Candidate) {
        let mut e = entry(MatchStatus::Good);
        e.local_width = Some(1000);
        e.local_height = Some(1000);
        let mut c = Candidate::new("https://danbooru.donmai.us/posts/1");
        c.source_name = Some("Danbooru".to_owned());
        c.similarity = similarity;
        c.similarity_measured = measured;
        if !measured {
            // An engine that reports a real score would make the match measured regardless.
            c.engine = "ascii2d".to_owned();
        }
        c.width = Some(w);
        c.height = Some(h);
        e.candidates.push(c.clone());
        (e, c)
    }

    #[test]
    fn sizes_are_human_sized_and_never_past_gb() {
        assert_eq!(human_size(None), None);
        assert_eq!(human_size(Some(0)).as_deref(), Some("0 B"));
        assert_eq!(human_size(Some(1023)).as_deref(), Some("1023 B"));
        assert_eq!(human_size(Some(1024)).as_deref(), Some("1.0 KB"));
        assert_eq!(human_size(Some(10_340)).as_deref(), Some("10.1 KB"));
        assert_eq!(human_size(Some(5 * 1024 * 1024)).as_deref(), Some("5.0 MB"));
        assert_eq!(human_size(Some(3 << 40)).as_deref(), Some("3072.0 GB"));
    }

    #[test]
    fn the_placeholder_says_why_the_row_has_no_match() {
        let mut e = entry(MatchStatus::NotFound);
        assert_eq!(no_candidate_text(&e), "No match found");
        e.error_message = Some("ascii2d blocked".to_owned());
        assert_eq!(
            no_candidate_text(&e),
            "No match found (not every engine could search: ascii2d blocked)"
        );
        e.status = MatchStatus::Error;
        assert_eq!(no_candidate_text(&e), "Search error: ascii2d blocked");
        e.error_message = None;
        assert_eq!(no_candidate_text(&e), "Search error: ");
        assert_eq!(
            no_candidate_text(&entry(MatchStatus::Searching)),
            "Searching\u{2026}"
        );
        assert_eq!(
            no_candidate_text(&entry(MatchStatus::NotSearched)),
            "Not searched yet"
        );
        assert_eq!(
            no_candidate_text(&entry(MatchStatus::Good)),
            "Not searched yet"
        );
    }

    #[test]
    fn the_caption_says_a_ranking_is_not_a_measurement() {
        let (e, _) = matched(95.0, true, 10, 10);
        assert_eq!(matched_caption(&e), "Matched image: Danbooru (95% similar)");
        let (e, _) = matched(76.0, false, 10, 10);
        assert_eq!(
            matched_caption(&e),
            "Matched image: Danbooru (~76% similar - ranking, not measured)"
        );
        assert_eq!(
            matched_caption(&entry(MatchStatus::NotFound)),
            "Matched image:"
        );
    }

    #[test]
    fn the_frame_label_repeats_only_an_unmeasured_score() {
        let (e, _) = matched(95.0, true, 10, 10);
        assert_eq!(matched_frame_label(&e), "Match \u{2014} Danbooru");
        let (e, _) = matched(77.0, false, 10, 10);
        assert_eq!(
            matched_frame_label(&e),
            "Match \u{2014} Danbooru \u{b7} ~77% \u{b7} ranking, not measured"
        );
        assert_eq!(matched_frame_label(&entry(MatchStatus::NotFound)), "Match");
    }

    #[test]
    fn the_match_info_names_borrowed_tags_and_an_incomplete_reason() {
        assert_eq!(matched_image_info_text(None), "");
        let (_, mut c) = matched(95.0, true, 1600, 1200);
        assert_eq!(matched_image_info_text(Some(&c)), "1600\u{d7}1200");
        c.remote_format = Some("JPEG".to_owned());
        c.remote_size_bytes = Some(2048);
        assert_eq!(
            matched_image_info_text(Some(&c)),
            "1600\u{d7}1200 \u{2022} JPEG \u{2022} 2.0 KB"
        );
        assert_eq!(
            matched_frame_meta(Some(&c)),
            "1600 \u{d7} 1200 \u{b7} JPEG \u{b7} 2.0 KB"
        );
        c.tags_borrowed_from = Some("Gelbooru".to_owned());
        c.incomplete_reason = Some("tags need a login".to_owned());
        assert_eq!(
            matched_image_info_text(Some(&c)),
            "1600\u{d7}1200 \u{2022} JPEG \u{2022} 2.0 KB\ntags from another match of the same image: Gelbooru\ntags need a login"
        );
        let bare = Candidate::new("https://x/");
        assert_eq!(
            matched_image_info_text(Some(&bare)),
            "Image info not available"
        );
        assert_eq!(matched_frame_meta(Some(&bare)), "");
        assert_eq!(matched_frame_meta(None), "");
    }

    #[test]
    fn the_local_info_reads_the_header_and_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.png");
        std::fs::write(&file, vec![0u8; 10_340]).unwrap();
        let path = file.to_str().unwrap();
        let png = |_: &str| {
            Some(ImageInfo {
                width: 560,
                height: 900,
                format: Some("PNG".to_owned()),
            })
        };
        assert_eq!(
            local_image_info_text(path, &png),
            "560\u{d7}900 \u{2022} PNG \u{2022} 10.1 KB"
        );
        assert_eq!(
            local_frame_meta(path, &png),
            "560 \u{d7} 900 \u{b7} PNG \u{b7} 10.1 KB"
        );
        let unreadable = |_: &str| None;
        assert_eq!(local_image_info_text(path, &unreadable), "10.1 KB");
        let gone = dir.path().join("gone.png");
        assert_eq!(
            local_image_info_text(gone.to_str().unwrap(), &unreadable),
            "Could not read image info"
        );
        assert_eq!(local_frame_meta(gone.to_str().unwrap(), &unreadable), "");
    }

    #[test]
    fn the_banner_says_whether_the_match_is_an_upgrade() {
        let (e, c) = matched(95.0, true, 2000, 2000);
        assert_eq!(comparison_banner_tier(&e, Some(&c)), BANNER_GOOD);
        assert_eq!(
            comparison_banner_text(&e, Some(&c)),
            "95% similar  \u{b7}  match is 2× larger"
        );
        let (e, c) = matched(80.0, false, 500, 500);
        assert_eq!(comparison_banner_tier(&e, Some(&c)), BANNER_UNCERTAIN);
        assert!(
            comparison_banner_text(&e, Some(&c))
                .starts_with("~80% similar (ranking)  \u{b7}  match is ")
        );
        let (e, c) = matched(90.0, true, 2000, 1000);
        assert_eq!(
            comparison_banner_tier(&e, Some(&c)),
            BANNER_UNCERTAIN,
            "a different shape"
        );
        assert_eq!(comparison_banner_tier(&e, None), BANNER_NEUTRAL);
        assert_eq!(comparison_banner_text(&e, None), "");
    }

    #[test]
    fn the_readout_splits_the_banner_into_parts() {
        let glyphs = |key: &str| match key {
            "good" => Some("\u{25cf}".to_owned()),
            "estimated" => Some("~".to_owned()),
            _ => None,
        };
        let (e, c) = matched(96.0, true, 2500, 2500);
        let r = comparison_readout(&e, Some(&c), &glyphs);
        assert_eq!(
            (
                r.glyph.as_str(),
                r.value.as_str(),
                r.label.as_str(),
                r.diff.as_str(),
                r.tier
            ),
            (
                "\u{25cf}",
                "96%",
                "Similarity",
                "\u{25b2} 2.5× larger",
                BANNER_GOOD
            )
        );
        assert_eq!(r.tooltip, comparison_banner_text(&e, Some(&c)));
        let (e, c) = matched(60.0, false, 500, 500);
        let r = comparison_readout(&e, Some(&c), &glyphs);
        assert_eq!(
            (r.glyph.as_str(), r.value.as_str(), r.label.as_str()),
            ("~", "~60%", "Ranking")
        );
        assert!(
            !r.diff.starts_with('\u{25b2}'),
            "a smaller match is not marked as an upgrade"
        );
        assert_eq!(
            comparison_readout(&e, None, &glyphs),
            ComparisonReadout::default()
        );
    }
}
