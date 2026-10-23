//! `core/hydrus_reconcile.py` and the logic of `workers/hydrus_reconcile_worker.py` without Qt
//! (Phase 4.d, DAN-2131): settling what "Queued" means, by asking Hydrus.
//!
//! Sending a URL to Hydrus's own downloader is asynchronous, and the inline poll
//! ([`crate::hydrus_import_poll`]) gives up after a minute. An entry it gave up on stays Queued,
//! and nothing asks again. The state is knowable, though: the file is identified by its SHA-256,
//! which every entry carries from duplicate detection, so this asks by hash rather than by URL
//! (the URL is often gone by now, the hash is not).
//!
//! [`reconcile`] takes the lookup as a callable so the decision can be tested without a Hydrus.
//! Python returns the entry objects; an owned `Entry` slice cannot also be borrowed by the result,
//! so [`ReconcileResult`] holds **indices into the slice that was given**.

use tracing::{debug, info, warn};

use crate::config::Settings;
use crate::hydrus_client::{DeletionState, DeletionStates, HydrusClient, HydrusError};
use crate::model::Entry;

/// Hydrus states that mean it resolved this file: it holds it, or held it and the user has since
/// deleted it. Both settle the question this asks, which is whether the SEND worked, not whether
/// the file is still there.
pub const RESOLVED_STATES: [DeletionState; 3] = [
    DeletionState::Present,
    DeletionState::Trashed,
    DeletionState::Deleted,
];

/// Hashes per request. Hydrus takes them in one query string, so this is about keeping that URL a
/// sane length. 256 keeps a 25,000-entry library to a hundred requests.
pub const BATCH_SIZE: usize = 256;

/// `ReconcileResult`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileResult {
    /// Hydrus has resolved these; now marked confirmed. Indices into the slice given.
    pub confirmed: Vec<usize>,
    /// Hydrus has never seen these. Indices into the slice given.
    pub unresolved: Vec<usize>,
    /// How many were asked about.
    pub checked: usize,
}

impl ReconcileResult {
    /// `ReconcileResult.summary`.
    pub fn summary(&self) -> String {
        if self.checked == 0 {
            return "Nothing was waiting on Hydrus".to_owned();
        }
        let mut parts = vec![format!(
            "confirmed {} of {}",
            self.confirmed.len(),
            self.checked
        )];
        if !self.unresolved.is_empty() {
            parts.push(format!("{} still unknown to Hydrus", self.unresolved.len()));
        }
        parts.join("; ")
    }
}

/// `entries_awaiting_confirmation`: entries sent to Hydrus that it has never acknowledged
/// holding. A hash is required: it is the only handle on the file, and an entry without one
/// cannot be asked about, so it is left alone rather than guessed at. Indices, in order.
pub fn entries_awaiting_confirmation(entries: &[Entry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.sent_to_hydrus
                && !e.hydrus_import_confirmed
                && e.hydrus_hash.as_deref().is_some_and(|h| !h.is_empty())
        })
        .map(|(i, _)| i)
        .collect()
}

/// `reconcile`: asks Hydrus about every entry still waiting, and confirms those it has resolved.
///
/// `states_for` maps hashes to Hydrus's reading of them, normally
/// [`HydrusClient::deletion_states`]. Only ever turns `hydrus_import_confirmed` ON: a hash Hydrus
/// does not know may simply not have finished downloading, and un-confirming something already
/// confirmed would lose information that was correct when it was recorded.
///
/// An error from `states_for` ends the call (Python lets it propagate). `should_stop` is asked
/// before every batch; `on_progress(hashes asked about, total hashes)` after every one.
pub async fn reconcile(
    entries: &mut [Entry],
    mut states_for: impl AsyncFnMut(&[String]) -> Result<DeletionStates, HydrusError>,
    batch_size: std::num::NonZeroUsize,
    should_stop: Option<&dyn Fn() -> bool>,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> Result<ReconcileResult, HydrusError> {
    let waiting = entries_awaiting_confirmation(entries);
    if waiting.is_empty() {
        return Ok(ReconcileResult::default());
    }

    // Two rows can share a hash (the same file added twice), and both deserve the answer.
    let mut hashes: Vec<String> = Vec::new();
    let mut by_hash: std::collections::HashMap<String, Vec<usize>> = Default::default();
    for &i in &waiting {
        let Some(hash) = entries[i].hydrus_hash.clone().filter(|h| !h.is_empty()) else {
            continue;
        };
        by_hash
            .entry(hash.clone())
            .or_insert_with(|| {
                hashes.push(hash);
                Vec::new()
            })
            .push(i);
    }

    let mut result = ReconcileResult::default();
    let batch = batch_size.get();
    let mut start = 0;
    while start < hashes.len() {
        if should_stop.is_some_and(|stop| stop()) {
            info!(
                "Reconcile stopped after {} of {} hash(es)",
                result.checked,
                hashes.len()
            );
            break;
        }
        let chunk = &hashes[start..(start + batch).min(hashes.len())];
        let states = states_for(chunk).await?;
        for hash in chunk {
            let state = states.get(hash).unwrap_or(DeletionState::Unknown);
            for &i in &by_hash[hash] {
                result.checked += 1;
                if RESOLVED_STATES.contains(&state) {
                    entries[i].hydrus_import_confirmed = true;
                    result.confirmed.push(i);
                } else {
                    result.unresolved.push(i);
                }
            }
        }
        if let Some(progress) = on_progress {
            progress((start + batch).min(hashes.len()), hashes.len());
        }
        start += batch;
    }

    info!(
        "Reconciled {} entr{} against Hydrus: {} confirmed, {} still unknown",
        result.checked,
        if result.checked == 1 { "y" } else { "ies" },
        result.confirmed.len(),
        result.unresolved.len()
    );
    Ok(result)
}

/// `HydrusReconcileWorker.run` without Qt: reconciles the entries still waiting on Hydrus against
/// the client built from the settings. Never fails: it runs unprompted at startup, and a Hydrus
/// that is offline or answering oddly is an ordinary condition, not a fault worth a crash. Any
/// failure is a warning and an empty result.
pub async fn run_reconcile(
    entries: &mut [Entry],
    settings: &Settings,
    should_stop: Option<&dyn Fn() -> bool>,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> ReconcileResult {
    let waiting = entries_awaiting_confirmation(entries);
    if waiting.is_empty() {
        debug!("Nothing is waiting on Hydrus; skipping reconcile");
        return ReconcileResult::default();
    }
    info!(
        "Reconciling {} entr{} still waiting on Hydrus",
        waiting.len(),
        if waiting.len() == 1 { "y" } else { "ies" }
    );
    let client = match HydrusClient::new(&settings.hydrus) {
        Ok(client) => client,
        Err(e) => {
            warn!("Could not reconcile with Hydrus: {e}");
            return ReconcileResult::default();
        }
    };
    run_reconcile_with(entries, &client, should_stop, on_progress).await
}

/// [`run_reconcile`] with the client already built (a test builds it against a local server).
pub async fn run_reconcile_with(
    entries: &mut [Entry],
    client: &HydrusClient,
    should_stop: Option<&dyn Fn() -> bool>,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> ReconcileResult {
    match reconcile(
        entries,
        async |hashes: &[String]| client.deletion_states(hashes).await,
        std::num::NonZeroUsize::new(BATCH_SIZE).expect("batch size"),
        should_stop,
        on_progress,
    )
    .await
    {
        Ok(result) => result,
        Err(e) => {
            warn!("Could not reconcile with Hydrus: {e}");
            ReconcileResult::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, hash: Option<&str>, sent: bool, confirmed: bool) -> Entry {
        let mut e = Entry::new(path);
        e.hydrus_hash = hash.map(str::to_owned);
        e.sent_to_hydrus = sent;
        e.hydrus_import_confirmed = confirmed;
        e
    }

    #[test]
    fn the_summary_reads_like_python_s() {
        let r = |confirmed: usize, unresolved: usize, checked: usize| ReconcileResult {
            confirmed: (0..confirmed).collect(),
            unresolved: (0..unresolved).collect(),
            checked,
        };
        assert_eq!(r(0, 0, 0).summary(), "Nothing was waiting on Hydrus");
        assert_eq!(r(3, 0, 3).summary(), "confirmed 3 of 3");
        assert_eq!(
            r(2, 1, 3).summary(),
            "confirmed 2 of 3; 1 still unknown to Hydrus"
        );
        // Python tests `not self.checked`, not the lists.
        assert_eq!(r(1, 0, 0).summary(), "Nothing was waiting on Hydrus");
    }

    #[test]
    fn only_sent_unconfirmed_entries_with_a_hash_are_waiting() {
        let entries = vec![
            entry("/a", Some("h1"), true, false),
            entry("/b", Some("h2"), true, true),
            entry("/c", Some("h3"), false, false),
            entry("/d", None, true, false),
            entry("/e", Some(""), true, false),
            entry("/f", Some("h6"), true, false),
        ];
        assert_eq!(entries_awaiting_confirmation(&entries), vec![0, 5]);
    }
}
