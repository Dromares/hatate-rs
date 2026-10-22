//! Errors from the net layer. They never carry a URL query, a header value or a
//! cookie (docs/net.md section 9).

use std::error::Error as StdError;
use std::fmt;

use url::Url;

use super::secret::redact_url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request could not be built (bad URL, bad header).
    InvalidRequest,
    /// TCP connect or HTTP transport failure.
    Connect,
    /// The server's certificate was rejected, or the TLS handshake failed.
    Tls,
    /// A connect or a read waited longer than the request's `timeout`.
    Timeout,
    /// The whole exchange passed the request's `deadline` (Python's `HardTimeoutError`).
    DeadlineExceeded,
    /// The host said it has no requests left for longer than `MAX_HOST_WAIT`.
    RateLimited,
    TooManyRedirects,
    /// The body could not be read to the end.
    Body,
    BodyTooLarge,
    Other,
}

impl ErrorKind {
    fn label(self) -> &'static str {
        match self {
            ErrorKind::InvalidRequest => "invalid request",
            ErrorKind::Connect => "connection failed",
            ErrorKind::Tls => "TLS failure",
            ErrorKind::Timeout => "timed out",
            ErrorKind::DeadlineExceeded => "deadline exceeded",
            ErrorKind::RateLimited => "rate limited",
            ErrorKind::TooManyRedirects => "too many redirects",
            ErrorKind::Body => "could not read the response body",
            ErrorKind::BodyTooLarge => "response body too large",
            ErrorKind::Other => "network error",
        }
    }
}

/// A failed request. `url` is already redacted: scheme, host, port and path only.
#[derive(Debug, Clone)]
pub struct NetError {
    kind: ErrorKind,
    url: Option<String>,
    detail: String,
}

impl NetError {
    pub(crate) fn new(kind: ErrorKind, url: Option<&Url>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        let detail = match url {
            Some(u) => scrub(&detail, u),
            None => detail,
        };
        NetError {
            kind,
            url: url.map(redact_url),
            detail,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// What went wrong, without the kind's label or the URL: `core/hard_timeout.py`'s message
    /// for a [`ErrorKind::DeadlineExceeded`], so a caller can show the text Python shows.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The redacted URL (no query), when the error belongs to a request.
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Whether a polite retry loop should try again: a transport failure, not a
    /// verdict about the request itself.
    pub(crate) fn is_transport(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Connect
                | ErrorKind::Tls
                | ErrorKind::Timeout
                | ErrorKind::Body
                | ErrorKind::Other
        )
    }

    pub(crate) fn from_reqwest(err: reqwest::Error, url: &Url) -> Self {
        let err = err.without_url();
        let kind = classify(&err);
        let detail = chain(&err);
        NetError::new(kind, Some(url), detail)
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind.label())?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        if let Some(url) = &self.url {
            write!(f, " ({url})")?;
        }
        Ok(())
    }
}

impl StdError for NetError {}

/// Walks `source()` and joins the messages.
fn chain(err: &(dyn StdError + 'static)) -> String {
    let mut parts = vec![err.to_string()];
    let mut next = err.source();
    while let Some(source) = next {
        parts.push(source.to_string());
        next = source.source();
    }
    parts.join(": ")
}

fn classify(err: &reqwest::Error) -> ErrorKind {
    let mut next: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = next {
        if is_tls(e) {
            return ErrorKind::Tls;
        }
        next = e.source();
    }
    if err.is_timeout() {
        ErrorKind::Timeout
    } else if err.is_connect() || err.is_request() {
        ErrorKind::Connect
    } else if err.is_body() || err.is_decode() {
        ErrorKind::Body
    } else {
        ErrorKind::Other
    }
}

/// A rustls error, or an `io::Error` wrapping one at any depth (tokio-rustls
/// wraps the handshake error in an `io::Error`, and hyper-util wraps that again).
fn is_tls(err: &(dyn StdError + 'static)) -> bool {
    if err.downcast_ref::<rustls::Error>().is_some() {
        return true;
    }
    let mut io = err.downcast_ref::<std::io::Error>();
    while let Some(e) = io {
        let Some(inner) = e.get_ref() else {
            return false;
        };
        if inner.downcast_ref::<rustls::Error>().is_some() {
            return true;
        }
        io = inner.downcast_ref::<std::io::Error>();
    }
    false
}

/// Removes the request's query string, and the full URL, from a message in case
/// a lower layer put them there.
fn scrub(detail: &str, url: &Url) -> String {
    let mut out = detail.replace(url.as_str(), &redact_url(url));
    if let Some(query) = url.query()
        && !query.is_empty()
    {
        out = out.replace(query, "[redacted]");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_never_shows_the_query() {
        let url =
            Url::parse("https://saucenao.com/search.php?api_key=SYNTHETICKEY&db=999").unwrap();
        let err = NetError::new(
            ErrorKind::Connect,
            Some(&url),
            "tcp connect error for https://saucenao.com/search.php?api_key=SYNTHETICKEY&db=999",
        );
        let shown = format!("{err} / {err:?}");
        assert!(!shown.contains("SYNTHETICKEY"), "{shown}");
        assert_eq!(err.url(), Some("https://saucenao.com/search.php"));
    }

    #[test]
    fn only_transport_failures_are_retryable_by_the_polite_loop() {
        let mk = |kind| NetError::new(kind, None, "x");
        assert!(mk(ErrorKind::Connect).is_transport());
        assert!(mk(ErrorKind::Timeout).is_transport());
        assert!(!mk(ErrorKind::DeadlineExceeded).is_transport());
        assert!(!mk(ErrorKind::RateLimited).is_transport());
        assert!(!mk(ErrorKind::InvalidRequest).is_transport());
    }
}
