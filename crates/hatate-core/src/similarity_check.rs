//! `core/similarity_check.py`: turning the scores engines make up into measured ones, from the
//! thumbnails they return.
//!
//! ascii2d, Google Images and Google Lens report no similarity at all, so they number their
//! results by position: 80%, 78%, 76%. That number looks exactly like IQDB's and SauceNAO's
//! measured ones in the same column, and it means nothing. [`measure_ordinal_similarities`]
//! compares each candidate's own thumbnail with the local file instead
//! ([`crate::image_compare::aligned_similarity`]); [`recheck_borrowed_measurement`] does the same
//! against a post's own image when the thumbnail an engine paired with it turned out not to be
//! the post's.
//!
//! **The cut.** All of `similarity_check.py` is here. Python's `ThreadPoolExecutor` of five
//! workers is five downloads in flight at once on the caller's task (`run_bounded`); the hashing
//! of each answer runs where it lands. `remote.download_bytes` is called as Python calls it, with
//! `settings.search_timeout` and the candidate's referer rule.

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use tracing::info;

use crate::availability::run_bounded;
use crate::boorus::BooruPageInfo;
use crate::config::Settings;
use crate::engine_ids::reports_real_similarity;
use crate::image_compare::{
    Source, aligned_similarity, image_dimensions, local_prints, local_prints_or_reason,
};
use crate::image_prep::basename;
use crate::model::{Candidate, Entry};
use crate::net::NetClient;
use crate::pyimage::DecompressionBomb;
use crate::pyraster;
use crate::remote::{self, Raised, referer_for_candidate};

/// A candidate's own thumbnail is enough to compare against: dhash works on a downscaled grayscale
/// grid, so a search engine's preview carries all the signal the hash uses. Bounded so a long
/// candidate list cannot turn into a long queue of downloads.
pub const SIMILARITY_THUMB_WORKERS: usize = 5;

/// A picture has to carry enough detail to be worth hashing. MEASURED on the same Google Lens
/// tile, both ways: with SafeSearch set to blur its thumbnail is 0.04 bytes per pixel and hashes an
/// IDENTICAL image to 78%; with SafeSearch off the same tile is 0.27 and hashes to 100%. A blurred
/// photo lands around 0.07, a real one at 0.19 and up, so the threshold sits between them.
pub const MIN_THUMB_BYTES_PER_PIXEL: f64 = 0.10;
pub const MIN_THUMB_PIXELS: usize = 64 * 64;

/// How far apart two width:height ratios may be and still be one picture. Generous, because a
/// re-upload is often trimmed a little: the case this exists for is a wide picture scored as a
/// tall one (1.41 against 0.71).
pub const ASPECT_TOLERANCE: f64 = 0.10;

/// What leaves these functions as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimilarityError {
    Raised(Raised),
    /// `Image.DecompressionBombError`.
    Bomb(DecompressionBomb),
}

impl fmt::Display for SimilarityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimilarityError::Raised(r) => write!(f, "{r}"),
            SimilarityError::Bomb(b) => write!(f, "{b}"),
        }
    }
}

impl std::error::Error for SimilarityError {}

impl From<Raised> for SimilarityError {
    fn from(r: Raised) -> Self {
        SimilarityError::Raised(r)
    }
}

impl From<DecompressionBomb> for SimilarityError {
    fn from(b: DecompressionBomb) -> Self {
        SimilarityError::Bomb(b)
    }
}

/// `_worth_hashing`: whether this image carries enough detail to compare against. A blurred
/// placeholder is still a valid image and still hashes, to something meaningless; refusing it
/// leaves the candidate with its ordinal score, which is poor information but honest.
pub fn worth_hashing(data: &[u8]) -> Result<bool, DecompressionBomb> {
    let Some((w, h)) = image_dimensions(Source::Bytes(data))? else {
        return Ok(false);
    };
    let pixels = w * h;
    if pixels < MIN_THUMB_PIXELS {
        return Ok(false);
    }
    Ok(data.len() as f64 / pixels as f64 >= MIN_THUMB_BYTES_PER_PIXEL)
}

/// `_recheck_borrowed_measurement`: gives a candidate a real, locally measured score from its own
/// page image. Two unrelated cases share the same download-and-compare tail: a score taken from a
/// thumbnail that was a picture seen ON the page and not the page's own (CONFIRMED on a real
/// image: a 4093x2894 zerochan post scored 100% from a 1105x1565 related-post thumbnail), and an
/// ordinal engine that left a candidate with no measurement at all because its thumbnail was
/// unusable. Either way, if the post's own image cannot be had, the candidate is marked
/// unmeasured, since what would be reported was never its own picture.
pub async fn recheck_borrowed_measurement(
    net: &NetClient,
    candidate: &mut Candidate,
    page_info: &BooruPageInfo,
    settings: &Settings,
    local_path: &str,
) -> Result<(), SimilarityError> {
    let reason: String = if candidate.similarity_measured {
        let (Some(pw), Some(ph)) = (
            page_info.width.filter(|&v| v != 0),
            page_info.height.filter(|&v| v != 0),
        ) else {
            return Ok(());
        };
        let Ok(data) = std::fs::read(local_path) else {
            return Ok(());
        };
        let Some((local_w, local_h)) = pyraster::pillow_size(&data)? else {
            return Ok(());
        };
        if local_w == 0 || local_h == 0 {
            return Ok(());
        }
        let local_ratio = f64::from(local_w) / f64::from(local_h);
        let post_ratio = pw as f64 / ph as f64;
        if (post_ratio - local_ratio).abs() / local_ratio.max(post_ratio) <= ASPECT_TOLERANCE {
            return Ok(());
        }
        format!(
            "a thumbnail that isn't its own (it is {pw}x{ph}, the local file {local_w}x{local_h})"
        )
    } else if reports_real_similarity(Some(&candidate.engine)) {
        return Ok(());
    } else {
        "a made-up ordinal position, never actually measured".to_owned()
    };

    let before = candidate.similarity;
    let source = page_info
        .preview_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(page_info.file_url.as_deref().filter(|s| !s.is_empty()));
    let mut remeasured = None;
    if let Some(source) = source {
        let referer = referer_for_candidate(candidate, Some(source));
        let data = remote::download_bytes(
            net,
            Some(source),
            settings.search_timeout,
            referer.as_deref(),
            None,
            None,
        )
        .await?
        .filter(|d| !d.is_empty());
        if let Some(data) = data {
            let prints = local_prints(Source::Path(Path::new(local_path)));
            remeasured = aligned_similarity(prints.as_ref(), Source::Bytes(&data)).0;
        }
    }
    let Some(remeasured) = remeasured else {
        candidate.similarity_measured = false;
        info!(
            "{}: scored {before:.0}% from {reason} - could not measure its own image, marked \
             unmeasured",
            candidate.url
        );
        return Ok(());
    };
    candidate.similarity = remeasured;
    candidate.similarity_measured = true;
    info!(
        "{}: scored {before:.0}% from {reason} - {remeasured:.0}% against its own image",
        candidate.url
    );
    Ok(())
}

/// What one candidate's measurement produced.
type Measured = (usize, Result<Option<f64>, SimilarityError>);

/// `measure_ordinal_similarities`: replaces made-up similarity scores with measured ones. A
/// perceptual hash is resolution- and re-encoding-independent, so a candidate's own preview is
/// enough to compare against without downloading originals. Candidates whose thumbnail cannot be
/// fetched or hashed keep their ordinal score, still flagged as unmeasured.
///
/// On an `Err` the other candidates have still been measured, as the worker pool finishes its
/// queue before Python re-raises the first failure in candidate order.
pub async fn measure_ordinal_similarities(
    net: &NetClient,
    entry: &Entry,
    candidates: &mut [Candidate],
    settings: &Settings,
) -> Result<(), SimilarityError> {
    let filename = basename(Path::new(&entry.path));
    // Why a candidate was skipped is worth a line each: silently dropping it leaves no way to
    // tell, after a run, whether a result kept its ordinal score because measuring said so or
    // because measuring never happened.
    let mut pending: Vec<usize> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if c.similarity_measured || reports_real_similarity(Some(&c.engine)) {
            continue;
        }
        if c.thumb_bytes.as_deref().is_some_and(|b| !b.is_empty())
            || c.thumb_url.as_deref().is_some_and(|u| !u.is_empty())
        {
            pending.push(i);
        } else {
            info!(
                "{filename}: not measuring {} result {} - it arrived with no thumbnail",
                c.engine, c.url
            );
        }
    }
    if pending.is_empty() {
        return Ok(());
    }

    // Hashed once, in every shape a thumbnail is compared against.
    let prints = match local_prints_or_reason(Source::Path(Path::new(&entry.path))) {
        Ok(p) => p,
        Err(why) => {
            info!(
                "{filename}: could not hash the local file ({why}) - leaving ordinal scores alone \
                 for all {} candidate(s)",
                pending.len()
            );
            return Ok(());
        }
    };

    let snapshot: &[Candidate] = candidates;
    let jobs: Vec<Pin<Box<dyn Future<Output = Measured> + '_>>> = pending
        .iter()
        .map(|&i| {
            let c = &snapshot[i];
            let (prints, filename) = (&prints, &filename);
            let job: Pin<Box<dyn Future<Output = Measured> + '_>> =
                Box::pin(async move { (i, measure_one(net, c, prints, settings, filename).await) });
            job
        })
        .collect();
    let workers = pending.len().min(SIMILARITY_THUMB_WORKERS);
    let mut results = run_bounded(jobs, workers).await;
    results.sort_by_key(|(i, _)| *i);

    let mut first_error: Option<SimilarityError> = None;
    for (i, outcome) in results {
        match outcome {
            Ok(Some(score)) => {
                candidates[i].similarity = score;
                candidates[i].similarity_measured = true;
            }
            Ok(None) => {}
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    let measured = pending
        .iter()
        .filter(|&&i| candidates[i].similarity_measured)
        .count();
    if let Some(e) = first_error {
        return Err(e);
    }
    if measured > 0 {
        info!(
            "{filename}: measured {measured}/{} ordinal score(s) against the local image",
            pending.len()
        );
    }
    Ok(())
}

/// `measure`: one candidate's thumbnail, compared. `Ok(None)` is a candidate left as it was.
async fn measure_one(
    net: &NetClient,
    candidate: &Candidate,
    prints: &crate::image_compare::LocalPrints,
    settings: &Settings,
    filename: &str,
) -> Result<Option<f64>, SimilarityError> {
    let mut thumb: Option<Vec<u8>> = candidate.thumb_bytes.clone();
    if thumb.is_none()
        && let Some(url) = candidate.thumb_url.as_deref().filter(|u| !u.is_empty())
    {
        let referer = referer_for_candidate(candidate, Some(url));
        thumb = remote::download_bytes(
            net,
            Some(url),
            settings.search_timeout,
            referer.as_deref(),
            None,
            None,
        )
        .await?;
    }
    let thumb_label = candidate
        .thumb_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .unwrap_or("(supplied with the result)");
    let Some(thumb) = thumb.filter(|t| !t.is_empty()) else {
        info!(
            "{filename}: not measuring {} result {} - its thumbnail {thumb_label} came back empty",
            candidate.engine, candidate.url
        );
        return Ok(None);
    };
    if !worth_hashing(&thumb)? {
        info!(
            "{filename}: not measuring {} result {} - its thumbnail {thumb_label} carries too \
             little detail to hash ({} bytes)",
            candidate.engine,
            candidate.url,
            thumb.len()
        );
        return Ok(None);
    }
    let (measured, mirrored) = aligned_similarity(Some(prints), Source::Bytes(&thumb));
    let Some(measured) = measured else {
        return Ok(None);
    };
    if mirrored {
        info!(
            "{} is the local image mirrored - scored {measured:.0}%, kept below auto-import",
            candidate.url
        );
    }
    Ok(Some(measured))
}
