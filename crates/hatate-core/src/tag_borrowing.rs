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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CandidateTag;

    fn candidate(
        url: &str,
        similarity: f64,
        tags: &[&str],
        fetched: bool,
        available: Option<bool>,
    ) -> Candidate {
        let mut c = Candidate::new(url);
        c.similarity = similarity;
        c.booru_tags = tags
            .iter()
            .map(|t| CandidateTag {
                name: (*t).to_owned(),
                namespace: None,
            })
            .collect();
        c.booru_tags_fetched = fetched;
        c.remote_available = available;
        c
    }

    fn settings(slack: f64, on: bool) -> Settings {
        let paths =
            crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| "/home/test".into()))
                .unwrap();
        let mut s = Settings::with_defaults(&paths);
        s.borrow_tags_from_other_matches = on;
        s.borrow_tags_similarity_slack = slack;
        s
    }

    /// `_borrow`: the page fetch is a fake that marks the candidate fetched and gives it nothing.
    async fn borrow(
        candidates: &mut [Candidate],
        s: &Settings,
        chosen: usize,
    ) -> (Option<String>, Vec<String>) {
        let mut fetched = Vec::new();
        let got = borrow_tags("b.png", candidates, chosen, s, async |c: &mut Candidate| {
            fetched.push(c.url.clone());
            c.booru_tags_fetched = true;
            Ok::<(), ()>(())
        })
        .await
        .unwrap();
        (got, fetched)
    }

    #[tokio::test]
    async fn a_close_enough_match_lends_its_tags() {
        // test_a_close_enough_match_lends_its_tags
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 91.0, &["cat", "hat"], true, None),
        ];
        let (lender, _) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender.as_deref(), Some("https://gelbooru.com/1"));
        let names: Vec<&str> = c[0].booru_tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["cat", "hat"]);
    }

    #[tokio::test]
    async fn the_chosen_match_stays_chosen_and_says_where_its_tags_came_from() {
        // test_the_chosen_match_stays_chosen, test_where_the_tags_came_from_is_recorded
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 91.0, &["cat"], true, None),
        ];
        borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(c[0].url, "https://mangadex.org/chapter/x");
        assert_eq!(
            c[0].tags_borrowed_from.as_deref(),
            Some("https://gelbooru.com/1")
        );
        // The lender keeps its own tags.
        assert_eq!(c[1].booru_tags.len(), 1);
    }

    #[tokio::test]
    async fn a_much_weaker_match_may_not_lend() {
        // test_a_much_weaker_match_may_not_lend
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 62.0, &["wrong"], true, None),
        ];
        let (lender, _) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender, None);
        assert!(c[0].booru_tags.is_empty());
    }

    #[tokio::test]
    async fn zero_slack_allows_only_an_equal_or_better_match() {
        // test_zero_slack_allows_only_an_equal_or_better_match
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 92.9, &["cat"], true, None),
        ];
        assert_eq!(borrow(&mut c, &settings(0.0, true), 0).await.0, None);
        let mut c = vec![
            candidate("https://mangadex.org/c", 93.0, &[], false, None),
            candidate("https://gelbooru.com/2", 93.0, &["cat"], true, None),
        ];
        assert_eq!(
            borrow(&mut c, &settings(0.0, true), 0).await.0.as_deref(),
            Some("https://gelbooru.com/2")
        );
    }

    #[tokio::test]
    async fn a_negative_slack_is_zero_slack() {
        // `max(0.0, slack)`
        let mut c = vec![
            candidate("a", 93.0, &[], false, None),
            candidate("b", 93.0, &["cat"], true, None),
        ];
        assert_eq!(
            borrow(&mut c, &settings(-10.0, true), 0).await.0.as_deref(),
            Some("b")
        );
        assert_eq!(candidates_that_may_lend(&c, 0, f64::NAN), vec![1]);
    }

    #[tokio::test]
    async fn a_match_with_its_own_tags_borrows_nothing_and_fetches_nothing() {
        // test_a_match_with_its_own_tags_borrows_nothing
        let mut c = vec![
            candidate("https://danbooru.donmai.us/1", 93.0, &["own"], true, None),
            candidate("https://gelbooru.com/1", 93.0, &["other"], true, None),
        ];
        let (lender, fetched) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender, None);
        assert!(fetched.is_empty());
    }

    #[tokio::test]
    async fn a_dead_match_is_not_asked() {
        // test_a_dead_match_is_not_asked
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 93.0, &[], false, Some(false)),
        ];
        let (lender, fetched) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender, None);
        assert!(fetched.is_empty());
    }

    #[tokio::test]
    async fn the_strongest_eligible_match_is_asked_first() {
        // test_the_strongest_eligible_match_is_asked_first
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/weak", 90.0, &["a"], true, None),
            candidate("https://gelbooru.com/strong", 92.0, &["b"], true, None),
        ];
        let (lender, _) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender.as_deref(), Some("https://gelbooru.com/strong"));
    }

    #[tokio::test]
    async fn it_walks_the_eligible_ones_but_fetches_each_once() {
        // test_only_one_alternative_is_fetched
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 92.0, &[], false, None),
            candidate("https://gelbooru.com/2", 91.0, &[], false, None),
        ];
        let (lender, fetched) = borrow(&mut c, &settings(5.0, true), 0).await;
        assert_eq!(lender, None);
        assert_eq!(
            fetched,
            ["https://gelbooru.com/1", "https://gelbooru.com/2"]
        );
    }

    #[tokio::test]
    async fn the_setting_turns_it_off() {
        // test_the_setting_turns_it_off
        let mut c = vec![
            candidate("https://mangadex.org/chapter/x", 93.0, &[], false, None),
            candidate("https://gelbooru.com/1", 93.0, &["cat"], true, None),
        ];
        let (lender, fetched) = borrow(&mut c, &settings(5.0, false), 0).await;
        assert_eq!(lender, None);
        assert!(fetched.is_empty());
    }

    #[tokio::test]
    async fn a_chosen_index_out_of_range_lends_nothing() {
        let mut c = vec![candidate("a", 93.0, &[], false, None)];
        assert_eq!(borrow(&mut c, &settings(5.0, true), 3).await.0, None);
        assert!(candidates_that_may_lend(&c, 3, 5.0).is_empty());
        assert!(candidates_that_may_lend(&[], 0, 5.0).is_empty());
    }

    #[tokio::test]
    async fn a_failing_fetch_propagates_like_an_exception() {
        let mut c = vec![
            candidate("a", 93.0, &[], false, None),
            candidate("b", 93.0, &[], false, None),
        ];
        let got = borrow_tags(
            "x",
            &mut c,
            0,
            &settings(5.0, true),
            async |_: &mut Candidate| Err("boom"),
        )
        .await;
        assert_eq!(got, Err("boom"));
    }

    #[test]
    fn equal_similarities_keep_the_order_they_came_in() {
        // `sorted` is stable
        let c = vec![
            candidate("chosen", 90.0, &[], false, None),
            candidate("first", 90.0, &[], false, None),
            candidate("second", 90.0, &[], false, None),
            candidate("better", 95.0, &[], false, None),
        ];
        assert_eq!(candidates_that_may_lend(&c, 0, 5.0), vec![3, 1, 2]);
    }
}
