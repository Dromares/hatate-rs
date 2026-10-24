//! Writes the working list out as CSV or JSON (`core/export.py`).
//!
//! One schema, two renderers: [`HEADERS`] is the single definition of what an exported row holds,
//! and both writers walk it. CSV joins a row's tags into one cell with `", "`; JSON carries them
//! as an array of `{name, namespace, source}` objects and is the lossless form. The tags are the
//! ones [`tags_to_send`] returns, so an export never disagrees with a Hydrus send.
//!
//! The bytes are Python's: the CSV is `csv.writer(lineterminator="\r\n")` with the default
//! `QUOTE_MINIMAL` dialect, and the JSON is `json.dumps(payload, indent=2, ensure_ascii=False)`
//! with no trailing newline. The JSON record's keys are in the order the Python dict builds them,
//! which is not [`HEADERS`] order: the tag fields come last.

use crate::config::Settings;
use crate::config::pyjson;
use crate::localtime;
use crate::model::{Entry, tags_to_send};
use crate::timeutil;

/// `SCHEMA_VERSION`: the format's own version, written into the JSON.
pub const SCHEMA_VERSION: i64 = 1;

/// `CSV_TAG_SEPARATOR`.
pub const CSV_TAG_SEPARATOR: &str = ", ";

/// `HEADERS`: the column names, in file order.
pub const HEADERS: [&str; 22] = [
    "filename",
    "path",
    "status",
    "matched_url",
    "site",
    "engine",
    "similarity",
    "similarity_measured",
    "local_width",
    "local_height",
    "match_width",
    "match_height",
    "tag_count",
    "tags",
    "candidate_count",
    "sent",
    "reviewed",
    "upscale_verdict",
    "cache",
    "error",
    "file_missing",
    "hydrus_hash",
];

/// The value of one field, typed the way the Python extractor returns it (`""` stands in for an
/// absent value, as it does there).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// The CSV form of `tags`: display strings.
    Tags(Vec<String>),
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

/// `_sent_state`: the same three states the Sent column shows.
fn sent_state(entry: &Entry) -> &'static str {
    if !entry.sent_to_hydrus {
        ""
    } else if entry.hydrus_import_confirmed {
        "sent"
    } else {
        "queued"
    }
}

/// `_cache_state`: provisional wins over where the result came from.
fn cache_state(entry: &Entry) -> String {
    if entry.searched_without_saucenao {
        "provisional".to_owned()
    } else {
        entry.result_source.clone().unwrap_or_default()
    }
}

/// `x or ""` over an optional integer: `None` and `0` are both `""`.
fn int_or_blank(value: Option<i64>) -> Value {
    match value {
        Some(v) if v != 0 => Value::Int(v),
        _ => s(""),
    }
}

/// `_row_values`: every field of one row, by header name, with the tag fields resolved against
/// the settings. Both renderers go through here.
pub fn row_values(entry: &Entry, settings: Option<&Settings>) -> Vec<(&'static str, Value)> {
    let sent = tags_to_send(entry, settings);
    let similarity = entry.similarity();
    HEADERS
        .iter()
        .map(|&key| {
            let value = match key {
                "filename" => s(entry.filename()),
                "path" => s(&entry.path),
                "status" => s(entry.status.label()),
                "matched_url" => s(entry.matched_url().unwrap_or("")),
                "site" => s(entry.booru_name().unwrap_or("")),
                "engine" => s(entry.selected_candidate().map_or("", |c| c.engine.as_str())),
                "similarity" => similarity.map_or_else(|| s(""), Value::Float),
                "similarity_measured" => match similarity {
                    Some(_) => Value::Bool(entry.similarity_measured()),
                    None => s(""),
                },
                "local_width" => int_or_blank(entry.local_width),
                "local_height" => int_or_blank(entry.local_height),
                "match_width" => int_or_blank(entry.match_width()),
                "match_height" => int_or_blank(entry.match_height()),
                "tag_count" => Value::Int(sent.len() as i64),
                "tags" => Value::Tags(sent.iter().map(|t| t.display()).collect()),
                "candidate_count" => Value::Int(entry.candidates.len() as i64),
                "sent" => s(sent_state(entry)),
                "reviewed" => Value::Bool(entry.reviewed),
                "upscale_verdict" => s(entry.upscale_verdict.as_deref().unwrap_or("")),
                "cache" => Value::Str(cache_state(entry)),
                "error" => s(entry.error_message.as_deref().unwrap_or("")),
                "file_missing" => Value::Bool(entry.file_missing),
                "hydrus_hash" => s(entry.hydrus_hash.as_deref().unwrap_or("")),
                _ => unreachable!("HEADERS names every field"),
            };
            (key, value)
        })
        .collect()
}

/// `str(float)`: Python's `repr`, with `nan` and `inf` in lower case.
fn float_str(x: f64) -> String {
    if x.is_nan() {
        "nan".to_owned()
    } else if x.is_infinite() {
        if x < 0.0 { "-inf" } else { "inf" }.to_owned()
    } else {
        let mut out = String::new();
        pyjson::float(&mut out, x);
        out
    }
}

/// `_csv_value`: one field, rendered for a spreadsheet. Booleans are `TRUE`/`FALSE`.
fn csv_value(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Value::Tags(tags) => tags.join(CSV_TAG_SEPARATOR),
        Value::Str(text) => text.clone(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => float_str(*f),
    }
}

/// One field as `csv.writer` writes it under `QUOTE_MINIMAL`: quoted when it holds the delimiter,
/// the quote character or a line break, with the quote doubled.
fn csv_field(field: &str, out: &mut String) {
    if field.contains([',', '"', '\r', '\n']) {
        out.push('"');
        out.push_str(&field.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(field);
    }
}

fn csv_row<'a>(fields: impl IntoIterator<Item = &'a str>, out: &mut String) {
    for (i, field) in fields.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        csv_field(field, out);
    }
    out.push_str("\r\n");
}

/// `to_csv`: the list as CSV, header row included, `\r\n` line ends.
pub fn to_csv<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    settings: Option<&Settings>,
) -> String {
    let mut out = String::new();
    csv_row(HEADERS, &mut out);
    for entry in entries {
        let cells: Vec<String> = row_values(entry, settings)
            .iter()
            .map(|(_, v)| csv_value(v))
            .collect();
        csv_row(cells.iter().map(String::as_str), &mut out);
    }
    out
}

/// A JSON string as `json.dumps(..., ensure_ascii=False)` writes it: only the quote, the
/// backslash and the control characters are escaped.
fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_scalar(value: &Value, out: &mut String) {
    match value {
        Value::Str(text) => json_string(text, out),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => pyjson::float(out, *f),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Tags(_) => unreachable!("the JSON record writes tags itself"),
    }
}

/// The key order of the JSON record: `_row_values` builds the non-tag fields in [`HEADERS`]
/// order, then adds `tags` and `tag_count`.
fn json_keys() -> impl Iterator<Item = &'static str> {
    HEADERS
        .iter()
        .copied()
        .filter(|k| *k != "tags" && *k != "tag_count")
        .chain(["tags", "tag_count"])
}

/// One record at nesting depth 2 (an element of `images`), `indent=2`.
fn json_record(entry: &Entry, settings: Option<&Settings>, out: &mut String) {
    let values = row_values(entry, settings);
    let get = |key: &str| &values.iter().find(|(k, _)| *k == key).expect("every key").1;
    out.push('{');
    for (i, key) in json_keys().enumerate() {
        if i > 0 {
            out.push(',');
        }
        pyjson::newline(out, 3);
        json_string(key, out);
        out.push_str(": ");
        if key == "tags" {
            let tags = tags_to_send(entry, settings);
            if tags.is_empty() {
                out.push_str("[]");
                continue;
            }
            out.push('[');
            for (j, tag) in tags.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                pyjson::newline(out, 4);
                out.push('{');
                pyjson::newline(out, 5);
                out.push_str("\"name\": ");
                json_string(&tag.name, out);
                out.push(',');
                pyjson::newline(out, 5);
                out.push_str("\"namespace\": ");
                match &tag.namespace {
                    Some(ns) => json_string(ns, out),
                    None => out.push_str("null"),
                }
                out.push(',');
                pyjson::newline(out, 5);
                out.push_str("\"source\": ");
                json_string(tag.source.as_str(), out);
                pyjson::newline(out, 4);
                out.push('}');
            }
            pyjson::newline(out, 3);
            out.push(']');
        } else {
            json_scalar(get(key), out);
        }
    }
    pyjson::newline(out, 2);
    out.push('}');
}

/// `to_json`: the list as JSON, an object with the images under `"images"`. `generated_at` is
/// epoch seconds, `None` for the present; it is written as local time.
pub fn to_json<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    generated_at: Option<f64>,
    settings: Option<&Settings>,
) -> String {
    let entries: Vec<&Entry> = entries.into_iter().collect();
    let moment = generated_at.unwrap_or_else(timeutil::now_epoch);
    let t = localtime::local_wall_clock(moment);
    let mut out = String::from("{");
    pyjson::newline(&mut out, 1);
    out.push_str(&format!("\"schema_version\": {SCHEMA_VERSION},"));
    pyjson::newline(&mut out, 1);
    out.push_str(&format!(
        "\"generated_at\": \"{:04}-{:02}-{:02}T{:02}:{:02}:{:02}\",",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    ));
    pyjson::newline(&mut out, 1);
    out.push_str(&format!("\"count\": {},", entries.len()));
    pyjson::newline(&mut out, 1);
    out.push_str("\"images\": ");
    if entries.is_empty() {
        out.push_str("[]");
    } else {
        out.push('[');
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            pyjson::newline(&mut out, 2);
            json_record(entry, settings, &mut out);
        }
        pyjson::newline(&mut out, 1);
        out.push(']');
    }
    pyjson::newline(&mut out, 0);
    out.push('}');
    out
}

/// `format_for_path`: `"json"` or `"csv"`, chosen from the filename's extension; anything
/// unrecognised is CSV.
pub fn format_for_path(path: &str) -> &'static str {
    if path.to_lowercase().ends_with(".json") {
        "json"
    } else {
        "csv"
    }
}

/// `render`.
pub fn render<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    fmt: &str,
    settings: Option<&Settings>,
) -> String {
    if fmt == "json" {
        to_json(entries, None, settings)
    } else {
        to_csv(entries, settings)
    }
}
