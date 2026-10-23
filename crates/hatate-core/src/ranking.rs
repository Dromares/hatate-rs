//! Ranking matches so the most useful one is recommended first (`core/ranking.py`).
//!
//! Similarity stays dominant: the quality bonus is capped well below the gaps that separate a real
//! match from a near-miss. Ranking is free: the score uses only what is known when candidates are
//! merged (similarity, whether a site parser exists for the URL, dimensions if the engine gave
//! them, and the availability sweep), never anything that needs a fetch.

use tracing::info;

use crate::boorus::find_parser;
use crate::model::Candidate;

/// `MAX_QUALITY_BONUS`: ceiling on everything quality can contribute.
pub const MAX_QUALITY_BONUS: f64 = 3.0;

/// `PARSER_BONUS`: a site with a parser can supply tags, a file URL and real dimensions.
pub const PARSER_BONUS: f64 = 1.5;

/// `MAX_RESOLUTION_BONUS`: resolution advantage over the local file, on a log scale.
pub const MAX_RESOLUTION_BONUS: f64 = 1.5;
/// `RESOLUTION_REFERENCE_RATIO`: 2x earns half, 4x earns all of it.
pub const RESOLUTION_REFERENCE_RATIO: f64 = 4.0;

/// `UNAVAILABLE_PENALTY`: confirmed-dead matches sink below everything else.
pub const UNAVAILABLE_PENALTY: f64 = 1000.0;

/// `_resolution_bonus`: reward for a match being larger than the local file. Zero (not a penalty)
/// when either size is unknown or not positive.
fn resolution_bonus(local_w: i64, local_h: i64, remote_w: i64, remote_h: i64) -> f64 {
    if [local_w, local_h, remote_w, remote_h]
        .iter()
        .any(|&v| v <= 0)
    {
        return 0.0;
    }
    let ratio = remote_w.max(remote_h) as f64 / local_w.max(local_h) as f64;
    if ratio <= 1.0 {
        return 0.0;
    }
    let scaled = ratio.ln() / RESOLUTION_REFERENCE_RATIO.ln();
    MAX_RESOLUTION_BONUS.min(MAX_RESOLUTION_BONUS * scaled)
}

fn has_parser(candidate: &Candidate) -> bool {
    !candidate.url.is_empty() && find_parser(&candidate.url).is_some()
}

/// `quality_bonus`: how much better than its similarity alone this candidate looks, from
/// information already in hand.
pub fn quality_bonus(
    candidate: &Candidate,
    local_width: Option<i64>,
    local_height: Option<i64>,
) -> f64 {
    let mut bonus = 0.0;
    if has_parser(candidate) {
        bonus += PARSER_BONUS;
    }
    bonus += resolution_bonus(
        local_width.unwrap_or(0),
        local_height.unwrap_or(0),
        candidate.width.unwrap_or(0),
        candidate.height.unwrap_or(0),
    );
    MAX_QUALITY_BONUS.min(bonus)
}

/// `candidate_score`: the value the dropdown is ordered by, highest first.
pub fn candidate_score(
    candidate: &Candidate,
    local_width: Option<i64>,
    local_height: Option<i64>,
) -> f64 {
    let mut score = candidate.similarity;
    if candidate.remote_available == Some(false) {
        score -= UNAVAILABLE_PENALTY;
    }
    score += quality_bonus(candidate, local_width, local_height);
    score
}

/// The terms `score_breakdown` returns, in the order Python's dict writes them.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreBreakdown {
    pub similarity: f64,
    pub has_parser: bool,
    pub parser_bonus: f64,
    pub resolution_bonus: f64,
    pub quality_bonus: f64,
    pub quality_bonus_cap: f64,
    pub unavailable_penalty_applied: bool,
    pub unavailable_penalty: f64,
    pub score: f64,
}

/// `score_breakdown`: why this candidate scored where it did.
pub fn score_breakdown(
    candidate: &Candidate,
    local_width: Option<i64>,
    local_height: Option<i64>,
) -> ScoreBreakdown {
    let has_parser = has_parser(candidate);
    let unavailable = candidate.remote_available == Some(false);
    ScoreBreakdown {
        similarity: candidate.similarity,
        has_parser,
        parser_bonus: if has_parser { PARSER_BONUS } else { 0.0 },
        resolution_bonus: resolution_bonus(
            local_width.unwrap_or(0),
            local_height.unwrap_or(0),
            candidate.width.unwrap_or(0),
            candidate.height.unwrap_or(0),
        ),
        quality_bonus: quality_bonus(candidate, local_width, local_height),
        quality_bonus_cap: MAX_QUALITY_BONUS,
        unavailable_penalty_applied: unavailable,
        unavailable_penalty: if unavailable {
            UNAVAILABLE_PENALTY
        } else {
            0.0
        },
        score: candidate_score(candidate, local_width, local_height),
    }
}

/// `rank_candidates`: the candidates ordered best-first. Stable within equal scores, so
/// candidates that tie keep the order the engines returned them in.
pub fn rank_candidates(
    candidates: Vec<Candidate>,
    local_width: Option<i64>,
    local_height: Option<i64>,
    filename: &str,
) -> Vec<Candidate> {
    if candidates.len() < 2 {
        return candidates;
    }
    // Python sorts on `-score`; negating keeps the same total order, ties included.
    let mut keyed: Vec<(f64, usize)> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (-candidate_score(c, local_width, local_height), i))
        .collect();
    keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if keyed[0].1 != 0 {
        let first = &candidates[keyed[0].1];
        let top = &candidates[0];
        info!(
            "{}: recommending {} over the most similar match {} - score {:.1} (similarity {:.0}%) vs {:.1} (similarity {:.0}%)",
            if filename.is_empty() {
                "match"
            } else {
                filename
            },
            first.url,
            top.url,
            candidate_score(first, local_width, local_height),
            first.similarity,
            candidate_score(top, local_width, local_height),
            top.similarity,
        );
    }
    let mut slots: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
    keyed
        .into_iter()
        .map(|(_, i)| slots[i].take().expect("each index appears once"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(url: &str, similarity: f64) -> Candidate {
        let mut c = Candidate::new(url);
        c.similarity = similarity;
        c
    }

    #[test]
    fn parser_site_outranks_an_unreadable_one_at_equal_similarity() {
        let a = cand("https://example.invalid/x", 90.0);
        let b = cand("https://danbooru.donmai.us/posts/1", 90.0);
        let ranked = rank_candidates(vec![a.clone(), b.clone()], None, None, "f");
        assert_eq!(ranked[0].url, b.url);
        assert_eq!(ranked[1].url, a.url);
    }

    #[test]
    fn similarity_stays_dominant_over_the_capped_bonus() {
        let mut high = cand("https://example.invalid/x", 98.0);
        high.width = Some(10);
        let mut low = cand("https://danbooru.donmai.us/posts/1", 94.0);
        low.width = Some(4000);
        low.height = Some(4000);
        let ranked = rank_candidates(vec![low.clone(), high.clone()], Some(100), Some(100), "");
        assert_eq!(ranked[0].url, high.url);
        assert_eq!(quality_bonus(&low, Some(100), Some(100)), MAX_QUALITY_BONUS);
    }

    #[test]
    fn ties_keep_engine_order() {
        let cs: Vec<Candidate> = (0..5)
            .map(|i| cand(&format!("https://example.invalid/{i}"), 80.0))
            .collect();
        let ranked = rank_candidates(cs.clone(), None, None, "");
        let urls: Vec<&str> = ranked.iter().map(|c| c.url.as_str()).collect();
        let want: Vec<&str> = cs.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, want);
    }

    #[test]
    fn unavailable_sinks_and_unknown_size_is_not_a_penalty() {
        let mut dead = cand("https://danbooru.donmai.us/posts/1", 99.0);
        dead.remote_available = Some(false);
        let alive = cand("https://example.invalid/y", 50.0);
        let ranked = rank_candidates(vec![dead.clone(), alive.clone()], None, None, "");
        assert_eq!(ranked[0].url, alive.url);
        let bd = score_breakdown(&dead, None, None);
        assert!(bd.unavailable_penalty_applied);
        assert_eq!(bd.unavailable_penalty, UNAVAILABLE_PENALTY);
        assert_eq!(bd.resolution_bonus, 0.0);
        let mut unknown = cand("https://example.invalid/z", 10.0);
        unknown.remote_available = None;
        assert!(!score_breakdown(&unknown, None, None).unavailable_penalty_applied);
    }

    #[test]
    fn resolution_bonus_is_log_scaled_and_clamped() {
        assert_eq!(resolution_bonus(100, 100, 100, 100), 0.0);
        assert_eq!(resolution_bonus(100, 100, 50, 50), 0.0);
        assert_eq!(resolution_bonus(0, 100, 400, 400), 0.0);
        assert!((resolution_bonus(100, 100, 200, 100) - 0.75).abs() < 1e-12);
        assert_eq!(resolution_bonus(100, 100, 4000, 4000), MAX_RESOLUTION_BONUS);
        assert_eq!(resolution_bonus(100, 100, 400, 100), MAX_RESOLUTION_BONUS);
    }

    #[test]
    fn short_lists_come_back_untouched() {
        assert!(rank_candidates(Vec::new(), None, None, "").is_empty());
        let one = vec![cand("https://example.invalid/a", 1.0)];
        assert_eq!(rank_candidates(one.clone(), None, None, ""), one);
    }
}
