//! Python's number coercions, for reading files the Python app wrote (or might read).
//!
//! The Python readers use `float(x)`, `int(x)` and `x or 0` on whatever `json.loads`
//! returned, so a stored string `"12"` or `12.0` still counts as 12. These helpers copy
//! the cases that can occur in a file; anything Python would raise `TypeError` on is
//! `None` here, which every caller turns into "treat as malformed".

use std::sync::LazyLock;

use markup5ever::data::NAMED_ENTITIES;
use regex::Regex;
use serde_json::Value;

/// Python truthiness of a JSON value (`x or default`).
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `int(x)`: ints, floats (truncated), bools and numeric strings.
pub fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| {
            let f = n.as_f64()?;
            (f.is_finite() && f.abs() < 9.0e18).then_some(f.trunc() as i64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `float(x)`: numbers, bools and numeric strings.
pub fn float(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `isinstance(x, (int, float))` (so `True` counts), as a float.
pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Python's `str.isspace()`: Unicode White_Space plus the four ASCII separators U+001C..U+001F,
/// which Python counts and Rust does not.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `s.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `s.split()`: runs of whitespace separate, empty pieces are dropped.
pub fn split(s: &str) -> impl Iterator<Item = &str> {
    s.split(is_space).filter(|piece| !piece.is_empty())
}

/// `html.unescape(s)`: HTML5 character references decoded the way CPython's `html` module does,
/// which is not the way a browser decodes an attribute. A named reference with no `;` is decoded
/// when it is one of the legacy names (`&amp`, `&copy`), and also by its longest legacy prefix
/// (`&ampfoo` is `&foo`), wherever it sits; a numeric one outside Unicode or in the C1 range is
/// remapped as CPython remaps it. The named table is the WHATWG one `html5ever` decodes with.
pub fn html_unescape(s: &str) -> String {
    static CHARREF: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0c <&#;]{1,32};?)")
            .expect("charref regex")
    });
    if !s.contains('&') {
        return s.to_string();
    }
    CHARREF
        .replace_all(s, |caps: &regex::Captures| {
            let body = &caps[1];
            match body.strip_prefix('#') {
                Some(number) => numeric_charref(number),
                None => named_charref(body),
            }
        })
        .into_owned()
}

fn numeric_charref(number: &str) -> String {
    let digits = number.trim_end_matches(';');
    let parsed = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => digits.parse(),
    };
    // Python's ints do not overflow; anything this large is past 0x10FFFF either way.
    let num = parsed.unwrap_or(u32::MAX);
    if let Some(&(_, replacement)) = INVALID_CHARREFS.iter().find(|(n, _)| *n == num) {
        return replacement.to_string();
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
        return '\u{fffd}'.to_string();
    }
    if is_invalid_codepoint(num) {
        return String::new();
    }
    char::from_u32(num).map_or_else(String::new, String::from)
}

fn named_charref(name: &str) -> String {
    let lookup = |key: &str| {
        NAMED_ENTITIES
            .get(key)
            .filter(|(first, _)| *first != 0)
            .map(|&(first, second)| {
                [first, second]
                    .into_iter()
                    .filter(|&cp| cp != 0)
                    .filter_map(char::from_u32)
                    .collect::<String>()
            })
    };
    if let Some(found) = lookup(name) {
        return found;
    }
    // The longest prefix (two characters or more, never the whole name) that is a legacy name.
    let ends: Vec<usize> = name.char_indices().map(|(i, _)| i).skip(2).collect();
    for end in ends.into_iter().rev() {
        if let Some(found) = lookup(&name[..end]) {
            return found + &name[end..];
        }
    }
    format!("&{name}")
}

/// `html._invalid_charrefs`: the numbers CPython maps to something other than their own
/// character (the Windows-1252 reading of 0x80..=0x9F, NUL, and CR).
const INVALID_CHARREFS: [(u32, char); 34] = [
    (0x00, '\u{fffd}'),
    (0x0d, '\r'),
    (0x80, '\u{20ac}'),
    (0x81, '\u{81}'),
    (0x82, '\u{201a}'),
    (0x83, '\u{192}'),
    (0x84, '\u{201e}'),
    (0x85, '\u{2026}'),
    (0x86, '\u{2020}'),
    (0x87, '\u{2021}'),
    (0x88, '\u{2c6}'),
    (0x89, '\u{2030}'),
    (0x8a, '\u{160}'),
    (0x8b, '\u{2039}'),
    (0x8c, '\u{152}'),
    (0x8d, '\u{8d}'),
    (0x8e, '\u{17d}'),
    (0x8f, '\u{8f}'),
    (0x90, '\u{90}'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201c}'),
    (0x94, '\u{201d}'),
    (0x95, '\u{2022}'),
    (0x96, '\u{2013}'),
    (0x97, '\u{2014}'),
    (0x98, '\u{2dc}'),
    (0x99, '\u{2122}'),
    (0x9a, '\u{161}'),
    (0x9b, '\u{203a}'),
    (0x9c, '\u{153}'),
    (0x9d, '\u{9d}'),
    (0x9e, '\u{17e}'),
    (0x9f, '\u{178}'),
];

/// `html._invalid_codepoints`: control characters and noncharacters, which decode to nothing.
fn is_invalid_codepoint(num: u32) -> bool {
    matches!(num, 0x1..=0x8 | 0xb | 0xe..=0x1f | 0x7f..=0x9f | 0xfdd0..=0xfdef)
        || (num & 0xfffe) == 0xfffe
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coercions_follow_python() {
        assert_eq!(int(&json!(12)), Some(12));
        assert_eq!(int(&json!(12.9)), Some(12));
        assert_eq!(int(&json!(-12.9)), Some(-12));
        assert_eq!(int(&json!(" 12 ")), Some(12));
        assert_eq!(int(&json!(true)), Some(1));
        assert_eq!(int(&json!("x")), None);
        assert_eq!(int(&json!(null)), None);
        assert_eq!(int(&json!([1])), None);
        assert_eq!(float(&json!("1.5")), Some(1.5));
        assert_eq!(float(&json!(3)), Some(3.0));
        assert_eq!(float(&json!(null)), None);
        assert_eq!(number(&json!("1")), None);
        assert_eq!(number(&json!(false)), Some(0.0));
        assert!(!truthy(&json!(0)) && !truthy(&json!("")) && !truthy(&json!([])));
        assert!(truthy(&json!(1)) && truthy(&json!("0")) && truthy(&json!({"a": 1})));
    }

    #[test]
    fn string_helpers_follow_python_whitespace() {
        // U+001F is whitespace to Python and not to Rust's `trim`; U+200B is neither.
        assert_eq!(strip("\u{1f} a b \u{a0}\u{3000}"), "a b");
        assert_eq!(strip("\u{200b}a"), "\u{200b}a");
        let pieces: Vec<&str> = split(" a\u{1c}b\t\u{85}c  ").collect();
        assert_eq!(pieces, ["a", "b", "c"]);
        assert_eq!(split("   ").count(), 0);
    }
}
