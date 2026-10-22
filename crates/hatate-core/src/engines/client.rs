//! What every engine client shares: the upload, the countdown ticker, the deadline, and how an
//! answer is turned into text.
//!
//! **The cut.** Python's engine `search` functions are cut as `engines/mod.rs` says: the answer
//! is interpreted by `*_from_response` (Phase 2), the request half is here. A client owns a
//! [`NetClient`] and the hosts it talks to. Both are handed to its constructor; nothing in the
//! shipped binary can redirect a host (no variable, flag or config key).
//!
//! **Deadlines.** Every request sets `deadline` as well as `timeout`, as `core/net.py`'s callers
//! do: a deadline over the whole exchange, body included, not a per-read timeout. The net layer
//! enforces it (`NetRequest::with_deadline`) and reports [`ErrorKind::DeadlineExceeded`], which a
//! client reports as [`HardTimeoutError`]'s message.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::task::JoinHandle;

use crate::hard_timeout::HardTimeoutError;
use crate::image_prep::{self, DEFAULT_MAX_BYTES, DEFAULT_MAX_DIMENSION};
use crate::net::{ErrorKind, NetError, NetResponse};

/// `OnTick = Callable[[str, float, float], None]`: `(label, seconds remaining, total seconds)`.
/// Called about once a second while a request is in flight, and once at the end with
/// `("", 0.0, 0.0)` to clear whatever shows it. Purely UI countdown feedback.
pub type OnTick = Arc<dyn Fn(&str, f64, f64) + Send + Sync + 'static>;

/// `ProgressTicker`'s interval.
const TICK_INTERVAL: Duration = Duration::from_secs(1);

/// `core/progress_ticker.py`: calls the callback once per interval while the guard lives, then
/// once with `("", 0, 0)` when it is dropped, whether the wrapped call succeeded, failed or timed
/// out. A no-op when there is no callback.
pub(crate) struct ProgressTicker {
    task: Option<JoinHandle<()>>,
    on_tick: Option<OnTick>,
}

impl ProgressTicker {
    pub(crate) fn start(label: &str, timeout: f64, on_tick: Option<OnTick>) -> Self {
        let task = on_tick.clone().map(|tick| {
            let label = label.to_owned();
            tokio::spawn(async move {
                let start = tokio::time::Instant::now();
                loop {
                    tokio::time::sleep(TICK_INTERVAL).await;
                    let remaining = (timeout - start.elapsed().as_secs_f64()).max(0.0);
                    tick(&label, remaining, timeout);
                    if remaining <= 0.0 {
                        break;
                    }
                }
            })
        });
        ProgressTicker { task, on_tick }
    }
}

impl Drop for ProgressTicker {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(tick) = &self.on_tick {
            tick("", 0.0, 0.0);
        }
    }
}

/// `seconds` as a `Duration`. A value no `Duration` can hold (negative, NaN, absurdly large) is
/// treated as zero (negative, NaN) or as the longest wait there is.
pub(crate) fn seconds(value: f64) -> Duration {
    if value.is_nan() || value <= 0.0 {
        Duration::ZERO
    } else {
        Duration::try_from_secs_f64(value).unwrap_or(Duration::MAX)
    }
}

/// What reading the upload failed with: Python's `OSError`, as `str(exc)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadFailure(pub String);

/// `prepare_upload_bytes(image_path)` with its defaults, off the async threads (decoding a large
/// image takes seconds). The prepared bytes are cached process-wide, so IQDB and trace.moe asked
/// about the same file in one search decode it once.
pub(crate) async fn read_upload(image_path: &Path) -> Result<(Bytes, String), ReadFailure> {
    let path = image_path.to_owned();
    let work = {
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
            image_prep::prepare_upload_bytes(&path, DEFAULT_MAX_BYTES, DEFAULT_MAX_DIMENSION)
        })
    };
    match work.await {
        Ok(Ok(prepared)) => Ok(prepared),
        Ok(Err(err)) => Err(ReadFailure(image_prep::os_error_text(&err, &path))),
        Err(join) => Err(ReadFailure(format!(
            "the image could not be prepared: {join}"
        ))),
    }
}

/// `urllib.parse.urlencode(pairs)`: `quote_plus` on each name and value (letters, digits and
/// `_.-~` as they are, a space as `+`, every other byte as `%XX`), joined with `&`. The `url`
/// crate's form encoder keeps `*` and writes `~` as `%7E`, so `params=` as `requests` sends it is
/// built here.
pub(crate) fn urlencode(pairs: &[(&str, &str)]) -> String {
    fn quote_plus(text: &str, out: &mut String) {
        for b in text.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                    out.push(char::from(b));
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
    }
    let mut out = String::new();
    for (i, (name, value)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        quote_plus(name, &mut out);
        out.push('=');
        quote_plus(value, &mut out);
    }
    out
}

/// A `multipart/form-data` upload as `requests` writes it.
pub(crate) struct Upload {
    /// `multipart/form-data; boundary=...`
    pub(crate) content_type: String,
    pub(crate) body: Bytes,
}

/// `files={field: (filename, data, "application/octet-stream")}`, encoded the way `requests`
/// and urllib3 2.x do (`encode_multipart_formdata`): one part, headers `Content-Disposition`
/// then `Content-Type`, the data as it is, and the WHATWG quoting for the name and filename: LF,
/// CR and `"` percent-encoded, nothing else touched. Built here and sent as a plain body rather
/// than through `reqwest`'s multipart, which writes a backslash before `"` and before a newline,
/// so a filename with either would not be the filename Python sends.
pub(crate) fn multipart_upload(field: &str, filename: &str, data: &[u8]) -> Upload {
    multipart_upload_as(field, filename, "application/octet-stream", data)
}

/// [`multipart_upload`] for a file given with its own content type, `files={field: (filename,
/// data, content_type)}`.
pub(crate) fn multipart_upload_as(
    field: &str,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Upload {
    let boundary = boundary();
    let mut body = Vec::with_capacity(data.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; {}; {}\r\n\
             Content-Type: {content_type}\r\n\r\n",
            header_param("name", field),
            header_param("filename", filename)
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Upload {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        body: Bytes::from(body),
    }
}

/// `urllib3.fields.format_multipart_header_param`: `name="value"` with `\n`, `\r` and `"`
/// percent-encoded.
fn header_param(name: &str, value: &str) -> String {
    let mut quoted = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\n' => quoted.push_str("%0A"),
            '\r' => quoted.push_str("%0D"),
            '"' => quoted.push_str("%22"),
            other => quoted.push(other),
        }
    }
    format!("{name}=\"{quoted}\"")
}

/// A boundary: 32 hex digits, like `uuid4().hex`.
fn boundary() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut out = String::with_capacity(32);
    while out.len() < 32 {
        // `RandomState` is seeded from the operating system's entropy on every construction.
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out.truncate(32);
    out
}

/// How a failed exchange is reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The deadline passed: Python's `HardTimeoutError`.
    Deadline(HardTimeoutError),
    /// Anything else the transport said: Python's `requests.RequestException`. The text is the
    /// net layer's (redacted, no query); `requests` words these differently, see `docs/net.md`.
    Transport(String),
}

pub(crate) fn failure_of(err: &NetError, deadline: Duration) -> Failure {
    match err.kind() {
        ErrorKind::DeadlineExceeded => Failure::Deadline(HardTimeoutError::trickling(deadline)),
        _ => Failure::Transport(err.to_string()),
    }
}

/// Runs `request` with the ticker counting down beside it.
pub(crate) async fn ticking<T>(
    label: &str,
    timeout: f64,
    on_tick: Option<OnTick>,
    request: impl Future<Output = T>,
) -> T {
    let _ticker = ProgressTicker::start(label, timeout, on_tick);
    request.await
}

/// `requests.Response.text`: the body decoded the way `requests` does, from the `Content-Type`
/// header alone.
///
/// - a `charset` parameter names the encoding;
/// - without one, a `text/*` type means ISO-8859-1 (RFC 2616's default, which `requests` keeps)
///   and `application/json` means UTF-8;
/// - with no type to go on, `requests` asks `chardet`. This port does not run a detector: valid
///   UTF-8 reads as UTF-8 and anything else as Windows-1252. Where the two disagree the text of
///   the page differs in its non-ASCII characters; see `docs/net.md`.
///
/// Encodings other than UTF-8, ASCII, ISO-8859-1 and Windows-1252 read as UTF-8, which is what
/// `requests` does for a name Python has no codec for. Invalid bytes become U+FFFD.
pub(crate) fn response_text(resp: &NetResponse) -> String {
    let encoding = match header_charset(resp.header("content-type")) {
        Charset::Named(name) => name,
        Charset::TextDefault => "iso-8859-1".to_owned(),
        Charset::JsonDefault => "utf-8".to_owned(),
        Charset::None => {
            if std::str::from_utf8(&resp.body).is_ok() {
                "utf-8".to_owned()
            } else {
                "cp1252".to_owned()
            }
        }
    };
    decode(&resp.body, &encoding)
}

enum Charset {
    Named(String),
    TextDefault,
    JsonDefault,
    None,
}

/// `requests.utils.get_encoding_from_headers`.
fn header_charset(content_type: Option<&str>) -> Charset {
    let Some(content_type) = content_type else {
        return Charset::None;
    };
    let mut parts = content_type.split(';');
    let kind = parts.next().unwrap_or("").trim().to_lowercase();
    for param in parts {
        if let Some((key, value)) = param.split_once('=')
            && key.trim().eq_ignore_ascii_case("charset")
        {
            let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
            if !value.is_empty() {
                return Charset::Named(value.to_lowercase());
            }
        }
    }
    if kind.contains("text") {
        Charset::TextDefault
    } else if kind.contains("application/json") {
        Charset::JsonDefault
    } else {
        Charset::None
    }
}

fn decode(body: &[u8], encoding: &str) -> String {
    match encoding.replace('_', "-").as_str() {
        "iso-8859-1" | "latin-1" | "latin1" | "iso8859-1" | "l1" => {
            body.iter().map(|&b| char::from(b)).collect()
        }
        "cp1252" | "windows-1252" => body.iter().map(|&b| cp1252(b)).collect(),
        "ascii" | "us-ascii" => body
            .iter()
            .map(|&b| if b < 0x80 { char::from(b) } else { '\u{fffd}' })
            .collect(),
        _ => String::from_utf8_lossy(body).into_owned(),
    }
}

/// Windows-1252 for one byte. Python's `cp1252` codec leaves five bytes undefined; with
/// `errors="replace"` they read as U+FFFD.
fn cp1252(b: u8) -> char {
    const HIGH: [u32; 32] = [
        0x20ac, 0xfffd, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160,
        0x2039, 0x0152, 0xfffd, 0x017d, 0xfffd, 0xfffd, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022,
        0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0xfffd, 0x017e, 0x0178,
    ];
    if (0x80..0xa0).contains(&b) {
        char::from_u32(HIGH[usize::from(b - 0x80)]).unwrap_or('\u{fffd}')
    } else {
        char::from(b)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use http::{HeaderMap, HeaderValue, StatusCode};
    use url::Url;

    use super::*;

    fn response(content_type: Option<&str>, body: &[u8]) -> NetResponse {
        let mut headers = HeaderMap::new();
        if let Some(ct) = content_type {
            headers.insert("content-type", HeaderValue::from_str(ct).unwrap());
        }
        NetResponse {
            status: StatusCode::OK,
            headers,
            url: Url::parse("https://a.example/").unwrap(),
            body: Bytes::copy_from_slice(body),
        }
    }

    #[test]
    fn text_is_decoded_as_requests_does() {
        // charset wins.
        assert_eq!(
            response_text(&response(
                Some("text/html; charset=UTF-8"),
                "caf\u{e9}".as_bytes()
            )),
            "caf\u{e9}"
        );
        assert_eq!(
            response_text(&response(
                Some("text/html; charset=\"utf-8\""),
                "caf\u{e9}".as_bytes()
            )),
            "caf\u{e9}"
        );
        // text/* with no charset is ISO-8859-1, so UTF-8 bytes come out as two characters.
        assert_eq!(
            response_text(&response(Some("text/html"), "caf\u{e9}".as_bytes())),
            "caf\u{c3}\u{a9}"
        );
        // application/json is UTF-8.
        assert_eq!(
            response_text(&response(Some("application/json"), "caf\u{e9}".as_bytes())),
            "caf\u{e9}"
        );
        // No type: UTF-8 if it is, else Windows-1252.
        assert_eq!(
            response_text(&response(None, "caf\u{e9}".as_bytes())),
            "caf\u{e9}"
        );
        assert_eq!(
            response_text(&response(None, b"caf\xe9 \x80")),
            "caf\u{e9} \u{20ac}"
        );
        // A name Python has no codec for reads as UTF-8; invalid bytes are replaced.
        assert_eq!(
            response_text(&response(Some("text/html; charset=nope-9"), b"a\xffb")),
            "a\u{fffd}b"
        );
        assert_eq!(
            response_text(&response(Some("text/plain; charset=ascii"), b"a\xe9")),
            "a\u{fffd}"
        );
    }

    #[test]
    fn the_upload_is_the_multipart_body_requests_writes() {
        let up = multipart_upload("file", "q\"u\ro\nte\\.png", b"DATA\r\n--x");
        let boundary = up
            .content_type
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap();
        assert_eq!(boundary.len(), 32);
        assert!(boundary.bytes().all(|b| b.is_ascii_hexdigit()));
        let want = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"q%22u%0Do%0Ate\\.png\"\r\nContent-Type: application/octet-stream\r\n\r\n\
             DATA\r\n--x\r\n--{boundary}--\r\n"
        );
        assert_eq!(up.body, Bytes::from(want));
        assert_ne!(
            multipart_upload("f", "x", b"").content_type,
            multipart_upload("f", "x", b"").content_type
        );
    }

    #[test]
    fn query_strings_are_written_as_urlencode_writes_them() {
        assert_eq!(
            urlencode(&[
                ("output_type", "2"),
                ("api_key", "k~k*k k&k=k/\u{e9}"),
                ("db", "999")
            ]),
            "output_type=2&api_key=k~k%2Ak+k%26k%3Dk%2F%C3%A9&db=999"
        );
        assert_eq!(urlencode(&[]), "");
    }

    #[test]
    fn seconds_never_panics() {
        assert_eq!(seconds(1.5), Duration::from_millis(1500));
        assert_eq!(seconds(-3.0), Duration::ZERO);
        assert_eq!(seconds(f64::NAN), Duration::ZERO);
        assert_eq!(seconds(f64::INFINITY), Duration::MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn the_ticker_counts_down_once_a_second_then_clears() {
        let seen: Arc<Mutex<Vec<(String, f64, f64)>>> = Arc::default();
        let sink = seen.clone();
        let on_tick: OnTick = Arc::new(move |label, remaining, total| {
            sink.lock()
                .unwrap()
                .push((label.to_owned(), remaining, total));
        });
        ticking("Waiting on IQDB", 3.0, Some(on_tick), async {
            tokio::time::sleep(Duration::from_millis(2500)).await;
        })
        .await;
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec![
                ("Waiting on IQDB".to_owned(), 2.0, 3.0),
                ("Waiting on IQDB".to_owned(), 1.0, 3.0),
                (String::new(), 0.0, 0.0),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_ticker_clears_when_the_request_fails_and_stops_at_zero() {
        let seen: Arc<Mutex<Vec<f64>>> = Arc::default();
        let sink = seen.clone();
        let on_tick: OnTick = Arc::new(move |_, remaining, _| sink.lock().unwrap().push(remaining));
        let out: Result<(), &str> = ticking("x", 2.0, Some(on_tick), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Err("late")
        })
        .await;
        assert!(out.is_err());
        // 1.0, 0.0 (then the loop ends: no tick at 3 s, 4 s ...), and the final clear.
        assert_eq!(seen.lock().unwrap().clone(), vec![1.0, 0.0, 0.0]);
    }

    #[tokio::test]
    async fn no_callback_means_no_ticker() {
        let out = ticking("x", 1.0, None, async { 5 }).await;
        assert_eq!(out, 5);
    }
}
