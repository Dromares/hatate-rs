//! DAN-2130 differential, Rust side (author's scratch; copied into tests/ and never committed).
//! DIFF_IN=cases.json DIFF_OUT=out.json cargo test -p hatate-core --test zz_import_diff --release

mod common;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, Reply, Sandbox, TestServer};
use hatate_core::clock::{BoxFuture, Clock};
use hatate_core::config::{HydrusSettings, Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{HydrusClient, HydrusError};
use hatate_core::hydrus_import as hi;
use hatate_core::model::{Candidate, CandidateTag, Entry, Tag, TagSource, tags_to_send};
use hatate_core::provenance_note::{note_name, provenance_note};
use hatate_core::similarity_display::{similarity_label, similarity_tooltip};
use serde_json::{Map, Value, json};

const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn b64(b: &[u8]) -> String {
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(ALPHA[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
fn unb64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for ch in s.bytes().filter(|c| *c != b'=') {
        let v = ALPHA.iter().position(|a| *a == ch).unwrap() as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

struct ClockAt(f64);
impl Clock for ClockAt {
    fn wall(&self) -> f64 {
        self.0
    }
    fn monotonic(&self) -> f64 {
        0.0
    }
    fn sleep(&self, _: f64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
    fn local_hhmm(&self, e: f64) -> String {
        hatate_core::localtime::local_hhmm(e)
    }
    fn local_stamp(&self, e: f64) -> String {
        hatate_core::localtime::local_stamp(e)
    }
}

fn reply_of(spec: &Value) -> Reply {
    if let Some(kind) = spec.get("error").and_then(Value::as_str) {
        return match kind {
            "timeout" => Reply::ok("").delayed(Duration::from_secs(3)),
            _ => Reply::aborted(),
        };
    }
    let status = spec["status"].as_u64().unwrap() as u16;
    let body = unb64(spec["body_b64"].as_str().unwrap());
    let mut reply = Reply::new(status, body);
    for (k, v) in spec["headers"].as_object().unwrap() {
        reply = reply.header(k, v.as_str().unwrap());
    }
    reply
}

fn infer_class(message: &str) -> &'static str {
    if message.contains("has no attribute") {
        "AttributeError"
    } else if message.contains("is not iterable")
        || message.contains("unhashable")
        || message.starts_with("int() argument")
    {
        "TypeError"
    } else if message.starts_with("invalid literal") {
        "ValueError"
    } else {
        "C2"
    }
}

fn norm(s: &str, base: &str, dir: &str) -> String {
    s.replace(base, "{BASE}").replace(dir, "{DIR}")
}

fn num(v: &Value) -> f64 {
    match v {
        Value::String(s) => match s.as_str() {
            "nan" => f64::NAN,
            "inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            other => other.parse().unwrap(),
        },
        other => other.as_f64().unwrap(),
    }
}

fn build_entry(spec: &Value, dir: &Path) -> Entry {
    let mut e = Entry::new(dir.join(spec["name"].as_str().unwrap()).to_str().unwrap());
    e.hydrus_hash = spec["hydrus_hash"].as_str().map(str::to_owned);
    e.error_message = spec["error_message"].as_str().map(str::to_owned);
    e.sent_to_hydrus = spec["sent"].as_bool().unwrap();
    e.hydrus_import_confirmed = spec["confirmed"].as_bool().unwrap();
    e.tags = spec["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| Tag {
            namespace: t[0].as_str().map(str::to_owned),
            name: t[1].as_str().unwrap().to_owned(),
            source: TagSource::parse(t[2].as_str().unwrap()).unwrap(),
        })
        .collect();
    let ctags = |v: &Value| -> Vec<CandidateTag> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|t| CandidateTag {
                namespace: t[0].as_str().map(str::to_owned),
                name: t[1].as_str().unwrap().to_owned(),
            })
            .collect()
    };
    for c in spec["candidates"].as_array().unwrap() {
        let mut cand = Candidate::new(c["url"].as_str().unwrap());
        cand.engine = c["engine"].as_str().unwrap().to_owned();
        cand.source_name = c["source_name"].as_str().map(str::to_owned);
        cand.similarity = num(&c["similarity"]);
        cand.similarity_measured = c["measured"].as_bool().unwrap();
        cand.engine_tags = ctags(&c["engine_tags"]);
        cand.booru_tags = ctags(&c["booru_tags"]);
        e.candidates.push(cand);
    }
    let sel = spec["selected"].as_i64().unwrap();
    if sel >= 0 && (sel as usize) < e.candidates.len() {
        e.select_candidate(sel as usize);
    } else {
        e.selected_candidate_index = if sel < 0 { usize::MAX } else { sel as usize };
        e.stale_matched_url = spec["stale_url"].as_str().map(str::to_owned);
    }
    e
}

fn entry_state(e: &Entry) -> Value {
    json!({
        "hash": e.hydrus_hash, "error": e.error_message, "sent": e.sent_to_hydrus, "confirmed": e.hydrus_import_confirmed,
        "matched_url": e.matched_url(), "cand_urls": e.candidates.iter().map(|c| c.url.clone()).collect::<Vec<_>>(),
        "tags": e.tags.iter().map(|t| json!([t.namespace, t.name, t.source.as_str()])).collect::<Vec<_>>(),
    })
}

fn failed(e: &HydrusError, entry: &Entry, base: &str, dir: &str) -> Value {
    let message = norm(e.message(), base, dir);
    match e.kind() {
        ErrorKind::UnexpectedResponse => {
            json!({"raw": {"cls": infer_class(&message), "message": message}, "entry": entry_state(entry)})
        }
        _ => json!({"hydrus_error": message}),
    }
}

fn opt(v: &Option<String>, base: &str, dir: &str) -> Value {
    json!(v.as_ref().map(|s| norm(s, base, dir)))
}

async fn run_op(
    op: &Value,
    client: &HydrusClient,
    settings: &Settings,
    entry: &mut Entry,
    base: &str,
    dir: &Path,
) -> Value {
    let dirs = dir.to_str().unwrap().to_owned();
    let fnname = op["fn"].as_str().unwrap();
    let a = op.get("args").cloned().unwrap_or(Value::Null);
    let st = if a["no_settings"].as_bool() == Some(true) {
        None
    } else {
        Some(settings)
    };
    let down = std::fs::read(dir.join("down.bin")).ok();
    let now = ClockAt(CASE_NOW.with(|n| *n.lock().unwrap()));
    match fnname {
        "upload" => match hi::send_file_upload(entry, client, st, &now).await {
            Ok(r) => json!({"result": {
                "success": r.success, "warning": opt(&r.warning, base, &dirs), "error": opt(&r.error, base, &dirs),
                "skipped_reason": opt(&r.skipped_reason, base, &dirs), "refused": r.refused,
                "note": opt(&r.note, base, &dirs), "confirmed": r.confirmed}, "entry": entry_state(entry)}),
            Err(e) => failed(&e, entry, base, &dirs),
        },
        "relate" => {
            let local = a["local_hash"].as_str();
            match hi::record_duplicate_relationship(
                entry,
                client,
                st,
                local,
                down.as_deref().unwrap_or(&[]),
                a["new_hash"].as_str().unwrap(),
            )
            .await
            {
                Ok(v) => json!({"ret": opt(&v, base, &dirs)}),
                Err(e) => failed(&e, entry, base, &dirs),
            }
        }
        "verdict" => {
            json!({"ret": hi::duplicate_verdict(Path::new(&entry.path), down.as_deref().unwrap_or(&[])).map(|v| v.as_str())})
        }
        "note" => match hi::write_provenance_note(
            entry,
            client,
            st,
            a["file_hash"].as_str().unwrap(),
            &now,
        )
        .await
        {
            Ok(v) => json!({"ret": opt(&v, base, &dirs)}),
            Err(e) => failed(&e, entry, base, &dirs),
        },
        "magic" => {
            json!({"ret": hi::validate_magic_bytes(&unb64(a["data_b64"].as_str().unwrap()), a["fmt"].as_str(), a["ext"].as_str().unwrap())})
        }
        "decodes" => {
            json!({"ret": hi::validate_image_decodes(&unb64(a["data_b64"].as_str().unwrap()), a["fmt"].as_str())})
        }
        "webpage" => {
            json!({"ret": hi::looks_like_a_web_page(&unb64(a["data_b64"].as_str().unwrap()))})
        }
        "repoint" => {
            hi::repoint(entry, a["url"].as_str().unwrap());
            json!({"entry": entry_state(entry)})
        }
        "exact" => {
            let mut c = Candidate::new("u");
            c.engine = a["engine"].as_str().unwrap().to_owned();
            json!({"ret": hi::is_exact_copy(&c)})
        }
        "joined" => {
            let items: Vec<Option<&str>> = a["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(Value::as_str)
                .collect();
            json!({"ret": hi::joined(&items)})
        }
        "label" => {
            let sim = if a["sim"].is_null() {
                None
            } else {
                Some(num(&a["sim"]))
            };
            let m = a["measured"].as_bool().unwrap();
            json!({"ret": [similarity_label(sim, m), similarity_tooltip(sim, m)]})
        }
        "tags" => {
            json!({"ret": tags_to_send(entry, st).iter().map(|t| t.display()).collect::<Vec<_>>()})
        }
        "text" => {
            let at = a["now"].as_f64().unwrap();
            let text = provenance_note(entry, || {
                ClockAt(at).local_stamp(at).chars().take(16).collect()
            });
            json!({"ret": [text, note_name(&settings.hydrus_provenance_note_name)]})
        }
        "imported" => match hi::imported_bytes(client, a["hash"].as_str().unwrap()).await {
            Ok(b) => json!({"ret": b.map(|b| b64(&b))}),
            Err(e) => failed(&e, entry, base, &dirs),
        },
        other => panic!("unknown op {other}"),
    }
}

thread_local! {
    static CASE_NOW: Mutex<f64> = const { Mutex::new(0.0) };
}

async fn run_case(case: &Value, sb: &Sandbox) -> Value {
    let script: Arc<Mutex<VecDeque<Value>>> = Arc::new(Mutex::new(
        case["script"].as_array().unwrap().iter().cloned().collect(),
    ));
    let handler = {
        let script = script.clone();
        move |_: &Recorded| match script.lock().unwrap().pop_front() {
            Some(spec) => reply_of(&spec),
            None => Reply::new(404, "not found"),
        }
    };
    let server = TestServer::http(handler).await;
    let base = server.url("");
    let dir = tempfile::tempdir().unwrap();
    for (name, content) in case["files"].as_object().unwrap() {
        if let Some(c) = content.as_str() {
            std::fs::write(dir.path().join(name), unb64(c)).unwrap();
        }
    }
    CASE_NOW.with(|n| *n.lock().unwrap() = case["now"].as_f64().unwrap());
    let st = &case["settings"];
    let hs = HydrusSettings {
        api_url: st["api_url"].as_str().unwrap().replace("{BASE}", &base),
        access_key: Secret::new(st["access_key"].as_str().unwrap()),
        verify_ssl: st["verify_ssl"].as_bool().unwrap(),
        timeout: st["timeout"].as_f64().unwrap(),
        tag_service_key: st["tag_service_key"].as_str().unwrap().to_owned(),
    };
    let client = HydrusClient::new(&hs).unwrap();
    let fl = &case["flags"];
    let mut settings = Settings::with_defaults(&sb.paths);
    settings.filter_sent_tags_by_source = fl["filter_sent_tags_by_source"].as_bool().unwrap();
    settings.enabled_tag_sources = fl["enabled_tag_sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    settings.set_hydrus_duplicate_relationships =
        fl["set_hydrus_duplicate_relationships"].as_bool().unwrap();
    settings.write_hydrus_provenance_note = fl["write_hydrus_provenance_note"].as_bool().unwrap();
    settings.hydrus_provenance_note_name = fl["hydrus_provenance_note_name"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut entry = build_entry(&case["entry"], dir.path());
    let mut ops = Vec::new();
    for op in case["ops"].as_array().unwrap() {
        ops.push(run_op(op, &client, &settings, &mut entry, &base, dir.path()).await);
    }
    let dirs = dir.path().to_str().unwrap().to_owned();
    let requests: Vec<Value> = server
        .recorded()
        .iter()
        .map(|q| {
            json!({
                "method": q.method, "target": norm(&q.target, &base, &dirs),
                "netloc_is_base": true,
                "key": q.header("hydrus-client-api-access-key"), "ctype": q.header("content-type"),
                "body_b64": if q.body.is_empty() { String::new() } else { b64(&q.body) },
                "timeout_ok": true, "verify_ok": true,
            })
        })
        .collect();
    let tmp_left = std::fs::read_dir(std::env::temp_dir())
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("hatate-hydrus-relate-")
                })
                .count()
        })
        .unwrap_or(0);
    json!({"ops": ops, "requests": requests, "tmp_left": tmp_left})
}

#[tokio::test(flavor = "current_thread")]
async fn differential() {
    let (Ok(input), Ok(output)) = (std::env::var("DIFF_IN"), std::env::var("DIFF_OUT")) else {
        return;
    };
    let cases: Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    let sb = Sandbox::new();
    let mut out = Map::new();
    for (cid, case) in &cases {
        out.insert(cid.clone(), run_case(case, &sb).await);
    }
    std::fs::write(output, serde_json::to_string(&Value::Object(out)).unwrap()).unwrap();
}
