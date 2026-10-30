//! The Recent tool calls table of Settings > MCP, without a widget in it
//! (`gui/settings_dialog.py` at `76c85c4`: `_read_mcp_audit_entries`, `_mcp_time_cell`,
//! `_mcp_target_cell`, `_mcp_outcome_weight`, `_mcp_outcome_cell`, `_mcp_outcome_tooltip` and the
//! row loop of `_refresh_mcp_audit_table`), and the Connection card's six states
//! (`_refresh_mcp_connection`).
//!
//! An audit line is read as JSON and kept as JSON: the table shows what is on disk, whatever
//! shape a line has. Python's truthiness and `str()` of a JSON value are spelled out here
//! ([`truthy`], [`py_str`]) because the cells depend on them (`outcome.get("error") or ...`,
//! `f"... {outcome.get('selected_candidate_index')}"`).

use hatate_core::hydrus_client::py_dumps;
use hatate_core::localtime::WallClock;
use serde_json::{Map, Value};
use std::path::Path;

/// `_MCP_AUDIT_DISPLAY_LIMIT`: the most recent entries the table shows.
pub const DISPLAY_LIMIT: usize = 200;

/// The glyph a sent file's Outcome carries (`theme.STATUS_GLYPHS['sent']`).
pub const SENT_GLYPH: &str = "\u{25cf}";

/// `_MCP_TIER_LABELS`: the Tier column, and the tier in a refused call's Outcome.
pub fn tier_label(tier: &str) -> &str {
    match tier {
        "read" => "Read",
        "local_write" => "Reversible",
        "research" => "Re-search",
        "hydrus_write" => "Hydrus writes",
        "destructive" => "Destructive",
        other => other,
    }
}

/// The ink tier an Outcome cell is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Ink65,
    Ink100,
}

impl Weight {
    pub fn name(self) -> &'static str {
        match self {
            Weight::Ink65 => "ink_65",
            Weight::Ink100 => "ink_100",
        }
    }
}

pub use hatate_core::pycompat::truthy;

/// `str(value)` of what `json.loads` gave.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        // a container's repr is not a cell anyone writes; the JSON text is the nearest honest one
        other => py_dumps(other),
    }
}

fn get<'a>(entry: &'a Value, key: &str) -> Option<&'a Value> {
    entry.as_object().and_then(|o| o.get(key))
}

/// `entry.get(key)` is truthy.
fn flag(entry: &Value, key: &str) -> bool {
    get(entry, key).is_some_and(truthy)
}

/// `entry.get("outcome") or {}`.
fn outcome_of(entry: &Value) -> Map<String, Value> {
    match get(entry, "outcome") {
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    }
}

fn okey(outcome: &Map<String, Value>, key: &str) -> bool {
    outcome.get(key).is_some_and(truthy)
}

/// `outcome.get("error") or outcome.get("skipped_reason")`, where only a truthy answer matters.
fn error_or_skipped(outcome: &Map<String, Value>) -> Option<&Value> {
    ["error", "skipped_reason"]
        .iter()
        .filter_map(|k| outcome.get(*k))
        .find(|v| truthy(v))
}

fn tool_name(entry: &Value) -> String {
    match get(entry, "tool") {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn is_hydrus_send(tool: &str) -> bool {
    matches!(tool, "send_upload" | "send_url" | "download_send")
}

/// `_read_mcp_audit_entries`: every parseable line, oldest first. Never fails: a missing file is
/// no calls yet, and a line that does not parse (the log's own last line, mid-write) is dropped.
pub fn read_entries(path: &Path) -> Vec<Value> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut entries = Vec::new();
    for line in hatate_core::mcp::bridge::splitlines(&text) {
        let line = hatate_core::pycompat::strip(line);
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            entries.push(v);
        }
    }
    entries
}

/// `strptime(timestamp, "%Y-%m-%dT%H:%M:%SZ")`, which takes one or two digits for every field
/// but the year, and an instant `datetime` can hold.
fn parse_timestamp(text: &str) -> Option<WallClock> {
    fn digits(s: &str, at: &mut usize, min: usize, max: usize) -> Option<i64> {
        let b = s.as_bytes();
        let start = *at;
        while *at < b.len() && *at - start < max && b[*at].is_ascii_digit() {
            *at += 1;
        }
        if *at - start < min {
            return None;
        }
        s[start..*at].parse().ok()
    }
    fn lit(s: &str, at: &mut usize, c: u8) -> Option<()> {
        (s.as_bytes().get(*at) == Some(&c)).then(|| *at += 1)
    }
    let mut at = 0;
    let year = digits(text, &mut at, 4, 4)?;
    lit(text, &mut at, b'-')?;
    let month = digits(text, &mut at, 1, 2)?;
    lit(text, &mut at, b'-')?;
    let day = digits(text, &mut at, 1, 2)?;
    lit(text, &mut at, b'T')?;
    let hour = digits(text, &mut at, 1, 2)?;
    lit(text, &mut at, b':')?;
    let minute = digits(text, &mut at, 1, 2)?;
    lit(text, &mut at, b':')?;
    let second = digits(text, &mut at, 1, 2)?;
    lit(text, &mut at, b'Z')?;
    if at != text.len() || year < 1 {
        return None;
    }
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > days {
        return None;
    }
    let secs =
        ((days_from_civil(year, month as u32, day as u32) * 24 + hour) * 60 + minute) * 60 + second;
    Some(WallClock::from_naive_secs(secs))
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `_mcp_time_cell`: `HH:MM:SS` for a call made today (UTC, as recorded), `MMM D, HH:MM:SS`
/// otherwise, and the text as it came when it is not a timestamp. `now` is the present in Unix
/// seconds.
pub fn time_cell(timestamp: &str, now: i64) -> String {
    let Some(when) = parse_timestamp(timestamp) else {
        return timestamp.to_owned();
    };
    let today = WallClock::from_naive_secs(now);
    let clock = format!("{:02}:{:02}:{:02}", when.hour, when.minute, when.second);
    if when.day_number() == today.day_number() {
        clock
    } else {
        format!("{} {}, {clock}", when.month_abbr(), when.day)
    }
}

/// `_mcp_target_cell`.
pub fn target_cell(entry: &Value) -> String {
    let row = get(entry, "row").filter(|v| !v.is_null());
    let candidate = get(entry, "candidate_index").filter(|v| !v.is_null());
    match (row, candidate) {
        (None, _) => "\u{2014}".to_owned(),
        (Some(r), None) => format!("#{}", py_str(r)),
        (Some(r), Some(c)) => format!("#{} \u{2192} candidate {}", py_str(r), py_str(c)),
    }
}

/// `_mcp_outcome_weight`.
pub fn outcome_weight(entry: &Value) -> Weight {
    if !flag(entry, "allowed") || flag(entry, "dry_run") {
        return Weight::Ink100;
    }
    let tool = tool_name(entry);
    let outcome = outcome_of(entry);
    if is_hydrus_send(&tool) {
        if okey(&outcome, "success") {
            return Weight::Ink65;
        }
        return if okey(&outcome, "error") || okey(&outcome, "skipped_reason") {
            Weight::Ink100
        } else {
            Weight::Ink65
        };
    }
    if tool == "research" {
        return if okey(&outcome, "research_started") && okey(&outcome, "completed") {
            Weight::Ink65
        } else {
            Weight::Ink100
        };
    }
    if outcome.contains_key("error") {
        Weight::Ink100
    } else {
        Weight::Ink65
    }
}

/// `_mcp_outcome_cell`: the cell's text and weight.
pub fn outcome_cell(entry: &Value) -> (String, Weight) {
    let tool = tool_name(entry);
    let outcome = outcome_of(entry);
    let weight = outcome_weight(entry);

    if !flag(entry, "allowed") {
        let tier = match get(entry, "tier") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => py_str(other),
            None => String::new(),
        };
        return (
            format!("Refused \u{2014} {} is off", tier_label(&tier)),
            weight,
        );
    }
    if flag(entry, "dry_run") {
        return (
            "Recorded, not run \u{2014} dry run is on".to_owned(),
            weight,
        );
    }
    let text = match tool.as_str() {
        t if is_hydrus_send(t) => {
            if okey(&outcome, "success") {
                format!("Allowed \u{2014} sent  {SENT_GLYPH}")
            } else {
                match error_or_skipped(&outcome) {
                    Some(e) => format!("Error \u{2014} {}", py_str(e)),
                    None => "Allowed".to_owned(),
                }
            }
        }
        "select_candidate" => format!(
            "Allowed \u{2014} picked candidate {}",
            outcome
                .get("selected_candidate_index")
                .map_or_else(|| "None".to_owned(), py_str)
        ),
        "toggle_reviewed" => format!(
            "Allowed \u{2014} marked {}",
            if okey(&outcome, "reviewed") {
                "reviewed"
            } else {
                "unreviewed"
            }
        ),
        "research" => {
            if okey(&outcome, "research_started") && okey(&outcome, "completed") {
                // the audit entry records the final state, not a before and after, so "new
                // candidates" is the candidate count over the re-searched rows
                let count: i64 = match outcome.get("results") {
                    Some(Value::Array(rows)) => rows
                        .iter()
                        .map(|r| {
                            r.get("candidate_count")
                                .and_then(Value::as_i64)
                                .unwrap_or(0)
                        })
                        .sum(),
                    _ => 0,
                };
                format!("Allowed \u{2014} re-searched, {count} new candidates")
            } else {
                format!(
                    "Error \u{2014} {}",
                    outcome
                        .get("reason")
                        .map_or_else(|| "research did not complete".to_owned(), py_str)
                )
            }
        }
        "remove_row" => "Allowed \u{2014} row removed".to_owned(),
        "reset_result" => "Allowed \u{2014} result reset".to_owned(),
        _ => match outcome.get("error") {
            Some(e) => format!("Error \u{2014} {}", py_str(e)),
            None => "Allowed".to_owned(),
        },
    };
    (text, weight)
}

/// `_mcp_outcome_tooltip`: the whole of what the tool returned, on hover.
pub fn outcome_tooltip(entry: &Value) -> String {
    if !flag(entry, "allowed") {
        return match get(entry, "reason") {
            Some(v) if truthy(v) => py_str(v),
            _ => String::new(),
        };
    }
    let outcome = outcome_of(entry);
    if flag(entry, "dry_run") {
        if let Some(note) = outcome.get("note").filter(|n| truthy(n)) {
            return py_str(note);
        }
    }
    py_dumps(&Value::Object(outcome))
}

/// One row of the table, as the cells read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub time: String,
    pub tool: String,
    pub tier: String,
    pub target: String,
    pub outcome: String,
    pub weight: Weight,
    pub tooltip: String,
}

/// What `_refresh_mcp_audit_table` draws: the rows newest first, and the caption over the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditView {
    pub rows: Vec<AuditRow>,
    pub caption: String,
}

/// `_refresh_mcp_audit_table` over the entries read. `actions_only` is the Actions filter
/// (`tier != "read"`); `now` is the present in Unix seconds.
pub fn view(entries: &[Value], actions_only: bool, now: i64) -> AuditView {
    let kept: Vec<&Value> = entries
        .iter()
        .filter(|e| {
            !actions_only || get(e, "tier").is_none_or(|t| t != &Value::String("read".to_owned()))
        })
        .collect();
    let total = kept.len();
    let start = total.saturating_sub(DISPLAY_LIMIT);
    let rows: Vec<AuditRow> = kept[start..]
        .iter()
        .rev()
        .map(|entry| {
            let (outcome, weight) = outcome_cell(entry);
            let timestamp = match get(entry, "timestamp") {
                Some(v) => py_str(v),
                None => String::new(),
            };
            let tier = match get(entry, "tier") {
                Some(Value::String(s)) => tier_label(s).to_owned(),
                Some(other) => py_str(other),
                None => String::new(),
            };
            AuditRow {
                time: time_cell(&timestamp, now),
                tool: match get(entry, "tool") {
                    Some(v) => py_str(v),
                    None => String::new(),
                },
                tier,
                target: target_cell(entry),
                outcome,
                weight,
                tooltip: outcome_tooltip(entry),
            }
        })
        .collect();
    let caption = if total > rows.len() {
        format!("Showing the most recent {} of {total} calls.", rows.len())
    } else {
        String::new()
    };
    AuditView { rows, caption }
}

/// The Connection card's state, 1 to 6 (`mcp-settings-spec.md` section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// Not enabled, or enabled with no failure and nothing running.
    Stopped = 1,
    Listening = 2,
    /// The optional package is missing (Python only: this build has the server in it).
    NotInstalled = 3,
    NoToken = 4,
    PortBusy = 5,
    /// Everything was in place and the start failed anyway.
    Failed = 6,
}

/// What the Connection card says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionText {
    pub state: Connection,
    pub primary: String,
    pub remedy: String,
    /// States 1 and 2 are drawn at `ink_65`, 3 to 6 at `ink_100`.
    pub weight: Weight,
    pub show_restart: bool,
    pub show_open_log: bool,
}

/// What the controller reports for the card.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerReport {
    pub running: bool,
    /// `last_error`: Python's text, used to pick the state and never shown (it can carry a token
    /// character).
    pub last_error: Option<String>,
    /// `last_error_user_message()`: what the card shows for a failure it has no wording for.
    pub user_message: Option<String>,
}

/// `_refresh_mcp_connection`. `has_server` is false when the dialog has no window to ask, in
/// which case an enabled box with nothing running reads Stopped.
pub fn connection(enabled: bool, port: i64, server: Option<&ServerReport>) -> ConnectionText {
    let state = if !enabled {
        Connection::Stopped
    } else if server.is_some_and(|s| s.running) {
        Connection::Listening
    } else {
        let err = server
            .and_then(|s| s.last_error.clone())
            .unwrap_or_default();
        let lowered = err.to_lowercase();
        if lowered.contains("not installed") {
            Connection::NotInstalled
        } else if lowered.contains("blank bearer token") {
            Connection::NoToken
        } else if lowered.contains("in use") {
            Connection::PortBusy
        } else if !err.is_empty() {
            Connection::Failed
        } else {
            Connection::Stopped
        }
    };
    let host = hatate_core::mcp::HOST;
    let (primary, remedy) = match state {
        Connection::Stopped => (
            "Stopped.".to_owned(),
            "Turn on \"Enable the MCP server\" above to let a connected AI read the queue \u{2014} and, once you choose, act on it.".to_owned(),
        ),
        Connection::Listening => (
            format!("Listening on {host}:{port}."),
            "A client with the token below can connect now.".to_owned(),
        ),
        Connection::NotInstalled => (
            "Can't start \u{2014} the mcp package isn't installed.".to_owned(),
            "Install it: venv/bin/pip install \"mcp>=1.2,<2\", then restart Hatate. (It's optional, so this app never installs it for you.)".to_owned(),
        ),
        Connection::NoToken => (
            "Can't start \u{2014} no token is set.".to_owned(),
            "Generate one below, then click \"Restart server.\"".to_owned(),
        ),
        Connection::PortBusy => (
            format!("Can't start \u{2014} port {port} is already in use."),
            "Something else on this machine is using that port \u{2014} maybe another copy of Hatate. Pick a different port below and click \"Restart server.\"".to_owned(),
        ),
        Connection::Failed => (
            server
                .and_then(|s| s.user_message.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "Couldn't start.".to_owned()),
            String::new(),
        ),
    };
    let healthy = matches!(state, Connection::Stopped | Connection::Listening);
    ConnectionText {
        state,
        primary,
        remedy,
        weight: if healthy {
            Weight::Ink65
        } else {
            Weight::Ink100
        },
        show_restart: enabled,
        show_open_log: state == Connection::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cell(entry: Value) -> (String, Weight) {
        outcome_cell(&entry)
    }

    // tests/test_mcp_settings_tab.py TestAuditOutcomeFormatting
    #[test]
    fn a_refused_call_names_its_tier_and_is_drawn_at_ink_100() {
        let e =
            json!({"allowed": false, "tier": "research", "reason": "irrelevant to the cell text"});
        assert_eq!(
            cell(e),
            (
                "Refused \u{2014} Re-search is off".to_owned(),
                Weight::Ink100
            )
        );
    }

    #[test]
    fn a_dry_run_is_recorded_not_run_at_ink_100() {
        let e = json!({"allowed": true, "dry_run": true, "tier": "hydrus_write",
            "outcome": {"note": "dry_run is on: nothing was changed."}});
        assert_eq!(
            cell(e),
            (
                "Recorded, not run \u{2014} dry run is on".to_owned(),
                Weight::Ink100
            )
        );
    }

    #[test]
    fn a_successful_hydrus_send_ends_in_the_sent_glyph_and_a_refused_one_does_not() {
        let sent = json!({"allowed": true, "dry_run": false, "tool": "send_upload",
            "outcome": {"row": 3, "success": true, "warning": null, "error": null}});
        let (text, weight) = cell(sent);
        assert_eq!(text, "Allowed \u{2014} sent  \u{25cf}");
        assert_eq!(weight, Weight::Ink65);
        let refused = json!({"allowed": false, "tier": "hydrus_write", "reason": "x"});
        assert!(!cell(refused).0.contains('\u{25cf}'));
    }

    #[test]
    fn a_failed_hydrus_send_is_an_error_at_ink_100_and_a_bare_one_is_plain_allowed() {
        let failed = json!({"allowed": true, "dry_run": false, "tool": "send_url",
            "outcome": {"success": false, "error": "no route"}});
        assert_eq!(
            cell(failed),
            ("Error \u{2014} no route".to_owned(), Weight::Ink100)
        );
        let skipped = json!({"allowed": true, "dry_run": false, "tool": "download_send",
            "outcome": {"success": false, "error": null, "skipped_reason": "already there"}});
        assert_eq!(
            cell(skipped),
            ("Error \u{2014} already there".to_owned(), Weight::Ink100)
        );
        let bare = json!({"allowed": true, "dry_run": false, "tool": "send_url", "outcome": {}});
        assert_eq!(cell(bare), ("Allowed".to_owned(), Weight::Ink65));
    }

    #[test]
    fn the_local_decisions_read_as_the_spec_table_says() {
        let pick = json!({"allowed": true, "dry_run": false, "tool": "select_candidate",
            "outcome": {"row": 1, "selected_candidate_index": 2}});
        assert_eq!(cell(pick).0, "Allowed \u{2014} picked candidate 2");
        let reviewed = json!({"allowed": true, "dry_run": false, "tool": "toggle_reviewed",
            "outcome": {"row": 1, "reviewed": true}});
        assert_eq!(cell(reviewed).0, "Allowed \u{2014} marked reviewed");
        let unreviewed = json!({"allowed": true, "dry_run": false, "tool": "toggle_reviewed",
            "outcome": {"row": 1, "reviewed": false}});
        assert_eq!(cell(unreviewed).0, "Allowed \u{2014} marked unreviewed");
        assert_eq!(
            cell(
                json!({"allowed": true, "dry_run": false, "tool": "remove_row",
                "outcome": {"row": 1, "removed": "x.png"}})
            )
            .0,
            "Allowed \u{2014} row removed"
        );
        assert_eq!(
            cell(
                json!({"allowed": true, "dry_run": false, "tool": "reset_result",
                "outcome": {"row": 1, "reset": true}})
            )
            .0,
            "Allowed \u{2014} result reset"
        );
    }

    #[test]
    fn research_counts_candidates_over_its_rows_and_an_unfinished_one_is_an_error() {
        let done = json!({"allowed": true, "dry_run": false, "tool": "research",
            "outcome": {"research_started": true, "completed": true,
                "results": [{"candidate_count": 2}, {"candidate_count": 3}, {}]}});
        assert_eq!(
            cell(done),
            (
                "Allowed \u{2014} re-searched, 5 new candidates".to_owned(),
                Weight::Ink65
            )
        );
        let stuck = json!({"allowed": true, "dry_run": false, "tool": "research",
            "outcome": {"research_started": true, "completed": false, "reason": "timed out"}});
        assert_eq!(
            cell(stuck),
            ("Error \u{2014} timed out".to_owned(), Weight::Ink100)
        );
        let blank = json!({"allowed": true, "dry_run": false, "tool": "research", "outcome": {}});
        assert_eq!(cell(blank).0, "Error \u{2014} research did not complete");
    }

    #[test]
    fn a_read_with_an_error_key_is_an_error_and_one_without_is_allowed() {
        let bad = json!({"allowed": true, "dry_run": false, "tool": "get_entry",
            "outcome": {"error": "row 9 does not exist"}});
        assert_eq!(
            cell(bad),
            (
                "Error \u{2014} row 9 does not exist".to_owned(),
                Weight::Ink100
            )
        );
        let ok = json!({"allowed": true, "dry_run": false, "tool": "list_queue", "outcome": {"rows": []}});
        assert_eq!(cell(ok), ("Allowed".to_owned(), Weight::Ink65));
        // `outcome` of null, as a line with no outcome has it
        let none =
            json!({"allowed": true, "dry_run": false, "tool": "list_queue", "outcome": null});
        assert_eq!(cell(none), ("Allowed".to_owned(), Weight::Ink65));
    }

    // TestTargetCell
    #[test]
    fn the_target_is_the_row_and_candidate_the_row_or_a_dash() {
        assert_eq!(
            target_cell(&json!({"row": 482, "candidate_index": 2})),
            "#482 \u{2192} candidate 2"
        );
        assert_eq!(
            target_cell(&json!({"row": 482, "candidate_index": null})),
            "#482"
        );
        assert_eq!(
            target_cell(&json!({"row": null, "candidate_index": null})),
            "\u{2014}"
        );
        assert_eq!(target_cell(&json!({})), "\u{2014}");
    }

    // TestTimeCell
    #[test]
    fn a_past_date_shows_the_month_and_day_and_today_only_the_time() {
        let now = 1_700_000_000; // 2023-11-14T22:13:20Z
        assert_eq!(time_cell("2020-01-02T03:04:05Z", now), "Jan 2, 03:04:05");
        assert_eq!(time_cell("2023-11-14T01:02:03Z", now), "01:02:03");
        assert_eq!(time_cell("2023-11-13T23:59:59Z", now), "Nov 13, 23:59:59");
    }

    #[test]
    fn a_timestamp_strptime_would_refuse_is_shown_as_it_came() {
        for bad in [
            "garbage",
            "",
            "2020-01-02T03:04:05",
            "2020-01-02 03:04:05Z",
            "2020-13-02T03:04:05Z",
            "2020-02-30T03:04:05Z",
            "2020-01-02T24:04:05Z",
            "2020-01-02T03:04:60Z",
        ] {
            assert_eq!(time_cell(bad, 0), bad);
        }
        // strptime takes one digit for the fields after the year
        assert_eq!(time_cell("2020-1-2T3:4:5Z", 0), "Jan 2, 03:04:05");
    }

    // TestReadAuditLog
    #[test]
    fn a_missing_log_is_empty_and_a_truncated_last_line_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_audit.jsonl");
        assert!(read_entries(&path).is_empty());
        std::fs::write(
            &path,
            "{\"tool\": \"list_queue\", \"tier\": \"read\"}\n\n{\"tool\": \"send_upload\", \"tier\": \"hydrus_writ",
        )
        .unwrap();
        let entries = read_entries(&path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["tool"], "list_queue");
    }

    #[test]
    fn the_table_is_newest_first_filtered_to_actions_and_capped_with_a_caption() {
        let mut entries = vec![
            json!({"timestamp": "2020-01-01T00:00:00Z", "tool": "list_queue",
            "tier": "read", "allowed": true, "dry_run": false, "row": null,
            "candidate_index": null, "outcome": {"rows": []}}),
        ];
        for i in 0..(DISPLAY_LIMIT + 5) {
            entries.push(
                json!({"timestamp": "2020-01-02T03:04:05Z", "tool": "select_candidate",
                "tier": "local_write", "allowed": true, "dry_run": false, "row": i,
                "candidate_index": 1, "outcome": {"selected_candidate_index": 1}}),
            );
        }
        let actions = view(&entries, true, 0);
        assert_eq!(actions.rows.len(), DISPLAY_LIMIT);
        assert_eq!(
            actions.rows[0].target,
            format!("#{} \u{2192} candidate 1", DISPLAY_LIMIT + 4)
        );
        assert_eq!(actions.rows[0].tier, "Reversible");
        assert_eq!(
            actions.caption,
            format!(
                "Showing the most recent {DISPLAY_LIMIT} of {} calls.",
                DISPLAY_LIMIT + 5
            )
        );
        let all = view(&entries, false, 0);
        assert!(
            all.caption
                .contains(&format!("of {} calls", DISPLAY_LIMIT + 6))
        );
        let few = view(&entries[..2], false, 0);
        assert_eq!(few.rows.len(), 2);
        assert_eq!(few.rows[1].tool, "list_queue");
        assert_eq!(few.caption, "");
    }

    #[test]
    fn the_tooltip_is_the_refusal_the_dry_run_note_or_the_outcome_as_python_dumps_it() {
        let refused =
            json!({"allowed": false, "reason": "\"research\" refused: Re-search is off."});
        assert_eq!(
            outcome_tooltip(&refused),
            "\"research\" refused: Re-search is off."
        );
        let dry = json!({"allowed": true, "dry_run": true,
            "outcome": {"note": "dry_run is on: nothing was changed."}});
        assert_eq!(outcome_tooltip(&dry), "dry_run is on: nothing was changed.");
        let real = json!({"allowed": true, "dry_run": false,
            "outcome": {"row": 1, "name": "caf\u{e9}", "ok": true, "n": null}});
        assert_eq!(
            outcome_tooltip(&real),
            "{\"row\": 1, \"name\": \"caf\\u00e9\", \"ok\": true, \"n\": null}"
        );
        assert_eq!(outcome_tooltip(&json!({"allowed": true})), "{}");
    }

    // TestConnectionStates
    fn report(running: bool, err: Option<&str>) -> ServerReport {
        ServerReport {
            running,
            last_error: err.map(str::to_owned),
            user_message: err.map(|_| "A message for the user.".to_owned()),
        }
    }

    #[test]
    fn the_six_states_read_as_the_spec_says() {
        let stopped = connection(false, 8787, Some(&report(true, None)));
        assert_eq!(stopped.state, Connection::Stopped);
        assert_eq!(stopped.primary, "Stopped.");
        assert!(!stopped.show_restart);

        let live = connection(true, 8787, Some(&report(true, None)));
        assert_eq!(live.state, Connection::Listening);
        assert_eq!(live.primary, "Listening on 127.0.0.1:8787.");
        assert_eq!(live.weight, Weight::Ink65);
        assert!(live.show_restart && !live.show_open_log);

        let missing = connection(
            true,
            8787,
            Some(&report(false, Some("The mcp package is not installed"))),
        );
        assert_eq!(missing.state, Connection::NotInstalled);

        let blank = connection(
            true,
            8787,
            Some(&report(
                false,
                Some("Blank bearer token: refusing to start the MCP server"),
            )),
        );
        assert_eq!(blank.state, Connection::NoToken);
        assert_eq!(blank.primary, "Can't start \u{2014} no token is set.");
        assert_eq!(blank.weight, Weight::Ink100);

        let busy = connection(
            true,
            18787,
            Some(&report(
                false,
                Some("MCP server did not start listening on port 18787 (port already in use?)"),
            )),
        );
        assert_eq!(busy.state, Connection::PortBusy);
        assert_eq!(
            busy.primary,
            "Can't start \u{2014} port 18787 is already in use."
        );

        let other = connection(
            true,
            8787,
            Some(&report(false, Some("something else broke"))),
        );
        assert_eq!(other.state, Connection::Failed);
        assert_eq!(other.primary, "A message for the user.");
        assert!(other.remedy.is_empty() && other.show_open_log);

        // enabled, nothing has tried to start, or there is no window to ask
        assert_eq!(
            connection(true, 8787, Some(&report(false, None))).state,
            Connection::Stopped
        );
        assert_eq!(connection(true, 8787, None).state, Connection::Stopped);
    }

    #[test]
    fn a_failure_card_shows_the_user_message_never_the_raw_text() {
        let raw = "Could not start the MCP server: 'latin-1' codec can't encode character '\\u2603' in position 7";
        let r = ServerReport {
            running: false,
            last_error: Some(raw.to_owned()),
            user_message: Some(
                "The MCP bearer token has a character that cannot be sent.".to_owned(),
            ),
        };
        let card = connection(true, 8787, Some(&r));
        assert!(!card.primary.contains("latin-1") && !card.primary.contains("position"));
        assert_eq!(card.state, Connection::Failed);
    }
}
