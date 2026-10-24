//! The MCP tool bodies (`gui/mcp_bridge.py`'s `McpToolHandlers`) and `core/mcp_images.py`
//! (Phase 5.e, DAN-2167) against a queue model on its own owner thread. Hydrus is a scripted
//! local server (plain http, as `hydrus.api_url` ships), the matches' CDN is a scripted local
//! https server through the net layer, every access key is a synthetic placeholder, and no live
//! service is contacted.
//!
//! The wire and transport differentials against the real Python are the broad check
//! (`worktrees/DAN-2167-differential.log`); these are the named, readable ones. The Python test
//! each one maps to is in `docs/mcp-handlers.md`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Reply, ScriptedEngines, Step, TestPki, TestServer, iqdb_match};
use hatate_core::batch::Silent;
use hatate_core::batch::research::BatchSearchLauncher;
use hatate_core::clock::ManualClock;
use hatate_core::config::{Secret, Settings};
use hatate_core::engine_runner::EngineRunner;
use hatate_core::hydrus_client::HydrusClient;
use hatate_core::mcp::handlers::{HandlerResult, McpHandlers};
use hatate_core::mcp::tool_handlers::{HandlerEnv, McpToolHandlers, Timeouts};
use hatate_core::model::{Candidate, Entry, MatchStatus};
use hatate_core::net::{NetClient, NetConfig};
use hatate_core::paths::Paths;
use hatate_core::queue::{
    EntryId, OwnerThread, QueueChannel, QueueModel, SearchDone, SearchLauncher, spawn_owner_thread,
};
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;
use serde_json::{Value, json};

const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const POST: &str = r#"{"id": 5, "image_width": 800, "image_height": 600, "rating": "g",
    "file_url": "https://cdn.donmai.us/f.png",
    "tag_string_general": "one two three four five six", "tag_string_artist": "someone"}"#;
const BOORU_URL: &str = "https://danbooru.donmai.us/posts/5";
const NOW: f64 = 1_792_725_790.0;

fn png(color: [u8; 3], size: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(size, size, image::Rgb(color));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A scripted Hydrus: add_file answers with the hash of what was sent; everything else with `{}`.
fn hydrus_answer(req: &common::Recorded) -> Reply {
    use sha2::{Digest, Sha256};
    let path = req.target.split('?').next().unwrap_or("");
    let body = match path {
        "/add_files/add_file" => {
            // The hash of what was sent: a path in a JSON body, or the bytes themselves.
            let sent = serde_json::from_slice::<Value>(&req.body)
                .ok()
                .and_then(|v| v["path"].as_str().map(str::to_owned))
                .and_then(|p| std::fs::read(p).ok())
                .unwrap_or_else(|| req.body.clone());
            let hash = format!("{:x}", Sha256::digest(&sent));
            json!({"status": 1, "hash": hash, "note": ""})
        }
        "/add_urls/get_url_info" => json!({"url_type": 0}),
        "/add_urls/add_url" => json!({"human_result_text": "URL added successfully"}),
        _ => json!({}),
    };
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}

/// A launcher that records what it was given and finishes at once with a changed copy.
#[derive(Default)]
struct Scripted {
    launched: Mutex<Vec<(Vec<String>, Vec<usize>)>>,
    never_finish: AtomicBool,
    refuse: AtomicBool,
}

impl SearchLauncher for Scripted {
    fn launch(
        &self,
        batch: Vec<(EntryId, Entry)>,
        pending: Vec<usize>,
        done: SearchDone,
    ) -> Result<(), String> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err("no".into());
        }
        let names = batch.iter().map(|(_, e)| e.filename().to_owned()).collect();
        self.launched.lock().unwrap().push((names, pending));
        if self.never_finish.load(Ordering::SeqCst) {
            std::mem::forget(done);
            return Ok(());
        }
        let results = batch
            .into_iter()
            .map(|(id, mut e)| {
                e.status = MatchStatus::Good;
                e.candidates = vec![Candidate::new("https://example.test/found")];
                (id, e)
            })
            .collect();
        std::thread::spawn(move || done.finish(results, vec![]));
        Ok(())
    }
}

struct World {
    _dir: tempfile::TempDir,
    settings: Arc<Mutex<Settings>>,
    handlers: Arc<McpToolHandlers>,
    owner: Option<OwnerThread>,
    channel: QueueChannel,
    hydrus: TestServer,
    cdn: TestServer,
    launcher: Arc<Scripted>,
    engine: Arc<SearchEngine>,
    engines: Arc<ScriptedEngines>,
    audit: PathBuf,
}

struct Opts {
    timeouts: Timeouts,
    hydrus_timeout: f64,
    real_launcher: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            timeouts: Timeouts::default(),
            hydrus_timeout: 5.0,
            real_launcher: false,
        }
    }
}

async fn world(entries: impl FnOnce(&Path) -> Vec<Entry>) -> World {
    world_with(Opts::default(), entries).await
}

async fn world_with(opts: Opts, entries: impl FnOnce(&Path) -> Vec<Entry>) -> World {
    let hosts = ["danbooru.donmai.us", "cdn.donmai.us"];
    let pki = TestPki::generate_for(&hosts);
    let cdn = TestServer::https(&pki, |req| {
        let path = req.target.split('?').next().unwrap_or("");
        match path {
            "/posts/5.json" => Reply::ok(POST).header("Content-Type", "application/json"),
            "/f.png" => Reply::ok(png([200, 10, 10], 16)).header("Content-Type", "image/png"),
            "/sample.png" => Reply::ok(png([10, 10, 200], 8)).header("Content-Type", "image/png"),
            "/slow.png" => Reply::ok(png([1, 2, 3], 4)).delayed(Duration::from_secs(2)),
            _ => Reply::new(404, "no route"),
        }
    })
    .await;
    let hydrus = TestServer::http(hydrus_answer).await;
    let mut cfg = NetConfig::default().with_root_certificate_der(pki.ca_der.clone());
    for host in hosts {
        cfg = cfg.with_host_override(host, cdn.addr);
    }
    let net = NetClient::new(cfg).unwrap();
    let dir = tempfile::Builder::new()
        .prefix("dan2167-handlers-")
        .tempdir()
        .unwrap();
    let root = dir.path().to_path_buf();
    let home: std::ffi::OsString = root.join("home").into();
    let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap();
    let clock = Arc::new(ManualClock::new(NOW, 0.0));
    let engines = Arc::new(ScriptedEngines::default());
    let runner = EngineRunner::new(engines.clone(), clock.clone());
    let engine = Arc::new(SearchEngine::with_recorder(
        net,
        runner,
        paths.clone(),
        clock.clone(),
        None,
    ));
    let mut s = Settings::with_defaults(&paths);
    s.hydrus.api_url = hydrus.url("");
    s.hydrus.access_key = Secret::new(KEY);
    s.hydrus.timeout = opts.hydrus_timeout;
    s.primary_engine = "iqdb".to_owned();
    s.secondary_engine_mode = "disabled".to_owned();
    s.enable_ascii2d = false;
    s.enable_tracemoe = false;
    s.enable_pawchive_index = false;
    s.delay_min_seconds = 0.0;
    s.delay_max_seconds = 0.0;
    let settings = Arc::new(Mutex::new(s));
    let model = QueueModel::new(entries(&root), settings.clone());
    let (owner, channel) = spawn_owner_thread(model);
    let launcher = Arc::new(Scripted::default());
    let runtime = tokio::runtime::Handle::current();
    let launcher_dyn: Arc<dyn SearchLauncher> = if opts.real_launcher {
        Arc::new(BatchSearchLauncher {
            engine: engine.clone(),
            settings: settings.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            quota_exhausted: Arc::new(|| false),
            quota_file: QuotaPauseFile::new(&paths),
            reporter: Arc::new(Silent),
            unit_random: Arc::new(|| 0.5),
            reset_run_state: Arc::new(|| {}),
            hydrus_client: Arc::new(HydrusClient::new),
            runtime: runtime.clone(),
        })
    } else {
        launcher.clone()
    };
    let audit = root.join("mcp_audit.jsonl");
    let handlers = Arc::new(McpToolHandlers::new(
        channel.clone(),
        HandlerEnv {
            timeouts: opts.timeouts,
            settings: settings.clone(),
            engine: engine.clone(),
            hydrus_client: Arc::new(HydrusClient::new),
            launcher: launcher_dyn,
            runtime,
            audit_path: audit.clone(),
        },
    ));
    World {
        _dir: dir,
        settings,
        handlers,
        owner: Some(owner),
        channel,
        hydrus,
        cdn,
        launcher,
        engine,
        engines,
        audit,
    }
}

impl World {
    /// Runs a handler on a blocking thread, as the server does.
    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&McpToolHandlers) -> T + Send + 'static,
    ) -> T {
        let h = self.handlers.clone();
        tokio::task::spawn_blocking(move || f(&h)).await.unwrap()
    }

    fn entries(&self) -> Vec<Entry> {
        self.channel
            .call(|m| Ok(m.entries().to_vec()), 5.0)
            .unwrap()
    }

    fn audit_lines(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.audit)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn hydrus_paths(&self) -> Vec<String> {
        self.hydrus
            .recorded()
            .iter()
            .map(|q| q.target.split('?').next().unwrap().to_owned())
            .collect()
    }

    fn finish(mut self) -> QueueModel {
        self.owner.take().unwrap().stop()
    }
}

fn entry(name: &str) -> Entry {
    Entry::new(format!("/tmp/{name}"))
}

fn j(out: HandlerResult) -> Value {
    out.expect("a handler result").to_json()
}

fn with_png(root: &Path, name: &str, color: [u8; 3]) -> Entry {
    let path = root.join(name);
    std::fs::write(&path, png(color, 16)).unwrap();
    Entry::new(path.to_string_lossy())
}

// ---------------------------------------------------------------------------------------------
// TestReadTools
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn list_queue_reports_every_row_by_default() {
    let w = world(|_| vec![entry("a.jpg"), entry("b.jpg")]).await;
    let r = j(w.call(|h| h.list_queue(0, 50, None)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["rows"][0]["filename"], "a.jpg");
    assert_eq!(
        (r["total_matching"].clone(), r["total_in_queue"].clone()),
        (json!(2), json!(2))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_queue_respects_offset_and_limit() {
    let w = world(|_| (0..5).map(|i| entry(&format!("{i}.jpg"))).collect()).await;
    let r = j(w.call(|h| h.list_queue(2, 2, None)).await);
    let names: Vec<&str> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["filename"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["2.jpg", "3.jpg"]);
    assert_eq!(r["total_matching"], 5, "rows past the page still count");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_queue_filter_keyword_unreviewed() {
    let w = world(|_| {
        let mut reviewed = entry("reviewed.jpg");
        reviewed.reviewed = true;
        vec![reviewed, entry("pending.jpg")]
    })
    .await;
    let r = j(w
        .call(|h| h.list_queue(0, 50, Some("unreviewed".into())))
        .await);
    assert_eq!(r["rows"].as_array().unwrap().len(), 1);
    assert_eq!(r["rows"][0]["filename"], "pending.jpg");
    assert_eq!(
        r["rows"][0]["row"], 1,
        "the row is the index into the FULL queue"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_entry_reports_the_requested_row() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    let r = j(w.call(|h| h.get_entry(0)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["filename"], "a.jpg");
    assert_eq!(r["status"], "not_searched");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_entry_out_of_range_reports_an_error_not_a_crash() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    let r = j(w.call(|h| h.get_entry(5)).await);
    assert_eq!(
        r["allowed"], true,
        "the TOOL was permitted; the ROW was invalid"
    );
    assert_eq!(r["error"], "No row 5 (queue has 1)");
    let r = j(w.call(|h| h.get_entry(-1)).await);
    assert_eq!(r["error"], "No row -1 (queue has 1)");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_actions_reports_every_tool_with_its_gate_state() {
    let w = world(|_| vec![]).await;
    let r = j(w.call(|h| h.list_actions()).await);
    let tools = r["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 14);
    let research = tools.iter().find(|t| t["tool"] == "research").unwrap();
    assert_eq!(
        research["enabled"], false,
        "allow_research is off by default"
    );
    assert!(research["refusal_reason"].is_string());
    let send = tools.iter().find(|t| t["tool"] == "send_upload").unwrap();
    assert_eq!(send["enabled"], true, "allow_hydrus_writes defaults on");
    assert!(send["refusal_reason"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_orders_by_score_with_the_breakdown() {
    let w = world(|_| {
        let mut e = entry("a.jpg");
        let mut weak = Candidate::new("https://example.test/weak");
        weak.similarity = 70.0;
        let mut strong = Candidate::new("https://example.test/strong");
        strong.similarity = 90.0;
        strong.remote_available = Some(false);
        let mut mid = Candidate::new("https://example.test/mid");
        mid.similarity = 80.0;
        e.candidates = vec![weak, strong, mid];
        e.selected_candidate_index = 2;
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.get_candidates(0)).await);
    let order: Vec<i64> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["candidate_index"].as_i64().unwrap())
        .collect();
    assert_eq!(order, [2, 0, 1], "the penalised 90 sorts last");
    assert_eq!(r["candidates"][0]["is_selected"], true);
    assert_eq!(r["candidates"][2]["unavailable_penalty_applied"], true);
    assert_eq!(r["candidates"][2]["unavailable_penalty"], 1000.0);
}

// ---------------------------------------------------------------------------------------------
// TestLocalWrites
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn select_candidate_switches_the_active_match() {
    let w = world(|_| {
        let mut e = entry("a.jpg");
        e.candidates = vec![Candidate::new("https://a"), Candidate::new("https://b")];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.select_candidate(0, 1)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["selected_candidate_index"], 1);
    let e = &w.entries()[0];
    assert_eq!(e.selected_candidate_index, 1);
    assert_eq!(e.matched_url(), Some("https://b"));
    let r = j(w.call(|h| h.select_candidate(0, 2)).await);
    assert_eq!(r["error"], "No candidate 2 for row 0");
    let r = j(w.call(|h| h.select_candidate(3, 0)).await);
    assert_eq!(r["error"], "No row 3");
}

#[tokio::test(flavor = "multi_thread")]
async fn toggle_reviewed_sets_the_flag_and_local_writes_are_never_gated() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    let r = j(w.call(|h| h.toggle_reviewed(0, true)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["reviewed"], true);
    assert!(r.get("reason").is_none());
    assert!(w.entries()[0].reviewed);
    let r = j(w.call(|h| h.toggle_reviewed(0, false)).await);
    assert_eq!(r["reviewed"], false);
}

// ---------------------------------------------------------------------------------------------
// TestGatingAndAudit
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_tier_is_refused_with_the_setting_named_and_the_entry_survives() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    let r = j(w.call(|h| h.remove_row(0)).await);
    assert_eq!(r["allowed"], false);
    assert!(
        r["reason"]
            .as_str()
            .unwrap()
            .contains("\"Let it remove rows and reset results\"")
    );
    assert_eq!(
        w.entries().len(),
        1,
        "a refusal never falls through to doing the thing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hydrus_write_is_refused_without_calling_hydrus_at_all() {
    let w = world(|r| vec![with_png(r, "a.png", [1, 2, 3])]).await;
    w.settings.lock().unwrap().mcp.allow_hydrus_writes = false;
    let r = j(w.call(|h| h.send_upload(0)).await);
    assert_eq!(r["allowed"], false);
    assert!(
        r["reason"]
            .as_str()
            .unwrap()
            .contains("\"Let it send files and tags to Hydrus\"")
    );
    assert!(w.hydrus_paths().is_empty(), "the gate runs before the body");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_and_an_allowed_call_are_both_audited() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.call(|h| h.remove_row(0)).await.unwrap();
    w.call(|h| h.get_entry(0)).await.unwrap();
    let lines = w.audit_lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        (lines[0]["allowed"].clone(), lines[0]["tool"].clone()),
        (json!(false), json!("remove_row"))
    );
    assert_eq!(lines[0]["row"], 0);
    assert_eq!(lines[0]["tier"], "destructive");
    assert_eq!(lines[1]["allowed"], true);
    assert_eq!(lines[1]["outcome"]["filename"], "a.jpg");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_audit_log_redacts_a_token_in_the_arguments() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.call(|h| h.list_queue(0, 5, Some("x".into())))
        .await
        .unwrap();
    let line = &w.audit_lines()[0];
    assert_eq!(
        line["arguments"],
        json!({"offset": 0, "limit": 5, "filter": "x"})
    );
}

// ---------------------------------------------------------------------------------------------
// TestDryRun
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn dry_run_changes_nothing_says_so_and_is_audited() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    {
        let mut s = w.settings.lock().unwrap();
        s.mcp.allow_destructive = true;
        s.mcp.dry_run = true;
    }
    let r = j(w.call(|h| h.remove_row(0)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["dry_run"], true);
    assert_eq!(r["would_call"], "remove_row");
    assert_eq!(r["arguments"], json!({"row": 0}));
    assert_eq!(r["note"], "dry_run is on: nothing was changed.");
    assert_eq!(w.entries().len(), 1, "nothing was actually removed");
    assert_eq!(w.audit_lines()[0]["dry_run"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_dry_run_off_performs_the_real_action() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.settings.lock().unwrap().mcp.allow_destructive = true;
    let r = j(w.call(|h| h.remove_row(0)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["dry_run"], false);
    assert_eq!(r["removed"], "a.jpg");
    assert!(w.entries().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn read_tools_ignore_dry_run() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.settings.lock().unwrap().mcp.dry_run = true;
    let r = j(w.call(|h| h.get_entry(0)).await);
    assert_eq!(r["allowed"], true);
    assert_eq!(r["filename"], "a.jpg");
    assert_eq!(r["dry_run"], false);
}

// ---------------------------------------------------------------------------------------------
// Destructive tools
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn reset_result_clears_the_match_and_drops_the_cached_result() {
    let hash = "ab".repeat(32);
    let h2 = hash.clone();
    let w = world(move |_| {
        let mut e = entry("a.jpg");
        e.status = MatchStatus::Good;
        e.reviewed = true;
        e.hydrus_hash = Some(h2);
        e.candidates = vec![Candidate::new("https://a")];
        vec![e, entry("fresh.jpg")]
    })
    .await;
    w.settings.lock().unwrap().mcp.allow_destructive = true;
    let cache_file = w.engine.cache.dir().join(format!("{hash}.json"));
    std::fs::create_dir_all(cache_file.parent().unwrap()).unwrap();
    std::fs::write(&cache_file, b"{}").unwrap();
    let r = j(w.call(|h| h.reset_result(0)).await);
    assert_eq!(
        (r["reset"].clone(), r["dropped_from_cache"].clone()),
        (json!(true), json!(true))
    );
    assert!(!cache_file.exists());
    let e = &w.entries()[0];
    assert_eq!(
        (e.status, e.reviewed, e.candidates.len()),
        (MatchStatus::NotSearched, false, 0)
    );
    let r = j(w.call(|h| h.reset_result(0)).await);
    assert_eq!(r["reset"], false);
    assert_eq!(r["reason"], "Nothing to reset - not searched yet");
    let r = j(w.call(|h| h.reset_result(1)).await);
    assert_eq!(r["reason"], "Nothing to reset - not searched yet");
    let r = j(w.call(|h| h.reset_result(9)).await);
    assert_eq!(r["error"], "No row 9");
}

// ---------------------------------------------------------------------------------------------
// get_images / get_diff and core/mcp_images.py over https
// ---------------------------------------------------------------------------------------------

fn candidate_on_cdn(path: &str) -> Candidate {
    let mut c = Candidate::new("https://example.test/post/1");
    c.direct_file_url = Some(format!("https://cdn.donmai.us{path}"));
    c
}

#[tokio::test(flavor = "multi_thread")]
async fn get_images_returns_both_pictures_at_full_resolution_over_https() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [9, 9, 9]);
        e.candidates = vec![candidate_on_cdn("/f.png")];
        vec![e]
    })
    .await;
    let out = w.call(|h| h.get_images(0, 0)).await.unwrap();
    let blocks = hatate_core::mcp::content::to_content_blocks(&out);
    assert_eq!(blocks.len(), 3, "text, local image, match image");
    assert_eq!(blocks[2]["mimeType"], "image/png");
    let text: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["match_image"]["is_full_resolution"], true);
    assert_eq!(text["match_image"]["url"], "https://cdn.donmai.us/f.png");
    assert!(text["match_image"]["fallback_reason"].is_null());
    let reqs = w.cdn.recorded();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].header("referer"),
        Some("https://example.test/post/1"),
        "the direct file url is fetched with the post as its referer"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_preview_is_the_fallback_and_says_so_with_no_referer() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [9, 9, 9]);
        let mut c = Candidate::new("https://example.test/post/1");
        c.preview_url = Some("https://cdn.donmai.us/sample.png".into());
        e.candidates = vec![c];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.get_images(0, 0)).await);
    assert_eq!(r["match_image"]["is_full_resolution"], false);
    assert_eq!(
        r["match_image"]["fallback_reason"],
        "No direct full-resolution file URL was available; returned the preview/thumbnail instead."
    );
    assert!(w.cdn.recorded()[0].header("referer").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn match_details_are_fetched_when_neither_url_is_known_and_kept_on_the_candidate() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [9, 9, 9]);
        let mut c = Candidate::new(BOORU_URL);
        c.source_name = Some("danbooru".into());
        e.candidates = vec![c];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.get_images(0, 0)).await);
    assert_eq!(r["match_image"]["url"], "https://cdn.donmai.us/f.png");
    assert_eq!(r["match_image"]["is_full_resolution"], true);
    let kept = w
        .channel
        .call(|m| Ok(m.entries()[0].candidates[0].clone()), 5.0)
        .unwrap();
    assert_eq!(
        kept.direct_file_url.as_deref(),
        Some("https://cdn.donmai.us/f.png"),
        "Python fills the candidate in place; the session keeps it"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_url_at_all_is_reported_rather_than_attempted() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [9, 9, 9]);
        e.candidates = vec![Candidate::new("https://example.test/post/1")];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.get_images(0, 0)).await);
    assert!(r["match_image"]["url"].is_null());
    assert_eq!(
        r["match_image"]["fallback_reason"],
        "This match has no image URL at all."
    );
    assert_eq!(r["match_image"]["fetch_failed"], Value::Null);
    assert_eq!(w.cdn.request_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_download_is_reported_rather_than_crashing() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [9, 9, 9]);
        e.candidates = vec![candidate_on_cdn("/missing.png")];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.get_images(0, 0)).await);
    assert_eq!(r["match_image"]["url"], "https://cdn.donmai.us/missing.png");
    assert_eq!(
        r["match_image"]["fallback_reason"],
        "The fetch failed - see the app log."
    );
    assert_eq!(r["match_image"]["is_full_resolution"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_download_is_bounded_by_the_hydrus_timeout_setting() {
    let w = world_with(
        Opts {
            hydrus_timeout: 0.3,
            ..Opts::default()
        },
        |r| {
            let mut e = with_png(r, "local.png", [9, 9, 9]);
            e.candidates = vec![candidate_on_cdn("/slow.png")];
            vec![e]
        },
    )
    .await;
    let started = std::time::Instant::now();
    let r = j(w.call(|h| h.get_images(0, 0)).await);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        r["match_image"]["fallback_reason"],
        "The fetch failed - see the app log."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_local_file_is_a_fetch_failure_not_an_error() {
    let w = world(|_| {
        let mut e = entry("gone.png");
        e.candidates = vec![candidate_on_cdn("/f.png")];
        vec![e]
    })
    .await;
    let out = w.call(|h| h.get_images(0, 0)).await.unwrap();
    let blocks = hatate_core::mcp::content::to_content_blocks(&out);
    assert_eq!(blocks.len(), 2, "the match image only");
    let text: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["local_image"]["fetch_failed"], true);
    let line = serde_json::to_string(&w.audit_lines()).unwrap();
    assert!(!line.contains("/home"), "{line}");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_images_for_a_missing_row_or_candidate_is_an_error_result() {
    let w = world(|_| vec![entry("a.png")]).await;
    let r = j(w.call(|h| h.get_images(0, 3)).await);
    assert_eq!(r["error"], "No row 0 / candidate 3 to fetch");
    let r = j(w.call(|h| h.get_diff(4, 0)).await);
    assert_eq!(r["error"], "No row 4 / candidate 0 to compare");
}

#[tokio::test(flavor = "multi_thread")]
async fn get_diff_of_identical_pictures_reports_no_difference_with_a_png_mask() {
    let w = world(|r| {
        let mut e = with_png(r, "local.png", [200, 10, 10]);
        e.candidates = vec![candidate_on_cdn("/f.png")];
        vec![e]
    })
    .await;
    let out = w.call(|h| h.get_diff(0, 0)).await.unwrap();
    let blocks = hatate_core::mcp::content::to_content_blocks(&out);
    assert_eq!(blocks.len(), 2);
    let text: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert!(text["changed_percent"].as_f64().unwrap() < 1.0);
    assert_eq!(text["match_is_full_resolution"], true);
    assert_eq!(text["diff_image"]["mime_type"], "image/png");
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(blocks[1]["data"].as_str().unwrap())
        .unwrap();
    let mask = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
    assert_eq!(mask.color(), image::ColorType::L8);
}

#[test]
fn compute_diff_reports_missing_and_undecodable_input_rather_than_failing() {
    let data = png([1, 2, 3], 20);
    let r = hatate_core::mcp_images::compute_diff(None, Some(&data)).unwrap();
    assert!(r.png_bytes.is_none() && !r.reliable);
    assert_eq!(
        r.verdict,
        "Could not decode one or both images for comparison."
    );
    let r = hatate_core::mcp_images::compute_diff(Some(b"not an image"), Some(&data)).unwrap();
    assert!(r.png_bytes.is_none() && !r.reliable && r.changed_percent == 0.0);
    let same = hatate_core::mcp_images::compute_diff(Some(&data), Some(&data)).unwrap();
    assert!(same.png_bytes.is_some() && same.changed_percent < 1.0);
}

#[test]
fn local_image_bytes_reads_the_real_file_and_a_missing_one_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local.png");
    std::fs::write(&path, png([1, 2, 3], 4)).unwrap();
    assert_eq!(
        hatate_core::mcp_images::local_image_bytes(path.to_str().unwrap()),
        Some(png([1, 2, 3], 4))
    );
    assert_eq!(
        hatate_core::mcp_images::local_image_bytes("/nonexistent/does-not-exist.png"),
        None
    );
}

// ---------------------------------------------------------------------------------------------
// The Hydrus sends
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn send_upload_uploads_the_local_file_and_marks_the_entry_sent() {
    let w = world(|r| vec![with_png(r, "a.png", [5, 5, 5])]).await;
    let r = j(w.call(|h| h.send_upload(0)).await);
    assert_eq!(r["success"], true);
    assert!(r["error"].is_null() && r["warning"].is_null());
    assert_eq!(w.hydrus_paths(), ["/add_files/add_file"]);
    let e = &w.entries()[0];
    assert!(e.sent_to_hydrus && e.hydrus_import_confirmed);
    use sha2::{Digest, Sha256};
    let sent = format!("{:x}", Sha256::digest(std::fs::read(&e.path).unwrap()));
    assert_eq!(e.hydrus_hash.as_deref(), Some(sent.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn send_upload_removes_the_row_when_remove_after_import_is_on() {
    let w = world(|r| {
        vec![
            with_png(r, "a.png", [5, 5, 5]),
            with_png(r, "b.png", [6, 6, 6]),
        ]
    })
    .await;
    w.settings.lock().unwrap().remove_after_import = true;
    let r = j(w.call(|h| h.send_upload(0)).await);
    assert_eq!(r["success"], true);
    let names: Vec<String> = w
        .entries()
        .iter()
        .map(|e| e.filename().to_owned())
        .collect();
    assert_eq!(names, ["b.png"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_without_an_access_key_or_a_row_says_so_and_calls_nothing() {
    let w = world(|r| vec![with_png(r, "a.png", [5, 5, 5])]).await;
    w.settings.lock().unwrap().hydrus.access_key = Secret::new("");
    for r in [
        j(w.call(|h| h.send_upload(0)).await),
        j(w.call(|h| h.send_url(0)).await),
        j(w.call(|h| h.download_send(0)).await),
    ] {
        assert_eq!(r["success"], false);
        assert_eq!(r["error"], "No Hydrus access key configured");
    }
    let r = j(w.call(|h| h.send_url(7)).await);
    assert_eq!(r["error"], "No row 7");
    assert!(w.hydrus_paths().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn send_url_without_a_matched_url_is_skipped_and_with_one_is_queued() {
    let w = world(|r| {
        let mut with_match = with_png(r, "b.png", [6, 6, 6]);
        with_match.candidates = vec![Candidate::new("https://example.test/post/9.png")];
        vec![with_png(r, "a.png", [5, 5, 5]), with_match]
    })
    .await;
    let r = j(w.call(|h| h.send_url(0)).await);
    assert_eq!(r["success"], false);
    assert_eq!(r["skipped_reason"], "no matched URL");
    let r = j(w.call(|h| h.send_url(1)).await);
    assert_eq!(r["success"], true);
    assert!(r["confirmed"].is_null());
    assert_eq!(r["removed_from_queue"], false);
    assert_eq!(r["removal_pending_confirmation"], false);
    assert!(w.entries()[1].sent_to_hydrus);
    assert!(w.hydrus_paths().contains(&"/add_urls/add_url".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn download_send_downloads_the_match_and_uploads_it() {
    let w = world(|r| {
        let mut e = with_png(r, "a.png", [5, 5, 5]);
        e.candidates = vec![candidate_on_cdn("/f.png")];
        vec![e]
    })
    .await;
    let r = j(w.call(|h| h.download_send(0)).await);
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(w.cdn.request_count(), 1);
    assert!(w.hydrus_paths().contains(&"/add_files/add_file".to_owned()));
    assert!(w.entries()[0].sent_to_hydrus);
    // This test is the only one that downloads a file called `a_matched.png`.
    let tmp_left = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .any(|d| {
            d.file_name()
                .to_string_lossy()
                .starts_with("hatate-linux-mcp-")
                && d.path().join("a_matched.png").exists()
        });
    assert!(!tmp_left, "the temporary download directory is removed");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_send_without_a_direct_url_is_skipped() {
    let w = world(|r| vec![with_png(r, "a.png", [5, 5, 5])]).await;
    let r = j(w.call(|h| h.download_send(0)).await);
    assert_eq!(r["skipped_reason"], "no direct file URL available");
    assert_eq!(w.cdn.request_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_that_outlives_its_limit_gives_the_timeout_text_and_still_finishes() {
    let slow = TestServer::http(|req| hydrus_answer(req).delayed(Duration::from_millis(700))).await;
    let w = world_with(
        Opts {
            timeouts: Timeouts {
                hydrus_send: 0.2,
                ..Timeouts::default()
            },
            ..Opts::default()
        },
        |r| vec![with_png(r, "a.png", [5, 5, 5])],
    )
    .await;
    w.settings.lock().unwrap().hydrus.api_url = slow.url("");
    let err = w
        .call(|h| h.send_upload(0))
        .await
        .expect_err("the wait is abandoned at the limit");
    assert_eq!(err.0, "Timed out after 0s waiting for the GUI thread");
    // Python's work keeps running on the GUI thread; so does this one, and it writes back.
    for _ in 0..100 {
        if w.entries()[0].sent_to_hydrus {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the send never finished");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hydrus_that_is_down_is_a_failed_send_with_the_python_message() {
    let w = world(|r| vec![with_png(r, "a.png", [5, 5, 5])]).await;
    // A port nothing listens on.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", dead.local_addr().unwrap().port());
    drop(dead);
    w.settings.lock().unwrap().hydrus.api_url = url;
    let r = j(w.call(|h| h.send_upload(0)).await);
    assert_eq!(r["success"], false);
    let msg = r["error"].as_str().unwrap();
    assert!(
        msg.starts_with("Could not reach Hydrus at http://127.0.0.1:"),
        "{msg}"
    );
    assert_eq!(w.entries()[0].error_message.as_deref(), Some(msg));
    assert!(!w.entries()[0].sent_to_hydrus);
}

// ---------------------------------------------------------------------------------------------
// research
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn research_starts_a_search_waits_for_it_and_reads_the_results() {
    let w = world(|_| vec![entry("a.jpg"), entry("b.jpg")]).await;
    w.settings.lock().unwrap().mcp.allow_research = true;
    let r = j(w.call(|h| h.research(vec![1, 0, 9])).await);
    assert_eq!(r["research_started"], true);
    assert_eq!(r["completed"], true);
    let results = r["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "row 9 does not exist");
    assert_eq!(
        (results[0]["row"].clone(), results[0]["filename"].clone()),
        (json!(1), json!("b.jpg"))
    );
    assert_eq!(results[0]["status"], "good");
    assert_eq!(
        w.launcher.launched.lock().unwrap().as_slice(),
        &[(vec!["b.jpg".to_owned(), "a.jpg".to_owned()], vec![0, 1])]
    );
    assert_eq!(w.audit_lines()[0]["row"], 1, "the audit row is rows[0]");
}

#[tokio::test(flavor = "multi_thread")]
async fn research_declines_while_a_search_runs_and_when_no_row_exists() {
    let w = world_with(
        Opts {
            timeouts: Timeouts {
                research_floor: 1.0,
                research_per_image: 0.0,
                ..Timeouts::default()
            },
            ..Opts::default()
        },
        |_| vec![entry("a.jpg")],
    )
    .await;
    w.settings.lock().unwrap().mcp.allow_research = true;
    let r = j(w.call(|h| h.research(vec![5])).await);
    assert_eq!(r["research_started"], false);
    assert_eq!(r["reason"], "No valid rows to search");
    w.launcher.never_finish.store(true, Ordering::SeqCst);
    let w2 = Arc::new(w);
    let again = {
        let w = w2.clone();
        tokio::spawn(async move { w.call(|h| h.research(vec![0])).await })
    };
    for _ in 0..100 {
        if !w2.launcher.launched.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let r = j(w2.call(|h| h.research(vec![0])).await);
    assert_eq!(r["reason"], "A search is already running");
    drop(again);
}

#[tokio::test(flavor = "multi_thread")]
async fn research_that_does_not_start_or_does_not_finish_says_so() {
    let w = world_with(
        Opts {
            timeouts: Timeouts {
                research_floor: 0.2,
                research_per_image: 0.0,
                ..Timeouts::default()
            },
            ..Opts::default()
        },
        |_| vec![entry("a.jpg")],
    )
    .await;
    w.settings.lock().unwrap().mcp.allow_research = true;
    w.launcher.refuse.store(true, Ordering::SeqCst);
    let r = j(w.call(|h| h.research(vec![0])).await);
    assert_eq!(r["reason"], "Search did not start");
    w.launcher.refuse.store(false, Ordering::SeqCst);
    w.launcher.never_finish.store(true, Ordering::SeqCst);
    let r = j(w.call(|h| h.research(vec![0])).await);
    assert_eq!(r["completed"], false);
    assert_eq!(r["filenames"], json!(["a.jpg"]));
    assert_eq!(
        r["note"],
        "Still running after 0s - check get_entry/get_candidates again shortly."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn research_through_the_merged_runner_bypasses_the_cache_and_keeps_the_rest() {
    let w = world_with(
        Opts {
            real_launcher: true,
            ..Opts::default()
        },
        |r| {
            vec![
                with_png(r, "a.png", [5, 5, 5]),
                with_png(r, "b.png", [6, 6, 6]),
            ]
        },
    )
    .await;
    w.settings.lock().unwrap().mcp.allow_research = true;
    w.settings.lock().unwrap().use_search_cache = true;
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(BOORU_URL, 96.5)])));
    let r = j(w.call(|h| h.research(vec![1])).await);
    assert_eq!(r["completed"], true, "{r}");
    assert_eq!(w.engines.calls(), ["iqdb"]);
    let es = w.entries();
    assert_ne!(es[1].status, MatchStatus::NotSearched);
    assert_eq!(es[1].candidates[0].url, BOORU_URL);
    assert_eq!(
        es[0].status,
        MatchStatus::NotSearched,
        "only the asked row is searched"
    );
    // A second research of the same bytes must search again: the cache is bypassed.
    w.engines
        .iqdb
        .push(Step::Return(Ok(vec![iqdb_match(BOORU_URL, 91.0)])));
    let r = j(w.call(|h| h.research(vec![1])).await);
    assert_eq!(r["completed"], true);
    assert_eq!(w.engines.calls(), ["iqdb", "iqdb"]);
    assert_eq!(w.entries()[1].similarity(), Some(91.0));
}

// ---------------------------------------------------------------------------------------------
// The channel under the handlers
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_handler_whose_queue_never_answers_gives_pythons_timeout_text() {
    let w = world_with(
        Opts {
            timeouts: Timeouts {
                default: 0.2,
                ..Timeouts::default()
            },
            ..Opts::default()
        },
        |_| vec![entry("a.jpg")],
    )
    .await;
    // Occupy the owner thread so the handler's call waits behind it.
    let ch = w.channel.clone();
    let blocker = std::thread::spawn(move || {
        let _ = ch.call(
            |_| {
                std::thread::sleep(Duration::from_millis(600));
                Ok(())
            },
            5.0,
        );
    });
    std::thread::sleep(Duration::from_millis(50));
    let err = w.call(|h| h.get_entry(0)).await.expect_err("times out");
    assert_eq!(err.0, "Timed out after 0s waiting for the GUI thread");
    blocker.join().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_invoker_stops_new_calls() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.call(|h| h.shutdown_invoker()).await;
    let err = w.call(|h| h.get_entry(0)).await.expect_err("refused");
    assert_eq!(err.0, "cannot schedule new futures after shutdown");
    drop(w.finish());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_model_is_handed_back_with_its_changes() {
    let w = world(|_| vec![entry("a.jpg")]).await;
    w.call(|h| h.toggle_reviewed(0, true)).await.unwrap();
    let mut model = w.finish();
    assert!(model.entries()[0].reviewed);
    assert!(
        !model.take_events().is_empty(),
        "the window would have redrawn the row"
    );
}
