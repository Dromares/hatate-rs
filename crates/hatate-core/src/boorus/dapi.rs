//! The Gelbooru family's Data-API fetch and its 401 latch (`gelbooru._get_api_post` and the
//! copies in `rule34` and `xbooru`).
//!
//! Python keeps, per module, the one most recent `(url, post)` the fetch produced (so
//! `parse_file_url`, `parse_preview_url` and `parse_dimensions`, called back-to-back for the same
//! URL, share one API call) and, in Gelbooru's only, a latch: once the API has answered 401 or
//! 403 it needs credentials this build does not have, and nothing asks again this session.
//! rule34 and Xbooru are kept as modules of their own for exactly that reason, so one host's
//! refusal does not skip another host's still-public API. Safebooru has no Data-API path of its
//! own: its `parse_file_url` is Gelbooru's, and so shares Gelbooru's memo and latch.
//!
//! The parsers take the post they are to read in [`Context::api_post`]; this is what fills it.

use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde_json::{Map, Value};
use tracing::{debug, info};

use super::gelbooru;
use crate::net::{NetClient, NetRequest};
use crate::pyresponse::PyResponse;
use crate::remote::ua_headers;
use crate::{pyurl, remote};

/// `timeout=15` of every Data-API call. No deadline: Python sets none.
pub const TIMEOUT: f64 = 15.0;

static POST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[?&]id=(\d+)").expect("post id regex"));

/// Which module's state a parser reads through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `gelbooru`, and `safebooru`, whose `parse_file_url` is Gelbooru's.
    Gelbooru,
    Rule34,
    Xbooru,
}

impl Family {
    /// The family a parser's API-first hooks belong to, by the parser's registry name.
    pub fn of_parser(name: &str) -> Option<Family> {
        match name {
            "gelbooru" | "safebooru" => Some(Family::Gelbooru),
            "rule34" => Some(Family::Rule34),
            "xbooru" => Some(Family::Xbooru),
            _ => None,
        }
    }
}

type Memo = Option<(String, Option<Map<String, Value>>)>;

/// One module's memo, and Gelbooru's latch.
#[derive(Debug, Default)]
struct ModuleState {
    last: Memo,
    unauthorized: bool,
}

/// The Data-API state of the three modules.
#[derive(Debug, Default)]
pub struct DapiState {
    gelbooru: Mutex<ModuleState>,
    rule34: Mutex<ModuleState>,
    xbooru: Mutex<ModuleState>,
}

impl DapiState {
    pub fn new() -> Self {
        Self::default()
    }

    fn module(&self, family: Family) -> &Mutex<ModuleState> {
        match family {
            Family::Gelbooru => &self.gelbooru,
            Family::Rule34 => &self.rule34,
            Family::Xbooru => &self.xbooru,
        }
    }

    /// `gelbooru._api_unauthorized`: the API has refused this session.
    pub fn gelbooru_latched(&self) -> bool {
        self.gelbooru.lock().unwrap().unauthorized
    }

    /// `_last_api_post` of a module: the URL its memo is for, and whether the API gave a post.
    pub fn memo(&self, family: Family) -> Option<(String, bool)> {
        self.module(family)
            .lock()
            .unwrap()
            .last
            .as_ref()
            .map(|(url, post)| (url.clone(), post.is_some()))
    }

    /// `_get_api_post(url)`: the memoised post when the last call was for this URL, else a fetch.
    pub async fn get_api_post(
        &self,
        net: &NetClient,
        family: Family,
        url: &str,
    ) -> Option<Map<String, Value>> {
        {
            let state = self.module(family).lock().unwrap();
            if let Some((memo_url, post)) = &state.last
                && memo_url == url
            {
                return post.clone();
            }
        }
        let latched = self.module(family).lock().unwrap().unauthorized;
        let (post, now_latched) = fetch_post(net, family, url, latched).await;
        let mut state = self.module(family).lock().unwrap();
        state.unauthorized |= now_latched;
        state.last = Some((url.to_string(), post.clone()));
        post
    }
}

/// `urlparse(url).netloc or None`: `None` where Python raises `ValueError` too.
fn host_of(url: &str) -> Option<String> {
    pyurl::try_urlparse(url, "")
        .ok()
        .map(|p| p.netloc)
        .filter(|n| !n.is_empty())
}

/// `_fetch_post_from_api`. Returns the post, and whether this call latched the module.
async fn fetch_post(
    net: &NetClient,
    family: Family,
    url: &str,
    latched: bool,
) -> (Option<Map<String, Value>>, bool) {
    if family == Family::Gelbooru && latched {
        return (None, false); // already refused once this session; go straight to HTML
    }
    let Some(id) = POST_ID_RE.captures(url).map(|m| m[1].to_string()) else {
        debug!("No post ID found in {url}, can't query the Data API");
        return (None, false);
    };
    let host = match family {
        Family::Gelbooru => host_of(url).unwrap_or_else(|| "gelbooru.com".to_string()),
        Family::Rule34 => "api.rule34.xxx".to_string(),
        Family::Xbooru => "xbooru.com".to_string(),
    };
    let api_url = format!("https://{host}/index.php?page=dapi&s=post&q=index&json=1&id={id}");
    let Ok(mut req) = NetRequest::get(&api_url, remote::secs(TIMEOUT)) else {
        return (None, false);
    };
    req.headers = ua_headers();
    let resp = match net.send(req).await {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) => {
            debug!("Data API request failed for {url}: {e} (falling back to HTML scrape)");
            return (None, false);
        }
    };
    if family == Family::Gelbooru && matches!(resp.status, 401 | 403) {
        // Not a per-post problem: the API needs credentials this build does not have, so every
        // future call would fail identically.
        info!(
            "{}'s Data API returned HTTP {} (it needs an api_key/user_id) - using page scraping \
             for the rest of this session instead of retrying it per match",
            host_of(url).unwrap_or_else(|| "Gelbooru".to_string()),
            resp.status
        );
        return (None, true);
    }
    if resp.status != 200 {
        debug!("Data API returned HTTP {} for {url}", resp.status);
        return (None, false);
    }
    let Ok(data) = resp.json() else {
        debug!("Data API response for {url} was not valid JSON");
        return (None, false);
    };
    match gelbooru::post_from_dapi(&data) {
        Some(post) => (Some(post.clone()), false),
        None => {
            debug!("Data API response for {url} had no post data");
            (None, false)
        }
    }
}
