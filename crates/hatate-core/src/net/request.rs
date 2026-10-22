//! The request and response types the tower stack carries.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::{AUTHORIZATION, COOKIE, HeaderName, HeaderValue, PROXY_AUTHORIZATION};
use http::{HeaderMap, StatusCode};
use tokio::time::Instant;
use url::Url;

use super::error::{ErrorKind, NetError};
use super::secret::{Cookies, Secret, redact_url};

/// The Hydrus API key header (`core/hydrus_client.py:130`).
pub const HYDRUS_KEY_HEADER: &str = "hydrus-client-api-access-key";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Head,
}

impl Method {
    pub(crate) fn to_http(self) -> http::Method {
        match self {
            Method::Get => http::Method::GET,
            Method::Post => http::Method::POST,
            Method::Head => http::Method::HEAD,
        }
    }
}

/// Redirect handling for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Redirects {
    /// Follow for GET and POST, do not follow for HEAD: what `requests` does
    /// when `allow_redirects` is left out.
    #[default]
    Default,
    Follow,
    Stop,
}

/// How a request is retried (docs/net.md sections 1 and 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetryMode {
    /// `core/net.py:137-145`: GET and HEAD retry once on a 429 with a short wait.
    Once429,
    /// `retry_429=False`.
    Never,
    /// The Pawchive crawl pattern (`core/pawchive_index.py:388-430`).
    Polite {
        max_retries: u32,
        /// Used when a 429 names no usable `Retry-After`.
        default_wait: Duration,
    },
}

impl RetryMode {
    /// `MAX_RETRIES = 3`, `RETRY_WAIT = 30.0` (`core/pawchive_index.py:58-59`).
    pub const fn pawchive() -> Self {
        RetryMode::Polite {
            max_retries: 3,
            default_wait: Duration::from_secs(30),
        }
    }
}

/// One part of a `multipart/form-data` upload.
#[derive(Clone)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub data: Bytes,
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Part")
            .field("name", &self.name)
            .field("filename", &self.filename)
            .field("len", &self.data.len())
            .finish()
    }
}

#[derive(Clone, Default)]
pub enum Body {
    #[default]
    Empty,
    Bytes(Bytes),
    Multipart(Vec<Part>),
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::Empty => f.write_str("Empty"),
            Body::Bytes(b) => write!(f, "Bytes({} bytes)", b.len()),
            Body::Multipart(parts) => write!(f, "Multipart({parts:?})"),
        }
    }
}

/// A body read that stops early, as Python's `stream=True` plus `iter_content(chunk)` does
/// (`core/boorus/_remote_size.py`): the body is taken in blocks of `chunk` bytes (the last one
/// shorter), and reading stops, with the connection dropped, after the first block that leaves
/// `done` true or the total at `cap` bytes. The response body is what was taken.
#[derive(Clone)]
pub struct BodyProbe {
    pub chunk: usize,
    pub cap: usize,
    pub done: ProbeDone,
}

/// Whether what has been taken so far is enough.
pub type ProbeDone = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

impl fmt::Debug for BodyProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyProbe")
            .field("chunk", &self.chunk)
            .field("cap", &self.cap)
            .finish_non_exhaustive()
    }
}

/// A request. `timeout` is mandatory, as in `core/net.py:108`.
#[derive(Clone)]
pub struct NetRequest {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub cookies: Cookies,
    pub body: Body,
    /// Per connect and per read, like `requests`' `timeout`.
    pub timeout: Duration,
    /// Bounds the whole exchange, body included.
    pub deadline: Option<Duration>,
    pub redirects: Redirects,
    pub retry: RetryMode,
    /// Least time between two requests to the same host.
    pub min_gap: Option<Duration>,
    /// Stop reading the body early (a ranged read of a file's first bytes).
    pub probe: Option<BodyProbe>,
    /// Whether the host's rate-limit state applies: it waits out, and records, the host's
    /// `X-RateLimit-*` headers. Only a request that Python sends outside `core/net.py`
    /// (ascii2d's own `requests.Session`) turns this off.
    pub(crate) gated: bool,
    /// Read the body only when the final status is this one; any other answer is returned with an
    /// empty body and the connection dropped unread (`stream=True` and a `resp.raw.read` made only
    /// for a 200, `core/availability.py`).
    pub body_only_on: Option<u16>,
    /// When the exchange began, for the deadline. Set by the stack.
    pub(crate) started: Option<Instant>,
}

impl NetRequest {
    pub fn new(method: Method, url: &str, timeout: Duration) -> Result<Self, NetError> {
        let parsed = Url::parse(url).map_err(|e| {
            // The parse error names the problem, never the input.
            NetError::new(ErrorKind::InvalidRequest, None, format!("bad URL: {e}"))
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(NetError::new(
                ErrorKind::InvalidRequest,
                None,
                "only http and https URLs with a host are supported",
            ));
        }
        Ok(NetRequest {
            method,
            url: parsed,
            headers: HeaderMap::new(),
            cookies: Cookies::new(),
            body: Body::Empty,
            timeout,
            deadline: None,
            redirects: Redirects::Default,
            retry: RetryMode::Once429,
            min_gap: None,
            probe: None,
            gated: true,
            body_only_on: None,
            started: None,
        })
    }

    pub fn get(url: &str, timeout: Duration) -> Result<Self, NetError> {
        Self::new(Method::Get, url, timeout)
    }

    pub fn post(url: &str, timeout: Duration) -> Result<Self, NetError> {
        Self::new(Method::Post, url, timeout)
    }

    pub fn head(url: &str, timeout: Duration) -> Result<Self, NetError> {
        Self::new(Method::Head, url, timeout)
    }

    /// Adds a query parameter.
    pub fn query(mut self, name: &str, value: &str) -> Self {
        self.url.query_pairs_mut().append_pair(name, value);
        self
    }

    /// Adds a query parameter whose value is a secret (SauceNAO's `api_key`).
    pub fn secret_query(self, name: &str, value: &Secret) -> Self {
        self.query(name, value.expose())
    }

    /// Sets a header. Fails on a name or value HTTP cannot carry.
    pub fn header(mut self, name: &str, value: &str) -> Result<Self, NetError> {
        let (name, value) = header_pair(name, value)?;
        self.headers.insert(name, value);
        Ok(self)
    }

    /// Sets a header whose value is a secret. It is marked sensitive, and is not
    /// forwarded to another host on a redirect.
    pub fn secret_header(mut self, name: &str, value: &Secret) -> Result<Self, NetError> {
        let (name, mut value) = header_pair(name, value.expose())?;
        value.set_sensitive(true);
        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn with_cookies(mut self, cookies: Cookies) -> Self {
        self.cookies = cookies;
        self
    }

    pub fn with_body(mut self, body: Bytes) -> Self {
        self.body = Body::Bytes(body);
        self
    }

    pub fn with_multipart(mut self, parts: Vec<Part>) -> Self {
        self.body = Body::Multipart(parts);
        self
    }

    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_redirects(mut self, redirects: Redirects) -> Self {
        self.redirects = redirects;
        self
    }

    pub fn with_retry(mut self, retry: RetryMode) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_min_gap(mut self, gap: Duration) -> Self {
        self.min_gap = Some(gap);
        self
    }

    /// The host's `X-RateLimit-*` state neither holds this request back nor learns from its
    /// answer: for a request Python makes without `core/net.py`.
    pub fn without_host_gate(mut self) -> Self {
        self.gated = false;
        self
    }

    pub fn with_probe(mut self, probe: BodyProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Reads the body only from an answer with this status (see [`NetRequest::body_only_on`]).
    pub fn with_body_only_on(mut self, status: u16) -> Self {
        self.body_only_on = Some(status);
        self
    }

    /// `host` or `host:port`, lower-cased: Python's `netloc` (`core/net.py:76`).
    pub fn host_key(&self) -> String {
        host_key(&self.url)
    }

    pub(crate) fn follows_redirects(&self) -> bool {
        match self.redirects {
            Redirects::Follow => true,
            Redirects::Stop => false,
            Redirects::Default => self.method != Method::Head,
        }
    }
}

pub(crate) fn host_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

fn header_pair(name: &str, value: &str) -> Result<(HeaderName, HeaderValue), NetError> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| NetError::new(ErrorKind::InvalidRequest, None, "invalid header name"))?;
    // The value is never echoed: it may be a secret.
    let value = HeaderValue::from_str(value)
        .map_err(|_| NetError::new(ErrorKind::InvalidRequest, None, "invalid header value"))?;
    Ok((name, value))
}

/// Whether a header must be kept off another host and out of dumps.
pub(crate) fn is_credential_header(name: &HeaderName, value: &HeaderValue) -> bool {
    value.is_sensitive()
        || name == AUTHORIZATION
        || name == PROXY_AUTHORIZATION
        || name == COOKIE
        || name.as_str() == HYDRUS_KEY_HEADER
}

impl fmt::Debug for NetRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let query_keys: Vec<_> = self
            .url
            .query_pairs()
            .map(|(k, _)| k.into_owned())
            .collect();
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(name, value)| {
                if is_credential_header(name, value) {
                    format!("{name}: [redacted]")
                } else {
                    format!("{name}: {}", value.to_str().unwrap_or("<binary>"))
                }
            })
            .collect();
        f.debug_struct("NetRequest")
            .field("method", &self.method)
            .field("url", &redact_url(&self.url))
            .field("query_keys", &query_keys)
            .field("headers", &headers)
            .field("cookies", &self.cookies)
            .field("body", &self.body)
            .field("timeout", &self.timeout)
            .field("deadline", &self.deadline)
            .field("redirects", &self.redirects)
            .field("retry", &self.retry)
            .field("min_gap", &self.min_gap)
            .field("probe", &self.probe)
            .finish()
    }
}

/// A fully read response.
#[derive(Clone)]
pub struct NetResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    /// The final URL, after redirects (`resp.url` in Python).
    pub url: Url,
    pub body: Bytes,
}

impl NetResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The body as text, replacing invalid UTF-8.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The `name=value` pair at the start of each `Set-Cookie` header.
    pub fn set_cookie_pairs(&self) -> Vec<(String, String)> {
        self.headers
            .get_all(http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|line| {
                let first = line.split(';').next()?;
                let (name, value) = first.split_once('=')?;
                let name = name.trim();
                (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
            })
            .collect()
    }
}

impl fmt::Debug for NetResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(name, value)| {
                if name == http::header::SET_COOKIE || is_credential_header(name, value) {
                    format!("{name}: [redacted]")
                } else {
                    format!("{name}: {}", value.to_str().unwrap_or("<binary>"))
                }
            })
            .collect();
        f.debug_struct("NetResponse")
            .field("status", &self.status.as_u16())
            .field("url", &redact_url(&self.url))
            .field("headers", &headers)
            .field("body_len", &self.body.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> NetRequest {
        NetRequest::get("https://Example.com:8443/p?x=1", Duration::from_secs(5)).unwrap()
    }

    #[test]
    fn host_key_is_python_netloc() {
        assert_eq!(req().host_key(), "example.com:8443");
        let r = NetRequest::get("https://Example.com/p", Duration::from_secs(5)).unwrap();
        assert_eq!(r.host_key(), "example.com");
    }

    #[test]
    fn rejects_non_http_urls() {
        assert!(NetRequest::get("file:///etc/passwd", Duration::from_secs(1)).is_err());
        assert!(NetRequest::get("not a url", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn head_does_not_follow_redirects_by_default() {
        let t = Duration::from_secs(1);
        assert!(
            NetRequest::get("http://a.example/", t)
                .unwrap()
                .follows_redirects()
        );
        assert!(
            NetRequest::post("http://a.example/", t)
                .unwrap()
                .follows_redirects()
        );
        let head = NetRequest::head("http://a.example/", t).unwrap();
        assert!(!head.follows_redirects());
        assert!(head.with_redirects(Redirects::Follow).follows_redirects());
    }

    #[test]
    fn set_cookie_pairs_take_the_first_pair_only() {
        let mut headers = HeaderMap::new();
        headers.append(
            http::header::SET_COOKIE,
            HeaderValue::from_static("auth=abc=def; Path=/; HttpOnly"),
        );
        headers.append(http::header::SET_COOKIE, HeaderValue::from_static("junk"));
        let resp = NetResponse {
            status: StatusCode::OK,
            headers,
            url: Url::parse("https://a.example/").unwrap(),
            body: Bytes::new(),
        };
        assert_eq!(
            resp.set_cookie_pairs(),
            vec![("auth".into(), "abc=def".into())]
        );
    }
}
