//! Is a match's page still there? Status codes, soft 404s, redirects to a site's index, and
//! Sankaku's API: one definition of "gone" (`core/availability.py`).
//!
//! Only a definite 404/410, a soft 404 or a redirect to a site's "no such post" page marks a
//! match gone. Anything ambiguous (a network error, a timeout, a 403, a 5xx) is `None`, which
//! every caller treats as alive. The network functions are `async` and take the [`NetClient`]
//! they send through; timeouts are seconds as `f64`, as the config holds them.
//!
//! Where the cut is: the sweep calls `normalize_url_for_hydrus`, which lives in the Hydrus import
//! (Phase 4); only that pure function is ported, in `hydrus_url`.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use std::task::Poll;

use http::HeaderMap;
use regex::Regex;
use tracing::{debug, info, warn};

use crate::boorus::fetch::SettingsSink;
use crate::boorus::fetch::capture_rotated_deviantart_cookies;
use crate::config::Settings;
use crate::hydrus_url::normalize_url_for_hydrus;
use crate::model::Candidate;
use crate::net::site::{self, SiteCredentials};
use crate::net::{BodyProbe, Cookies, ErrorKind, NetClient, NetError, NetResponse, Redirects};
use crate::paths::Paths;
use crate::progress::{OnTick, ticked};
use crate::pyresponse::PyResponse;
use crate::pyurl;
use crate::remote::{self, Raised, encode_fault, set_header, ua_headers};
use crate::timeutil::now_epoch;

/// Hosts with no `boorus/` parser (so nothing else establishes whether their content still
/// exists) that are nonetheless worth an explicit availability check. An opt-in list rather than
/// "check every unknown host": each entry costs an extra request per candidate.
pub const AVAILABILITY_CHECK_HOSTS: [&str; 8] = [
    "deviantart.com",
    "fav.me",
    "sankakucomplex.com",
    "yande.re",
    "konachan.com",
    "konachan.net",
    "reactor.cc",
    "joyreactor.com",
];

/// Whether the URL names one of [`AVAILABILITY_CHECK_HOSTS`] anywhere in it (a substring test, as
/// in Python).
pub fn should_availability_check(url: &str) -> bool {
    let lowered = url.to_lowercase();
    AVAILABILITY_CHECK_HOSTS
        .iter()
        .any(|host| lowered.contains(host))
}

/// The plain (watermarked) sibling of a `canonicalize_url`-rewritten JoyReactor/reactor.cc `full/`
/// URL: `^(https?://[^/]+/pics/post/)full/(.+)$`, case-insensitive. `.` does not match a newline,
/// and `$` matches before a final one.
static REACTOR_FULL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(https?://[^/]+/pics/post/)full/(.+?)\n?$").expect("reactor full regex")
});

/// The URL a definitely gone `full/` file falls back to, if `url` has that shape.
fn reactor_fallback_url(url: &str) -> Option<String> {
    let found = REACTOR_FULL_RE.captures(url)?;
    Some(format!("{}{}", &found[1], &found[2]))
}

/// `canonicalize_url` rewrites a JoyReactor/reactor.cc match to its `full/` (unwatermarked) file
/// before anyone has confirmed that file exists. If the check just found it definitively gone,
/// that is not the post being gone: the ORIGINAL (watermarked) file the site served very likely
/// still is, so fall back to that rather than reporting the whole match dead.
///
/// Returns `None` (unknown: the fallback URL itself has not been checked) when a fallback
/// happens, otherwise `available` unchanged.
pub fn reactor_full_fallback(candidate: &mut Candidate, available: Option<bool>) -> Option<bool> {
    if available != Some(false) || candidate.url.is_empty() {
        return available;
    }
    let Some(fallback) = reactor_fallback_url(&candidate.url) else {
        return available;
    };
    info!(
        "{}: full-res file is gone, falling back to {fallback}",
        candidate.url
    );
    candidate.url = fallback;
    None
}

/// Concurrency for the up-front availability sweep: several hosts at once, bounded.
pub const AVAILABILITY_SWEEP_WORKERS: usize = 5;

/// HTTP statuses that mean the resource is definitively gone, as opposed to temporarily
/// unreachable. Deliberately narrow: a 403 is often hotlink protection or a bot check, a 5xx is a
/// server problem, and a timeout says nothing at all.
pub const DEFINITELY_GONE_STATUSES: [u16; 2] = [404, 410];

/// Only this much of the body is scanned for a soft-404 marker.
pub const SOFT_404_SCAN_BYTES: usize = 60_000;

/// Sites that serve a "this post was deleted" page with HTTP 200, keyed by a fragment of the URL
/// (a substring test, in this order), with the lower-case markers to look for in the page body.
const SOFT_404_MARKERS: [(&str, &[&str]); 5] = [
    (
        "sankakucomplex.com",
        &[
            "<title>no content | sankaku",
            "/posts/show_empty",
            "nothing is visible to you here",
        ],
    ),
    (
        "pixiv.net",
        &[
            "該当作品は削除されたか",
            "存在しない作品idです",
            "work has been deleted or the id does not exist",
        ],
    ),
    ("yande.re", &["this post was deleted", "/assets/stubs/"]),
    ("konachan.com", &["this post was deleted", "/assets/stubs/"]),
    ("konachan.net", &["this post was deleted", "/assets/stubs/"]),
];

/// Sites that answer a missing post by REDIRECTING to a generic "nothing here" page: the path
/// fragments that mean it, found in where the request landed but not in where it started.
const SOFT_404_REDIRECTS: [(&str, &[&str]); 5] = [
    (
        "sankakucomplex.com",
        &["/errors/not_found", "/posts/show_empty"],
    ),
    ("gelbooru.com", &["s=list"]),
    ("safebooru.org", &["s=list"]),
    ("rule34.xxx", &["s=list"]),
    ("xbooru.com", &["s=list"]),
];

/// The markers for the first site whose fragment the URL contains, or none.
pub fn soft_404_markers_for(url: &str) -> &'static [&'static str] {
    let lowered = url.to_lowercase();
    SOFT_404_MARKERS
        .iter()
        .find(|(fragment, _)| lowered.contains(fragment))
        .map_or(&[], |(_, markers)| *markers)
}

/// Whether this site needs its POST PAGE checked rather than its image file, because that is the
/// only place its "gone" signal appears: sites that serve error text with HTTP 200 and sites that
/// redirect to a generic empty page.
pub fn has_soft_404_detection(url: &str) -> bool {
    let lowered = url.to_lowercase();
    SOFT_404_MARKERS
        .iter()
        .any(|(fragment, _)| lowered.contains(fragment))
        || SOFT_404_REDIRECTS
            .iter()
            .any(|(fragment, _)| lowered.contains(fragment))
}

/// Landing on a site's BROWSE INDEX means the request never reached a post. Compared exactly on
/// the final URL's path, never as a substring: the index is `/en/posts` and a real post is
/// `/en/posts/4100422`.
const INDEX_REDIRECT_PATHS: [(&str, &[&str]); 1] =
    [("sankakucomplex.com", &["/posts", "/en/posts", "/", ""])];

/// The index path this request was bounced to, or `None`.
pub fn redirect_to_index(original_url: &str, final_url: Option<&str>) -> Option<String> {
    let final_url = final_url.filter(|u| !u.is_empty())?;
    let lowered_original = original_url.to_lowercase();
    for (fragment, index_paths) in INDEX_REDIRECT_PATHS {
        if !lowered_original.contains(fragment) {
            continue;
        }
        let Ok(parsed) = pyurl::try_urlparse(final_url, "") else {
            return None;
        };
        let path = parsed.path.trim_end_matches('/').to_lowercase();
        if index_paths.contains(&path.as_str()) {
            return Some(if path.is_empty() {
                "/".to_string()
            } else {
                path
            });
        }
    }
    None
}

/// The path fragment that proves this URL landed on a site's generic "no such post" page, or
/// `None`. Matched against the ORIGINAL URL's site, so a site cannot be judged by rules belonging
/// to whatever it redirected to; and the fragment must be in where we LANDED but not where we
/// started, or a Gelbooru list page asked for directly would report itself gone.
pub fn redirect_means_gone(original_url: &str, final_url: Option<&str>) -> Option<&'static str> {
    let final_url = final_url.filter(|u| !u.is_empty())?;
    let lowered_original = original_url.to_lowercase();
    let lowered_final = final_url.to_lowercase();
    for (fragment, path_fragments) in SOFT_404_REDIRECTS {
        if !lowered_original.contains(fragment) {
            continue;
        }
        for path_fragment in path_fragments {
            if lowered_final.contains(path_fragment) && !lowered_original.contains(path_fragment) {
                return Some(path_fragment);
            }
        }
    }
    None
}

static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex"));

/// The `<title>` of a fetched page, for logging: whether a marker miss means "a live post" or "a
/// bot check, age gate or login wall".
pub fn page_title(body: &str) -> String {
    if body.is_empty() {
        return String::new();
    }
    let Some(found) = TITLE_RE.captures(body) else {
        let start: String = body.chars().take(60).collect();
        return format!(
            "<no title; body starts {}>",
            crate::pyrepr::py_repr(&serde_json::Value::String(start))
        );
    };
    crate::pycompat::split(&found[1])
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// Sankaku's post ids changed shape: the new ones are short alphanumeric strings, the old ones
/// were numbers, and every search engine's index is still full of those.
static SANKAKU_POST_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)sankakucomplex\.com/(?:[a-z]{2}/)?posts?/(?:show/)?([A-Za-z0-9]+)")
        .expect("sankaku post id regex")
});
const SANKAKU_API_POST: &str = "https://sankakuapi.com/posts/";
/// A post the API describes as anything else, deleted or held back, is not one the user can open.
const SANKAKU_LIVE_STATUS: &str = "active";

/// Whether this URL cannot resolve for anyone, without asking anything: Sankaku's numeric ids
/// stopped resolving entirely (its API answers "invalid id" for each), so such a URL is a dead
/// link no matter what became of the post behind it.
pub fn url_is_structurally_dead(url: &str) -> bool {
    SANKAKU_POST_ID_RE
        .captures(url)
        .is_some_and(|found| found[1].bytes().all(|b| b.is_ascii_digit()))
}

/// Candidates whose URL could not be opened by anyone, removed.
pub fn drop_structurally_dead(candidates: Vec<Candidate>, filename: &str) -> Vec<Candidate> {
    let before = candidates.len();
    let live: Vec<Candidate> = candidates
        .into_iter()
        .filter(|c| !url_is_structurally_dead(&c.url))
        .collect();
    if live.len() != before {
        info!(
            "{}: dropped {} match(es) whose URL can no longer resolve",
            if filename.is_empty() { "?" } else { filename },
            before - live.len()
        );
    }
    live
}

/// The settings a check may read its credentials from and save a renewed DeviantArt session
/// cookie into (`capture_rotated_deviantart_cookies`). Shared behind a lock because the sweep's
/// workers run at once.
pub struct SettingsAccess<'a> {
    pub settings: &'a Mutex<Settings>,
    pub paths: &'a Paths,
}

impl SettingsAccess<'_> {
    fn credentials(&self) -> SiteCredentials {
        remote::credentials(&self.settings.lock().unwrap())
    }

    fn timeout(&self) -> f64 {
        self.settings.lock().unwrap().search_timeout
    }
}

/// The message `urlparse(url).netloc` raises with (`ValueError`), for the URLs `pyurl` refuses.
pub fn urlparse_message(url: &str) -> String {
    let netloc = pyurl::urlparse(url, "").netloc;
    if netloc.contains('[') != netloc.contains(']') {
        return "Invalid IPv6 URL".to_string();
    }
    if netloc.is_ascii() {
        let host_port = netloc.rsplit_once('@').map_or(netloc.as_str(), |(_, r)| r);
        if let Some((before, bracketed)) = host_port.split_once('[') {
            let (hostname, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
            if !before.is_empty() || (!port.is_empty() && !port.starts_with(':')) {
                return "Invalid IPv6 URL".to_string();
            }
            if hostname.starts_with(['v', 'V']) {
                return "IPvFuture address is invalid".to_string();
            }
            return format!(
                "{} does not appear to be an IPv4 or IPv6 address",
                crate::pyrepr::py_repr(&serde_json::Value::String(hostname.to_string()))
            );
        }
    }
    format!(
        "netloc {} contains invalid characters under NFKC normalization",
        crate::pyrepr::py_repr(&serde_json::Value::String(netloc))
    )
}

/// The exception that left `_do_request` and was not one `requests` raised (so the handler in
/// `_page_says_available` did not catch it), for a body read that failed.
fn body_fault(e: &NetError) -> Raised {
    // A connection that broke is urllib3's `ProtocolError`; a body its `Content-Encoding` could not
    // decode is its `DecodeError`.
    let detail = e.detail();
    let broke = [
        "from connection",
        "connection",
        "incomplete",
        "closed",
        "reset",
    ]
    .iter()
    .any(|word| detail.to_lowercase().contains(word));
    Raised::BodyRead {
        class: if broke {
            "ProtocolError"
        } else {
            "DecodeError"
        },
        message: detail.to_string(),
    }
}

/// `bytes.decode("utf-8", errors="ignore")`.
fn decode_ignoring_errors(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
    }
    out
}

/// Whether a match's source URL still exists. See [`page_says_available`].
///
/// Sankaku gets a second opinion, and only ever a harsher one. Its page can no longer prove a
/// post is THERE (an anonymous visitor is redirected to a login screen with HTTP 200), so a "yes"
/// or a "don't know" from the page means very little, and its API is asked instead. A page that
/// already proved the post gone is believed as it is: the API can add a "gone" verdict, never
/// overturn one, and never costs a request when the page has already settled it.
pub async fn check_url_available(
    net: &NetClient,
    url: Option<&str>,
    timeout: f64,
    referer: Option<&str>,
    on_tick: Option<OnTick<'_>>,
    cookies: Option<Cookies>,
    settings: Option<&SettingsAccess<'_>>,
) -> Result<Option<bool>, Raised> {
    let verdict = page_says_available(
        net,
        url,
        timeout,
        referer,
        on_tick,
        cookies.clone(),
        settings,
    )
    .await?;
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        return Ok(verdict);
    };
    if verdict == Some(false) {
        return Ok(verdict);
    }
    let cookies = cookies
        .filter(|c| !c.is_empty())
        .or_else(|| settings.and_then(|s| site::cookies_for_url(url, &s.credentials())));
    let second = sankaku_post_exists(net, url, timeout, cookies).await;
    Ok(second.or(verdict))
}

/// Whether a Sankaku post is really there, asked of its API. `None` for anything that is not a
/// Sankaku post URL (so the caller carries on with the ordinary check) and for any answer that
/// settles nothing.
///
/// The API answers anonymously and says outright: a NUMERIC id gets HTTP 400 "invalid id" (those
/// ids stopped resolving when Sankaku changed their format, so the URL is dead whatever became of
/// the post); a modern id gets the post, with a `status`. A post can also be there and still be
/// unusable: `status` "active" but `file_url`, `sample_url` and `preview_url` all null and
/// `redirect_to_signup` true. There is nothing to fetch and nothing to look at, so it counts as
/// gone. Configured Sankaku cookies are sent, so a logged-in user gets their own answer.
pub async fn sankaku_post_exists(
    net: &NetClient,
    url: &str,
    timeout: f64,
    cookies: Option<Cookies>,
) -> Option<bool> {
    let found = SANKAKU_POST_ID_RE.captures(url)?;
    let post_id = found[1].to_string();
    let numeric = post_id.bytes().all(|b| b.is_ascii_digit());

    let api = format!("{SANKAKU_API_POST}{post_id}");
    // The user's Sankaku login goes to Sankaku's own API host, as it does in Python (a plain dict
    // follows the request wherever it goes); it stays within that domain.
    let cookies = cookies.map(|c| c.with_scope(["sankakuapi.com"]));
    let resp = match remote::get_request(&api, timeout, None, &ua_headers(), cookies) {
        Ok(req) => net.send(req).await,
        Err(e) => Err(e),
    };
    let resp = match resp {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) => {
            debug!("Sankaku API check failed for {url} ({e}) - treating as unknown");
            return None;
        }
    };

    if resp.status == 400 && numeric {
        debug!("Sankaku: {url} uses a numeric id, which no longer resolves");
        return Some(false);
    }
    if matches!(resp.status, 404 | 410) {
        return Some(false);
    }
    if resp.status != 200 {
        return None; // rate limited, blocked, or down
    }
    let post = resp.json().ok()?;
    let Some(post) = post.as_object() else {
        return Some(false);
    };
    let truthy = |key: &str| post.get(key).is_some_and(crate::pycompat::truthy);
    if !truthy("id") {
        return Some(false);
    }
    // `str(post.get("status") or "").lower()`: a string is itself, and any other truthy value
    // prints as something that is not "active".
    let status = match post.get("status") {
        Some(serde_json::Value::String(s)) => s.to_lowercase(),
        Some(other) if crate::pycompat::truthy(other) => "\u{0}other".to_string(),
        _ => String::new(),
    };
    if !status.is_empty() && status != SANKAKU_LIVE_STATUS {
        debug!("Sankaku: {url} is {status:?} rather than active");
        return Some(false);
    }
    if !["file_url", "sample_url", "preview_url"]
        .iter()
        .any(|field| truthy(field))
    {
        info!(
            "Sankaku: {url} exists but its picture is withheld (redirect_to_signup={}) - nothing \
             to fetch and nothing to look at, so it counts as gone",
            post.get("redirect_to_signup")
                .map_or_else(|| "None".to_string(), crate::pyrepr::py_str)
        );
        return Some(false);
    }
    Some(true)
}

/// Checks whether a match's source URL still exists.
///
/// `Some(true)` if it responded OK, `Some(false)` for a definite "gone", and `None` when the
/// answer is genuinely unknown: a network error, a timeout, a 403 (very often hotlink protection
/// or a bot check), or a server error. Treating "couldn't reach it" as "it's gone" would throw
/// away good matches whenever the network hiccups.
///
/// "Gone" covers a proper 404/410 AND a soft 404: some sites serve a "this post was deleted" page
/// with HTTP 200, so for those a bounded chunk of the page body is scanned for their error text.
/// Uses GET with the body unread unless it is needed (some booru CDNs don't answer HEAD at all),
/// under a wall-clock deadline over the whole exchange.
///
/// Errors are what Python lets escape its `except (requests.RequestException, HardTimeoutError)`:
/// a URL `urlparse` refuses, a header `http.client` cannot encode, a body urllib3 could not read.
pub async fn page_says_available(
    net: &NetClient,
    url: Option<&str>,
    timeout: f64,
    referer: Option<&str>,
    on_tick: Option<OnTick<'_>>,
    cookies: Option<Cookies>,
    settings: Option<&SettingsAccess<'_>>,
) -> Result<Option<bool>, Raised> {
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        return Ok(None);
    };
    // The site's own headers where it has them. Sankaku in particular hides its "No Content"
    // answer from this project's User-Agent and bounces to the browse index instead.
    let creds = settings.map(SettingsAccess::credentials);
    let mut headers: HeaderMap = site::headers_for_url(url, creds.as_ref())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(ua_headers);
    let refused = referer
        .filter(|r| !r.is_empty())
        .filter(|referer| !set_header(&mut headers, "Referer", referer));

    let markers = soft_404_markers_for(url);

    // The whole request, building it included, runs under the ticker: Python's `_do_request` is
    // the thread the ticker watches, so the clearing tick fires however it ends.
    let sent = ticked("Checking availability", timeout, on_tick, async {
        // No time at all: Python's `done.wait(timeout)` gives up before anything can come back,
        // and the request it started is abandoned.
        if timeout.is_nan() || timeout <= 0.0 {
            return Err(Ok(None));
        }
        // `net.request` takes the URL's netloc before anything else, so a URL `urlparse` refuses
        // raises here and not as a `requests` error.
        if pyurl::try_urlparse(url, "").is_err() {
            return Err(Err(Raised::ValueError(urlparse_message(url))));
        }
        let Ok(mut req) = remote::get_request(url, timeout, None, &headers, cookies) else {
            return Err(Ok(None));
        };
        if let Some(referer) = refused {
            // A referer `requests` would refuse fails the request (`None`); one `http.client`
            // cannot encode escapes the handler.
            return Err(encode_fault(referer).map_or(Ok(None), Err));
        }
        req = req.with_redirects(Redirects::Follow);
        req = if markers.is_empty() {
            // `stream=True` and the body never read; 0 is a status no answer has.
            req.with_body_only_on(0)
        } else {
            // Only pull the bounded prefix needed for the marker check, and only from a 200.
            req.with_body_only_on(200).with_probe(BodyProbe {
                chunk: SOFT_404_SCAN_BYTES,
                cap: SOFT_404_SCAN_BYTES,
                done: Arc::new(|_| false),
            })
        };
        Ok(tokio::time::timeout(remote::secs(timeout), net.send(req)).await)
    })
    .await;
    let resp: NetResponse = match sent {
        Err(done) => return done,
        Ok(Err(_elapsed)) => {
            debug!("Availability check for {url} was inconclusive: timed out");
            return Ok(None);
        }
        Ok(Ok(Err(e))) if e.kind() == ErrorKind::Body => return Err(body_fault(&e)),
        Ok(Ok(Err(e))) => {
            debug!("Availability check for {url} was inconclusive: {e}");
            return Ok(None);
        }
        Ok(Ok(Ok(resp))) => resp,
    };

    let status = resp.status.as_u16();
    let final_url = resp.url.to_string();
    let content_type = crate::pyresponse::header(&resp.headers, "content-type").unwrap_or_default();
    let body = if status == 200 && !markers.is_empty() {
        decode_ignoring_errors(&resp.body)
    } else {
        String::new()
    };

    if let Some(access) = settings {
        // Closed already, but a closed Response keeps the cookies it already received. Only ever
        // does anything for deviantart.com/fav.me.
        let mut guard = access.settings.lock().unwrap();
        let mut sink = SettingsSink {
            settings: &mut guard,
            paths: access.paths,
        };
        capture_rotated_deviantart_cookies(url, &PyResponse::from_net(resp), &mut sink);
    }

    if DEFINITELY_GONE_STATUSES.contains(&status) {
        info!("Source URL is gone (HTTP {status}): {url}");
        return Ok(Some(false));
    }

    // Judged on where the request LANDED, not on how it finished: a redirect to a site's generic
    // "nothing here" page ends in a perfectly healthy HTTP 200.
    if let Some(bounced) = redirect_to_index(url, Some(&final_url)) {
        // NOT proof of deletion. Sankaku bounces every post URL to a login page or its index now
        // that the legacy site is behind SSO. Unverifiable is the honest answer.
        info!(
            "Source URL was bounced to the site's browse index ({bounced}) rather than a post - \
             cannot verify it either way, so leaving the match alone: {url}"
        );
        return Ok(None);
    }
    if let Some(landed_on) = redirect_means_gone(url, Some(&final_url)) {
        info!(
            "Source URL redirected to {landed_on}, the site's \"no such post\" page - treating \
             as gone: {url}"
        );
        return Ok(Some(false));
    }

    if (200..400).contains(&status) {
        if !markers.is_empty() && !body.is_empty() {
            let lowered = body.to_lowercase();
            for marker in markers {
                if lowered.contains(&marker.to_lowercase()) {
                    info!(
                        "Source URL returned HTTP {status} but its page says the content was \
                         deleted (matched {marker:?}): {url}"
                    );
                    return Ok(Some(false));
                }
            }
        }
        if markers.is_empty() {
            debug!(
                "Availability check for {url}: HTTP {status}, no body markers defined for this \
                 host - treating as available"
            );
        } else {
            debug!(
                "Availability check for {url}: HTTP {status}, scanned {} bytes against {} \
                 marker(s), none matched - treating as available. title={:?} content_type={:?}",
                body.len(),
                markers.len(),
                page_title(&body),
                content_type
            );
        }
        return Ok(Some(true));
    }
    debug!("Availability check for {url} inconclusive (HTTP {status}) - not treating as gone");
    Ok(None)
}

/// Runs `jobs` with at most `limit` in flight at once, on this task, and returns each result in
/// the order it finished.
pub(crate) async fn run_bounded<'f, T>(
    jobs: Vec<Pin<Box<dyn Future<Output = T> + 'f>>>,
    limit: usize,
) -> Vec<T> {
    let mut queue = jobs.into_iter();
    let mut active: Vec<Pin<Box<dyn Future<Output = T> + 'f>>> = Vec::new();
    let mut done = Vec::new();
    std::future::poll_fn(|cx| {
        loop {
            while active.len() < limit.max(1) {
                match queue.next() {
                    Some(job) => active.push(job),
                    None => break,
                }
            }
            if active.is_empty() {
                return Poll::Ready(());
            }
            let mut finished = false;
            let mut at = 0;
            while at < active.len() {
                match active[at].as_mut().poll(cx) {
                    Poll::Ready(value) => {
                        done.push(value);
                        drop(active.remove(at));
                        finished = true;
                    }
                    Poll::Pending => at += 1,
                }
            }
            if !finished {
                return Poll::Pending;
            }
        }
    })
    .await;
    done
}

/// Why one candidate's check did not produce a verdict: what Python's worker raised.
#[derive(Debug)]
enum CheckFailed {
    /// `normalize_url_for_hydrus` -> `urlparse` raised.
    Url(pyurl::UrlValueError),
    Raised(Raised),
}

impl std::fmt::Display for CheckFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckFailed::Url(_) => f.write_str("urlparse raised ValueError"),
            CheckFailed::Raised(r) => write!(f, "{r}"),
        }
    }
}

/// Checks every candidate's source URL up front so dead ones can be dropped before the user ever
/// sees them, rather than being discovered one at a time as they are clicked. Mutates each
/// candidate's `remote_available` in place (and its URL, for the JoyReactor fallback).
///
/// Only a definite 404/410 marks a candidate dead; anything ambiguous stays `None`, which is
/// treated as alive everywhere downstream.
pub async fn sweep_candidate_availability(
    net: &NetClient,
    candidates: &mut [Candidate],
    access: &SettingsAccess<'_>,
    filename: &str,
    on_tick: Option<OnTick<'_>>,
) {
    let pending: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.remote_available.is_none() && !c.url.is_empty())
        .map(|(i, _)| i)
        .collect();
    if pending.is_empty() {
        return;
    }
    if let Some(tick) = on_tick {
        let n = pending.len() as f64;
        tick(&format!("Checking {} match(es)", pending.len()), n, n);
    }

    type Verdict = (usize, Result<(Option<bool>, Option<String>), CheckFailed>);
    let jobs: Vec<Pin<Box<dyn Future<Output = Verdict> + '_>>> = pending
        .iter()
        .map(|&index| {
            let url = candidates[index].url.clone();
            let job: Pin<Box<dyn Future<Output = Verdict> + '_>> = Box::pin(async move {
                let outcome = check_one(net, &url, access).await;
                (index, outcome)
            });
            job
        })
        .collect();
    let workers = AVAILABILITY_SWEEP_WORKERS.min(pending.len());
    let results = run_bounded(jobs, workers).await;

    let mut gone = 0usize;
    let now = now_epoch();
    for (index, outcome) in results {
        let (available, new_url) = match outcome {
            Ok(verdict) => verdict,
            Err(e) => {
                warn!("Availability sweep check raised: {e}");
                continue;
            }
        };
        let candidate = &mut candidates[index];
        if let Some(url) = new_url {
            candidate.url = url;
        }
        candidate.record_availability(available, now);
        if available == Some(false) {
            gone += 1;
        }
    }

    if let Some(tick) = on_tick {
        tick("", 0.0, 0.0);
    }
    if gone > 0 {
        info!(
            "{filename}: availability sweep found {gone}/{} match(es) gone",
            pending.len()
        );
    }
}

/// One candidate's check: the post page rather than the direct file, sent logged in where
/// configured (Sankaku hides account-only posts from anonymous requests behind a blank page).
/// Returns the verdict and, for a JoyReactor `full/` file that is gone, the URL to fall back to.
async fn check_one(
    net: &NetClient,
    url: &str,
    access: &SettingsAccess<'_>,
) -> Result<(Option<bool>, Option<String>), CheckFailed> {
    // The page must be the modern `/artworks/` form for Pixiv's 404 to be reported correctly.
    let target = normalize_url_for_hydrus(url).map_err(CheckFailed::Url)?;
    let cookies = site::cookies_for_url(&target, &access.credentials());
    let available = check_url_available(
        net,
        Some(&target),
        access.timeout(),
        None,
        None,
        cookies,
        Some(access),
    )
    .await
    .map_err(CheckFailed::Raised)?;
    // `_reactor_full_fallback(candidate, ...)` reads the candidate's own URL, not the target.
    if available == Some(false)
        && let Some(fallback) = reactor_fallback_url(url)
    {
        info!("{url}: full-res file is gone, falling back to {fallback}");
        return Ok((None, Some(fallback)));
    }
    Ok((available, None))
}

/// Re-verifies a cached result's candidates before showing them. A cached result carries the
/// candidates that were alive WHEN IT WAS SAVED, and nothing re-checks them on the way back out,
/// so a post deleted since is served up again every time. Uses the same sweep a fresh search
/// does, so there is one definition of "gone", and runs it concurrently.
pub async fn recheck_cached_candidates(
    net: &NetClient,
    mut candidates: Vec<Candidate>,
    access: &SettingsAccess<'_>,
    filename: &str,
    on_tick: Option<OnTick<'_>>,
) -> Vec<Candidate> {
    sweep_candidate_availability(net, &mut candidates, access, filename, on_tick).await;
    let before = candidates.len();
    let live: Vec<Candidate> = candidates
        .into_iter()
        .filter(|c| c.remote_available != Some(false))
        .collect();
    if live.len() != before {
        info!(
            "{filename}: dropped {} cached match(es) that have since gone",
            before - live.len()
        );
    }
    live
}
