//! Entry JSON in and out (spec 3.4, 3.4.1, 3.4.2).
//!
//! **Out**: the derived `Serialize` of the model through a formatter that prints what
//! `json.dumps(...)` prints: `", "` and `": "` separators, everything outside printable
//! ASCII as `\uXXXX`, floats as Python's `repr`.
//!
//! **In**: `_entry_from_dict` and `_candidate_from_dict` (`core/session.py:97-148`,
//! `core/search_cache.py:152-223`) re-done on a `serde_json::Value`, because rule 6.2
//! needs their exact leniency and their exact failures:
//!
//! - what makes the Python app *raise* (so the row is skipped and counted): a non-object, a
//!   candidate without `url`, a tag without `name`, a list field that is not a list, a
//!   `tag_version` that `int()` rejects, a `selected_candidate_index` that is not an integer;
//! - what it *shrugs off*: an unknown `status` (`not_searched`), an unknown tag `source`
//!   (`User`), an index out of range (`0`), extra keys, an expired availability verdict;
//! - what it *carries* although the value is the wrong type. The model cannot, so Rust uses
//!   the field's default and records a [`Notice::FieldCoerced`] (never the value).
//!
//! The restore order of 3.4 is kept: candidates, then `select_candidate`, then the saved
//! tags overwrite the derived ones when there are any (rule 6.2.5).

use std::io;

use serde::Serialize;
use serde_json::ser::{CharEscape, Formatter, Serializer};
use serde_json::{Map, Value};

use super::report::Corrupt;
use crate::config::pyjson;
use crate::model::{
    Candidate, CandidateTag, Entry, MatchStatus, Tag, TagSource, tag_version_for_url,
};

/// How long a saved availability verdict stands (`AVAILABILITY_VERDICT_TTL_DAYS`).
const AVAILABILITY_TTL_SECONDS: f64 = 30.0 * 86400.0;

// ---- writing ----

struct PyFormatter;

impl Formatter for PyFormatter {
    fn begin_array_value<W>(&mut self, w: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first { Ok(()) } else { w.write_all(b", ") }
    }

    fn begin_object_key<W>(&mut self, w: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first { Ok(()) } else { w.write_all(b", ") }
    }

    fn begin_object_value<W>(&mut self, w: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        w.write_all(b": ")
    }

    fn write_f64<W>(&mut self, w: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut s = String::new();
        pyjson::float(&mut s, value);
        w.write_all(s.as_bytes())
    }

    /// `ensure_ascii=True`: DEL and everything non-ASCII become `\uXXXX`, lower-case hex,
    /// astral characters as a surrogate pair.
    fn write_string_fragment<W>(&mut self, w: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut plain = String::with_capacity(fragment.len());
        for c in fragment.chars() {
            if c.is_ascii() && c != '\u{7f}' {
                plain.push(c);
            } else {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    plain.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
        w.write_all(plain.as_bytes())
    }

    fn write_char_escape<W>(&mut self, w: &mut W, ce: CharEscape) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        // serde_json's own escapes already match Python's for ASCII: \" \\ \b \f \n \r \t
        // and \u00xx in lower-case hex for the other control characters.
        let s: &[u8] = match ce {
            CharEscape::Quote => b"\\\"",
            CharEscape::ReverseSolidus => b"\\\\",
            CharEscape::Solidus => b"/",
            CharEscape::Backspace => b"\\b",
            CharEscape::FormFeed => b"\\f",
            CharEscape::LineFeed => b"\\n",
            CharEscape::CarriageReturn => b"\\r",
            CharEscape::Tab => b"\\t",
            CharEscape::AsciiControl(byte) => {
                return write!(w, "\\u{byte:04x}");
            }
        };
        w.write_all(s)
    }
}

/// `json.dumps(value)` for anything the model serialises.
pub(crate) fn to_python_json<T: Serialize + ?Sized>(value: &T) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut ser = Serializer::with_formatter(&mut buf, PyFormatter);
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

// ---- reading ----

/// Names of the fields that held the wrong type and fell back to their default.
pub(crate) type Coerced = Vec<&'static str>;

/// One decoded element.
pub(crate) enum Decoded {
    Entry(Box<Entry>),
    /// No usable path: dropped silently, not counted as corrupt (`_entry_from_dict` returns
    /// `None`).
    Ignored,
}

/// Python truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn opt_string(d: &Map<String, Value>, key: &'static str, coerced: &mut Coerced) -> Option<String> {
    match d.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            coerced.push(key);
            None
        }
    }
}

fn opt_i64(d: &Map<String, Value>, key: &'static str, coerced: &mut Coerced) -> Option<i64> {
    match d.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                return Some(i);
            }
            // 800.0 is the same size as 800.
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 9.0e15 {
                    return Some(f as i64);
                }
            }
            coerced.push(key);
            None
        }
        Some(_) => {
            coerced.push(key);
            None
        }
    }
}

/// What `for x in d.get(key, [])` can iterate without raising: an array's elements, or
/// nothing at all for an empty object or empty string. Anything else raises in Python (a
/// non-empty object or string yields strings that are not tag dicts).
fn iterable(v: &Value, why: Corrupt) -> Result<&[Value], Corrupt> {
    match v {
        Value::Array(a) => Ok(a),
        Value::Object(o) if o.is_empty() => Ok(&[]),
        Value::String(s) if s.is_empty() => Ok(&[]),
        _ => Err(why),
    }
}

/// `{"name", "namespace"}` of one tag object. `Ok(None)` is a tag whose `name` is present but
/// not a string, which is dropped (Python would carry it and fail later in the UI).
fn tag_parts(
    v: &Value,
    coerced: &mut Coerced,
) -> Result<Option<(String, Option<String>)>, Corrupt> {
    let Value::Object(d) = v else {
        return Err(Corrupt::TagNotAnObject);
    };
    let Some(name) = d.get("name") else {
        return Err(Corrupt::TagWithoutName);
    };
    let Value::String(name) = name else {
        coerced.push("tags.name");
        return Ok(None);
    };
    let namespace = opt_string(d, "namespace", coerced);
    Ok(Some((name.clone(), namespace)))
}

fn candidate_tags(
    d: &Map<String, Value>,
    key: &'static str,
    coerced: &mut Coerced,
) -> Result<Vec<CandidateTag>, Corrupt> {
    // `d.get("engine_tags", [])`: a missing key is an empty list, a stored null raises.
    let Some(v) = d.get(key) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in iterable(v, Corrupt::TagListNotAList)? {
        if let Some((name, namespace)) = tag_parts(item, coerced)? {
            out.push(CandidateTag { name, namespace });
        }
    }
    Ok(out)
}

/// Whether `int(d.get("tag_version", 0) or 0)` succeeds (it is the first thing
/// `_candidate_from_dict` does, so a failure drops the row). The value itself is not used:
/// a session load always restores `booru_tags_fetched = False`.
fn tag_version_parses(v: Option<&Value>) -> bool {
    let Some(v) = v else { return true };
    if !truthy(v) {
        return true;
    }
    match v {
        Value::Bool(_) | Value::Number(_) => true,
        Value::String(s) => python_int_text(s),
        _ => false,
    }
}

/// `int(str)` in base 10: optional sign, digits, single underscores between digits, with
/// surrounding whitespace ignored. ASCII digits only (Python also takes other Unicode
/// decimal digits).
fn python_int_text(s: &str) -> bool {
    let t = s.trim();
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return false;
    }
    t.chars().all(|c| c.is_ascii_digit() || c == '_')
}

/// `_restore_availability`: a verdict counts only if it is a real boolean and dated with a
/// number no older than 30 days. `now` is epoch seconds.
fn availability(d: &Map<String, Value>, now: f64) -> (Option<bool>, Option<f64>) {
    let Some(Value::Bool(verdict)) = d.get("remote_available") else {
        return (None, None);
    };
    let Some(Value::Number(stamp)) = d.get("availability_checked_at") else {
        return (None, None);
    };
    let Some(checked_at) = stamp.as_f64() else {
        return (None, None);
    };
    if now - checked_at > AVAILABILITY_TTL_SECONDS {
        return (None, None);
    }
    (Some(*verdict), Some(checked_at))
}

fn candidate_from_value(v: &Value, now: f64, coerced: &mut Coerced) -> Result<Candidate, Corrupt> {
    let Value::Object(d) = v else {
        return Err(Corrupt::CandidateNotAnObject);
    };
    if !tag_version_parses(d.get("tag_version")) {
        return Err(Corrupt::BadTagVersion);
    }
    let url = match d.get("url") {
        None => return Err(Corrupt::CandidateWithoutUrl),
        Some(Value::String(s)) => s.clone(),
        Some(_) => {
            // Present but null or another type: Python loads it; the model needs a string.
            coerced.push("url");
            String::new()
        }
    };
    let similarity = match d.get("similarity") {
        None => 0.0,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::Null) => {
            // A `NaN` written by Python reaches here as null (see `tolerant`).
            0.0
        }
        Some(_) => {
            coerced.push("similarity");
            0.0
        }
    };
    let engine = match d.get("engine") {
        None => "IQDB".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => {
            coerced.push("engine");
            "IQDB".to_owned()
        }
    };
    let (remote_available, availability_checked_at) = availability(d, now);
    Ok(Candidate {
        tag_version: tag_version_for_url(&url),
        source_name: opt_string(d, "source_name", coerced),
        thumb_url: opt_string(d, "thumb_url", coerced),
        similarity,
        similarity_measured: d.get("similarity_measured").is_some_and(truthy),
        width: opt_i64(d, "width", coerced),
        height: opt_i64(d, "height", coerced),
        engine,
        engine_tags: candidate_tags(d, "engine_tags", coerced)?,
        booru_tags: candidate_tags(d, "booru_tags", coerced)?,
        direct_file_url: opt_string(d, "direct_file_url", coerced),
        preview_url: opt_string(d, "preview_url", coerced),
        remote_format: opt_string(d, "remote_format", coerced),
        remote_size_bytes: opt_i64(d, "remote_size_bytes", coerced),
        incomplete_reason: opt_string(d, "incomplete_reason", coerced),
        tags_borrowed_from: opt_string(d, "tags_borrowed_from", coerced),
        remote_available,
        availability_checked_at,
        restricted: opt_string(d, "restricted", coerced),
        url,
        thumb_bytes: None,
        page_index: 0,
    })
}

/// `d.get("selected_candidate_index", 0)` followed by `0 <= index < len(candidates)` and
/// `candidates[index]`, for a non-empty list: an in-range integer is used, an out-of-range
/// number (or NaN) means `0`, an in-range *float* raises (list indices must be integers),
/// and a non-number raises on the comparison.
fn selected_index(v: Option<&Value>, len: usize) -> Result<usize, Corrupt> {
    let in_range = |i: i128| (0..len as i128).contains(&i).then_some(i as usize);
    match v {
        None => Ok(0),
        Some(Value::Bool(b)) => Ok(in_range(i128::from(*b)).unwrap_or(0)),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                return Ok(in_range(i128::from(i)).unwrap_or(0));
            }
            if let Some(u) = n.as_u64() {
                return Ok(in_range(i128::from(u)).unwrap_or(0));
            }
            match n.as_f64() {
                Some(f) if f >= 0.0 && f < len as f64 => Err(Corrupt::BadSelectedIndex),
                _ => Ok(0),
            }
        }
        Some(_) => Err(Corrupt::BadSelectedIndex),
    }
}

/// `_entry_from_dict`.
pub(crate) fn entry_from_value(
    v: &Value,
    now: f64,
    coerced: &mut Coerced,
) -> Result<Decoded, Corrupt> {
    let Value::Object(d) = v else {
        return Err(Corrupt::NotAnObject);
    };
    let path = match d.get("path") {
        None => return Ok(Decoded::Ignored),
        Some(p) if !truthy(p) => return Ok(Decoded::Ignored),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(Corrupt::PathNotAString),
    };
    let mut entry = Entry::new(path);
    entry.hydrus_hash = opt_string(d, "hydrus_hash", coerced);
    entry.error_message = opt_string(d, "error_message", coerced);
    entry.result_source = opt_string(d, "result_source", coerced);
    entry.searched_without_saucenao = d.get("searched_without_saucenao").is_some_and(truthy);
    entry.sent_to_hydrus = d.get("sent_to_hydrus").is_some_and(truthy);
    entry.hydrus_import_confirmed = d.get("hydrus_import_confirmed").is_some_and(truthy);
    entry.reviewed = d.get("reviewed").is_some_and(truthy);
    entry.upscale_verdict = opt_string(d, "upscale_verdict", coerced);
    entry.upscale_check_detail = opt_string(d, "upscale_check_detail", coerced);
    entry.local_width = opt_i64(d, "local_width", coerced);
    entry.local_height = opt_i64(d, "local_height", coerced);

    // `MatchStatus(d.get("status") or "not_searched")`, ValueError -> not_searched.
    entry.status = match d.get("status") {
        Some(Value::String(s)) => MatchStatus::parse(s).unwrap_or_default(),
        _ => MatchStatus::NotSearched,
    };

    // Restore order (3.4, rule 6.2.5): candidates, select, then the saved tags.
    if let Some(list) = d.get("candidates").filter(|c| truthy(c)) {
        for c in iterable(list, Corrupt::CandidatesNotAList)? {
            entry
                .candidates
                .push(candidate_from_value(c, now, coerced)?);
        }
    }
    if !entry.candidates.is_empty() {
        let index = selected_index(d.get("selected_candidate_index"), entry.candidates.len())?;
        entry.select_candidate(index);
    }

    let mut restored = Vec::new();
    if let Some(list) = d.get("tags").filter(|t| truthy(t)) {
        for raw in iterable(list, Corrupt::TagListNotAList)? {
            let Value::Object(t) = raw else {
                return Err(Corrupt::TagNotAnObject);
            };
            // `TagSource(raw.get("source", "User"))`, ValueError -> User.
            let source = match t.get("source") {
                Some(Value::String(s)) => TagSource::parse(s).unwrap_or_default(),
                _ => TagSource::User,
            };
            if let Some((name, namespace)) = tag_parts(raw, coerced)? {
                restored.push(Tag {
                    name,
                    namespace,
                    source,
                });
            }
        }
    }
    if !restored.is_empty() {
        entry.tags = restored;
    }
    Ok(Decoded::Entry(Box::new(entry)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: f64 = 1_800_000_000.0;

    fn load(v: Value) -> Result<Option<Entry>, Corrupt> {
        let mut coerced = Vec::new();
        entry_from_value(&v, NOW, &mut coerced).map(|d| match d {
            Decoded::Entry(e) => Some(*e),
            Decoded::Ignored => None,
        })
    }

    #[test]
    fn encoder_matches_json_dumps() {
        let mut e = Entry::new("/ü/\u{7f}/😀.png");
        e.error_message = Some("a\"b\\c\n\u{1}".to_owned());
        e.status = MatchStatus::Good;
        let mut c = Candidate::new("https://x.invalid/1");
        c.similarity = 81.0;
        c.width = Some(1600);
        e.candidates.push(c);
        let text = to_python_json(&e).unwrap();
        assert!(text.starts_with(
            r#"{"path": "/\u00fc/\u007f/\ud83d\ude00.png", "hydrus_hash": null, "status": "good", "#
        ));
        assert!(text.contains(r#""error_message": "a\"b\\c\n\u0001""#));
        assert!(
            text.contains(r#""similarity": 81.0, "similarity_measured": false, "width": 1600"#)
        );
        assert!(text.ends_with(r#""tag_version": 0}], "tags": []}"#));
    }

    #[test]
    fn non_finite_similarity_is_written_as_null_not_nan() {
        let mut c = Candidate::new("u");
        c.similarity = f64::NAN;
        assert!(
            to_python_json(&c)
                .unwrap()
                .contains(r#""similarity": null"#)
        );
    }

    #[test]
    fn missing_path_is_silently_ignored_and_a_non_object_is_corrupt() {
        for v in [
            json!({}),
            json!({"path": ""}),
            json!({"path": null}),
            json!({"path": 0}),
        ] {
            assert_eq!(load(v).unwrap(), None);
        }
        assert_eq!(
            load(json!({"path": 7})).unwrap_err(),
            Corrupt::PathNotAString
        );
        for v in [json!([]), json!("x"), json!(null), json!(3)] {
            assert_eq!(load(v).unwrap_err(), Corrupt::NotAnObject);
        }
    }

    #[test]
    fn unknown_status_and_source_fall_back() {
        let e = load(json!({
            "path": "/a", "status": "banana",
            "tags": [{"name": "x", "source": "Nowhere"}, {"name": "y"}, {"name": "z", "source": 5}]
        }))
        .unwrap()
        .unwrap();
        assert_eq!(e.status, MatchStatus::NotSearched);
        assert!(e.tags.iter().all(|t| t.source == TagSource::User));
        assert_eq!(e.tags.len(), 3);
        for v in [json!(5), json!(null), json!(["good"]), json!("")] {
            let e = load(json!({"path": "/a", "status": v})).unwrap().unwrap();
            assert_eq!(e.status, MatchStatus::NotSearched);
        }
    }

    #[test]
    fn candidate_without_url_makes_the_row_corrupt() {
        let r = load(json!({"path": "/a", "status": "banana", "candidates": [{"engine": "IQDB"}]}));
        assert_eq!(r.unwrap_err(), Corrupt::CandidateWithoutUrl);
        // A url that is present but null is carried by Python, so the row survives.
        let e = load(json!({"path": "/a", "candidates": [{"url": null}]}))
            .unwrap()
            .unwrap();
        assert_eq!(e.candidates[0].url, "");
    }

    #[test]
    fn list_fields_that_python_cannot_iterate_are_corrupt() {
        let bad = |extra: Value| {
            let mut v = json!({"path": "/a"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            load(v)
        };
        assert!(bad(json!({"candidates": 5})).is_err());
        assert!(bad(json!({"candidates": "abc"})).is_err());
        assert!(bad(json!({"candidates": {"k": 1}})).is_err());
        assert!(bad(json!({"candidates": ["x"]})).is_err());
        assert!(bad(json!({"tags": [1]})).is_err());
        assert!(bad(json!({"tags": [{"namespace": null}]})).is_err());
        assert!(bad(json!({"candidates": [{"url": "u", "engine_tags": null}]})).is_err());
        assert!(bad(json!({"candidates": [{"url": "u", "booru_tags": [{"x": 1}]}]})).is_err());
        // Falsy values stand for "none".
        for ok in [
            json!(null),
            json!(false),
            json!(0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(bad(json!({"candidates": ok.clone(), "tags": ok})).is_ok());
        }
        // Inside a candidate only a missing key is "none"; an empty object or string iterates.
        assert!(bad(json!({"candidates": [{"url": "u", "engine_tags": {}}]})).is_ok());
        assert!(bad(json!({"candidates": [{"url": "u", "engine_tags": ""}]})).is_ok());
    }

    #[test]
    fn tag_version_must_survive_int() {
        let with = |tv: Value| {
            load(json!({"path": "/a", "candidates": [{"url": "u", "tag_version": tv}]}))
        };
        for ok in [
            json!(1),
            json!(1.7),
            json!("7"),
            json!(" -3 "),
            json!("1_0"),
            json!(""),
            json!(null),
            json!(true),
            json!([]),
        ] {
            assert!(with(ok).is_ok());
        }
        for bad in [
            json!("abc"),
            json!("1.5"),
            json!("_1"),
            json!("1__0"),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert_eq!(with(bad).unwrap_err(), Corrupt::BadTagVersion);
        }
    }

    #[test]
    fn selected_index_rules() {
        let with = |i: Option<Value>| {
            let mut v = json!({"path": "/a", "candidates": [{"url": "a"}, {"url": "b"}]});
            if let Some(i) = i {
                v["selected_candidate_index"] = i;
            }
            load(v).map(|e| e.unwrap().selected_candidate_index)
        };
        assert_eq!(with(None).unwrap(), 0);
        assert_eq!(with(Some(json!(1))).unwrap(), 1);
        assert_eq!(with(Some(json!(2))).unwrap(), 0);
        assert_eq!(with(Some(json!(-1))).unwrap(), 0);
        assert_eq!(
            with(Some(serde_json::from_str("99999999999999999999").unwrap())).unwrap(),
            0
        );
        assert_eq!(with(Some(json!(true))).unwrap(), 1);
        assert_eq!(with(Some(json!(5.5))).unwrap(), 0);
        assert_eq!(
            with(Some(json!(1.0))).unwrap_err(),
            Corrupt::BadSelectedIndex
        );
        assert_eq!(
            with(Some(json!(null))).unwrap_err(),
            Corrupt::BadSelectedIndex
        );
        assert_eq!(
            with(Some(json!("1"))).unwrap_err(),
            Corrupt::BadSelectedIndex
        );
        // With no candidates the index is never looked at.
        let e = load(json!({"path": "/a", "selected_candidate_index": "x"}))
            .unwrap()
            .unwrap();
        assert_eq!(e.selected_candidate_index, 0);
    }

    #[test]
    fn availability_needs_a_bool_and_a_fresh_number() {
        let with = |avail: Value, at: Value| {
            let e = load(json!({"path": "/a", "candidates": [
                {"url": "u", "remote_available": avail, "availability_checked_at": at}]}))
            .unwrap()
            .unwrap();
            (
                e.candidates[0].remote_available,
                e.candidates[0].availability_checked_at,
            )
        };
        let fresh = NOW - 86400.0;
        assert_eq!(with(json!(true), json!(fresh)), (Some(true), Some(fresh)));
        assert_eq!(with(json!(false), json!(fresh)), (Some(false), Some(fresh)));
        assert_eq!(with(json!(null), json!(fresh)), (None, None));
        assert_eq!(with(json!(1), json!(fresh)), (None, None));
        assert_eq!(with(json!(true), json!(null)), (None, None));
        assert_eq!(with(json!(true), json!(true)), (None, None));
        assert_eq!(with(json!(true), json!("1")), (None, None));
        assert_eq!(with(json!(true), json!(NOW - 31.0 * 86400.0)), (None, None));
        assert_eq!(with(json!(true), json!(NOW - 29.0 * 86400.0)).0, Some(true));
    }

    #[test]
    fn saved_tags_overwrite_the_derived_ones_only_when_present() {
        let cand = json!({"url": "u", "engine_tags": [{"name": "e", "namespace": null}],
                          "booru_tags": [{"name": "b", "namespace": "artist"}]});
        let derived = load(json!({"path": "/a", "candidates": [cand.clone()]}))
            .unwrap()
            .unwrap();
        let names: Vec<_> = derived
            .tags
            .iter()
            .map(|t| (t.display(), t.source))
            .collect();
        assert_eq!(
            names,
            [
                ("e".to_owned(), TagSource::SearchEngine),
                ("artist:b".to_owned(), TagSource::Booru)
            ]
        );
        let saved = load(json!({"path": "/a", "candidates": [cand],
            "tags": [{"name": "only", "namespace": null, "source": "User"}]}))
        .unwrap()
        .unwrap();
        assert_eq!(saved.tags.len(), 1);
        assert_eq!(saved.tags[0].name, "only");
    }

    #[test]
    fn wrong_typed_values_use_defaults_and_are_reported() {
        let mut coerced = Vec::new();
        let d = entry_from_value(
            &json!({"path": "/a", "hydrus_hash": 5, "local_width": "wide", "error_message": [],
                    "candidates": [{"url": "u", "similarity": "high", "width": 1.5}]}),
            NOW,
            &mut coerced,
        )
        .unwrap();
        let Decoded::Entry(e) = d else { panic!() };
        assert_eq!(e.hydrus_hash, None);
        assert_eq!(e.local_width, None);
        assert_eq!(e.candidates[0].similarity, 0.0);
        assert_eq!(
            coerced,
            [
                "hydrus_hash",
                "error_message",
                "local_width",
                "similarity",
                "width"
            ]
        );
    }
}
