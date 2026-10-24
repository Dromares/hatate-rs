//! Fetching the actual pictures an MCP tool call hands back (`core/mcp_images.py`).
//!
//! `get_images` and `get_diff` must return the real images at FULL resolution: the match as it is,
//! not the downscaled sample the review screen shows. This is the policy of the compare dialog's
//! `load_comparison()` without the Qt types, so it runs on the tool's own thread.

use std::path::Path;
use std::sync::Mutex;

use image::ExtendedColorType;
use image::ImageEncoder;
use image::codecs::png::PngEncoder;
use tracing::{info, warn};

use crate::config::Settings;
use crate::image_compare::Source;
use crate::image_diff::{self, DiffError};
use crate::model::Candidate;
use crate::pyimage;
use crate::remote::{self, Raised};
use crate::search_engine::SearchEngine;

/// `FullResolutionMatch`: the result of trying to fetch a candidate's real picture.
#[derive(Debug, Clone, PartialEq)]
pub struct FullResolutionMatch {
    pub data: Option<Vec<u8>>,
    pub url: Option<String>,
    /// True iff `data` came from `direct_file_url`, not a fallback.
    pub is_full_resolution: bool,
    pub fallback_reason: Option<String>,
}

/// `local_image_bytes`: the local file's own bytes, read fresh. `None` when it cannot be read.
pub fn local_image_bytes(path: &str) -> Option<Vec<u8>> {
    match std::fs::read(Path::new(path)) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            warn!("Could not read local file {path} for an MCP image tool: {e}");
            None
        }
    }
}

/// `fetch_full_resolution_match`: the candidate's picture at full resolution.
///
/// Falls back to the preview or thumbnail only when no direct file URL is available at all, and
/// says so in the result rather than silently handing back a sample. The candidate is filled in
/// as `fetch_candidate_details` fills it, in place, so the caller can keep what was learned.
/// `Err` is an exception Python leaves uncaught out of the download.
pub async fn fetch_full_resolution_match(
    candidate: &mut Candidate,
    engine: &SearchEngine,
    settings: &Mutex<Settings>,
    local_path: Option<&str>,
) -> Result<FullResolutionMatch, Raised> {
    let empty = |s: &Option<String>| s.as_deref().is_none_or(str::is_empty);
    if empty(&candidate.direct_file_url) && empty(&candidate.preview_url) {
        info!(
            "Fetching match details before returning its image: {}",
            candidate.url
        );
        if let Err(e) = engine
            .fetch_candidate_details(candidate, settings, None, local_path)
            .await
        {
            warn!("Could not fetch match details for {}: {e}", candidate.url);
        }
    }

    let mut url = candidate.direct_file_url.clone().filter(|u| !u.is_empty());
    let mut is_full_resolution = true;
    let mut fallback_reason = None;
    if url.is_none() {
        url = candidate
            .preview_url
            .clone()
            .filter(|u| !u.is_empty())
            .or_else(|| candidate.thumb_url.clone().filter(|u| !u.is_empty()));
        is_full_resolution = false;
        fallback_reason = Some(
            "No direct full-resolution file URL was available; returned the preview/thumbnail instead."
                .to_owned(),
        );
    }
    let Some(url) = url else {
        return Ok(FullResolutionMatch {
            data: None,
            url: None,
            is_full_resolution: false,
            fallback_reason: Some("This match has no image URL at all.".to_owned()),
        });
    };

    let timeout = settings.lock().expect("settings lock").hydrus.timeout;
    let referer = (Some(url.as_str()) == candidate.direct_file_url.as_deref())
        .then(|| candidate.url.clone());
    let data = remote::download_bytes(
        &engine.net,
        Some(&url),
        timeout,
        referer.as_deref(),
        None,
        None,
    )
    .await?;
    Ok(match data {
        None => FullResolutionMatch {
            data: None,
            url: Some(url),
            is_full_resolution: false,
            fallback_reason: Some("The fetch failed - see the app log.".to_owned()),
        },
        Some(bytes) => FullResolutionMatch {
            data: Some(bytes),
            url: Some(url),
            is_full_resolution,
            fallback_reason,
        },
    })
}

/// `DiffResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffResult {
    pub png_bytes: Option<Vec<u8>>,
    pub changed_percent: f64,
    pub verdict: String,
    pub reliable: bool,
}

/// `compute_diff`: the structural difference map as PNG bytes ready to hand back as an image
/// content block. `Err` is the exception Python's `difference_map` raises while loading a picture
/// whose header opened but whose pixels did not.
pub fn compute_diff(
    local_bytes: Option<&[u8]>,
    match_bytes: Option<&[u8]>,
) -> Result<DiffResult, DiffError> {
    let (Some(local), Some(matched)) = (decodes(local_bytes), decodes(match_bytes)) else {
        return Ok(DiffResult {
            png_bytes: None,
            changed_percent: 0.0,
            verdict: "Could not decode one or both images for comparison.".to_owned(),
            reliable: false,
        });
    };
    let diff = image_diff::difference_map(Source::Bytes(local), Source::Bytes(matched))?;
    let png_bytes = diff.mask.as_deref().map(|mask| {
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(mask, diff.width as u32, diff.height as u32, ExtendedColorType::L8)
            .expect("a grayscale mask encodes");
        out
    });
    Ok(DiffResult {
        png_bytes,
        changed_percent: diff.changed_percent(),
        verdict: diff.verdict,
        reliable: diff.reliable,
    })
}

/// `_decode`: `Image.open` on the bytes. Only the header is read here; a picture that opens but
/// cannot be loaded fails later, inside the difference map.
fn decodes(data: Option<&[u8]>) -> Option<&[u8]> {
    let data = data.filter(|d| !d.is_empty())?;
    match pyimage::image_size_checked(data) {
        Ok(Some(_)) => Some(data),
        Ok(None) => {
            warn!("Could not decode image data for an MCP image tool: cannot identify image file");
            None
        }
        Err(bomb) => {
            warn!("Could not decode image data for an MCP image tool: {bomb}");
            None
        }
    }
}
