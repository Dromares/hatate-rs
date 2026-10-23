//! `JSONRPCMessage.model_validate`: which JSON-RPC message a POST body is, and, when it is none,
//! pydantic's complaint about all four candidates (`Validation error: N validation errors for
//! JSONRPCMessage ...`), which the transport sends back as the 400 body.
//!
//! The models are `mcp.types.JSONRPCRequest`, `JSONRPCNotification`, `JSONRPCResponse` and
//! `JSONRPCError`, tried as a smart-mode union: the first that validates exactly wins, and an
//! error lists every branch's errors in the branches' field order.

use serde_json::{Map, Value};

use super::pyvalidate::{FieldError, err, format_errors};

pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Option<Map<String, Value>>,
    },
    Notification,
    Response,
}

fn loc(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_owned()).collect()
}

fn missing(parts: &[&str], input: &Value) -> Box<FieldError> {
    err(&loc(parts), "Field required", "missing", input)
}

fn method_field(
    model: &str,
    map: &Map<String, Value>,
    whole: &Value,
    errors: &mut Vec<FieldError>,
) {
    match map.get("method") {
        None => errors.push(*missing(&[model, "method"], whole)),
        Some(Value::String(_)) => {}
        Some(v) => errors.push(*err(
            &loc(&[model, "method"]),
            "Input should be a valid string",
            "string_type",
            v,
        )),
    }
}

fn params_field(model: &str, map: &Map<String, Value>, errors: &mut Vec<FieldError>) {
    match map.get("params") {
        None | Some(Value::Null | Value::Object(_)) => {}
        Some(v) => errors.push(*err(
            &loc(&[model, "params"]),
            "Input should be a valid dictionary",
            "dict_type",
            v,
        )),
    }
}

fn jsonrpc_field(
    model: &str,
    map: &Map<String, Value>,
    whole: &Value,
    errors: &mut Vec<FieldError>,
) {
    match map.get("jsonrpc") {
        None => errors.push(*missing(&[model, "jsonrpc"], whole)),
        Some(Value::String(s)) if s == "2.0" => {}
        Some(v) => errors.push(*err(
            &loc(&[model, "jsonrpc"]),
            "Input should be '2.0'",
            "literal_error",
            v,
        )),
    }
}

/// `id: str | int`. Returns whether the id is exactly a `str` or an `int` (a lax coercion, such as
/// `true` or `2.0`, validates but is not an exact match, which is what keeps a `Request` from
/// beating a `Notification` on such a message).
fn id_field(
    model: &str,
    map: &Map<String, Value>,
    whole: &Value,
    errors: &mut Vec<FieldError>,
) -> bool {
    match map.get("id") {
        None => {
            errors.push(*missing(&[model, "id"], whole));
            false
        }
        Some(Value::String(_)) => true,
        Some(Value::Number(n)) if !n.is_f64() => true,
        Some(Value::Bool(_)) => false,
        Some(v @ Value::Number(n))
            if n.as_f64()
                .is_some_and(|x| x.is_finite() && x.fract() == 0.0) =>
        {
            let _ = v;
            false
        }
        Some(v) => {
            errors.push(*err(
                &loc(&[model, "id", "str"]),
                "Input should be a valid string",
                "string_type",
                v,
            ));
            let int_error = super::pyvalidate::coerce_int(v, &loc(&[model, "id", "int"])).err();
            if let Some(e) = int_error {
                errors.push(*e);
            }
            false
        }
    }
}

fn model_type(model: &str, input: &Value) -> FieldError {
    *err(
        &loc(&[model]),
        &format!("Input should be a valid dictionary or instance of {model}"),
        "model_type",
        input,
    )
}

/// Validates `raw` as a JSON-RPC message.
pub fn classify(raw: &Value) -> Result<Message, String> {
    let Value::Object(map) = raw else {
        let errors: Vec<FieldError> = [
            "JSONRPCRequest",
            "JSONRPCNotification",
            "JSONRPCResponse",
            "JSONRPCError",
        ]
        .iter()
        .map(|m| model_type(m, raw))
        .collect();
        return Err(text(&errors));
    };
    // JSONRPCRequest: method, params, jsonrpc, id.
    let mut req = Vec::new();
    method_field("JSONRPCRequest", map, raw, &mut req);
    params_field("JSONRPCRequest", map, &mut req);
    jsonrpc_field("JSONRPCRequest", map, raw, &mut req);
    let exact_id = id_field("JSONRPCRequest", map, raw, &mut req);
    // JSONRPCNotification: method, params, jsonrpc.
    let mut note = Vec::new();
    method_field("JSONRPCNotification", map, raw, &mut note);
    params_field("JSONRPCNotification", map, &mut note);
    jsonrpc_field("JSONRPCNotification", map, raw, &mut note);
    // JSONRPCResponse: jsonrpc, id, result.
    let mut resp = Vec::new();
    jsonrpc_field("JSONRPCResponse", map, raw, &mut resp);
    id_field("JSONRPCResponse", map, raw, &mut resp);
    match map.get("result") {
        None => resp.push(*missing(&["JSONRPCResponse", "result"], raw)),
        Some(Value::Object(_)) => {}
        Some(v) => resp.push(*err(
            &loc(&["JSONRPCResponse", "result"]),
            "Input should be a valid dictionary",
            "dict_type",
            v,
        )),
    }
    // JSONRPCError: jsonrpc, id, error.
    let mut error = Vec::new();
    jsonrpc_field("JSONRPCError", map, raw, &mut error);
    id_field("JSONRPCError", map, raw, &mut error);
    match map.get("error") {
        None => error.push(*missing(&["JSONRPCError", "error"], raw)),
        Some(inner @ Value::Object(e)) => {
            match e.get("code") {
                None => error.push(*missing(&["JSONRPCError", "error", "code"], inner)),
                Some(v) => {
                    if let Err(f) =
                        super::pyvalidate::coerce_int(v, &loc(&["JSONRPCError", "error", "code"]))
                    {
                        error.push(*f);
                    }
                }
            }
            match e.get("message") {
                None => error.push(*missing(&["JSONRPCError", "error", "message"], inner)),
                Some(Value::String(_)) => {}
                Some(v) => error.push(*err(
                    &loc(&["JSONRPCError", "error", "message"]),
                    "Input should be a valid string",
                    "string_type",
                    v,
                )),
            }
        }
        Some(v) => error.push(*err(
            &loc(&["JSONRPCError", "error"]),
            "Input should be a valid dictionary or instance of ErrorData",
            "model_type",
            v,
        )),
    }
    if req.is_empty() && exact_id {
        let method = map
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let params = map.get("params").and_then(Value::as_object).cloned();
        let id = map.get("id").cloned().unwrap_or(Value::Null);
        return Ok(Message::Request { id, method, params });
    }
    if note.is_empty() {
        return Ok(Message::Notification);
    }
    if resp.is_empty() || error.is_empty() {
        return Ok(Message::Response);
    }
    let all: Vec<FieldError> = req
        .into_iter()
        .chain(note)
        .chain(resp)
        .chain(error)
        .collect();
    Err(text(&all))
}

fn text(errors: &[FieldError]) -> String {
    format!(
        "Validation error: {}",
        format_errors("JSONRPCMessage", errors)
    )
}
