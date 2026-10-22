//! `core/multipage_resolve.py`: which image of a multi-image post the local file is.
//!
//! [`resolve_multipage_candidate`] points a multi-page post's candidate at the page that actually
//! matches the local file, instead of its first page. Two independent signals say which page it
//! really was: SauceNAO names the page in its own thumbnail URL (free, but it can be stale, so it
//! is checked against the live page list and never overrules the comparison), and each page's
//! thumbnail, hashed and compared with the local file (authoritative, but a download per page).
//! The dimensions in the page list are tried first, then hashing, and SauceNAO's claim breaks a
//! tie that hashing reports it cannot. Past [`MAX_PAGES_TO_COMPARE`] a page something has already
//! named can still be checked on its own for one download.
//!
//! It leaves the candidate untouched whenever the answer is not clear: being wrong about this is
//! exactly the problem being fixed, and the first page is at least a predictable default.
//!
//! **The cut.** All of `multipage_resolve.py` is here: `_use_page` and `_resolve_multipage_candidate`.
//! The network calls are `remote::fetch_text` (the page list, with the site's headers) and
//! `remote::download_bytes` (each thumbnail), unchanged, through the [`NetClient`] the caller
//! holds. Both Python calls pass `settings.hydrus.timeout` as the download timeout, and so does
//! this. `search_engine.py`'s re-export of the function is the caller's, in a later ticket.

use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::boorus::{BooruPageInfo, Context, Page, find_parser, host, pixiv};
use crate::config::Settings;
use crate::image_compare::{Source, image_dimensions};
use crate::model::Candidate;
use crate::multipage::{
    MAX_PAGES_TO_COMPARE, PageSize, check_page, choose_matching_page, dimensions_contradict_page,
    dimensions_identify_page, page_index_from_thumbnail, page_url_for_index,
};
use crate::net::NetClient;
use crate::pyimage::DecompressionBomb;
use crate::pyurl::UrlValueError;
use crate::remote::{self, Raised};

/// What leaves `_resolve_multipage_candidate` as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// From `remote.fetch_text` or `remote.download_bytes` (a hard deadline, mostly).
    Raised(Raised),
    /// `Image.DecompressionBombError`, from reading the local file's dimensions or hashing.
    Bomb(DecompressionBomb),
    /// `ValueError` from `urlparse` of the candidate's or the thumbnail's URL.
    Url(UrlValueError),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Raised(r) => write!(f, "{r}"),
            ResolveError::Bomb(b) => write!(f, "{b}"),
            ResolveError::Url(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for ResolveError {}

impl From<Raised> for ResolveError {
    fn from(r: Raised) -> Self {
        ResolveError::Raised(r)
    }
}

impl From<DecompressionBomb> for ResolveError {
    fn from(b: DecompressionBomb) -> Self {
        ResolveError::Bomb(b)
    }
}

impl From<UrlValueError> for ResolveError {
    fn from(u: UrlValueError) -> Self {
        ResolveError::Url(u)
    }
}

/// `a or b`: the first non-empty string.
fn first_non_empty<'a>(options: &[Option<&'a str>]) -> Option<&'a str> {
    options.iter().flatten().copied().find(|s| !s.is_empty())
}

fn page_urls(page: &Page) -> Option<&serde_json::Map<String, Value>> {
    page.get("urls").and_then(Value::as_object)
}

fn url_of<'a>(page: &'a Page, key: &str) -> Option<&'a str> {
    page_urls(page)?.get(key).and_then(Value::as_str)
}

fn int_of(page: &Page, key: &str) -> Option<i64> {
    page.get(key).and_then(Value::as_i64)
}

/// `_use_page`: repoints a candidate at one page of its post. Page 1 is repointed too, rather than
/// returned from early: the first page is the default for a post whose page nobody named, but not
/// for one whose URL named a page itself (a tweet's `/photo/3`).
fn use_page(candidate: &mut Candidate, pages: &[Page], index: usize, page_count: i64, why: &str) {
    let page = &pages[index];
    let direct = first_non_empty(&[url_of(page, "original")])
        .map(str::to_owned)
        .or_else(|| {
            page_url_for_index(candidate.direct_file_url.as_deref().unwrap_or(""), index)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| candidate.direct_file_url.clone());
    candidate.direct_file_url = direct;
    candidate.preview_url = first_non_empty(&[
        url_of(page, "regular"),
        url_of(page, "small"),
        candidate.preview_url.as_deref(),
    ])
    .map(str::to_owned);
    candidate.page_index = index as i64;
    // The dimensions from the artwork record describe page 1, so they no longer apply. This
    // page's own are right there in the page list, so use them; failing that, absent beats
    // confidently wrong.
    candidate.width = int_of(page, "width");
    candidate.height = int_of(page, "height");
    if index == 0 {
        debug!(
            "{}: the local image is page 1 of {page_count} after all ({why})",
            candidate.url
        );
        return;
    }
    info!(
        "{}: {why} - using page {} of {page_count} instead of the first",
        candidate.url,
        index + 1
    );
}

fn sha256_of_file(path: &str) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// `_resolve_multipage_candidate`: points a multi-page post's candidate at the page that actually
/// matches the local file, instead of its first page. `page_info` is the record the candidate's
/// own page fetch just made, when there is one: Pawchive's post API is both, and is not fetched
/// twice.
pub async fn resolve_multipage_candidate(
    net: &NetClient,
    candidate: &mut Candidate,
    page_count: i64,
    settings: &Settings,
    local_path: &str,
    page_info: Option<&BooruPageInfo>,
) -> Result<(), ResolveError> {
    // Any parser that can list a post's pages gets this, not just Pixiv: the work is generic and
    // the answer comes from hashing, which does not care what site the pages came from.
    if host::raises_value_error(&candidate.url) {
        return Err(UrlValueError.into());
    }
    let Some(parser) = find_parser(&candidate.url) else {
        return Ok(());
    };
    let (Some(pages_api_url), Some(parse_pages)) = (parser.pages_api_url, parser.parse_pages)
    else {
        return Ok(());
    };
    let Some(pages_url) = pages_api_url(&candidate.url).filter(|u| !u.is_empty()) else {
        return Ok(());
    };

    // SauceNAO names the page in its own thumbnail URL, but only for Pixiv: the pattern is that
    // index's, and reading another site's thumbnail through it would invent a page number.
    let mut reported: Option<i128> = None;
    if parser.name == pixiv::PARSER.name {
        reported = page_index_from_thumbnail(
            candidate.thumb_url.as_deref(),
            pixiv::illust_id(&candidate.url).as_deref(),
        )?;
    }
    // The page list is fetched even for posts far past the hashing cap. It is one small API call,
    // and it is what makes those posts resolvable at all.
    debug!(
        "{} has {page_count} pages - working out which one matches",
        candidate.url
    );
    let body = match page_info {
        Some(info)
            if info.body.as_deref().is_some_and(|b| !b.is_empty())
                && info.fetched_url.as_deref() == Some(pages_url.as_str()) =>
        {
            // The page list IS the record just fetched: no second request for the same URL.
            info.body.clone()
        }
        _ => remote::fetch_text(net, &pages_url, settings, Some(&candidate.url)).await?,
    };
    let Some(body) = body.filter(|b| !b.is_empty()) else {
        debug!(
            "Could not read the page list for {}; keeping the first page",
            candidate.url
        );
        return Ok(());
    };

    let pages: Vec<Page> = parse_pages(&body, &candidate.url, &Context::default());
    if pages.len() < 2 {
        return Ok(());
    }

    // Some sites name each file by its SHA-256 (pawchive's paths ARE the hash). Then the page
    // holding the local file is simply the one with its hash: certain, and not a single thumbnail
    // downloaded.
    let page_hashes: Vec<Option<&str>> = pages
        .iter()
        .map(|p| p.get("sha256").and_then(Value::as_str))
        .collect();
    if page_hashes.iter().any(|h| h.is_some_and(|h| !h.is_empty())) {
        let local_sha256 = sha256_of_file(local_path);
        if let Some(i) = local_sha256
            .as_deref()
            .and_then(|l| page_hashes.iter().position(|h| *h == Some(l)))
        {
            use_page(
                candidate,
                &pages,
                i,
                page_count,
                "it is the very same file (its SHA-256 matches)",
            );
            return Ok(());
        }
        // Not the same file, but the engine that found it matched ONE of the post's images, and
        // its thumbnail names that image by hash.
        let thumb_url = candidate.thumb_url.clone().unwrap_or_default();
        let named = page_hashes
            .iter()
            .flatten()
            .find(|h| !h.is_empty() && thumb_url.contains(**h))
            .copied();
        if let Some(named) = named {
            let i = page_hashes
                .iter()
                .position(|h| *h == Some(named))
                .unwrap_or(0);
            use_page(
                candidate,
                &pages,
                i,
                page_count,
                "the image the search matched is this one (named by its thumbnail)",
            );
            return Ok(());
        }
    }

    if let Some(r) = reported
        && usize::try_from(r).map_or(true, |r| r >= pages.len())
    {
        // The post has been edited since SauceNAO indexed it.
        info!(
            "{}: SauceNAO reports page {} but the post now has {} - its index is stale, ignoring \
             it",
            candidate.url,
            r.saturating_add(1),
            pages.len()
        );
        reported = None;
    }

    let local_size: Option<(i64, i64)> =
        image_dimensions(Source::Path(std::path::Path::new(local_path)))?
            .map(|(w, h)| (w as i64, h as i64));
    let page_sizes: Vec<PageSize> = pages
        .iter()
        .map(|p| Some((int_of(p, "width"), int_of(p, "height"))))
        .collect();

    if let Some(r) = reported
        && dimensions_contradict_page(local_size, &page_sizes, r as usize)
    {
        info!(
            "{}: SauceNAO reports page {}, but that page isn't the local image's shape - ignoring \
             it",
            candidate.url,
            r + 1
        );
        reported = None;
    }

    // Cheapest possible answer: if exactly one page is the local file's size and that is the page
    // SauceNAO named, two independent sources agree and there is nothing left to establish.
    if let (Some(r), Some(size)) = (reported, local_size)
        && dimensions_identify_page(local_size, &page_sizes) == Some(r as usize)
    {
        use_page(
            candidate,
            &pages,
            r as usize,
            page_count,
            &format!(
                "SauceNAO's thumbnail names page {} and it's the only page at {}x{}",
                r + 1,
                size.0,
                size.1
            ),
        );
        return Ok(());
    }

    if page_count > MAX_PAGES_TO_COMPARE as i64 {
        // Too many pages to ask which one matches, but not too many to ask whether a particular
        // one does: that costs a single download however big the post is.
        let suspect = dimensions_identify_page(local_size, &page_sizes);
        let mut named: Vec<usize> = Vec::new();
        for i in [reported.map(|r| r as usize), suspect]
            .into_iter()
            .flatten()
        {
            if !named.contains(&i) {
                named.push(i);
            }
        }
        for index in named {
            let page = &pages[index];
            let url = first_non_empty(&[url_of(page, "small"), url_of(page, "regular")]);
            let data = remote::download_bytes(
                net,
                url,
                settings.hydrus.timeout,
                Some(&candidate.url),
                None,
                None,
            )
            .await?
            .filter(|d| !d.is_empty());
            let Some(data) = data else { continue };
            let check = check_page(Source::Path(std::path::Path::new(local_path)), &data)?;
            if check.confirmed {
                use_page(
                    candidate,
                    &pages,
                    index,
                    page_count,
                    &format!(
                        "page {} was checked directly and {}",
                        index + 1,
                        check.reason
                    ),
                );
                return Ok(());
            }
            debug!(
                "{}: page {} isn't it - {}",
                candidate.url,
                index + 1,
                check.reason
            );
        }
        info!(
            "{} has {page_count} pages - too many to compare them all, and no single page could \
             be confirmed, so keeping the first",
            candidate.url
        );
        return Ok(());
    }

    let mut thumbnails: Vec<(usize, Vec<u8>)> = Vec::new();
    for (index, page) in pages.iter().take(MAX_PAGES_TO_COMPARE).enumerate() {
        let Some(thumb_url) = first_non_empty(&[url_of(page, "small"), url_of(page, "regular")])
        else {
            continue;
        };
        let data = remote::download_bytes(
            net,
            Some(thumb_url),
            settings.hydrus.timeout,
            Some(&candidate.url),
            None,
            None,
        )
        .await?
        .filter(|d| !d.is_empty());
        if let Some(data) = data {
            thumbnails.push((index, data));
        }
    }

    let found = choose_matching_page(Source::Path(std::path::Path::new(local_path)), &thumbnails)?;

    if let Some(m) = found.as_ref().filter(|m| m.confident) {
        // Hashing wins outright where it has an answer, including against SauceNAO: the one
        // measured disagreement was a stale index, and hashing had it right.
        if let Some(r) = reported
            && r != m.index as i128
        {
            info!(
                "{}: SauceNAO's thumbnail names page {} but the images say page {} - going with \
                 the images",
                candidate.url,
                r + 1,
                m.index + 1
            );
        }
        use_page(candidate, &pages, m.index, page_count, &m.reason);
        return Ok(());
    }

    let Some(reported) = reported else {
        if let Some(m) = &found {
            info!("Keeping page 1 of {} - {}", candidate.url, m.reason);
        }
        return Ok(());
    };

    // Hashing could not choose. SauceNAO's claim is worth acting on only where the refusal was a
    // tie between pages that look alike: the case it is uniquely placed to settle. When nothing in
    // the post resembles the local file, the post is the wrong one or the image has been
    // re-cropped, and a page number from anywhere is a guess.
    if let Some(m) = &found
        && !m.indistinguishable
    {
        info!(
            "Keeping page 1 of {} - {}, so SauceNAO's page {} isn't worth acting on either",
            candidate.url,
            m.reason,
            reported + 1
        );
        return Ok(());
    }

    let why = if found.is_some() {
        format!(
            "the pages are too alike to tell apart, so going with the page SauceNAO's thumbnail \
             names ({})",
            reported + 1
        )
    } else {
        format!(
            "SauceNAO's thumbnail names page {} and the pages couldn't be compared",
            reported + 1
        )
    };
    use_page(candidate, &pages, reported as usize, page_count, &why);
    Ok(())
}
