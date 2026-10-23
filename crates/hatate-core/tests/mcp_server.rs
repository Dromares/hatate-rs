//! The MCP server over a real loopback socket, plain http (the scheme it ships on), through the
//! controller's own constructor, with scripted handlers. Every credential is a placeholder.
//!
//! The expected texts are what `mcp 1.30.0`'s FastMCP answers (captured from the Python app at
//! `76c85c4`; the differential in the PR re-runs them against the live Python server).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hatate_core::config::McpSettings;
use hatate_core::error::ErrorKind;
use hatate_core::mcp::content::{ImageValue, ToolOutput};
use hatate_core::mcp::handlers::{HandlerError, HandlerResult, McpHandlers};
use hatate_core::mcp::server::McpServerController;
use serde_json::{Value, json};

const TOKEN: &str = "synthetic-test-token";

#[derive(Default)]
struct Scripted {
    calls: Mutex<Vec<String>>,
    shutdowns: Mutex<u32>,
}

impl Scripted {
    fn answer(&self, name: &str) -> HandlerResult {
        self.calls.lock().unwrap().push(name.to_owned());
        if name == "remove_row" {
            return Err(HandlerError::new("boom"));
        }
        if name == "get_images" {
            return Ok(ToolOutput::new().push("row", json!(1)).push_image(
                "match_image",
                ImageValue {
                    bytes: Some(b"png".to_vec()),
                    entries: vec![("mime_type".into(), json!("image/png"))],
                },
            ));
        }
        Ok(ToolOutput::new().push("called", json!(name)))
    }
}

impl McpHandlers for Scripted {
    fn list_queue(&self, _: i64, _: i64, _: Option<String>) -> HandlerResult {
        self.answer("list_queue")
    }
    fn get_entry(&self, _: i64) -> HandlerResult {
        self.answer("get_entry")
    }
    fn get_candidates(&self, _: i64) -> HandlerResult {
        self.answer("get_candidates")
    }
    fn get_images(&self, _: i64, _: i64) -> HandlerResult {
        self.answer("get_images")
    }
    fn get_diff(&self, _: i64, _: i64) -> HandlerResult {
        self.answer("get_diff")
    }
    fn list_actions(&self) -> HandlerResult {
        self.answer("list_actions")
    }
    fn select_candidate(&self, _: i64, _: i64) -> HandlerResult {
        self.answer("select_candidate")
    }
    fn toggle_reviewed(&self, _: i64, _: bool) -> HandlerResult {
        self.answer("toggle_reviewed")
    }
    fn research(&self, _: Vec<i64>) -> HandlerResult {
        self.answer("research")
    }
    fn send_upload(&self, _: i64) -> HandlerResult {
        self.answer("send_upload")
    }
    fn send_url(&self, _: i64) -> HandlerResult {
        self.answer("send_url")
    }
    fn download_send(&self, _: i64) -> HandlerResult {
        self.answer("download_send")
    }
    fn remove_row(&self, _: i64) -> HandlerResult {
        self.answer("remove_row")
    }
    fn reset_result(&self, _: i64) -> HandlerResult {
        self.answer("reset_result")
    }
    fn shutdown_invoker(&self) {
        *self.shutdowns.lock().unwrap() += 1;
    }
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn settings(port: u16) -> McpSettings {
    McpSettings {
        enabled: true,
        port: i64::from(port),
        token: TOKEN.into(),
        ..McpSettings::default()
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    /// The JSON of the one `data:` line of an SSE reply.
    fn event(&self) -> Value {
        let data = self
            .body
            .strip_prefix("event: message\r\ndata: ")
            .and_then(|b| b.strip_suffix("\r\n\r\n"));
        serde_json::from_str(data.unwrap_or_else(|| panic!("not an SSE reply: {:?}", self.body)))
            .unwrap()
    }
}

fn http(port: u16, method: &str, headers: &[(&str, &str)], body: &str) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut req = format!(
        "{method} /mcp HTTP/1.1\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (n, v) in headers {
        req.push_str(&format!("{n}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(n, v)| (n.to_lowercase(), v.to_owned()))
        .collect::<Vec<_>>();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v == "chunked");
    let body = if chunked {
        dechunk(rest)
    } else {
        rest.to_owned()
    };
    Response {
        status,
        headers,
        body,
    }
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&tail[..n]);
        rest = &tail[n + 2..];
    }
    out
}

fn authed(extra: &[(&'static str, String)]) -> Vec<(String, String)> {
    let mut h = vec![
        ("host".to_owned(), "127.0.0.1:1".to_owned()),
        ("authorization".to_owned(), format!("Bearer {TOKEN}")),
        ("content-type".to_owned(), "application/json".to_owned()),
        (
            "accept".to_owned(),
            "application/json, text/event-stream".to_owned(),
        ),
    ];
    for (n, v) in extra {
        h.retain(|(name, _)| name != n);
        h.push(((*n).to_owned(), v.clone()));
    }
    h
}

fn post(port: u16, extra: &[(&'static str, String)], body: &Value) -> Response {
    let headers = authed(extra);
    let refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    http(port, "POST", &refs, &body.to_string())
}

fn initialize(port: u16) -> String {
    let r = post(
        port,
        &[],
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    assert_eq!(r.status, 200);
    r.header("mcp-session-id").unwrap().to_owned()
}

fn call(port: u16, sid: &str, id: u64, name: &str, args: Value) -> Value {
    post(
        port,
        &[("mcp-session-id", sid.to_owned())],
        &json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
    .event()
}

fn started() -> (McpServerController, Arc<Scripted>, u16) {
    let handlers = Arc::new(Scripted::default());
    let port = free_port();
    let mut c = McpServerController::new(handlers.clone());
    assert!(c.start(&settings(port)), "{:?}", c.last_error());
    (c, handlers, port)
}

#[test]
fn the_bearer_token_gates_every_request() {
    let (mut c, _, port) = started();
    for (label, auth) in [
        ("missing", None),
        ("wrong", Some("Bearer nope")),
        ("scheme", Some("bearer synthetic-test-token")),
    ] {
        let mut headers = vec![
            ("host", "127.0.0.1:1"),
            ("content-type", "application/json"),
        ];
        if let Some(a) = auth {
            headers.push(("authorization", a));
        }
        let r = http(port, "POST", &headers, "{}");
        assert_eq!(
            (r.status, r.body.as_str(), r.header("content-type")),
            (401, "Unauthorized", Some("text/plain")),
            "{label}"
        );
    }
    c.stop(true);
}

#[test]
fn initialize_and_tools_list_match_the_python_server() {
    let (mut c, _, port) = started();
    let r = post(
        port,
        &[],
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("text/event-stream"));
    assert_eq!(r.header("cache-control"), Some("no-cache, no-transform"));
    assert_eq!(r.header("x-accel-buffering"), Some("no"));
    assert_eq!(r.header("mcp-session-id").unwrap().len(), 32);
    let mut event = r.event();
    // serverInfo.version is the granted difference.
    event["result"]["serverInfo"]["version"] = json!("X");
    assert_eq!(
        event.to_string(),
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"experimental":{},"prompts":{"listChanged":false},"resources":{"subscribe":false,"listChanged":false},"tools":{"listChanged":false}},"serverInfo":{"name":"hatate-linux","version":"X"}}}"#
    );
    let sid = r.header("mcp-session-id").unwrap().to_owned();
    let note = post(
        port,
        &[("mcp-session-id", sid.clone())],
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    assert_eq!(
        (note.status, note.body.as_str(), note.header("content-type")),
        (202, "", Some("application/json"))
    );
    let list = post(
        port,
        &[("mcp-session-id", sid)],
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    )
    .event();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/mcp/python_tools_list.json")).unwrap();
    assert_eq!(
        list["result"]["tools"], expected,
        "tools/list is the Python server's, key for key"
    );
    c.stop(true);
}

#[test]
fn every_tool_is_called_through_the_wire_and_a_raising_handler_is_an_error_result() {
    let (mut c, handlers, port) = started();
    let sid = initialize(port);
    let args = json!({"row": 1, "candidate_index": 0, "reviewed": true, "rows": [1]});
    for (i, spec) in hatate_core::mcp::tools::TOOLS.iter().enumerate() {
        let params: serde_json::Map<String, Value> = hatate_core::mcp::schema::params(spec.name)
            .unwrap()
            .iter()
            .filter(|p| p.default.is_none())
            .map(|p| (p.name.to_owned(), args[p.name].clone()))
            .collect();
        let reply = call(port, &sid, 10 + i as u64, spec.name, Value::Object(params));
        let result = &reply["result"];
        if spec.name == "remove_row" {
            assert_eq!(
                result,
                &json!({"content":[{"type":"text","text":"Error executing tool remove_row: boom"}],"isError":true})
            );
        } else if spec.name == "get_images" {
            assert_eq!(
                result["content"][1],
                json!({"type":"image","data":"cG5n","mimeType":"image/png"})
            );
            assert_eq!(result["isError"], false);
        } else {
            assert_eq!(
                result["content"][0]["text"],
                format!("{{\"called\": \"{}\"}}", spec.name)
            );
            assert_eq!(result["isError"], false);
        }
    }
    assert_eq!(handlers.calls.lock().unwrap().len(), 14);
    c.stop(true);
}

#[test]
fn bad_arguments_unknown_tools_and_unknown_methods_read_like_fastmcp() {
    let (mut c, _, port) = started();
    let sid = initialize(port);
    let text = |reply: &Value| {
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        text(&call(port, &sid, 2, "get_entry", json!({}))),
        "Error executing tool get_entry: 1 validation error for get_entryArguments\nrow\n  Field required [type=missing, input_value={}, input_type=dict]\n    For further information visit https://errors.pydantic.dev/2.13/v/missing"
    );
    assert_eq!(
        text(&call(port, &sid, 3, "nope", json!({}))),
        "Unknown tool: nope"
    );
    assert_eq!(
        call(port, &sid, 4, "get_entry", json!({"row": "7"}))["result"]["isError"],
        false
    );
    let unknown = post(
        port,
        &[("mcp-session-id", sid.clone())],
        &json!({"jsonrpc":"2.0","id":5,"method":"bogus"}),
    );
    assert_eq!(
        unknown.event(),
        json!({"jsonrpc":"2.0","id":5,"error":{"code":-32602,"message":"Invalid request parameters","data":""}})
    );
    let unregistered = post(
        port,
        &[("mcp-session-id", sid.clone())],
        &json!({"jsonrpc":"2.0","id":6,"method":"logging/setLevel","params":{"level":"debug"}}),
    );
    assert_eq!(
        unregistered.event(),
        json!({"jsonrpc":"2.0","id":6,"error":{"code":-32601,"message":"Method not found"}})
    );
    let ping = post(
        port,
        &[("mcp-session-id", sid)],
        &json!({"jsonrpc":"2.0","id":"abc","method":"ping"}),
    );
    assert_eq!(
        ping.event(),
        json!({"jsonrpc":"2.0","id":"abc","result":{}})
    );
    c.stop(true);
}

#[test]
fn transport_errors_have_fastmcps_status_and_body() {
    let (mut c, _, port) = started();
    let sid = initialize(port);
    let err = |status: u16, message: &str, code: i64| -> (u16, String) {
        (
            status,
            json!({"jsonrpc":"2.0","id":"server-error","error":{"code":code,"message":message}})
                .to_string(),
        )
    };
    let ping = json!({"jsonrpc":"2.0","id":2,"method":"ping"});
    let r = post(port, &[], &ping);
    assert_eq!(
        (r.status, r.body.clone()),
        err(400, "Bad Request: Missing session ID", -32600)
    );
    assert!(
        r.header("mcp-session-id").is_some(),
        "a fresh transport's error carries its own id"
    );
    let r = post(port, &[("mcp-session-id", "nope".to_owned())], &ping);
    assert_eq!(
        (r.status, r.body.clone()),
        err(404, "Session not found", -32600)
    );
    assert!(r.header("mcp-session-id").is_none());
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("accept", "application/json".to_owned()),
        ],
        &ping,
    );
    assert_eq!(
        (r.status, r.body),
        err(
            406,
            "Not Acceptable: Client must accept both application/json and text/event-stream",
            -32600
        )
    );
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("mcp-protocol-version", "1999-01-01".to_owned()),
        ],
        &ping,
    );
    assert_eq!(
        (r.status, r.body),
        err(
            400,
            "Bad Request: Unsupported protocol version: 1999-01-01. Supported versions: 2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25",
            -32600
        )
    );
    let headers = authed(&[("mcp-session-id", sid.clone())]);
    let refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let r = http(port, "POST", &refs, "not json");
    assert_eq!(
        (r.status, r.body),
        err(
            400,
            "Parse error: Expecting value: line 1 column 1 (char 0)",
            -32700
        )
    );
    let r = http(port, "PUT", &refs, "{}");
    assert_eq!(r.status, 405);
    assert_eq!(r.header("allow"), Some("GET, POST, DELETE"));
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("content-type", "text/plain".to_owned()),
        ],
        &ping,
    );
    assert_eq!(
        (r.status, r.body.as_str(), r.header("content-type")),
        (400, "Invalid Content-Type header", None)
    );
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("host", "evil.example".to_owned()),
        ],
        &ping,
    );
    assert_eq!((r.status, r.body.as_str()), (421, "Invalid Host header"));
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("origin", "http://evil.example".to_owned()),
        ],
        &ping,
    );
    assert_eq!((r.status, r.body.as_str()), (403, "Invalid Origin header"));
    let r = post(
        port,
        &[
            ("mcp-session-id", sid.clone()),
            ("origin", "http://127.0.0.1:3000".to_owned()),
        ],
        &ping,
    );
    assert_eq!(r.status, 200);
    let r = http(port, "DELETE", &refs, "");
    assert_eq!(r.status, 200);
    let r = post(port, &[("mcp-session-id", sid)], &ping);
    assert_eq!((r.status, r.body), err(404, "Session not found", -32600));
    c.stop(true);
}

#[test]
fn a_busy_port_is_reported_then_stop_and_restart_work() {
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    let handlers = Arc::new(Scripted::default());
    let mut c = McpServerController::new(handlers.clone());
    assert!(!c.start(&settings(port)));
    assert_eq!(
        c.last_error(),
        Some(
            format!(
                "MCP server did not start listening on port {port} within 5s (port already in use?)"
            )
            .as_str()
        )
    );
    assert_eq!(c.last_error_kind(), Some(ErrorKind::McpPortBusy));
    assert!(!c.running());
    drop(held);
    assert!(c.start(&settings(port)));
    assert_eq!(c.last_error(), None, "a successful start clears the error");
    assert!(c.running());
    assert!(
        c.start(&settings(port)),
        "start on a running server is true"
    );
    let other = free_port();
    assert!(c.restart(&settings(other)));
    assert_eq!(post(other, &[], &json!({})).status, 400);
    assert_eq!(
        *handlers.shutdowns.lock().unwrap(),
        0,
        "restart leaves the invoker alone"
    );
    c.stop(true);
    assert!(!c.running());
    assert_eq!(*handlers.shutdowns.lock().unwrap(), 1);
    c.stop(true);
}

#[test]
fn start_refuses_disabled_blank_and_unencodable_tokens() {
    let handlers = Arc::new(Scripted::default());
    let mut c = McpServerController::new(handlers);
    let mut s = settings(free_port());
    s.enabled = false;
    assert!(!c.start(&s));
    assert_eq!(c.last_error(), None);
    s.enabled = true;
    s.token = "".into();
    assert!(!c.start(&s));
    assert_eq!(
        c.last_error(),
        Some("Blank bearer token: refusing to start the MCP server")
    );
    s.token = "tok\u{2603}en".into();
    assert!(!c.start(&s));
    assert_eq!(
        c.last_error(),
        Some(
            "Could not start the MCP server: 'latin-1' codec can't encode character '\\u2603' in position 10: ordinal not in range(256)"
        )
    );
    // A failed start never leaks the token into the user-visible text.
    assert!(!c.last_error().unwrap().contains("tok"));
    s.token = TOKEN.into();
    s.port = free_port().into();
    assert!(c.start(&s));
    assert_eq!(c.last_error(), None, "a good start clears the last error");
    c.stop(true);
    // stop() is safe on a controller that never started.
    let mut never = McpServerController::new(Arc::new(Scripted::default()));
    never.stop(true);
    never.stop(true);
}
