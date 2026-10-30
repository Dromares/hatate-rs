//! The action differential, Rust side (DAN-2215 and DAN-2218, Phase 6 acceptance).
//!
//! Runs the same scripted action sequences the Python driver runs through `MainWindow` at
//! `76c85c4`, through the Rust window, and writes one JSON digest per step in the same shape.
//! The comparer, the scenario generator and the Python driver live outside the repo
//! (`worktrees/DAN-2215-tools` and `worktrees/DAN-2218-tools`, the tree is Rust only); this is the half that has to be Rust.
//!
//! Test-only. It needs a display and reads its inputs from the environment, and nothing of that
//! exists in the shipped binary: `HATATE_DIFF_SCENARIOS` (JSONL), `HATATE_DIFF_OUT`,
//! `HATATE_DIFF_FILES` (where a scenario's files live), `HATATE_DIFF_NOW` (the frozen clock) and
//! `HATATE_DIFF_ROOT` (the temp HOME). Without `HATATE_DIFF_SCENARIOS` it does nothing.

mod common;
#[path = "differential/hydrus_diff.rs"]
mod hydrus_diff;

use common::*;
use gtk4::prelude::*;
use hatate_core::paths::Paths;
use hatate_gtk::dialogs::{Asked, Buttons, Kind, Response};
use hatate_gtk::window::{Env, MainWindow};
use libadwaita as adw;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct Ctx {
    /// The running scenario's `kind`: `compare` adds the Review/compare digest (DAN-2340).
    kind: RefCell<String>,
    /// The scripted Hydrus of a `hydrus` scenario (DAN-2220); it lives for one scenario.
    hydrus: RefCell<Option<hydrus_diff::Server>>,
    files: PathBuf,
    root: PathBuf,
    now: f64,
}

fn main() {
    let Some(scenarios) = std::env::var_os("HATATE_DIFF_SCENARIOS") else {
        println!("differential: skipped (HATATE_DIFF_SCENARIOS is not set)");
        return;
    };
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!("differential: no display");
        std::process::exit(1);
    }
    let out = std::env::var("HATATE_DIFF_OUT").expect("HATATE_DIFF_OUT");
    let ctx = Ctx {
        kind: RefCell::new(String::new()),
        hydrus: RefCell::new(None),
        files: PathBuf::from(std::env::var("HATATE_DIFF_FILES").expect("HATATE_DIFF_FILES")),
        root: PathBuf::from(std::env::var("HATATE_DIFF_ROOT").expect("HATATE_DIFF_ROOT")),
        now: std::env::var("HATATE_DIFF_NOW")
            .expect("HATATE_DIFF_NOW")
            .parse()
            .expect("HATATE_DIFF_NOW is a number"),
    };
    let text = std::fs::read_to_string(scenarios).expect("scenario file");
    let specs: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("scenario line"))
        .collect();

    let fonts = tempfile::tempdir().expect("font dir");
    hatate_gtk::fonts::load(fonts.path()).expect("the bundled fonts load");
    let out = Rc::new(RefCell::new(std::fs::File::create(out).expect("out file")));
    let ctx = Rc::new(ctx);
    let code = hatate_gtk::app::with_application(move |app| {
        for spec in &specs {
            let id = spec["id"].clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_scenario(app, &ctx, spec)
            }));
            let line = match result {
                Ok(trace) => json!({"id": id, "side": "rs", "trace": trace}),
                Err(e) => {
                    let why = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_default();
                    json!({"id": id, "side": "rs", "error": format!("panic: {why}")})
                }
            };
            let mut f = out.borrow_mut();
            writeln!(f, "{line}").unwrap();
            f.flush().unwrap();
        }
        app.quit();
    });
    let _ = code;
}

fn abs(ctx: &Ctx, p: &str) -> PathBuf {
    if p.starts_with('/') {
        PathBuf::from(p)
    } else {
        ctx.files.join(p)
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn make_files(ctx: &Ctx, spec: &Value) {
    let _ = std::fs::remove_dir_all(&ctx.files);
    std::fs::create_dir_all(&ctx.files).unwrap();
    if let Some(files) = spec["files"].as_object() {
        for (rel, content) in files {
            let p = ctx.files.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            // `@dir` and `@missing` are the unopenable inputs: the path is a directory, or there
            // is nothing at it (DAN-2231). Mode 000 is no variant: the run is root in its own
            // user namespace and reads it.
            match content.as_str() {
                Some("@dir") => {
                    std::fs::create_dir_all(&p).unwrap();
                    continue;
                }
                Some("@missing") => continue,
                _ => {}
            }
            // The scenario holds the bytes as latin-1 text, as the Python driver writes them.
            // `@mode000:` is a file nobody may read (a non-root run only; DAN-2340).
            let text = content.as_str().unwrap();
            let (text, locked) = match text.strip_prefix("@mode000:") {
                Some(rest) => (rest, true),
                None => (text, false),
            };
            let bytes: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
            std::fs::write(&p, bytes).unwrap();
            if locked {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
            }
        }
    }
}

fn run_scenario(app: &adw::Application, ctx: &Ctx, spec: &Value) -> Vec<Value> {
    *ctx.kind.borrow_mut() = spec["kind"].as_str().unwrap_or("").to_owned();
    HELD.with(|h| h.borrow_mut().clear());
    make_files(ctx, spec);
    let _ = std::fs::remove_dir_all(&ctx.root);
    let paths = temp_paths(&ctx.root);
    std::fs::create_dir_all(paths.config_dir()).unwrap();
    copy_dir(
        Path::new(spec["profile"].as_str().expect("profile")),
        paths.config_dir(),
    );

    // A `hydrus` scenario points the profile at a scripted server on a free port (both sides do it
    // the same way, and the comparer sets the url aside).
    *ctx.hydrus.borrow_mut() = None;
    if spec["hydrus"].is_object() {
        let server = hydrus_diff::start(&spec["hydrus"]);
        let cfg = paths.config_dir().join("config.json");
        let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        doc["hydrus"]["api_url"] = json!(server.url());
        std::fs::write(&cfg, doc.to_string()).unwrap();
        *ctx.hydrus.borrow_mut() = Some(server);
    }

    let script = Rc::new(Script::default());
    let opened = Rc::new(RefCell::new(Vec::<PathBuf>::new()));
    let mut env = Env::new(paths.clone(), spec["unclean"].as_bool().unwrap_or(false));
    env.prompter = Some(script.clone());
    // Start Search runs against scripted engines (never a network), as in the smoke tests.
    let (backend, search) = common::search::Scripted::new(&paths);
    env.search = Some(backend);
    let now = ctx.now;
    env.clock = Rc::new(move || now);
    {
        let opened = opened.clone();
        env.open_path = Rc::new(move |p| opened.borrow_mut().push(p.to_path_buf()));
    }
    let w = MainWindow::new(app, env);
    w.win.present();
    // The startup hash pass lands before the first step, not during a one-image run.
    pump_for(400);
    settle(&w);
    let mut trace = vec![digest(ctx, &paths, &w, &script, &opened, "start")];
    script.reset();
    opened.borrow_mut().clear();

    for (i, step) in spec["steps"].as_array().unwrap().iter().enumerate() {
        let op = step["op"].as_str().unwrap();
        // The Delete from Hydrus box is Delete/Cancel here and Yes/No in Python (design gate 2.6):
        // `do_step` answers it.
        for a in step["msg"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|_| op != "delete")
        {
            script.push(Answer::Msg(match a.as_str() {
                Some("yes") => Response::Yes,
                Some("keep") => Response::Keep,
                Some("replace") => Response::Replace,
                Some("cancel") => Response::Cancel,
                Some("remove") => Response::Remove,
                _ => Response::No,
            }));
        }
        WIRE_LAST.with(|l| *l.borrow_mut() = Value::Null);
        do_step(ctx, &w, &script, &search, step);
        settle(&w);
        trace.push(digest(
            ctx,
            &paths,
            &w,
            &script,
            &opened,
            &format!("{i}:{op}"),
        ));
        script.reset();
        opened.borrow_mut().clear();
    }
    w.win.close();
    settle(&w);
    trace.push(digest(ctx, &paths, &w, &script, &opened, "close"));
    w.win.destroy();
    pump_for(20);
    trace
}

fn do_step(
    ctx: &Ctx,
    w: &MainWindow,
    script: &Script,
    search: &common::search::Scripted,
    step: &Value,
) {
    let op = step["op"].as_str().unwrap();
    let strs = |k: &str| -> Vec<String> {
        step[k]
            .as_array()
            .map(|a| a.iter().map(|x| x.as_str().unwrap().to_owned()).collect())
            .unwrap_or_default()
    };
    match op {
        "add_files" => {
            script.push(Answer::Files(
                strs("paths").iter().map(|p| abs(ctx, p)).collect(),
            ));
            w.activate_action("add-files");
        }
        "add_folder" => {
            script.push(Answer::Path(step["folder"].as_str().map(|f| abs(ctx, f))));
            w.activate_action("add-folder");
        }
        "drop" => w.drop_paths(strs("paths").iter().map(|p| abs(ctx, p)).collect()),
        "save_session_as" => {
            std::fs::create_dir_all(ctx.files.join("named")).unwrap();
            script.push(Answer::Path(
                step["name"]
                    .as_str()
                    .map(|n| ctx.files.join("named").join(n)),
            ));
            w.activate_action("save-session-as");
        }
        "open_session" => {
            script.push(Answer::Path(step["path"].as_str().map(|p| abs(ctx, p))));
            w.activate_action("open-session");
        }
        "clear_session" => w.activate_action("clear-session"),
        "routes" => ctx
            .hydrus
            .borrow()
            .as_ref()
            .expect("a hydrus scenario")
            .set_routes(&step["routes"]),
        "send" => w.action_send_to_hydrus(&w.s.queue_view.selected_ids()),
        "import_url" => w.action_import_url_to_hydrus(&w.s.queue_view.selected_ids()),
        "download_send" => w.action_download_and_send_to_hydrus(&w.s.queue_view.selected_ids()),
        "reconcile" => w.action_reconcile_with_hydrus(),
        "show" => w.show_rows_in_hydrus(&w.s.queue_view.selected_ids()),
        "delete" => {
            let yes = step["msg"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|a| a.as_str())
                == Some("yes");
            script.push(Answer::Msg(if yes {
                Response::Delete
            } else {
                Response::Cancel
            }));
            w.delete_rows_from_hydrus(&w.s.queue_view.selected_ids());
        }
        "set_theme" => w.set_theme(step["mode"].as_str().unwrap()),
        "s_open" => {
            w.activate_action("preferences");
            // the Lens check reads a browser-less process, as the Python driver patches it to
            if let Some(d) = w.s.settings_dialog.borrow().as_ref() {
                *d.lens_env.borrow_mut() = hatate_core::lens_browser::deps::BrowserEnv::default();
            }
        }
        "s_press" => {
            // Browse asks for a path: the step names the answer (null is Cancel)
            script.push(Answer::Path(step["pick"].as_str().map(|p| abs(ctx, p))));
            settings_step(w, step)
        }
        "s_set" | "s_find" | "s_jump" | "s_tab" | "s_ok" | "s_cancel" => settings_step(w, step),
        // 6.i: the MCP page's buttons (no Browse answer to script) and the Shortcuts page's
        "m_press" | "m_clear" | "m_reset" | "hold_port" | "release_port" | "wire" => {
            mcp_step(w, step)
        }
        "autosave" => w.autosave_now(),
        "delete_files" => {
            for p in strs("paths") {
                let _ = std::fs::remove_file(abs(ctx, &p));
            }
        }
        "open_log" => w.activate_action("open-log"),
        "start" | "continue_without_saucenao" => run_search_step(w, search, step),
        "alert" => {
            search.alerts.raise_alert(&alert_of(&step["alert"]));
        }
        "site" => {
            let all = hatate_gtk::menu::all_site_options();
            let i = all
                .iter()
                .position(|s| s == step["site"].as_str().unwrap())
                .expect("a known site");
            w.activate_action(&hatate_gtk::menu::site_action(i));
        }
        "sites_all" => w.activate_action(if step["on"].as_bool().unwrap() {
            "sites-select-all"
        } else {
            "sites-select-none"
        }),
        "filter_text" => w.s.filter_bar.text.set_text(step["text"].as_str().unwrap()),
        "filter_isolate" | "filter_toggle" | "filter_all" => {
            let bar = &w.s.filter_bar;
            let (prefix, group) = match step["kind"].as_str().unwrap() {
                "status" => ("Status", "fstatus"),
                "site" => ("Site", "fsite"),
                _ => ("Upscale", "fupscale"),
            };
            let label = step["label"].as_str().unwrap();
            let hit = bar.present(prefix).iter().position(|l| l == label);
            if op == "filter_all" {
                bar.set_all_named(prefix, true);
            } else if let Some(i) = hit {
                if op == "filter_isolate" {
                    bar.set_all_named(prefix, false);
                }
                bar.button(prefix)
                    .activate_action(&format!("{group}.v{i}"), None)
                    .unwrap();
            }
        }
        "filter_clear" => w.s.filter_bar.clear_all(),
        "sort" => {
            for _ in 0..step["times"].as_u64().unwrap_or(1) {
                w.s.queue_view.on_header_clicked(
                    hatate_core::table_cells::Column::ALL
                        [step["column"].as_u64().unwrap() as usize],
                );
            }
        }
        "select" => {
            let ids = w.s.queue_view.visible_ids();
            let pick: Vec<_> = step["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|r| ids.get(r.as_u64().unwrap() as usize).copied())
                .collect();
            // The Python driver clears the selection first, so even an unchanged set fires its selection
            // handler again; GTK says nothing for an identical set, so clear here too.
            w.s.queue_view.select_ids(&[]);
            w.s.queue_view.select_ids(&pick);
        }
        "select_status" => {
            let st = hatate_core::model::MatchStatus::ALL
                .into_iter()
                .find(|s| s.as_str() == step["status"].as_str().unwrap())
                .expect("status");
            w.select_rows_by_status(st);
        }
        "reset" => w.reset_rows(&w.s.queue_view.selected_ids()),
        "reviewed" => w.set_rows_reviewed(
            &w.s.queue_view.selected_ids(),
            step["value"].as_bool().unwrap(),
        ),
        "remove" => w.remove_rows(&w.s.queue_view.selected_ids()),
        "export" => {
            std::fs::create_dir_all(ctx.files.join("out")).unwrap();
            let filter = match step["filter"].as_str().unwrap() {
                "csv" => "CSV spreadsheet (*.csv)",
                "json" => "JSON (*.json)",
                "all" => "All files (*)",
                _ => "",
            };
            script.push(Answer::Save(
                step["path"]
                    .as_str()
                    .map(|p| (abs(ctx, p), filter.to_owned())),
            ));
            if step["scope"] == "all" {
                w.action_export_results(None);
            } else {
                w.action_export_results(Some(&w.s.queue_view.selected_ids()));
            }
        }
        "write_tags" => {
            if step["scope"] == "all" {
                w.action_write_tag_files(None);
            } else {
                w.action_write_tag_files(Some(&w.s.queue_view.selected_ids()));
            }
        }
        "clear_cache" => w.action_clear_search_cache(),
        "set_mode" => w.set_mode(step["key"].as_str().unwrap()),
        "view" => {
            // A click on a disabled button does nothing in Qt; `set_active` would not care.
            let b = &w.s.review.view_buttons[step["key"].as_str().unwrap()];
            if b.is_sensitive() {
                b.set_active(true);
            }
        }
        "slider" => {
            w.s.review
                .wipe_slider
                .set_value(step["value"].as_f64().unwrap())
        }
        "zoom" => zoom(&w.s.review.wipe, step["what"].as_str().unwrap()),
        "open_compare" => {
            if let Some(id) = w.current_id() {
                w.open_compare_dialog(id);
            }
        }
        "dlg_mode" | "dlg_slider" | "dlg_zoom" | "dlg_close" => {
            // The guard box answered instead of a dialog: nothing to drive.
            let Some(d) = w.s.compare_open.borrow().clone() else {
                return;
            };
            match op {
                "dlg_mode" => match step["mode"].as_str().unwrap() {
                    "wipe" => d.wipe_radio.set_active(true),
                    "local" => d.local_radio.set_active(true),
                    "match" => d.matched_radio.set_active(true),
                    // A disabled radio cannot be clicked (`set_active` would not care).
                    _ if !d.diff_radio.is_sensitive() => {}
                    _ => d.diff_radio.set_active(true),
                },
                "dlg_slider" => d.slider.set_value(step["value"].as_f64().unwrap()),
                "dlg_zoom" => zoom(&d.view, step["what"].as_str().unwrap()),
                _ => d.window.close(),
            }
        }
        other => panic!("unknown op {other}"),
    }
}

fn zoom(v: &hatate_gtk::wipe_view::WipeView, what: &str) {
    match what {
        "in" => v.zoom_in(),
        "out" => v.zoom_out(),
        "fit" => v.zoom_to_fit(),
        _ => v.zoom_to_actual(),
    }
}

fn alert_of(a: &Value) -> hatate_core::engine_alerts::EngineAlert {
    let t = |k: &str| a[k].as_str().unwrap().to_owned();
    hatate_core::engine_alerts::EngineAlert {
        key: t("key"),
        engine: t("engine"),
        title: t("title"),
        body: t("body"),
        remedy: t("remedy"),
    }
}

/// `start` and `continue_without_saucenao`: script the engines, press the button, and turn the
/// loop until the run is over, stopping or raising an alert once the scripted number of images
/// has been searched (the Python driver does the same, see `run_search_step` there).
fn run_search_step(w: &MainWindow, search: &common::search::Scripted, step: &Value) {
    use common::search::scripted::Step;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    let op = step["op"].as_str().unwrap();
    let delay_ms = step["delay_ms"]
        .as_u64()
        .filter(|d| *d > 0)
        .unwrap_or(if step["stop_after"].is_u64() { 150 } else { 0 });
    let urls = step["urls"].as_array().cloned().unwrap_or_default();
    let engine = step["engine"].as_str().unwrap();
    for (i, o) in step["outcomes"].as_array().unwrap().iter().enumerate() {
        let o = o.as_str().unwrap();
        let wait = Duration::from_millis(delay_ms);
        let list: Vec<&str> = urls
            .get(i)
            .and_then(|u| u.as_array())
            .map(|u| u.iter().map(|x| x.as_str().unwrap()).collect())
            .unwrap_or_default();
        if engine == "iqdb" {
            let answer = match o {
                "error" => Err(hatate_core::engines::iqdb::IqdbError(
                    "scripted error".into(),
                )),
                "good" => Ok(list
                    .iter()
                    .enumerate()
                    .map(|(k, u)| common::search::scripted::iqdb_match(u, 95.0 - 5.0 * k as f64))
                    .collect()),
                _ => Ok(Vec::new()),
            };
            search.engines.iqdb.push(Step::After(wait, answer));
        } else {
            let answer = match o {
                "error" => Err(hatate_core::engines::saucenao::SauceNaoError(
                    "scripted error".into(),
                )),
                _ => Ok(Vec::new()),
            };
            search.engines.saucenao.push(Step::After(wait, answer));
        }
    }
    let calls = search.engines.calls().len();
    let quota = match (step["quota_after"].as_u64(), op) {
        (Some(k), _) => calls + k as usize,
        (None, "continue_without_saucenao") => 0,
        _ => usize::MAX,
    };
    search.quota_after_calls.store(quota, Ordering::SeqCst);
    if op == "start" {
        w.activate_action("toggle-search");
    } else {
        w.s.quota_banner.keep_going.emit_clicked();
    }
    let stop_after = step["stop_after"].as_u64();
    let alert = step.get("alert").filter(|a| a.is_object());
    let (mut stopped, mut raised) = (false, false);
    let t = std::time::Instant::now();
    while w.s.run_active.get() && t.elapsed() < Duration::from_secs(30) {
        pump_for(5);
        // "3/5 searched": the strip's own count, which is what the Python bar's value is.
        let done =
            w.s.shell
                .run_progress_label
                .text()
                .split('/')
                .next()
                .and_then(|n| n.replace(',', "").parse::<u64>().ok())
                .unwrap_or(0);
        if let Some(k) = stop_after {
            if !stopped && done >= k {
                w.activate_action("toggle-search");
                stopped = true;
            }
        }
        if let Some(a) = alert {
            if !raised && w.s.run_active.get() && done >= a["after_progress"].as_u64().unwrap() {
                search.alerts.raise_alert(&alert_of(a));
                raised = true;
            }
        }
    }
}

/// Lets the main loop turn until nothing the window started is still running.
fn settle(w: &MainWindow) {
    pump_for(30);
    pump_until(20_000, || {
        !w.s.file_hash_worker.is_running()
            && !w.s.candidate_worker.is_running()
            && !w.s.missing_file_worker.is_running()
            && !w.s.hydrus_lookup_worker.is_running()
            && !w.s.hydrus_reconcile_worker.is_running()
            && !w.s.hydrus_import_poll_worker.is_running()
            && !w.s.hydrus_batch_worker.is_running()
            && !w
                .s
                .autosave_inflight
                .load(std::sync::atomic::Ordering::SeqCst)
    });
    pump_for(60);
    for _ in 0..5 {
        pump_for(10);
    }
    settle_compare(w);
}

/// The Review wipe and the compare dialog load off the main loop (DAN-2340): wait until nothing is
/// loading and the digest part has stopped moving. A no-op for a window that never opened them.
fn settle_compare(w: &MainWindow) {
    let sample = || compare_digest(w).to_string();
    let ready = || {
        !w.s.comparison.borrow().loading
            && w.s
                .compare_open
                .borrow()
                .as_ref()
                .is_none_or(|d| !d.summary_text().is_empty())
    };
    if w.s.comparison.borrow().loaded_for.is_none()
        && w.s.compare_open.borrow().is_none()
        && !w.s.comparison.borrow().loading
    {
        return;
    }
    let mut last = sample();
    let t = std::time::Instant::now();
    loop {
        pump_for(120);
        let now = sample();
        if ready() && now == last {
            break;
        }
        last = now;
        if t.elapsed() > std::time::Duration::from_secs(15) {
            break;
        }
    }
}

/// The Review page and the compare dialog as the Python driver's `compare_digest` reads them.
fn compare_digest(w: &MainWindow) -> Value {
    fn zoom_fmt(t: &str) -> String {
        let mut out = String::new();
        let mut in_digits = false;
        for c in t.chars() {
            if c.is_ascii_digit() {
                if !in_digits {
                    out.push('#');
                }
                in_digits = true;
            } else {
                in_digits = false;
                out.push(c);
            }
        }
        out
    }
    /// The layout-free reading of the zoom: the factor (1.0 = fit) while it is a power of 1.25 from
    /// fit, else the percentage (after a 100% click the factor is 100/fit%, the widget's business).
    fn zoom_free(v: &hatate_gtk::wipe_view::WipeView) -> Value {
        let f = v.zoom_factor();
        let k = if f > 0.0 { f.ln() / 1.25f64.ln() } else { 0.0 };
        if (k - k.round()).abs() < 1e-6 {
            json!(["f", (f * 1e6).round() / 1e6])
        } else {
            json!(["p", (v.zoom_percent() * 1e3).round() / 1e3])
        }
    }
    fn view_of(v: &hatate_gtk::wipe_view::WipeView) -> Value {
        let (l, r) = v.has_pictures();
        json!({"pictures": [l, r], "differences": v.showing_differences(),
               "position": (v.position() * 1e6).round() / 1e6,
               "zoom_free": zoom_free(v), "x_zoom": (v.zoom_factor() * 1e6).round() / 1e6})
    }
    let r = &w.s.review;
    let row = w.current_id().and_then(|id| {
        w.s.queue
            .borrow()
            .entry(id)
            .map(|e| e.filename().to_owned())
    });
    let mut out = json!({
        "mode": w.s.shell.current_mode(),
        "row": row,
        "view": r.current_view(),
        "page": r.stack.visible_child_name().map(|n| n.to_string()).unwrap_or_default(),
        "match_label": r.match_label.text().to_string(),
        "note": r.note.text().to_string(),
        "note_visible": r.note.is_visible(),
        "note_enabled": r.note.is_sensitive(),
        "slider": r.wipe_slider.value().round() as i64,
        "slider_enabled": r.wipe_slider.is_sensitive(),
        "slider_row_visible": r.slider_row.is_visible(),
        "zoom_tools_visible": r.zoom_tools.is_visible(),
        "zoom_fmt": zoom_fmt(r.zoom_label.text().as_str()),
        "diff_button_enabled": r.view_buttons["diff"].is_sensitive(),
        "wipe": view_of(&r.wipe),
    });
    if (r.wipe.zoom_percent() - 100.0).abs() < 0.5 {
        out["zoom_label_at_100"] = json!(r.zoom_label.text().to_string());
    }
    out["rs_error_detail"] = json!({"text": r.error_detail.text().to_string(),
                                 "visible": r.error_detail.is_visible()});
    out["dialog"] = match w.s.compare_open.borrow().as_ref() {
        None => Value::Null,
        Some(d) => json!({
            "title": d.window.title().map(|t| t.to_string()).unwrap_or_default(),
            "summary": d.summary_text().iter().map(|l| l.replace('\u{a0}', " ")).collect::<Vec<_>>(),
            "radios": [d.wipe_radio.is_active(), d.local_radio.is_active(),
                       d.matched_radio.is_active(), d.diff_radio.is_active()],
            "diff_radio_enabled": d.diff_radio.is_sensitive(),
            "slider": d.slider.value().round() as i64,
            "slider_enabled": d.slider.is_sensitive(),
            "slider_left": d.slider_left_label.text().to_string(),
            "slider_right": d.slider_right_label.text().to_string(),
            "diff_note": d.diff_note.text().to_string(),
            "diff_note_visible": d.diff_note.is_visible(),
            "zoom_fmt": zoom_fmt(d.zoom_label.text().as_str()),
            "view": view_of(&d.view),
        }),
    };
    out
}

/// The text of every label under `w`, depth first, in tree order.
fn labels(w: &gtk4::Widget, out: &mut Vec<String>) {
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        out.push(l.text().to_string());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        labels(&child, out);
        c = child.next_sibling();
    }
}

fn labels_of(w: &impl IsA<gtk4::Widget>) -> Vec<String> {
    let mut out = Vec::new();
    labels(w.upcast_ref(), &mut out);
    out
}

/// The labels of every `.meta-line` box under `w`.
fn meta_lines(w: &gtk4::Widget, out: &mut Vec<String>) {
    if w.has_css_class("meta-line") {
        labels(w, out);
        return;
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        meta_lines(&child, out);
        c = child.next_sibling();
    }
}

/// Qt's `not isHidden()`: the widget's own visible flag, not whether its ancestors are shown.
fn flag(w: &impl IsA<gtk4::Widget>) -> bool {
    w.property::<bool>("visible")
}

fn kind(k: Kind) -> &'static str {
    match k {
        Kind::Information => "information",
        Kind::Warning => "warning",
        Kind::Critical => "critical",
        Kind::Question => "question",
    }
}

fn button(r: Response) -> &'static str {
    match r {
        Response::Ok => "ok",
        Response::Yes => "yes",
        Response::No => "no",
        Response::Keep => "keep",
        Response::Replace => "replace",
        Response::Remove => "remove",
        Response::Delete => "delete",
        Response::Cancel => "cancel",
    }
}

/// The candidate-detail pane (DAN-2339 condition 2): tag list, match label, picker, caption.
fn review_detail(w: &MainWindow) -> Value {
    let r = &w.s.review;
    let mut tags = Vec::new();
    let mut i = 0;
    while let Some(row) = r.tag_list.row_at_index(i) {
        let child = row.child();
        tags.push(
            child
                .clone()
                .and_downcast::<gtk4::Label>()
                .map(|l| l.text().to_string())
                .or_else(|| {
                    child
                        .and_downcast::<gtk4::EditableLabel>()
                        .map(|l| l.text().to_string())
                })
                .unwrap_or_default(),
        );
        i += 1;
    }
    let combo: Vec<String> = (0..r.candidate_model.n_items())
        .filter_map(|i| r.candidate_model.string(i))
        .map(|s| s.to_string())
        .collect();
    let index = if combo.is_empty() {
        -1
    } else {
        r.candidate_combo.selected() as i64
    };
    json!({"tags": tags, "match": r.match_label.text().to_string(), "combo": combo,
           "combo_index": index, "matched_caption": r.matched_caption.left.text().to_string()})
}

fn digest(
    ctx: &Ctx,
    paths: &Paths,
    w: &MainWindow,
    script: &Script,
    opened: &Rc<RefCell<Vec<PathBuf>>>,
    step: &str,
) -> Value {
    let s = &w.s;
    let sh = &s.shell;
    let q = s.queue.borrow();
    let entries = q.entries();
    let page = if entries.is_empty() {
        if s.nothing_survived.borrow().is_some() {
            "survived"
        } else {
            "empty"
        }
    } else {
        "rows"
    };
    let progress = (sh.progress_bar.fraction() * 1e6).round() / 1e6;
    let mut survived_meta = Value::Null;
    if let Some(p) = s.nothing_survived.borrow().as_ref() {
        let mut m = Vec::new();
        meta_lines(p.root.upcast_ref(), &mut m);
        survived_meta = json!(m);
    }
    let dialogs: Vec<Value> = script
        .shown
        .borrow()
        .iter()
        .zip(script.results.borrow().iter())
        .map(|(d, r)| {
            let mut dialog = json!({
                "kind": kind(d.kind), "title": d.title, "text": d.text,
                "buttons": match d.buttons {
                    Buttons::YesNo => "yes_no",
                    Buttons::KeepReplaceCancel => "keep_replace_cancel",
                    Buttons::RemoveCancel => "remove_cancel",
                    Buttons::DeleteCancel => "delete_cancel",
                    Buttons::Ok => "ok",
                    Buttons::GotIt => "got_it",
                },
                "default": button(d.default),
                "result": if matches!(d.buttons, Buttons::Ok | Buttons::GotIt) { "ok" } else { button(*r) },
            });
            // `setDetailedText`, when the box has any: older steps have none, so their digests
            // do not change.
            if !d.detail.is_empty() {
                dialog["detail"] = json!(d.detail);
            }
            dialog
        })
        .collect();
    let choosers: Vec<Value> = script
        .asked
        .borrow()
        .iter()
        .map(|a| match a {
            Asked::OpenFiles { title, filter } => {
                json!({"kind": "open_files", "title": title, "filter": filter})
            }
            Asked::Folder { title } => json!({"kind": "folder", "title": title}),
            Asked::OpenFile { title, filter } => {
                json!({"kind": "open_file", "title": title, "filter": filter})
            }
            Asked::SaveFile {
                title,
                default_name,
                filter,
            } => json!({"kind": "save_file", "title": title,
                        "default_name": Path::new(default_name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        "filter": filter}),
            Asked::AddTags { prompt } => json!({"kind": "add_tags", "prompt": prompt}),
            Asked::MatchConditions { opened_on } => {
                json!({"kind": "match_conditions", "opened_on": opened_on})
            }
        })
        .collect();
    let rb = &s.run_banner;
    let qb = &s.quota_banner;
    let mut run_meta = Vec::new();
    labels(rb.parts.meta_slot.upcast_ref(), &mut run_meta);
    let mut quota_meta = Vec::new();
    labels(qb.parts.meta_slot.upcast_ref(), &mut quota_meta);
    let _ = labels_of(&rb.parts.body);
    let mut digest = json!({
        "step": step,
        "status": sh.status_text(),
        "button": sh.search_toggle_btn.label().map(|l| l.to_string()).unwrap_or_default(),
        "active": s.run_active.get(),
        "run_note": sh.run_note_label.text(),
        "run_progress": sh.run_progress_label.text(),
        "run_eta": sh.run_eta_label.text(),
        "sent": sh.sent_count_label.text(),
        "wait": sh.wait_countdown_label.text(),
        "quota_label": sh.saucenao_quota_label.text(),
        "progress": progress,
        "list": entries.iter().map(|e| json!({
            "path": e.path, "hash": e.hydrus_hash, "status": e.status.as_str(),
            "sent": e.sent_to_hydrus, "confirmed": e.hydrus_import_confirmed,
            "reviewed": e.reviewed, "interrupted": e.interrupted_mid_search,
        })).collect::<Vec<_>>(),
        "page": page,
        "detail": review_detail(w),
        "ghost": sh.root.glyph(),
        "mode": sh.current_mode(),
        "run_banner": {
            "visible": flag(&rb.root),
            "body": rb.parts.body.text().to_string(),
            "meta": run_meta,
        },
        "quota_banner": {
            "visible": flag(&qb.root),
            "body": qb.parts.body.text().to_string(),
            "review": qb.review.label().map(|l| l.to_string()).unwrap_or_default(),
            "review_visible": flag(&qb.review),
            "meta": quota_meta,
        },
        "survived_meta": survived_meta,
        "table": table_text(w),
        "selected": selected_paths(w),
        "sort": s.queue_view.sort_state().map_or(Value::Null, |(c, asc)| json!([c as u8, asc])),
        "filter": filter_state(w),
        "tree": tree_state(ctx),
        "dialogs": dialogs,
        "choosers": choosers,
        "opened": opened.borrow().iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
        "files": profile_files(ctx, paths),
        "pause_file": pause_file(paths),
    });
    if ctx.kind.borrow().as_str() == "compare" {
        digest["compare"] = compare_digest(w);
    }
    if ctx.kind.borrow().as_str() == "settings" {
        digest["settings"] = settings_digest(w, paths);
    }
    if ctx.kind.borrow().as_str() == "mcp" {
        digest["mcp"] = mcp_digest(w, paths);
        digest["mcp"]["wire"] = WIRE_LAST.with(|l| l.borrow().clone());
        digest["mcp"]["audit"] = audit_lines(paths);
    }
    if let Some(h) = ctx.hydrus.borrow().as_ref() {
        digest["hydrus"] = h.log();
        digest["errors"] = json!(
            entries
                .iter()
                .map(|e| e.error_message.clone())
                .collect::<Vec<_>>()
        );
    }
    digest
}

fn pause_file(paths: &Paths) -> Value {
    match std::fs::read_to_string(paths.config_dir().join("saucenao_quota_pause.json")) {
        Ok(t) => serde_json::from_str::<Value>(&t).unwrap_or_else(|_| json!("unreadable")),
        Err(_) => Value::Null,
    }
}

/// `session.db` rows, `config.json` and the named sessions: what the step left on disk.
fn profile_files(ctx: &Ctx, paths: &Paths) -> Value {
    let mut session_rows = Value::Null;
    let db = paths.session_db();
    if db.exists() {
        let uri = format!("file:{}?mode=ro", db.display());
        session_rows = match rusqlite::Connection::open_with_flags(
            uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        ) {
            Ok(conn) => {
                let q = conn.prepare("SELECT path, ordinal, data FROM entries ORDER BY ordinal");
                match q {
                    Ok(mut stmt) => {
                        let rows: Vec<Value> = stmt
                            .query_map([], |r| {
                                let p: String = r.get(0)?;
                                let o: i64 = r.get(1)?;
                                let d: String = r.get(2)?;
                                Ok(json!([p, o, serde_json::from_str::<Value>(&d).unwrap()]))
                            })
                            .unwrap()
                            .map(|r| r.unwrap())
                            .collect();
                        Value::Array(rows)
                    }
                    Err(_) => json!("error: OperationalError"),
                }
            }
            Err(_) => json!("error: OperationalError"),
        };
    }
    let config = match std::fs::read_to_string(paths.config_file()) {
        Ok(t) => serde_json::from_str::<Value>(&t).unwrap_or_else(|_| json!("unreadable")),
        Err(_) => Value::Null,
    };
    let mut named: BTreeMap<String, Value> = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(ctx.files.join("named")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") {
                continue;
            }
            let v = match std::fs::read_to_string(e.path())
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                Some(mut v) => {
                    if let Some(o) = v.as_object_mut() {
                        o.shift_remove("saved_at");
                    }
                    v
                }
                None => json!("unreadable"),
            };
            named.insert(name, v);
        }
    }
    json!({"session_rows": session_rows, "config": config, "named": named})
}

/// Each row's display text (`data(DisplayRole)`, so the model's text and not the chip drawn from
/// it, which the pure differential covers), Thumb left out, in screen order.
fn table_text(w: &MainWindow) -> Value {
    use hatate_core::table_cells::{Column, cell_text};
    let view = &w.s.queue_view;
    let q = w.s.queue.borrow();
    Value::Array(
        view.visible_ids()
            .into_iter()
            .filter_map(|id| q.entry(id))
            .map(|e| {
                Value::Array(
                    Column::ALL[1..]
                        .iter()
                        .map(|c| json!(cell_text(e, *c)))
                        .collect(),
                )
            })
            .collect(),
    )
}

/// The selected rows' paths, in screen order.
fn selected_paths(w: &MainWindow) -> Value {
    let view = &w.s.queue_view;
    let q = w.s.queue.borrow();
    let mut rows: Vec<(u32, String)> = view
        .selected_ids()
        .into_iter()
        .filter_map(|id| Some((view.row_of(id)?, q.entry(id)?.path.clone())))
        .collect();
    rows.sort();
    json!(rows.into_iter().map(|(_, p)| p).collect::<Vec<_>>())
}

fn filter_state(w: &MainWindow) -> Value {
    let bar = &w.s.filter_bar;
    let menu = |prefix: &str| -> (Vec<String>, Vec<bool>) {
        let names = bar.present(prefix);
        let ticks = (0..names.len()).map(|i| bar.ticked(prefix, i)).collect();
        (names, ticks)
    };
    let (status_menu, status_checked) = menu("Status");
    let (site_menu, site_checked) = menu("Site");
    let (upscale_menu, upscale_checked) = menu("Upscale");
    json!({
        "text": bar.text.text().to_string(),
        "status": bar.button("Status").label().map(|l| l.to_string()),
        "site": bar.button("Site").label().map(|l| l.to_string()),
        "upscale": bar.button("Upscale").label().map(|l| l.to_string()),
        "count": bar.count.label().to_string(),
        "count_tip": bar.count.tooltip_text().map(|t| t.to_string()).unwrap_or_default(),
        "clear_enabled": bar.clear.is_sensitive(),
        "status_menu": status_menu, "status_checked": status_checked,
        "site_menu": site_menu, "site_checked": site_checked,
        "upscale_menu": upscale_menu, "upscale_checked": upscale_checked,
    })
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Every file under the scenario's directory (size, crc, mode), the named sessions apart.
fn tree_state(ctx: &Ctx) -> Value {
    use std::os::unix::fs::PermissionsExt;
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Value>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(data) = std::fs::read(&p) {
                let rel = p.strip_prefix(base).unwrap();
                if rel.components().any(|c| c.as_os_str() == "named") {
                    continue;
                }
                let mode = std::fs::metadata(&p)
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0);
                out.insert(
                    rel.to_string_lossy().into_owned(),
                    json!([data.len(), crc32(&data), mode]),
                );
                if rel
                    .components()
                    .next()
                    .is_some_and(|c| c.as_os_str() == "out")
                {
                    let head = &data[..data.len().min(700)];
                    out.insert(
                        format!("{}#text", rel.to_string_lossy()),
                        json!(String::from_utf8_lossy(head)),
                    );
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(&ctx.files, &ctx.files, &mut out);
    json!(out)
}

// -- Settings (DAN-2221) -----------------------------------------------------------------------

fn decode_value(v: &Value) -> hatate_gtk::settings_values::Value {
    use hatate_gtk::settings_values::Value as V;
    if let Some(b) = v.get("b") {
        V::Bool(b.as_bool().unwrap())
    } else if let Some(n) = v.get("n") {
        V::Num(n.as_f64().unwrap())
    } else if let Some(t) = v.get("t") {
        V::Text(t.as_str().unwrap().to_owned())
    } else if let Some(c) = v.get("c") {
        V::Choice(c.as_str().unwrap().to_owned())
    } else {
        V::Checked(
            v["k"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect(),
        )
    }
}

fn encode_value(v: &hatate_gtk::settings_values::Value) -> Value {
    use hatate_gtk::settings_values::Value as V;
    match v {
        V::Bool(b) => json!({"b": b}),
        V::Num(n) => json!({"n": n}),
        V::Text(t) => json!({"t": t}),
        V::Choice(c) => json!({"c": c}),
        V::Checked(k) => json!({"k": k}),
    }
}

fn settings_step(w: &MainWindow, step: &Value) {
    let d =
        w.s.settings_dialog
            .borrow()
            .clone()
            .expect("the Settings dialog is open");
    match step["op"].as_str().unwrap() {
        "s_set" => d.set_value(step["id"].as_str().unwrap(), &decode_value(&step["v"])),
        "s_find" => d.find.set_text(step["text"].as_str().unwrap()),
        "s_jump" => d.jump_to_setting(step["i"].as_u64().unwrap() as usize),
        "s_tab" => d
            .tabs
            .set_current_page(Some(step["i"].as_u64().unwrap() as u32)),
        "s_press" => d.press(step["id"].as_str().unwrap()),
        "s_ok" => d.click_ok(),
        "s_cancel" => d.click_cancel(),
        other => panic!("{other}"),
    }
}

/// The open Settings dialog as the Python driver reads it: the tabs, the Find box and its
/// results, every control's value, the pipeline line. `config.json` as bytes, not parsed.
fn settings_digest(w: &MainWindow, paths: &Paths) -> Value {
    let config_text = std::fs::read_to_string(paths.config_file()).ok();
    let open = w.s.settings_dialog.borrow().clone();
    let dialog = match open {
        None => Value::Null,
        Some(d) => {
            let values: BTreeMap<String, Value> = d
                .values()
                .0
                .iter()
                .map(|(k, v)| (k.clone(), encode_value(v)))
                .collect();
            let results = d.result_texts();
            json!({
                "tabs": d.tab_names(),
                "tab": d.current_tab(),
                "find": d.find.text().to_string(),
                "results": results,
                "results_visible": d.results_visible(),
                "activatable": (0..results.len()).map(|i| d.result_is_activatable(i)).collect::<Vec<_>>(),
                "pipeline": d.live_text("engine_pipeline"),
                "values": values,
            })
        }
    };
    json!({"config_text": config_text, "dialog": dialog})
}

thread_local! {
    /// The last `wire` step's reply, the session id the server gave, and the digest reads them.
    static WIRE_LAST: RefCell<Value> = const { RefCell::new(Value::Null) };
    static WIRE_SID: RefCell<String> = const { RefCell::new(String::new()) };
    /// Ports the scenario holds with a listener of its own (the Python driver does the same).
    static HELD: RefCell<Vec<std::net::TcpListener>> = const { RefCell::new(Vec::new()) };
}

/// The 6.i steps: the MCP page's buttons, the Shortcuts page's Clear and Reset all, and a port
/// the scenario holds so that the server cannot bind it.
fn mcp_step(w: &MainWindow, step: &Value) {
    let op = step["op"].as_str().unwrap();
    match op {
        "hold_port" => {
            let port = step["port"].as_u64().unwrap() as u16;
            let l = std::net::TcpListener::bind(("127.0.0.1", port)).expect("hold the port");
            HELD.with(|h| h.borrow_mut().push(l));
            return;
        }
        "release_port" => {
            HELD.with(|h| h.borrow_mut().clear());
            return;
        }
        "wire" => return wire_step(step),
        _ => {}
    }
    let d =
        w.s.settings_dialog
            .borrow()
            .clone()
            .expect("the Settings dialog is open");
    match op {
        "m_press" => d.press(step["id"].as_str().unwrap()),
        "m_clear" => d
            .shortcuts
            .edit(step["id"].as_str().unwrap())
            .expect("an action")
            .set(""),
        "m_reset" => d.shortcuts.reset.emit_clicked(),
        other => panic!("{other}"),
    }
}

/// The open dialog's MCP and Shortcuts state as the Python driver reads it: the values of those
/// two pages, the Connection card, the hints, the conflict line, and the server's own state.
/// `config.json` as bytes.
fn mcp_digest(w: &MainWindow, paths: &Paths) -> Value {
    let config_text = std::fs::read_to_string(paths.config_file()).ok();
    let report = w.mcp_report();
    let dialog = match w.s.settings_dialog.borrow().clone() {
        None => Value::Null,
        Some(d) => {
            let values: BTreeMap<String, Value> = d
                .values()
                .0
                .iter()
                .filter(|(k, _)| k.starts_with("mcp_") || k.starts_with("shortcut."))
                .map(|(k, v)| (k.clone(), encode_value(v)))
                .collect();
            json!({
                "tab": d.current_tab(),
                "values": values,
                "primary": d.live_text("mcp_connection_primary"),
                "remedy": d.live_text("mcp_connection_remedy"),
                "token_hint": d.live_text("mcp_token_hint"),
                "dry_run_hint": d.live_text("mcp_dry_run_hint"),
                "conflict": d.shortcuts.conflict.text().to_string(),
            })
        }
    };
    json!({
        "config_text": config_text,
        "dialog": dialog,
        "server_running": report.running,
    })
}

/// The audit file, one JSON value per line, the timestamp (the wall clock) set aside.
fn audit_lines(paths: &Paths) -> Value {
    let text = std::fs::read_to_string(paths.mcp_audit_log()).unwrap_or_default();
    Value::Array(
        text.lines()
            .map(|l| {
                let mut v: Value = serde_json::from_str(l).expect("an audit line is JSON");
                v["timestamp"] = json!("T");
                v
            })
            .collect(),
    )
}

/// One raw HTTP request to the server the window started (6.i's wire differential). The client
/// runs on a thread of its own while this one keeps the main loop turning, because a tool call
/// is answered by the window. `$SID` in a header value is the session id the last reply gave.
fn wire_step(step: &Value) {
    let port = step["port"].as_u64().unwrap() as u16;
    let sid = WIRE_SID.with(|s| s.borrow().clone());
    let method = step["method"].as_str().unwrap().to_owned();
    let body = step["body"].as_str().unwrap_or("").to_owned();
    let headers: Vec<(String, String)> = step["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h[0].as_str().unwrap().to_owned(),
                h[1].as_str().unwrap().replace("$SID", &sid),
            )
        })
        .collect();
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let out = slot.clone();
    std::thread::spawn(move || {
        let reply = wire_http(port, &method, &headers, &body);
        *out.lock().unwrap() = Some(reply);
    });
    let done = pump_until(30_000, || slot.lock().unwrap().is_some());
    assert!(done, "the server did not answer");
    let (status, headers, body) = slot.lock().unwrap().take().unwrap();
    let mut shown = serde_json::Map::new();
    for (n, v) in &headers {
        match n.as_str() {
            "date" | "content-length" | "transfer-encoding" | "connection" => {}
            "mcp-session-id" => {
                WIRE_SID.with(|s| *s.borrow_mut() = v.clone());
                shown.insert(n.clone(), json!(format!("<{} chars>", v.len())));
            }
            _ => {
                shown.insert(n.clone(), json!(v));
            }
        }
    }
    let body = match body.strip_prefix("event: message\r\ndata: ") {
        Some(rest) => {
            let data = rest.strip_suffix("\r\n\r\n").unwrap_or(rest);
            serde_json::from_str(data).unwrap_or(json!(body))
        }
        None => json!(body),
    };
    let reply = json!({"status": status, "headers": shown, "body": body});
    WIRE_LAST.with(|l| *l.borrow_mut() = reply);
}

fn wire_http(
    port: u16,
    method: &str,
    headers: &[(String, String)],
    body: &str,
) -> (u16, Vec<(String, String)>, String) {
    use std::io::Read;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(25)))
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
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").expect("a reply");
    let mut lines = head.lines();
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(n, v)| (n.to_lowercase(), v.to_owned()))
        .collect();
    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v == "chunked");
    let mut body = String::new();
    if chunked {
        let mut rest = rest;
        while let Some((size, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            body.push_str(&tail[..n]);
            rest = &tail[n + 2..];
        }
    } else {
        body = rest.to_owned();
    }
    (status, headers, body)
}
