//! The innermost service: one HTTP exchange on a pooled `reqwest::Client`.
//!
//! Differences from handing the request to reqwest directly, all from `core/net.py`:
//! - `timeout` is per connect and per read, not the whole request (`requests` semantics);
//! - `deadline` bounds the whole exchange, body included, and stops it when it passes;
//! - redirects are followed here, one hop at a time, so HEAD can stay put by default
//!   and credentials never follow a redirect to another host (docs/net.md section 8);
//! - the client keeps no cookies (`core/net.py:25-28`).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;

use http::header::{COOKIE, HeaderValue, LOCATION, USER_AGENT};
use http::{HeaderMap, StatusCode};
use tokio::time::{Instant, timeout};
use tower::Service;
use url::Url;

use super::config::NetConfig;
use super::error::{ErrorKind, NetError};
use super::request::{Body, Method, NetRequest, NetResponse, is_credential_header};

#[derive(Clone)]
pub(crate) struct Transport {
    client: Arc<RwLock<reqwest::Client>>,
    cfg: Arc<NetConfig>,
}

impl Transport {
    pub(crate) fn new(cfg: Arc<NetConfig>) -> Result<Self, NetError> {
        let client = build_client(&cfg)?;
        Ok(Transport {
            client: Arc::new(RwLock::new(client)),
            cfg,
        })
    }

    /// Replaces the client, dropping every pooled connection.
    pub(crate) fn reset_pool(&self) -> Result<(), NetError> {
        let fresh = build_client(&self.cfg)?;
        *self.client.write().unwrap() = fresh;
        Ok(())
    }
}

fn build_client(cfg: &NetConfig) -> Result<reqwest::Client, NetError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .tls_danger_accept_invalid_certs(!cfg.verify_tls);
    for (host, addr) in &cfg.host_overrides {
        builder = builder.resolve(host, *addr);
    }
    if !cfg.extra_root_certs_der.is_empty() {
        let certs = cfg
            .extra_root_certs_der
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                NetError::new(
                    ErrorKind::InvalidRequest,
                    None,
                    "an extra root certificate is not valid DER",
                )
            })?;
        builder = builder.tls_certs_merge(certs);
    }
    builder.build().map_err(|e| {
        NetError::new(
            ErrorKind::Other,
            None,
            format!("could not build the HTTP client: {}", e.without_url()),
        )
    })
}

impl Service<NetRequest> for Transport {
    type Response = NetResponse;
    type Error = NetError;
    type Future = Pin<Box<dyn Future<Output = Result<NetResponse, NetError>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), NetError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: NetRequest) -> Self::Future {
        let client = self.client.read().unwrap().clone();
        let cfg = self.cfg.clone();
        Box::pin(execute(client, cfg, req))
    }
}

/// How long the next wait may last, and whether the deadline (not the timeout)
/// is what limits it.
fn slice(per_wait: Duration, deadline_at: Option<Instant>) -> Option<(Duration, bool)> {
    match deadline_at {
        None => Some((per_wait, false)),
        Some(at) => {
            let left = at.saturating_duration_since(Instant::now());
            if left.is_zero() {
                None
            } else if left < per_wait {
                Some((left, true))
            } else {
                Some((per_wait, false))
            }
        }
    }
}

fn expired(url: &Url, by_deadline: bool) -> NetError {
    if by_deadline {
        NetError::new(
            ErrorKind::DeadlineExceeded,
            Some(url),
            "the exchange passed its deadline",
        )
    } else {
        NetError::new(ErrorKind::Timeout, Some(url), "no data within the timeout")
    }
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

async fn execute(
    client: reqwest::Client,
    cfg: Arc<NetConfig>,
    req: NetRequest,
) -> Result<NetResponse, NetError> {
    let started = req.started.unwrap_or_else(Instant::now);
    let deadline_at = req.deadline.map(|d| started + d);
    let origin_host = req.url.host_str().unwrap_or("").to_ascii_lowercase();
    let origin_https = req.url.scheme() == "https";
    let follow = req.follows_redirects();
    let mut url = req.url.clone();
    let mut method = req.method;
    let mut body = req.body.clone();
    let mut hops = 0usize;

    loop {
        let Some((wait, by_deadline)) = slice(req.timeout, deadline_at) else {
            return Err(expired(&url, true));
        };
        let builder = build_request(
            &client,
            &cfg,
            &req,
            method,
            &url,
            &body,
            &origin_host,
            origin_https,
        )?;
        let mut resp = match timeout(wait, builder.send()).await {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => return Err(NetError::from_reqwest(e, &url)),
            Err(_) => return Err(expired(&url, by_deadline)),
        };

        let status = resp.status();
        if follow && is_redirect(status) {
            if let Some(location) = resp.headers().get(LOCATION).and_then(|v| v.to_str().ok()) {
                hops += 1;
                if hops > cfg.max_redirects {
                    return Err(NetError::new(
                        ErrorKind::TooManyRedirects,
                        Some(&url),
                        format!("more than {} redirects", cfg.max_redirects),
                    ));
                }
                let next = url
                    .join(location)
                    .ok()
                    .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
                    .ok_or_else(|| {
                        NetError::new(
                            ErrorKind::InvalidRequest,
                            Some(&url),
                            "redirect to an unusable location",
                        )
                    })?;
                // 301, 302 and 303 turn a POST into a GET and drop its body; 307
                // and 308 repeat the request as it was.
                if matches!(status.as_u16(), 301..=303) && method == Method::Post {
                    method = Method::Get;
                    body = Body::Empty;
                }
                url = next;
                continue;
            }
        }

        let headers = resp.headers().clone();
        let mut buf: Vec<u8> = Vec::new();
        if method != Method::Head {
            // With a probe, `buf` holds what has been taken in whole blocks; `pending` is the
            // partial block still arriving.
            let mut pending: Vec<u8> = Vec::new();
            let mut stopped = false;
            loop {
                let Some((wait, by_deadline)) = slice(req.timeout, deadline_at) else {
                    return Err(expired(&url, true));
                };
                match timeout(wait, resp.chunk()).await {
                    Ok(Ok(Some(chunk))) => {
                        let Some(probe) = &req.probe else {
                            if buf.len() + chunk.len() > cfg.max_body_bytes {
                                return Err(NetError::new(
                                    ErrorKind::BodyTooLarge,
                                    Some(&url),
                                    format!("more than {} bytes", cfg.max_body_bytes),
                                ));
                            }
                            buf.extend_from_slice(&chunk);
                            continue;
                        };
                        pending.extend_from_slice(&chunk);
                        while pending.len() >= probe.chunk.max(1) {
                            let block: Vec<u8> = pending.drain(..probe.chunk.max(1)).collect();
                            buf.extend_from_slice(&block);
                            if (probe.done)(&buf) || buf.len() >= probe.cap {
                                stopped = true;
                                break;
                            }
                        }
                        if stopped {
                            break;
                        }
                    }
                    Ok(Ok(None)) => {
                        if let Some(probe) = &req.probe
                            && !pending.is_empty()
                        {
                            buf.extend_from_slice(&pending);
                            let _ = (probe.done)(&buf);
                        }
                        break;
                    }
                    Ok(Err(e)) => return Err(NetError::from_reqwest(e, &url)),
                    Err(_) => return Err(expired(&url, by_deadline)),
                }
            }
        }
        return Ok(NetResponse {
            status,
            headers,
            url,
            body: buf.into(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn build_request(
    client: &reqwest::Client,
    cfg: &NetConfig,
    req: &NetRequest,
    method: Method,
    url: &Url,
    body: &Body,
    origin_host: &str,
    origin_https: bool,
) -> Result<reqwest::RequestBuilder, NetError> {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    // Credentials stay on the host they were attached for, and never go from
    // https to http.
    let same_site = host == origin_host && !(origin_https && url.scheme() == "http");

    let mut headers = HeaderMap::new();
    for (name, value) in req.headers.iter() {
        if same_site || !is_credential_header(name, value) {
            headers.append(name.clone(), value.clone());
        }
    }
    if !headers.contains_key(USER_AGENT) {
        let ua = HeaderValue::from_str(&cfg.user_agent).map_err(|_| {
            NetError::new(
                ErrorKind::InvalidRequest,
                None,
                "invalid default User-Agent",
            )
        })?;
        headers.insert(USER_AGENT, ua);
    }
    if !headers.contains_key(COOKIE)
        && req.cookies.allows(&host, origin_host)
        && let Some(cookie) = req.cookies.header_value()
        && !(origin_https && url.scheme() == "http")
    {
        let mut value = HeaderValue::from_str(cookie.expose()).map_err(|_| {
            NetError::new(
                ErrorKind::InvalidRequest,
                Some(url),
                "a cookie value cannot be sent in a header",
            )
        })?;
        value.set_sensitive(true);
        headers.insert(COOKIE, value);
    }

    let mut builder = client
        .request(method.to_http(), url.clone())
        .headers(headers);
    match body {
        Body::Empty => {}
        Body::Bytes(bytes) => builder = builder.body(bytes.clone()),
        Body::Multipart(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                let mut p = reqwest::multipart::Part::bytes(part.data.to_vec());
                if let Some(name) = &part.filename {
                    p = p.file_name(name.clone());
                }
                if let Some(mime) = &part.mime {
                    p = p.mime_str(mime).map_err(|_| {
                        NetError::new(
                            ErrorKind::InvalidRequest,
                            Some(url),
                            "invalid multipart MIME type",
                        )
                    })?;
                }
                form = form.part(part.name.clone(), p);
            }
            builder = builder.multipart(form);
        }
    }
    Ok(builder)
}
