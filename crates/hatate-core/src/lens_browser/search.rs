//! The per-search half of `core/lens_browser.py`: `_search_area`, `_widen_search_area`, `_drag`,
//! `_settle`, `_click_if_present`, `_reveal_explicit_results`, `_collect_tiles` and `_one`, with
//! their constants. 5.c.
//!
//! [`LensSearch`] is the shipped [`SearchStep`]: the lifecycle (launch, profile, warm-up, challenge
//! wait, a window closed under a job) is `lens_browser.rs`'s, and it hands this the page and the
//! three things `_one` calls on `self`. The two addresses are fields, not constants of the
//! step: [`LensSearch::shipped`] is the only value the binary builds, and a test builds its own.

use std::sync::Arc;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::elements::ElementHandle;
use super::page::{LensPage, Locator, PageError};
use super::{JobRequest, SearchStep, StepCtx, StepError, browser_gone, first_line};
use crate::clock::BoxFuture;
use crate::engines::google_images::{GOOGLE_URL, SEARCH_BY_IMAGE_URL};
use crate::engines::lens_prep::{EXACT_TILES_JS, upload_script_to};
use crate::engines::lens_search::LensPayloads;
use crate::pycase::{ignorecase_matches, is_word_char};
use crate::pycompat;

/// `WHOLE_ENOUGH`: a search area at least this wide and tall is the whole picture for practical
/// purposes. Lens reports 98% x 98% for an uncropped image, and dragging for two percent would
/// cost a re-search for nothing.
pub const WHOLE_ENOUGH: f64 = 0.95;

/// `DRAG_OVERSHOOT_PX`: dragged slightly past the corner so the handle lands on the edge rather
/// than a pixel short of it. Lens clamps to the image.
pub const DRAG_OVERSHOOT_PX: f64 = 8.0;

/// Waiting is on results, not the clock: a step waits until the response it caused has arrived and
/// gone quiet, never longer than the fixed wait it replaces.
pub const SETTLE_POLL_MS: u64 = 250;
pub const SETTLE_QUIET_MS: u64 = 1500;
pub const AFTER_RESULTS_MAX_MS: u64 = 6000;
pub const AFTER_WIDEN_MAX_MS: u64 = 7000;
pub const AFTER_TAB_MAX_MS: u64 = 7000;
pub const AFTER_REVEAL_MAX_MS: u64 = 8000;
/// The exact-match tiles are drawn a moment after their response lands.
pub const TILE_RENDER_MAX_MS: u64 = 2000;
/// A control is looked for once, not waited for: absent is the common case.
pub const CLICK_TIMEOUT_MS: u64 = 5000;

/// `EXPLICIT_REVEAL_LABELS`: Google hides the matches for an explicit image behind a confirmation
/// ("These results may be explicit" with a control to go on), found by its words.
pub const EXPLICIT_REVEAL_LABELS: [&str; 4] = [
    "See exact matches",
    "Show explicit results",
    "See results",
    "Show results",
];

/// The two result tabs, in the order they are opened.
const TABS: [&str; 2] = ["Visual matches", "Exact matches"];

/// `RESULTS_URL_RE.search(url)`: `vsrid=|/sorry/`.
pub fn is_results_url(url: &str) -> bool {
    url.contains("vsrid=") || url.contains("/sorry/")
}

/// `"/search" in response.url`.
fn is_search_url(url: &str) -> bool {
    url.contains("/search")
}

/// `SOURCE_HINT_RE.search(body) is not None`, with `re.IGNORECASE`:
///
/// ```text
/// https?:(?:\\?/){2}(?!\w*\.?google|gstatic|googleusercontent|www\.w3\.org|schema\.org)[a-z0-9.\-]+\.[a-z]{2,}
/// ```
///
/// Hosts that mean a response actually carries results rather than chrome. The `regex` crate has
/// no look-ahead, and its `(?i)` folds by another rule than Python's, so this is the pattern
/// walked by hand, with `pycase` for the letters.
pub fn source_hint(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let letter = |c: char| ('a'..='z').any(|p| ignorecase_matches(p, c));
    let host_char = |c: char| c.is_ascii_digit() || c == '.' || c == '-' || letter(c);
    let word = |c: char| is_word_char(c);
    let matches_at = |at: usize, text: &str| -> bool {
        // A case-insensitive literal, `.` standing for itself.
        let mut i = at;
        for p in text.chars() {
            match chars.get(i) {
                Some(&c) if ignorecase_matches(p, c) => i += 1,
                _ => return false,
            }
        }
        true
    };
    for start in 0..chars.len() {
        if !matches_at(start, "http") {
            continue;
        }
        let mut i = start + 4;
        if matches_at(i, "s") {
            i += 1;
        }
        if chars.get(i) != Some(&':') {
            continue;
        }
        i += 1;
        let mut ok = true;
        for _ in 0..2 {
            if chars.get(i) == Some(&'\\') {
                i += 1;
            }
            if chars.get(i) == Some(&'/') {
                i += 1;
            } else {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        let p = i;
        // The look-ahead: `\w*\.?google` (some run of word characters, an optional dot, then
        // "google"), or one of the four hosts.
        let run = chars[p..].iter().take_while(|c| word(**c)).count();
        let excluded = (0..=run).any(|k| {
            matches_at(p + k, "google")
                || (chars.get(p + k) == Some(&'.') && matches_at(p + k + 1, "google"))
        }) || matches_at(p, "gstatic")
            || matches_at(p, "googleusercontent")
            || matches_at(p, "www.w3.org")
            || matches_at(p, "schema.org");
        if excluded {
            continue;
        }
        // `[a-z0-9.\-]+\.[a-z]{2,}`: a dot, not first, followed by two letters, inside a run of host characters.
        let host = chars[p..].iter().take_while(|c| host_char(**c)).count();
        if (1..host).any(|j| {
            chars[p + j] == '.'
                && chars.get(p + j + 1).is_some_and(|c| letter(*c))
                && chars.get(p + j + 2).is_some_and(|c| letter(*c))
        }) {
            return true;
        }
    }
    false
}

/// One handle of the search area: `{"element", "cx", "cy", "x", "y"}`.
struct Corner {
    element: ElementHandle,
    cx: f64,
    cy: f64,
    x: f64,
    y: f64,
}

/// `_search_area`'s result: the two corners.
struct Area {
    top_left: Corner,
    bottom_right: Corner,
}

/// `(\d+)%`.
static PERCENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)%").expect("percent"));

/// The shipped per-search step.
pub struct LensSearch {
    google_url: String,
    upload_url: String,
}

impl LensSearch {
    /// Google, as the binary uses it.
    pub fn shipped() -> Self {
        LensSearch::with_urls(GOOGLE_URL, SEARCH_BY_IMAGE_URL)
    }

    /// The page the search starts on and the address the upload form posts to. The only way to
    /// point Lens anywhere else, and only a test calls it.
    pub fn with_urls(google_url: &str, upload_url: &str) -> Self {
        LensSearch {
            google_url: google_url.to_owned(),
            upload_url: upload_url.to_owned(),
        }
    }

    /// `_search_area`: the region Lens is searching, read from the crop handles' aria-labels
    /// ("top left corner of search area: left 9%, top 4%"). Every percentage is measured from the
    /// image's left/top edge, whatever edge the label names. Not read from the URL, whose `vsint`
    /// says the whole image even while the handles say 9%-87%.
    async fn search_area(page: &LensPage) -> Result<Option<Area>, PageError> {
        let mut top_left = None;
        let mut bottom_right = None;
        for element in page
            .query_selector_all("[aria-label*='corner of search area']")
            .await?
        {
            let label = element
                .get_attribute("aria-label")
                .await?
                .unwrap_or_default();
            let corner = ["top left", "bottom right"]
                .into_iter()
                .find(|c| label.starts_with(c));
            let boxed = element.bounding_box().await?;
            let (Some(corner), Some(boxed)) = (corner, boxed) else {
                continue;
            };
            let found: Vec<&str> = PERCENT_RE
                .captures_iter(&label)
                .filter_map(|c| c.get(1).map(|m| m.as_str()))
                .collect();
            if found.len() != 2 {
                continue;
            }
            // A number past 64 bits is not read: a label never states one.
            let (Some(first), Some(second)) = (
                crate::boorus::sizes::py_int(found[0]),
                crate::boorus::sizes::py_int(found[1]),
            ) else {
                continue;
            };
            let percentages = [first, second];
            let found = Corner {
                element,
                cx: boxed.x + boxed.width / 2.0,
                cy: boxed.y + boxed.height / 2.0,
                x: percentages[0] as f64 / 100.0,
                y: percentages[1] as f64 / 100.0,
            };
            if corner == "top left" {
                top_left = Some(found);
            } else {
                bottom_right = Some(found);
            }
        }
        Ok(match (top_left, bottom_right) {
            (Some(top_left), Some(bottom_right)) => Some(Area {
                top_left,
                bottom_right,
            }),
            _ => None,
        })
    }

    /// `_widen_search_area`: drag the crop back out to the whole picture.
    ///
    /// Lens picks a region of the upload and searches only that. Usually it is the whole image,
    /// but it can settle on one detail and answer about that instead (78% x 93% and smaller).
    /// There is no URL to rewrite, so this moves the handles the way a person would: the two
    /// corner handles and the percentages they state give the image's rectangle, and each is
    /// dragged to the matching corner of it.
    async fn widen_search_area(
        ctx: &StepCtx,
        job: &JobRequest,
        capture: &super::elements::ResponseCapture,
    ) -> Result<bool, StepError> {
        if !job.whole_image {
            return Ok(false);
        }
        let page = ctx.page();
        let Some(area) = Self::search_area(page).await? else {
            return Ok(false);
        };
        let (top_left, bottom_right) = (&area.top_left, &area.bottom_right);
        let width = bottom_right.x - top_left.x;
        let height = bottom_right.y - top_left.y;
        if width <= 0.0 || height <= 0.0 {
            return Ok(false);
        }
        if width >= WHOLE_ENOUGH && height >= WHOLE_ENOUGH {
            return Ok(false); // already the whole picture, near enough
        }

        // Solve for the image's on-screen rectangle from two handles whose positions AND
        // percentages are both known.
        let image_w = (bottom_right.cx - top_left.cx) / width;
        let image_h = (bottom_right.cy - top_left.cy) / height;
        let image_x = top_left.cx - image_w * top_left.x;
        let image_y = top_left.cy - image_h * top_left.y;

        tracing::info!(
            "Lens cropped the search to {:.0}% x {:.0}% of the image - dragging it back out to \
             the whole picture",
            width * 100.0,
            height * 100.0
        );
        Self::drag(
            page,
            &top_left.element,
            image_x - DRAG_OVERSHOOT_PX,
            image_y - DRAG_OVERSHOOT_PX,
        )
        .await?;
        // The handles are re-rendered by the first drag, so they have to be looked up again
        // rather than reused.
        if let Some(again) = Self::search_area(page).await? {
            Self::drag(
                page,
                &again.bottom_right.element,
                image_x + image_w + DRAG_OVERSHOOT_PX,
                image_y + image_h + DRAG_OVERSHOOT_PX,
            )
            .await?;
        }
        Self::settle(ctx, capture, AFTER_WIDEN_MAX_MS, true).await?;
        ctx.wait_out_challenge("after widening the search area")
            .await?;
        Ok(true)
    }

    /// `_drag`.
    async fn drag(
        page: &LensPage,
        element: &ElementHandle,
        to_x: f64,
        to_y: f64,
    ) -> Result<(), PageError> {
        let Some(boxed) = element.bounding_box().await? else {
            return Ok(());
        };
        page.mouse_move(boxed.x + boxed.width / 2.0, boxed.y + boxed.height / 2.0, 1)
            .await?;
        page.mouse_down().await?;
        page.mouse_move(to_x, to_y, 12).await?;
        page.mouse_up().await?;
        page.wait_for_timeout(1200).await
    }

    /// `_settle`: waits until the results a step asked for have arrived.
    ///
    /// Done when a new response has come in and nothing further has for [`SETTLE_QUIET_MS`], or,
    /// with `need_new` false, when nothing has for that long at all. Never longer than `max_ms`,
    /// the fixed wait this replaces, so a step is never slower than it was.
    async fn settle(
        ctx: &StepCtx,
        capture: &super::elements::ResponseCapture,
        max_ms: u64,
        need_new: bool,
    ) -> Result<(), StepError> {
        let start = capture.len();
        let mut seen = start;
        let (mut waited, mut quiet) = (0, 0);
        while waited < max_ms {
            ctx.page().wait_for_timeout(SETTLE_POLL_MS).await?;
            waited += SETTLE_POLL_MS;
            if ctx.is_challenged().await? {
                return Ok(()); // the caller calls the user straight away
            }
            if capture.len() != seen {
                seen = capture.len();
                quiet = 0;
                continue;
            }
            quiet += SETTLE_POLL_MS;
            if quiet >= SETTLE_QUIET_MS && (seen > start || !need_new) {
                return Ok(());
            }
        }
        Ok(())
    }

    /// `_click_if_present`: clicks the control if the page has it, without waiting out a timeout
    /// for one it doesn't, which is the common case.
    async fn click_if_present(locator: Locator) -> Result<bool, StepError> {
        let attempt = async {
            if locator.count().await? == 0 {
                return Ok(false);
            }
            locator.click(CLICK_TIMEOUT_MS).await?;
            Ok::<bool, PageError>(true)
        };
        match attempt.await {
            Ok(clicked) => Ok(clicked),
            Err(error) => {
                let error = StepError::Page(error);
                if browser_gone(&error) {
                    return Err(error);
                }
                tracing::debug!(
                    "Could not click ({})",
                    first_line(&error_text(&error), "Error")
                );
                Ok(false)
            }
        }
    }

    /// `_reveal_explicit_results`: click past the "these results may be explicit" confirmation.
    async fn reveal_explicit_results(
        ctx: &StepCtx,
        capture: &super::elements::ResponseCapture,
    ) -> Result<bool, StepError> {
        for label in EXPLICIT_REVEAL_LABELS {
            if !Self::click_if_present(ctx.page().get_by_text_exact(label)).await? {
                continue;
            }
            tracing::info!("Confirmed Google's explicit-results notice ({label:?})");
            Self::settle(ctx, capture, AFTER_REVEAL_MAX_MS, true).await?;
            ctx.wait_out_challenge("after the explicit-results notice")
                .await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `_collect_tiles`: the rendered result tiles, their text and their thumbnail. Read from the
    /// DOM rather than the response body, because the body's thumbnails are deferred
    /// placeholders. This is the only way an Exact-matches result gets a picture to be compared
    /// against.
    async fn collect_tiles(page: &LensPage) -> Result<Vec<Value>, StepError> {
        let mut waited = 0;
        loop {
            let tiles = match page.evaluate_value(EXACT_TILES_JS).await {
                Ok(tiles) => tiles,
                Err(error) => {
                    // A missing tab is normal.
                    let error = StepError::Page(error);
                    if browser_gone(&error) {
                        return Err(error);
                    }
                    tracing::debug!("Could not read the result tiles ({})", error_text(&error));
                    return Ok(Vec::new());
                }
            };
            let found: Vec<Value> = tiles
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter(|t| {
                            t.as_object()
                                .and_then(|o| o.get("thumb"))
                                .is_some_and(pycompat::truthy)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            if !found.is_empty() || waited >= TILE_RENDER_MAX_MS {
                return Ok(found);
            }
            page.wait_for_timeout(SETTLE_POLL_MS * 2).await?;
            waited += SETTLE_POLL_MS * 2;
        }
    }

    /// `_one`: uploads one image and reads what Lens shows for it.
    async fn one(&self, ctx: StepCtx, job: Arc<JobRequest>) -> Result<LensPayloads, StepError> {
        ctx.warm_up().await?;
        // Nothing here needs the user, so the window keeps to itself, until and unless Google
        // asks for a robot check below.
        ctx.quiet_attention();
        let page = ctx.page().clone();
        let capture = page.capture_responses(is_search_url, source_hint).await?;
        let timeout_ms = (job.timeout * 1000.0) as u64;
        // Bound before the try: an exception on the way to the widen must not leave the return
        // below reaching for a name that was never assigned.
        let mut widened_from = 0;
        let mut tiles: Vec<Value> = Vec::new();
        let outcome: Result<(), StepError> = async {
            page.goto(&self.google_url, timeout_ms).await?;
            page.evaluate_value(&upload_script_to(
                &job.image_bytes,
                &job.filename,
                &self.upload_url,
            ))
            .await?;
            page.wait_for_url(is_results_url, timeout_ms).await?;

            ctx.wait_out_challenge("after the upload").await?;

            Self::settle(&ctx, &capture, AFTER_RESULTS_MAX_MS, false).await?;
            // Everything captured so far belongs to the CROPPED search. Anything arriving after
            // this point is the widened one, and is preferred, but only if it arrives. Clearing
            // the list outright meant a widen whose re-search was not captured left the image
            // with NOTHING, which is worse than answering from the crop.
            widened_from = capture.len();
            if !Self::widen_search_area(&ctx, &job, &capture).await? {
                widened_from = 0;
            }
            // BOTH result tabs, because they hold different things. "Visual matches" lists
            // pictures that look alike and embeds their URLs. "Exact matches" lists the SAME
            // picture, the booru posts this app is actually after.
            for tab in TABS {
                // Absent when that tab is already the one showing: the explicit-results
                // confirmation lands on Exact matches.
                if Self::click_if_present(page.get_by_link(tab)).await? {
                    Self::settle(&ctx, &capture, AFTER_TAB_MAX_MS, true).await?;
                    ctx.wait_out_challenge(&format!("after opening {tab}"))
                        .await?;
                } else {
                    tracing::debug!("No {tab} tab to click");
                }
                Self::reveal_explicit_results(&ctx, &capture).await?;
                if tab == "Exact matches" {
                    tiles = Self::collect_tiles(&page).await?;
                }
            }
            Ok(())
        }
        .await;
        // The `finally`.
        let bodies = capture.since(0);
        capture.stop();
        page.release_handles().await;
        // Clears whatever attention the search itself asked for.
        ctx.quiet_attention();
        outcome?;

        // Every response is handed back, not just the biggest: clicking through the
        // explicit-results notice loads a further page, and the largest body afterwards was one
        // that parsed to nothing while a smaller, earlier one held all the matches. The caller
        // picks by what actually parses. Responses from after the crop was widened win, since
        // they are about the whole picture, but if the widened search produced none that were
        // captured, the cropped ones are still better than returning nothing at all.
        let widened: Vec<String> = bodies.get(widened_from..).unwrap_or_default().to_vec();
        let payloads = if widened.is_empty() { bodies } else { widened };
        Ok(LensPayloads {
            payloads,
            tiles: Value::Array(tiles),
        })
    }
}

fn error_text(error: &StepError) -> String {
    match error {
        StepError::Lens(e) => e.python_text().to_owned(),
        StepError::Page(e) => e.message().to_owned(),
    }
}

impl SearchStep for LensSearch {
    fn one(
        &self,
        ctx: StepCtx,
        job: Arc<JobRequest>,
    ) -> BoxFuture<'_, Result<LensPayloads, StepError>> {
        Box::pin(self.one(ctx, job))
    }
}
