//! What the server calls for a tool (`McpToolHandlers`, `_HANDLER_METHOD_NAMES`, `dispatch`).
//!
//! The handlers themselves (the tool bodies, gating, dry-run and audit plumbing) are 5.e's. This
//! ticket fixes the trait they implement and tests the server over scripted handlers.

use super::content::ToolOutput;
use super::schema::ToolCall;
use super::tools::TOOLS;

/// An exception out of a handler. The text is `str(exc)`, which the server prints after
/// `Error executing tool <name>: `.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerError(pub String);

impl HandlerError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type HandlerResult = Result<ToolOutput, HandlerError>;

/// One method per tool, named as `_HANDLER_METHOD_NAMES` names them. Arguments arrive validated
/// and coerced. Methods block: the server runs each call on a blocking thread.
pub trait McpHandlers: Send + Sync + 'static {
    fn list_queue(&self, offset: i64, limit: i64, filter_text: Option<String>) -> HandlerResult;
    fn get_entry(&self, row: i64) -> HandlerResult;
    fn get_candidates(&self, row: i64) -> HandlerResult;
    fn get_images(&self, row: i64, candidate_index: i64) -> HandlerResult;
    fn get_diff(&self, row: i64, candidate_index: i64) -> HandlerResult;
    fn list_actions(&self) -> HandlerResult;
    fn select_candidate(&self, row: i64, candidate_index: i64) -> HandlerResult;
    fn toggle_reviewed(&self, row: i64, reviewed: bool) -> HandlerResult;
    fn research(&self, rows: Vec<i64>) -> HandlerResult;
    fn send_upload(&self, row: i64) -> HandlerResult;
    fn send_url(&self, row: i64) -> HandlerResult;
    fn download_send(&self, row: i64) -> HandlerResult;
    fn remove_row(&self, row: i64) -> HandlerResult;
    fn reset_result(&self, row: i64) -> HandlerResult;

    /// `handlers.invoker.shutdown()`, which `stop()` calls unless told not to. The invoker is
    /// 5.e's; a handler set with nothing to shut down leaves this empty.
    fn shutdown_invoker(&self) {}
}

/// `_HANDLER_METHOD_NAMES`: tool name to the handler method with the same behaviour. All
/// fourteen are the identity, as in Python.
pub const HANDLER_METHOD_NAMES: [(&str, &str); 14] = [
    ("list_queue", "list_queue"),
    ("get_entry", "get_entry"),
    ("get_candidates", "get_candidates"),
    ("get_images", "get_images"),
    ("get_diff", "get_diff"),
    ("list_actions", "list_actions"),
    ("select_candidate", "select_candidate"),
    ("toggle_reviewed", "toggle_reviewed"),
    ("research", "research"),
    ("send_upload", "send_upload"),
    ("send_url", "send_url"),
    ("download_send", "download_send"),
    ("remove_row", "remove_row"),
    ("reset_result", "reset_result"),
];

/// `_HANDLER_METHOD_NAMES[tool_name]`.
pub fn handler_method_name(tool_name: &str) -> Option<&'static str> {
    HANDLER_METHOD_NAMES
        .iter()
        .find(|(tool, _)| *tool == tool_name)
        .map(|(_, method)| *method)
}

/// `dispatch(handlers, tool_name, arguments)`: calls the handler method for the validated call.
pub fn dispatch(handlers: &dyn McpHandlers, call: ToolCall) -> HandlerResult {
    match call {
        ToolCall::ListQueue {
            offset,
            limit,
            filter_text,
        } => handlers.list_queue(offset, limit, filter_text),
        ToolCall::GetEntry { row } => handlers.get_entry(row),
        ToolCall::GetCandidates { row } => handlers.get_candidates(row),
        ToolCall::GetImages {
            row,
            candidate_index,
        } => handlers.get_images(row, candidate_index),
        ToolCall::GetDiff {
            row,
            candidate_index,
        } => handlers.get_diff(row, candidate_index),
        ToolCall::ListActions => handlers.list_actions(),
        ToolCall::SelectCandidate {
            row,
            candidate_index,
        } => handlers.select_candidate(row, candidate_index),
        ToolCall::ToggleReviewed { row, reviewed } => handlers.toggle_reviewed(row, reviewed),
        ToolCall::Research { rows } => handlers.research(rows),
        ToolCall::SendUpload { row } => handlers.send_upload(row),
        ToolCall::SendUrl { row } => handlers.send_url(row),
        ToolCall::DownloadSend { row } => handlers.download_send(row),
        ToolCall::RemoveRow { row } => handlers.remove_row(row),
        ToolCall::ResetResult { row } => handlers.reset_result(row),
    }
}

/// The tool names the registry and the method table must agree on.
pub fn registry_names_agree() -> bool {
    TOOLS.len() == HANDLER_METHOD_NAMES.len()
        && TOOLS.iter().all(|t| handler_method_name(t.name).is_some())
}

#[cfg(test)]
pub(crate) mod scripted {
    //! A handler set that records each call and answers from a script, for the tests in this
    //! module tree.
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    #[derive(Default)]
    pub struct Recorder {
        pub calls: Mutex<Vec<(String, serde_json::Value)>>,
        pub fail_on: Option<&'static str>,
    }

    impl Recorder {
        fn note(&self, name: &str, args: serde_json::Value) -> HandlerResult {
            self.calls.lock().unwrap().push((name.to_owned(), args));
            if self.fail_on == Some(name) {
                return Err(HandlerError::new("boom"));
            }
            Ok(ToolOutput::new().push("called", json!(name)))
        }
    }

    impl McpHandlers for Recorder {
        fn list_queue(
            &self,
            offset: i64,
            limit: i64,
            filter_text: Option<String>,
        ) -> HandlerResult {
            self.note("list_queue", json!([offset, limit, filter_text]))
        }
        fn get_entry(&self, row: i64) -> HandlerResult {
            self.note("get_entry", json!([row]))
        }
        fn get_candidates(&self, row: i64) -> HandlerResult {
            self.note("get_candidates", json!([row]))
        }
        fn get_images(&self, row: i64, c: i64) -> HandlerResult {
            self.note("get_images", json!([row, c]))
        }
        fn get_diff(&self, row: i64, c: i64) -> HandlerResult {
            self.note("get_diff", json!([row, c]))
        }
        fn list_actions(&self) -> HandlerResult {
            self.note("list_actions", json!([]))
        }
        fn select_candidate(&self, row: i64, c: i64) -> HandlerResult {
            self.note("select_candidate", json!([row, c]))
        }
        fn toggle_reviewed(&self, row: i64, r: bool) -> HandlerResult {
            self.note("toggle_reviewed", json!([row, r]))
        }
        fn research(&self, rows: Vec<i64>) -> HandlerResult {
            self.note("research", json!([rows]))
        }
        fn send_upload(&self, row: i64) -> HandlerResult {
            self.note("send_upload", json!([row]))
        }
        fn send_url(&self, row: i64) -> HandlerResult {
            self.note("send_url", json!([row]))
        }
        fn download_send(&self, row: i64) -> HandlerResult {
            self.note("download_send", json!([row]))
        }
        fn remove_row(&self, row: i64) -> HandlerResult {
            self.note("remove_row", json!([row]))
        }
        fn reset_result(&self, row: i64) -> HandlerResult {
            self.note("reset_result", json!([row]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scripted::Recorder;
    use super::*;
    use crate::mcp::schema::{params, validate};
    use serde_json::{Map, json};

    #[test]
    fn every_tool_has_a_handler_method_name_and_no_stray_entries() {
        assert!(registry_names_agree());
        for (tool, _) in HANDLER_METHOD_NAMES {
            assert!(
                crate::mcp::tools::tool(tool).is_some(),
                "{tool} is not a tool"
            );
        }
    }

    #[test]
    fn dispatch_calls_the_matching_method_with_the_validated_arguments() {
        let rec = Recorder::default();
        let args = json!({"row": "4", "candidate_index": 1})
            .as_object()
            .unwrap()
            .clone();
        let kw = validate("get_images", &args).unwrap();
        let call = ToolCall::from_kwargs("get_images", &kw).unwrap();
        dispatch(&rec, call).unwrap();
        assert_eq!(
            rec.calls.lock().unwrap().as_slice(),
            &[("get_images".to_owned(), json!([4, 1]))]
        );
    }

    #[test]
    fn every_registered_tool_dispatches_without_error() {
        let rec = Recorder::default();
        for t in &TOOLS {
            let mut args = Map::new();
            for p in params(t.name)
                .unwrap()
                .iter()
                .filter(|p| p.default.is_none())
            {
                let v = match p.name {
                    "reviewed" => json!(true),
                    "rows" => json!([1]),
                    _ => json!(0),
                };
                args.insert(p.name.to_owned(), v);
            }
            let kw = validate(t.name, &args).unwrap();
            dispatch(&rec, ToolCall::from_kwargs(t.name, &kw).unwrap()).unwrap();
        }
        assert_eq!(rec.calls.lock().unwrap().len(), TOOLS.len());
    }
}
