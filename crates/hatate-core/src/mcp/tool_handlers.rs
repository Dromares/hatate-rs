//! The tool bodies (`McpToolHandlers` of `gui/mcp_bridge.py`), gated and audited.
//!
//! One instance lives as long as the server does. Every public method corresponds to exactly one
//! tool; [`super::handlers::McpHandlers`] is how the server reaches them. Python's handlers drive
//! a `MainWindow`; these drive a [`QueueModel`] through its [`QueueChannel`] (the window's list,
//! `MainThreadInvoker`), the merged search runner, and the Hydrus import paths, and do not
//! re-implement any of them.
//!
//! Where Python runs a Hydrus send ON the GUI thread (the window is blocked until Hydrus
//! answers), the model here stays free: the entry is copied out, sent, and the result written back
//! in one step. The call keeps Python's 60 s limit and its timeout text, and a send that outlives
//! it still finishes and still writes back, as Python's does.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use super::audit::{self, AuditEntry};
use super::content::{ImageValue, ToolOutput};
use super::handlers::{HandlerError, HandlerResult, McpHandlers};
use super::tools::{self, TOOLS};
use crate::batch::staging::rmtree_ignore_errors;
use crate::config::{HydrusSettings, McpSettings, Settings};
use crate::hydrus_client::{HydrusClient, HydrusError};
use crate::hydrus_import::{
    ImportResult, download_and_send, send_file_upload, send_url_or_download,
};
use crate::mcp_images;
use crate::model::Entry;
use crate::pycase::casefold;
use crate::pycompat::strip;
use crate::queue::{
    DEFAULT_TIMEOUT_SECONDS, EntryId, HYDRUS_SEND_TIMEOUT_SECONDS, QueueChannel, QueueModel,
    RESEARCH_TIMEOUT_FLOOR_SECONDS, RESEARCH_TIMEOUT_PER_IMAGE_SECONDS, RemoveReason,
    ResearchStart, SearchLauncher, has_result, needs_review, start_research,
};
use crate::ranking;
use crate::remote::secs;
use crate::search_engine::SearchEngine;

/// How the handlers build a Hydrus client: the binary passes [`HydrusClient::new`], a test one
/// that trusts its local server's certificate.
pub type HydrusClientFactory =
    Arc<dyn Fn(&HydrusSettings) -> Result<HydrusClient, HydrusError> + Send + Sync>;

/// The limits on a marshaled call, in seconds. Python's constants; a test shortens them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeouts {
    pub default: f64,
    pub hydrus_send: f64,
    pub research_per_image: f64,
    pub research_floor: f64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            default: DEFAULT_TIMEOUT_SECONDS,
            hydrus_send: HYDRUS_SEND_TIMEOUT_SECONDS,
            research_per_image: RESEARCH_TIMEOUT_PER_IMAGE_SECONDS,
            research_floor: RESEARCH_TIMEOUT_FLOOR_SECONDS,
        }
    }
}

/// Everything a handler set needs besides the queue.
pub struct HandlerEnv {
    pub timeouts: Timeouts,
    /// `window.settings`: the one object the GUI edits; read on every call.
    pub settings: Arc<Mutex<Settings>>,
    pub engine: Arc<SearchEngine>,
    pub hydrus_client: HydrusClientFactory,
    pub launcher: Arc<dyn SearchLauncher>,
    /// Where the blocking handler threads run the async parts.
    pub runtime: tokio::runtime::Handle,
    /// `mcp_audit.AUDIT_LOG_FILE`; overridable so a test never touches the real config directory.
    pub audit_path: PathBuf,
}

/// `McpToolHandlers`.
pub struct McpToolHandlers {
    channel: QueueChannel,
    env: Arc<HandlerEnv>,
}

type Fields = Vec<(&'static str, Value)>;

impl McpToolHandlers {
    pub fn new(channel: QueueChannel, env: HandlerEnv) -> McpToolHandlers {
        McpToolHandlers {
            channel,
            env: Arc::new(env),
        }
    }

    fn mcp_settings(&self) -> McpSettings {
        self.env.settings.lock().expect("settings lock").mcp.clone()
    }

    /// `_run_tool`: gate the call against the live settings, apply `dry_run`, audit it (allowed,
    /// refused or dry-run alike), and run the body.
    fn run_tool(
        &self,
        tool_name: &str,
        arguments: Map<String, Value>,
        row: Option<i64>,
        candidate_index: Option<i64>,
        perform: impl FnOnce() -> HandlerResult,
    ) -> HandlerResult {
        let settings = self.mcp_settings();
        let spec =
            tools::tool(tool_name).ok_or_else(|| HandlerError::new(format!("{tool_name:?}")))?;
        let tier = spec.tier.as_str();
        let refusal =
            tools::gate(tool_name, &settings).map_err(|e| HandlerError::new(e.detail()))?;
        if let Some(reason) = refusal {
            let mut entry = AuditEntry::new(tool_name, tier, false);
            entry.arguments = Some(&arguments);
            entry.reason = Some(&reason);
            entry.row = row;
            entry.candidate_index = candidate_index;
            audit::record(&entry, &self.env.audit_path, settings.audit_log_max_bytes);
            return Ok(ToolOutput::new()
                .push("allowed", json!(false))
                .push("reason", json!(reason)));
        }

        let dry_run = settings.dry_run && spec.tier.is_dry_runnable();
        let mut result = if dry_run {
            ToolOutput::new()
                .push("dry_run", json!(true))
                .push("would_call", json!(tool_name))
                .push("arguments", Value::Object(arguments.clone()))
                .push("note", json!("dry_run is on: nothing was changed."))
        } else {
            let mut result = perform()?;
            if !result.fields.iter().any(|(k, _)| k == "dry_run") {
                result = result.push("dry_run", json!(false));
            }
            result
        };

        let outcome = result.to_audit_json();
        let mut entry = AuditEntry::new(tool_name, tier, true);
        entry.arguments = Some(&arguments);
        entry.dry_run = dry_run;
        entry.row = row;
        entry.candidate_index = candidate_index;
        entry.outcome = Some(&outcome);
        audit::record(&entry, &self.env.audit_path, settings.audit_log_max_bytes);
        result.fields.insert(
            0,
            (
                "allowed".to_owned(),
                super::content::ResultValue::Json(json!(true)),
            ),
        );
        Ok(result)
    }

    /// `invoker.call(fn, timeout)`: a failure is the exception text.
    fn on_model<T, F>(&self, timeout: f64, f: F) -> Result<T, HandlerError>
    where
        T: Send + 'static,
        F: FnOnce(&mut QueueModel) -> Result<T, String> + Send + 'static,
    {
        self.channel
            .call(f, timeout)
            .map_err(|e| HandlerError::new(e.text()))
    }

    /// The lookup `get_images` and `get_diff` share: the entry's path and the candidate, copied
    /// out of the model, or `None` when the row or the candidate does not exist.
    fn lookup_candidate(
        &self,
        row: i64,
        candidate_index: i64,
    ) -> Result<Option<(EntryId, String, crate::model::Candidate)>, HandlerError> {
        self.on_model(self.env.timeouts.default, move |m| {
            let Some(id) = m.id_at(row) else {
                return Ok(None);
            };
            let entry = m.entry(id).expect("live id");
            let Some(candidate) = usize::try_from(candidate_index)
                .ok()
                .and_then(|i| entry.candidates.get(i))
            else {
                return Ok(None);
            };
            Ok(Some((id, entry.path.clone(), candidate.clone())))
        })
    }

    /// `fetch_full_resolution_match`, then keeps what it learned about the candidate: Python
    /// fills the candidate in place, and the session later holds the result.
    fn fetch_match(
        &self,
        id: EntryId,
        path: &str,
        candidate_index: i64,
        mut candidate: crate::model::Candidate,
    ) -> Result<mcp_images::FullResolutionMatch, HandlerError> {
        let fetched = self
            .env
            .runtime
            .block_on(mcp_images::fetch_full_resolution_match(
                &mut candidate,
                &self.env.engine,
                &self.env.settings,
                Some(path),
            ));
        let url = candidate.url.clone();
        self.channel.post(move |m| {
            if let Some(slot) = m
                .entry_mut(id)
                .and_then(|e| {
                    usize::try_from(candidate_index)
                        .ok()
                        .and_then(|i| e.candidates.get_mut(i))
                })
                .filter(|c| c.url == url)
            {
                *slot = candidate;
            }
        });
        fetched.map_err(|raised| HandlerError::new(raised.to_string()))
    }

    // ------------------------------------------------------------------
    // The Hydrus sends
    // ------------------------------------------------------------------

    /// `invoker.call(work, timeout=HYDRUS_SEND_TIMEOUT_SECONDS)` for a body that does network
    /// I/O: runs on its own thread, so the wait can end at the limit while the send goes on.
    fn send_with_limit(&self, row: i64, kind: SendKind) -> HandlerResult {
        let (tx, rx) = mpsc::sync_channel(1);
        let job = SendJob {
            channel: self.channel.clone(),
            env: self.env.clone(),
            row,
            kind,
        };
        std::thread::Builder::new()
            .name("mcp-hydrus-send".to_owned())
            .spawn(move || {
                let _ = tx.send(job.run());
            })
            .map_err(|e| HandlerError::new(e.to_string()))?;
        let limit = self.env.timeouts.hydrus_send;
        match rx.recv_timeout(secs(limit)) {
            Ok(result) => result,
            Err(_) => Err(HandlerError::new(format!(
                "Timed out after {limit:.0}s waiting for the GUI thread"
            ))),
        }
    }

    fn args(pairs: Fields) -> Map<String, Value> {
        pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    fn done(fields: Fields) -> HandlerResult {
        let mut out = ToolOutput::new();
        for (k, v) in fields {
            out = out.push(k, v);
        }
        Ok(out)
    }
}

impl McpHandlers for McpToolHandlers {
    fn list_queue(&self, offset: i64, limit: i64, filter_text: Option<String>) -> HandlerResult {
        let arguments = Self::args(vec![
            ("offset", json!(offset)),
            ("limit", json!(limit)),
            ("filter", json!(filter_text)),
        ]);
        self.run_tool("list_queue", arguments, None, None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let mut rows: Vec<Value> = Vec::new();
                let mut matched: i64 = 0;
                for (index, entry) in m.entries().iter().enumerate() {
                    if filter_text.as_deref().is_some_and(|t| !t.is_empty())
                        && !matches_filter(entry, filter_text.as_deref().unwrap_or(""))
                    {
                        continue;
                    }
                    if matched < offset {
                        matched += 1;
                        continue;
                    }
                    if (rows.len() as i64) >= limit {
                        matched += 1;
                        continue;
                    }
                    rows.push(queue_row(index, entry));
                    matched += 1;
                }
                Ok(ToolOutput::new()
                    .push("rows", Value::Array(rows))
                    .push("total_matching", json!(matched))
                    .push("total_in_queue", json!(m.len())))
            })
        })
    }

    fn get_entry(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("get_entry", arguments, Some(row), None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let Some(entry) = m.entry_at(row) else {
                    return Ok(ToolOutput::new().push(
                        "error",
                        json!(format!("No row {row} (queue has {})", m.len())),
                    ));
                };
                let mut out = ToolOutput::new().push("row", json!(row));
                for (k, v) in entry_detail(entry) {
                    out = out.push(&k, v);
                }
                Ok(out)
            })
        })
    }

    fn get_candidates(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("get_candidates", arguments, Some(row), None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let Some(entry) = m.entry_at(row) else {
                    return Ok(ToolOutput::new().push(
                        "error",
                        json!(format!("No row {row} (queue has {})", m.len())),
                    ));
                };
                let mut scored: Vec<(f64, Map<String, Value>)> = Vec::new();
                for (index, candidate) in entry.candidates.iter().enumerate() {
                    let b =
                        ranking::score_breakdown(candidate, entry.local_width, entry.local_height);
                    let mut item = Map::new();
                    item.insert("candidate_index".into(), json!(index));
                    item.insert("url".into(), json!(candidate.url));
                    item.insert("source_name".into(), json!(candidate.source_name));
                    item.insert("engine".into(), json!(candidate.engine));
                    item.insert("width".into(), json!(candidate.width));
                    item.insert("height".into(), json!(candidate.height));
                    item.insert(
                        "is_selected".into(),
                        json!(index == entry.selected_candidate_index),
                    );
                    item.insert("remote_available".into(), json!(candidate.remote_available));
                    item.insert("restricted".into(), json!(candidate.restricted));
                    item.insert("similarity".into(), num(b.similarity));
                    item.insert("has_parser".into(), json!(b.has_parser));
                    item.insert("parser_bonus".into(), num(b.parser_bonus));
                    item.insert("resolution_bonus".into(), num(b.resolution_bonus));
                    item.insert("quality_bonus".into(), num(b.quality_bonus));
                    item.insert("quality_bonus_cap".into(), num(b.quality_bonus_cap));
                    item.insert(
                        "unavailable_penalty_applied".into(),
                        json!(b.unavailable_penalty_applied),
                    );
                    item.insert("unavailable_penalty".into(), num(b.unavailable_penalty));
                    item.insert("score".into(), num(b.score));
                    scored.push((b.score, item));
                }
                // `scored.sort(key=lambda item: -item["score"])`: stable, best first.
                scored.sort_by(|a, b| {
                    (-a.0)
                        .partial_cmp(&(-b.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                Ok(ToolOutput::new().push("row", json!(row)).push(
                    "candidates",
                    Value::Array(scored.into_iter().map(|(_, i)| Value::Object(i)).collect()),
                ))
            })
        })
    }

    fn get_images(&self, row: i64, candidate_index: i64) -> HandlerResult {
        let arguments = Self::args(vec![
            ("row", json!(row)),
            ("candidate_index", json!(candidate_index)),
        ]);
        self.run_tool(
            "get_images",
            arguments,
            Some(row),
            Some(candidate_index),
            || {
                let Some((id, path, candidate)) = self.lookup_candidate(row, candidate_index)?
                else {
                    return Self::done(vec![(
                        "error",
                        json!(format!(
                            "No row {row} / candidate {candidate_index} to fetch"
                        )),
                    )]);
                };
                let local_bytes = mcp_images::local_image_bytes(&path);
                let matched = self.fetch_match(id, &path, candidate_index, candidate)?;
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("candidate_index", json!(candidate_index))
                    .push_image(
                        "local_image",
                        ImageValue::new(
                            local_bytes,
                            vec![("mime_type".into(), json!(guess_mime(&path)))],
                        ),
                    )
                    .push_image(
                        "match_image",
                        ImageValue::new(
                            matched.data,
                            vec![
                                (
                                    "mime_type".into(),
                                    json!(guess_mime(matched.url.as_deref().unwrap_or(""))),
                                ),
                                ("url".into(), json!(matched.url)),
                                (
                                    "is_full_resolution".into(),
                                    json!(matched.is_full_resolution),
                                ),
                                ("fallback_reason".into(), json!(matched.fallback_reason)),
                            ],
                        ),
                    ))
            },
        )
    }

    fn get_diff(&self, row: i64, candidate_index: i64) -> HandlerResult {
        let arguments = Self::args(vec![
            ("row", json!(row)),
            ("candidate_index", json!(candidate_index)),
        ]);
        self.run_tool(
            "get_diff",
            arguments,
            Some(row),
            Some(candidate_index),
            || {
                let Some((id, path, candidate)) = self.lookup_candidate(row, candidate_index)?
                else {
                    return Self::done(vec![(
                        "error",
                        json!(format!(
                            "No row {row} / candidate {candidate_index} to compare"
                        )),
                    )]);
                };
                let local_bytes = mcp_images::local_image_bytes(&path);
                let matched = self.fetch_match(id, &path, candidate_index, candidate)?;
                let diff =
                    mcp_images::compute_diff(local_bytes.as_deref(), matched.data.as_deref())
                        .map_err(|e| HandlerError::new(e.detail()))?;
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("candidate_index", json!(candidate_index))
                    .push_image(
                        "diff_image",
                        ImageValue::new(
                            diff.png_bytes,
                            vec![("mime_type".into(), json!("image/png"))],
                        ),
                    )
                    .push("changed_percent", num(diff.changed_percent))
                    .push("verdict", json!(diff.verdict))
                    .push("reliable", json!(diff.reliable))
                    .push(
                        "match_is_full_resolution",
                        json!(matched.is_full_resolution),
                    ))
            },
        )
    }

    fn list_actions(&self) -> HandlerResult {
        self.run_tool("list_actions", Map::new(), None, None, || {
            let settings = self.mcp_settings();
            let mut listed = Vec::new();
            for spec in &TOOLS {
                let refusal =
                    tools::gate(spec.name, &settings).map_err(|e| HandlerError::new(e.detail()))?;
                listed.push(json!({
                    "tool": spec.name,
                    "tier": spec.tier.as_str(),
                    "review_action_id": spec.review_action_id,
                    "enabled": refusal.is_none(),
                    "refusal_reason": refusal,
                }));
            }
            Self::done(vec![
                ("tools", Value::Array(listed)),
                ("dry_run", json!(settings.dry_run)),
            ])
        })
    }

    fn select_candidate(&self, row: i64, candidate_index: i64) -> HandlerResult {
        let arguments = Self::args(vec![
            ("row", json!(row)),
            ("candidate_index", json!(candidate_index)),
        ]);
        self.run_tool(
            "select_candidate",
            arguments,
            Some(row),
            Some(candidate_index),
            || {
                self.on_model(self.env.timeouts.default, move |m| {
                    let Some(id) = m.id_at(row) else {
                        return Ok(ToolOutput::new().push("error", json!(format!("No row {row}"))));
                    };
                    let count = m.entry(id).map_or(0, |e| e.candidates.len());
                    let Some(index) = usize::try_from(candidate_index).ok().filter(|i| *i < count)
                    else {
                        return Ok(ToolOutput::new().push(
                            "error",
                            json!(format!("No candidate {candidate_index} for row {row}")),
                        ));
                    };
                    m.select_candidate(id, index);
                    let selected = m.entry(id).map_or(0, |e| e.selected_candidate_index);
                    Ok(ToolOutput::new()
                        .push("row", json!(row))
                        .push("selected_candidate_index", json!(selected)))
                })
            },
        )
    }

    fn toggle_reviewed(&self, row: i64, reviewed: bool) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row)), ("reviewed", json!(reviewed))]);
        self.run_tool("toggle_reviewed", arguments, Some(row), None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let Some(id) = m.id_at(row) else {
                    return Ok(ToolOutput::new().push("error", json!(format!("No row {row}"))));
                };
                m.set_reviewed(id, reviewed);
                let now = m.entry(id).is_some_and(|e| e.reviewed);
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("reviewed", json!(now)))
            })
        })
    }

    fn research(&self, rows: Vec<i64>) -> HandlerResult {
        let arguments = Self::args(vec![("rows", json!(rows))]);
        let first = rows.first().copied();
        self.run_tool("research", arguments, first, None, || {
            self.perform_research(rows)
        })
    }

    fn send_upload(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("send_upload", arguments, Some(row), None, || {
            self.send_with_limit(row, SendKind::Upload)
        })
    }

    fn send_url(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("send_url", arguments, Some(row), None, || {
            self.send_with_limit(row, SendKind::Url)
        })
    }

    fn download_send(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("download_send", arguments, Some(row), None, || {
            self.send_with_limit(row, SendKind::Download)
        })
    }

    fn remove_row(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        self.run_tool("remove_row", arguments, Some(row), None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let Some(id) = m.id_at(row) else {
                    return Ok(ToolOutput::new().push("error", json!(format!("No row {row}"))));
                };
                let filename = m.entry(id).expect("live id").filename().to_owned();
                m.remove(&[id], RemoveReason::Requested);
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("removed", json!(filename)))
            })
        })
    }

    fn reset_result(&self, row: i64) -> HandlerResult {
        let arguments = Self::args(vec![("row", json!(row))]);
        let engine = self.env.engine.clone();
        self.run_tool("reset_result", arguments, Some(row), None, || {
            self.on_model(self.env.timeouts.default, move |m| {
                let Some(id) = m.id_at(row) else {
                    return Ok(ToolOutput::new().push("error", json!(format!("No row {row}"))));
                };
                if !has_result(m.entry(id).expect("live id")) {
                    return Ok(ToolOutput::new()
                        .push("row", json!(row))
                        .push("reset", json!(false))
                        .push("reason", json!("Nothing to reset - not searched yet")));
                }
                let hash = m.reset_result(id).flatten();
                let dropped = hash
                    .filter(|h| !h.is_empty())
                    .and_then(|h| crate::cache::CacheKey::parse(&h).ok())
                    .is_some_and(|key| engine.cache.drop_entry(&key).unwrap_or(false));
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("reset", json!(true))
                    .push("dropped_from_cache", json!(dropped)))
            })
        })
    }

    fn shutdown_invoker(&self) {
        self.channel.shutdown();
    }
}

impl McpToolHandlers {
    /// `_perform_research`.
    fn perform_research(&self, rows: Vec<i64>) -> HandlerResult {
        let channel = self.channel.clone();
        let launcher = self.env.launcher.clone();
        let wanted = rows.clone();
        let started = self.on_model(self.env.timeouts.default, move |m| {
            Ok(start_research(m, &channel, launcher.as_ref(), &wanted))
        })?;
        let (filenames, flag) = match started {
            ResearchStart::Declined(reason) => {
                return Self::done(vec![
                    ("research_started", json!(false)),
                    ("reason", json!(reason)),
                ]);
            }
            ResearchStart::Started { filenames, flag } => (filenames, flag),
        };
        let t = self.env.timeouts;
        let timeout = t
            .research_floor
            .max(filenames.len() as f64 * t.research_per_image);
        if !flag.wait(timeout) {
            return Self::done(vec![
                ("research_started", json!(true)),
                ("completed", json!(false)),
                ("filenames", json!(filenames)),
                (
                    "note",
                    json!(format!(
                        "Still running after {timeout:.0}s - check get_entry/get_candidates again shortly."
                    )),
                ),
            ]);
        }
        let results = self.on_model(self.env.timeouts.default, move |m| {
            let mut results = Vec::new();
            for row in rows {
                if let Some(entry) = m.entry_at(row) {
                    let mut item = Map::new();
                    item.insert("row".into(), json!(row));
                    item.extend(entry_detail(entry));
                    results.push(Value::Object(item));
                }
            }
            Ok(results)
        })?;
        Self::done(vec![
            ("research_started", json!(true)),
            ("completed", json!(true)),
            ("results", Value::Array(results)),
        ])
    }
}

// ---------------------------------------------------------------------------------------------
// A Hydrus send
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum SendKind {
    Upload,
    Url,
    Download,
}

/// What the model does with the sent entry once the send has ended.
enum After {
    /// `_remove_entries([entry], reason=..., keep_place=...)`.
    Remove(&'static str),
    /// `_refresh_entry_row(entry)`.
    Refresh,
    /// A skipped send: Python returns before touching the table.
    Nothing,
}

struct SendJob {
    channel: QueueChannel,
    env: Arc<HandlerEnv>,
    row: i64,
    kind: SendKind,
}

impl SendJob {
    fn on_model<T, F>(&self, f: F) -> Result<T, HandlerError>
    where
        T: Send + 'static,
        F: FnOnce(&mut QueueModel) -> Result<T, String> + Send + 'static,
    {
        self.channel
            .call(f, self.env.timeouts.hydrus_send)
            .map_err(|e| HandlerError::new(e.text()))
    }

    /// Writes the sent copy back over the row, then does what the window does next.
    fn finish(&self, id: EntryId, entry: Entry, after: After) -> Result<(), HandlerError> {
        self.on_model(move |m| {
            if let Some(slot) = m.entry_mut(id) {
                *slot = entry;
                match after {
                    After::Remove(why) => m.remove(&[id], RemoveReason::After(why)),
                    After::Refresh => m.refresh_row(id),
                    After::Nothing => {}
                }
            }
            Ok(())
        })
    }

    fn run(&self) -> HandlerResult {
        let row = self.row;
        let found = self.on_model(move |m| {
            Ok(m.id_at(row)
                .map(|id| (id, m.entry(id).expect("live id").clone())))
        })?;
        let Some((id, mut entry)) = found else {
            return Ok(ToolOutput::new().push("error", json!(format!("No row {row}"))));
        };
        let settings = self.env.settings.lock().expect("settings lock").clone();
        if settings.hydrus.access_key.is_empty() {
            return Ok(ToolOutput::new()
                .push("row", json!(row))
                .push("success", json!(false))
                .push("error", json!("No Hydrus access key configured")));
        }
        let client = (self.env.hydrus_client)(&settings.hydrus)
            .map_err(|e| HandlerError::new(e.message().to_owned()))?;
        let engine = &self.env.engine;
        let remove_after_import = || {
            self.env
                .settings
                .lock()
                .expect("settings lock")
                .remove_after_import
        };

        match self.kind {
            SendKind::Upload => {
                let result = self
                    .env
                    .runtime
                    .block_on(send_file_upload(
                        &mut entry,
                        &client,
                        Some(&settings),
                        &*engine.clock,
                    ))
                    .map_err(|e| HandlerError::new(e.message().to_owned()))?;
                let after = if result.success && remove_after_import() {
                    After::Remove("sent-to-Hydrus")
                } else {
                    After::Refresh
                };
                self.finish(id, entry, after)?;
                Ok(ToolOutput::new()
                    .push("row", json!(row))
                    .push("success", json!(result.success))
                    .push("warning", json!(result.warning))
                    .push("error", json!(result.error)))
            }
            SendKind::Url | SendKind::Download => {
                let tmp = TempDir::new("hatate-linux-mcp-")
                    .map_err(|e| HandlerError::new(e.to_string()))?;
                let sent: Result<ImportResult, HandlerError> = if matches!(self.kind, SendKind::Url)
                {
                    self.env
                        .runtime
                        .block_on(send_url_or_download(
                            &mut entry,
                            &client,
                            engine,
                            Some(&*self.env.settings),
                            tmp.path(),
                        ))
                        .map_err(|e| HandlerError::new(e.message()))
                } else {
                    self.env
                        .runtime
                        .block_on(download_and_send(
                            &mut entry,
                            &client,
                            &engine.net,
                            settings.search_timeout,
                            tmp.path(),
                            Some(&settings),
                            &*engine.clock,
                        ))
                        .map_err(|e| HandlerError::new(e.message()))
                };
                drop(tmp);
                let result = match sent {
                    Ok(r) => r,
                    Err(e) => {
                        // The entry was mutated in place before the exception left Python's call.
                        self.finish(id, entry, After::Nothing)?;
                        return Err(e);
                    }
                };
                if let Some(reason) = result.skipped_reason.as_deref().filter(|r| !r.is_empty()) {
                    self.finish(id, entry, After::Nothing)?;
                    return Ok(ToolOutput::new()
                        .push("row", json!(row))
                        .push("success", json!(false))
                        .push("skipped_reason", json!(reason)));
                }
                if matches!(self.kind, SendKind::Url) {
                    let confirmed = result.confirmed == Some(true);
                    let remove = result.success && confirmed && remove_after_import();
                    let after = if remove {
                        After::Remove("downloaded-and-sent-to-Hydrus")
                    } else {
                        After::Refresh
                    };
                    let pending = result.success && !confirmed && remove_after_import();
                    self.finish(id, entry, after)?;
                    Ok(ToolOutput::new()
                        .push("row", json!(row))
                        .push("success", json!(result.success))
                        .push("confirmed", json!(result.confirmed))
                        .push("error", json!(result.error))
                        .push("removed_from_queue", json!(remove))
                        .push("removal_pending_confirmation", json!(pending)))
                } else {
                    let after = if result.success && remove_after_import() {
                        After::Remove("downloaded-and-sent-to-Hydrus")
                    } else {
                        After::Refresh
                    };
                    self.finish(id, entry, after)?;
                    Ok(ToolOutput::new()
                        .push("row", json!(row))
                        .push("success", json!(result.success))
                        .push("error", json!(result.error)))
                }
            }
        }
    }
}

/// `tempfile.mkdtemp(prefix=...)` and the `shutil.rmtree(..., ignore_errors=True)` after it.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> std::io::Result<TempDir> {
        use std::os::unix::fs::DirBuilderExt;
        const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
        for _ in 0..10_000 {
            let mut bytes = [0u8; 8];
            if getrandom::fill(&mut bytes).is_err() {
                bytes = [7; 8];
            }
            let suffix: String = bytes
                .iter()
                .map(|b| CHARS[usize::from(*b) % CHARS.len()] as char)
                .collect();
            let path = std::env::temp_dir().join(format!("{prefix}{suffix}"));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(TempDir { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "No usable temporary directory name found",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        rmtree_ignore_errors(&self.path);
    }
}

// ---------------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------------

/// A float as JSON; Python prints `NaN`/`Infinity` where JSON has none, a case no tool reaches.
fn num(value: f64) -> Value {
    json!(value)
}

fn queue_row(index: usize, entry: &Entry) -> Value {
    json!({
        "row": index,
        "filename": entry.filename(),
        "status": entry.status.as_str(),
        "reviewed": entry.reviewed,
        "sent_to_hydrus": entry.sent_to_hydrus,
        "similarity": entry.similarity().map(num),
        "candidate_count": entry.candidates.len(),
    })
}

fn entry_detail(entry: &Entry) -> Map<String, Value> {
    let selected = entry.selected_candidate();
    let mut out = Map::new();
    out.insert("filename".into(), json!(entry.filename()));
    out.insert("path".into(), json!(entry.path));
    out.insert("status".into(), json!(entry.status.as_str()));
    out.insert("reviewed".into(), json!(entry.reviewed));
    out.insert("sent_to_hydrus".into(), json!(entry.sent_to_hydrus));
    out.insert("matched_url".into(), json!(entry.matched_url()));
    out.insert("booru_name".into(), json!(entry.booru_name()));
    out.insert("similarity".into(), json!(entry.similarity().map(num)));
    out.insert(
        "similarity_measured".into(),
        json!(entry.similarity_measured()),
    );
    out.insert("candidate_count".into(), json!(entry.candidates.len()));
    out.insert(
        "selected_candidate_index".into(),
        json!(entry.selected_candidate_index),
    );
    out.insert("local_width".into(), json!(entry.local_width));
    out.insert("local_height".into(), json!(entry.local_height));
    out.insert("match_width".into(), json!(selected.and_then(|c| c.width)));
    out.insert(
        "match_height".into(),
        json!(selected.and_then(|c| c.height)),
    );
    out.insert("upscale_verdict".into(), json!(entry.upscale_verdict));
    out.insert(
        "upscale_check_detail".into(),
        json!(entry.upscale_check_detail),
    );
    out.insert("error_message".into(), json!(entry.error_message));
    out.insert(
        "tags".into(),
        Value::Array(
            entry
                .tags
                .iter()
                .map(|t| {
                    json!({
                        "namespace": t.namespace,
                        "name": t.name,
                        "source": t.source.as_str(),
                    })
                })
                .collect(),
        ),
    );
    out
}

/// `_matches_filter`: a deliberately small filter language for `list_queue`. A bare word matches
/// the filename (case-insensitively); three keywords match the review state; the name of a
/// status matches that status.
pub fn matches_filter(entry: &Entry, text: &str) -> bool {
    let needle = casefold(strip(text));
    if needle.is_empty() {
        return true;
    }
    match needle.as_str() {
        "unreviewed" => return needs_review(entry),
        "reviewed" => return entry.reviewed,
        "sent" => return entry.sent_to_hydrus,
        _ => {}
    }
    if needle == entry.status.as_str() {
        return true;
    }
    casefold(entry.filename()).contains(&needle)
}

/// `_guess_mime`.
pub fn guess_mime(path_or_url: &str) -> &'static str {
    let before_query = path_or_url.split('?').next().unwrap_or("");
    match path_suffix(before_query).to_ascii_lowercase().as_str() {
        ".jpg" | ".jpeg" => "image/jpeg",
        ".png" => "image/png",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".bmp" => "image/bmp",
        _ => "image/jpeg",
    }
}

/// `PurePosixPath(text).suffix`: empty and `.` components are dropped, and a name that starts or
/// ends with its only dot has no suffix.
fn path_suffix(text: &str) -> &str {
    let name = text
        .rsplit('/')
        .find(|part| !part.is_empty() && *part != ".")
        .unwrap_or("");
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => &name[i..],
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_follows_the_suffix_before_any_query() {
        assert_eq!(guess_mime("/a/b.PNG"), "image/png");
        assert_eq!(guess_mime("https://x/y/z.webp?size=large"), "image/webp");
        assert_eq!(guess_mime("https://x/y/z.jpeg"), "image/jpeg");
        assert_eq!(guess_mime("https://x/y/"), "image/jpeg");
        assert_eq!(guess_mime("noext"), "image/jpeg");
        assert_eq!(
            guess_mime(".png"),
            "image/jpeg",
            "a leading dot is not a suffix"
        );
        assert_eq!(
            guess_mime("a.png/"),
            "image/png",
            "a trailing slash is not part of the name"
        );
        assert_eq!(guess_mime("a.bmp/./."), "image/bmp");
        assert_eq!(guess_mime("a.gif."), "image/jpeg");
        assert_eq!(guess_mime(""), "image/jpeg");
    }

    #[test]
    fn the_filter_language_is_the_small_one() {
        let mut e = Entry::new("/q/Cat Photo.png");
        assert!(matches_filter(&e, "  cat "));
        assert!(matches_filter(&e, "UNREVIEWED"));
        assert!(!matches_filter(&e, "reviewed"));
        assert!(matches_filter(&e, "not_searched"));
        e.reviewed = true;
        assert!(matches_filter(&e, "reviewed"));
        assert!(!matches_filter(&e, "unreviewed"));
        assert!(!matches_filter(&e, "sent"));
        assert!(matches_filter(&e, "   "), "blank text matches everything");
        assert!(matches_filter(&Entry::new("/q/Strasse.png"), "STRASSE"));
        assert!(
            matches_filter(&Entry::new("/q/STRASSE.png"), "straße"),
            "casefold, not lowercase"
        );
    }
}
