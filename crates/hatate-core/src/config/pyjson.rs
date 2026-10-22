//! Writer for the byte format of Python's `json.dumps(obj, indent=2)` (spec 2.1).
//!
//! Only what `config.json` needs: 2-space indent, `": "` after keys, `,` at the end of
//! the line, `\uXXXX` for everything outside printable ASCII, floats in Python `repr`
//! form, `[]` and `{}` for empty containers, no trailing newline.

use std::fmt::Write;

/// `"\n"` plus two spaces per level.
pub(crate) fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// A JSON string with `ensure_ascii=True`: `ESCAPE_ASCII` in CPython's `json.encoder`
/// escapes everything outside space..`~`, so DEL is `\u007f` too. Lower-case hex.
pub(crate) fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

/// A float the way Python's `float.__repr__` prints it (`repr`, "short" style).
///
/// Shortest digits that round-trip; fixed notation when the decimal exponent is in
/// `-4 <= exp10 < 16`, otherwise `d.ddde+XX` with at least two exponent digits; a
/// fixed-notation integer keeps a `.0`. Non-finite values use Python's `json` tokens.
pub(crate) fn float(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x.is_infinite() {
        out.push_str(if x < 0.0 { "-Infinity" } else { "Infinity" });
        return;
    }
    if x == 0.0 {
        out.push_str(if x.is_sign_negative() { "-0.0" } else { "0.0" });
        return;
    }
    // Rust's `{:e}` prints the shortest round-trip digits: "-1.2345e-5", "1e16".
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("LowerExp always has an exponent");
    let exp10: i32 = exp.parse().expect("LowerExp exponent is an integer");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    out.push_str(sign);
    if (-4..16).contains(&exp10) {
        if exp10 < 0 {
            out.push_str("0.");
            for _ in 0..(-exp10 - 1) {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            let int_len = exp10 as usize + 1;
            if digits.len() <= int_len {
                out.push_str(&digits);
                for _ in 0..(int_len - digits.len()) {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if exp10 < 0 { '-' } else { '+' },
            exp10.abs()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: f64) -> String {
        let mut s = String::new();
        float(&mut s, x);
        s
    }

    fn s(x: &str) -> String {
        let mut o = String::new();
        string(&mut o, x);
        o
    }

    // Expected values are Python 3 `repr(float)`.
    #[test]
    fn floats_match_python_repr() {
        assert_eq!(f(45.0), "45.0");
        assert_eq!(f(0.5), "0.5");
        assert_eq!(f(-2.25), "-2.25");
        assert_eq!(f(0.0), "0.0");
        assert_eq!(f(-0.0), "-0.0");
        assert_eq!(f(0.1), "0.1");
        assert_eq!(f(1234.5678), "1234.5678");
        assert_eq!(f(0.0001), "0.0001");
        assert_eq!(f(0.00001), "1e-05");
        assert_eq!(f(0.000123), "0.000123");
        assert_eq!(f(0.0000123), "1.23e-05");
        assert_eq!(f(1e15), "1000000000000000.0");
        assert_eq!(f(1e16), "1e+16");
        assert_eq!(f(1.5e16), "1.5e+16");
        assert_eq!(f(123456789012345680.0), "1.2345678901234568e+17");
        assert_eq!(f(1e300), "1e+300");
        assert_eq!(f(5e-324), "5e-324");
        assert_eq!(f(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(f(100.0), "100.0");
        assert_eq!(f(12345.0), "12345.0");
        assert_eq!(f(f64::NAN), "NaN");
        assert_eq!(f(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn strings_match_ensure_ascii() {
        assert_eq!(s("plain"), "\"plain\"");
        assert_eq!(s("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(s("\n\r\t\u{8}\u{c}"), "\"\\n\\r\\t\\b\\f\"");
        assert_eq!(s("\u{1}\u{1f}\u{7f}"), "\"\\u0001\\u001f\\u007f\"");
        assert_eq!(s("é"), "\"\\u00e9\"");
        assert_eq!(s("日本"), "\"\\u65e5\\u672c\"");
        // Outside the BMP: a surrogate pair, as Python writes it.
        assert_eq!(s("😀"), "\"\\ud83d\\ude00\"");
        // The forward slash is not escaped.
        assert_eq!(s("a/b"), "\"a/b\"");
    }
}
