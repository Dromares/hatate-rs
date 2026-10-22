//! Search-engine result parsers (`core/yandex.py`, `core/google_images.py`): what an engine's
//! answer says once it is in hand.
//!
//! **Where Python's engine functions are cut.** Each `search` in Python uploads the image, waits,
//! reads the answer and interprets it. The interpreting is here: the parse of the page or payload
//! and the choice of what to keep. The *request* half is Phase 3: reading and downscaling the
//! file, the upload, the deadline, the network errors, and the module state Python keeps in
//! globals (`yandex._blocked`, its rest after a captcha, `google_images._blocked`).
//!
//! No function here opens a socket or reads a file.

pub mod google_images;
pub mod yandex;

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
