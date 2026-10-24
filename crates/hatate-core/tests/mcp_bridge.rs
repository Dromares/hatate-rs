//! The stdio bridge's HTTP leg against the real server and against scripted raw servers, plain
//! http on loopback (the scheme it ships on). Every credential is a placeholder.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use hatate_core::config::McpSettings;
use hatate_core::mcp::bridge::run;
use hatate_core::mcp::content::ToolOutput;
use hatate_core::mcp::handlers::{HandlerResult, McpHandlers};
use hatate_core::mcp::server::McpServerController;
use serde_json::{Value, json};

const TOKEN: &str = "synthetic-bridge-token";

struct Echo;

macro_rules! echo {
    ($($name:ident($($arg:ident: $ty:ty),*)),* $(,)?) => {
        $(fn $name(&self, $($arg: $ty),*) -> HandlerResult {
            let _ = ($(&$arg,)*);
            Ok(ToolOutput::new().push("tool", json!(stringify!($name))))
        })*
    };
}

impl McpHandlers for Echo {
    echo!(
        list_queue(a: i64, b: i64, c: Option<String>),
        get_entry(a: i64),
        get_candidates(a: i64),
        get_images(a: i64, b: i64),
        get_diff(a: i64, b: i64),
        list_actions(),
        select_candidate(a: i64, b: i64),
        toggle_reviewed(a: i64, b: bool),
        research(a: Vec<i64>),
        send_upload(a: i64),
        send_url(a: i64),
        download_send(a: i64),
        remove_row(a: i64),
        reset_result(a: i64),
    );
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn bridge(port: i64, token: &str, timeout: f64, input: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(port, token, timeout, input.as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// A server that answers every request with `reply` (raw bytes) and records what it was sent.
fn scripted(reply: &'static [u8]) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            stream
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&data).into_owned();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let want = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if body.len() >= want {
                        break;
                    }
                }
            }
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data).into_owned());
            let _ = stream.write_all(reply);
        }
    });
    (port, seen)
}

#[test]
fn a_session_is_carried_across_lines_against_the_real_server() {
    let port = free_port();
    let s = McpSettings {
        enabled: true,
        port: i64::from(port),
        token: TOKEN.into(),
        ..McpSettings::default()
    };
    let mut server = McpServerController::new(Arc::new(Echo));
    assert!(server.start(&s));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        "",
        "   ",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_entry","arguments":{"row":3}}}"#,
    ]
    .join("\n");
    let (code, out, err) = bridge(i64::from(port), TOKEN, 5.0, &input);
    assert_eq!((code, err.as_str()), (0, ""));
    let lines: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "the notification's 202 prints nothing: {out}"
    );
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(
        lines[1]["result"]["content"][0]["text"],
        "{\"tool\": \"get_entry\"}"
    );
    // A wrong token is relayed as the server's plain-text 401, one line.
    let (code, out, _) = bridge(
        i64::from(port),
        "wrong",
        5.0,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    assert_eq!((code, out.as_str()), (0, "Unauthorized\n"));
    server.stop(true);
}

#[test]
fn a_server_that_is_down_answers_the_request_id_with_a_jsonrpc_error() {
    let port = free_port();
    let (code, out, _) = bridge(
        i64::from(port),
        TOKEN,
        5.0,
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\nnot json\n",
    );
    assert_eq!(code, 0);
    let url = format!("http://127.0.0.1:{port}/mcp");
    let cause = format!(
        "HTTPConnectionPool(host='127.0.0.1', port={port}): Max retries exceeded with url: /mcp (Caused by NewConnectionError(\"HTTPConnection(host='127.0.0.1', port={port}): Failed to establish a new connection: [Errno 111] Connection refused\"))"
    );
    let message = format!("Could not reach the app's MCP server at {url}: {cause}");
    let expect = |id: Value| {
        format!(
            "{{\"jsonrpc\": \"2.0\", \"id\": {id}, \"error\": {{\"code\": -32000, \"message\": {}}}}}\n",
            json!(message)
        )
    };
    // json.dumps escapes nothing here (all ASCII), so serde's string form is the same text.
    assert_eq!(out, format!("{}{}", expect(json!(7)), expect(json!(null))));
    // Neither the token nor a path of ours is in what the client sees.
    assert!(!out.contains(TOKEN));
}

#[test]
fn a_server_that_never_answers_is_a_read_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let _keep = thread::spawn(move || {
        let held: Vec<TcpStream> = listener.incoming().flatten().take(1).collect();
        thread::sleep(Duration::from_secs(3));
        drop(held);
    });
    let (_, out, _) = bridge(
        i64::from(port),
        TOKEN,
        0.3,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    let line: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        line["error"]["message"],
        format!(
            "Could not reach the app's MCP server at http://127.0.0.1:{port}/mcp: HTTPConnectionPool(host='127.0.0.1', port={port}): Read timed out. (read timeout=0.3)"
        )
    );
}

#[test]
fn non_json_and_event_stream_bodies_are_relayed_the_way_python_relays_them() {
    let (port, seen) = scripted(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: 11\r\n\r\n  hello  \n ");
    let (_, out, _) = bridge(i64::from(port), TOKEN, 5.0, "{\"a\":1}\n");
    assert_eq!(out, "hello\n");
    let request = seen.lock().unwrap()[0].clone();
    assert!(request.starts_with("POST /mcp HTTP/1.1\r\n"));
    let lower = request.to_lowercase();
    assert!(lower.contains(&format!("authorization: bearer {TOKEN}")));
    assert!(lower.contains("content-type: application/json\r\n"));
    assert!(lower.contains("accept: application/json, text/event-stream\r\n"));
    assert!(request.ends_with("{\"a\":1}"));
    assert!(!lower.contains("mcp-session-id"));

    // An event stream: only `data:` lines, stripped, split the Python way; the session id sticks.
    let body = "event: message\r\ndata: one\r\n: ping\r\ndata:two \rdata:\x0b\x0cdata: three\n";
    let reply: &'static [u8] = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nMcp-Session-Id: abc123\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
        .into_boxed_slice(),
    );
    let (port, seen) = scripted(reply);
    let (_, out, _) = bridge(i64::from(port), TOKEN, 5.0, "{\"a\":1}\n{\"b\":2}\n");
    assert_eq!(out, "one\ntwo\nthree\none\ntwo\nthree\n");
    let second = seen.lock().unwrap()[1].to_lowercase();
    assert!(second.contains("mcp-session-id: abc123"), "{second}");
}
