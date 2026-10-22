//! Reading JSON the way Python's `json.loads` does, for the two places it differs from
//! strict JSON in files the Python app itself wrote.
//!
//! `json.dumps` writes `NaN`, `Infinity` and `-Infinity` for non-finite floats, and writes a
//! lone surrogate (a file name that was not valid UTF-8) as an unpaired `\udXXX` escape.
//! `json.loads` takes both back; `serde_json` rejects both, which would make Rust drop a row
//! Python reads (rule 6.2.1, 6.5). So a document that fails to parse is normalised once and
//! tried again: non-finite tokens become `null` (a number the model cannot hold becomes the
//! field's default), an unpaired surrogate escape becomes U+FFFD. Documents that parse the
//! first time are never touched.

use serde_json::Value;

/// Parse `text`, falling back to the normalised text when strict parsing fails.
pub(crate) fn parse(text: &str) -> Result<Value, serde_json::Error> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(first) => {
            let fixed = normalise(text);
            if fixed == text {
                return Err(first);
            }
            serde_json::from_str::<Value>(&fixed).map_err(|_| first)
        }
    }
}

/// `text` with non-finite number tokens and unpaired surrogate escapes replaced.
pub(crate) fn normalise(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_string = false;
    while i < b.len() {
        let c = b[i];
        if in_string {
            match c {
                b'"' => {
                    in_string = false;
                    out.push('"');
                    i += 1;
                }
                b'\\' if b.get(i + 1) == Some(&b'u') => {
                    let Some(unit) = hex4(b, i + 2) else {
                        // Not four hex digits: leave it for the parser to reject.
                        out.push_str("\\u");
                        i += 2;
                        continue;
                    };
                    match unit {
                        0xD800..=0xDBFF => match (b.get(i + 6), b.get(i + 7), hex4(b, i + 8)) {
                            (Some(b'\\'), Some(b'u'), Some(low @ 0xDC00..=0xDFFF)) => {
                                out.push_str(&format!("\\u{unit:04x}\\u{low:04x}"));
                                i += 12;
                            }
                            _ => {
                                out.push_str("\\ufffd");
                                i += 6;
                            }
                        },
                        0xDC00..=0xDFFF => {
                            out.push_str("\\ufffd");
                            i += 6;
                        }
                        _ => {
                            out.push_str(&text[i..i + 6]);
                            i += 6;
                        }
                    }
                }
                b'\\' => {
                    // Any other escape: copy both bytes so `\"` does not end the string.
                    let end = (i + 2).min(b.len());
                    out.push_str(&text[i..end]);
                    i = end;
                }
                _ => {
                    // Copy one whole character.
                    let len = utf8_len(c);
                    out.push_str(&text[i..i + len]);
                    i += len;
                }
            }
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push('"');
            i += 1;
        } else if let Some(len) = nonfinite_token(b, i) {
            out.push_str("null");
            i += len;
        } else {
            let len = utf8_len(c);
            out.push_str(&text[i..i + len]);
            i += len;
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0..=0x7f => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

fn hex4(b: &[u8], at: usize) -> Option<u32> {
    let digits = b.get(at..at + 4)?;
    let mut v = 0u32;
    for d in digits {
        v = v * 16 + (*d as char).to_digit(16)?;
    }
    Some(v)
}

/// Length of a `NaN` / `Infinity` / `-Infinity` token starting at `i`, when it stands alone.
fn nonfinite_token(b: &[u8], i: usize) -> Option<usize> {
    for token in ["NaN", "Infinity", "-Infinity"] {
        let t = token.as_bytes();
        if b[i..].starts_with(t) {
            let after = b.get(i + t.len());
            let ends = after.is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
            let before = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
            if ends && before {
                return Some(t.len());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_documents_are_untouched() {
        let v = parse(r#"{"a": [1, 2.5, "NaN", "\ud83d\ude00"]}"#).unwrap();
        assert_eq!(v["a"][2], "NaN");
        assert_eq!(v["a"][3], "😀");
    }

    #[test]
    fn python_extensions_are_read() {
        let v = parse(r#"{"s": NaN, "i": Infinity, "n": -Infinity, "t": "NaN Infinity"}"#).unwrap();
        assert!(v["s"].is_null() && v["i"].is_null() && v["n"].is_null());
        assert_eq!(v["t"], "NaN Infinity");
    }

    #[test]
    fn lone_surrogates_become_replacement_characters() {
        let v = parse(r#"{"p": "a\udcffb", "q": "\ud800", "ok": "\ud83d\ude00", "e": "\\ud800"}"#)
            .unwrap();
        assert_eq!(v["p"], "a\u{fffd}b");
        assert_eq!(v["q"], "\u{fffd}");
        assert_eq!(v["ok"], "😀");
        // An escaped backslash is not a surrogate escape.
        assert_eq!(v["e"], "\\ud800");
    }

    #[test]
    fn real_syntax_errors_stay_errors() {
        assert!(parse("{").is_err());
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"a": NaNa}"#).is_err());
        assert!(parse(r#"{"a": "\u12"}"#).is_err());
        assert!(parse("").is_err());
    }
}
