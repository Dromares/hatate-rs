//! A minimal MCP client for the window tests: plain http to `127.0.0.1` (the scheme the server
//! ships on), a synthetic bearer token, JSON-RPC over the streamable-http transport. The window
//! owns the queue, so a call needs the main loop turning: [`Client::call`] runs the request on its
//! own thread and pumps the main context until the answer is in.

#![allow(dead_code)]

use super::pump_until;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// Not a credential: the token every test window is given.
pub const TOKEN: &str = "synthetic-test-token";

/// A port nothing is listening on right now.
pub fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The JSON of the one `data:` line of an SSE reply.
    pub fn event(&self) -> Value {
        let data = self
            .body
            .strip_prefix("event: message\r\ndata: ")
            .and_then(|b| b.strip_suffix("\r\n\r\n"));
        serde_json::from_str(data.unwrap_or_else(|| panic!("not an SSE reply: {:?}", self.body)))
            .unwrap()
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

fn http(port: u16, headers: &[(String, String)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut req = format!(
        "POST /mcp HTTP/1.1\r\nconnection: close\r\ncontent-length: {}\r\n",
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
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(n, v)| (n.to_lowercase(), v.to_owned()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v == "chunked");
    Reply {
        status,
        headers,
        body: if chunked {
            dechunk(rest)
        } else {
            rest.to_owned()
        },
    }
}

pub struct Client {
    pub port: u16,
    session: Option<String>,
    next_id: u64,
}

impl Client {
    pub fn new(port: u16) -> Client {
        Client {
            port,
            session: None,
            next_id: 1,
        }
    }

    fn headers(&self, token: &str) -> Vec<(String, String)> {
        let mut h = vec![
            ("host".to_owned(), "127.0.0.1:1".to_owned()),
            ("authorization".to_owned(), format!("Bearer {token}")),
            ("content-type".to_owned(), "application/json".to_owned()),
            (
                "accept".to_owned(),
                "application/json, text/event-stream".to_owned(),
            ),
        ];
        if let Some(sid) = &self.session {
            h.push(("mcp-session-id".to_owned(), sid.clone()));
        }
        h
    }

    /// One request, with the main loop turning while it is answered.
    pub fn post_with(&self, token: &str, body: &Value) -> Reply {
        let (port, headers, body) = (self.port, self.headers(token), body.to_string());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(http(port, &headers, &body));
        });
        let mut reply = None;
        assert!(
            pump_until(60_000, || {
                reply = rx.try_recv().ok();
                reply.is_some()
            }),
            "the server did not answer"
        );
        reply.unwrap()
    }

    pub fn post(&self, body: &Value) -> Reply {
        self.post_with(TOKEN, body)
    }

    pub fn initialize(&mut self) {
        let r = self.post(
            &json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"t","version":"1"}}}),
        );
        assert_eq!(r.status, 200, "{}", r.body);
        self.session = r.header("mcp-session-id").map(str::to_owned);
        assert!(self.session.is_some());
    }

    /// `tools/call`: the JSON-RPC answer.
    pub fn call(&mut self, name: &str, args: Value) -> Value {
        if self.session.is_none() {
            self.initialize();
        }
        self.next_id += 1;
        self.post(
            &json!({"jsonrpc":"2.0","id":self.next_id,"method":"tools/call",
            "params":{"name":name,"arguments":args}}),
        )
        .event()
    }

    /// The JSON a tool answered with (`content[0].text`), or the error text as a string.
    pub fn payload(&mut self, name: &str, args: Value) -> Value {
        let answer = self.call(name, args);
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text in {answer}"))
            .to_owned();
        serde_json::from_str(&text).unwrap_or(Value::String(text))
    }

    /// Whether a connection to the port is accepted at all.
    pub fn listening(&self) -> bool {
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", self.port).parse().unwrap(),
            Duration::from_millis(500),
        )
        .is_ok()
    }
}
