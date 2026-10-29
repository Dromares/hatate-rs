//! The scripted Hydrus of the action differential (DAN-2220), Rust side.
//!
//! The same table-driven server runs in the Python driver (`py_driver.py`, outside the repo):
//! `routes` maps a path to the list of replies it gives in turn (the last one repeats), and
//! `/get_files/file_metadata` and `/add_files/delete_files` are answered from a per-hash state table
//! unless a route names them. Both sides log every request the same way, so the request log is
//! compared as it stands. Every access key is a synthetic placeholder and the only address is
//! `127.0.0.1`.

use crate::common::hydrus::{Hydrus, Recorded, Reply};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct St {
    routes: BTreeMap<String, (Vec<Value>, usize)>,
    hstate: BTreeMap<String, String>,
    tags: BTreeMap<String, Vec<String>>,
    stubborn: bool,
}

pub struct Server {
    pub h: Hydrus,
    st: Arc<Mutex<St>>,
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(target: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = target
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, val) = p.split_once('=').unwrap_or((p, ""));
            (unquote(k), unquote(val))
        })
        .collect();
    v.sort();
    v
}

fn json_reply(code: u16, v: &Value) -> Reply {
    Reply::new(code, v.to_string()).header("Content-Type", "application/json")
}

fn answer(st: &Mutex<St>, q: &Recorded) -> Reply {
    let path = q.target.split('?').next().unwrap_or("").to_owned();
    let mut st = st.lock().unwrap();
    if let Some((list, idx)) = st.routes.get_mut(&path) {
        if !list.is_empty() {
            let r = list[(*idx).min(list.len() - 1)].clone();
            *idx += 1;
            return json_reply(r["code"].as_u64().unwrap_or(200) as u16, &r["json"]);
        }
    }
    let hashes: Vec<String> = if path == "/get_files/file_metadata" {
        query(&q.target)
            .into_iter()
            .find(|(k, _)| k == "hashes")
            .and_then(|(_, v)| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    } else if path == "/add_files/delete_files" {
        serde_json::from_slice::<Value>(&q.body)
            .ok()
            .and_then(|b| serde_json::from_value(b["hashes"].clone()).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    match path.as_str() {
        "/get_files/file_metadata" => {
            let meta: Vec<Value> = hashes
                .iter()
                .enumerate()
                .map(|(n, h)| match st.hstate.get(h) {
                    None => json!({"hash": h, "file_id": Value::Null}),
                    Some(s) => json!({
                        "hash": h, "file_id": n + 1, "size": 94, "ext": ".png",
                        "is_local": s == "present", "is_trashed": s == "trashed",
                        "is_deleted": s == "deleted",
                        "tags": {"svc": {"display_tags": {"0": st.tags.get(h).cloned().unwrap_or_default()}}},
                    }),
                })
                .collect();
            json_reply(200, &json!({"metadata": meta}))
        }
        "/add_files/delete_files" => {
            if !st.stubborn {
                for h in hashes {
                    if st.hstate.contains_key(&h) {
                        st.hstate.insert(h, "trashed".to_owned());
                    }
                }
            }
            json_reply(200, &json!({}))
        }
        _ => json_reply(200, &json!({})),
    }
}

/// Starts the server for a scenario's `hydrus` spec: `{state, tags, routes, stubborn}`.
pub fn start(spec: &Value) -> Server {
    let mut st = St::default();
    for (p, list) in spec["routes"].as_object().into_iter().flatten() {
        st.routes
            .insert(p.clone(), (list.as_array().cloned().unwrap_or_default(), 0));
    }
    for (h, s) in spec["state"].as_object().into_iter().flatten() {
        st.hstate.insert(h.clone(), s.as_str().unwrap().to_owned());
    }
    for (h, t) in spec["tags"].as_object().into_iter().flatten() {
        st.tags.insert(
            h.clone(),
            t.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect(),
        );
    }
    st.stubborn = spec["stubborn"].as_bool().unwrap_or(false);
    let st = Arc::new(Mutex::new(st));
    let s2 = st.clone();
    let h = Hydrus::http(move |q| answer(&s2, q));
    Server { h, st }
}

impl Server {
    pub fn url(&self) -> &str {
        &self.h.url
    }

    /// A step may replace the routes (and nothing else) from then on.
    pub fn set_routes(&self, routes: &Value) {
        let mut st = self.st.lock().unwrap();
        st.routes.clear();
        for (p, list) in routes.as_object().into_iter().flatten() {
            st.routes
                .insert(p.clone(), (list.as_array().cloned().unwrap_or_default(), 0));
        }
    }

    /// Every request so far: method, path, sorted decoded query, whether the key header came, and
    /// the body (parsed when JSON, else its length).
    pub fn log(&self) -> Value {
        json!(
            self.h
                .recorded()
                .iter()
                .map(|q| {
                    let body = if q
                        .header("content-type")
                        .is_some_and(|c| c.contains("json"))
                    {
                        serde_json::from_slice::<Value>(&q.body).unwrap_or(Value::Null)
                    } else {
                        json!({"len": q.body.len()})
                    };
                    json!({
                        "m": q.method,
                        "p": q.target.split('?').next().unwrap_or(""),
                        "q": query(&q.target),
                        "key": q.header("hydrus-client-api-access-key").is_some(),
                        "body": body,
                    })
                })
                .collect::<Vec<_>>()
        )
    }
}
