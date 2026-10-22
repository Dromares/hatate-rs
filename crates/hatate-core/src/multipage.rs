//! `core/multipage.py`: picking the right page of a multi-page post.
//!
//! A Pixiv artwork can hold dozens of images under one URL and a search engine reports the artwork,
//! not the page. Nothing in the match metadata reliably says which page it was, but "which of
//! these images is the one I have?" is what a perceptual hash answers: each page's small thumbnail
//! is hashed and compared with the local image, and the closest wins, when it is clearly closer
//! than the rest ([`choose_matching_page`]). SauceNAO names the page in its own thumbnail URL
//! ([`page_index_from_thumbnail`]), which is evidence and never truth. A page something has
//! already named can be confirmed for one download ([`check_page`]) however many pages the post
//! has.
//!
//! The hashes are [`crate::image_compare::dhash`]'s, equal to Pillow's. The page choice is a
//! comparison of hamming distances, and one bit flips it, which is why that matters.

use std::sync::LazyLock;

use regex::Regex;
use tracing::debug;

use crate::boorus::host;
use crate::boorus::sizes::py_int;
use crate::image_compare::{Hash, Source, dhash, hamming_distance};
use crate::pyimage::DecompressionBomb;
use crate::pyurl::{self, UrlValueError};

/// Beyond this, the comparison costs more than the error it prevents.
pub const MAX_PAGES_TO_COMPARE: usize = 30;

/// At or below this, a page is not merely the closest, it is essentially the same image.
pub const NEAR_EXACT_DISTANCE: u32 = 5;

/// When the best page is not a near-exact match, it must beat the runner-up by this much.
pub const MIN_DISTANCE_MARGIN: u32 = 4;

/// Past this, the "best" page does not resemble the local image, so the comparison is untrustworthy.
pub const MAX_PLAUSIBLE_DISTANCE: u32 = 12;

/// A flat or near-flat image hashes to almost no set bits, which lands spuriously close to
/// everything, so it is refused rather than matched by accident.
pub const MIN_INFORMATIVE_BITS: u32 = 6;

/// How far the aspect ratio of a page may sit from the local file's before it counts as a
/// different picture rather than a re-encode.
pub const ASPECT_RATIO_TOLERANCE: f64 = 0.05;

/// The outcome of [`choose_matching_page`]: the closest page and whether to act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMatch {
    pub index: usize,
    pub distance: u32,
    pub runner_up_distance: Option<u32>,
    pub confident: bool,
    pub reason: String,
    /// Set when the refusal was "these pages are the same picture as far as I can tell", as
    /// opposed to "none of them look like this at all": a tie is worth breaking with weaker
    /// evidence, a post that does not contain the image at all is not.
    pub indistinguishable: bool,
}

/// Which page best matches the local image. `page_thumbnails` is `(page_index, image_bytes)` for
/// each candidate page. `None` when there is nothing to decide between or the evidence is too weak
/// to prefer one page, in which case the caller keeps the post's first page.
pub fn choose_matching_page(
    local: Source<'_>,
    page_thumbnails: &[(usize, Vec<u8>)],
) -> Result<Option<PageMatch>, DecompressionBomb> {
    if page_thumbnails.len() < 2 {
        return Ok(None);
    }
    let Some(local_hash) = dhash(local, 8)? else {
        debug!("Could not hash the local image; leaving the page choice alone");
        return Ok(None);
    };
    if local_hash.popcount() < MIN_INFORMATIVE_BITS {
        debug!(
            "The local image is nearly featureless ({} bits of detail) - not enough to tell the \
             pages apart",
            local_hash.popcount()
        );
        return Ok(None);
    }

    let mut scored: Vec<(u32, usize)> = Vec::new();
    for (index, data) in page_thumbnails {
        let theirs: Option<Hash> = dhash(Source::Bytes(data), 8)?;
        if let Some(distance) = hamming_distance(Some(&local_hash), theirs.as_ref()) {
            scored.push((distance, *index));
        }
    }
    if scored.len() < 2 {
        return Ok(None);
    }
    scored.sort();
    let (best_distance, best_index) = scored[0];
    let runner_up_distance = scored[1].0;
    let mut m = PageMatch {
        index: best_index,
        distance: best_distance,
        runner_up_distance: Some(runner_up_distance),
        confident: false,
        reason: String::new(),
        indistinguishable: false,
    };

    if best_distance > MAX_PLAUSIBLE_DISTANCE {
        m.reason = format!(
            "closest page {best_index} still differs by {best_distance}/64 - the thumbnails may \
             not have downloaded, or this match is poor"
        );
        return Ok(Some(m));
    }

    let near_exact = best_distance <= NEAR_EXACT_DISTANCE;
    if near_exact && best_distance == runner_up_distance {
        // Two pages equally indistinguishable from the local image. Either would arguably be
        // right, but "arguably" is not a basis to pick.
        m.indistinguishable = true;
        m.reason = format!(
            "pages {best_index} and {} are both {best_distance}/64 from the local image - \
             indistinguishable, so not choosing between them",
            scored[1].1
        );
        return Ok(Some(m));
    }

    if !near_exact && runner_up_distance - best_distance < MIN_DISTANCE_MARGIN {
        m.indistinguishable = true;
        m.reason = format!(
            "pages {best_index} and {} are equally close ({best_distance} vs \
             {runner_up_distance}/64) - no basis to choose",
            scored[1].1
        );
        return Ok(Some(m));
    }

    m.confident = true;
    m.reason = format!(
        "page {best_index} matches at {best_distance}/64 (next closest {runner_up_distance})"
    );
    Ok(Some(m))
}

/// SauceNAO's thumbnail filename for a Pixiv result: the illust ID, then the page it matched.
/// Multi-image posts sit under a `/manga/` path segment and are named `89861006_p1.jpg`;
/// single-image ones are `12345678_p0_master1200.jpg`. Older entries are `12345678_s.jpg` with no
/// page at all, which this deliberately does not match: no marker, no claim.
static SAUCENAO_PIXIV_THUMB_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\d+)_p(\d+)(?:_master\d+)?\.jpg$").expect("saucenao pixiv thumb regex")
});

/// The page SauceNAO says it matched, from its own thumbnail URL. Evidence, not truth: the caller
/// bounds-checks it against the live page list and never lets it override a confident hash.
///
/// `None` for anything that is not an unambiguous SauceNAO Pixiv thumbnail, including IQDB's
/// thumbnails, which are a different host and carry no page information. `Err` where
/// `urlsplit` raises `ValueError`. A page number too large for an `i64` is `i64::MAX`, which is
/// what every caller treats a number past the page count as.
pub fn page_index_from_thumbnail(
    thumb_url: Option<&str>,
    illust_id: Option<&str>,
) -> Result<Option<i64>, UrlValueError> {
    let (Some(thumb_url), Some(illust_id)) = (
        thumb_url.filter(|s| !s.is_empty()),
        illust_id.filter(|s| !s.is_empty()),
    ) else {
        return Ok(None);
    };
    let split = pyurl::try_urlparse(thumb_url, "")?;
    let host = host::hostname(thumb_url).unwrap_or_default();
    if host != "saucenao.com" && !host.ends_with(".saucenao.com") {
        return Ok(None);
    }
    // `urlsplit`'s path keeps the `;params` that `urlparse` separates.
    let path = if split.params.is_empty() {
        split.path.clone()
    } else {
        format!("{};{}", split.path, split.params)
    };
    if !path.contains("/res/pixiv") {
        return Ok(None);
    }
    let name = path.rsplit('/').next().unwrap_or("");
    let Some(caps) = SAUCENAO_PIXIV_THUMB_RE.captures(name) else {
        return Ok(None);
    };
    if &caps[1] != illust_id {
        debug!(
            "SauceNAO thumbnail {thumb_url} names illust {} but the result is {illust_id} - \
             ignoring the page it claims",
            &caps[1]
        );
        return Ok(None);
    }
    Ok(Some(py_int(&caps[2]).unwrap_or(i64::MAX)))
}

/// A page's size as the page list gives it: `(width, height)`, either of which may be missing.
pub type PageSize = Option<(Option<i64>, Option<i64>)>;

/// The one page whose dimensions are exactly the local file's. `None` when no page matches, or when
/// several do, in which case the dimensions have identified nothing. The cheapest possible
/// confirmation: Pixiv's page list already carries every page's width and height.
pub fn dimensions_identify_page(
    local_size: Option<(i64, i64)>,
    page_sizes: &[PageSize],
) -> Option<usize> {
    let (lw, lh) = local_size?;
    let hits: Vec<usize> = page_sizes
        .iter()
        .enumerate()
        .filter(|(_, size)| size.is_some_and(|(w, h)| w == Some(lw) && h == Some(lh)))
        .map(|(i, _)| i)
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

/// Whether the page list argues AGAINST the given page. Compares aspect ratios rather than exact
/// sizes, because the local file is routinely a resized copy.
pub fn dimensions_contradict_page(
    local_size: Option<(i64, i64)>,
    page_sizes: &[PageSize],
    index: usize,
) -> bool {
    let Some((local_w, local_h)) = local_size else {
        return false;
    };
    if index >= page_sizes.len() || local_w == 0 || local_h == 0 {
        return false;
    }
    let Some((Some(w), Some(h))) = page_sizes[index] else {
        return false;
    };
    if w == 0 || h == 0 {
        return false;
    }
    let local_ratio = local_w as f64 / local_h as f64;
    (w as f64 / h as f64 - local_ratio).abs() / local_ratio > ASPECT_RATIO_TOLERANCE
}

/// The outcome of [`check_page`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageCheck {
    pub distance: Option<u32>,
    pub confirmed: bool,
    pub reason: String,
}

/// Whether one particular page IS the local image. Confirmation demands a near-exact result: there
/// is no runner-up here to be better than, so the reading has to stand on its own.
pub fn check_page(local: Source<'_>, thumbnail: &[u8]) -> Result<PageCheck, DecompressionBomb> {
    let Some(local_hash) = dhash(local, 8)? else {
        return Ok(PageCheck {
            reason: "the local image couldn't be hashed".to_owned(),
            ..PageCheck::default()
        });
    };
    if local_hash.popcount() < MIN_INFORMATIVE_BITS {
        // A near-flat image sits spuriously close to everything, so it would "confirm" any page.
        return Ok(PageCheck {
            reason: "the local image is nearly featureless".to_owned(),
            ..PageCheck::default()
        });
    }
    let theirs = dhash(Source::Bytes(thumbnail), 8)?;
    let Some(distance) = hamming_distance(Some(&local_hash), theirs.as_ref()) else {
        return Ok(PageCheck {
            reason: "that page's thumbnail couldn't be hashed".to_owned(),
            ..PageCheck::default()
        });
    };
    if distance <= NEAR_EXACT_DISTANCE {
        return Ok(PageCheck {
            distance: Some(distance),
            confirmed: true,
            reason: format!("it matches the local image at {distance}/64"),
        });
    }
    Ok(PageCheck {
        distance: Some(distance),
        confirmed: false,
        reason: format!(
            "it differs from the local image by {distance}/64, too much to call it the same \
             picture"
        ),
    })
}

/// Rewrites a Pixiv page-0 file URL to another page: `12345678_p0.jpg` is the first, `_p4` the
/// fifth. `None` when the URL does not carry that marker, rather than constructing something that
/// merely looks plausible.
pub fn page_url_for_index(first_page_url: &str, index: usize) -> Option<String> {
    if first_page_url.is_empty() || !first_page_url.contains("_p0") {
        return None;
    }
    if index == 0 {
        return Some(first_page_url.to_owned());
    }
    // Only the LAST occurrence: a directory in the path could contain "_p0" too.
    let at = first_page_url.rfind("_p0")?;
    Some(format!(
        "{}_p{index}{}",
        &first_page_url[..at],
        &first_page_url[at + 3..]
    ))
}
