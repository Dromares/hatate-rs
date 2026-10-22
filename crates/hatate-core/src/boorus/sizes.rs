//! Reading image dimensions off a booru post page (`core/boorus/_sizes.py`).
//!
//! Gelbooru, Safebooru, Yande.re and Konachan all descend from Danbooru 1.x, so they share a
//! Statistics sidebar of the form
//!
//! ```text
//! Id: 3484690
//! Posted: 2016-12-23 13:27:09
//! Size: 1232x918
//! Rating: Explicit
//! ```
//!
//! The interesting part, which character sits between the two numbers, is a property of that
//! shared lineage, not of one site: it renders as ASCII "x" in some views and as `&times;`
//! (U+00D7) in others, and a pattern matching only "x" silently returns no dimensions at all.

use std::sync::LazyLock;

use regex::Regex;

use crate::pycompat;

/// Every separator these sites have been seen to use, plus the spacing variants that appear when
/// the numbers sit in separate elements.
static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bSize:\s*(\d[\d,]*)\s*[x\u{d7}\u{2715}\u{2716}*]\s*(\d[\d,]*)")
        .expect("size regex")
});

/// Anything of the form "1500x2000": a looser fallback for pages that state the dimensions
/// without the "Size:" label, e.g. inside a download link's text.
static BARE_DIMENSIONS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(\d{2,6})\s*[x\u{d7}\u{2715}\u{2716}]\s*(\d{2,6})\b")
        .expect("bare dimensions regex")
});

/// Used only to explain a miss in the log.
static SIZE_CONTEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is).{0,40}\bSize:.{0,40}").expect("size context regex"));

/// `\p{Nd}`, the characters Python's `\d` and `int()` both accept.
static DECIMAL_DIGIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\p{Nd}$").expect("digit regex"));

fn is_decimal(c: char) -> bool {
    DECIMAL_DIGIT.is_match(c.encode_utf8(&mut [0; 4]))
}

/// The value of a decimal digit in any script (`int("١٢")` is 12 in Python). Decimal digits come
/// in unbroken runs of ten, zero first (a Unicode stability guarantee), so the value is the
/// offset into the run it sits in.
fn digit_value(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(10) {
        return Some(d);
    }
    if !is_decimal(c) {
        return None;
    }
    let mut zero = c as u32;
    while let Some(prev) = char::from_u32(zero - 1)
        && is_decimal(prev)
    {
        zero -= 1;
    }
    Some((c as u32 - zero) % 10)
}

/// `int(raw.replace(",", ""))` for text the patterns above matched. `None` where the number does
/// not fit an `i64` (Python has no such limit; no real page states one that large).
pub(super) fn to_int(raw: &str) -> Option<i64> {
    raw.chars().filter(|&c| c != ',').try_fold(0i64, |acc, c| {
        acc.checked_mul(10)?.checked_add(i64::from(digit_value(c)?))
    })
}

/// `int(raw)` for text Python's `int()` accepts: surrounding whitespace, one optional sign, then
/// decimal digits of any script with single underscores between them. `None` where Python raises
/// `ValueError`, or where the value does not fit an `i64`.
pub(super) fn py_int(raw: &str) -> Option<i64> {
    let s = pycompat::strip(raw);
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c == '_' || is_decimal(c));
    if !well_formed {
        return None;
    }
    // Accumulate negatively so that i64::MIN parses.
    digits
        .chars()
        .filter(|&c| c != '_')
        .try_fold(0i64, |acc, c| {
            let digit = i64::from(digit_value(c)?);
            let acc = acc.checked_mul(10)?;
            if negative {
                acc.checked_sub(digit)
            } else {
                acc.checked_add(digit)
            }
        })
}

fn pair(text: &str, re: &Regex) -> (Option<i64>, Option<i64>) {
    match re.captures(text) {
        Some(m) => (to_int(&m[1]), to_int(&m[2])),
        None => (None, None),
    }
}

/// Dimensions from a "Size: WxH" label, or `(None, None)`.
pub fn parse_size_label(text: &str) -> (Option<i64>, Option<i64>) {
    pair(text, &SIZE_RE)
}

/// Dimensions from an unlabelled "1500x2000", for pages that state them without the Statistics
/// label.
///
/// Deliberately a SECOND choice: without the label nothing ties the numbers to the image, so on
/// the wrong slice of text this could pick up something unrelated. Callers should hand it a
/// narrow region, a single link's text, not a whole page.
pub fn parse_bare_dimensions(text: &str) -> (Option<i64>, Option<i64>) {
    pair(text, &BARE_DIMENSIONS_RE)
}

/// What the page actually says around "Size:", for logging a miss. Guessing at this markup from
/// outside has already produced one wrong fix, so a failure should report what it saw.
pub fn describe_size_context(text: &str) -> String {
    match SIZE_CONTEXT_RE.find(text) {
        Some(m) => pycompat::split(m.as_str()).collect::<Vec<_>>().join(" "),
        None => "<no 'Size:' on the page>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_label_takes_every_separator() {
        for sep in [
            "x", "X", "\u{d7}", "\u{2715}", "\u{2716}", "*", " x ", "\u{d7} ",
        ] {
            let text = format!("Id: 1\nSize: 1232{sep}918\nRating: Explicit");
            assert_eq!(parse_size_label(&text), (Some(1232), Some(918)), "{sep:?}");
        }
        assert_eq!(
            parse_size_label("size: 1,232 x 1,918"),
            (Some(1232), Some(1918))
        );
        assert_eq!(parse_size_label("no size here"), (None, None));
        assert_eq!(parse_size_label(""), (None, None));
        // The label has to be a word of its own.
        assert_eq!(parse_size_label("Filesize: 1x2"), (None, None));
    }

    #[test]
    fn bare_dimensions_need_two_to_six_digits_each_side() {
        assert_eq!(
            parse_bare_dimensions("PNG (1500x2000, 2.5 MB)"),
            (Some(1500), Some(2000))
        );
        assert_eq!(
            parse_bare_dimensions("1500 \u{d7} 2000"),
            (Some(1500), Some(2000))
        );
        assert_eq!(parse_bare_dimensions("5x7"), (None, None));
        assert_eq!(parse_bare_dimensions("1234567x7654321"), (None, None));
        // The `*` the label accepts is not accepted here.
        assert_eq!(parse_bare_dimensions("1500*2000"), (None, None));
    }

    #[test]
    fn digits_in_other_scripts_count_as_python_counts_them() {
        // Arabic-Indic, fullwidth and mathematical bold digits all go through `int()`.
        assert_eq!(
            parse_size_label("Size: \u{661}\u{662}x\u{663}"),
            (Some(12), Some(3))
        );
        assert_eq!(
            parse_size_label("Size: \u{ff11}\u{ff19}x\u{ff10}"),
            (Some(19), Some(0))
        );
        assert_eq!(
            parse_size_label("Size: \u{1d7ce}\u{1d7d8}x\u{1d7d7}"),
            (Some(0), Some(9))
        );
        // Too large for an i64: Python would return the big int, this says nothing.
        assert_eq!(
            parse_size_label("Size: 99999999999999999999x1"),
            (None, Some(1))
        );
    }

    #[test]
    fn py_int_reads_what_pythons_int_reads() {
        for (raw, want) in [
            ("12", Some(12)),
            (" +7\t", Some(7)),
            ("-5", Some(-5)),
            ("1_000", Some(1000)),
            ("\u{661}\u{662}", Some(12)),
            ("\u{2003}8\u{2003}", Some(8)),
            ("-9223372036854775808", Some(i64::MIN)),
            ("9223372036854775808", None),
            ("", None),
            ("+", None),
            ("--1", None),
            ("+ 1", None),
            ("1__0", None),
            ("_1", None),
            ("1_", None),
            ("1.5", None),
            ("1e3", None),
            ("0x10", None),
            ("None", None),
        ] {
            assert_eq!(py_int(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn a_miss_is_described_by_what_the_page_says() {
        assert_eq!(
            describe_size_context("<li>Id: 1</li>\n<li>Size:   huge  </li>"),
            "<li>Id: 1</li> <li>Size: huge </li>"
        );
        assert_eq!(describe_size_context("nothing"), "<no 'Size:' on the page>");
    }
}
