//! What Google Lens does with the matches once they are parsed (`core/google_lens.py`:
//! `rule34_searches`, `_recover_rule34_searches`, `_resolve_subreddits`, `_is_taggable`,
//! `taggable_first`, `keep_taggable_first`, `effective_timeout`).
//!
//! Two of these reach the network in Python (`boorus.rule34.find_post_by_search` and
//! `reddit.find_post`). Neither is ported yet, so they are behind [`LensSites`]: a test hands in
//! scripted answers, and the shipped [`NoSites`] finds nothing, which is the answer Python gets
//! when its own request fails. The shipped build never reaches them anyway: its browser answers
//! "unavailable" before any page is parsed.

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

/// The two lookups of `search` that go to the network and are not ported yet.
pub trait LensSites: Send + Sync + 'static {
    /// `boorus.rule34.find_post_by_search(tags, local_hash)`: `(url, thumb_url, distance)`.
    fn find_rule34_post(
        &self,
        tags: &str,
        local_hash: Option<&Hash>,
    ) -> Option<(String, Option<String>, i64)>;

    /// `reddit.find_post(subreddit, media_id)`.
    fn find_reddit_post(&self, subreddit: &str, media_id: &str) -> Option<String>;
}

/// The shipped answer: neither lookup finds anything.
pub struct NoSites;

impl LensSites for NoSites {
    fn find_rule34_post(&self, _: &str, _: Option<&Hash>) -> Option<(String, Option<String>, i64)> {
        None
    }

    fn find_reddit_post(&self, _: &str, _: &str) -> Option<String> {
        None
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
        if let Some((url, thumb, distance)) = sites.find_rule34_post(&tags, local_hash.as_ref()) {
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
) -> (Vec<GoogleLensMatch>, Vec<GoogleLensMatch>) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut resolve = |group: Vec<GoogleLensMatch>| {
        let mut kept = Vec::new();
        for mut m in group {
            let Some(url) =
                reddit::resolve_listing(&m.url, m.thumb_url.as_deref(), |sub, media| {
                    sites.find_reddit_post(sub, media)
                })
            else {
                continue;
            };
            if seen.contains(&url) {
                continue;
            }
            m.url.clone_from(&url);
            seen.insert(url);
            kept.push(m);
        }
        kept
    };
    let exact = resolve(exact);
    let visual = resolve(visual);
    (exact, visual)
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
