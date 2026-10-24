//! What Google Lens does with the matches once they are parsed (`core/google_lens.py`:
//! `rule34_searches`, `_recover_rule34_searches`, `_resolve_subreddits`, `_is_taggable`,
//! `taggable_first`, `keep_taggable_first`, `effective_timeout`).
//!
//! Two of these reach the network in Python (`boorus.rule34.find_post_by_search` and
//! `reddit.find_post`). They are behind [`LensSites`]: the shipped build uses
//! [`super::lens_sites::OnlineSites`], which runs the two lookups over the net layer, and a test
//! hands in scripted answers or [`NoSites`], which finds nothing, the answer Python gets when its
//! own request fails.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::google_lens::{GoogleLensMatch, tile_text};
use crate::boorus::{self, host::raises_value_error};
use crate::image_compare::Hash;
use crate::pycompat::strip;
use crate::reddit;

/// `MIN_BUDGET_SECONDS`: the least time a Lens search gets, whatever the general search timeout is
/// set to. The page reaches its results URL in about a second and then leaves its body empty for
/// well over half a minute while its JavaScript fetches the results.
pub const MIN_BUDGET_SECONDS: f64 = 90.0;

/// `MAX_RESULTS`.
pub const MAX_RESULTS: usize = 8;

/// `RESERVED_VISUAL_SLOTS`: slots kept for the best-ranked taggable visual matches, so a page with
/// 8+ taggable exact matches does not crowd every visual one out.
pub const RESERVED_VISUAL_SLOTS: usize = 2;

/// `MAX_RULE34_SEARCHES`.
pub const MAX_RULE34_SEARCHES: usize = 3;

/// `boorus.rule34.LISTING_HASH_SIZE`.
pub const LISTING_HASH_SIZE: usize = 16;

/// An exception Python lets out of `search` that is not a `GoogleLensError`: `find_parser`'s
/// `ValueError` out of `urlparse`, or a `DecompressionBombError`. The text is the Rust side's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unexpected(pub String);

/// What `boorus.rule34.find_post_by_search` returns: `(url, thumb_url, distance)`.
pub type Rule34Hit = (String, Option<String>, i64);

/// The two lookups of `search` that go to the network. An `Err` is an exception Python lets out of
/// the lookup (a decompression bomb in a thumbnail, a hard deadline), which ends the search.
pub trait LensSites: Send + Sync + 'static {
    /// `boorus.rule34.find_post_by_search(tags, local_hash)`.
    fn find_rule34_post(
        &self,
        tags: &str,
        local_hash: Option<&Hash>,
    ) -> Result<Option<Rule34Hit>, Unexpected>;

    /// `reddit.find_post(subreddit, media_id)`.
    fn find_reddit_post(
        &self,
        subreddit: &str,
        media_id: &str,
    ) -> Result<Option<String>, Unexpected>;
}

/// A lookup that finds nothing: the test double (and what Python gets when its request fails).
pub struct NoSites;

impl LensSites for NoSites {
    fn find_rule34_post(&self, _: &str, _: Option<&Hash>) -> Result<Option<Rule34Hit>, Unexpected> {
        Ok(None)
    }

    fn find_reddit_post(&self, _: &str, _: &str) -> Result<Option<String>, Unexpected> {
        Ok(None)
    }
}

/// `effective_timeout`: the budget a Lens search actually gets. The general search timeout is
/// sized for one HTTP request; this is a browser loading a JavaScript application, so the setting
/// is a floor, not a ceiling.
pub fn effective_timeout(timeout: f64) -> f64 {
    // Python's `max(timeout, MIN_BUDGET_SECONDS)` returns the first argument unless the second is
    // larger, so a NaN timeout stays NaN.
    if MIN_BUDGET_SECONDS > timeout {
        MIN_BUDGET_SECONDS
    } else {
        timeout
    }
}

/// `_RULE34_SEARCH_TILE_RE`: `Rule 34 / parent:10755759 250x177 Rule 34`; group 1 is the search.
static RULE34_SEARCH_TILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Rule 34 / (.+?)(?: \d[\d,]*x\d[\d,]*)?(?: Rule 34)?$").expect("tile regex")
});

/// `_PRECISE_TAG_RE`, compiled with `re.IGNORECASE`. Python folds `i` with U+0130 and U+0131
/// (`crate::pycase`), so that letter is spelled out as a class.
static PRECISE_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[pP][aA][rR][eE][nN][tT]|[iI\u{130}\u{131}][dD]|[mM][dD]5|[pP][oO][oO][lL]):")
        .expect("precise tag regex")
});

/// `rule34_searches`: the rule34.xxx searches named by exact-match tiles, precise first.
pub fn rule34_searches(tiles: &Value) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if let Some(list) = tiles.as_array() {
        for tile in list {
            let Some(tile) = tile.as_object() else {
                continue;
            };
            let text = tile_text(tile, "text");
            if let Some(hit) = RULE34_SEARCH_TILE_RE.captures(strip(&text)) {
                let search = strip(&hit[1]).to_owned();
                if !found.contains(&search) {
                    found.push(search);
                }
            }
        }
    }
    let precise = |q: &&String| PRECISE_TAG_RE.is_match(q);
    let mut out: Vec<String> = found.iter().filter(precise).cloned().collect();
    out.extend(found.iter().filter(|q| !precise(q)).cloned());
    out
}

/// `_recover_rule34_searches`: rule34.xxx posts Lens listed only by the search they were found
/// in. Stops at the first one found: they are all exact matches of the same picture, and each
/// search costs a page plus a page of thumbnails.
pub fn recover_rule34_searches(
    tiles: &Value,
    image_bytes: &[u8],
    sites: &dyn LensSites,
) -> Result<Vec<GoogleLensMatch>, Unexpected> {
    let mut searches = rule34_searches(tiles);
    searches.truncate(MAX_RULE34_SEARCHES);
    if searches.is_empty() {
        return Ok(Vec::new());
    }
    let local_hash = crate::image_compare::dhash(
        crate::image_compare::Source::Bytes(image_bytes),
        LISTING_HASH_SIZE,
    )
    .map_err(|bomb| Unexpected(bomb.to_string()))?;
    for tags in searches {
        if let Some((url, thumb, distance)) = sites.find_rule34_post(&tags, local_hash.as_ref())? {
            tracing::info!(
                "Recovered {url} from Lens's \"Rule 34 / {tags}\" exact match (distance {distance})"
            );
            return Ok(vec![GoogleLensMatch {
                url,
                thumb_url: thumb,
                similarity: 0.0,
                title: Some(format!("Rule 34 / {tags}")),
                source_name: Some("rule34.xxx".to_owned()),
                width: None,
                height: None,
            }]);
        }
    }
    Ok(Vec::new())
}

/// `_resolve_subreddits`: swaps each subreddit listing for the post its picture is in. What
/// cannot be resolved safely is dropped, and a URL that resolves to one already listed is not
/// listed twice (across both groups).
pub fn resolve_subreddits(
    exact: Vec<GoogleLensMatch>,
    visual: Vec<GoogleLensMatch>,
    sites: &dyn LensSites,
) -> Result<(Vec<GoogleLensMatch>, Vec<GoogleLensMatch>), Unexpected> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolve = |group: Vec<GoogleLensMatch>| -> Result<Vec<GoogleLensMatch>, Unexpected> {
        let mut kept = Vec::new();
        for mut m in group {
            let mut raised = None;
            let resolved = reddit::resolve_listing(&m.url, m.thumb_url.as_deref(), |sub, media| {
                sites.find_reddit_post(sub, media).unwrap_or_else(|e| {
                    raised = Some(e);
                    None
                })
            });
            if let Some(error) = raised {
                return Err(error);
            }
            let Some(url) = resolved else {
                continue;
            };
            if seen.contains(&url) {
                continue;
            }
            m.url.clone_from(&url);
            seen.insert(url);
            kept.push(m);
        }
        Ok(kept)
    };
    let exact = resolve(exact)?;
    let visual = resolve(visual)?;
    Ok((exact, visual))
}

/// `_is_taggable`: `boorus.find_parser(url) is not None`. `Err` where `find_parser` raises
/// `ValueError` out of `urlparse`.
fn is_taggable(m: &GoogleLensMatch) -> Result<bool, Unexpected> {
    if raises_value_error(&m.url) {
        return Err(Unexpected("invalid URL".to_owned()));
    }
    Ok(boorus::find_parser(&m.url).is_some())
}

/// `taggable_first`: results from sites `boorus/` can read tags from, ahead of the rest. Stable:
/// each half keeps Google's order.
pub fn taggable_first(matches: Vec<GoogleLensMatch>) -> Result<Vec<GoogleLensMatch>, Unexpected> {
    let flags = matches
        .iter()
        .map(is_taggable)
        .collect::<Result<Vec<_>, _>>()?;
    let (mut taggable, rest): (Vec<_>, Vec<_>) =
        matches.into_iter().zip(flags).partition(|(_, flag)| *flag);
    taggable.extend(rest);
    Ok(taggable.into_iter().map(|(m, _)| m).collect())
}

/// `keep_taggable_first`: cuts the list to `limit` without letting untaggable sites crowd out
/// ones that give tags. What is kept stays in the order it was given. `visual_start` and
/// `reserve_visual` guarantee the best-ranked taggable visual matches up to `reserve_visual`
/// slots even when 8+ taggable exact matches would fill the list.
pub fn keep_taggable_first(
    matches: Vec<GoogleLensMatch>,
    limit: usize,
    visual_start: usize,
    reserve_visual: usize,
) -> Result<Vec<GoogleLensMatch>, Unexpected> {
    if matches.len() <= limit {
        return Ok(matches);
    }
    let taggable: Vec<usize> = matches
        .iter()
        .enumerate()
        .filter_map(|(i, m)| is_taggable(m).map(|t| t.then_some(i)).transpose())
        .collect::<Result<_, _>>()?;
    let taggable_visual: Vec<usize> = taggable
        .iter()
        .copied()
        .filter(|i| *i >= visual_start)
        .collect();
    let mut kept: HashSet<usize> = taggable_visual.into_iter().take(reserve_visual).collect();
    for i in taggable {
        if kept.len() >= limit {
            break;
        }
        kept.insert(i);
    }
    for i in 0..matches.len() {
        if kept.len() >= limit {
            break;
        }
        kept.insert(i);
    }
    Ok(matches
        .into_iter()
        .enumerate()
        .filter(|(i, _)| kept.contains(i))
        .map(|(_, m)| m)
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn m(url: &str) -> GoogleLensMatch {
        GoogleLensMatch {
            url: url.to_owned(),
            thumb_url: None,
            similarity: 0.0,
            title: None,
            source_name: None,
            width: None,
            height: None,
        }
    }

    fn urls(matches: &[GoogleLensMatch]) -> Vec<&str> {
        matches.iter().map(|m| m.url.as_str()).collect()
    }

    const TAGGABLE: &str = "https://danbooru.donmai.us/posts/";
    const PLAIN: &str = "https://example.com/p/";

    #[test]
    fn the_budget_is_a_floor_not_a_ceiling() {
        assert_eq!(effective_timeout(30.0), 90.0);
        assert_eq!(effective_timeout(90.0), 90.0);
        assert_eq!(effective_timeout(300.0), 300.0);
    }

    #[test]
    fn precise_rule34_searches_come_first_and_repeats_are_dropped() {
        let tiles = json!([
            {"text": "Rule 34 / blue_eyes 250x177 Rule 34"},
            {"text": "Rule 34 / parent:10755759 250x177 Rule 34"},
            "not a tile",
            {"text": "  Rule 34 / blue_eyes  "},
            {"text": "Rule 34 / ID:5"},
            {"text": "Rule 34 | 4585855 / - Rule 34"},
            {"text": null},
        ]);
        assert_eq!(
            rule34_searches(&tiles),
            vec!["parent:10755759", "ID:5", "blue_eyes"]
        );
        assert!(rule34_searches(&json!(null)).is_empty());
    }

    #[test]
    fn taggable_sites_go_ahead_of_the_rest_and_each_half_keeps_its_order() {
        let input = vec![
            m(&format!("{PLAIN}1")),
            m(&format!("{TAGGABLE}2")),
            m(&format!("{PLAIN}3")),
            m(&format!("{TAGGABLE}4")),
        ];
        let out = taggable_first(input).unwrap();
        assert_eq!(
            urls(&out),
            vec![
                &format!("{TAGGABLE}2")[..],
                &format!("{TAGGABLE}4")[..],
                &format!("{PLAIN}1")[..],
                &format!("{PLAIN}3")[..],
            ]
        );
    }

    #[test]
    fn a_malformed_host_is_the_exception_python_lets_out() {
        let err = taggable_first(vec![m("http://[bad/")]).unwrap_err();
        assert_eq!(err, Unexpected("invalid URL".to_owned()));
    }

    #[test]
    fn a_list_under_the_limit_is_untouched() {
        let input: Vec<_> = (0..8).map(|i| m(&format!("{PLAIN}{i}"))).collect();
        assert_eq!(keep_taggable_first(input.clone(), 8, 0, 0).unwrap(), input);
    }

    #[test]
    fn untaggable_sites_do_not_crowd_out_one_that_gives_tags() {
        // Ranks 0..11 are untaggable, rank 12 is the one post that would tag the image.
        let mut input: Vec<_> = (0..12).map(|i| m(&format!("{PLAIN}{i}"))).collect();
        input.push(m(&format!("{TAGGABLE}9")));
        let out = keep_taggable_first(input, 8, 0, 0).unwrap();
        assert_eq!(out.len(), 8);
        assert!(urls(&out).contains(&format!("{TAGGABLE}9").as_str()));
        // The rest fill what is left, in the order they were given.
        assert_eq!(urls(&out)[0], format!("{PLAIN}0"));
    }

    #[test]
    fn reserved_slots_keep_the_best_taggable_visual_matches_in_a_crowd_of_exact_ones() {
        // DAN-357: 10 taggable exact matches would fill all 8 slots on their own.
        let mut input: Vec<_> = (0..10).map(|i| m(&format!("{TAGGABLE}{i}"))).collect();
        input.extend((100..103).map(|i| m(&format!("{TAGGABLE}{i}"))));
        let out = keep_taggable_first(input.clone(), 8, 10, 2).unwrap();
        let kept: Vec<_> = urls(&out)
            .iter()
            .map(|u| u.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(kept, vec!["0", "1", "2", "3", "4", "5", "100", "101"]);
        let without = keep_taggable_first(input, 8, 10, 0).unwrap();
        assert_eq!(urls(&without).len(), 8);
        assert!(!urls(&without).iter().any(|u| u.ends_with("/100")));
    }

    struct FindsReddit;

    impl LensSites for FindsReddit {
        fn find_rule34_post(
            &self,
            tags: &str,
            _: Option<&Hash>,
        ) -> Result<Option<Rule34Hit>, Unexpected> {
            Ok((tags == "parent:1").then(|| {
                (
                    "https://rule34.xxx/index.php?page=post&s=view&id=1".to_owned(),
                    Some("https://thumb/1.jpg".to_owned()),
                    3,
                )
            }))
        }

        fn find_reddit_post(
            &self,
            subreddit: &str,
            media_id: &str,
        ) -> Result<Option<String>, Unexpected> {
            Ok(Some(format!(
                "https://www.reddit.com/r/{subreddit}/comments/{media_id}/"
            )))
        }
    }

    #[test]
    fn a_subreddit_listing_is_swapped_for_its_post_or_dropped_and_never_listed_twice() {
        let listing = m("https://www.reddit.com/r/Pics/");
        let mut with_image = listing.clone();
        with_image.thumb_url = Some("https://preview.redd.it/abc123.jpg?width=100".to_owned());
        let (exact, visual) = resolve_subreddits(
            vec![with_image.clone(), listing.clone()],
            vec![with_image, m(&format!("{PLAIN}1"))],
            &FindsReddit,
        )
        .unwrap();
        // The one with an image resolves; the listing with none is dropped; the second copy of
        // the resolved URL (in the other group) is not listed again.
        assert_eq!(
            urls(&exact),
            vec!["https://www.reddit.com/r/Pics/comments/abc123/"]
        );
        assert_eq!(urls(&visual), vec![&format!("{PLAIN}1")[..]]);
        // A lookup that finds nothing falls back to the image link itself.
        let mut lone = listing;
        lone.thumb_url = Some("https://preview.redd.it/abc123.jpg".to_owned());
        let (exact, _) = resolve_subreddits(vec![lone], vec![], &NoSites).unwrap();
        assert_eq!(urls(&exact), vec!["https://i.redd.it/abc123.jpg"]);
    }

    #[test]
    fn the_first_rule34_search_that_finds_its_post_is_the_one_used() {
        let tiles = json!([
            {"text": "Rule 34 / blue_eyes"},
            {"text": "Rule 34 / parent:1"},
        ]);
        let png = {
            let mut out = Vec::new();
            image::RgbImage::from_pixel(8, 8, image::Rgb([9, 9, 9]))
                .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .unwrap();
            out
        };
        let found = recover_rule34_searches(&tiles, &png, &FindsReddit).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].url,
            "https://rule34.xxx/index.php?page=post&s=view&id=1"
        );
        assert_eq!(found[0].title.as_deref(), Some("Rule 34 / parent:1"));
        assert_eq!(found[0].source_name.as_deref(), Some("rule34.xxx"));
        assert!(
            recover_rule34_searches(&tiles, &png, &NoSites)
                .unwrap()
                .is_empty()
        );
        assert!(
            recover_rule34_searches(&json!([]), &png, &FindsReddit)
                .unwrap()
                .is_empty()
        );
    }
}
