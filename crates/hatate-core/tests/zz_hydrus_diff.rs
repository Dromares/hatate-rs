//! DAN-2126 differential, Rust side (author's scratch; copied into tests/ and never committed).
//! DIFF_IN=cases.json DIFF_OUT=out.json cargo test -p hatate-core --test zz_hydrus_diff --release

mod common;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, Reply, Sandbox, TestServer};
use hatate_core::config::{HydrusSettings, OrderedMap, Secret, Settings};
use hatate_core::error::ErrorKind;
use hatate_core::hydrus_client::{HydrusClient, HydrusError};
use hatate_core::hydrus_tag_lookup as htl;
use hatate_core::model::{Entry, Tag, TagSource};
use hatate_core::net;
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

fn reply_of(spec: &Value) -> Reply {
    if let Some(kind) = spec.get("error").and_then(Value::as_str) {
        return match kind {
            "timeout" => Reply::ok("").delayed(Duration::from_secs(3)),
            _ => Reply::aborted(),
        };
    }
    let status = spec["status"].as_u64().unwrap() as u16;
    let mut body = unb64(spec["body_b64"].as_str().unwrap());
    let mut promise: Option<usize> = None;
    if let Some(after) = spec.get("break_after").and_then(Value::as_u64) {
        if (after as usize) < body.len() {
            body.truncate(after as usize);
            promise = spec
                .get("promise")
                .and_then(Value::as_u64)
                .map(|p| p as usize);
        }
    }
    let mut reply = Reply::new(status, body);
    for (k, v) in spec["headers"].as_object().unwrap() {
        reply = reply.header(k, v.as_str().unwrap());
    }
    if let Some(p) = promise {
        reply = reply.content_length(&p.to_string()).closing();
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

fn cause_of(e: &HydrusError) -> Value {
    match e.kind() {
        ErrorKind::Net(net::ErrorKind::Timeout)
        | ErrorKind::Net(net::ErrorKind::DeadlineExceeded) => json!("timeout"),
        ErrorKind::Net(net::ErrorKind::Tls) => json!("tls"),
        ErrorKind::Net(net::ErrorKind::InvalidRequest) => json!("request"),
        ErrorKind::Net(_) => json!("conn"),
        _ => Value::Null,
    }
}

fn norm(s: &str, base: &str, dir: &str) -> String {
    s.replace(base, "{BASE}").replace(dir, "{DIR}")
}

fn err_json(e: &HydrusError, base: &str, dir: &str) -> Value {
    let message = norm(e.message(), base, dir);
    match e.kind() {
        ErrorKind::UnexpectedResponse => {
            json!({"raw": {"cls": infer_class(&message), "message": message}})
        }
        ErrorKind::Io => json!({"raw": {"cls": "OSError", "message": message}}),
        _ => json!({"err": {"status": e.status_code(), "message": message, "cause": cause_of(e)}}),
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_owned())
        .collect()
}

fn entry_state(e: &Entry) -> Value {
    json!({
        "hash": e.hydrus_hash,
        "tags": e.tags.iter().map(|t| json!([t.namespace, t.name, t.source.as_str()])).collect::<Vec<_>>(),
    })
}

async fn run_op(
    op: &Value,
    client: &Result<HydrusClient, HydrusError>,
    settings: &Value,
    base: &str,
    dir: &std::path::Path,
    sb: &Sandbox,
) -> Value {
    let dirs = dir.to_str().unwrap().to_owned();
    let fail = |e: &HydrusError| err_json(e, base, &dirs);
    let fnname = op["fn"].as_str().unwrap();
    let a = op.get("args").cloned().unwrap_or(Value::Null);
    // Ops that do not need the client.
    match fnname {
        "hash_from_filename" => {
            return json!({"ret": htl::hash_from_filename(a["path"].as_str().unwrap())});
        }
        "parse_tag" => {
            let t = htl::parse_tag(a["raw"].as_str().unwrap(), TagSource::Hydrus);
            return json!({"ret": [t.namespace, t.name]});
        }
        "verify_filename_hashes" => {
            let paths: Vec<String> = strings(&a["paths"])
                .iter()
                .map(|p| p.replace("{DIR}", &dirs))
                .collect();
            let ok = match a.get("sample").and_then(Value::as_u64) {
                Some(n) => htl::verify_filename_hashes_sampled(&paths, n as usize),
                None => htl::verify_filename_hashes(&paths),
            };
            return json!({"ret": ok});
        }
        "looks_like_service_key" => {
            return json!({"ret": hatate_core::hydrus_client::looks_like_service_key(a["value"].as_str().unwrap())});
        }
        "get_file_path_url" => {
            return match client {
                Ok(c) => {
                    json!({"ret": norm(&c.get_file_path_url(a["file_id"].as_i64().unwrap()), base, &dirs)})
                }
                Err(e) => fail(e),
            };
        }
        "apply" | "worker" => {
            let mut entries: Vec<Entry> = a["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    let mut ent =
                        Entry::new(dir.join(e["name"].as_str().unwrap()).to_str().unwrap());
                    ent.hydrus_hash = e["hash"].as_str().map(str::to_owned);
                    ent
                })
                .collect();
            for ent in &mut entries {
                for p in a["pre"].as_array().unwrap() {
                    ent.add_tags(
                        vec![Tag {
                            name: p["name"].as_str().unwrap().to_owned(),
                            namespace: p["namespace"].as_str().map(str::to_owned),
                            source: TagSource::parse(p["source"].as_str().unwrap()).unwrap(),
                        }],
                        None,
                    );
                }
            }
            let mut s = Settings::with_defaults(&sb.paths);
            s.hydrus.api_url = settings["api_url"]
                .as_str()
                .unwrap()
                .replace("{BASE}", base);
            s.hydrus.access_key = Secret::new(settings["access_key"].as_str().unwrap());
            s.hydrus.verify_ssl = settings["verify_ssl"].as_bool().unwrap();
            s.hydrus.timeout = settings["timeout"].as_f64().unwrap();
            s.enable_tag_namespace_remap = a["settings"]["enable_tag_namespace_remap"]
                .as_bool()
                .unwrap();
            s.tag_namespace_remap = OrderedMap(
                a["settings"]["tag_namespace_remap"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                    .collect(),
            );
            let res = if fnname == "apply" {
                match htl::apply_existing_hydrus_tags(&mut entries, &s).await {
                    Ok(r) => r,
                    Err(e) => {
                        return json!({"raw": {"cls": infer_class(e.message()), "message": norm(e.message(), base, &dirs)}});
                    }
                }
            } else {
                htl::run_tag_lookup(&mut entries, &s).await
            };
            return json!({"ret": {
                "tagged": res.tagged_count,
                "error": res.error.map(|m| norm(&m, base, &dirs)),
                "entries": entries.iter().map(entry_state).collect::<Vec<_>>(),
            }});
        }
        _ => {}
    }
    let client = match client {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    match fnname {
        "get_thumbnail" => match client.fetch_thumbnail(a["hash"].as_str().unwrap()).await {
            Ok(bytes) => json!({"ret_b64": b64(&bytes)}),
            Err(e) => {
                let cause = match e.kind() {
                    ErrorKind::Net(net::ErrorKind::Timeout) => json!(["exc", "timeout"]),
                    ErrorKind::Net(net::ErrorKind::InvalidRequest) => Value::Null,
                    ErrorKind::Net(_) => json!(["exc", "connect"]),
                    _ => json!(["status", e.status_code()]),
                };
                json!({"none": cause})
            }
        },
        "download_file" => {
            let dest = a["dest"].as_str().unwrap().replace("{DIR}", &dirs);
            match client
                .download_file(a["file_id"].as_i64().unwrap(), std::path::Path::new(&dest))
                .await
            {
                Ok(()) => json!({"ret": null}),
                Err(e) => fail(&e),
            }
        }
        "post_json" => {
            let extra: Vec<(String, String)> = a["extra"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let extra_ref: Vec<(&str, &str)> = extra
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let body = if a["body"].is_null() {
                None
            } else {
                Some(a["body"].clone())
            };
            match client
                .post_json(a["path"].as_str().unwrap(), body.as_ref(), &extra_ref)
                .await
            {
                Ok(v) => json!({"ret": v}),
                Err(e) => fail(&e),
            }
        }
        "post_raw" => {
            let extra: Vec<(String, String)> = a["extra"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let extra_ref: Vec<(&str, &str)> = extra
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            match client
                .post_raw(
                    a["path"].as_str().unwrap(),
                    bytes::Bytes::from(unb64(a["data_b64"].as_str().unwrap())),
                    &extra_ref,
                )
                .await
            {
                Ok(v) => json!({"ret": v}),
                Err(e) => fail(&e),
            }
        }
        "get_file_metadata" => {
            let ids: Vec<i64> = a["file_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_i64().unwrap())
                .collect();
            match client.get_file_metadata(&ids).await {
                Ok(rows) => {
                    json!({"ret": rows.iter().map(|x| json!([x.file_id, x.hash, x.mime, x.width, x.height, x.tags])).collect::<Vec<_>>()})
                }
                Err(e) => fail(&e),
            }
        }
        "get_tags_and_sizes_for_hashes" => match client
            .get_tags_and_sizes_for_hashes(&strings(&a["hashes"]))
            .await
        {
            Ok(m) => {
                json!({"ret": m.iter().map(|(h, i)| json!([h, [i.tags, i.size]])).collect::<Vec<_>>()})
            }
            Err(e) => fail(&e),
        },
        "get_tags_for_hashes" => match client.get_tags_for_hashes(&strings(&a["hashes"])).await {
            Ok(m) => json!({"ret": m.iter().map(|(h, t)| json!([h, t])).collect::<Vec<_>>()}),
            Err(e) => fail(&e),
        },
        "filter_known_hashes" => match client.filter_known_hashes(&strings(&a["hashes"])).await {
            Ok(s) => json!({"ret": s.into_iter().collect::<Vec<_>>()}),
            Err(e) => fail(&e),
        },
        "list_tag_services" => match client.list_tag_services().await {
            Ok(v) => json!({"ret": v.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>()}),
            Err(e) => fail(&e),
        },
        "resolve_to_service_key" => match client
            .resolve_to_service_key(a["value"].as_str().unwrap())
            .await
        {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "test_connection" => match client.test_connection().await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "get_services" => match client.get_services().await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "search_files" => match client.search_files(&strings(&a["tags"])).await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "missing_permission" => match client
            .missing_permission(a["permission_id"].as_i64().unwrap())
            .await
        {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "file_id_for_hash" => match client.file_id_for_hash(a["hash"].as_str().unwrap()).await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "get_url_info" => match client.get_url_info(a["url"].as_str().unwrap()).await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        "get_url_files" => match client.get_url_files(a["url"].as_str().unwrap()).await {
            Ok(v) => json!({"ret": v}),
            Err(e) => fail(&e),
        },
        other => panic!("unknown op {other}"),
    }
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
    let st = &case["settings"];
    let hs = HydrusSettings {
        api_url: st["api_url"].as_str().unwrap().replace("{BASE}", &base),
        access_key: Secret::new(st["access_key"].as_str().unwrap()),
        verify_ssl: st["verify_ssl"].as_bool().unwrap(),
        timeout: st["timeout"].as_f64().unwrap(),
        tag_service_key: st["tag_service_key"].as_str().unwrap().to_owned(),
    };
    let client = HydrusClient::new(&hs);
    let mut ops = Vec::new();
    for op in case["ops"].as_array().unwrap() {
        ops.push(run_op(op, &client, st, &base, dir.path(), sb).await);
    }
    let dirs = dir.path().to_str().unwrap().to_owned();
    let requests: Vec<Value> = server
        .recorded()
        .iter()
        .map(|q| {
            json!({
                "method": q.method,
                "target": norm(&q.target, &base, &dirs),
                "key": q.header("hydrus-client-api-access-key"),
                "ctype": q.header("content-type"),
                "xextra": q.header("x-extra").or(q.header("x-a")),
                "body_b64": if q.body.is_empty() { String::new() } else { b64(&q.body) },
                "netloc_is_base": true, "timeout_ok": true, "verify_ok": true,
            })
        })
        .collect();
    let mut files = Map::new();
    for name in ["out.bin", "o.bin"] {
        if let Ok(bytes) = std::fs::read(dir.path().join(name)) {
            files.insert(name.to_owned(), json!(b64(&bytes)));
        }
    }
    json!({"ops": ops, "requests": requests, "files": files, "sleeps": []})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
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
