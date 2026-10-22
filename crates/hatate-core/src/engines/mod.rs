//! Search-engine result parsers (`core/iqdb.py`, `core/ascii2d.py`, `core/tracemoe.py`,
//! `core/pawchive_lookup.py`, `core/saucenao.py`, `core/google_lens.py`): what an engine's answer
//! says once it is in hand.
//!
//! **Where Python's engine functions are cut.** Each `search` in Python uploads the image, waits,
//! reads the answer, and interprets it. The interpreting is here: the HTTP status to error
//! mapping (413, 429, anything but 200), the check that an HTML page is the engine's and not a
//! block page, the JSON header checks, and the parse. The *request* half is Phase 3: reading and
//! downscaling the file, building the multipart body and its headers, the deadline, the network
//! errors (`requests.RequestException`, `HardTimeoutError`, `OSError`), SauceNAO's burst-window
//! wait and its one retry after a 429, and the module state Python keeps in globals
//! (`ascii2d._blocked`, SauceNAO's last quota and daily-limit latch). Where a function in this
//! module would have set such a global, it returns the new value (SauceNAO's [`QuotaReading`] and
//! daily-limit flag) or a distinct error (ascii2d's `Blocked`) for the client to act on.
//!
//! A function that reads more than one response (ascii2d's colour page, then the feature page it
//! points at) takes them all as arguments. A response is `(status, final URL, text)`.
//!
//! **Yandex and Google Images.** `core/yandex.py` and `core/google_images.py` are cut the same
//! way; the request half of each (`search`, the module state `yandex._blocked` and
//! `google_images._blocked`) is Phase 3.
//!
//! No function here opens a socket or reads a file.

pub mod ascii2d;
pub mod google_images;
pub mod google_lens;
pub mod iqdb;
pub mod pawchive_lookup;
pub mod saucenao;
pub mod tracemoe;
pub mod yandex;

use std::sync::LazyLock;

use regex::Regex;

use crate::boorus::sizes::to_int;

/// An HTTP answer, as far as the parsers read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response<'a> {
    pub status: u16,
    /// The URL the answer came from after redirects (`resp.url`); empty when the client has none.
    pub url: &'a str,
    pub text: &'a str,
}

/// `(\d+)\s*[x×]\s*(\d+)` (`\s` is Python's: `[\s\x1c-\x1f]`), the dimensions in a result's
/// text. `\d` is Unicode in both.
static DIMENSIONS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d+)[\s\x1c-\x1f]*[x\u{d7}][\s\x1c-\x1f]*(\d+)").expect("dimensions regex")
});

/// `_extract_dimensions`: the first `WxH` in `text`, as `(width, height)`, or `(None, None)`. A
/// number past `i64` reads as absent (Python's `int` has no limit).
pub(crate) fn extract_dimensions(text: &str) -> (Option<i64>, Option<i64>) {
    match DIMENSIONS_RE.captures(text) {
        Some(m) => (to_int(&m[1]), to_int(&m[2])),
        None => (None, None),
    }
}

/// `href` as an absolute web URL the way IQDB and SauceNAO accept one: protocol-relative
/// (`//host/...`) or `http://` / `https://`, after stripping.
pub(crate) fn looks_like_url(href: &str) -> bool {
    let href = crate::pycompat::strip(href);
    href.starts_with("//") || href.starts_with("http://") || href.starts_with("https://")
}

/// Python raised, instead of returning, because of an input the service never sends: a value of
/// a type the code does not check (a results `thumb` that is a list, a `url` that is a number, a
/// Vision `responses` that is a string), or a URL `urlparse` refuses with `ValueError`. In Python the exception leaves the engine's `search` and the engine runner reports
/// the engine as failed; it is not "no results", which would be recorded as a search that found
/// nothing. The Phase 3 client is told which, and maps this to its own failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnexpectedInput;

impl std::fmt::Display for UnexpectedInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an input on which the Python parser raises")
    }
}

impl std::error::Error for UnexpectedInput {}

/// What `for x in value or []` visits, for the elements that are objects. A falsy value is empty;
/// an array is its items; a non-empty object or string is iterated by key or by character, and
/// neither is an object; a number or `true` is not iterable, which Python raises on.
pub(crate) fn objects_in(
    value: Option<&serde_json::Value>,
) -> Result<Vec<&serde_json::Map<String, serde_json::Value>>, UnexpectedInput> {
    use serde_json::Value;
    match value {
        None | Some(Value::Null | Value::Bool(false)) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.iter().filter_map(Value::as_object).collect()),
        Some(Value::Object(_) | Value::String(_)) => Ok(Vec::new()),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => Ok(Vec::new()),
        Some(Value::Bool(true) | Value::Number(_)) => Err(UnexpectedInput),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_take_the_first_pair_with_either_separator() {
        assert_eq!(
            extract_dimensions("1200x1600 [x]"),
            (Some(1200), Some(1600))
        );
        assert_eq!(
            extract_dimensions("a 800 \u{d7}\u{1f}600 b 1x2"),
            (Some(800), Some(600))
        );
        assert_eq!(extract_dimensions("1200 X 1600"), (None, None));
        assert_eq!(extract_dimensions(""), (None, None));
        // Any script's digits, as Python's `\d` and `int()` read them.
        assert_eq!(
            extract_dimensions("\u{661}\u{662}x\u{663}"),
            (Some(12), Some(3))
        );
        // Past `i64`: absent, where Python would keep the integer.
        assert_eq!(
            extract_dimensions("99999999999999999999x5"),
            (None, Some(5))
        );
    }

    #[test]
    fn urls_are_absolute_or_protocol_relative_after_stripping() {
        assert!(looks_like_url("https://a.example/x"));
        assert!(looks_like_url("  //a.example/x"));
        assert!(looks_like_url("http://a"));
        assert!(!looks_like_url("/posts/1"));
        assert!(!looks_like_url("ftp://a"));
        assert!(!looks_like_url(""));
        assert!(!looks_like_url("HTTPS://a"));
    }
}
