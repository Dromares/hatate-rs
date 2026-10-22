//! The fetch half of `core/boorus/__init__.py`: `fetch_page_info`, `fetch_tags` and `local_md5`.
//!
//! [`page_info_from_body`](super::page_info_from_body) is what runs once the body is in hand;
//! this is everything before it (which URL to fetch, the request, what the status means) and the
//! network steps Python takes from inside a parser's hooks, which here happen first and fill the
//! [`Context`] the hooks read:
//!
//! - the Gelbooru family's Data-API post (`gelbooru`, `safebooru`, `rule34`, `xbooru`),
//! - Pawchive's creator name, and the ranged read of its original for the dimensions,
//! - the ranged read of a Reddit post's original, for the dimensions.
//!
//! The order of the requests is the order Python makes them in (`parse`, then the file URL, then
//! the dimensions). State Python keeps in module globals is in [`BooruSession`].

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use http::HeaderMap;
use md5::{Digest, Md5};
use tracing::{debug, error, info, warn};

use super::dapi::{DapiState, Family};
use super::remote_size::RemoteSizes;
use super::{
    BooruError, BooruPageInfo, Context, FetchContext, SiteParser, find_parser, page_info_from_body,
};
use crate::config::Settings;
use crate::lru::LruCache;
use crate::model::Tag;
use crate::net::site::merge_rotated_deviantart_cookies;
use crate::net::{Cookies, ErrorKind, NetClient, USER_AGENT};
use crate::parser_health::ParserHealth;
use crate::paths::Paths;
use crate::progress::{OnTick, ticked};
use crate::pyresponse::PyResponse;
use crate::remote::{get_request, ua_headers};
use crate::{pycompat, reddit};

use super::{pawchive, redditpost};

/// `ARTIST_TIMEOUT`: the creator lookup's own timeout, with no deadline.
pub const ARTIST_TIMEOUT: f64 = 10.0;
/// `_md5_cache = LRUCache(256)`.
pub const MD5_CACHE_ENTRIES: usize = 256;
/// `ARTIST_CACHE_ENTRIES = 512`.
pub const ARTIST_CACHE_ENTRIES: usize = 512;

type Md5Key = (String, i128, u64);

/// What Python keeps in module globals across fetches: the parser-health counters, the file-hash
/// cache, the Data-API memos and latch, Pawchive's creator names and the remote image sizes.
#[derive(Debug)]
pub struct BooruSession {
    pub health: ParserHealth,
    pub dapi: DapiState,
    pub remote_sizes: RemoteSizes,
    artists: Mutex<LruCache<String, String>>,
    md5: Mutex<LruCache<Md5Key, String>>,
}

impl Default for BooruSession {
    fn default() -> Self {
        BooruSession {
            health: ParserHealth::new(),
            dapi: DapiState::new(),
            remote_sizes: RemoteSizes::new(),
            artists: Mutex::new(LruCache::new(ARTIST_CACHE_ENTRIES)),
            md5: Mutex::new(LruCache::new(MD5_CACHE_ENTRIES)),
        }
    }
}

impl BooruSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pawchive's creator names, least recently used first (`_artist_cache`).
    pub fn artist_cache(&self) -> Vec<(String, String)> {
        let mut cache = self.artists.lock().unwrap();
        let keys = cache.keys().to_vec();
        keys.into_iter()
            .map(|k| {
                let v = cache.get(&k).unwrap_or_default();
                (k, v)
            })
            .collect()
    }

    /// The local file's MD5, or `None` if it cannot be read.
    ///
    /// Only used by parsers that have no other way to identify a post: Sankaku, whose legacy URLs
    /// no longer resolve to anything, so the file itself is the only key left. Keyed on the file's
    /// identity rather than its name so an edited file is not matched by a stale digest.
    pub fn local_md5(&self, path: Option<&Path>) -> Option<String> {
        let path = path.filter(|p| !p.as_os_str().is_empty())?;
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) => {
                debug!("Could not stat {} for hashing: {e}", path.display());
                return None;
            }
        };
        let mtime_ns = meta
            .modified()
            .ok()
            .map_or(0, |t| match t.duration_since(UNIX_EPOCH) {
                Ok(d) => d.as_nanos() as i128,
                Err(e) => -(e.duration().as_nanos() as i128),
            });
        let key: Md5Key = (path.to_string_lossy().into_owned(), mtime_ns, meta.len());
        if let Some(cached) = self.md5.lock().unwrap().get(&key) {
            return (!cached.is_empty()).then_some(cached);
        }
        let mut digest = Md5::new();
        let read = (|| -> std::io::Result<()> {
            let mut file = File::open(path)?;
            let mut chunk = vec![0u8; 1024 * 1024];
            loop {
                let n = file.read(&mut chunk)?;
                if n == 0 {
                    return Ok(());
                }
                digest.update(&chunk[..n]);
            }
        })();
        if let Err(e) = read {
            debug!("Could not read {} for hashing: {e}", path.display());
            self.md5.lock().unwrap().set(key, String::new());
            return None;
        }
        let value: String = digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.md5.lock().unwrap().set(key, value.clone());
        Some(value)
    }

    /// The cached digests' keys, for a test of the cache.
    pub fn md5_cache_len(&self) -> usize {
        self.md5.lock().unwrap().len()
    }
}

/// A DeviantArt fetch can be handed the settings it may save a renewed session cookie into
/// (`capture_rotated_deviantart_cookies`). Left out for fetches that use ad hoc cookies never read
/// from settings (the Settings dialog's "Test DeviantArt..." button), so an unsaved paste cannot
/// be merged into the saved cookie string.
pub struct SettingsSink<'a> {
    pub settings: &'a mut Settings,
    pub paths: &'a Paths,
}

/// `gone_redirect(url, final_url)`: why the site's redirect means the post is gone.
pub type GoneRedirect<'a> = &'a (dyn Fn(&str, Option<&str>) -> Option<String> + Send + Sync);

/// `fetch_page_info`'s keyword arguments.
pub struct FetchOptions<'a> {
    pub timeout: f64,
    pub on_tick: Option<OnTick<'a>>,
    pub cookies: Option<Cookies>,
    pub headers: Option<HeaderMap>,
    pub gone_markers: &'a [&'a str],
    pub local_path: Option<&'a Path>,
    pub gone_redirect: Option<GoneRedirect<'a>>,
    pub settings: Option<SettingsSink<'a>>,
}

impl Default for FetchOptions<'_> {
    fn default() -> Self {
        FetchOptions {
            timeout: 30.0,
            on_tick: None,
            cookies: None,
            headers: None,
            gone_markers: &[],
            local_path: None,
            gone_redirect: None,
            settings: None,
        }
    }
}

/// Runs a hook, returning `None` (and logging) if it panics: Python's `try/except Exception`.
fn guarded<T>(what: &str, url: &str, run: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
        .map_err(|_| warn!("{what} raised for {url}"))
        .ok()
}

fn exc_text(e: &crate::net::NetError) -> String {
    e.to_string()
}

/// Fetches a booru post page once and extracts its tags, direct file URL, and preview URL, so
/// callers needing all three do not make three requests. If given, `on_tick(label, remaining,
/// total)` fires roughly once a second while the request is in flight.
pub async fn fetch_page_info(
    net: &NetClient,
    session: &BooruSession,
    url: &str,
    mut opts: FetchOptions<'_>,
) -> Result<BooruPageInfo, BooruError> {
    let Some(parser) = find_parser(url) else {
        debug!("No parser matches {url}, skipping booru page fetch");
        return Ok(BooruPageInfo::default());
    };

    // A parser can define resolve_fetch_url to fetch something other than the post page itself:
    // e621's uses it to hit its official JSON API instead of scraping HTML.
    let mut fetch_url = url.to_string();
    if let Some(hook) = parser.resolve_fetch_url_with_context {
        // The local file's md5 is computed only for parsers that ask for it, since on a network
        // share it means reading the file again.
        let ctx = FetchContext {
            local_md5: opts.local_path.and_then(|p| session.local_md5(Some(p))),
        };
        fetch_url = guarded("resolve_fetch_url_with_context", url, || hook(url, &ctx))
            .unwrap_or_else(|| url.to_string());
    } else if let Some(hook) = parser.resolve_fetch_url {
        fetch_url =
            guarded("resolve_fetch_url", url, || hook(url)).unwrap_or_else(|| url.to_string());
    }

    debug!("Fetching {fetch_url} (parser: {})", parser.name);
    let headers = opts.headers.clone().unwrap_or_else(ua_headers);
    let timeout = opts.timeout;
    let sent = match get_request(
        &fetch_url,
        timeout,
        Some(timeout),
        &headers,
        opts.cookies.take(),
    ) {
        Ok(req) => ticked("Fetching booru page", timeout, opts.on_tick, net.send(req)).await,
        Err(e) => Err(e),
    };
    let resp = match sent {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) if e.kind() == ErrorKind::DeadlineExceeded => {
            error!(
                "Fetching {fetch_url} exceeded the hard deadline: {}",
                e.detail()
            );
            return Err(BooruError::Fetch(format!(
                "Fetching {fetch_url} timed out: {}",
                e.detail()
            )));
        }
        Err(e) => {
            error!("Could not fetch {fetch_url}: {e}");
            let reason: String = exc_text(&e).chars().take(120).collect();
            session.health.record_failure(parser.name, url, &reason);
            return Err(BooruError::Fetch(format!(
                "Could not fetch {fetch_url}: {e}"
            )));
        }
    };
    let mut resp = resp;
    if let Some(sink) = opts.settings.as_mut() {
        capture_rotated_deviantart_cookies(&fetch_url, &resp, sink);
    }
    // A server that sends "text/html" with no charset makes requests fall back to ISO-8859-1,
    // which turns every UTF-8 character into mojibake: "x" becomes "Ã—", and a Japanese artist
    // tag becomes noise. e-shuushuu does exactly this, and it was silently corrupting its
    // non-ASCII tags and defeating the dimension parser. Assume UTF-8 when nothing was declared:
    // it is what these sites actually serve.
    let content_type = resp.header("Content-Type").unwrap_or_default();
    if resp.encoding.as_deref().is_some_and(|e| !e.is_empty())
        && !content_type.to_lowercase().contains("charset")
    {
        debug!(
            "{fetch_url} declared no charset; decoding as UTF-8 rather than {:?}",
            resp.encoding
        );
        resp.encoding = Some("utf-8".to_string());
    }
    // Decoded ONCE: every hook below reads the same string.
    let body = resp.text();

    debug!("{fetch_url} -> HTTP {}", resp.status);
    if matches!(resp.status, 404 | 410) {
        info!("Post is gone (HTTP {}): {fetch_url}", resp.status);
        return Err(BooruError::ContentGone(format!(
            "{fetch_url} returned HTTP {} - the post is gone",
            resp.status
        )));
    }
    if resp.status != 200 {
        // A parser can recognise its site's "you need an account" status and say so. Without
        // this the response is just a failed fetch, indistinguishable from a timeout or a
        // rate-limit, and the user is left with a blank match and no idea that signing in would
        // fill it in.
        if let Some(hook) = parser.restriction_for_status {
            let ctx = Context::default();
            let reason = guarded("restriction_for_status", url, || {
                hook(resp.status, &body, url, &ctx)
            })
            .flatten();
            if let Some(reason) = reason.filter(|r| !r.is_empty()) {
                info!(
                    "{url} is account-restricted (HTTP {}): {reason}",
                    resp.status
                );
                return Ok(BooruPageInfo {
                    restricted: Some(reason),
                    fetched: true,
                    ..BooruPageInfo::default()
                });
            }
        }
        return Err(BooruError::Fetch(format!(
            "{fetch_url} returned HTTP {}",
            resp.status
        )));
    }

    // Some sites keep a deleted post's URL alive and answer HTTP 200 with a notice where the
    // image should be. The body is already in hand, so checking costs nothing, and it is the ONLY
    // check a single-candidate match gets.
    let mut gone_reason = None;
    // A redirect can be the "gone" signal instead of any body text. Gelbooru has no deleted-post
    // wording at all: it answers HTTP 200 and bounces to its post LIST.
    if let Some(gone_redirect) = opts.gone_redirect
        && let Some(landed) = gone_redirect(url, Some(&resp.url)).filter(|l| !l.is_empty())
    {
        info!("{url} redirected to the site's \"no such post\" page ({landed:?})");
        gone_reason = Some(format!("redirected to {landed}"));
    }
    if gone_reason.is_none() && !opts.gone_markers.is_empty() {
        gone_reason = super::gone_marker(&body, opts.gone_markers);
    }

    let ctx = network_context(net, session, parser, &body, url).await;
    let mut info = page_info_from_body(parser, &body, url, &fetch_url, &ctx);
    info.gone_reason = gone_reason;
    if info.tags.is_empty() {
        // For most sites an empty result means the markup moved. Counted, so a parser that has
        // quietly stopped working can be noticed by the app rather than only by the user's eye.
        if !parser.empty_result_is_normal {
            session.health.record_empty(parser.name, url);
        }
    } else {
        session.health.record_success(parser.name, info.tags.len());
        debug!(
            "Extracted {} tag(s), file_url={:?}, preview_url={:?}, dimensions={:?}x{:?} from {url}",
            info.tags.len(),
            info.file_url,
            info.preview_url,
            info.width,
            info.height
        );
    }
    Ok(info)
}

/// Back-compat wrapper for callers that only want the tags.
pub async fn fetch_tags(
    net: &NetClient,
    session: &BooruSession,
    url: &str,
    timeout: f64,
) -> Result<Vec<Tag>, BooruError> {
    let opts = FetchOptions {
        timeout,
        ..FetchOptions::default()
    };
    Ok(fetch_page_info(net, session, url, opts).await?.tags)
}

/// Everything a parser's hooks would have fetched from inside themselves, in Python's order.
async fn network_context(
    net: &NetClient,
    session: &BooruSession,
    parser: &SiteParser,
    body: &str,
    url: &str,
) -> Context {
    let mut ctx = Context::default();
    if let Some(family) = Family::of_parser(parser.name) {
        ctx.api_post = session.dapi.get_api_post(net, family, url).await;
    }
    match parser.name {
        "pawchive" => {
            artist_step(net, session, body, url, &mut ctx).await;
            if let Some(file_url) = pawchive::dimensions_request(body) {
                let size = session
                    .remote_sizes
                    .image_size(net, &file_url, Some(USER_AGENT))
                    .await;
                ctx.remember_image_size(&file_url, size);
            }
        }
        "redditpost" => {
            if let Some(file_url) = redditpost::dimensions_request(body, url) {
                let size = session
                    .remote_sizes
                    .image_size(net, &file_url, Some(reddit::USER_AGENT))
                    .await;
                ctx.remember_image_size(&file_url, size);
            }
        }
        _ => {}
    }
    ctx
}

/// `_artist_name`: the creator's display name, which the post itself does not carry. Worth a
/// second request: plenty of posts have no tags of their own, so without this they would
/// contribute nothing at all. Cached per creator (LRU of 512), bounded by its own timeout; any
/// failure just means no artist tag, and is remembered as a miss.
async fn artist_step(
    net: &NetClient,
    session: &BooruSession,
    body: &str,
    url: &str,
    ctx: &mut Context,
) {
    let Some(request) = pawchive::artist_profile_request(body, url, ctx) else {
        return;
    };
    let cached = session.artists.lock().unwrap().get(&request.key);
    if let Some(name) = cached {
        ctx.artist_cache.insert(request.key, name);
        return;
    }
    let name = match fetch_artist(net, &request.profile_url).await {
        Ok(name) => name,
        Err(()) => {
            // `.get` on a JSON value that is not an object: `AttributeError`, which the handler
            // (`RequestException`, `ValueError`) does not catch. Nothing is remembered.
            ctx.artist_lookup_raised = true;
            return;
        }
    };
    let name = name.map(|n| pycompat::strip(&n).to_string());
    let remembered = name.clone().unwrap_or_default();
    session
        .artists
        .lock()
        .unwrap()
        .set(request.key.clone(), remembered);
    ctx.remember_artist(&request.key, name.as_deref());
}

/// The profile's `name`, as `(resp.json() or {}).get("name") if status == 200 else None`:
/// `Ok(None)` for anything that is a failure or has no string name, `Err` where Python raises.
async fn fetch_artist(net: &NetClient, profile_url: &str) -> Result<Option<String>, ()> {
    let Ok(req) = get_request(profile_url, ARTIST_TIMEOUT, None, &ua_headers(), None) else {
        return Ok(None);
    };
    let resp = match net.send(req).await {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) => {
            debug!("Could not read the Pawchive creator name from {profile_url}: {e}");
            return Ok(None);
        }
    };
    if resp.status != 200 {
        return Ok(None);
    }
    let Ok(json) = resp.json() else {
        debug!("Could not read the Pawchive creator name from {profile_url}: not JSON");
        return Ok(None);
    };
    match &json {
        v if !pycompat::truthy(v) => Ok(None),
        serde_json::Value::Object(map) => Ok(map
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)),
        _ => Err(()),
    }
}

/// Folds a renewed DeviantArt session cookie back into settings
/// (`site_access.capture_rotated_deviantart_cookies`).
fn capture_rotated_deviantart_cookies(url: &str, resp: &PyResponse, sink: &mut SettingsSink<'_>) {
    let set_cookies: Vec<(String, String)> = resp
        .headers
        .get_all(http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|line| {
            let (name, value) = line.split(';').next()?.split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
        })
        .collect();
    let Some(rotated) = merge_rotated_deviantart_cookies(
        url,
        resp.status,
        &set_cookies,
        sink.settings.deviantart_cookies.expose(),
    ) else {
        return;
    };
    sink.settings.deviantart_cookies = crate::config::Secret::new(rotated.cookies.expose());
    if let Err(e) = sink.settings.save(sink.paths) {
        warn!("Could not save the renewed DeviantArt session cookie: {e:?}");
    }
    // Names only, never the values, which are live session credentials.
    info!(
        "DeviantArt renewed its session cookie(s) ({}) - saved the refresh",
        rotated.names.join(", ")
    );
}
