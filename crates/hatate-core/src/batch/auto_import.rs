//! Hydrus auto-import in the batch loop (Phase 4.g, DAN-2132): `SearchWorker._maybe_auto_import`
//! and `_switch_to_importable_match` of `workers/search_worker.py`, and the headless counterpart
//! of the window's `_on_auto_imported` (dropping an imported row when `remove_after_import` is
//! on).
//!
//! **Errors.** Python's `_maybe_auto_import` is called outside the loop's `try`, so whatever
//! escapes it ends the worker thread: the rest of the queue is never searched. Here that is a
//! [`RunFault`], and the batch ends with [`super::Ending::Failed`] (the list and the session are
//! kept as they stand). What the import functions catch themselves (a `HydrusError`) is on the
//! returned [`ImportResult`].

use std::path::Path;

use tracing::{error, info, warn};

use super::run::BatchEnv;
use super::staging::StagingError;
use crate::config::Settings;
use crate::engine_ids::reports_real_similarity;
use crate::error::ErrorKind;
use crate::hydrus_client::HydrusClient;
use crate::hydrus_import::{
    ImportResult, UrlImportError, download_and_send, download_instead, finish_url_import,
    send_file_upload, send_url_to_importer, url_import_refusal,
};
use crate::hydrus_import_poll::{PollOptions, poll_single_url_import};
use crate::hydrus_url::normalize_url_for_hydrus;
use crate::model::{Entry, MatchStatus};

/// `MAX_IMPORT_ALTERNATIVES`: how many other matches to ask Hydrus about once the chosen one is
/// refused.
pub const MAX_IMPORT_ALTERNATIVES: usize = 5;

/// Why a run ended on an exception.
#[derive(Debug, Clone)]
pub enum RunFault {
    /// The staging directory, or the sweep of the old ones.
    Staging(StagingError),
    /// A Hydrus or URL function raised something its callers do not catch.
    Import(UrlImportError),
    /// The Hydrus client could not be built (Python's constructor cannot fail).
    Client(String),
    /// `assert entry.matched_url is not None` failed.
    Assertion,
}

impl RunFault {
    pub fn kind(&self) -> ErrorKind {
        match self {
            RunFault::Staging(e) => e.kind(),
            RunFault::Import(e) => e.kind(),
            RunFault::Client(_) => ErrorKind::Io,
            RunFault::Assertion => ErrorKind::InvalidInput,
        }
    }

    /// Python's `str(exc)`.
    pub fn message(&self) -> String {
        match self {
            RunFault::Staging(e) => e.message(),
            RunFault::Import(e) => e.message(),
            RunFault::Client(text) => text.clone(),
            RunFault::Assertion => "AssertionError".to_owned(),
        }
    }

    /// What to tell a person: what failed and what to do; no path, URL query, key or OS text.
    pub fn user_message(&self) -> String {
        match self {
            RunFault::Staging(e) => e.user_message(),
            RunFault::Import(e) => e.user_message(),
            RunFault::Client(_) => {
                "Hatate could not set up its connection to Hydrus. Check the Hydrus address in \
                 the settings."
                    .to_owned()
            }
            RunFault::Assertion => {
                "Hatate lost track of the match it was importing and stopped the run.".to_owned()
            }
        }
    }

    /// The raw text for a log.
    pub fn detail(&self) -> String {
        match self {
            RunFault::Staging(e) => e.detail(),
            RunFault::Import(e) => e.detail(),
            RunFault::Client(text) => text.clone(),
            RunFault::Assertion => "AssertionError".to_owned(),
        }
    }
}

impl PartialEq for RunFault {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && self.message() == other.message()
    }
}
impl Eq for RunFault {}

impl std::fmt::Display for RunFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for RunFault {}

impl From<StagingError> for RunFault {
    fn from(e: StagingError) -> Self {
        RunFault::Staging(e)
    }
}
impl From<UrlImportError> for RunFault {
    fn from(e: UrlImportError) -> Self {
        RunFault::Import(e)
    }
}
impl From<crate::hydrus_client::HydrusError> for RunFault {
    fn from(e: crate::hydrus_client::HydrusError) -> Self {
        RunFault::Import(e.into())
    }
}
impl From<crate::hydrus_url::UrlValueError> for RunFault {
    fn from(e: crate::hydrus_url::UrlValueError) -> Self {
        RunFault::Import(e.into())
    }
}

/// `_maybe_auto_import`: sends a just-found match straight to Hydrus if it is confident enough.
/// `Ok(None)` when nothing was attempted (the early returns); otherwise the result, which the
/// caller hands to [`removes_row`] and the reporter.
pub async fn maybe_auto_import(
    env: &BatchEnv<'_>,
    entry: &mut Entry,
    client: &HydrusClient,
    tmp_dir: Option<&Path>,
) -> Result<Option<ImportResult>, RunFault> {
    if entry.sent_to_hydrus {
        // Already handled (e.g. served from cache after an earlier manual send).
        return Ok(None);
    }
    if !matches!(entry.status, MatchStatus::Good | MatchStatus::Poor) {
        return Ok(None);
    }
    let settings = env.settings.lock().unwrap().clone();
    let Some(similarity) = entry.similarity() else {
        return Ok(None);
    };
    if similarity < settings.auto_import_min_similarity {
        return Ok(None);
    }

    if let Some(selected) = entry.selected_candidate() {
        if selected.remote_available == Some(false) {
            // This match's source is confirmed gone, so there is nothing for Hydrus to fetch.
            // Skipping is especially worth it for the url_importer method, which would otherwise
            // burn the whole confirmation timeout waiting on a download that cannot succeed.
            info!(
                "Skipping auto-import for {} - its matched source is gone ({})",
                entry.filename(),
                selected.url
            );
            return Ok(None);
        }
    }

    let method = settings.auto_import_method.as_str();
    info!(
        "Auto-importing {} ({similarity:.0}% similarity, method={method})",
        entry.filename()
    );
    let clock: &dyn crate::clock::Clock = &*env.engine.clock;
    let net = &env.engine.net;
    let mut result;
    if method == "url_importer" {
        result = send_url_to_importer(entry, client, net, Some(&settings)).await?;
        if result.refused && switch_to_importable_match(entry, client, &settings).await? {
            result = send_url_to_importer(entry, client, net, Some(&settings)).await?;
        }
        if result.refused {
            if let Some(tmp_dir) = tmp_dir {
                // No match Hydrus can take - Hatate fetches the file itself where the site lets
                // it (see `download_instead`).
                result = download_instead(
                    entry,
                    client,
                    env.engine,
                    Some(env.settings),
                    tmp_dir,
                    &result,
                )
                .await?;
                if let Some(note) = result.note.as_deref().filter(|n| !n.is_empty()) {
                    info!("Auto-import: {note} for {}", entry.filename());
                }
            }
        }
        let needs_confirmation = settings.remove_after_import
            || settings.set_hydrus_duplicate_relationships
            || settings.write_hydrus_provenance_note;
        if result.success && result.confirmed.is_none() && needs_confirmation {
            // Don't just trust that Hydrus *accepted* the request - poll it to confirm the file
            // was imported before this can be removed from the list. If none of the three things
            // that need a confirmed hash is switched on, skip this and save the API calls.
            info!(
                "Auto-import (url_importer): confirming {} with Hydrus before it can be removed \
                 from the list…",
                entry.filename()
            );
            let Some(matched) = entry.matched_url().map(str::to_owned) else {
                // `assert entry.matched_url is not None`
                return Err(RunFault::Assertion);
            };
            let url = normalize_url_for_hydrus(&matched)?;
            let stop = || env.stop.load(std::sync::atomic::Ordering::SeqCst);
            let progress = |remaining: f64, total: f64| {
                env.reporter
                    .countdown("Confirming Hydrus import", remaining, total);
            };
            let confirmed_hash = poll_single_url_import(
                client,
                &url,
                &PollOptions {
                    timeout: settings.url_import_confirm_timeout,
                    interval: settings.url_import_confirm_interval,
                    stop_check: Some(&stop),
                    on_progress: Some(&progress),
                },
                clock,
            )
            .await?;
            if let Some(confirmed_hash) = confirmed_hash.filter(|h| !h.is_empty()) {
                // Read BEFORE the assignment below overwrites it: this is the only hash that
                // still identifies the copy the user already had.
                let local_hash = entry.hydrus_hash.clone();
                entry.hydrus_hash = Some(confirmed_hash.clone());
                result.confirmed = Some(true);
                info!(
                    "Confirmed Hydrus import for {} (hash={confirmed_hash})",
                    entry.filename()
                );
                result.warning = finish_url_import(
                    entry,
                    client,
                    Some(&settings),
                    local_hash.as_deref(),
                    &confirmed_hash,
                    clock,
                )
                .await?;
                if let Some(warning) = result.warning.as_deref().filter(|w| !w.is_empty()) {
                    entry.error_message = Some(warning.to_owned());
                    warn!("Auto-import: {warning} for {}", entry.filename());
                }
            } else {
                result.confirmed = Some(false);
                warn!(
                    "Could not confirm Hydrus import for {} within the poll window - leaving it \
                     in the list (it may still be downloading)",
                    entry.filename()
                );
            }
            env.reporter.countdown("", 0.0, 0.0);
        }
    } else if method == "download_send" {
        let Some(tmp_dir) = tmp_dir else {
            error!(
                "Auto-import (download_send): no staging directory available for {}",
                entry.filename()
            );
            return Ok(None);
        };
        result = download_and_send(
            entry,
            client,
            net,
            settings.search_timeout,
            tmp_dir,
            Some(&settings),
            clock,
        )
        .await?;
    } else {
        result = send_file_upload(entry, client, Some(&settings), clock).await?;
    }

    if result.success && result.confirmed == Some(false) {
        warn!(
            "Auto-import sent {} to Hydrus, but Hydrus has not confirmed importing it - check \
             Hydrus's downloader page",
            entry.filename()
        );
    } else if result.success {
        info!("Auto-import succeeded for {}", entry.filename());
    } else if let Some(reason) = result.skipped_reason.as_deref().filter(|r| !r.is_empty()) {
        tracing::debug!("Auto-import skipped for {}: {reason}", entry.filename());
    } else {
        warn!(
            "Auto-import failed for {}: {}",
            entry.filename(),
            result.error.as_deref().unwrap_or("None")
        );
    }
    Ok(Some(result))
}

/// `_switch_to_importable_match`: selects the best OTHER match Hydrus's URL importer can take.
///
/// The chosen match is often a comic-aggregator page Hydrus cannot read, sitting at the same
/// measured 100% as a booru post it can. Only matches that clear the auto-import threshold on a
/// MEASURED score are considered, so falling back never lowers the bar.
pub async fn switch_to_importable_match(
    entry: &mut Entry,
    client: &HydrusClient,
    settings: &Settings,
) -> Result<bool, RunFault> {
    let threshold = settings.auto_import_min_similarity;
    let mut tried = 0usize;
    for index in 0..entry.candidates.len() {
        let candidate = &entry.candidates[index];
        if index == entry.selected_candidate_index || tried >= MAX_IMPORT_ALTERNATIVES {
            continue;
        }
        if candidate.remote_available == Some(false) || candidate.similarity < threshold {
            continue;
        }
        if !(candidate.similarity_measured || reports_real_similarity(Some(&candidate.engine))) {
            continue;
        }
        if !(candidate.url.starts_with("http://") || candidate.url.starts_with("https://")) {
            continue;
        }
        tried += 1;
        let url = normalize_url_for_hydrus(&candidate.url)?;
        if url_import_refusal(client, &url)
            .await?
            .is_some_and(|why| !why.is_empty())
        {
            continue;
        }
        info!(
            "Auto-import: Hydrus can't take {}'s top match, using {} ({:.0}%) instead",
            entry.filename(),
            candidate.url,
            candidate.similarity
        );
        entry.select_candidate(index);
        entry.error_message = None;
        return Ok(true);
    }
    Ok(false)
}

/// `_on_auto_imported`, the part that changes the list: a row that went to Hydrus is dropped when
/// `remove_after_import` is on, unless Hydrus never confirmed the import (the file may still be
/// downloading).
pub fn removes_row(settings: &Settings, result: &ImportResult) -> bool {
    result.success && result.confirmed != Some(false) && settings.remove_after_import
}
