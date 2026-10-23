//! DAN-2163 differential, Rust side (copied into crates/hatate-core/tests/ by the runner, never
//! committed). Reads DAN2163_CASES/cases.json, writes DAN2163_OUT.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hatate_core::clock::{BoxFuture, Clock, ManualClock};
use hatate_core::engine_alerts::{EngineAlert, EngineAlerts};
use hatate_core::engine_runner::LensError;
use hatate_core::engines::google_lens::GoogleLensMatch;
use hatate_core::engines::lens_prep::{self, LENS_READS, MAX_UPLOAD_BYTES};
use hatate_core::engines::lens_rank::{self, LensSites};
use hatate_core::engines::lens_search::{LensClient, LensFetch, LensFetchError, LensMissing, LensPayloads};
use hatate_core::image_compare::Hash;
use hatate_core::image_prep::{DEFAULT_MAX_DIMENSION, prepare_upload_bytes};
use hatate_core::pyraster;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

fn sha(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn head(b: &[u8]) -> String {
    b.iter().take(3).map(|x| format!("{x:02x}")).collect()
}

fn dims_mode(data: &[u8]) -> (Value, Value) {
    match pyraster::open(data, None) {
        Ok(im) => {
            let mode = match im.mode {
                pyraster::Mode::L => "L",
                pyraster::Mode::La => "LA",
                pyraster::Mode::Rgb => "RGB",
                pyraster::Mode::Rgba => "RGBA",
                pyraster::Mode::Cmyk => "CMYK",
            };
            (json!([im.width, im.height]), json!(mode))
        }
        Err(_) => (Value::Null, Value::Null),
    }
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn mk(d: &Value) -> GoogleLensMatch {
    GoogleLensMatch {
        url: d["url"].as_str().unwrap().to_owned(),
        thumb_url: opt_str(&d["thumb_url"]),
        similarity: d["similarity"].as_f64().unwrap(),
        title: opt_str(&d["title"]),
        source_name: opt_str(&d["source_name"]),
        width: d["width"].as_i64(),
        height: d["height"].as_i64(),
    }
}

fn match_json(m: &GoogleLensMatch) -> Value {
    json!({"url": m.url, "thumb_url": m.thumb_url, "similarity": m.similarity, "title": m.title,
           "source_name": m.source_name, "width": m.width, "height": m.height})
}

fn tag_idx(ms: &[Value], f: impl FnOnce(Vec<GoogleLensMatch>) -> Result<Vec<GoogleLensMatch>, lens_rank::Unexpected>) -> Value {
    // Matches are told apart by a unique `title` stamped on the way in.
    let mut owned: Vec<GoogleLensMatch> = ms.iter().map(mk).collect();
    for (i, m) in owned.iter_mut().enumerate() {
        m.title = Some(format!("#{i}"));
    }
    match f(owned) {
        Ok(out) => Value::Array(out.iter().map(|m| json!(m.title.as_ref().unwrap()[1..].parse::<usize>().unwrap())).collect()),
        Err(_) => json!("Unexpected"),
    }
}

fn site_r34(tags: &str) -> Option<(String, Option<String>, i64)> {
    let n = tags.bytes().map(u32::from).sum::<u32>() % 3;
    let chars = tags.chars().count();
    match n {
        0 => None,
        1 => Some((format!("https://rule34.xxx/index.php?page=post&s=view&id={chars}"), None, 3)),
        _ => Some((
            format!("https://rule34.xxx/index.php?page=post&s=view&id={}", chars + 1000),
            Some("https://img.example/t.jpg".to_owned()),
            7,
        )),
    }
}

struct Sites {
    calls: Arc<Mutex<Vec<Value>>>,
}

impl LensSites for Sites {
    fn find_rule34_post(&self, tags: &str, local_hash: Option<&Hash>) -> Option<(String, Option<String>, i64)> {
        self.calls.lock().unwrap().push(json!(["rule34", tags, local_hash.map(|h| format!("0x{}", h.to_hex()))]));
        site_r34(tags)
    }

    fn find_reddit_post(&self, subreddit: &str, media_id: &str) -> Option<String> {
        self.calls.lock().unwrap().push(json!(["reddit", subreddit, media_id]));
        let n = format!("{subreddit}/{media_id}").bytes().map(u32::from).sum::<u32>() % 3;
        if n == 0 { Some(format!("https://www.reddit.com/r/{subreddit}/comments/{media_id}/x/")) } else { None }
    }
}

struct Browser {
    answer: Mutex<Option<Value>>,
    calls: Arc<Mutex<Vec<Value>>>,
}

impl LensFetch for Browser {
    fn fetch_results_payloads<'a>(
        &'a self,
        image: &'a [u8],
        filename: &'a str,
        timeout: f64,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        self.calls.lock().unwrap().push(json!([image.len(), sha(image), head(image), filename, timeout, whole_image]));
        let a = self.answer.lock().unwrap().clone().unwrap();
        let out = if let Some(ok) = a.get("ok") {
            Ok(LensPayloads {
                payloads: ok["payloads"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_owned()).collect(),
                tiles: ok["tiles"].clone(),
            })
        } else {
            let message = a["message"].as_str().unwrap().to_owned();
            match a["exc"].as_str().unwrap() {
                "browser" => Err(LensFetchError::Browser(message)),
                "challenge" => Err(LensFetchError::ChallengeUnanswered(message)),
                _ => Err(LensFetchError::Unavailable {
                    message,
                    missing: match a.get("missing").and_then(Value::as_str) {
                        Some("playwright") => Some(LensMissing::Playwright),
                        Some("chromium") => Some(LensMissing::Chromium),
                        _ => None,
                    },
                }),
            }
        };
        Box::pin(std::future::ready(out))
    }
}

fn err_json(e: &LensError) -> Value {
    match e {
        LensError::Unavailable(m) => json!(["Unavailable", m]),
        LensError::Blocked(m) => json!(["Blocked", m]),
        LensError::Failed(m) => json!(["Failed", m]),
        LensError::Unexpected(_) => json!(["Unexpected", ""]),
    }
}

async fn run_scenario(sc: &Value, img: &Path) -> Value {
    let clock = Arc::new(ManualClock::new(1_000_000.0, 500.0));
    let alerts = Arc::new(EngineAlerts::new());
    let raised: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&raised);
    alerts.subscribe(Arc::new(move |a: &EngineAlert| {
        sink.lock().unwrap().push(json!([a.key, a.engine, a.title, a.body, a.remedy]));
    }));
    let fetch_calls = Arc::new(Mutex::new(Vec::new()));
    let site_calls = Arc::new(Mutex::new(Vec::new()));
    let browser = Arc::new(Browser { answer: Mutex::new(None), calls: Arc::clone(&fetch_calls) });
    struct Shared(Arc<Browser>);
    impl LensFetch for Shared {
        fn fetch_results_payloads<'a>(&'a self, i: &'a [u8], f: &'a str, t: f64, w: bool) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
            self.0.fetch_results_payloads(i, f, t, w)
        }
    }
    let client = LensClient::with_fetch(Arc::new(Shared(Arc::clone(&browser))))
        .with_clock(Arc::clone(&clock) as Arc<dyn Clock>)
        .with_alerts(Arc::clone(&alerts))
        .with_sites(Arc::new(Sites { calls: Arc::clone(&site_calls) }))
        .running_as_appimage(sc["appimage"].as_bool().unwrap());

    let mut log = Vec::new();
    let mut seen_sleeps = 0usize;
    for step in sc["steps"].as_array().unwrap() {
        fetch_calls.lock().unwrap().clear();
        site_calls.lock().unwrap().clear();
        raised.lock().unwrap().clear();
        let mut rec = Map::new();
        let op = step["op"].as_str().unwrap();
        rec.insert("op".into(), json!(op));
        match op {
            "search" => {
                *browser.answer.lock().unwrap() = Some(step["fetch"].clone());
                let path = img.join(step["image"].as_str().unwrap());
                let res = client.search(&path, step["timeout"].as_f64().unwrap(), None, step["whole"].as_bool().unwrap()).await;
                rec.insert(
                    "result".into(),
                    match res {
                        Ok(ms) => json!({"ok": ms.iter().map(match_json).collect::<Vec<_>>()}),
                        Err(e) => json!({"err": err_json(&e)}),
                    },
                );
            }
            "advance" => clock.advance(step["seconds"].as_f64().unwrap()),
            "reset" => client.reset_blocked_flag(),
            "end_rest" => client.end_rest(),
            "wait" => {
                let ticks = Arc::new(Mutex::new(Vec::new()));
                let calls = Arc::new(Mutex::new(0usize));
                let cap = step["cap"].as_u64();
                let on_tick: Option<hatate_core::engines::client::OnTick> = if step["ticks"].as_bool().unwrap() {
                    let t = Arc::clone(&ticks);
                    Some(Arc::new(move |l: &str, left: f64, total: f64| t.lock().unwrap().push(json!([l, left, total]))))
                } else {
                    None
                };
                let should_stop: Option<hatate_core::engines::saucenao::ShouldStop> = cap.map(|cap| {
                    let c = Arc::clone(&calls);
                    Arc::new(move || {
                        let mut n = c.lock().unwrap();
                        *n += 1;
                        *n as u64 > cap
                    }) as hatate_core::engines::saucenao::ShouldStop
                });
                client.wait_for_turn(on_tick, should_stop).await;
                rec.insert("ticks".into(), Value::Array(ticks.lock().unwrap().clone()));
                rec.insert("stop_calls".into(), json!(*calls.lock().unwrap()));
            }
            other => panic!("{other}"),
        }
        rec.insert("fetch".into(), Value::Array(fetch_calls.lock().unwrap().clone()));
        rec.insert("sites".into(), Value::Array(site_calls.lock().unwrap().clone()));
        let sleeps = clock.sleeps();
        rec.insert("sleeps".into(), json!(sleeps[seen_sleeps..]));
        seen_sleeps = sleeps.len();
        rec.insert("alerts".into(), Value::Array(raised.lock().unwrap().clone()));
        rec.insert("state".into(), json!([client.is_blocked(), client.is_resting(), client.resting_until(), clock.monotonic()]));
        log.push(Value::Object(rec));
    }
    Value::Array(log)
}

#[tokio::test(flavor = "multi_thread")]
async fn differential() {
    let dir = PathBuf::from(std::env::var("DAN2163_CASES").unwrap());
    let out = PathBuf::from(std::env::var("DAN2163_OUT").unwrap());
    let cases: Value = serde_json::from_slice(&std::fs::read(dir.join("cases.json")).unwrap()).unwrap();
    let img = dir.join("img");
    let mut res = Map::new();

    res.insert(
        "media".into(),
        Value::Array(cases["media"].as_array().unwrap().iter().map(|h| json!(lens_prep::media_type(&unhex(h.as_str().unwrap())))).collect()),
    );

    let mut prep = Map::new();
    let mut anim = Map::new();
    for name in cases["prep"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let path = img.join(name);
        let entry = match lens_prep::prepare_for_lens(&path) {
            Ok(up) => {
                let (dims, mode) = dims_mode(&up.data);
                // The pixels `_to_jpeg` was handed: only when this picture went down that road.
                let flat = match prepare_upload_bytes(&path, MAX_UPLOAD_BYTES, DEFAULT_MAX_DIMENSION) {
                    Ok((data, _)) => {
                        let mime = lens_prep::media_type(&data);
                        let direct = mime.is_some_and(|m| LENS_READS.contains(&m)) && !lens_prep::is_animated(&data);
                        if direct {
                            Value::Null
                        } else {
                            match lens_prep::flattened_rgb(&data) {
                                Some((w, h, rgb)) => json!([w, h, "RGB", sha(&rgb)]),
                                None => Value::Null,
                            }
                        }
                    }
                    Err(_) => Value::Null,
                };
                json!({"ok": {"mime": up.media_type, "filename": up.filename, "len": up.data.len(), "sha": sha(&up.data),
                              "head": head(&up.data), "dims": dims, "mode": mode, "flat": flat}})
            }
            Err(lens_prep::PrepareError::Refused(m)) => json!({"refused": m}),
            Err(e @ lens_prep::PrepareError::Unreadable { .. }) => json!({"os": e.detail()}),
        };
        prep.insert(name.to_owned(), entry);
        if path.is_file() {
            let data = std::fs::read(&path).unwrap();
            let jpeg = lens_prep::to_jpeg(&data);
            let flat = lens_prep::flattened_rgb(&data).map(|(w, h, rgb)| json!([w, h, "RGB", sha(&rgb)]));
            anim.insert(
                name.to_owned(),
                json!({"animated": lens_prep::is_animated(&data),
                       "to_jpeg": jpeg.as_ref().map(|j| json!({"flat": flat, "dims": dims_mode(j).0}))}),
            );
        }
    }
    res.insert("prep".into(), Value::Object(prep));
    res.insert("anim".into(), Value::Object(anim));

    let mut ups = Vec::new();
    for u in cases["upload"].as_array().unwrap() {
        let file = u["file"].as_str().unwrap();
        let data = if file.is_empty() { Vec::new() } else { std::fs::read(img.join(file)).unwrap() };
        let s = lens_prep::upload_script(&data, u["filename"].as_str().unwrap());
        ups.push(json!({"len": s.chars().count(), "sha": sha(s.as_bytes()), "ascii": s.is_ascii()}));
    }
    res.insert("upload".into(), Value::Array(ups));

    res.insert("r34".into(), Value::Array(cases["r34"].as_array().unwrap().iter().map(|t| json!(lens_rank::rule34_searches(t))).collect()));
    res.insert(
        "taggable".into(),
        Value::Array(cases["taggable"].as_array().unwrap().iter().map(|ms| tag_idx(ms.as_array().unwrap(), lens_rank::taggable_first)).collect()),
    );
    res.insert(
        "keep".into(),
        Value::Array(
            cases["keep"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    tag_idx(c["matches"].as_array().unwrap(), |ms| {
                        lens_rank::keep_taggable_first(
                            ms,
                            c["limit"].as_u64().unwrap() as usize,
                            c["visual_start"].as_u64().unwrap() as usize,
                            c["reserve"].as_u64().unwrap() as usize,
                        )
                    })
                })
                .collect(),
        ),
    );
    res.insert(
        "timeouts".into(),
        Value::Array(cases["timeouts"].as_array().unwrap().iter().map(|t| json!(lens_rank::effective_timeout(t.as_f64().unwrap()))).collect()),
    );

    res.insert("localtime".into(), Value::Array(cases["localtime"].as_array().unwrap().iter().map(|t| json!(hatate_core::localtime::local_hhmm(t.as_f64().unwrap()))).collect()));

    let mut scenarios = Vec::new();
    for sc in cases["scenarios"].as_array().unwrap() {
        scenarios.push(run_scenario(sc, &img).await);
    }
    res.insert("scenarios".into(), Value::Array(scenarios));
    std::fs::write(out, serde_json::to_vec(&Value::Object(res)).unwrap()).unwrap();
    let _ = VecDeque::<u8>::new();
}
