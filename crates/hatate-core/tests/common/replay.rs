//! The shared replay server (DAN-2057): a local https server that serves a scripted list of
//! responses, in order, and records every request it gets. Test support only (this file is
//! under `tests/`, the crate's `dev-dependencies` build it); `docs/net.md` section 11 documents
//! it. Every Phase 3 engine test reuses it.
//!
//! It is [`TestServer`] with a script: the Nth request gets the Nth scripted [`Reply`]. A request
//! that arrives after the script has run out gets a 599 and is counted in
//! [`ReplayServer::overflow`], so a client that sends more requests than the recorded exchange
//! fails the test instead of hanging. A reply left unserved is counted by
//! [`ReplayServer::unused`]; [`ReplayServer::assert_script_consumed`] fails on either.
//!
//! Hosts are injected: tests build a client against [`ReplayServer::url`], never a real host.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::net_server::{Recorded, Reply, TestPki, TestServer};

#[derive(Default)]
struct Script {
    replies: VecDeque<Reply>,
    overflow: usize,
}

pub struct ReplayServer {
    server: TestServer,
    script: Arc<Mutex<Script>>,
}

impl ReplayServer {
    /// An https server (certificate from `pki`) that answers with `script`, in order.
    pub async fn https(pki: &TestPki, script: Vec<Reply>) -> Self {
        let state = Arc::new(Mutex::new(Script {
            replies: script.into(),
            overflow: 0,
        }));
        let handler = state.clone();
        let server = TestServer::https(pki, move |_| {
            let mut script = handler.lock().unwrap();
            match script.replies.pop_front() {
                Some(reply) => reply,
                None => {
                    script.overflow += 1;
                    Reply::new(599, "replay script exhausted")
                }
            }
        })
        .await;
        ReplayServer {
            server,
            script: state,
        }
    }

    /// `https://127.0.0.1:<port><path>`
    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }

    /// Every request received, in order.
    pub fn recorded(&self) -> Vec<Recorded> {
        self.server.recorded()
    }

    /// How many scripted replies were not served.
    pub fn unused(&self) -> usize {
        self.script.lock().unwrap().replies.len()
    }

    /// How many requests arrived after the script ran out.
    pub fn overflow(&self) -> usize {
        self.script.lock().unwrap().overflow
    }

    /// Fails unless every scripted reply was served and no extra request arrived.
    pub fn assert_script_consumed(&self) {
        assert_eq!(
            self.overflow(),
            0,
            "requests arrived after the script ran out"
        );
        assert_eq!(self.unused(), 0, "scripted replies were never asked for");
    }
}

/// What the tests read off a [`Recorded`] request.
pub trait RecordedExt {
    /// The request target's path, without the query.
    fn path(&self) -> String;
    /// The query as decoded `(name, value)` pairs, in order.
    fn query(&self) -> Vec<(String, String)>;
    /// The `multipart/form-data` body as parts. Empty when the request has none.
    fn form_parts(&self) -> Vec<FormPart>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

impl RecordedExt for Recorded {
    fn path(&self) -> String {
        self.target.split('?').next().unwrap_or("").to_owned()
    }

    fn query(&self) -> Vec<(String, String)> {
        match self.target.split_once('?') {
            Some((_, q)) => url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            None => Vec::new(),
        }
    }

    fn form_parts(&self) -> Vec<FormPart> {
        let Some(content_type) = self.header("content-type") else {
            return Vec::new();
        };
        let Some(boundary) = content_type
            .split(';')
            .filter_map(|p| p.trim().strip_prefix("boundary="))
            .next()
        else {
            return Vec::new();
        };
        let delimiter = format!("--{}", boundary.trim_matches('"')).into_bytes();
        let mut parts = Vec::new();
        for chunk in split_on(&self.body, &delimiter).into_iter().skip(1) {
            // Each chunk is `\r\n<headers>\r\n\r\n<data>\r\n`; the last one is `--\r\n`.
            let Some(chunk) = chunk.strip_prefix(b"\r\n") else {
                continue;
            };
            let Some(split) = find(chunk, b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&chunk[..split]).into_owned();
            let data = chunk[split + 4..]
                .strip_suffix(b"\r\n")
                .unwrap_or(&chunk[split + 4..]);
            let mut part = FormPart {
                name: String::new(),
                filename: None,
                content_type: None,
                data: data.to_vec(),
            };
            for line in head.split("\r\n") {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                match name.trim().to_ascii_lowercase().as_str() {
                    "content-disposition" => {
                        for attr in value.split(';') {
                            let attr = attr.trim();
                            if let Some(v) = attr.strip_prefix("name=") {
                                part.name = v.trim_matches('"').to_owned();
                            } else if let Some(v) = attr.strip_prefix("filename=") {
                                part.filename = Some(v.trim_matches('"').to_owned());
                            }
                        }
                    }
                    "content-type" => part.content_type = Some(value.trim().to_owned()),
                    _ => {}
                }
            }
            parts.push(part);
        }
        parts
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn split_on<'a>(haystack: &'a [u8], needle: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut rest = haystack;
    while let Some(i) = find(rest, needle) {
        out.push(&rest[..i]);
        rest = &rest[i + needle.len()..];
    }
    out.push(rest);
    out
}

// ---- corpus `mocked_transport` items -----------------------------------------------------

/// One `mocked_transport` item of the corpus: the recorded exchange and what Python returned.
pub struct MockedItem {
    pub id: String,
    /// `call.json`.
    pub call: Value,
    /// The body of the last scripted response (`input.*`).
    pub input: Vec<u8>,
    /// The entry of `expected.json` for the callable.
    pub expected: Value,
}

impl MockedItem {
    /// The scripted answers as replies, in order. The last carries `input.*` as its body, the
    /// earlier ones their inline `text` or `json`.
    pub fn script(&self) -> Vec<Reply> {
        let responses = self.call["responses"].as_array().expect("responses");
        responses
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let status = u16::try_from(r["status"].as_u64().expect("status")).unwrap();
                let body: Vec<u8> = if r["body"] == "input" || i + 1 == responses.len() {
                    self.input.clone()
                } else if let Some(text) = r["text"].as_str() {
                    text.as_bytes().to_vec()
                } else if let Some(json) = r.get("json") {
                    json.to_string().into_bytes()
                } else {
                    Vec::new()
                };
                let mut reply = Reply::new(status, body);
                if let Some(headers) = r["headers"].as_object() {
                    for (name, value) in headers {
                        reply = reply.header(name, value.as_str().unwrap_or(""));
                    }
                }
                reply
            })
            .collect()
    }

    /// The recorded requests' method and path-with-query, in order, with the host dropped (the
    /// client under test talks to the replay server, not to the recorded host).
    pub fn recorded_targets(&self) -> Vec<(String, String)> {
        self.call["request"]
            .as_array()
            .expect("request")
            .iter()
            .map(|r| {
                let url = url::Url::parse(r["url"].as_str().expect("url")).expect("url parses");
                let mut target = url.path().to_owned();
                if let Some(q) = url.query() {
                    target.push('?');
                    target.push_str(q);
                }
                (r["method"].as_str().expect("method").to_owned(), target)
            })
            .collect()
    }
}

/// Every `mocked_transport` item of the corpus whose first Python callable is `callable`.
pub fn mocked_items(callable: &str) -> Vec<MockedItem> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("corpus/MANIFEST.json")).unwrap())
            .expect("MANIFEST.json parses");
    manifest["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|i| i["item_kind"] == "mocked_transport")
        .filter(|i| i["python_callables"][0] == callable)
        .map(|i| {
            let dir = root.join(i["dir"].as_str().unwrap());
            let read = |key: &str| std::fs::read(dir.join(i[key].as_str().unwrap())).unwrap();
            let expected: Value = serde_json::from_slice(&read("expected")).unwrap();
            MockedItem {
                id: i["id"].as_str().unwrap().to_owned(),
                call: serde_json::from_slice(&read("call")).unwrap(),
                input: read("input"),
                expected: expected["results"][callable].clone(),
            }
        })
        .collect()
}

/// `serde_json` value equality with numbers compared as numbers (an integer and the float with
/// the same value are the same number).
pub fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x == y || (x.as_f64().is_some() && x.as_f64() == y.as_f64())
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| same_json(v, w)))
        }
        _ => a == b,
    }
}
