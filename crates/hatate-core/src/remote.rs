//! Fetching the things a match points at: its image, a follow-up API page, a HEAD for its format
//! and size (`core/remote.py`).
//!
//! Every function here is best-effort in the way the Python is: a network failure is "nothing",
//! not an error, with the two exceptions Python leaves uncaught (see [`Raised`]). Each is
//! `async` and takes the [`NetClient`] it sends through. Timeouts are seconds as `f64`, as the
//! config holds them; where Python passes `deadline=timeout` so does this, and the deadline is
//! over the whole exchange, body included (`docs/net.md`).

use std::time::Duration;

use http::HeaderMap;
use tracing::debug;

use crate::config::Settings;
use crate::model::Candidate;
use crate::net::site::{self, SiteCredentials};
use crate::net::{
    Cookies, ErrorKind, NetClient, NetError, NetRequest, Redirects, Secret, USER_AGENT,
};
use crate::progress::{OnTick, ticked};
use crate::pyresponse::PyResponse;

/// What Python raises out of a function whose own handlers do not catch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raised {
    /// `HardTimeoutError`: `fetch_text` catches only `requests.RequestException`, and a hard
    /// deadline is not one, so it leaves the function.
    HardTimeout(String),
    /// `ValueError`: `int()` of a `Content-Length` that `str.isdigit()` accepts but `int()` does
    /// not (superscript digits).
    ValueError(String),
}

impl std::fmt::Display for Raised {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Raised::HardTimeout(m) | Raised::ValueError(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Raised {}

/// A timeout in seconds as a `Duration`: negative and NaN are zero, and an absurd value is capped
/// at what a `Duration` can add to an `Instant`.
pub fn secs(value: f64) -> Duration {
    if value.is_nan() || value <= 0.0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(value.min(1.0e9))
    }
}

/// `net.get(url, headers=..., timeout=timeout, deadline=deadline, cookies=...)` as a request.
pub(crate) fn get_request(
    url: &str,
    timeout: f64,
    deadline: Option<f64>,
    headers: &HeaderMap,
    cookies: Option<Cookies>,
) -> Result<NetRequest, NetError> {
    let mut req = NetRequest::get(url, secs(timeout))?;
    req.headers = headers.clone();
    if let Some(deadline) = deadline {
        req = req.with_deadline(secs(deadline));
    }
    // `cookies=cookies or None`: an empty jar is no cookies.
    if let Some(cookies) = cookies.filter(|c| !c.is_empty()) {
        req = req.with_cookies(cookies);
    }
    Ok(req)
}

/// A header map holding one `User-Agent`.
pub(crate) fn ua_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        http::header::USER_AGENT,
        http::HeaderValue::from_static(USER_AGENT),
    );
    h
}

/// A header value Python could have put in a `str`: `requests` refuses a value with a newline or
/// a control character, and so does the net layer.
pub(crate) fn set_header(headers: &mut HeaderMap, name: &str, value: &str) -> bool {
    match (
        http::HeaderName::from_bytes(name.as_bytes()),
        http::HeaderValue::from_str(value),
    ) {
        (Ok(n), Ok(v)) => {
            headers.insert(n, v);
            true
        }
        _ => false,
    }
}

/// `credentials` for the site-access layer from the settings that hold them.
pub fn credentials(settings: &Settings) -> SiteCredentials {
    // Two `Secret` types exist (config and net); the value crosses by `expose`, nowhere else.
    let s = |v: &crate::config::Secret| Secret::new(v.expose());
    SiteCredentials {
        pixiv_session_cookie: s(&settings.pixiv_session_cookie),
        sankaku_cookies: s(&settings.sankaku_cookies),
        animepictures_cookies: s(&settings.animepictures_cookies),
        deviantart_cookies: s(&settings.deviantart_cookies),
        e621_username: s(&settings.e621_username),
        e621_api_key: s(&settings.e621_api_key),
    }
}

/// Best-effort download of a URL's raw bytes: thumbnails and, for the GUI, a matched image's
/// full-resolution file. Failure here never fails the overall search, so a failure is `None`
/// (callers that need the download to succeed check for it themselves).
///
/// Sends a `Referer` when given one, for the hotlink protection some CDNs enforce: they serve an
/// HTML page instead of the real file to a request without one. A `data:` URI carries its bytes
/// inline (Google Lens's exact-match tiles hand their thumbnails over that way) and is decoded
/// instead of requested; one that decodes to nothing is a failure, not a file.
pub async fn download_bytes(
    net: &NetClient,
    url: Option<&str>,
    timeout: f64,
    referer: Option<&str>,
    on_tick: Option<OnTick<'_>>,
    cookies: Option<Cookies>,
) -> Option<Vec<u8>> {
    let url = url.filter(|u| !u.is_empty())?;
    if url.starts_with("data:") {
        return match data_uri_bytes(url) {
            Ok(bytes) if !bytes.is_empty() => Some(bytes),
            Ok(_) => None,
            Err(()) => {
                debug!("Could not decode a data: URI");
                None
            }
        };
    }
    let mut headers = ua_headers();
    if let Some(referer) = referer.filter(|r| !r.is_empty()) {
        // A referer `requests` would refuse to send fails the request, which is a `None` here.
        if !set_header(&mut headers, "Referer", referer) {
            return None;
        }
    }
    let req = get_request(url, timeout, Some(timeout), &headers, cookies).ok()?;
    let resp = ticked("Downloading image", timeout, on_tick, net.send(req))
        .await
        .ok()?;
    (resp.status.as_u16() == 200).then(|| resp.body.to_vec())
}

/// GETs a URL and returns its body as text, or `None`.
///
/// Used for the secondary API calls a parser cannot make itself, the page list of a multi-image
/// post, say, which arrive after the main fetch and so cannot go through the parser's
/// single-response contract.
///
/// Site-specific headers apply here exactly as they do to the main fetch. Skipping them was a
/// real failure: Pixiv's `/ajax/illust/{id}/pages` sent with the plain app User-Agent and a
/// session cookie came back as a Cloudflare challenge (HTTP 403), so every multi-page Pixiv post
/// silently fell back to page 1.
///
/// A request that fails is `None`; a hard deadline passing is [`Raised::HardTimeout`], because
/// Python's handler catches `requests.RequestException` and `HardTimeoutError` is not one.
pub async fn fetch_text(
    net: &NetClient,
    url: &str,
    settings: &Settings,
    referer: Option<&str>,
) -> Result<Option<String>, Raised> {
    if url.is_empty() {
        return Ok(None);
    }
    let creds = credentials(settings);
    let mut headers = site::headers_for_url(url, Some(&creds)).unwrap_or_else(ua_headers);
    if let Some(referer) = referer.filter(|r| !r.is_empty())
        && !set_header(&mut headers, "Referer", referer)
    {
        return Ok(None);
    }
    let cookies = site::cookies_for_url(url, &creds);
    let timeout = settings.search_timeout;
    let Ok(req) = get_request(url, timeout, Some(timeout), &headers, cookies) else {
        return Ok(None);
    };
    let resp = match net.send(req).await {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) if e.kind() == ErrorKind::DeadlineExceeded => {
            return Err(Raised::HardTimeout(e.detail().to_string()));
        }
        Err(e) => {
            debug!("Could not fetch {url}: {e}");
            return Ok(None);
        }
    };
    if resp.status != 200 {
        debug!("{url} returned HTTP {}", resp.status);
        return Ok(None);
    }
    Ok(Some(resp.text()))
}

/// A lightweight HEAD for a match's format (from `Content-Type`) and file size (from
/// `Content-Length`), without downloading the file. Returns `(format, size_bytes)`. Best-effort:
/// many sites do not support HEAD or omit these headers, so failure is normal and non-fatal and
/// just leaves the info blank in the UI.
///
/// Sends a `Referer` (the originating post page) when given one: several image CDNs, Gelbooru's
/// among them, enforce hotlink protection and serve an HTML block page instead of the image to a
/// request with none, which would otherwise look like a correctly extracted image URL that
/// mysteriously "is" HTML.
pub async fn fetch_remote_info(
    net: &NetClient,
    url: Option<&str>,
    timeout: f64,
    referer: Option<&str>,
    on_tick: Option<OnTick<'_>>,
) -> Result<(Option<String>, Option<i64>), Raised> {
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        return Ok((None, None));
    };
    let mut headers = ua_headers();
    if let Some(referer) = referer.filter(|r| !r.is_empty())
        && !set_header(&mut headers, "Referer", referer)
    {
        return Ok((None, None));
    }
    debug!("HEAD {url} (referer={referer:?})");
    let Ok(mut req) = NetRequest::head(url, secs(timeout)) else {
        return Ok((None, None));
    };
    req.headers = headers;
    req = req
        .with_deadline(secs(timeout))
        .with_redirects(Redirects::Follow);
    let resp = match ticked("Checking image info", timeout, on_tick, net.send(req)).await {
        Ok(resp) => PyResponse::from_net(resp),
        Err(e) => {
            debug!("HEAD {url} failed: {e}");
            return Ok((None, None));
        }
    };
    let content_type = resp.header("Content-Type").unwrap_or_default();
    let format = format_from_content_type(&content_type);
    debug!(
        "HEAD {url} -> HTTP {}, Content-Type={content_type:?} -> format={format:?}",
        resp.status
    );
    let size = match resp.header("Content-Length") {
        Some(length) if is_digit_str(&length) => py_int_of_digits(&length)?,
        _ => None,
    };
    Ok((format, size))
}

/// `str.isdigit()`: non-empty, every character a digit. Within Latin-1, where header bytes land,
/// that is `0`-`9` and the superscripts `¹ ² ³`.
fn is_digit_str(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '\u{b2}' | '\u{b3}' | '\u{b9}'))
}

/// `int(s)` of a string `isdigit()` accepted: a superscript digit is a `ValueError`. Python's int
/// has no ceiling; a size past `i64` cannot be held here, and counts as unknown.
fn py_int_of_digits(s: &str) -> Result<Option<i64>, Raised> {
    if s.chars().any(|c| !c.is_ascii_digit()) {
        return Err(Raised::ValueError(format!(
            "invalid literal for int() with base 10: {s:?}"
        )));
    }
    Ok(s.parse::<i64>().ok())
}

/// The format name for a `Content-Type`: the known ones by their usual names, any other
/// `type/subtype` as the upper-cased subtype, anything else nothing.
pub fn format_from_content_type(content_type: &str) -> Option<String> {
    let ct = crate::pycompat::strip(content_type.split(';').next().unwrap_or("")).to_lowercase();
    let known = match ct.as_str() {
        "image/jpeg" | "image/jpg" => Some("JPEG"),
        "image/png" => Some("PNG"),
        "image/gif" => Some("GIF"),
        "image/webp" => Some("WEBP"),
        "image/bmp" => Some("BMP"),
        "image/avif" => Some("AVIF"),
        "video/webm" => Some("WEBM"),
        "video/mp4" => Some("MP4"),
        _ => None,
    };
    if let Some(known) = known {
        return Some(known.to_string());
    }
    ct.contains('/')
        .then(|| ct.rsplit('/').next().unwrap_or("").to_uppercase())
}

/// The Referer to send when fetching a match's own CDN, for sites that enforce hotlink
/// protection. Only applies when the URL being fetched is one of the candidate's own booru-hosted
/// URLs (`preview_url`, `direct_file_url`), never the search engine's own thumbnail host, which
/// is a different origin entirely.
///
/// Pixiv's CDN (`i.pximg.net`) is a special case: it is known to require the bare
/// `https://www.pixiv.net/` origin, not the exact artwork page. That is the value other
/// Pixiv-downloading tools use, and is more reliable than "use the source page", which works for
/// most other booru sites.
pub fn referer_for_candidate(
    candidate: &Candidate,
    url_being_fetched: Option<&str>,
) -> Option<String> {
    // `x not in (a, b)` is `==` against each, and `None == None` holds.
    if url_being_fetched != candidate.preview_url.as_deref()
        && url_being_fetched != candidate.direct_file_url.as_deref()
    {
        return None;
    }
    if !candidate.url.is_empty() && candidate.url.contains("pixiv.net") {
        return Some("https://www.pixiv.net/".to_string());
    }
    Some(candidate.url.clone())
}

// -- data: URIs --------------------------------------------------------------------------------

/// The bytes a `data:` URI carries. `Err` is what `download_bytes` catches
/// (`ValueError`, `binascii.Error`).
fn data_uri_bytes(url: &str) -> Result<Vec<u8>, ()> {
    let (header, payload) = match url.split_once(',') {
        Some((header, payload)) => (header, payload),
        None => (url, ""),
    };
    if header.contains(";base64") {
        b64decode(payload)
    } else {
        Ok(unquote_to_bytes(payload))
    }
}

/// `base64.b64decode(str)`: ASCII text only (`ValueError` otherwise), characters outside the
/// alphabet skipped, and a short last group an error (`binascii.a2b_base64`, non-strict).
pub fn b64decode(payload: &str) -> Result<Vec<u8>, ()> {
    if !payload.is_ascii() {
        return Err(());
    }
    let mut out = Vec::with_capacity(payload.len() / 4 * 3);
    let (mut quad_pos, mut left, mut pads) = (0u8, 0u8, 0u8);
    for &byte in payload.as_bytes() {
        if byte == b'=' {
            if quad_pos >= 2 {
                pads += 1;
                if quad_pos + pads >= 4 {
                    // A pad sequence ends the input: the quad is already decoded.
                    return Ok(out);
                }
            }
            continue;
        }
        let Some(value) = b64_value(byte) else {
            continue;
        };
        pads = 0;
        match quad_pos {
            0 => {
                quad_pos = 1;
                left = value;
            }
            1 => {
                quad_pos = 2;
                out.push((left << 2) | (value >> 4));
                left = value & 0x0f;
            }
            2 => {
                quad_pos = 3;
                out.push((left << 4) | (value >> 2));
                left = value & 0x03;
            }
            _ => {
                quad_pos = 0;
                out.push((left << 6) | value);
                left = 0;
            }
        }
    }
    if quad_pos != 0 {
        return Err(());
    }
    Ok(out)
}

fn b64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// `urllib.parse.unquote_to_bytes(str)`: the string as UTF-8 with each `%XX` run turned into its
/// byte; a `%` not followed by two hex digits stays.
pub fn unquote_to_bytes(s: &str) -> Vec<u8> {
    let raw = s.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                raw.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                raw.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            )
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_come_from_the_content_type() {
        let f = |ct| format_from_content_type(ct);
        assert_eq!(f("image/jpeg").as_deref(), Some("JPEG"));
        assert_eq!(f("IMAGE/JPG; charset=x").as_deref(), Some("JPEG"));
        assert_eq!(f(" image/png ").as_deref(), Some("PNG"));
        assert_eq!(f("video/webm").as_deref(), Some("WEBM"));
        assert_eq!(f("image/tiff").as_deref(), Some("TIFF"));
        assert_eq!(f("text/html; charset=utf-8").as_deref(), Some("HTML"));
        assert_eq!(
            f("application/octet-stream").as_deref(),
            Some("OCTET-STREAM")
        );
        assert_eq!(f("nonsense"), None);
        assert_eq!(f(""), None);
    }

    #[test]
    fn base64_follows_binascii_non_strict() {
        assert_eq!(b64decode("aGk=").unwrap(), b"hi");
        assert_eq!(b64decode("aGk").is_err(), true);
        assert_eq!(b64decode("aG k=\n").unwrap(), b"hi");
        assert_eq!(b64decode("a").is_err(), true);
        assert_eq!(b64decode("aGk=junk").unwrap(), b"hi");
        assert_eq!(b64decode("").unwrap(), b"");
        assert_eq!(b64decode("\u{e9}").is_err(), true);
    }

    #[test]
    fn percent_escapes_become_bytes_and_a_stray_percent_stays() {
        assert_eq!(unquote_to_bytes("a%20b%zz%4"), b"a b%zz%4");
        assert_eq!(unquote_to_bytes("\u{e9}%C3%A9"), "éé".as_bytes());
    }

    #[test]
    fn a_data_uri_decodes_in_place_and_an_empty_one_is_nothing() {
        assert_eq!(data_uri_bytes("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert_eq!(data_uri_bytes("data:text/plain,a%20b").unwrap(), b"a b");
        assert_eq!(data_uri_bytes("data:;base64,").unwrap(), b"");
        assert!(data_uri_bytes("data:;base64,a").is_err());
    }

    #[test]
    fn the_referer_is_for_the_candidates_own_urls_only() {
        let mut c = Candidate::new("https://gelbooru.com/post/1");
        c.preview_url = Some("https://img/p.jpg".into());
        c.direct_file_url = Some("https://img/f.png".into());
        assert_eq!(
            referer_for_candidate(&c, Some("https://img/f.png")).as_deref(),
            Some("https://gelbooru.com/post/1")
        );
        assert_eq!(
            referer_for_candidate(&c, Some("https://iqdb/thumb.jpg")),
            None
        );
        // None is "equal" to an unset URL, as it is in Python.
        c.preview_url = None;
        assert_eq!(
            referer_for_candidate(&c, None).as_deref(),
            Some("https://gelbooru.com/post/1")
        );
        c.url = "https://www.pixiv.net/artworks/9".into();
        assert_eq!(
            referer_for_candidate(&c, Some("https://img/f.png")).as_deref(),
            Some("https://www.pixiv.net/")
        );
    }

    #[test]
    fn digits_are_what_isdigit_says() {
        assert!(is_digit_str("123"));
        assert!(!is_digit_str(""));
        assert!(!is_digit_str(" 12"));
        assert!(!is_digit_str("-1"));
        assert!(is_digit_str("\u{b2}"));
        assert!(py_int_of_digits("\u{b2}").is_err());
        assert_eq!(py_int_of_digits("007").unwrap(), Some(7));
        assert_eq!(py_int_of_digits("99999999999999999999").unwrap(), None);
    }
}
