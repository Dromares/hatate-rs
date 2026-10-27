//! The Queue's right-click menu as data (`gui/table_context_menu.py` at `76c85c4`; DAN-2217).
//!
//! Python builds a `QMenu` of `MenuItem`s, each carrying its label, handler, enabled flag and
//! tooltip. This is the same menu with the Qt taken out: [`build`] takes the whole list and the
//! rows the menu was opened on, and returns the entries in Python's order with Python's labels,
//! counts, enabled flags and tooltips, and for each one the [`Action`] it runs. The window turns
//! that into a popover; a differential can compare it with Python's list without a display.
//!
//! The re-search entries are here so the menu is the whole menu, but their handlers (and the
//! Hydrus ones) belong to later tickets: DAN-2217 registers them, 6.e and 6.g wire them.

use crate::model::{Entry, MatchStatus};
use crate::queue::{has_result, needs_review};

/// `_update_clear_cache_action_label`: `Clear Search Cache (1.5 KB)…`; an empty cache says `0 B`.
pub fn clear_cache_label(size_bytes: u64) -> String {
    let size = crate::preview_text::human_size(i64::try_from(size_bytes).ok());
    format!(
        "Clear Search Cache ({})\u{2026}",
        size.as_deref().unwrap_or("0 B")
    )
}

/// `count_label`: "Reset result" for one row, "Reset results on 12 selected images" for more.
/// `plural` takes `{n}`.
pub fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        singular.to_owned()
    } else {
        plural.replace("{n}", &count.to_string())
    }
}

/// The selections the "Select by ..." sub-menus make that a status cannot express
/// (`SENT_STATES`, `CACHE_STATES`, `REVIEW_STATES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Predicate {
    NotSent,
    Queued,
    Sent,
    MissingFromDisk,
    Provisional,
    FromCache,
    FreshlySearched,
    NeedsReview,
    Reviewed,
    Decided,
}

impl Predicate {
    pub const SENT: [Predicate; 4] = [
        Predicate::NotSent,
        Predicate::Queued,
        Predicate::Sent,
        Predicate::MissingFromDisk,
    ];
    pub const CACHE: [Predicate; 3] = [
        Predicate::Provisional,
        Predicate::FromCache,
        Predicate::FreshlySearched,
    ];
    pub const REVIEW: [Predicate; 3] = [
        Predicate::NeedsReview,
        Predicate::Reviewed,
        Predicate::Decided,
    ];

    /// The words the menu entry and the status line use for it.
    pub fn description(self) -> &'static str {
        match self {
            Predicate::NotSent => "Not sent to Hydrus",
            Predicate::Queued => "Queued (unconfirmed)",
            Predicate::Sent => "Sent to Hydrus",
            Predicate::MissingFromDisk => "Missing from disk",
            Predicate::Provisional => "Provisional (searched without SauceNAO)",
            Predicate::FromCache => "From the cache",
            Predicate::FreshlySearched => "Freshly searched",
            Predicate::NeedsReview => "Needs review",
            Predicate::Reviewed => "Reviewed (kept the local file)",
            Predicate::Decided => "Decided (reviewed or sent)",
        }
    }

    pub fn test(self, e: &Entry) -> bool {
        let source = e.result_source.as_deref();
        match self {
            Predicate::NotSent => !e.sent_to_hydrus,
            Predicate::Queued => e.sent_to_hydrus && !e.hydrus_import_confirmed,
            Predicate::Sent => e.sent_to_hydrus && e.hydrus_import_confirmed,
            Predicate::MissingFromDisk => e.file_missing,
            Predicate::Provisional => e.searched_without_saucenao,
            Predicate::FromCache => source == Some("cached") && !e.searched_without_saucenao,
            Predicate::FreshlySearched => source == Some("fresh") && !e.searched_without_saucenao,
            Predicate::NeedsReview => needs_review(e),
            Predicate::Reviewed => e.reviewed,
            Predicate::Decided => !needs_review(e),
        }
    }
}

/// What an entry does when it is picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SelectByStatus(MatchStatus),
    SelectByPredicate(Predicate),
    Research,
    Reset,
    SendToHydrus,
    ImportUrlToHydrus,
    DownloadAndSendToHydrus,
    /// `_set_rows_reviewed(entries, reviewed)`.
    SetReviewed(bool),
    Compare,
    CheckUpscaling,
    ShowInHydrus,
    CheckAvailability,
    Export,
    WriteTagFiles,
    DeleteFromHydrus,
    Remove,
    ResearchWithEngine(&'static str),
    ResearchWithOppositeEngine,
}

/// One entry: what it says, whether it can be used, its tooltip ("" for none) and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub enabled: bool,
    pub tooltip: &'static str,
    pub action: Action,
}

/// An item or a separator (Python's `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Item(Item),
    Separator,
}

/// The "Search with a specific engine" sub-menu's tooltip.
pub const ENGINE_MENU_TOOLTIP: &str = "Runs a fresh search using only the engine you pick, ignoring your configured primary/fallback order for this one search. Results bypass the cache.";

/// The whole menu: the four "Select by" sub-menus, which are always there, and, when rows are
/// selected, the lines that act on them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// "Select by Status", "Select by Sent State", "Select by Cache State", "Select by Review State".
    pub select_by: Vec<(&'static str, Vec<Item>)>,
    pub rows: Option<RowLines>,
}

/// The lines a selection adds: Re-search and Reset, the engine sub-menu, then the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowLines {
    pub head: Vec<Line>,
    pub engines: Vec<Line>,
    pub tail: Vec<Line>,
}

const TIP_RESET: &str = "Puts these back to Not searched, discarding the match, its tags and any error - as though they had just been added.\n\nTags you typed yourself and tags Hydrus already holds are kept; only the ones this search brought in are dropped. Whether a file has been sent to Hydrus is left alone too, since that stays true either way.\n\nThe saved cache entry is dropped as well, so the next search really does search rather than handing back the result being discarded here.\n\nNothing on disk, and nothing in Hydrus, is touched.";
const TIP_SEND: &str =
    "Uploads the local file, associates the matched URL with it, and adds the tag list.";
const TIP_IMPORT_URL: &str = "Hands the matched URL to Hydrus's own downloader, which fetches and imports the file itself - like pasting it into Hydrus's URL box.";
const TIP_DOWNLOAD: &str = "Downloads the actual full-resolution matched image ourselves (not your local file, not through Hydrus's downloader) and uploads that to Hydrus with the URL and tags already attached.";
const TIP_REVIEW: &str = "Records that you have looked at these and decided against sending them - the local copy is better, or the match is wrong.\n\nNothing is sent, changed on disk, or touched in Hydrus. It only takes the row out of \"Needs review\", so a pass over a large library can be put down and picked up without hunting for your place: press N, or use Select by Review State.\n\nResetting a result clears the mark, since the decision was about the match being discarded.";
const TIP_COMPARE: &str = "Opens the local file and its match side by side with a wipe slider, fetching the match at full resolution rather than the downscaled preview, and reports whether they're actually the same image.";
const TIP_UPSCALE: &str = "Checks image metadata for known AI upscaler tool names (reliable when present), compares against the matched source's size, and tests whether a downscale+re-upscale round-trip closely reconstructs the image (a heuristic - can't reliably detect AI-based upscaling on its own).";
const TIP_SHOW: &str = "Opens a new page in the Hydrus client showing these files, and switches Hydrus to it.\n\nThe opposite direction to everything else here: instead of sending a result to Hydrus, it jumps to the copy Hydrus already holds, so you can check it against its real tags, duplicates and ratings.\n\nOnly works for files Hydrus actually has, and needs the \"manage pages\" permission on your access key - which nothing else here uses, so it may need adding.";
const TIP_AVAIL: &str = "Checks whether each matched site still has the content, and offers to drop any that are definitely gone (deleted/removed posts) from this image's match list. Only removes matches returning a definite 404/410 - anything unreachable or merely blocked stays, since that doesn't mean the post is gone. Works on one image at a time.";
const TIP_EXPORT: &str =
    "Writes just these rows out as CSV or JSON. Files > Export Results does the whole list.";
const TIP_TAGS: &str = "Writes each image's tags to a text file next to the image itself, one tag per line - what other taggers and Hydrus's own sidecar importer read.\n\nThe only thing in this menu that writes into your picture folders. It shows you what it is about to write and never replaces a text file already there without asking. Files > Write Tag Files does the whole list.";
const TIP_DELETE: &str = "Tells Hydrus to delete these files, which is the only correct way to get rid of something living in Hydrus's own store.\n\nDeleting the file off disk instead leaves Hydrus's record behind, so it goes on believing it holds a file that isn't there - a worse state than either keeping it or deleting it properly.\n\nHydrus moves them to its trash, so this is undoable from Hydrus itself until the trash is emptied.";
const TIP_REMOVE: &str = "Takes them out of THIS list only. The files, and Hydrus, are untouched.";
const TIP_OPPOSITE: &str = "Per row, uses whichever engine did NOT find that row's current match - so a SauceNAO match gets retried on IQDB and vice versa. Rows with no match yet fall back to whichever engine isn't your configured primary.";

/// `ENGINE_ITEMS`: (engine, label, tooltip); `None` is a separator.
const ENGINE_ITEMS: [Option<(&str, &str, &str)>; 11] = [
    Some((
        "iqdb",
        "IQDB only",
        "Searches IQDB and nothing else. IQDB has no daily quota, so this is the one to use when SauceNAO's allowance is spent.",
    )),
    Some((
        "saucenao",
        "SauceNAO only",
        "Searches SauceNAO and nothing else.",
    )),
    None,
    Some((
        "ascii2d",
        "ascii2d only",
        "Colour and feature search. Often finds Pixiv and Twitter posts IQDB misses. Does not report a real similarity percentage.",
    )),
    Some((
        "tracemoe",
        "trace.moe only",
        "Anime-screenshot search: which series, episode, and timestamp. Anonymous use has a monthly quota - leave this for actual screenshots.",
    )),
    Some((
        "iqdb3d",
        "IQDB 3D only",
        "3d.iqdb.org - the 3D/CG index, same protocol as IQDB.",
    )),
    Some((
        "googleimages",
        "Google Images only",
        "Reverse-searches the whole web rather than a booru index. Worth trying on an image the others cannot place: it finds the page a picture was posted on, though it brings back no tags. Needs the Cloud Vision API key from Settings > Engine.",
    )),
    Some((
        "pawchive",
        "Pawchive only",
        "Looks this exact file up on pawchive.pw by its SHA-256 - Patreon and Fanbox posts that SauceNAO and IQDB don't index. Only finds identical copies, and sends the file's hash, not the picture.",
    )),
    Some((
        "pawchiveindex",
        "Pawchive index only",
        "Searches the local index of pawchive artists (Files > Pawchive Index) for this picture, including resized or re-saved copies. Local - nothing is sent.",
    )),
    Some((
        "googlelens",
        "Google Lens only",
        "The reverse image search from images.google.com, read out of a real browser. Usually the best of these on art nothing else can place - and the slowest, since every image is a page load. Needs the optional PyQt6-WebEngine package.",
    )),
    Some((
        "yandex",
        "Yandex only",
        "yandex.com's reverse image search. Finds rule34.us, Danbooru and Xbooru posts the others miss, and keeps only posts on sites the app can read tags from. Uploads the picture to Yandex.",
    )),
];

fn item(label: impl Into<String>, action: Action, enabled: bool, tooltip: &'static str) -> Line {
    Line::Item(Item {
        label: label.into(),
        enabled,
        tooltip,
        action,
    })
}

/// `_all_reviewed`: the whole selection is already marked. A mixed selection is not, so the entry
/// marks rather than unmarks.
pub fn all_reviewed(selected: &[&Entry]) -> bool {
    !selected.is_empty() && selected.iter().all(|e| e.reviewed)
}

fn review_mark_label(selected: &[&Entry], count: usize) -> String {
    if all_reviewed(selected) {
        count_label(
            count,
            "Unmark as reviewed",
            "Unmark {n} selected images as reviewed",
        )
    } else {
        count_label(
            count,
            "Mark as reviewed",
            "Mark {n} selected images as reviewed",
        )
    }
}

fn predicate_items(all: &[Entry], predicates: &[Predicate]) -> Vec<Item> {
    predicates
        .iter()
        .map(|&p| {
            let matching = all.iter().filter(|e| p.test(e)).count();
            Item {
                label: format!("{} ({matching})", p.description()),
                enabled: matching > 0,
                tooltip: "",
                action: Action::SelectByPredicate(p),
            }
        })
        .collect()
}

/// `selection_items`: statuses in `sort_rank` order (good, review, not found, error, searching,
/// not searched).
fn status_items(all: &[Entry]) -> Vec<Item> {
    const BY_RANK: [MatchStatus; 6] = [
        MatchStatus::Good,
        MatchStatus::Poor,
        MatchStatus::NotFound,
        MatchStatus::Error,
        MatchStatus::Searching,
        MatchStatus::NotSearched,
    ];
    BY_RANK
        .iter()
        .map(|&s| {
            let matching = all.iter().filter(|e| e.status == s).count();
            Item {
                label: format!("{} ({matching})", s.label()),
                enabled: matching > 0,
                tooltip: "",
                action: Action::SelectByStatus(s),
            }
        })
        .collect()
}

/// `row_items`: everything that acts on the selected rows, with Python's separators.
fn row_lines(selected: &[&Entry]) -> (Vec<Line>, Vec<Line>) {
    let count = selected.len();
    let resettable = selected.iter().filter(|e| has_result(e)).count();
    let head = vec![
        item(
            count_label(
                count,
                "Re-search selected image",
                "Re-search {n} selected images",
            ),
            Action::Research,
            true,
            "",
        ),
        item(
            count_label(
                count,
                "Reset result",
                "Reset results on {n} selected images",
            ),
            Action::Reset,
            resettable > 0,
            TIP_RESET,
        ),
    ];
    let tail = vec![
        Line::Separator,
        item(
            "Send File + URL + Tags to Hydrus (upload)",
            Action::SendToHydrus,
            true,
            TIP_SEND,
        ),
        item(
            "Send URL to Hydrus's URL Importer\u{2026}",
            Action::ImportUrlToHydrus,
            true,
            TIP_IMPORT_URL,
        ),
        item(
            "Download Matched Image + Send to Hydrus with Tags",
            Action::DownloadAndSendToHydrus,
            true,
            TIP_DOWNLOAD,
        ),
        Line::Separator,
        item(
            review_mark_label(selected, count),
            Action::SetReviewed(!all_reviewed(selected)),
            true,
            TIP_REVIEW,
        ),
        Line::Separator,
        item(
            "Compare with Match\u{2026}",
            Action::Compare,
            count == 1,
            TIP_COMPARE,
        ),
        Line::Separator,
        item(
            count_label(
                count,
                "Check for Upscaling",
                "Check {n} images for Upscaling",
            ),
            Action::CheckUpscaling,
            true,
            TIP_UPSCALE,
        ),
        Line::Separator,
        item(
            count_label(
                count,
                "Show in Hydrus",
                "Show {n} selected images in Hydrus",
            ),
            Action::ShowInHydrus,
            selected.iter().any(|e| e.hydrus_hash.is_some()),
            TIP_SHOW,
        ),
        Line::Separator,
        item(
            "Check Match Availability (remove dead links)",
            Action::CheckAvailability,
            count == 1,
            TIP_AVAIL,
        ),
        Line::Separator,
        item(
            count_label(
                count,
                "Export this image\u{2026}",
                "Export {n} selected images\u{2026}",
            ),
            Action::Export,
            true,
            TIP_EXPORT,
        ),
        item(
            count_label(
                count,
                "Write Tag File beside this image\u{2026}",
                "Write Tag Files beside {n} selected images\u{2026}",
            ),
            Action::WriteTagFiles,
            true,
            TIP_TAGS,
        ),
        Line::Separator,
        item(
            count_label(
                count,
                "Delete from Hydrus\u{2026}",
                "Delete {n} files from Hydrus\u{2026}",
            ),
            Action::DeleteFromHydrus,
            true,
            TIP_DELETE,
        ),
        item(
            count_label(count, "Remove selected file", "Remove {n} selected files"),
            Action::Remove,
            true,
            TIP_REMOVE,
        ),
    ];
    (head, tail)
}

/// `engine_items`.
fn engine_lines(count: usize) -> Vec<Line> {
    let mut lines: Vec<Line> = ENGINE_ITEMS
        .iter()
        .map(|spec| match spec {
            None => Line::Separator,
            Some((engine, label, tooltip)) => {
                item(*label, Action::ResearchWithEngine(engine), true, tooltip)
            }
        })
        .collect();
    lines.push(Line::Separator);
    lines.push(item(
        count_label(
            count,
            "Search with Opposite Engine",
            "Search {n} images with Opposite Engine",
        ),
        Action::ResearchWithOppositeEngine,
        true,
        TIP_OPPOSITE,
    ));
    lines
}

/// `build(window, rows)`: the menu for `all` (the whole list, `window.entries`) with `selected`
/// (the entries of the selected rows) under the pointer. With nothing selected only the "Select
/// by" sub-menus are offered.
pub fn build(all: &[Entry], selected: &[&Entry]) -> Menu {
    let select_by = vec![
        ("Select by Status", status_items(all)),
        (
            "Select by Sent State",
            predicate_items(all, &Predicate::SENT),
        ),
        (
            "Select by Cache State",
            predicate_items(all, &Predicate::CACHE),
        ),
        (
            "Select by Review State",
            predicate_items(all, &Predicate::REVIEW),
        ),
    ];
    if selected.is_empty() {
        return Menu {
            select_by,
            rows: None,
        };
    }
    let (head, tail) = row_lines(selected);
    Menu {
        select_by,
        rows: Some(RowLines {
            head,
            engines: engine_lines(selected.len()),
            tail,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, status: MatchStatus) -> Entry {
        let mut e = Entry::new(path);
        e.status = status;
        e
    }

    fn labels(lines: &[Line]) -> Vec<Option<&str>> {
        lines
            .iter()
            .map(|l| match l {
                Line::Item(i) => Some(i.label.as_str()),
                Line::Separator => None,
            })
            .collect()
    }

    #[test]
    fn a_count_in_the_label_only_past_one() {
        assert_eq!(
            count_label(1, "Reset result", "Reset results on {n} selected images"),
            "Reset result"
        );
        assert_eq!(
            count_label(12, "Reset result", "Reset results on {n} selected images"),
            "Reset results on 12 selected images"
        );
        assert_eq!(count_label(0, "a", "{n} b"), "0 b");
    }

    #[test]
    fn nothing_selected_offers_only_the_four_select_by_menus_in_rank_order() {
        let all = vec![
            entry("/a.png", MatchStatus::NotSearched),
            entry("/b.png", MatchStatus::Good),
        ];
        let menu = build(&all, &[]);
        assert!(menu.rows.is_none());
        let titles: Vec<_> = menu.select_by.iter().map(|(t, _)| *t).collect();
        assert_eq!(
            titles,
            [
                "Select by Status",
                "Select by Sent State",
                "Select by Cache State",
                "Select by Review State"
            ]
        );
        let statuses: Vec<_> = menu.select_by[0]
            .1
            .iter()
            .map(|i| (i.label.as_str(), i.enabled))
            .collect();
        assert_eq!(
            statuses,
            [
                ("Found (good) (1)", true),
                ("Found (review) (0)", false),
                ("Not found (0)", false),
                ("Error (0)", false),
                ("Searching... (0)", false),
                ("Not searched (1)", true),
            ]
        );
    }

    #[test]
    fn the_sent_cache_and_review_menus_count_over_the_whole_list() {
        let mut sent = entry("/s.png", MatchStatus::Good);
        sent.sent_to_hydrus = true;
        sent.hydrus_import_confirmed = true;
        let mut queued = entry("/q.png", MatchStatus::Good);
        queued.sent_to_hydrus = true;
        let mut cached = entry("/c.png", MatchStatus::Good);
        cached.result_source = Some("cached".into());
        let mut prov = entry("/p.png", MatchStatus::Good);
        prov.result_source = Some("cached".into());
        prov.searched_without_saucenao = true;
        let all = vec![
            sent,
            queued,
            cached,
            prov,
            entry("/n.png", MatchStatus::NotSearched),
        ];
        let menu = build(&all, &[]);
        let count =
            |menu: usize| -> Vec<String> { menu_labels(&menu_items(&build(&all, &[]), menu)) };
        assert_eq!(
            count(1),
            [
                "Not sent to Hydrus (3)",
                "Queued (unconfirmed) (1)",
                "Sent to Hydrus (1)",
                "Missing from disk (0)"
            ]
        );
        assert_eq!(
            count(2),
            [
                "Provisional (searched without SauceNAO) (1)",
                "From the cache (1)",
                "Freshly searched (0)"
            ]
        );
        assert_eq!(
            count(3)[0],
            "Needs review (3)",
            "sent and unsent rows with a result are both waiting, as `needs_review` says"
        );
        let _ = menu;
    }

    fn menu_items(menu: &Menu, at: usize) -> Vec<Item> {
        menu.select_by[at].1.clone()
    }

    fn menu_labels(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn one_selected_row_reads_in_the_singular_and_enables_the_single_row_entries() {
        let mut e = entry("/a.png", MatchStatus::Good);
        e.hydrus_hash = Some("ab".repeat(32));
        let all = vec![e.clone()];
        let rows = build(&all, &[&all[0]]).rows.unwrap();
        assert_eq!(
            labels(&rows.head),
            [Some("Re-search selected image"), Some("Reset result")]
        );
        let tail = labels(&rows.tail);
        assert!(tail.contains(&Some("Compare with Match\u{2026}")));
        assert!(tail.contains(&Some("Remove selected file")));
        assert!(tail.contains(&Some("Mark as reviewed")));
        let enabled = |lines: &[Line], label: &str| -> bool {
            lines
                .iter()
                .find_map(|l| match l {
                    Line::Item(i) if i.label == label => Some(i.enabled),
                    _ => None,
                })
                .unwrap()
        };
        assert!(enabled(&rows.tail, "Compare with Match\u{2026}"));
        assert!(enabled(&rows.tail, "Show in Hydrus"));
        assert!(enabled(
            &rows.tail,
            "Check Match Availability (remove dead links)"
        ));
    }

    #[test]
    fn several_rows_carry_the_count_and_switch_off_the_single_row_entries() {
        let all = vec![
            entry("/a.png", MatchStatus::Good),
            entry("/b.png", MatchStatus::NotSearched),
            entry("/c.png", MatchStatus::NotSearched),
        ];
        let sel: Vec<&Entry> = all.iter().collect();
        let rows = build(&all, &sel).rows.unwrap();
        let tail = labels(&rows.tail);
        for want in [
            "Mark 3 selected images as reviewed",
            "Check 3 images for Upscaling",
            "Show 3 selected images in Hydrus",
            "Export 3 selected images\u{2026}",
            "Write Tag Files beside 3 selected images\u{2026}",
            "Delete 3 files from Hydrus\u{2026}",
            "Remove 3 selected files",
        ] {
            assert!(tail.contains(&Some(want)), "{want}");
        }
        let disabled: Vec<_> = rows
            .tail
            .iter()
            .filter_map(|l| match l {
                Line::Item(i) if !i.enabled => Some(i.label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            disabled,
            [
                "Compare with Match\u{2026}",
                "Show 3 selected images in Hydrus",
                "Check Match Availability (remove dead links)"
            ],
            "Show in Hydrus is off because no selected row has a hash"
        );
        assert_eq!(
            labels(&rows.engines).last().copied().flatten(),
            Some("Search 3 images with Opposite Engine")
        );
    }

    #[test]
    fn reset_is_off_until_a_selected_row_has_a_result() {
        let none = vec![entry("/a.png", MatchStatus::NotSearched)];
        let rows = build(&none, &[&none[0]]).rows.unwrap();
        let Line::Item(reset) = &rows.head[1] else {
            panic!()
        };
        assert!(!reset.enabled);
        let mut done = entry("/b.png", MatchStatus::Good);
        done.candidates = vec![];
        let hit = vec![done];
        let rows = build(&hit, &[&hit[0]]).rows.unwrap();
        let Line::Item(reset) = &rows.head[1] else {
            panic!()
        };
        assert_eq!(reset.enabled, has_result(&hit[0]));
    }

    #[test]
    fn the_review_entry_marks_unless_every_selected_row_is_already_marked() {
        let mut a = entry("/a.png", MatchStatus::Good);
        a.reviewed = true;
        let b = entry("/b.png", MatchStatus::Good);
        let all = vec![a, b];
        let both: Vec<&Entry> = all.iter().collect();
        let mixed = build(&all, &both).rows.unwrap();
        assert!(mixed.tail.iter().any(|l| matches!(l, Line::Item(i)
            if i.action == Action::SetReviewed(true) && i.label == "Mark 2 selected images as reviewed")));
        let marked = build(&all, &[&all[0]]).rows.unwrap();
        assert!(marked.tail.iter().any(|l| matches!(l, Line::Item(i)
            if i.action == Action::SetReviewed(false) && i.label == "Unmark as reviewed")));
    }

    #[test]
    fn the_engine_menu_lists_all_ten_engines_then_the_opposite_one() {
        let all = vec![entry("/a.png", MatchStatus::Good)];
        let rows = build(&all, &[&all[0]]).rows.unwrap();
        let engines: Vec<_> = rows
            .engines
            .iter()
            .filter_map(|l| match l {
                Line::Item(Item {
                    action: Action::ResearchWithEngine(e),
                    ..
                }) => Some(*e),
                _ => None,
            })
            .collect();
        assert_eq!(
            engines,
            [
                "iqdb",
                "saucenao",
                "ascii2d",
                "tracemoe",
                "iqdb3d",
                "googleimages",
                "pawchive",
                "pawchiveindex",
                "googlelens",
                "yandex"
            ]
        );
        assert_eq!(
            rows.engines
                .iter()
                .filter(|l| **l == Line::Separator)
                .count(),
            2
        );
    }

    #[test]
    fn the_clear_cache_label_carries_the_cache_size() {
        assert_eq!(clear_cache_label(0), "Clear Search Cache (0 B)\u{2026}");
        assert_eq!(
            clear_cache_label(1536),
            "Clear Search Cache (1.5 KB)\u{2026}"
        );
        assert_eq!(
            clear_cache_label(5 * 1024 * 1024),
            "Clear Search Cache (5.0 MB)\u{2026}"
        );
    }
}
