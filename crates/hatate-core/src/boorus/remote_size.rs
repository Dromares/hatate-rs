//! An image's width and height, read from the first bytes of the file
//! (`core/boorus/_remote_size.py`).
//!
//! For sites whose page or API states no dimensions but whose image host honours Range requests:
//! Pawchive's file host, Reddit's `i.redd.it`. An image's header states its size, so a few KB are
//! read (never the whole file), and reading stops as soon as the size is known. A JPEG's size sits
//! after its EXIF/ICC blocks, which are usually a few KB but can run long, hence the cap rather
//! than a fixed small range.
//!
//! Cached by URL. A network failure is not cached (a timeout says nothing about the file), but a
//! file whose size cannot be read from its start is.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing::debug;

use crate::lru::LruCache;
use crate::net::{BodyProbe, NetClient, NetRequest};
use crate::pyimage::{self, DecompressionBomb};
use crate::pyurl;
use crate::remote::{set_header, ua_headers};

pub const MAX_BYTES: usize = 512 * 1024;
pub const TIMEOUT: f64 = 10.0;
pub const CACHE_ENTRIES: usize = 1024;
pub const CHUNK: usize = 16 * 1024;

pub type Size = (Option<i64>, Option<i64>);

/// The cache (`_remote_size._cache`) and the reads that fill it.
#[derive(Debug)]
pub struct RemoteSizes {
    cache: Mutex<LruCache<String, Size>>,
}

impl Default for RemoteSizes {
    fn default() -> Self {
        RemoteSizes {
            cache: Mutex::new(LruCache::new(CACHE_ENTRIES)),
        }
    }
}

impl RemoteSizes {
    pub fn new() -> Self {
        Self::default()
    }

    /// The URLs read so far and what was learned of each, least recently used first.
    pub fn entries(&self) -> Vec<(String, Size)> {
        let cache = self.cache.lock().unwrap();
        cache.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// `image_size(url, user_agent)`: the original's `(width, height)`, or `(None, None)`.
    ///
    /// `Err` is the `DecompressionBombError` Pillow raises from `ImageFile.Parser.feed`, which
    /// `image_size` does not catch: nothing is cached and the caller sees it.
    pub async fn image_size(
        &self,
        net: &NetClient,
        url: &str,
        user_agent: Option<&str>,
    ) -> Result<Size, DecompressionBomb> {
        if let Some(cached) = self.cache.lock().unwrap().get(&url.to_string()) {
            return Ok(cached);
        }
        let mut headers = http::HeaderMap::new();
        set_header(&mut headers, "Range", &format!("bytes=0-{}", MAX_BYTES - 1));
        if let Some(agent) = user_agent.filter(|a| !a.is_empty())
            && !set_header(&mut headers, "User-Agent", agent)
        {
            return Ok((None, None));
        }
        if !headers.contains_key(http::header::USER_AGENT) {
            headers.extend(ua_headers().into_iter().filter_map(|(n, v)| Some((n?, v))));
        }
        let Ok(mut req) =
            NetRequest::get(&pyurl::requote_uri(url), Duration::from_secs_f64(TIMEOUT))
        else {
            // Python's `requests` refuses the URL the same way, as a `RequestException`.
            return Ok((None, None));
        };
        req.headers = headers;
        req = req.with_probe(BodyProbe {
            chunk: CHUNK,
            cap: MAX_BYTES,
            done: Arc::new(|taken| !matches!(pyimage::image_size_checked(taken), Ok(None))),
        });
        let resp = match net.send(req).await {
            Ok(resp) => resp,
            Err(e) => {
                debug!("Could not read dimensions of {url}: {e}");
                return Ok((None, None));
            }
        };
        let mut size = None;
        if matches!(resp.status.as_u16(), 200 | 206) {
            size = pyimage::image_size_checked(&resp.body)?;
        } else {
            debug!(
                "{url} answered {} when asked for its first bytes",
                resp.status.as_u16()
            );
        }
        let result = match size {
            Some((w, h)) if w > 0 && h > 0 => (Some(i64::from(w)), Some(i64::from(h))),
            _ => (None, None),
        };
        self.cache.lock().unwrap().set(url.to_string(), result);
        Ok(result)
    }
}
