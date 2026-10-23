//! Each tool's arguments: the `inputSchema` `tools/list` prints and the validation a call goes
//! through. FastMCP derives both from the handler's Python signature; here they are a table, one
//! row per `McpToolHandlers` method at `76c85c4`, because Rust has no signature to read.

use serde_json::{Map, Value, json};

use super::pyvalidate::{
    Annotation, FieldError, coerce_bool, coerce_int, coerce_list_of_int, coerce_optional_str,
    format_errors, pre_parse_json,
};

/// One parameter of a handler method.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    /// pydantic's `title`: the field name, underscores to spaces, each word capitalised.
    pub title: &'static str,
    pub annotation: Annotation,
    /// `None` for a required parameter.
    pub default: Option<Default>,
}

#[derive(Debug, Clone, Copy)]
pub enum Default {
    Int(i64),
    Null,
}

const fn required(name: &'static str, title: &'static str, annotation: Annotation) -> Param {
    Param {
        name,
        title,
        annotation,
        default: None,
    }
}

const ROW: Param = required("row", "Row", Annotation::Int);
const CANDIDATE: Param = required("candidate_index", "Candidate Index", Annotation::Int);

const LIST_QUEUE: [Param; 3] = [
    Param {
        name: "offset",
        title: "Offset",
        annotation: Annotation::Int,
        default: Some(Default::Int(0)),
    },
    Param {
        name: "limit",
        title: "Limit",
        annotation: Annotation::Int,
        default: Some(Default::Int(50)),
    },
    Param {
        name: "filter_text",
        title: "Filter Text",
        annotation: Annotation::OptionalStr,
        default: Some(Default::Null),
    },
];
const ROW_ONLY: [Param; 1] = [ROW];
const ROW_AND_CANDIDATE: [Param; 2] = [ROW, CANDIDATE];
const TOGGLE: [Param; 2] = [ROW, required("reviewed", "Reviewed", Annotation::Bool)];
const RESEARCH: [Param; 1] = [required("rows", "Rows", Annotation::ListOfInt)];
const NONE: [Param; 0] = [];

/// The parameters of a tool's handler method, in signature order. `None` for an unknown tool.
pub fn params(tool: &str) -> Option<&'static [Param]> {
    Some(match tool {
        "list_queue" => &LIST_QUEUE,
        "get_entry" | "get_candidates" | "send_upload" | "send_url" | "download_send"
        | "remove_row" | "reset_result" => &ROW_ONLY,
        "get_images" | "get_diff" | "select_candidate" => &ROW_AND_CANDIDATE,
        "list_actions" => &NONE,
        "toggle_reviewed" => &TOGGLE,
        "research" => &RESEARCH,
        _ => return None,
    })
}

fn property(p: &Param) -> Value {
    // pydantic prints a property's keys in alphabetical order.
    let mut m = Map::new();
    match p.annotation {
        Annotation::OptionalStr => {
            m.insert(
                "anyOf".into(),
                json!([{"type": "string"}, {"type": "null"}]),
            );
        }
        Annotation::ListOfInt => {
            m.insert("items".into(), json!({"type": "integer"}));
        }
        Annotation::Int | Annotation::Bool => {}
    }
    match p.default {
        Some(Default::Int(n)) => {
            m.insert("default".into(), json!(n));
        }
        Some(Default::Null) => {
            m.insert("default".into(), Value::Null);
        }
        None => {}
    }
    m.insert("title".into(), json!(p.title));
    match p.annotation {
        Annotation::Int => {
            m.insert("type".into(), json!("integer"));
        }
        Annotation::Bool => {
            m.insert("type".into(), json!("boolean"));
        }
        Annotation::ListOfInt => {
            m.insert("type".into(), json!("array"));
        }
        Annotation::OptionalStr => {}
    }
    Value::Object(m)
}

/// The `inputSchema` of a tool, key for key as pydantic prints it (the `title` keys are the
/// granted difference in the ticket, but they are reproduced, so the comparison is exact).
pub fn input_schema(tool: &str) -> Option<Value> {
    let params = params(tool)?;
    let mut properties = Map::new();
    for p in params {
        properties.insert(p.name.to_owned(), property(p));
    }
    let mut schema = Map::new();
    schema.insert("properties".into(), Value::Object(properties));
    let required: Vec<Value> = params
        .iter()
        .filter(|p| p.default.is_none())
        .map(|p| json!(p.name))
        .collect();
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    schema.insert("title".into(), json!(format!("{tool}Arguments")));
    schema.insert("type".into(), json!("object"));
    Some(Value::Object(schema))
}

/// `tools/list`'s description of a tool: the name with spaces, plus the setting that turns it on.
pub fn description(spec: &super::tools::ToolSpec) -> String {
    let mut text = spec.name.replace('_', " ");
    if let Some(field) = spec.gate_field {
        text.push_str(&format!(
            " - disabled until mcp.{} is turned on",
            field.name()
        ));
    }
    text
}

/// `call_fn_with_arg_validation`'s validation step: pre-parse, validate, and keep the declared
/// parameters (an unknown key is ignored, as the pydantic model ignores extras). The `Err` is
/// the text after `Error executing tool <name>: `.
pub fn validate(tool: &str, raw: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let params = params(tool).ok_or_else(|| format!("Unknown tool: {tool}"))?;
    let names: Vec<&str> = params.iter().map(|p| p.name).collect();
    let data = pre_parse_json(&names, raw);
    let mut errors: Vec<FieldError> = Vec::new();
    let mut out = Map::new();
    for p in params {
        let loc = vec![p.name.to_owned()];
        let Some(value) = data.get(p.name) else {
            match p.default {
                Some(Default::Int(n)) => {
                    out.insert(p.name.to_owned(), json!(n));
                }
                Some(Default::Null) => {
                    out.insert(p.name.to_owned(), Value::Null);
                }
                None => errors.push(FieldError {
                    loc,
                    msg: "Field required",
                    kind: "missing",
                    input: Value::Object(data.clone()),
                }),
            }
            continue;
        };
        let coerced = match p.annotation {
            Annotation::Int => coerce_int(value, &loc),
            Annotation::Bool => coerce_bool(value, &loc),
            Annotation::OptionalStr => coerce_optional_str(value, &loc),
            Annotation::ListOfInt => match coerce_list_of_int(value, &loc, &mut errors) {
                Some(v) => Ok(v),
                None => continue,
            },
        };
        match coerced {
            Ok(v) => {
                out.insert(p.name.to_owned(), v);
            }
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(format_errors(&format!("{tool}Arguments"), &errors))
    }
}

/// A validated call, one variant per tool.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolCall {
    ListQueue {
        offset: i64,
        limit: i64,
        filter_text: Option<String>,
    },
    GetEntry {
        row: i64,
    },
    GetCandidates {
        row: i64,
    },
    GetImages {
        row: i64,
        candidate_index: i64,
    },
    GetDiff {
        row: i64,
        candidate_index: i64,
    },
    ListActions,
    SelectCandidate {
        row: i64,
        candidate_index: i64,
    },
    ToggleReviewed {
        row: i64,
        reviewed: bool,
    },
    Research {
        rows: Vec<i64>,
    },
    SendUpload {
        row: i64,
    },
    SendUrl {
        row: i64,
    },
    DownloadSend {
        row: i64,
    },
    RemoveRow {
        row: i64,
    },
    ResetResult {
        row: i64,
    },
}

impl ToolCall {
    /// The registry name of the tool this call is for.
    pub fn tool_name(&self) -> &'static str {
        match self {
            ToolCall::ListQueue { .. } => "list_queue",
            ToolCall::GetEntry { .. } => "get_entry",
            ToolCall::GetCandidates { .. } => "get_candidates",
            ToolCall::GetImages { .. } => "get_images",
            ToolCall::GetDiff { .. } => "get_diff",
            ToolCall::ListActions => "list_actions",
            ToolCall::SelectCandidate { .. } => "select_candidate",
            ToolCall::ToggleReviewed { .. } => "toggle_reviewed",
            ToolCall::Research { .. } => "research",
            ToolCall::SendUpload { .. } => "send_upload",
            ToolCall::SendUrl { .. } => "send_url",
            ToolCall::DownloadSend { .. } => "download_send",
            ToolCall::RemoveRow { .. } => "remove_row",
            ToolCall::ResetResult { .. } => "reset_result",
        }
    }

    /// The keyword arguments the handler method is called with.
    pub fn kwargs(&self) -> Map<String, Value> {
        let mut m = Map::new();
        match self {
            ToolCall::ListQueue {
                offset,
                limit,
                filter_text,
            } => {
                m.insert("offset".into(), json!(offset));
                m.insert("limit".into(), json!(limit));
                m.insert("filter_text".into(), json!(filter_text));
            }
            ToolCall::GetImages {
                row,
                candidate_index,
            }
            | ToolCall::GetDiff {
                row,
                candidate_index,
            }
            | ToolCall::SelectCandidate {
                row,
                candidate_index,
            } => {
                m.insert("row".into(), json!(row));
                m.insert("candidate_index".into(), json!(candidate_index));
            }
            ToolCall::ToggleReviewed { row, reviewed } => {
                m.insert("row".into(), json!(row));
                m.insert("reviewed".into(), json!(reviewed));
            }
            ToolCall::Research { rows } => {
                m.insert("rows".into(), json!(rows));
            }
            ToolCall::ListActions => {}
            ToolCall::GetEntry { row }
            | ToolCall::GetCandidates { row }
            | ToolCall::SendUpload { row }
            | ToolCall::SendUrl { row }
            | ToolCall::DownloadSend { row }
            | ToolCall::RemoveRow { row }
            | ToolCall::ResetResult { row } => {
                m.insert("row".into(), json!(row));
            }
        }
        m
    }

    /// Builds the call from the keyword arguments [`validate`] returned. `None` for an unknown
    /// tool, or for a value that does not fit the Rust type (an integer beyond 64 bits, which
    /// Python's unbounded `int` would pass on).
    pub fn from_kwargs(tool: &str, kw: &Map<String, Value>) -> Option<ToolCall> {
        let int = |name: &str| kw.get(name).and_then(Value::as_i64);
        Some(match tool {
            "list_queue" => ToolCall::ListQueue {
                offset: int("offset")?,
                limit: int("limit")?,
                filter_text: kw
                    .get("filter_text")
                    .and_then(|v| v.as_str().map(str::to_owned)),
            },
            "get_entry" => ToolCall::GetEntry { row: int("row")? },
            "get_candidates" => ToolCall::GetCandidates { row: int("row")? },
            "get_images" => ToolCall::GetImages {
                row: int("row")?,
                candidate_index: int("candidate_index")?,
            },
            "get_diff" => ToolCall::GetDiff {
                row: int("row")?,
                candidate_index: int("candidate_index")?,
            },
            "list_actions" => ToolCall::ListActions,
            "select_candidate" => ToolCall::SelectCandidate {
                row: int("row")?,
                candidate_index: int("candidate_index")?,
            },
            "toggle_reviewed" => ToolCall::ToggleReviewed {
                row: int("row")?,
                reviewed: kw.get("reviewed")?.as_bool()?,
            },
            "research" => ToolCall::Research {
                rows: kw
                    .get("rows")?
                    .as_array()?
                    .iter()
                    .map(Value::as_i64)
                    .collect::<Option<Vec<_>>>()?,
            },
            "send_upload" => ToolCall::SendUpload { row: int("row")? },
            "send_url" => ToolCall::SendUrl { row: int("row")? },
            "download_send" => ToolCall::DownloadSend { row: int("row")? },
            "remove_row" => ToolCall::RemoveRow { row: int("row")? },
            "reset_result" => ToolCall::ResetResult { row: int("row")? },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tools::TOOLS;

    #[test]
    fn every_registered_tool_has_a_schema_and_a_call_variant() {
        for t in &TOOLS {
            let schema = input_schema(t.name).unwrap();
            assert_eq!(schema["title"], format!("{}Arguments", t.name));
            let kw = validate(t.name, &sample_args(t.name)).unwrap();
            let call = ToolCall::from_kwargs(t.name, &kw).unwrap();
            assert_eq!(call.tool_name(), t.name);
            assert_eq!(call.kwargs(), kw, "{}", t.name);
        }
        assert!(input_schema("nope").is_none());
    }

    fn sample_args(tool: &str) -> Map<String, Value> {
        let all = json!({"row": 1, "candidate_index": 2, "reviewed": true, "rows": [1, 2]});
        let mut m = Map::new();
        for p in params(tool).unwrap() {
            if p.default.is_none() {
                m.insert(p.name.to_owned(), all[p.name].clone());
            }
        }
        m
    }

    #[test]
    fn list_queue_and_research_schemas_print_as_pydantic_does() {
        assert_eq!(
            input_schema("list_queue").unwrap().to_string(),
            r#"{"properties":{"offset":{"default":0,"title":"Offset","type":"integer"},"limit":{"default":50,"title":"Limit","type":"integer"},"filter_text":{"anyOf":[{"type":"string"},{"type":"null"}],"default":null,"title":"Filter Text"}},"title":"list_queueArguments","type":"object"}"#
        );
        assert_eq!(
            input_schema("research").unwrap().to_string(),
            r#"{"properties":{"rows":{"items":{"type":"integer"},"title":"Rows","type":"array"}},"required":["rows"],"title":"researchArguments","type":"object"}"#
        );
    }

    #[test]
    fn missing_and_bad_arguments_read_like_pydantics() {
        let e = validate("get_entry", &Map::new()).unwrap_err();
        assert_eq!(
            e,
            "1 validation error for get_entryArguments\nrow\n  Field required [type=missing, input_value={}, \
             input_type=dict]\n    For further information visit https://errors.pydantic.dev/2.13/v/missing"
        );
        let e = validate(
            "research",
            json!({"rows": [1, "a", 2.5]}).as_object().unwrap(),
        )
        .unwrap_err();
        assert!(
            e.starts_with("2 validation errors for researchArguments\nrows.1\n"),
            "{e}"
        );
        assert!(e.contains("\nrows.2\n"));
        // Defaults fill in, extras are dropped, strings are coerced.
        let ok = validate(
            "list_queue",
            json!({"offset": "3", "zz": 1}).as_object().unwrap(),
        )
        .unwrap();
        assert_eq!(
            ok,
            *json!({"offset": 3, "limit": 50, "filter_text": null})
                .as_object()
                .unwrap()
        );
    }

    #[test]
    fn descriptions_name_the_gate() {
        use crate::mcp::tools::tool;
        assert_eq!(description(tool("get_entry").unwrap()), "get entry");
        assert_eq!(
            description(tool("research").unwrap()),
            "research - disabled until mcp.allow_research is turned on"
        );
    }
}
