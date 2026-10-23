//! The MCP server's audit log: one line per tool call, kept up to a size cap, redacted and
//! append-only (`core/mcp_audit.py`).
//!
//! JSON Lines rather than one document: a crash mid-write loses at most the line being
//! appended. A refusal is part of the answer to "what did it do, and why" as much as an action
//! is, so [`record`] is called for every call: allowed, refused or dry-run.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::hydrus_client::py_dumps;

/// Keys redacted wherever they appear in a call's arguments or outcome, however deeply nested,
/// matched case-insensitively. The bearer token and the Hydrus access key are the two secrets
/// this server ever touches.
pub const REDACTED_KEYS: [&str; 7] = [
    "token",
    "bearer",
    "access_key",
    "accesskey",
    "api_key",
    "apikey",
    "authorization",
];
pub const REDACTED_VALUE: &str = "<redacted>";

/// `_redact(value)`.
pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, val)| {
                    let hidden = REDACTED_KEYS.contains(&key.to_lowercase().as_str());
                    (
                        key.clone(),
                        if hidden {
                            Value::String(REDACTED_VALUE.to_owned())
                        } else {
                            redact(val)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        other => other.clone(),
    }
}

/// What one `record(...)` call logs. The defaults are the Python keyword defaults.
#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    pub tool: &'a str,
    pub tier: &'a str,
    pub allowed: bool,
    pub arguments: Option<&'a Map<String, Value>>,
    pub reason: Option<&'a str>,
    pub dry_run: bool,
    pub row: Option<i64>,
    pub candidate_index: Option<i64>,
    pub outcome: Option<&'a Value>,
}

impl<'a> AuditEntry<'a> {
    pub fn new(tool: &'a str, tier: &'a str, allowed: bool) -> Self {
        Self {
            tool,
            tier,
            allowed,
            arguments: None,
            reason: None,
            dry_run: false,
            row: None,
            candidate_index: None,
            outcome: None,
        }
    }
}

/// `time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())` for a Unix time.
pub fn iso_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// `_iso_now()`.
pub fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    iso_from_unix(secs)
}

/// The line `record` appends, without its newline: `json.dumps(entry, default=str)`.
pub fn entry_line(entry: &AuditEntry<'_>, timestamp: &str) -> String {
    let arguments = match entry.arguments {
        Some(map) if !map.is_empty() => redact(&Value::Object(map.clone())),
        _ => json!({}),
    };
    let outcome = entry.outcome.map_or(Value::Null, redact);
    let mut line = Map::new();
    line.insert("timestamp".into(), json!(timestamp));
    line.insert("tool".into(), json!(entry.tool));
    line.insert("tier".into(), json!(entry.tier));
    line.insert("allowed".into(), json!(entry.allowed));
    line.insert(
        "reason".into(),
        entry.reason.map_or(Value::Null, |r| json!(r)),
    );
    line.insert("dry_run".into(), json!(entry.dry_run));
    line.insert("row".into(), entry.row.map_or(Value::Null, |r| json!(r)));
    line.insert(
        "candidate_index".into(),
        entry.candidate_index.map_or(Value::Null, |r| json!(r)),
    );
    line.insert("arguments".into(), arguments);
    line.insert("outcome".into(), outcome);
    py_dumps(&Value::Object(line))
}

/// `record(...)`: appends one audit entry. Never fails: a logging failure must not take down the
/// tool call it is trying to describe, so it is logged instead.
pub fn record(entry: &AuditEntry<'_>, path: &Path, max_bytes: i64) {
    append(&entry_line(entry, &iso_now()), path, max_bytes);
}

/// `_append(entry, path, max_bytes)`, for a line already built.
pub fn append(line: &str, path: &Path, max_bytes: i64) {
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        rotate_if_needed(path, max_bytes);
        let mut handle = OpenOptions::new().create(true).append(true).open(path)?;
        handle.write_all(format!("{line}\n").as_bytes())
    })();
    if let Err(error) = result {
        tracing::warn!(
            "Could not write MCP audit log entry to {}: {error}",
            path.display()
        );
    }
}

/// `_rotate_if_needed(path, max_bytes)`: keeps exactly one rotated generation (`<name>.1`),
/// enough to stop an unattended overnight run from growing the log without bound.
pub fn rotate_if_needed(path: &Path, max_bytes: i64) {
    let result = (|| -> std::io::Result<()> {
        let Ok(meta) = fs::metadata(path) else {
            return Ok(());
        };
        let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
        if size >= max_bytes {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            name.push(".1");
            fs::rename(path, path.with_file_name(name))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        tracing::warn!("Could not rotate MCP audit log {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn iso_format_matches_strftime() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_from_unix(-1), "1969-12-31T23:59:59Z");
        assert_eq!(iso_now().len(), 20);
    }

    #[test]
    fn creates_the_file_and_its_parent_and_writes_one_object_per_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/er/audit.jsonl");
        let mut args = Map::new();
        args.insert("row".into(), json!(3));
        let mut entry = AuditEntry::new("get_entry", "read", true);
        entry.arguments = Some(&args);
        record(&entry, &path, 5_000_000);
        record(
            &AuditEntry::new("get_entry", "read", true),
            &path,
            5_000_000,
        );
        let rows = lines(&path);
        assert_eq!(rows.len(), 2);
        assert!(rows[0]["timestamp"].as_str().unwrap().ends_with('Z'));
        assert_eq!(rows[0]["arguments"], json!({"row": 3}));
        assert_eq!(
            rows[1]["arguments"],
            json!({}),
            "no arguments is an empty object"
        );
    }

    #[test]
    fn a_refusal_a_dry_run_and_a_candidate_index_are_logged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let mut refused = AuditEntry::new("research", "research", false);
        refused.reason = Some("research refused");
        record(&refused, &path, 5_000_000);
        let mut dry = AuditEntry::new("remove_row", "destructive", true);
        dry.dry_run = true;
        dry.row = Some(2);
        dry.candidate_index = Some(1);
        record(&dry, &path, 5_000_000);
        let rows = lines(&path);
        assert_eq!(rows[0]["allowed"], false);
        assert_eq!(rows[0]["reason"], "research refused");
        assert_eq!(rows[1]["dry_run"], true);
        assert_eq!(rows[1]["candidate_index"], 1);
        assert_eq!(rows[1]["row"], 2);
    }

    #[test]
    fn line_format_is_pythons_json_dumps_with_the_fixed_key_order() {
        let line = entry_line(
            &AuditEntry::new("list_actions", "read", true),
            "2026-01-02T03:04:05Z",
        );
        assert_eq!(
            line,
            "{\"timestamp\": \"2026-01-02T03:04:05Z\", \"tool\": \"list_actions\", \"tier\": \"read\", \
             \"allowed\": true, \"reason\": null, \"dry_run\": false, \"row\": null, \
             \"candidate_index\": null, \"arguments\": {}, \"outcome\": null}"
        );
    }

    #[test]
    fn redaction_covers_every_key_nested_and_case_insensitive() {
        for key in REDACTED_KEYS {
            let v = json!({ key: "s", "keep": 1, "n": [{ key.to_uppercase(): "t", "x": {"Token": "u"} }] });
            let out = redact(&v);
            assert_eq!(out[key], REDACTED_VALUE);
            assert_eq!(out["keep"], 1);
            assert_eq!(out["n"][0][key.to_uppercase()], REDACTED_VALUE);
            assert_eq!(out["n"][0]["x"]["Token"], REDACTED_VALUE);
        }
        // A redacted key's whole value goes, whatever it holds.
        assert_eq!(redact(&json!({"token": {"a": 1}}))["token"], REDACTED_VALUE);
    }

    #[test]
    fn secrets_never_reach_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let mut args = Map::new();
        args.insert("token".into(), json!("synthetic-bearer"));
        let outcome = json!({"hydrus": {"access_key": "synthetic-key"}});
        let mut entry = AuditEntry::new("send_url", "hydrus_write", true);
        entry.arguments = Some(&args);
        entry.outcome = Some(&outcome);
        record(&entry, &path, 5_000_000);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("synthetic"));
        assert!(text.contains(REDACTED_VALUE));
    }

    #[test]
    fn rotates_once_the_cap_is_reached_and_keeps_one_generation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jsonl");
        let one = entry_line(
            &AuditEntry::new("list_actions", "read", true),
            "2026-01-02T03:04:05Z",
        );
        let cap = i64::try_from(one.len() + 1).unwrap() * 2;
        for _ in 0..3 {
            record(&AuditEntry::new("list_actions", "read", true), &path, cap);
        }
        // The third call found the file at the cap and moved it aside.
        assert_eq!(lines(&dir.path().join("a.jsonl.1")).len(), 2);
        assert_eq!(lines(&path).len(), 1);
        for _ in 0..4 {
            record(&AuditEntry::new("list_actions", "read", true), &path, cap);
        }
        assert_eq!(
            lines(&dir.path().join("a.jsonl.1")).len(),
            2,
            "still one rotated generation"
        );
        // Below the cap everything stays in one file.
        let other = dir.path().join("b.jsonl");
        for _ in 0..3 {
            record(
                &AuditEntry::new("list_actions", "read", true),
                &other,
                5_000_000,
            );
        }
        assert_eq!(lines(&other).len(), 3);
        assert!(!dir.path().join("b.jsonl.1").exists());
    }

    #[test]
    fn a_write_failure_is_swallowed() {
        let dir = tempfile::tempdir().unwrap();
        // The parent "directory" is a file: create_dir_all fails, record must not panic.
        let blocker = dir.path().join("f");
        fs::write(&blocker, b"x").unwrap();
        record(
            &AuditEntry::new("list_actions", "read", true),
            &blocker.join("a.jsonl"),
            5_000_000,
        );
    }
}
