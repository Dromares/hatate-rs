//! The argument validation FastMCP puts in front of a tool, and the error text it produces.
//!
//! FastMCP builds a pydantic model from each handler's signature, pre-parses string arguments as
//! JSON (`FuncMetadata.pre_parse_json`), validates in pydantic's lax mode, and reports a failure
//! as `Error executing tool <name>: <str(ValidationError)>`. A client sees that text, and the
//! values it passes are the coerced ones (`"1"` becomes `1`), so both are reproduced here for the
//! four annotations the tool surface uses: `int`, `bool`, `Optional[str]` and `List[int]`.
//! Messages carry pydantic 2.13's wording and error URLs (`mcp 1.30.0`'s resolved version).

use serde_json::{Map, Number, Value};

use crate::pyrepr::py_repr;

/// pydantic's `errors.pydantic.dev/<version>/v/<type>` stem.
const ERROR_URL_STEM: &str = "https://errors.pydantic.dev/2.13/v/";

/// An annotation a tool argument can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Int,
    Bool,
    OptionalStr,
    ListOfInt,
}

/// One pydantic error (`loc`, `msg`, `type`, the offending input).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldError {
    pub loc: Vec<String>,
    pub msg: String,
    pub kind: &'static str,
    pub input: Value,
}

/// `type(x).__name__` for a JSON-derived value.
fn input_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// pydantic-core's `input_value` repr: the `repr()`, middle elided past 50 characters (the first
/// 25, `...`, the last 24).
fn truncated_repr(v: &Value) -> String {
    let repr = py_repr(v);
    let chars: Vec<char> = repr.chars().collect();
    if chars.len() <= 50 {
        return repr;
    }
    let head: String = chars[..25].iter().collect();
    let tail: String = chars[chars.len() - 24..].iter().collect();
    format!("{head}...{tail}")
}

/// `str(ValidationError)` for `model_name` (`get_entryArguments`).
pub fn input_type_name(v: &Value) -> &'static str {
    input_type(v)
}

/// `str(ValidationError)` for `model_name` (`get_entryArguments`).
pub fn format_errors(model_name: &str, errors: &[FieldError]) -> String {
    let n = errors.len();
    let mut out = format!(
        "{n} validation error{} for {model_name}",
        if n == 1 { "" } else { "s" }
    );
    for e in errors {
        out.push('\n');
        out.push_str(&e.loc.join("."));
        out.push_str(&format!(
            "\n  {} [type={}, input_value={}, input_type={}]\n    For further information visit {ERROR_URL_STEM}{}",
            e.msg,
            e.kind,
            truncated_repr(&e.input),
            input_type(&e.input),
            e.kind
        ));
    }
    out
}

pub(crate) fn err(loc: &[String], msg: &str, kind: &'static str, input: &Value) -> Box<FieldError> {
    Box::new(FieldError {
        loc: loc.to_vec(),
        msg: msg.to_owned(),
        kind,
        input: input.clone(),
    })
}

/// Python's `str.strip()` set, which pydantic-core applies before parsing a number.
fn strip(s: &str) -> &str {
    crate::pycompat::strip(s)
}

/// Digits of an int string with single underscores between digits allowed (`"1_0"` is 10).
fn clean_int_digits(body: &str) -> Option<String> {
    if body.is_empty() || !body.is_ascii() {
        return None;
    }
    let mut out = String::with_capacity(body.len());
    let mut prev_digit = false;
    for (i, c) in body.char_indices() {
        if c.is_ascii_digit() {
            out.push(c);
            prev_digit = true;
        } else if c == '_' && prev_digit && body[i + 1..].starts_with(|n: char| n.is_ascii_digit())
        {
            prev_digit = false;
        } else {
            return None;
        }
    }
    Some(out)
}

/// `int` in lax mode.
pub fn coerce_int(v: &Value, loc: &[String]) -> Result<Value, Box<FieldError>> {
    match v {
        Value::Bool(b) => Ok(Value::from(i64::from(*b))),
        Value::Number(n) if !n.is_f64() => Ok(v.clone()),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if !x.is_finite() {
                Err(err(
                    loc,
                    "Input should be a finite number",
                    "finite_number",
                    v,
                ))
            } else if x.fract() != 0.0 {
                Err(err(
                    loc,
                    "Input should be a valid integer, got a number with a fractional part",
                    "int_from_float",
                    v,
                ))
            } else {
                float_to_int(x)
                    .ok_or_else(|| err(loc, "Input should be a valid integer", "int_type", v))
            }
        }
        Value::String(s) => parse_int_str(s, v, loc),
        _ => Err(err(loc, "Input should be a valid integer", "int_type", v)),
    }
}

fn float_to_int(x: f64) -> Option<Value> {
    if x.abs() < 9.2e18 {
        Some(Value::from(x as i64))
    } else {
        Number::from_f64(x).map(Value::Number)
    }
}

fn parse_int_str(s: &str, input: &Value, loc: &[String]) -> Result<Value, Box<FieldError>> {
    let bad = || {
        err(
            loc,
            "Input should be a valid integer, unable to parse string as an integer",
            "int_parsing",
            input,
        )
    };
    let t = strip(s);
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if let Some(digits) = clean_int_digits(body) {
        let text = if neg { format!("-{digits}") } else { digits };
        return match text.parse::<i64>() {
            Ok(n) => Ok(Value::from(n)),
            Err(_) => text
                .parse::<f64>()
                .ok()
                .and_then(Number::from_f64)
                .map(Value::Number)
                .ok_or_else(bad),
        };
    }
    // A decimal that is a whole number ("1.0") is accepted; one with a fraction is not, and
    // neither is "1." or ".5".
    if let Some((whole, frac)) = body.split_once('.') {
        let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
        if digits(whole) && digits(frac) && frac.bytes().all(|b| b == b'0') {
            if let Ok(x) = t.parse::<f64>() {
                return float_to_int(x).ok_or_else(bad);
            }
        }
    }
    Err(bad())
}

/// `bool` in lax mode.
pub fn coerce_bool(v: &Value, loc: &[String]) -> Result<Value, Box<FieldError>> {
    let bad = || {
        err(
            loc,
            "Input should be a valid boolean, unable to interpret input",
            "bool_parsing",
            v,
        )
    };
    match v {
        Value::Bool(_) => Ok(v.clone()),
        // A whole number other than 0 and 1 (int or float) is "unable to interpret"; a float with
        // a fraction is simply not a boolean.
        Value::Number(n) if !n.is_f64() => match n.as_i64() {
            Some(0) => Ok(Value::Bool(false)),
            Some(1) => Ok(Value::Bool(true)),
            _ => Err(bad()),
        },
        Value::Number(n) => match n.as_f64() {
            Some(0.0) => Ok(Value::Bool(false)),
            Some(1.0) => Ok(Value::Bool(true)),
            Some(x) if x.fract() == 0.0 => Err(bad()),
            _ => Err(err(loc, "Input should be a valid boolean", "bool_type", v)),
        },
        Value::String(s) => match s.to_lowercase().as_str() {
            "0" | "off" | "f" | "false" | "n" | "no" => Ok(Value::Bool(false)),
            "1" | "on" | "t" | "true" | "y" | "yes" => Ok(Value::Bool(true)),
            _ => Err(bad()),
        },
        _ => Err(err(loc, "Input should be a valid boolean", "bool_type", v)),
    }
}

/// `Optional[str]` in lax mode: `None` or a `str`, nothing else.
pub fn coerce_optional_str(v: &Value, loc: &[String]) -> Result<Value, Box<FieldError>> {
    match v {
        Value::Null | Value::String(_) => Ok(v.clone()),
        _ => Err(err(loc, "Input should be a valid string", "string_type", v)),
    }
}

/// `List[int]` in lax mode: every item validated as an `int`, errors collected in order.
pub fn coerce_list_of_int(
    v: &Value,
    loc: &[String],
    errors: &mut Vec<FieldError>,
) -> Option<Value> {
    let Value::Array(items) = v else {
        errors.push(*err(loc, "Input should be a valid list", "list_type", v));
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    let mut ok = true;
    for (i, item) in items.iter().enumerate() {
        let mut item_loc = loc.to_vec();
        item_loc.push(i.to_string());
        match coerce_int(item, &item_loc) {
            Ok(value) => out.push(value),
            Err(e) => {
                ok = false;
                errors.push(*e);
            }
        }
    }
    ok.then(|| Value::Array(out))
}

/// `FuncMetadata.pre_parse_json`: a string argument that parses as JSON into a list, a dict,
/// `null` or a boolean-less container replaces the string. A string, number or bool result is
/// skipped (Python's `isinstance(x, str | int | float)`, where `bool` is an `int`).
pub fn pre_parse_json(fields: &[&str], data: &Map<String, Value>) -> Map<String, Value> {
    let mut out = data.clone();
    for (key, value) in data {
        if !fields.contains(&key.as_str()) {
            continue;
        }
        let Value::String(text) = value else { continue };
        let Ok(parsed) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if matches!(parsed, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
            continue;
        }
        out.insert(key.clone(), parsed);
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn loc() -> Vec<String> {
        vec!["row".to_owned()]
    }

    #[test]
    fn ints_coerce_the_way_pydantic_lax_mode_does() {
        for (input, want) in [
            (json!(1), 1),
            (json!(true), 1),
            (json!(false), 0),
            (json!(2.0), 2),
            (json!("7"), 7),
            (json!(" 7 "), 7),
            (json!("+5"), 5),
            (json!("-5"), -5),
            (json!("1_0"), 10),
            (json!("1.0"), 1),
            (json!(-0.0), 0),
        ] {
            assert_eq!(coerce_int(&input, &loc()).unwrap(), json!(want), "{input}");
        }
        for (input, kind) in [
            (json!(1.5), "int_from_float"),
            (json!("1.5"), "int_parsing"),
            (json!("1e2"), "int_parsing"),
            (json!("x"), "int_parsing"),
            (json!("1__0"), "int_parsing"),
            (json!("_1"), "int_parsing"),
            (json!(""), "int_parsing"),
            (json!(null), "int_type"),
            (json!([1]), "int_type"),
            (json!({"a": 1}), "int_type"),
        ] {
            assert_eq!(
                coerce_int(&input, &loc()).unwrap_err().kind,
                kind,
                "{input}"
            );
        }
    }

    #[test]
    fn bools_coerce_the_way_pydantic_lax_mode_does() {
        for t in [
            json!(true),
            json!(1),
            json!(1.0),
            json!("yes"),
            json!("TRUE"),
            json!("On"),
            json!("t"),
        ] {
            assert_eq!(coerce_bool(&t, &loc()).unwrap(), json!(true), "{t}");
        }
        for f in [
            json!(false),
            json!(0),
            json!("no"),
            json!("False"),
            json!("0"),
        ] {
            assert_eq!(coerce_bool(&f, &loc()).unwrap(), json!(false), "{f}");
        }
        assert_eq!(
            coerce_bool(&json!(2), &loc()).unwrap_err().kind,
            "bool_parsing"
        );
        assert_eq!(
            coerce_bool(&json!("maybe"), &loc()).unwrap_err().kind,
            "bool_parsing"
        );
        assert_eq!(
            coerce_bool(&json!(null), &loc()).unwrap_err().kind,
            "bool_type"
        );
    }

    #[test]
    fn error_text_is_pydantics() {
        let e = coerce_int(&json!("x"), &loc()).unwrap_err();
        assert_eq!(
            format_errors("get_entryArguments", &[*e]),
            "1 validation error for get_entryArguments\nrow\n  Input should be a valid integer, unable to parse string \
             as an integer [type=int_parsing, input_value='x', input_type=str]\n    For further information visit \
             https://errors.pydantic.dev/2.13/v/int_parsing"
        );
        let long = coerce_int(&json!("a".repeat(60)), &loc()).unwrap_err();
        let text = format_errors("m", &[*long]);
        assert!(
            text.contains(&format!(
                "input_value='{}...{}'",
                "a".repeat(24),
                "a".repeat(23)
            )),
            "{text}"
        );
    }

    #[test]
    fn pre_parse_replaces_only_containers_and_null() {
        let data = json!({"rows": "[1, 2]", "offset": "5", "filter_text": "null", "x": "[9]", "row": "\"s\""});
        let out = pre_parse_json(
            &["rows", "offset", "filter_text", "row"],
            data.as_object().unwrap(),
        );
        assert_eq!(out["rows"], json!([1, 2]));
        assert_eq!(out["offset"], json!("5"));
        assert_eq!(out["filter_text"], json!(null));
        assert_eq!(out["x"], json!("[9]"), "an unknown key is left alone");
        assert_eq!(out["row"], json!("\"s\""));
    }
}
