//! ascii2d reverse image search (`core/ascii2d.py`).
//!
//! ascii2d has no JSON API, so Python uploads the file and scrapes the HTML. Two searches run per
//! image: `/search/color/{hash}` (a colour histogram, strong at exact Pixiv and Twitter reposts)
//! and `/search/bovw/{hash}` (feature search, looser). ascii2d reports no similarity, so
//! candidates get an ordinal score that steps down from a start value per kind.
//!
//! This is the half that reads the answers: [`parse_results`] is `parse_results`, and
//! [`search_from_responses`] is the part of `search` after the colour page (and, when there was
//! one, the feature page) is in hand. The two requests, the session headers, the hard timeout and
//! the process-wide `_blocked` latch are the engine client's (Phase 3): a
//! [`Ascii2dError::Blocked`] is the signal to set the latch, and the feature page is fetched from
//! [`bovw_url`] only when that returns one.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use regex::Regex;
use scraper::{ElementRef, Selector};
use serde::Serialize;
use tracing::{debug, error, warn};

use super::client::{Failure, OnTick, multipart_upload, read_upload, response_text, ticking};
use super::session::{Session, SessionRequest};
use super::{Response, extract_dimensions};
use crate::boorus::parsed::{classes_of, soup_of, text_stripped};
use crate::hard_timeout::HardTimeoutError;
use crate::net::{Method, NetClient};
use crate::{pycompat, pyurl};

pub const ASCII2D_URL: &str = "https://ascii2d.net/";
pub const SEARCH_FILE_URL: &str = "https://ascii2d.net/search/file";
pub const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Colour search is the more precise of the two; start just under a typical IQDB best match.
pub const COLOR_SIMILARITY_START: f64 = 86.0;
pub const BOVW_SIMILARITY_START: f64 = 80.0;
pub const SIMILARITY_STEP: f64 = 2.0;
pub const MAX_RESULTS_PER_SEARCH: usize = 6;

/// Cloudflare's interstitial. Checked so an ordinary 403 is not mistaken for a bot challenge.
pub const CHALLENGE_MARKERS: [&str; 4] = [
    "just a moment",
    "cf-browser-verification",
    "cf_chl_",
    "challenge-platform",
];

/// The shown message when ascii2d is behind a Cloudflare challenge.
const BLOCKED_MESSAGE: &str = "ascii2d is behind a Cloudflare bot check and won't accept uploads from this app. Nothing is wrong with your setup and there is no setting that fixes it - turn ascii2d off under Settings > Engine until the site stops challenging.";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ascii2dMatch {
    pub url: String,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source_name: Option<String>,
    /// "color" or "bovw".
    pub search_kind: String,
}

/// `Ascii2dError` and its subclass `Ascii2dBlockedError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ascii2dError {
    /// `Ascii2dError`: any failure, with its user-facing message.
    Failed(String),
    /// `Ascii2dBlockedError`: ascii2d answered with a Cloudflare bot challenge. Nothing about the
    /// request is wrong and nothing the user can configure fixes it, so the client latches it for
    /// the rest of the batch and stops asking.
    Blocked,
}

impl std::fmt::Display for Ascii2dError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ascii2dError::Failed(message) => f.write_str(message),
            Ascii2dError::Blocked => f.write_str(BLOCKED_MESSAGE),
        }
    }
}

impl std::error::Error for Ascii2dError {}

static COLOR_HASH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/search/color/([0-9a-f]+)").expect("colour hash regex"));
static ANY_ELEMENT: LazyLock<Selector> = LazyLock::new(|| Selector::parse("*").expect("selector"));
static ANY_A: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a").expect("selector"));
static ANY_IMG: LazyLock<Selector> = LazyLock::new(|| Selector::parse("img").expect("selector"));

/// `_looks_like_a_challenge`: whether the first 4000 characters of the page carry a marker.
fn looks_like_a_challenge(text: &str) -> bool {
    let head: String = text.chars().take(4000).collect::<String>().to_lowercase();
    CHALLENGE_MARKERS.iter().any(|marker| head.contains(marker))
}

/// `_hash_from_url`: the colour search's hash in its URL.
pub fn hash_from_url(url: &str) -> Option<String> {
    COLOR_HASH_RE.captures(url).map(|m| m[1].to_owned())
}

/// Where the feature search for a colour page lives, or `None` when the colour URL carries no
/// hash (then `search` asks no second question).
pub fn bovw_url(color_url: &str) -> Option<String> {
    bovw_url_on(ASCII2D_URL, color_url)
}

/// [`bovw_url`] against another address: the client asks the host it was given, which a test
/// serves from a local server. `base` ends in `/`, as [`ASCII2D_URL`] does.
pub fn bovw_url_on(base: &str, color_url: &str) -> Option<String> {
    hash_from_url(color_url).map(|hash| format!("{base}search/bovw/{hash}"))
}

/// The parts of `search` after the colour page is in hand.
///
/// `color` is the answer to the upload POST; `bovw` is the answer to the GET of [`bovw_url`], or
/// `None` when no such request was made or it failed (a failed feature search keeps the colour
/// results). The result is the colour matches followed by the feature matches, a URL kept once
/// at its higher score.
pub fn search_from_responses(
    color: &Response<'_>,
    bovw: Option<&Response<'_>>,
) -> Result<Vec<Ascii2dMatch>, Ascii2dError> {
    if color.status == 413 {
        return Err(Ascii2dError::Failed(
            "ascii2d rejected the image as too large, even after downscaling".to_owned(),
        ));
    }
    if color.status != 200 {
        if looks_like_a_challenge(color.text) {
            error!(
                "ascii2d served a Cloudflare bot challenge (HTTP {}) instead of results - skipping it for the rest of this search",
                color.status
            );
            return Err(Ascii2dError::Blocked);
        }
        return Err(Ascii2dError::Failed(format!(
            "ascii2d returned HTTP {}",
            color.status
        )));
    }

    if !color.text.to_lowercase().contains("ascii2d") && !color.text.contains("item-box") {
        return Err(Ascii2dError::Failed(
            "ascii2d's response didn't look like a real results page (possible block/CAPTCHA/rate limit)"
                .to_owned(),
        ));
    }

    let mut matches = parse_results(color.text, color.url, "color");
    debug!(
        "ascii2d colour search parsed into {} match(es)",
        matches.len()
    );

    if let (Some(bovw_url), Some(bovw)) = (bovw_url(color.url), bovw)
        && bovw.status == 200
    {
        let page_url = if bovw.url.is_empty() {
            &bovw_url
        } else {
            bovw.url
        };
        let found = parse_results(bovw.text, page_url, "bovw");
        debug!(
            "ascii2d feature search parsed into {} match(es)",
            found.len()
        );
        matches.extend(found);
    }
    Ok(dedupe(matches))
}

/// `parse_results`: one results page into matches. `kind` is "color" or anything else for the
/// feature search, which starts lower.
pub fn parse_results(html: &str, page_url: &str, kind: &str) -> Vec<Ascii2dMatch> {
    let soup = soup_of(html);
    let start = if kind == "color" {
        COLOR_SIMILARITY_START
    } else {
        BOVW_SIMILARITY_START
    };
    let mut matches = Vec::new();
    let mut rank = 0usize;

    for el in soup.select(&ANY_ELEMENT) {
        if !classes_of(el).contains(&"item-box") {
            continue;
        }
        let box_ = el;
        let source_urls = source_urls(box_);
        if source_urls.is_empty() {
            continue; // the uploaded image itself, or a box with no sources
        }
        if rank >= MAX_RESULTS_PER_SEARCH {
            break;
        }

        let thumb_url = box_
            .select(&ANY_IMG)
            .next()
            .and_then(|img| img.value().attr("src"))
            .filter(|src| !src.is_empty())
            .map(|src| {
                let base = if page_url.is_empty() {
                    ASCII2D_URL
                } else {
                    page_url
                };
                pyurl::urljoin(base, pycompat::strip(src))
            });

        let (width, height) = extract_dimensions(&text_stripped(box_, " "));
        let similarity = (start - rank as f64 * SIMILARITY_STEP).max(50.0);
        rank += 1;

        for url in source_urls {
            matches.push(Ascii2dMatch {
                source_name: host_label(&url),
                url,
                thumb_url: thumb_url.clone(),
                similarity,
                width,
                height,
                search_kind: kind.to_owned(),
            });
        }
    }
    matches
}

/// `_source_urls`: external source links from one result box, in document order. ascii2d's own
/// thumbnail, hash and search links are skipped.
fn source_urls(box_: ElementRef<'_>) -> Vec<String> {
    let detail = box_
        .select(&ANY_ELEMENT)
        .find(|d| classes_of(*d).contains(&"detail-box"))
        .unwrap_or(box_);
    let mut urls: Vec<String> = Vec::new();
    for a in detail.select(&ANY_A) {
        let href = pycompat::strip(a.value().attr("href").unwrap_or(""));
        let href = match href.strip_prefix("//") {
            Some(_) => format!("https:{href}"),
            None => href.to_owned(),
        };
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            continue;
        }
        let host = pyurl::urlparse(&href, "").netloc.to_lowercase();
        if host.contains("ascii2d.net") {
            continue;
        }
        if urls.contains(&href) {
            continue;
        }
        urls.push(href);
    }
    urls
}

/// `_host_label`: the host of a URL, lower-cased, without a leading `www.`; `None` for none.
fn host_label(url: &str) -> Option<String> {
    let host = pyurl::urlparse(url, "").netloc.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_owned())
}

/// `_dedupe`: the same source URL can appear in both searches; keep the higher score, at the
/// position of the first.
pub fn dedupe(matches: Vec<Ascii2dMatch>) -> Vec<Ascii2dMatch> {
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut best: Vec<Ascii2dMatch> = Vec::new();
    for m in matches {
        match at.get(&m.url) {
            Some(&i) if m.similarity > best[i].similarity => best[i] = m,
            Some(_) => {}
            None => {
                at.insert(m.url.clone(), best.len());
                best.push(m);
            }
        }
    }
    best
}

/// The request half of `core/ascii2d.py`: `search`, `is_blocked` and `reset_blocked_flag`.
///
/// One upload POST (a `requests.Session`: see [`super::session`], which keeps its cookies and
/// follows its redirects), then, when the answer names a colour search, a GET of the feature
/// search at [`bovw_url_on`] with the same session. What the answers mean is
/// [`search_from_responses`]. The `_blocked` latch Python keeps in a module global is the
/// client's; clones share it. Both addresses come from the constructor, so a test serves them
/// from a local server; `new` uses the real one.
#[derive(Clone)]
pub struct Ascii2dClient {
    net: NetClient,
    base: String,
    blocked: Arc<AtomicBool>,
}

impl Ascii2dClient {
    /// The real ascii2d.net.
    pub fn new(net: NetClient) -> Self {
        Self::with_base(net, ASCII2D_URL)
    }

    /// The same client against another address (a local https server in tests), `base` ending in
    /// `/`. Not reachable from `config.json`, the environment or a flag.
    pub fn with_base(net: NetClient, base: &str) -> Self {
        Ascii2dClient {
            net,
            base: base.to_owned(),
            blocked: Arc::new(AtomicBool::new(false)),
        }
    }

    /// `is_blocked`: ascii2d has answered with a Cloudflare challenge and the batch should stop
    /// asking.
    pub fn is_blocked(&self) -> bool {
        self.blocked.load(Ordering::SeqCst)
    }

    /// `reset_blocked_flag`: cleared when a new search starts, in case the block has lifted.
    pub fn reset_blocked_flag(&self) {
        self.blocked.store(false, Ordering::SeqCst);
    }

    /// `search`: upload to ascii2d and return colour + feature matches, colour first.
    ///
    /// `timeout` is both the per-read timeout and a deadline over each exchange (the upload with
    /// its redirects, then the feature search separately), as `run_with_hard_timeout(..., timeout)`
    /// is around each in Python. `on_tick` is the UI countdown, see [`OnTick`].
    pub async fn search(
        &self,
        image_path: &Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> Result<Vec<Ascii2dMatch>, Ascii2dError> {
        debug!("Uploading {} to ascii2d", image_path.display());
        let (data, filename) = read_upload(image_path)
            .await
            .map_err(|e| Ascii2dError::Failed(format!("Could not read image: {}", e.0)))?;
        let upload = multipart_upload("file", &filename, &data);
        let mut session = Session::new(
            self.net.clone(),
            vec![
                ("User-Agent", USER_AGENT.to_owned()),
                ("Referer", self.base.clone()),
            ],
        );
        let file_url = format!("{}search/file", self.base);
        let started = Instant::now();
        let sent = ticking(
            "Waiting on ascii2d",
            timeout,
            on_tick.clone(),
            session.send(
                SessionRequest {
                    method: Method::Post,
                    url: &file_url,
                    upload: Some(&upload),
                },
                timeout,
                started,
            ),
        )
        .await;
        let color = match sent {
            Ok(resp) => resp,
            Err(Failure::Deadline(HardTimeoutError(why))) => {
                error!(
                    "ascii2d request for {} exceeded the hard deadline",
                    image_path.display()
                );
                return Err(Ascii2dError::Failed(format!(
                    "ascii2d request timed out: {why}"
                )));
            }
            Err(Failure::Transport(why)) => {
                error!("ascii2d request failed for {}", image_path.display());
                return Err(Ascii2dError::Failed(format!(
                    "ascii2d request failed: {why}"
                )));
            }
        };
        let color_text = response_text(&color);
        let color_url = color.url.as_str().to_owned();
        let color_answer = Response {
            status: color.status.as_u16(),
            url: &color_url,
            text: &color_text,
        };
        // Everything up to the feature search: the status mapping, the challenge check, the
        // sanity check of the page. An error here is the answer.
        let colour_only = match search_from_responses(&color_answer, None) {
            Ok(matches) => matches,
            Err(err) => {
                if err == Ascii2dError::Blocked {
                    self.blocked.store(true, Ordering::SeqCst);
                }
                return Err(err);
            }
        };
        let Some(bovw_url) = bovw_url_on(&self.base, &color_url) else {
            return Ok(colour_only);
        };

        let started = Instant::now();
        let fetched = ticking(
            "Waiting on ascii2d (feature)",
            timeout,
            on_tick,
            session.send(
                SessionRequest {
                    method: Method::Get,
                    url: &bovw_url,
                    upload: None,
                },
                timeout,
                started,
            ),
        )
        .await;
        match fetched {
            Ok(bovw) => {
                let bovw_text = response_text(&bovw);
                let bovw_final = bovw.url.as_str().to_owned();
                let bovw_answer = Response {
                    status: bovw.status.as_u16(),
                    url: &bovw_final,
                    text: &bovw_text,
                };
                search_from_responses(&color_answer, Some(&bovw_answer))
            }
            Err(failure) => {
                // Colour results are already in hand; a failed feature search must not throw
                // those away.
                warn!("ascii2d feature search failed (colour results kept)");
                debug!("ascii2d feature search failure: {failure:?}");
                Ok(colour_only)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head><title>ascii2d</title></head><body>
    <div class="item-box"><div class="image-box"><img src="/thumbnail/query.jpg"></div>
      <div class="hash">aabbcc</div><small>800x600 JPEG</small></div>
    <div class="item-box"><div class="image-box"><img src="/thumbnail/hit.jpg"></div>
      <div class="detail-box"><h6>
        <a href="https://www.pixiv.net/artworks/12345">pixiv</a>
        <a href="//twitter.com/i/web/status/99">twitter</a>
        <a href="https://ascii2d.net/search/color/aabbcc">more</a>
        <a href="https://www.pixiv.net/artworks/12345">again</a>
        <a href="/relative">rel</a><a>none</a>
      </h6><small>1200x1600 PNG</small></div></div></body></html>"#;

    #[test]
    fn the_query_box_is_skipped_and_every_source_link_is_a_match() {
        let got = parse_results(PAGE, "https://ascii2d.net/search/color/aabbcc", "color");
        let urls: Vec<_> = got.iter().map(|m| m.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://www.pixiv.net/artworks/12345",
                "https://twitter.com/i/web/status/99"
            ]
        );
        assert!(
            got.iter()
                .all(|m| m.similarity == 86.0 && m.search_kind == "color")
        );
        assert_eq!(got[0].source_name.as_deref(), Some("pixiv.net"));
        assert_eq!(
            got[1].thumb_url.as_deref(),
            Some("https://ascii2d.net/thumbnail/hit.jpg")
        );
        assert_eq!((got[0].width, got[0].height), (Some(1200), Some(1600)));
    }

    #[test]
    fn scores_step_down_per_box_and_floor_at_fifty() {
        let boxes: String = (0..9)
            .map(|i| format!("<div class=item-box><a href=\"https://s.example/{i}\">x</a></div>"))
            .collect();
        let got = parse_results(&format!("<html>{boxes}"), "", "bovw");
        let scores: Vec<_> = got.iter().map(|m| m.similarity).collect();
        // Six boxes at most, 80, 78, ... 70.
        assert_eq!(scores, [80.0, 78.0, 76.0, 74.0, 72.0, 70.0]);
        // A deep rank would floor at 50; MAX_RESULTS_PER_SEARCH keeps rank under it.
        assert_eq!(
            (COLOR_SIMILARITY_START - 5.0 * SIMILARITY_STEP).max(50.0),
            76.0
        );
    }

    #[test]
    fn a_box_without_a_detail_box_is_read_whole_and_a_box_with_no_source_is_not_counted() {
        let html = "<html><div class=item-box><a href=\"https://a.example/1\">x</a></div>\
                    <div class=item-box><a href=\"/rel\">x</a></div>\
                    <div class=item-box><a href=\"https://b.example/2\">x</a></div>";
        let got = parse_results(html, "", "color");
        assert_eq!(got.len(), 2);
        // The empty box took no rank.
        assert_eq!((got[0].similarity, got[1].similarity), (86.0, 84.0));
        assert_eq!(got[0].thumb_url, None);
    }

    #[test]
    fn the_hash_gives_the_feature_url() {
        assert_eq!(
            hash_from_url("https://ascii2d.net/search/color/AaBb09"),
            Some("AaBb09".into())
        );
        assert_eq!(
            bovw_url("https://ascii2d.net/search/color/aabbcc"),
            Some("https://ascii2d.net/search/bovw/aabbcc".into())
        );
        assert_eq!(bovw_url("https://ascii2d.net/search/file"), None);
        assert_eq!(bovw_url(""), None);
    }

    fn resp<'a>(status: u16, url: &'a str, text: &'a str) -> Response<'a> {
        Response { status, url, text }
    }

    #[test]
    fn a_challenge_is_blocked_and_any_other_failure_is_not() {
        let challenge = "<html><title>Just a moment...</title>challenge-platform</html>";
        assert_eq!(
            search_from_responses(&resp(403, "", challenge), None),
            Err(Ascii2dError::Blocked)
        );
        assert_eq!(
            search_from_responses(&resp(403, "", "nope"), None),
            Err(Ascii2dError::Failed("ascii2d returned HTTP 403".into()))
        );
        assert_eq!(
            search_from_responses(&resp(413, "", "x"), None)
                .unwrap_err()
                .to_string(),
            "ascii2d rejected the image as too large, even after downscaling"
        );
        // Only the first 4000 characters are looked at.
        let late = format!("{}just a moment", "x".repeat(4000));
        assert_eq!(
            search_from_responses(&resp(503, "", &late), None),
            Err(Ascii2dError::Failed("ascii2d returned HTTP 503".into()))
        );
        assert!(
            Ascii2dError::Blocked
                .to_string()
                .starts_with("ascii2d is behind a Cloudflare bot check")
        );
    }

    #[test]
    fn a_200_that_is_not_ascii2d_is_an_error() {
        let err = search_from_responses(&resp(200, "", "<html>hello</html>"), None).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("ascii2d's response didn't look like a real results page")
        );
        // "ascii2d" in any case, or an item box, is enough.
        assert!(
            search_from_responses(&resp(200, "", "<p>ASCII2D"), None)
                .unwrap()
                .is_empty()
        );
        assert!(
            search_from_responses(&resp(200, "", "<p>item-box"), None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn feature_results_follow_colour_ones_and_a_repeated_url_keeps_its_higher_score() {
        let url = "https://ascii2d.net/search/color/aabbcc";
        let bovw_page = "<html><body>\
            <div class=item-box><a href=\"https://www.pixiv.net/artworks/12345\">p</a></div>\
            <div class=item-box><a href=\"https://new.example/1\">n</a></div></body></html>";
        let got =
            search_from_responses(&resp(200, url, PAGE), Some(&resp(200, "", bovw_page))).unwrap();
        let rows: Vec<_> = got
            .iter()
            .map(|m| (m.url.as_str(), m.similarity, m.search_kind.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("https://www.pixiv.net/artworks/12345", 86.0, "color"),
                ("https://twitter.com/i/web/status/99", 86.0, "color"),
                ("https://new.example/1", 78.0, "bovw"),
            ]
        );
        // A failed or non-200 feature search keeps the colour results; so does no hash.
        let colour_only =
            search_from_responses(&resp(200, url, PAGE), Some(&resp(503, "", bovw_page))).unwrap();
        assert_eq!(colour_only.len(), 2);
        let no_hash = search_from_responses(
            &resp(200, "https://ascii2d.net/search/file", PAGE),
            Some(&resp(200, "", bovw_page)),
        )
        .unwrap();
        assert_eq!(no_hash.len(), 2);
    }

    #[test]
    fn dedupe_replaces_in_place_only_for_a_strictly_higher_score() {
        let m = |url: &str, similarity: f64, kind: &str| Ascii2dMatch {
            url: url.into(),
            thumb_url: None,
            similarity,
            width: None,
            height: None,
            source_name: None,
            search_kind: kind.into(),
        };
        let got = dedupe(vec![
            m("a", 70.0, "color"),
            m("b", 60.0, "color"),
            m("a", 80.0, "bovw"),
            m("b", 60.0, "bovw"),
        ]);
        let rows: Vec<_> = got
            .iter()
            .map(|m| (m.url.as_str(), m.similarity, m.search_kind.as_str()))
            .collect();
        assert_eq!(rows, [("a", 80.0, "bovw"), ("b", 60.0, "color")]);
    }

    #[test]
    fn a_host_label_drops_www_and_keeps_the_port() {
        assert_eq!(
            host_label("https://WWW.Pixiv.net/x"),
            Some("pixiv.net".into())
        );
        assert_eq!(
            host_label("https://a.example:8080/x"),
            Some("a.example:8080".into())
        );
        assert_eq!(host_label("mailto:x"), None);
    }
}
