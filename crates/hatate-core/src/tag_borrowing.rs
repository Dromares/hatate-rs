//! Taking tags from another match of the same picture (`core/tag_borrowing.py`).
//!
//! Some sites cannot supply tags at all (MangaDex has none, Sankaku is reachable only by file
//! hash), so a chosen match can arrive tagless while another match of the SAME picture carries
//! them. The rule that keeps this honest is similarity: a candidate may lend its tags only when it
//! is about as good a match as the chosen one, because then it IS the same picture on another
//! site. The chosen match stays chosen; only its tags come from elsewhere, and where they came
//! from is recorded.
//!
//! Python's `fetch_details` callable is an async closure here, because fetching a candidate's
//! details is a network call. Candidates are named by index, so the closure can borrow one of them
//! mutably while the slice is not otherwise in use.

use tracing::info;

use crate::config::Settings;
use crate::model::Candidate;

/// `candidates_that_may_lend`: the indices of the other matches close enough to lend their tags,
/// best first. `slack` is how far below the chosen match's similarity a candidate may be.
///
/// A match already known to be gone has nothing to lend. Python also skips a candidate whose
/// similarity is `None`; a [`Candidate`] always has a number (`0.0` when the engine gave none), so
/// there is no such case here.
pub fn candidates_that_may_lend(
    candidates: &[Candidate],
    chosen_index: usize,
    slack: f64,
) -> Vec<usize> {
    let Some(chosen) = candidates.get(chosen_index) else {
        return Vec::new();
    };
    // `max(0.0, slack)`: NaN is not greater than 0.0, so it stays 0.0, as `f64::max` does.
    let floor = chosen.similarity - 0.0f64.max(slack);
    let mut eligible: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            *i != chosen_index && c.similarity >= floor && c.remote_available != Some(false)
        })
        .map(|(i, _)| i)
        .collect();
    // `sorted(key=-similarity)` is stable, so equal scores keep the order they came in.
    eligible.sort_by(|a, b| {
        candidates[*b]
            .similarity
            .partial_cmp(&candidates[*a].similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    eligible
}

/// `borrow_tags`: fills a tagless chosen match from the best eligible alternative. Returns the URL
/// the tags came from, or `None` if nothing was borrowed. At most one alternative is fetched per
/// candidate asked, best first, and it stops at the first that has tags.
///
/// `fetch_details` is the detail-fetching callable (an error from it propagates, as an exception
/// does in Python). `filename` is only for the log line.
pub async fn borrow_tags<E>(
    filename: &str,
    candidates: &mut [Candidate],
    chosen_index: usize,
    settings: &Settings,
    mut fetch_details: impl AsyncFnMut(&mut Candidate) -> Result<(), E>,
) -> Result<Option<String>, E> {
    if !settings.borrow_tags_from_other_matches {
        return Ok(None);
    }
    let Some(chosen) = candidates.get(chosen_index) else {
        return Ok(None);
    };
    if !chosen.booru_tags.is_empty() {
        return Ok(None); // it has its own tags; nothing to do
    }
    let slack = settings.borrow_tags_similarity_slack;
    for lender_index in candidates_that_may_lend(candidates, chosen_index, slack) {
        if !candidates[lender_index].booru_tags_fetched {
            fetch_details(&mut candidates[lender_index]).await?;
        }
        if candidates[lender_index].booru_tags.is_empty() {
            continue;
        }
        let lender_tags = candidates[lender_index].booru_tags.clone();
        let lender_url = candidates[lender_index].url.clone();
        let lender_similarity = candidates[lender_index].similarity;
        let chosen = &mut candidates[chosen_index];
        info!(
            "{filename}: the chosen match has no tags - borrowed {} from {lender_url} ({:.0}% vs {:.0}%)",
            lender_tags.len(),
            lender_similarity,
            chosen.similarity,
        );
        chosen.booru_tags = lender_tags;
        chosen.tags_borrowed_from = Some(lender_url.clone());
        return Ok(Some(lender_url));
    }
    Ok(None)
}
