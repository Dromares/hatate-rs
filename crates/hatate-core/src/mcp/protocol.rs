//! FastMCP's streamable-HTTP behaviour as a function from a request to a reply: bearer check,
//! routing, the session table, the transport's header checks, JSON-RPC parsing and the
//! lowlevel server's method table (`mcp 1.30.0`: `server/streamable_http.py`,
//! `streamable_http_manager.py`, `transport_security.py`, `lowlevel/server.py` and
//! `fastmcp/server.py`).
//!
//! Nothing here touches a socket, so the whole table is unit-tested directly; `server.rs` is
//! the thin hyper layer on top.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use super::content::to_content_blocks;
use super::handlers::{McpHandlers, dispatch};
use super::schema::{ToolCall, description, input_schema, validate};
use super::tools::TOOLS;
use crate::pawchive_index::json_error;

/// What `mcp 1.30.0` reports as `serverInfo.name` for `FastMCP("hatate-linux")`.
pub const SERVER_NAME: &str = "hatate-linux";
/// `serverInfo.version`. FastMCP reports the `mcp` package's own version; this is the Rust
/// build's (a difference the ticket grants by name).
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// `SUPPORTED_PROTOCOL_VERSIONS`, oldest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
/// `LATEST_PROTOCOL_VERSION`, and what a client that asks for an unknown version is answered with.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
/// `DEFAULT_NEGOTIATED_VERSION`: assumed when a request carries no `MCP-Protocol-Version`.
const DEFAULT_NEGOTIATED_VERSION: &str = "2025-03-26";
/// `DEFAULT_MAX_REQUEST_BODY_SIZE`.
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

/// One request, already read off the socket. Header names are lower-case; a header may repeat.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path without its query.
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Starlette's `request.headers.get(name)`: the first value, decoded as latin-1.
    fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.iter().map(|b| char::from(*b)).collect())
    }
}

/// How a reply's body is produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyBody {
    Bytes(Vec<u8>),
    /// A `GET` event stream: headers now, no events, held open until the server stops.
    HeldOpen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// Lower-case names, in the order they are sent.
    pub headers: Vec<(String, String)>,
    pub body: ReplyBody,
}

impl Reply {
    fn bytes(status: u16, headers: Vec<(&str, String)>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: headers
                .into_iter()
                .map(|(n, v)| (n.to_owned(), v))
                .collect(),
            body: ReplyBody::Bytes(body.into()),
        }
    }

    /// The body as text, for tests and the differential.
    pub fn text(&self) -> String {
        match &self.body {
            ReplyBody::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            ReplyBody::HeldOpen => String::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A plain-text reply with no content type of its own (Starlette's `Response("...")`).
fn plain(status: u16, text: &str) -> Reply {
    Reply::bytes(status, vec![], text)
}

/// `Response("Not Found", 404)` from Starlette's router: `text/plain; charset=utf-8`.
fn not_found_route() -> Reply {
    Reply::bytes(
        404,
        vec![("content-type", "text/plain; charset=utf-8".to_owned())],
        "Not Found",
    )
}

fn unauthorized() -> Reply {
    Reply::bytes(
        401,
        vec![("content-type", "text/plain".to_owned())],
        "Unauthorized",
    )
}

fn jsonrpc_error_body(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = Map::new();
    error.insert("code".into(), json!(code));
    error.insert("message".into(), json!(message));
    if let Some(data) = data {
        error.insert("data".into(), data);
    }
    json!({"jsonrpc": "2.0", "id": id, "error": Value::Object(error)})
}

/// `_error_response` of the session manager: a JSON-RPC error with no session header.
fn manager_error(status: u16, message: &str, code: i64) -> Reply {
    Reply::bytes(
        status,
        vec![("content-type", "application/json".to_owned())],
        jsonrpc_error_body(&json!("server-error"), code, message, None).to_string(),
    )
}

/// What the lowlevel server answers a request with.
enum Outcome {
    Result(Value),
    Error {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

impl Outcome {
    fn invalid_params() -> Self {
        Outcome::Error {
            code: INVALID_PARAMS,
            message: "Invalid request parameters".into(),
            data: Some(json!("")),
        }
    }

    fn method_not_found() -> Self {
        Outcome::Error {
            code: METHOD_NOT_FOUND,
            message: "Method not found".into(),
            data: None,
        }
    }

    /// An exception out of a registered handler: code 0 and `str(exc)`.
    fn exception(message: String) -> Self {
        Outcome::Error {
            code: 0,
            message,
            data: None,
        }
    }
}

/// The session table and everything a request needs. Cheap to clone behind an `Arc`.
pub struct Core {
    pub handlers: Arc<dyn McpHandlers>,
    expected_authorization: Vec<u8>,
    sessions: Mutex<HashMap<String, Session>>,
}

#[derive(Default)]
struct Session {
    /// Whether a `GET` stream is open (`GET_STREAM_KEY` in `_request_streams`).
    get_stream: bool,
}

/// The session a request is served by.
enum Transport {
    Existing(String),
    /// A transport for a request with no `Mcp-Session-Id`: it has an id (its errors carry it)
    /// but is only kept if the request is an `initialize`.
    Fresh(String),
}

impl Transport {
    fn id(&self) -> &str {
        match self {
            Transport::Existing(id) | Transport::Fresh(id) => id,
        }
    }
}

/// `uuid4().hex`.
fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // A system with no entropy source cannot serve; a time-derived id keeps the server up.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes = nanos.to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Core {
    pub fn new(handlers: Arc<dyn McpHandlers>, token: &str) -> Self {
        // `f"Bearer {token}".encode("latin-1")`: a character outside latin-1 cannot be encoded,
        // which Python raises on at construction; here it can never match instead.
        let expected_authorization = format!("Bearer {token}")
            .chars()
            .map(|c| u8::try_from(u32::from(c)).ok())
            .collect::<Option<Vec<u8>>>()
            .unwrap_or_default();
        Self {
            handlers,
            expected_authorization,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Open sessions, for tests.
    pub fn session_count(&self) -> usize {
        self.sessions.lock().map_or(0, |s| s.len())
    }

    /// `_with_bearer_auth` and then the Starlette app: one HTTP request in, one reply out.
    pub async fn handle(self: &Arc<Self>, req: Request) -> Reply {
        // `dict(scope["headers"])`: with a repeated header the last value wins.
        let presented = req
            .headers
            .iter()
            .rev()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.as_slice());
        if presented != Some(self.expected_authorization.as_slice())
            || self.expected_authorization.is_empty()
        {
            return unauthorized();
        }
        if req.path == "/mcp/" {
            // Starlette's `redirect_slashes`.
            let host = req.header("host").unwrap_or_default();
            let query = req
                .query
                .as_ref()
                .map(|q| format!("?{q}"))
                .unwrap_or_default();
            return Reply::bytes(
                307,
                vec![
                    ("location", format!("http://{host}/mcp{query}")),
                    ("content-length", "0".to_owned()),
                ],
                Vec::new(),
            );
        }
        if req.path != "/mcp" {
            return not_found_route();
        }
        self.manager(req).await
    }

    /// `StreamableHTTPSessionManager._handle_stateful_request`.
    async fn manager(self: &Arc<Self>, req: Request) -> Reply {
        let session_header = req.header("mcp-session-id");
        let transport = match session_header {
            Some(id) => {
                let known = self.sessions.lock().is_ok_and(|s| s.contains_key(&id));
                if !known {
                    return manager_error(404, "Session not found", INVALID_REQUEST);
                }
                Transport::Existing(id)
            }
            None => Transport::Fresh(new_session_id()),
        };
        let (reply, keep) = self.transport(&transport, &req).await;
        if keep && let Transport::Fresh(id) = &transport {
            if let Ok(mut sessions) = self.sessions.lock() {
                sessions.insert(id.clone(), Session::default());
            }
        }
        if let Transport::Existing(id) = &transport {
            // `transport.is_terminated`: a DELETE ends the session.
            if req.method == "DELETE" && reply.status == 200 {
                if let Ok(mut sessions) = self.sessions.lock() {
                    sessions.remove(id);
                }
            }
        }
        reply
    }

    /// A transport-made error: JSON-RPC body, session header.
    fn error(&self, t: &Transport, status: u16, message: &str, code: i64) -> Reply {
        Reply::bytes(
            status,
            vec![
                ("content-type", "application/json".to_owned()),
                ("mcp-session-id", t.id().to_owned()),
            ],
            jsonrpc_error_body(&json!("server-error"), code, message, None).to_string(),
        )
    }

    /// `StreamableHTTPServerTransport._handle_request`. The flag says whether a fresh transport
    /// should be kept as a session.
    async fn transport(self: &Arc<Self>, t: &Transport, req: &Request) -> (Reply, bool) {
        let is_post = req.method == "POST";
        if let Some(reply) = security(req, is_post) {
            return (reply, false);
        }
        match req.method.as_str() {
            "POST" => self.post(t, req).await,
            "GET" => (self.get(t, req), false),
            "DELETE" => (self.delete(t, req), false),
            _ => {
                let mut reply = self.error(t, 405, "Method Not Allowed", INVALID_REQUEST);
                reply
                    .headers
                    .push(("allow".to_owned(), "GET, POST, DELETE".to_owned()));
                (reply, false)
            }
        }
    }

    fn accept_flags(req: &Request) -> (bool, bool) {
        let accept = req.header("accept").unwrap_or_default();
        let types: Vec<&str> = accept.split(',').map(str::trim).collect();
        (
            types.iter().any(|m| m.starts_with("application/json")),
            types.iter().any(|m| m.starts_with("text/event-stream")),
        )
    }

    /// `_validate_session` and `_validate_protocol_version`.
    fn validate_headers(&self, t: &Transport, req: &Request) -> Option<Reply> {
        match req.header("mcp-session-id") {
            None => {
                return Some(self.error(
                    t,
                    400,
                    "Bad Request: Missing session ID",
                    INVALID_REQUEST,
                ));
            }
            Some(id) if id.is_empty() => {
                return Some(self.error(
                    t,
                    400,
                    "Bad Request: Missing session ID",
                    INVALID_REQUEST,
                ));
            }
            Some(id) if id != t.id() => {
                return Some(self.error(
                    t,
                    404,
                    "Not Found: Invalid or expired session ID",
                    INVALID_REQUEST,
                ));
            }
            Some(_) => {}
        }
        let version = req.header("mcp-protocol-version");
        let version = version.as_deref().unwrap_or(DEFAULT_NEGOTIATED_VERSION);
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
            return Some(self.error(
                t,
                400,
                &format!(
                    "Bad Request: Unsupported protocol version: {version}. Supported versions: {}",
                    SUPPORTED_PROTOCOL_VERSIONS.join(", ")
                ),
                INVALID_REQUEST,
            ));
        }
        None
    }

    fn get(&self, t: &Transport, req: &Request) -> Reply {
        let (_, has_sse) = Self::accept_flags(req);
        if !has_sse {
            return self.error(
                t,
                406,
                "Not Acceptable: Client must accept text/event-stream",
                INVALID_REQUEST,
            );
        }
        if let Some(reply) = self.validate_headers(t, req) {
            return reply;
        }
        let Transport::Existing(id) = t else {
            return self.error(t, 400, "Bad Request: Missing session ID", INVALID_REQUEST);
        };
        let conflict = self.sessions.lock().is_ok_and(|mut s| match s.get_mut(id) {
            Some(session) => std::mem::replace(&mut session.get_stream, true),
            None => false,
        });
        if conflict {
            return self.error(
                t,
                409,
                "Conflict: Only one SSE stream is allowed per session",
                INVALID_REQUEST,
            );
        }
        Reply {
            status: 200,
            headers: vec![
                ("cache-control".into(), "no-cache, no-transform".into()),
                ("connection".into(), "keep-alive".into()),
                ("content-type".into(), "text/event-stream".into()),
                ("mcp-session-id".into(), id.clone()),
                ("x-accel-buffering".into(), "no".into()),
            ],
            body: ReplyBody::HeldOpen,
        }
    }

    fn delete(&self, t: &Transport, req: &Request) -> Reply {
        if let Some(reply) = self.validate_headers(t, req) {
            return reply;
        }
        Reply::bytes(
            200,
            vec![
                ("content-type", "application/json".to_owned()),
                ("mcp-session-id", t.id().to_owned()),
            ],
            Vec::new(),
        )
    }

    async fn post(self: &Arc<Self>, t: &Transport, req: &Request) -> (Reply, bool) {
        let (has_json, has_sse) = Self::accept_flags(req);
        if !(has_json && has_sse) {
            return (
                self.error(
                    t,
                    406,
                    "Not Acceptable: Client must accept both application/json and text/event-stream",
                    INVALID_REQUEST,
                ),
                false,
            );
        }
        let content_type = req.header("content-type").unwrap_or_default();
        let json_type = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .split(',')
            .any(|p| p.trim() == "application/json");
        if !json_type {
            return (
                self.error(
                    t,
                    415,
                    "Unsupported Media Type: Content-Type must be application/json",
                    INVALID_REQUEST,
                ),
                false,
            );
        }
        let Ok(text) = std::str::from_utf8(&req.body) else {
            // `json.loads(bytes)` raises UnicodeDecodeError, which is not a JSONDecodeError: it
            // reaches the transport's catch-all.
            return (
                self.error(t, 500, "Error handling POST request", INTERNAL_ERROR),
                false,
            );
        };
        let raw: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                let message = json_error::message_for(text).unwrap_or_else(|| e.to_string());
                return (
                    self.error(t, 400, &format!("Parse error: {message}"), PARSE_ERROR),
                    false,
                );
            }
        };
        let message = match classify(&raw) {
            Ok(m) => m,
            Err(()) => {
                return (
                    self.error(
                        t,
                        400,
                        "Validation error: invalid JSON-RPC message",
                        INVALID_PARAMS,
                    ),
                    false,
                );
            }
        };
        let is_initialize =
            matches!(&message, Message::Request { method, .. } if method == "initialize");
        if !is_initialize {
            if let Some(reply) = self.validate_headers(t, req) {
                return (reply, false);
            }
        } else if let Some(id) = req.header("mcp-session-id") {
            if matches!(t, Transport::Existing(_)) && id != t.id() {
                return (
                    self.error(
                        t,
                        404,
                        "Not Found: Invalid or expired session ID",
                        INVALID_REQUEST,
                    ),
                    false,
                );
            }
        }
        let Message::Request { id, method, params } = message else {
            return (
                Reply::bytes(
                    202,
                    vec![
                        ("content-type", "application/json".to_owned()),
                        ("mcp-session-id", t.id().to_owned()),
                    ],
                    Vec::new(),
                ),
                false,
            );
        };
        let outcome = self.lowlevel(&method, params).await;
        let body = match outcome {
            Outcome::Result(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Outcome::Error {
                code,
                message,
                data,
            } => jsonrpc_error_body(&id, code, &message, data),
        };
        let event = format!("event: message\r\ndata: {body}\r\n\r\n");
        (
            Reply::bytes(
                200,
                vec![
                    ("cache-control", "no-cache, no-transform".to_owned()),
                    ("connection", "keep-alive".to_owned()),
                    ("content-type", "text/event-stream".to_owned()),
                    ("mcp-session-id", t.id().to_owned()),
                    ("x-accel-buffering", "no".to_owned()),
                ],
                event,
            ),
            is_initialize,
        )
    }

    /// The lowlevel server plus FastMCP's registered handlers, for one validated request.
    async fn lowlevel(
        self: &Arc<Self>,
        method: &str,
        params: Option<Map<String, Value>>,
    ) -> Outcome {
        let params_ref = params.as_ref();
        match method {
            "ping" => Outcome::Result(json!({})),
            "initialize" => initialize(params_ref),
            "tools/list" => {
                if !optional_str(params_ref, "cursor") {
                    return Outcome::invalid_params();
                }
                let tools: Vec<Value> = TOOLS
                    .iter()
                    .map(|spec| {
                        json!({
                            "name": spec.name,
                            "description": description(spec),
                            "inputSchema": input_schema(spec.name),
                        })
                    })
                    .collect();
                Outcome::Result(json!({"tools": tools}))
            }
            "tools/call" => self.call_tool(params_ref).await,
            "prompts/list" if optional_str(params_ref, "cursor") => {
                Outcome::Result(json!({"prompts": []}))
            }
            "resources/list" if optional_str(params_ref, "cursor") => {
                Outcome::Result(json!({"resources": []}))
            }
            "resources/templates/list" if optional_str(params_ref, "cursor") => {
                Outcome::Result(json!({"resourceTemplates": []}))
            }
            "prompts/list" | "resources/list" | "resources/templates/list" => {
                Outcome::invalid_params()
            }
            "prompts/get" => match required_str(params_ref, "name") {
                Some(name) => Outcome::exception(format!("Unknown prompt: {name}")),
                None => Outcome::invalid_params(),
            },
            "resources/read" => match required_str(params_ref, "uri") {
                Some(uri) => Outcome::exception(format!("Unknown resource: {uri}")),
                None => Outcome::invalid_params(),
            },
            // Valid client requests FastMCP registers no handler for.
            "logging/setLevel" => requires(params_ref, &["level"]),
            "resources/subscribe" | "resources/unsubscribe" => requires(params_ref, &["uri"]),
            "completion/complete" => requires(params_ref, &["ref", "argument"]),
            "tasks/get" | "tasks/result" | "tasks/cancel" => requires(params_ref, &["taskId"]),
            "tasks/list" => Outcome::method_not_found(),
            _ => Outcome::invalid_params(),
        }
    }

    async fn call_tool(self: &Arc<Self>, params: Option<&Map<String, Value>>) -> Outcome {
        let Some(name) = required_str(params, "name") else {
            return Outcome::invalid_params();
        };
        let arguments = match params.and_then(|p| p.get("arguments")) {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Outcome::invalid_params(),
        };
        let text_error = |text: String| {
            Outcome::Result(json!({"content": [{"type": "text", "text": text}], "isError": true}))
        };
        if super::tools::tool(&name).is_none() {
            return text_error(format!("Unknown tool: {name}"));
        }
        let kwargs = match validate(&name, &arguments) {
            Ok(kw) => kw,
            Err(message) => return text_error(format!("Error executing tool {name}: {message}")),
        };
        let Some(call) = ToolCall::from_kwargs(&name, &kwargs) else {
            return text_error(format!(
                "Error executing tool {name}: argument out of range"
            ));
        };
        let handlers = Arc::clone(&self.handlers);
        let joined = tokio::task::spawn_blocking(move || dispatch(handlers.as_ref(), call)).await;
        match joined {
            Ok(Ok(output)) => {
                Outcome::Result(json!({"content": to_content_blocks(&output), "isError": false}))
            }
            Ok(Err(error)) => text_error(format!("Error executing tool {name}: {error}")),
            Err(_) => text_error(format!("Error executing tool {name}: the handler panicked")),
        }
    }
}

fn initialize(params: Option<&Map<String, Value>>) -> Outcome {
    let Some(p) = params else {
        return Outcome::invalid_params();
    };
    let version = p.get("protocolVersion").and_then(Value::as_str);
    let client_ok = p.get("capabilities").is_some_and(Value::is_object)
        && p.get("clientInfo")
            .and_then(Value::as_object)
            .is_some_and(|c| {
                c.get("name").is_some_and(Value::is_string)
                    && c.get("version").is_some_and(Value::is_string)
            });
    let (Some(version), true) = (version, client_ok) else {
        return Outcome::invalid_params();
    };
    let negotiated = if SUPPORTED_PROTOCOL_VERSIONS.contains(&version) {
        version
    } else {
        LATEST_PROTOCOL_VERSION
    };
    Outcome::Result(json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "experimental": {},
            "prompts": {"listChanged": false},
            "resources": {"subscribe": false, "listChanged": false},
            "tools": {"listChanged": false},
        },
        "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
    }))
}

/// A param that may be absent or `null` but is a string when present.
fn optional_str(params: Option<&Map<String, Value>>, key: &str) -> bool {
    params
        .and_then(|p| p.get(key))
        .is_none_or(|v| v.is_null() || v.is_string())
}

fn required_str(params: Option<&Map<String, Value>>, key: &str) -> Option<String> {
    params?.get(key)?.as_str().map(str::to_owned)
}

/// A valid-looking request for a method with no registered handler: `Method not found` when its
/// required params are there, `Invalid request parameters` when not.
fn requires(params: Option<&Map<String, Value>>, keys: &[&str]) -> Outcome {
    match params {
        Some(p) if keys.iter().all(|k| p.get(*k).is_some_and(|v| !v.is_null())) => {
            Outcome::method_not_found()
        }
        _ => Outcome::invalid_params(),
    }
}

/// A JSON-RPC message: a request (has a `method` and an `id`), a notification (a `method` and
/// no usable `id`), or a response or error from the client.
enum Message {
    Request {
        id: Value,
        method: String,
        params: Option<Map<String, Value>>,
    },
    Notification,
    Response,
}

/// `JSONRPCMessage.model_validate`, for the shapes a client sends. `Err` for a shape pydantic
/// rejects (the text of its complaint is not reproduced; see `docs/mcp-server.md`).
fn classify(raw: &Value) -> Result<Message, ()> {
    let Value::Object(map) = raw else {
        return Err(());
    };
    if map.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(());
    }
    if let Some(method) = map.get("method").and_then(Value::as_str) {
        let params = match map.get("params") {
            None | Some(Value::Null) => None,
            Some(Value::Object(m)) => Some(m.clone()),
            Some(_) => return Err(()),
        };
        let id = map
            .get("id")
            .filter(|id| id.is_string() || (id.is_number() && !id.is_f64()));
        return Ok(match id {
            Some(id) => Message::Request {
                id: id.clone(),
                method: method.to_owned(),
                params,
            },
            None => Message::Notification,
        });
    }
    if map.contains_key("result") || map.contains_key("error") {
        return Ok(Message::Response);
    }
    Err(())
}

/// `TransportSecurityMiddleware.validate_request` with the settings FastMCP turns on for a
/// loopback host: `Host` and `Origin` must be loopback names, and a `POST` must say JSON.
fn security(req: &Request, is_post: bool) -> Option<Reply> {
    if is_post {
        let ct = req.header("content-type");
        if !ct.is_some_and(|c| c.to_lowercase().starts_with("application/json")) {
            return Some(plain(400, "Invalid Content-Type header"));
        }
    }
    // `allowed_hosts` are `127.0.0.1:*`-style patterns: the part before `:*` plus a colon must
    // prefix the value, so a bare `127.0.0.1` with no port does not match.
    let allowed =
        |value: &str, bases: &[&str]| bases.iter().any(|b| value.starts_with(&format!("{b}:")));
    let host = req.header("host");
    if !host
        .as_deref()
        .is_some_and(|h| !h.is_empty() && allowed(h, &["127.0.0.1", "localhost", "[::1]"]))
    {
        return Some(plain(421, "Invalid Host header"));
    }
    if let Some(origin) = req.header("origin") {
        if !origin.is_empty()
            && !allowed(
                &origin,
                &["http://127.0.0.1", "http://localhost", "http://[::1]"],
            )
        {
            return Some(plain(403, "Invalid Origin header"));
        }
    }
    None
}
