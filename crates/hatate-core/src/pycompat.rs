//! Python's number coercions, for reading files the Python app wrote (or might read).
//!
//! The Python readers use `float(x)`, `int(x)` and `x or 0` on whatever `json.loads`
//! returned, so a stored string `"12"` or `12.0` still counts as 12. These helpers copy
//! the cases that can occur in a file; anything Python would raise `TypeError` on is
//! `None` here, which every caller turns into "treat as malformed".

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
