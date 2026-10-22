//! A `requests.Response` as the Python callers read it: `.text`, `.json()`, `.encoding` and the
//! header lookups (`requests` 2.34, `core/net.py`, `core/boorus/__init__.py`).
//!
//! The net layer hands back bytes. Python code never looks at bytes: it reads `resp.text`, which
//! decodes with the charset the server declared (or, with none declared on a `text/*` type, with
//! ISO-8859-1), and `resp.json()`. Doing the same here is what keeps a body in a legacy charset,
//! or one with no `Content-Type` at all, reading the way Python reads it.
//!
//! What is supported, and what is not (`docs/net.md`, "Response text"):
//!
//! - Charsets: UTF-8, ASCII, ISO-8859-1 (Latin-1) and Windows-1252, under the names and aliases
//!   Python's codec registry knows for them, and UTF-16 and UTF-32 with their BOM rules. A
//!   declared charset Python would also decode (Shift_JIS, EUC-JP, GBK, ...) is decoded as UTF-8
//!   here, and one Python has no codec for is decoded as UTF-8 with replacement in both
//!   (`LookupError` in `Response.text`).
//! - `Content-Type` missing: `requests` asks `charset_normalizer` to guess. Here the body is read
//!   as UTF-8 with replacement; a body that is not UTF-8 reads differently.

use bytes::Bytes;
use http::HeaderMap;
use serde_json::Value;

use crate::net::NetResponse;

/// A fully read response, with the encoding `requests` would have chosen for it.
#[derive(Debug, Clone)]
pub struct PyResponse {
    pub status: u16,
    pub headers: HeaderMap,
    /// The final URL, after redirects (`resp.url`).
    pub url: String,
    pub content: Bytes,
    /// `resp.encoding`: what `get_encoding_from_headers` gave, until a caller overwrites it.
    pub encoding: Option<String>,
}

impl PyResponse {
    pub fn from_net(resp: NetResponse) -> PyResponse {
        let encoding = encoding_from_headers(header(&resp.headers, "content-type").as_deref());
        PyResponse {
            status: resp.status.as_u16(),
            url: resp.url.to_string(),
            content: resp.body,
            headers: resp.headers,
            encoding,
        }
    }

    /// `resp.headers.get(name)`: the value as Python reads it (bytes as Latin-1).
    pub fn header(&self, name: &str) -> Option<String> {
        header(&self.headers, name)
    }

    /// `resp.text`.
    pub fn text(&self) -> String {
        if self.content.is_empty() {
            return String::new();
        }
        decode(&self.content, self.encoding.as_deref())
    }

    /// `resp.json()`. An error is `ValueError` in Python (`JSONDecodeError`), which every caller
    /// here catches as "this is not JSON".
    pub fn json(&self) -> Result<Value, serde_json::Error> {
        if self.encoding.is_none() && self.content.len() > 3 {
            // `guess_json_utf`, then a strict decode; a body that does not decode falls through
            // to `.text`, and one that decodes but does not parse is the error.
            if let Some(text) =
                guess_json_utf(&self.content).and_then(|enc| strict(&self.content, enc))
            {
                return serde_json::from_str(&text);
            }
        }
        serde_json::from_str(&self.text())
    }
}

/// A header value as Python reads it: each byte is the code point of that number.
pub fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(name)?;
    Some(value.as_bytes().iter().map(|b| char::from(*b)).collect())
}

/// `requests.utils.get_encoding_from_headers` for the one header it reads.
pub fn encoding_from_headers(content_type: Option<&str>) -> Option<String> {
    let content_type = content_type.filter(|c| !c.is_empty())?;
    let mut tokens = content_type.split(';');
    let main = tokens.next().unwrap_or("").trim();
    for param in tokens {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        if let Some(eq) = param.find('=') {
            let key = param[..eq].trim_matches(['"', '\'', ' ']).to_lowercase();
            if key == "charset" {
                let value = param[eq + 1..].trim_matches(['"', '\'', ' ']);
                return Some(value.trim_matches(['\'', '"']).to_string());
            }
        }
    }
    if main.contains("text") {
        return Some("ISO-8859-1".to_string());
    }
    if main.contains("application/json") {
        return Some("utf-8".to_string());
    }
    None
}

/// Python's codec-registry normalisation of an encoding name: lower case, runs of anything but a
/// letter, digit or `.` as one `_`.
fn normalise(name: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending = true;
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Utf8,
    Utf8Sig,
    Ascii,
    Latin1,
    Cp1252,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

fn codec(name: &str) -> Option<Codec> {
    Some(match normalise(name).as_str() {
        "utf_8" | "utf8" | "u8" | "utf" | "utf8_ucs2" | "utf8_ucs4" | "cp65001" => Codec::Utf8,
        "utf_8_sig" => Codec::Utf8Sig,
        "ascii" | "us_ascii" | "646" | "ansi_x3.4_1968" | "ansi_x3_4_1968" | "ansi_x3.4_1986"
        | "cp367" | "csascii" | "ibm367" | "iso646_us" | "iso_646.irv_1991" | "iso_ir_6" | "us" => {
            Codec::Ascii
        }
        "latin_1" | "latin1" | "latin" | "l1" | "iso8859_1" | "iso_8859_1" | "iso_8859_1_1987"
        | "iso_ir_100" | "8859" | "cp819" | "ibm819" | "csisolatin1" | "iso8859" => Codec::Latin1,
        "cp1252" | "windows_1252" | "1252" => Codec::Cp1252,
        "utf_16" | "utf16" | "u16" => Codec::Utf16,
        "utf_16_le" | "utf_16le" | "utf16le" => Codec::Utf16Le,
        "utf_16_be" | "utf_16be" | "utf16be" => Codec::Utf16Be,
        "utf_32" | "utf32" | "u32" => Codec::Utf32,
        "utf_32_le" | "utf_32le" | "utf32le" => Codec::Utf32Le,
        "utf_32_be" | "utf_32be" | "utf32be" => Codec::Utf32Be,
        _ => return None,
    })
}

/// `str(content, encoding, errors="replace")`, falling back to UTF-8 with replacement for an
/// encoding this port does not carry (see the module docs).
pub fn decode(content: &[u8], encoding: Option<&str>) -> String {
    match encoding.and_then(codec) {
        Some(codec) => decode_with(content, codec),
        None => String::from_utf8_lossy(content).into_owned(),
    }
}

/// CP1252 differs from Latin-1 only in 0x80..=0x9F; five of those bytes are undefined.
const CP1252_HIGH: [char; 32] = [
    '\u{20ac}', '\u{fffd}', '\u{201a}', '\u{0192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02c6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{fffd}', '\u{017d}', '\u{fffd}',
    '\u{fffd}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02dc}', '\u{2122}', '\u{0161}', '\u{203a}', '\u{0153}', '\u{fffd}', '\u{017e}', '\u{0178}',
];

fn decode_with(content: &[u8], codec: Codec) -> String {
    match codec {
        Codec::Utf8 => String::from_utf8_lossy(content).into_owned(),
        Codec::Utf8Sig => {
            String::from_utf8_lossy(content.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(content))
                .into_owned()
        }
        Codec::Ascii => content
            .iter()
            .map(|b| {
                if *b < 0x80 {
                    char::from(*b)
                } else {
                    '\u{fffd}'
                }
            })
            .collect(),
        Codec::Latin1 => content.iter().map(|b| char::from(*b)).collect(),
        Codec::Cp1252 => content
            .iter()
            .map(|b| match *b {
                0x80..=0x9f => CP1252_HIGH[usize::from(*b - 0x80)],
                b => char::from(b),
            })
            .collect(),
        Codec::Utf16 | Codec::Utf16Le | Codec::Utf16Be => decode_utf16(content, codec),
        Codec::Utf32 | Codec::Utf32Le | Codec::Utf32Be => decode_utf32(content, codec),
    }
}

fn decode_utf16(content: &[u8], codec: Codec) -> String {
    let (mut body, mut big) = (content, matches!(codec, Codec::Utf16Be));
    if codec == Codec::Utf16 {
        // Native order is little-endian here; a BOM chooses and is consumed.
        if content.starts_with(&[0xff, 0xfe]) {
            body = &content[2..];
        } else if content.starts_with(&[0xfe, 0xff]) {
            body = &content[2..];
            big = true;
        }
    }
    let mut units = Vec::with_capacity(body.len() / 2);
    for pair in body.chunks_exact(2) {
        units.push(if big {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        });
    }
    let mut out: String = char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{fffd}'))
        .collect();
    if body.len() % 2 == 1 {
        out.push('\u{fffd}');
    }
    out
}

fn decode_utf32(content: &[u8], codec: Codec) -> String {
    let (mut body, mut big) = (content, matches!(codec, Codec::Utf32Be));
    if codec == Codec::Utf32 {
        if content.starts_with(&[0xff, 0xfe, 0, 0]) {
            body = &content[4..];
        } else if content.starts_with(&[0, 0, 0xfe, 0xff]) {
            body = &content[4..];
            big = true;
        }
    }
    let mut out = String::new();
    for quad in body.chunks_exact(4) {
        let n = if big {
            u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]])
        } else {
            u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]])
        };
        out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
    }
    if body.len() % 4 != 0 {
        out.push('\u{fffd}');
    }
    out
}

/// A strict decode, for the JSON path: `None` where Python raises `UnicodeDecodeError`.
fn strict(content: &[u8], codec: Codec) -> Option<String> {
    match codec {
        Codec::Utf8 => std::str::from_utf8(content).ok().map(str::to_string),
        Codec::Utf8Sig => {
            std::str::from_utf8(content.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(content))
                .ok()
                .map(str::to_string)
        }
        _ => {
            let text = decode_with(content, codec);
            (!text.contains('\u{fffd}')).then_some(text)
        }
    }
}

/// `requests.utils.guess_json_utf`: which UTF a JSON body is in, from its BOM or its NUL pattern.
fn guess_json_utf(data: &[u8]) -> Option<Codec> {
    let sample = data.get(..4)?;
    if sample.starts_with(&[0xff, 0xfe, 0, 0]) || sample.starts_with(&[0, 0, 0xfe, 0xff]) {
        return Some(Codec::Utf32);
    }
    if sample.starts_with(&[0xef, 0xbb, 0xbf]) {
        // "utf-8-sig": the BOM is dropped before the parse.
        return Some(Codec::Utf8Sig);
    }
    if sample.starts_with(&[0xff, 0xfe]) || sample.starts_with(&[0xfe, 0xff]) {
        return Some(Codec::Utf16);
    }
    let nulls = sample.iter().filter(|b| **b == 0).count();
    if nulls == 0 {
        return Some(Codec::Utf8);
    }
    if nulls == 2 {
        if sample[0] == 0 && sample[2] == 0 {
            return Some(Codec::Utf16Be);
        }
        if sample[1] == 0 && sample[3] == 0 {
            return Some(Codec::Utf16Le);
        }
    }
    if nulls == 3 {
        if sample[..3] == [0, 0, 0] {
            return Some(Codec::Utf32Be);
        }
        if sample[1..] == [0, 0, 0] {
            return Some(Codec::Utf32Le);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_encoding_comes_from_the_declared_charset_then_the_type() {
        let enc = |h| encoding_from_headers(Some(h));
        assert_eq!(
            enc("text/html; charset=Shift_JIS").as_deref(),
            Some("Shift_JIS")
        );
        assert_eq!(
            enc("text/html; charset=\"utf-8\"").as_deref(),
            Some("utf-8")
        );
        assert_eq!(enc("text/html; CHARSET = 'x'").as_deref(), Some("x"));
        assert_eq!(enc("text/html").as_deref(), Some("ISO-8859-1"));
        assert_eq!(enc("application/json").as_deref(), Some("utf-8"));
        assert_eq!(enc("image/png"), None);
        assert_eq!(encoding_from_headers(None), None);
        assert_eq!(encoding_from_headers(Some("")), None);
    }

    #[test]
    fn text_is_decoded_the_way_requests_decodes_it() {
        assert_eq!(decode("é".as_bytes(), Some("utf-8")), "é");
        assert_eq!(decode("é".as_bytes(), Some("ISO-8859-1")), "Ã©");
        assert_eq!(decode(b"\x80\x81\xe9", Some("cp1252")), "\u{20ac}\u{fffd}é");
        assert_eq!(decode(b"a\xffb", Some("ascii")), "a\u{fffd}b");
        assert_eq!(decode(b"a\xffb", Some("utf-8")), "a\u{fffd}b");
        assert_eq!(decode(b"a\xffb", Some("no-such-codec")), "a\u{fffd}b");
        assert_eq!(decode(b"\xff\xfea\0b\0", Some("utf-16")), "ab");
        assert_eq!(decode(b"\xfe\xff\0a\0b", Some("utf-16")), "ab");
        assert_eq!(decode(b"a\0b\0", Some("utf-16")), "ab");
        assert_eq!(decode(b"\0a\0b", Some("UTF-16BE")), "ab");
        assert_eq!(decode(b"a\0\0\0", Some("utf_32")), "a");
        assert_eq!(decode(b"x", Some("utf-16")), "\u{fffd}");
    }

    #[test]
    fn json_with_no_encoding_is_sniffed_for_its_utf() {
        let resp = |body: &[u8]| PyResponse {
            status: 200,
            headers: HeaderMap::new(),
            url: String::new(),
            content: Bytes::copy_from_slice(body),
            encoding: None,
        };
        assert_eq!(resp(b"{\"a\": 1}").json().unwrap()["a"], 1);
        assert_eq!(
            resp(b"{\x00\"\x00a\x00\"\x00:\x001\x00}\x00")
                .json()
                .unwrap()["a"],
            1
        );
        assert!(resp(b"nope").json().is_err());
    }
}
