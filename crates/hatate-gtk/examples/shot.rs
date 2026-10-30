//! Renders the real window to a PNG, headless (`xvfb-run`), with a temp profile and the bundled
//! fonts: `cargo run --example shot -- OUT.png [dark|light] [WxH|min] [STATE]`. A test-only tool:
//! the shipped binary has no way to do this. Shows only the synthetic profile it creates.
//!
//! `shot --all OUTDIR [--only a,b]` renders the whole shot set (the `SHOT_SET` table: every state, dark and
//! light, at 1100x800 and, where the table says so, at the minimum size), one child process per PNG,
//! and writes `OUTDIR/index.tsv` with each PNG's header size next to the size its surface states as its
//! minimum (DAN-2356: the header decides, not the host toolkit's numbers).
//!
//! `STATE` is one of `empty` (default), `rows`, `crashed`, `survived`, `quota`, `dialog` (the
//! Clear Session question), `dialog-nolabel` (the same with the Queue page's placeholder label hidden, DAN-2256), `logs` (the log viewer), `logs-selected` (the log viewer with the path
//! line selected and focused) or `logs-selected-body` (the log body selected and focused), `menu` (the first menu open) or `focus` (the
//! first button focused with the ring showing), `failed` (the Failed banner), `activity` (the
//! Activity page) or `parser-health` (the Parser Health dialog), `focus-files` / `focus-folder` (the stamp-filled Add
//! files, the hollow Add folder, focused with the ring showing) or `hover-folder` (Add folder under
//! the pointer) or `menu-hover` (the Files menu open with `Save Session As…` under the pointer). The Queue states (6.d) are `queue` (24 rows
//! with real pictures, all six statuses), `queue-selected` (two rows selected), `queue-sorted` (by
//! Status, the chevron showing), `queue-filter` (a filename filter with the hidden count),
//! `queue-filter-empty` (a filter that hides every row), `queue-menu` (the row context menu over a
//! selected row), `queue-remove` (the Remove question), `queue-focus` (the first row focused with
//! the ring showing) and `queue-focus-selected` (the same row, selected), `queue-5000` (5,000 rows, scrolled into the middle) and `queue-5000-scroll`
//! (the same, printing the scroll frame timing). The Review states (6.f) are `review` (a Good row with two
//! matches and a tag list, Side by side), `review-empty` (nothing selected), `review-nomatch` (a Not found row),
//! `review-picker` (the candidate picker open), `review-addtags` and `review-conditions` (the two dialogs) and
//! `review-focus-<combo|mark|addtags|taglist|view|next>` (that control focused with the ring showing). The Wipe and Differences states (6.f-2) are
//! `review-wipe`, `review-differences`, `review-wipe-nomatch` (only the local picture), `review-error` (the local file is gone; design
//! gate 2.8), `review-focus-<wslider|wview|zoom|popout>` (a Wipe control focused with the ring showing), `review-compare` and
//! `review-compare-differences` (the compare window, which is its own surface), `review-compare-nomatch` (its "no match" box) and
//! `review-focus-compare-<slider|diff>`. The Hydrus and Pawchive states (6.g) are `hydrus-delete` (the Delete from Hydrus box, Cancel default), `hydrus-query`, `hydrus-query-results` and `hydrus-query-focus` (the Query Hydrus window, empty with its search button focused, so the ring is the diff against `hydrus-query`; with synthetic results as `hydrus-query-results`), `hydrus-empty` (the empty page with Query Hydrus enabled) and `pawchive` (the Pawchive Index window over an empty index). The Settings states (6.h) are `settings-<0..8>` (that tab of the Settings window, in Python's order: General to MCP), `settings-find` (a query with results), `settings-find-none` ("Nothing matches that."), `settings-lens` (the Engine page with Google Lens ticked and its dependencies missing; libadwaita presents the warning box as a window of its own, which this snapshot does not hold), `settings-found` (a jumped-to setting wearing the stamp) and `settings-focus-<find|tab|ok|cancel|check|spin|combo|entry>` (that control focused with the ring showing; `check` and `entry` are on the Hydrus page, `combo` on the Import page, the rest on General). The 6.i states are `settings-mcp-running` and `settings-mcp-held` (the MCP tab with the server started at launch on a free port, and with the port held by another listener), `settings-mcp-audit` (synthetic Recent tool calls, All), `settings-shortcuts-conflict`, `settings-shortcuts-record` (the Shortcuts tab with two actions on one key, and with a recorder waiting for its key) and `settings-focus-<mcp|mcp-tier|shortcut>` (the server box, the research tier's check box, the first key button), and `settings-mcp-tiers`, `settings-mcp-tiers-dry`, `settings-mcp-tiers-low` (the middle of the MCP page: scrolled to the Dry run box, the same with Dry run ticked, scrolled to the Hydrus box). The Got it box libadwaita presents as a window of its own, which this snapshot does not hold. The 6.j follow-up states (DAN-2472) are `parser-health-bottom` (the site list scrolled to its end, `zerochan` drawn) and `review-picker-kb` (the candidate popup with the keyboard's row, the second, stamped and the current candidate marked); `settings-found` has its first result as the keyboard's row, and `settings-lens-box` is the Lens alert's own toplevel. The size `min` is the window's stated minimum.

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::config::Settings;
use hatate_core::model::{Candidate, Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::SessionStore;
use hatate_gtk::settings_form::Control;
use hatate_gtk::settings_tests::Test;
use hatate_gtk::window::{Env, MIN_HEIGHT, MIN_WIDTH, MainWindow};
use hatate_gtk::{app, fonts};
use std::cell::Cell;
use std::ffi::OsString;
use std::rc::Rc;

const NOW: f64 = 1_792_000_000.0;

/// The Queue's synthetic rows: small real pictures (so the thumbnail column has something to
/// decode), the six statuses in turn, and a measured similarity on the searched ones.
fn queue_rows(pics: &std::path::Path, n: usize) -> Vec<Entry> {
    (0..n)
        .map(|i| {
            let path = pics.join(format!("synthetic-{i:04}.png"));
            let hue = (i as u8).wrapping_mul(40);
            image::RgbaImage::from_fn(96, 64, |x, y| {
                image::Rgba([
                    hue.wrapping_add(x as u8),
                    60 + y as u8,
                    200u8.wrapping_sub(hue),
                    255,
                ])
            })
            .save(&path)
            .expect("synthetic picture");
            let mut e = Entry::new(path.to_string_lossy().into_owned());
            e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
            if matches!(e.status, MatchStatus::Good | MatchStatus::Poor) {
                let mut c = Candidate::new(format!("https://example.invalid/post/{i}"));
                c.similarity = if e.status == MatchStatus::Good {
                    94.0
                } else {
                    71.0
                };
                c.similarity_measured = true;
                c.width = Some(1200);
                c.height = Some(800);
                e.candidates.push(c);
            }
            e.hydrus_hash = Some(format!("{i:064x}"));
            e
        })
        .collect()
}

/// The shot set: `(state, also at the minimum size)`. Every state is rendered dark and light at 1100x800;
/// the `true` ones again at the minimum size of the surface they show. States that only exist to print
/// a measurement (`queue-5000*`, `tab*`) are not here.
const SHOT_SET: &[(&str, bool)] = &[
    ("empty", true),
    ("rows", false),
    ("crashed", false),
    ("survived", false),
    ("quota", true),
    ("dialog", false),
    ("dialog-nolabel", false),
    ("logs", false),
    ("logs-selected", false),
    ("logs-selected-body", false),
    ("menu", false),
    ("menu-hover", false),
    ("focus", false),
    ("focus-files", false),
    ("focus-folder", false),
    ("hover-folder", false),
    ("failed", true),
    ("activity", true),
    ("parser-health", true),
    ("parser-health-bottom", true),
    ("review", true),
    ("review-empty", true),
    ("review-nomatch", true),
    ("review-picker", true),
    ("review-picker-kb", true),
    ("review-addtags", true),
    ("review-conditions", true),
    ("review-focus-combo", false),
    ("review-focus-mark", true),
    ("review-focus-next", false),
    ("review-focus-addtags", false),
    ("review-focus-taglist", false),
    ("review-focus-view", false),
    ("review-wipe", true),
    ("review-differences", true),
    ("review-wipe-nomatch", true),
    ("review-error", true),
    ("review-focus-wslider", true),
    ("review-focus-wview", false),
    ("review-focus-zoom", false),
    ("review-focus-popout", false),
    ("review-compare", false),
    ("review-compare-differences", false),
    ("review-compare-nomatch", false),
    ("review-focus-compare-slider", false),
    ("review-focus-compare-diff", false),
    ("hydrus-empty", true),
    ("hydrus-delete", true),
    ("hydrus-query", true),
    ("hydrus-query-results", true),
    ("hydrus-query-focus", true),
    ("pawchive", true),
    ("pawchive-artists", true),
    ("hydrus-delete-confirm", true),
    ("settings-0", true),
    ("settings-1", true),
    ("settings-2", true),
    ("settings-3", true),
    ("settings-4", true),
    ("settings-5", true),
    ("settings-6", true),
    ("settings-7", true),
    ("settings-8", true),
    ("settings-find", true),
    ("settings-find-none", true),
    ("settings-found", true),
    ("settings-lens", true),
    ("settings-focus-find", false),
    ("settings-focus-tab", false),
    ("settings-focus-ok", false),
    ("settings-focus-cancel", false),
    ("settings-focus-check", false),
    ("settings-focus-spin", false),
    ("settings-focus-combo", false),
    ("settings-focus-entry", false),
    ("settings-mcp-running", true),
    ("settings-mcp-held", true),
    ("settings-mcp-audit", true),
    ("settings-shortcuts-conflict", true),
    ("settings-shortcuts-record", true),
    ("settings-focus-mcp", false),
    ("settings-focus-mcp-tier", true),
    ("settings-focus-shortcut", false),
    ("settings-mcp-tiers", true),
    ("settings-mcp-tiers-dry", true),
    ("settings-mcp-tiers-low", true),
    ("queue", true),
    ("queue-selected", false),
    ("queue-sorted", false),
    ("queue-filter", false),
    ("queue-filter-empty", true),
    ("queue-menu", false),
    ("queue-remove", false),
    ("queue-focus", false),
    ("queue-focus-selected", false),
    ("parser-health-kb", true),
    ("settings-lens-box", true),
    // 6.h-2: the five site Tests and the Hydrus Test / Fetch in each state, and the Review Shortcuts dialog.
    ("settings-tests-running", true),
    ("settings-tests-ok", true),
    ("settings-tests-error", true),
    ("settings-tests-running-low", true),
    ("settings-tests-ok-low", true),
    ("settings-tests-error-low", true),
    ("settings-hydrus-running", true),
    ("settings-hydrus-ok", true),
    ("settings-hydrus-failed", true),
    ("settings-hydrus-fetched", true),
    ("settings-hydrus-fetched-open", true),
    ("settings-focus-test", false),
    ("settings-focus-fetch", false),
    ("shortcuts-dialog", true),
    ("shortcuts-dialog-overrides", true),
    ("shortcuts-focus-close", false),
];

/// The size a state's surface states as its minimum, for the `fit` column of `index.tsv`; `None` for a
/// surface with no stated minimum (the log viewer, Parser Health, the compare window, alert dialogs
/// are sized by their content).
fn stated_minimum(state: &str) -> Option<(i32, i32)> {
    if state.starts_with("hydrus-query") {
        Some((
            hatate_gtk::hydrus_query::MIN_WIDTH,
            hatate_gtk::hydrus_query::MIN_HEIGHT,
        ))
    } else if state.starts_with("shortcuts") {
        Some((hatate_gtk::shortcuts_dialog::MIN_WIDTH, 360))
    } else if state == "settings-lens-box" {
        // An alert dialog's own toplevel, sized by its content.
        None
    } else if state.starts_with("settings") {
        Some((
            hatate_gtk::settings_dialog::MIN_WIDTH,
            hatate_gtk::settings_dialog::MIN_HEIGHT,
        ))
    } else if state.starts_with("pawchive") {
        Some((
            hatate_gtk::pawchive_dialog::MIN_WIDTH,
            hatate_gtk::pawchive_dialog::MIN_HEIGHT,
        ))
    } else if state.starts_with("parser-health") {
        // Python only calls `resize(760, 460)` (`gtk-6e.md` section 3 calls it the minimum): the dialog's
        // own size, which the shot must be, so the fit column checks it. The dialog also states it as
        // its floor (`parser_health::MIN_WIDTH`/`MIN_HEIGHT`, DAN-2472 item 7).
        Some((760, 460))
    } else if state.starts_with("logs") || state.contains("compare") {
        None
    } else {
        // `window.width()` is the window without its 5 px client-side shadow: 1100x800 renders as a
        // 1090x790 PNG, so the minimum does too.
        Some((MIN_WIDTH - 10, MIN_HEIGHT - 10))
    }
}

/// `width x height` from a PNG's IHDR.
fn png_size(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    (&head[..8] == b"\x89PNG\r\n\x1a\n").then(|| {
        (
            u32::from_be_bytes(head[16..20].try_into().unwrap()),
            u32::from_be_bytes(head[20..24].try_into().unwrap()),
        )
    })
}

/// `shot --all OUTDIR [--only a,b,c]`: the whole set, a child process per PNG (a GTK application runs
/// once per process). Exit code 1 if any child failed or wrote no PNG; a minimum-size shot whose header
/// differs from the stated minimum is marked `OVER`/`UNDER` in the `fit` column, not failed here.
fn run_all() -> glib::ExitCode {
    let mut args = std::env::args().skip(2);
    let dir = std::path::PathBuf::from(args.next().expect("OUTDIR"));
    let only: Option<Vec<String>> = match (args.next().as_deref(), args.next()) {
        (Some("--only"), Some(list)) => Some(list.split(',').map(str::to_owned).collect()),
        _ => None,
    };
    std::fs::create_dir_all(&dir).expect("OUTDIR");
    let exe = std::env::current_exe().expect("this binary");
    let (mut index, mut failed, mut n) = (
        String::from("file\tstate\tscheme\tsize\tpng\tstated_min\tfit\n"),
        0,
        0,
    );
    for scheme in ["dark", "light"] {
        for &(state, min) in SHOT_SET {
            if only.as_ref().is_some_and(|o| !o.iter().any(|s| s == state)) {
                continue;
            }
            for size in std::iter::once("1100x800").chain(min.then_some("min")) {
                let name = if size == "min" {
                    format!("{state}-min-{scheme}.png")
                } else {
                    format!("{state}-{scheme}.png")
                };
                let out = dir.join(&name);
                let _ = std::fs::remove_file(&out);
                let run = std::process::Command::new(&exe)
                    .arg(&out)
                    .args([scheme, size, state])
                    .output();
                let ok = run.as_ref().is_ok_and(|r| r.status.success()) && out.exists();
                let png = png_size(&out);
                let stated = (size == "min").then(|| stated_minimum(state)).flatten();
                let fit = match (ok, size, png, stated) {
                    (false, ..) => "FAILED".to_string(),
                    (_, "min", Some((w, h)), Some((sw, sh)))
                        if (w as i32, h as i32) == (sw, sh) =>
                    {
                        "FIT".into()
                    }
                    (_, "min", Some((w, h)), Some((sw, sh)))
                        if w as i32 >= sw && h as i32 >= sh =>
                    {
                        "OVER".into()
                    }
                    (_, "min", Some(_), Some(_)) => "UNDER".into(),
                    _ => "-".into(),
                };
                if !ok {
                    failed += 1;
                    eprintln!(
                        "FAILED {name}: {}",
                        run.map(|r| String::from_utf8_lossy(&r.stderr)
                            .lines()
                            .last()
                            .unwrap_or("")
                            .to_string())
                            .unwrap_or_default()
                    );
                }
                n += 1;
                index.push_str(&format!(
                    "{name}\t{state}\t{scheme}\t{size}\t{}\t{}\t{fit}\n",
                    png.map_or("-".into(), |(w, h)| format!("{w}x{h}")),
                    stated.map_or("-".into(), |(w, h)| format!("{w}x{h}")),
                ));
            }
        }
    }
    std::fs::write(dir.join("index.tsv"), &index).expect("index.tsv");
    println!(
        "SHOT-SET {n} shots, {failed} failed, index {}",
        dir.join("index.tsv").display()
    );
    if failed == 0 {
        glib::ExitCode::SUCCESS
    } else {
        glib::ExitCode::FAILURE
    }
}

/// The window's pixels as `(width, height, BGRA premultiplied)`.
fn pixels_of(
    win: &gtk4::Window,
    renderer: &gtk4::gsk::Renderer,
) -> Option<(usize, usize, Vec<u8>)> {
    let tex = texture_of(win.upcast_ref(), renderer)?;
    let (w, h) = (tex.width() as usize, tex.height() as usize);
    let mut buf = vec![0u8; w * h * 4];
    tex.download(&mut buf, w * 4);
    Some((w, h, buf))
}

/// What the Tab walk has seen so far.
#[derive(Default)]
struct TabWalk {
    i: usize,
    plain: Option<(usize, usize, Vec<u8>)>,
    seen: Vec<String>,
    stops: Vec<gtk4::Widget>,
    without: usize,
}

/// `shot ... --tabs`: Tab walked from the top of `win`, one `TABSTOP` line per stop (DAN-2424 line 7).
/// For each stop the window is drawn twice, once with the focus taken away and once with the stop focused
/// and the ring showing, and the pixels inside the stop's bounds (plus 6 px) are compared: a stop where
/// nothing changes has no visible indicator (`indicator=NO`). A crop of the stop with its ring is written
/// next to the shot as `<out stem>-tab<i>.png`, so the table can name the file that shows each stop.
/// The walk is a timer, two ticks per stop, because a focus change reaches the style only when the frame
/// clock runs, and it does not run inside the tick that started the walk.
fn start_tab_walk(
    win: gtk4::Window,
    renderer: gtk4::gsk::Renderer,
    out: std::path::PathBuf,
    app: Option<gtk4::Application>,
) {
    win.set_focus_visible(true);
    let st = std::rc::Rc::new(std::cell::RefCell::new(TabWalk::default()));
    glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
        let mut st = st.borrow_mut();
        let finish = |st: &TabWalk| {
            println!(
                "TABWALK stops={} without_indicator={}",
                st.seen.len(),
                st.without
            );
            if let Some(a) = &app {
                a.quit();
            }
            glib::ControlFlow::Break
        };
        if st.plain.is_none() {
            // phase A: the focus is taken away; the next tick draws the window without a ring
            if st.i >= 80 {
                return finish(&st);
            }
            GtkWindowExt::set_focus(&win, gtk4::Widget::NONE);
            st.plain = Some((0, 0, Vec::new()));
            return glib::ControlFlow::Continue;
        }
        if st.plain.as_ref().is_some_and(|p| p.2.is_empty()) {
            // phase B: record the plain window, then move the focus on from the last stop
            st.plain = pixels_of(&win, &renderer);
            if st.plain.is_none() {
                return finish(&st);
            }
            if let Some(prev) = st.stops.last() {
                prev.grab_focus();
            }
            // A headless pointer that moves turns the ring off (GTK drops `focus-visible` on pointer
            // input); a person tabbing has just used the keyboard, so it is on.
            win.set_focus_visible(true);
            win.child_focus(gtk4::DirectionType::TabForward);
            win.set_focus_visible(true);
            return glib::ControlFlow::Continue;
        }
        // phase C: the stop is focused and has been drawn
        let Some(f) = GtkWindowExt::focus(&win) else {
            return finish(&st);
        };
        let what = describe(&f).replace(['\n', '|'], " ");
        let i = st.i;
        if st.seen.first() == Some(&what) && i > 0 {
            return finish(&st);
        }
        let (w, h, plain) = st.plain.take().unwrap();
        let Some((_, _, ringed)) = pixels_of(&win, &renderer) else {
            return finish(&st);
        };
        let b = f
            .compute_bounds(&win)
            .unwrap_or_else(|| gtk4::graphene::Rect::new(0.0, 0.0, 0.0, 0.0));
        let clamp = |v: f32, hi: usize| (v as i32).clamp(0, hi as i32) as usize;
        let (x0, y0) = (clamp(b.x() - 6.0, w), clamp(b.y() - 6.0, h));
        let (x1, y1) = (
            clamp(b.x() + b.width() + 6.0, w),
            clamp(b.y() + b.height() + 6.0, h),
        );
        let mut diff = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let o = (y * w + x) * 4;
                if (0..4).any(|c| plain[o + c].abs_diff(ringed[o + c]) > 8) {
                    diff += 1;
                }
            }
        }
        let crop = out.with_file_name(format!(
            "{}-tab{i}.png",
            out.file_stem().and_then(|s| s.to_str()).unwrap_or("shot")
        ));
        if x1 > x0 && y1 > y0 {
            let mut c = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for y in y0..y1 {
                c.extend_from_slice(&ringed[(y * w + x0) * 4..(y * w + x1) * 4]);
            }
            gdk::MemoryTexture::new(
                (x1 - x0) as i32,
                (y1 - y0) as i32,
                gdk::MemoryFormat::B8g8r8a8Premultiplied,
                &glib::Bytes::from_owned(c),
                (x1 - x0) * 4,
            )
            .save_to_png(&crop)
            .expect("crop");
        }
        if diff == 0 {
            st.without += 1;
        }
        println!(
            "TABSTOP {i} {what} | bounds {},{} {}x{} | diffpx {diff} | indicator={} | {}",
            b.x() as i32,
            b.y() as i32,
            b.width() as i32,
            b.height() as i32,
            if diff > 0 { "yes" } else { "NO" },
            crop.file_name().and_then(|s| s.to_str()).unwrap_or("-"),
        );
        st.stops.push(f);
        st.seen.push(what);
        st.i += 1;
        glib::ControlFlow::Continue
    });
}

/// The surfaces whose Tab order the exit check lists (`shot --tabs`): every window and page, in the
/// states that put controls on them.
const TAB_STATES: &[&str] = &[
    "empty",
    "rows",
    "queue-selected",
    "queue-filter",
    "crashed",
    "survived",
    "quota",
    "failed",
    "activity",
    "logs",
    "parser-health",
    "review",
    "review-nomatch",
    "review-picker",
    "review-wipe",
    "review-differences",
    "review-compare",
    "hydrus-empty",
    "hydrus-query",
    "hydrus-query-results",
    "pawchive-artists",
    "settings-0",
    "settings-1",
    "settings-2",
    "settings-3",
    "settings-4",
    "settings-5",
    "settings-6",
    "settings-7",
    "settings-8",
    "settings-mcp-running",
    "settings-shortcuts-conflict",
    "settings-hydrus-fetched",
    "shortcuts-dialog",
];

/// `shot --tabs OUTDIR [--only a,b]`: the Tab walk of every surface in `TAB_STATES`, dark and light, a child
/// process each. Writes `OUTDIR/tabs.tsv` (state, scheme, stop, widget, bounds, changed pixels, indicator,
/// the crop that shows the stop) and the crops. Exit code 1 if a walk failed or a stop shows no indicator.
fn run_tabs() -> glib::ExitCode {
    let mut args = std::env::args().skip(2);
    let dir = std::path::PathBuf::from(args.next().expect("OUTDIR"));
    let only: Option<Vec<String>> = match (args.next().as_deref(), args.next()) {
        (Some("--only"), Some(list)) => Some(list.split(',').map(str::to_owned).collect()),
        _ => None,
    };
    std::fs::create_dir_all(&dir).expect("OUTDIR");
    let exe = std::env::current_exe().expect("this binary");
    let mut tsv = String::from("state\tscheme\tstop\twidget\tbounds\tdiffpx\tindicator\tcrop\n");
    let (mut stops, mut without, mut failed) = (0, 0, 0);
    for &state in TAB_STATES {
        if only.as_ref().is_some_and(|o| !o.iter().any(|s| s == state)) {
            continue;
        }
        for scheme in ["dark", "light"] {
            let out = dir.join(format!("{state}-{scheme}.png"));
            let run = std::process::Command::new(&exe)
                .arg(&out)
                .args([scheme, "1100x800", state, "--tabs"])
                .output();
            let text = run
                .as_ref()
                .map(|r| String::from_utf8_lossy(&r.stdout).into_owned())
                .unwrap_or_default();
            if !run.as_ref().is_ok_and(|r| r.status.success()) || !text.contains("TABWALK") {
                failed += 1;
                eprintln!("FAILED tab walk {state}-{scheme}");
                continue;
            }
            for l in text.lines().filter(|l| l.starts_with("TABSTOP ")) {
                let parts: Vec<&str> = l["TABSTOP ".len()..].split(" | ").collect();
                if parts.len() != 5 {
                    failed += 1;
                    eprintln!("FAILED tab walk {state}-{scheme}: malformed line {l:?}");
                    continue;
                }
                let (i, widget) = parts[0].split_once(' ').unwrap_or((parts[0], ""));
                let ind = parts[3].trim_start_matches("indicator=");
                stops += 1;
                if ind != "yes" {
                    without += 1;
                }
                tsv.push_str(&format!(
                    "{state}\t{scheme}\t{i}\t{widget}\t{}\t{}\t{ind}\t{}\n",
                    parts[1].trim_start_matches("bounds "),
                    parts[2].trim_start_matches("diffpx "),
                    parts[4]
                ));
            }
        }
    }
    std::fs::write(dir.join("tabs.tsv"), &tsv).expect("tabs.tsv");
    println!(
        "TABS {stops} stops, {without} without an indicator, {failed} walks failed, {}",
        dir.join("tabs.tsv").display()
    );
    if failed == 0 && without == 0 {
        glib::ExitCode::SUCCESS
    } else {
        glib::ExitCode::FAILURE
    }
}

/// Every state `shot` renders: the table, the two scroll measurements and the `tab*` walks.
fn known_state(s: &str) -> bool {
    SHOT_SET.iter().any(|&(n, _)| n == s)
        || matches!(s, "queue-5000" | "queue-5000-scroll")
        || s.starts_with("tab")
}

fn main() -> glib::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--all") {
        return run_all();
    }
    if std::env::args().nth(1).as_deref() == Some("--tabs") {
        return run_tabs();
    }
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("OUT.png"));
    let scheme = args.next().unwrap_or_else(|| "dark".into());
    let size = args.next();
    let state = args.next().unwrap_or_else(|| "empty".into());
    let tabs = args.next().as_deref() == Some("--tabs");
    // Python's `render_states.py` fails loudly on a name it does not know and writes nothing
    // (`test_unknown_state_fails_loudly_and_writes_nothing`); a typo here used to render the
    // default window and exit 0 (DAN-2223).
    if !known_state(&state) {
        eprintln!(
            "shot: no such state {state:?}: the states are the SHOT_SET table, queue-5000[-scroll] and tab*"
        );
        return glib::ExitCode::from(2);
    }
    let home = tempfile::tempdir().expect("temp profile");
    let root = home.path().to_path_buf();
    let lookup = move |k: &str| -> Option<OsString> {
        match k {
            "HOME" => Some(root.clone().into()),
            "XDG_CONFIG_HOME" => Some(root.join("config").into()),
            "XDG_CACHE_HOME" => Some(root.join("cache").into()),
            _ => None,
        }
    };
    let paths = Paths::from_lookup(lookup).expect("paths");
    std::fs::create_dir_all(paths.config_dir()).expect("profile dir");
    let mut settings = Settings::with_defaults(&paths);
    settings.theme = scheme;
    if state.starts_with("review") {
        // Everything the page shows is already in the rows: nothing to fetch.
        settings.retrieve_tags_from_booru = false;
        settings.drop_dead_matches = false;
    }
    // The 6.i MCP states: the server is on with a synthetic token. `settings-mcp-held` keeps a
    // listener on the port, so the window's server cannot bind it; the others take a free one.
    // `_held` lives to the end of `main`.
    let mut _held = None;
    if state.starts_with("settings-mcp") || state.starts_with("settings-focus-mcp") {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let port = listener.local_addr().expect("its address").port();
        settings.mcp.enabled = true;
        settings.mcp.port = i64::from(port);
        settings.mcp.token = "synthetic-token-for-shots-only".into();
        if state == "settings-mcp-held" {
            _held = Some(listener);
        } else {
            drop(listener);
        }
        if state == "settings-mcp-audit" {
            // Synthetic lines in the audit log's own format (`mcp/audit.rs::entry_line`).
            let lines = [
                ("list_queue", "read", true, false, "null", "{\"count\": 12}"),
                (
                    "select_candidate",
                    "local_write",
                    true,
                    false,
                    "null",
                    "{\"row\": 3, \"selected_candidate_index\": 1}",
                ),
                (
                    "toggle_reviewed",
                    "local_write",
                    true,
                    true,
                    "null",
                    "{\"row\": 4}",
                ),
                (
                    "send_to_hydrus",
                    "hydrus_write",
                    false,
                    false,
                    "\"off\"",
                    "null",
                ),
                ("remove_row", "destructive", false, false, "\"off\"", "null"),
            ];
            let mut text = String::new();
            for (i, (tool, tier, allowed, dry, reason, outcome)) in lines.iter().enumerate() {
                text.push_str(&format!(
                    "{{\"timestamp\": \"2026-10-30T08:00:{:02}Z\", \"tool\": \"{tool}\", \"tier\": \"{tier}\", \"allowed\": {allowed}, \"reason\": {reason}, \"dry_run\": {dry}, \"row\": 3, \"candidate_index\": null, \"arguments\": {{}}, \"outcome\": {outcome}}}\n",
                    i * 7 + 1
                ));
            }
            if let Some(dir) = paths.mcp_audit_log().parent() {
                std::fs::create_dir_all(dir).expect("audit dir");
            }
            std::fs::write(paths.mcp_audit_log(), text).expect("audit log");
        }
    }
    if state == "shortcuts-dialog-overrides" {
        // two keys changed, one taken off: the dialog shows the keys the app has now, `(unbound)` included
        settings.review_shortcuts = hatate_core::config::OrderedMap(vec![
            ("next_row".to_owned(), "J".to_owned()),
            ("compare".to_owned(), "Ctrl+Shift+K".to_owned()),
            ("remove_row".to_owned(), String::new()),
        ]);
    }
    settings.save(&paths).expect("save settings");
    if let Err(e) = fonts::load(&home.path().join("fonts")) {
        eprintln!("fonts: {e}");
    }

    // Synthetic sessions only: names that say so, never a real path.
    // The files exist (tiny, synthetic), so the missing-file check has nothing to report.
    let pics = home.path().join("synthetic");
    std::fs::create_dir_all(&pics).expect("pictures dir");
    let rows = |n: usize| -> Vec<Entry> {
        (0..n)
            .map(|i| {
                let path = pics.join(format!("image-{i:02}.png"));
                std::fs::write(&path, format!("synthetic image {i}")).expect("synthetic file");
                let mut e = Entry::new(path.to_string_lossy().into_owned());
                e.status = [
                    MatchStatus::Good,
                    MatchStatus::Poor,
                    MatchStatus::NotFound,
                    MatchStatus::NotSearched,
                ][i % 4];
                e.hydrus_hash = Some(format!("{i:064x}"));
                e
            })
            .collect()
    };
    let save = |entries: &[Entry]| {
        let writer = SessionStore::new(&paths).open_writer().expect("writer");
        writer.save(entries, NOW - 600.0).expect("seed");
    };
    let mut unclean = false;
    match state.as_str() {
        "rows" | "dialog" | "dialog-nolabel" | "failed" | "activity" | "parser-health"
        | "parser-health-kb" | "tab-failed" | "tab-activity" => save(&rows(12)),
        st if st.starts_with("review") && st != "review-bare" => save(&review_rows(&pics, st)),
        st if st.starts_with("queue") || st.starts_with("hydrus-delete") => save(&queue_rows(
            &pics,
            if st.starts_with("queue-5000") {
                5000
            } else {
                24
            },
        )),
        "crashed" => {
            unclean = true;
            let mut r = rows(8);
            r[1].status = MatchStatus::Searching;
            r[0].sent_to_hydrus = true;
            r[0].hydrus_import_confirmed = true;
            save(&r);
        }
        "survived" => {
            unclean = true;
            save(&[]);
        }
        "quota" => {
            save(&rows(8));
            let pause = QuotaPause {
                paused_at: NOW - 3600.0,
                searched: 90,
                remaining: 410,
                reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW),
            };
            QuotaPauseFile::new(&paths).record(&pause).expect("pause");
        }
        _ => {}
    }

    let mut env = Env::new(paths, unclean);
    if state.starts_with("logs") {
        // A few synthetic records, so the log body has text to measure.
        for line in [
            "2026-10-24T11:00:00Z  INFO hatate_gtk::app: window up (synthetic)",
            "2026-10-24T11:00:01Z  INFO hatate_gtk::session_ops: session restored, 3 rows (synthetic)",
            "2026-10-24T11:00:02Z  WARN hatate_gtk::startup: previous run did not close cleanly (synthetic)",
        ] {
            env.log_ring.push(line.into());
        }
    }
    env.clock = Rc::new(|| NOW);
    if state == "review-error" {
        // The picture is gone on purpose; the startup "Missing files" question is not what this
        // state shows, so the check is told nothing is missing.
        env.missing_files = std::sync::Arc::new(|_, _| Default::default());
    }
    let out2 = out.clone();
    let code = app::run_with(env, move |window| {
        let (w, h) = match size.as_deref() {
            Some("min") => (MIN_WIDTH, MIN_HEIGHT),
            Some(s) => s
                .split_once('x')
                .map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap()))
                .unwrap_or((1100, 650)),
            None => (1100, 650),
        };
        window.win.set_default_size(w, h);
        prepare(window, &state);
        if size.as_deref() == Some("min") {
            // The 6.g windows are their own surfaces with their own minimum, not the main
            // window's: the Query window states it; the Pawchive window's is its own constant.
            if state.starts_with("hydrus-query") {
                if let Some(d) = window.s.hydrus_query_dialog.borrow().as_ref() {
                    d.window.set_default_size(
                        hatate_gtk::hydrus_query::MIN_WIDTH,
                        hatate_gtk::hydrus_query::MIN_HEIGHT,
                    );
                }
            } else if state.starts_with("settings") {
                if let Some(d) = window.s.settings_dialog.borrow().as_ref() {
                    d.window.set_default_size(
                        hatate_gtk::settings_dialog::MIN_WIDTH,
                        hatate_gtk::settings_dialog::MIN_HEIGHT,
                    );
                }
            } else if state.starts_with("shortcuts") {
                if let Some(d) = window.s.shortcuts_dialog.borrow().as_ref() {
                    d.window
                        .set_default_size(hatate_gtk::shortcuts_dialog::MIN_WIDTH, 360);
                }
            } else if state.starts_with("pawchive") {
                if let Some(d) = window.s.pawchive_dialog.borrow().as_ref() {
                    d.window.set_default_size(
                        hatate_gtk::pawchive_dialog::MIN_WIDTH,
                        hatate_gtk::pawchive_dialog::MIN_HEIGHT,
                    );
                }
            }
        }
        let win = window.win.clone();
        let main_win = window.win.clone();
        let frames = Rc::new(Cell::new(0u32));
        let out = out2.clone();
        let state = state.clone();
        win.add_tick_callback(move |win, _| {
            frames.set(frames.get() + 1);
            if (state == "review-nomatch" || state == "review-addtags") && frames.get() <= 12 {
                // The window never gets smaller once it has grown: what its minimum was while it was
                // first laid out (hosted `ci` shows 812 where the settled minimum is 758).
                let content =
                    libadwaita::prelude::AdwApplicationWindowExt::content(&main_win).unwrap();
                println!(
                    "FRAME {state} {} win={}x{} win_min_h={} content_min_h={} content_at_width={}",
                    frames.get(),
                    win.width(),
                    win.height(),
                    win.measure(gtk4::Orientation::Vertical, win.width()).0,
                    content.measure(gtk4::Orientation::Vertical, -1).0,
                    content.measure(gtk4::Orientation::Vertical, win.width()).0,
                );
                if frames.get() == 2 {
                    println!("FRAMECHAIN {}", tallest_chain(&content, win.width()));
                }
            }
            let settle = match state.as_str() {
                "menu-hover" => 60,
                "queue-5000-scroll" => 400,
                "parser-health-kb" => 90,
                st if st.starts_with("review") => 60,
                _ => 14,
            };
            if frames.get() < settle || !READY.load(std::sync::atomic::Ordering::SeqCst) {
                return glib::ControlFlow::Continue;
            }
            // The log viewer is its own window; its shot is that window's, not the main one's.
            let other = if state.starts_with("logs") {
                Some("Logs")
            } else if state.starts_with("parser-health") {
                Some("Parser Health")
            } else if state.starts_with("hydrus-query") {
                Some("Query Hydrus")
            } else if state.starts_with("pawchive") {
                Some("Pawchive Index")
            } else if state.starts_with("shortcuts") {
                Some(hatate_gtk::shortcuts_dialog::TITLE)
            } else if state == "settings-lens-box" {
                // The box is an alert dialog libadwaita presents as a toplevel of its own: the shot is
                // that toplevel, found by what it holds (its title is the heading, or none).
                Some(ALERT_TOPLEVEL)
            } else if state.starts_with("settings") {
                Some("Settings")
            } else if state.starts_with("review-compare-d")
                || state.starts_with("review-compare-l")
                || state == "review-compare"
                || state.starts_with("review-focus-compare")
            {
                Some("Compare - review-0.png")
            } else {
                None
            };
            let logs_window = other.and_then(|title| {
                gtk4::Window::list_toplevels()
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk4::Window>().ok())
                    .find(|w| {
                        if title == ALERT_TOPLEVEL {
                            return w.is_mapped()
                                && w.upcast_ref::<gtk4::Widget>()
                                    != main_win.upcast_ref::<gtk4::Widget>()
                                && find_type(w.upcast_ref(), "AdwAlertDialog").is_some();
                        }
                        w.is_mapped()
                            && w.title().is_some_and(|t| {
                                t == title || (title == "Pawchive Index" && t.starts_with(title))
                            })
                    })
            });
            if state == "settings-lens-box" {
                for w in gtk4::Window::list_toplevels() {
                    println!(
                        "TOPLEVEL {} mapped={} title={:?}",
                        w.type_().name(),
                        w.is_mapped(),
                        w.downcast_ref::<gtk4::Window>().and_then(|w| w.title())
                    );
                }
            }
            let main_app = win.application();
            let win: &gtk4::Window = logs_window.as_ref().unwrap_or(win.upcast_ref());
            if state.starts_with("review") {
                // What sets the window's height, for a toolkit whose numbers differ from the author's.
                let content =
                    libadwaita::prelude::AdwApplicationWindowExt::content(&main_win).unwrap();
                println!(
                    "TALLEST {state} win={}x{} {}",
                    win.width(),
                    win.height(),
                    tallest_chain(&content, win.width())
                );
            }
            let renderer = win.native().and_then(|n| n.renderer()).expect("renderer");
            let tex = texture_of(win.upcast_ref(), &renderer).expect("snapshot");
            tex.save_to_png(&out).expect("png");
            println!("SHOT {} {}x{}", out.display(), win.width(), win.height());
            if tabs {
                // The walk quits the application when it is done; the popover shots below are not taken.
                start_tab_walk(win.clone(), renderer.clone(), out.clone(), main_app.clone());
                return glib::ControlFlow::Break;
            }
            // Where every label is, for the measuring script (`<shot>.rects`, tab separated:
            // x y w h state text); the contrast table is computed from the pixels under each box.
            let mut rects = String::new();
            // Behind a dialog the page is under the scrim and is not what a person reads: only the
            // dialog's own labels are measured.
            let dialog = if matches!(
                state.as_str(),
                "review-addtags"
                    | "review-conditions"
                    | "review-compare-nomatch"
                    | "hydrus-delete"
                    | "hydrus-delete-confirm"
                    | "settings-lens-box"
            ) {
                find_type(win.upcast_ref(), "AdwAlertDialog")
            } else {
                None
            };
            label_rects(
                dialog.as_ref().unwrap_or(win.upcast_ref()),
                win.upcast_ref(),
                &mut rects,
            );
            std::fs::write(out.with_extension("rects"), rects).expect("rects");
            // The dialog's widget tree, for reading a toolkit-specific paint artefact (DAN-2245
            // item 4): type, css classes, border box in window coordinates, mapped, first text.
            if state.starts_with("dialog") {
                let mut tree = String::new();
                tree_dump(win.upcast_ref(), win.upcast_ref(), 0, &mut tree);
                std::fs::write(out.with_extension("tree"), tree).expect("tree");
            }
            // A popover is its own surface, so the window's snapshot does not hold it.
            if let Some(p) = visible_popover_menu(win.upcast_ref()) {
                if let Some(tex) = texture_of(p.upcast_ref(), &renderer) {
                    let popover_png = out.with_file_name(format!(
                        "{}-popover.png",
                        out.file_stem().unwrap().to_string_lossy()
                    ));
                    tex.save_to_png(&popover_png).expect("popover png");
                    let mut prects = String::new();
                    label_rects(p.upcast_ref(), p.upcast_ref(), &mut prects);
                    std::fs::write(popover_png.with_extension("rects"), prects).expect("rects");
                    println!(
                        "SHOT {} {}x{}",
                        popover_png.display(),
                        p.width(),
                        p.height()
                    );
                }
            }
            if let Some(app) = main_app {
                app.quit();
            }
            glib::ControlFlow::Break
        });
    });
    drop(home);
    code
}

/// The 6.g states: a synthetic access key (the page's Hydrus actions are gated on it), then the
/// surface. Nothing here reaches a server: the Query window's results are put in the list by
/// hand, and the Delete box is shown before its request.
fn prepare_hydrus(window: &MainWindow, state: &str) {
    window.s.settings.lock().unwrap().hydrus.access_key =
        hatate_core::config::Secret::new("synthetic-access-key");
    match state {
        "hydrus-empty" => {}
        "hydrus-delete" => {
            let ids = window.s.queue_view.visible_ids();
            window.delete_rows_from_hydrus(&ids[1..3]);
        }
        // The destructive confirm itself (Cancel default, Delete in the destructive style), with
        // the text and detail lines `on_delete_states` builds for 2 deletable + 1 left alone.
        "hydrus-delete-confirm" => window.s.prompter.message_detailed(
            hatate_gtk::dialogs::Kind::Warning,
            "Delete from Hydrus",
            "Tell Hydrus to delete 2 file(s)?\n\n1 of the selected file(s) aren't in Hydrus and \
             will be left alone.\n\nHydrus moves them to its trash, so you can still get them \
             back from Hydrus until the trash is emptied.\n\nThey'll also be taken out of this \
             list.",
            "synthetic-0001.png\nsynthetic-0002.png",
            hatate_gtk::dialogs::Buttons::DeleteCancel,
            Box::new(|_| {}),
        ),
        "pawchive" => window.activate_action("pawchive-index"),
        // Three artists with images, one selected, one being indexed (Stop enabled, the progress
        // line showing): the populated Pawchive window.
        "pawchive-artists" => {
            let index =
                hatate_core::pawchive_index::PawchiveIndex::new(window.s.paths.pawchive_index_db());
            for (service, id, name, images, posts) in [
                ("patreon", "1001", "Synthetic Artist", 1284usize, 96usize),
                ("fanbox", "2002", "Another Synthetic", 37, 5),
                ("fanbox", "3003", "Third Synthetic", 0, 0),
            ] {
                index.add_creator(service, id, Some(name)).expect("artist");
                let rows: Vec<_> = (0..images)
                    .map(|i| hatate_core::pawchive_index::ImageRow {
                        service: service.to_owned(),
                        user_id: id.to_owned(),
                        post_id: format!("p{i}"),
                        page_index: 0,
                        sha256: format!("{i:064x}"),
                        path: format!("/synthetic/{id}/{i}.png"),
                        title: None,
                        dhash8: Some(format!("{i:016x}")),
                        dhash16: Some(format!("{i:064x}")),
                    })
                    .collect();
                index.add_images(&rows).expect("images");
                if posts > 0 {
                    index
                        .finish_creator(service, id, Some(name), posts)
                        .expect("stamp");
                }
            }
            window.activate_action("pawchive-index");
            let d = window
                .s
                .pawchive_dialog
                .borrow()
                .clone()
                .expect("the Pawchive window is open");
            d.select_creators(&[("fanbox".to_owned(), "2002".to_owned())]);
            d.set_busy(true);
            d.on_progress("Indexing Third Synthetic (fanbox)", 3, 8);
        }
        _ => {
            window.activate_action("query-hydrus");
            let dialog = window
                .s
                .hydrus_query_dialog
                .borrow()
                .clone()
                .expect("the Query Hydrus window is open");
            if !matches!(state, "hydrus-query" | "hydrus-query-focus") {
                dialog.set_tags_text("creator:synthetic artist\nsolo");
                for text in [
                    "00000000aaaa\u{2026}  (image/png, 640x480, 4 tag(s))",
                    "00000001bbbb\u{2026}  (image/jpeg, 1024x768, 12 tag(s)) [already in list]",
                    "00000002cccc\u{2026}  (image/png, 800x600, 7 tag(s))",
                ] {
                    let label = gtk4::Label::new(Some(text));
                    label.set_xalign(0.0);
                    label.set_wrap(true);
                    label.set_margin_start(8);
                    label.set_margin_end(8);
                    label.set_margin_top(4);
                    label.set_margin_bottom(4);
                    dialog.results.append(&label);
                }
                dialog.select_results(&[0, 2]);
            }
            if state == "hydrus-query-focus" {
                dialog.window.set_focus_visible(true);
                dialog.search_button.grab_focus();
            }
        }
    }
}

/// What the state does once the window is up.
fn prepare(window: &MainWindow, state: &str) {
    match state {
        st if st.starts_with("queue") => prepare_queue(window, st),
        st if st.starts_with("review") => prepare_review(window, st),
        st if st.starts_with("hydrus") || st.starts_with("pawchive") => prepare_hydrus(window, st),
        st if st.starts_with("settings") => prepare_settings(window, st),
        st if st.starts_with("shortcuts") => {
            window.action_show_shortcuts();
            if st == "shortcuts-focus-close" {
                if let Some(d) = window.s.shortcuts_dialog.borrow().as_ref() {
                    d.window.set_focus_visible(true);
                    d.close_button.grab_focus();
                }
            }
        }
        "dialog" => window.activate_action("clear-session"),
        // The control for the glyph fragment under the dialog (DAN-2256): the same shot with the
        // Queue page's placeholder label hidden, so any pixel under the dialog's border that
        // survives is not that label's descenders.
        "dialog-nolabel" => {
            hide_label_starting(window.win.upcast_ref(), "12 image(s)");
            window.activate_action("clear-session");
        }
        "logs" => window.activate_action("view-logs"),
        // 6.e: the Failed banner (a synthetic fault message, path-free), the Activity page and
        // the Parser Health dialog (its live check is off in this build, DAN-2290).
        "failed" => window.show_failed_banner("The run stopped on a fault (synthetic)."),
        "activity" => window.set_mode("activity"),
        "parser-health" => window.activate_action("parser-health"),
        // The list scrolled to its end, so the last site (`zerochan`) is drawn: what End does from
        // anywhere in the dialog (DAN-2472 item 1), set on the adjustment after the dialog is laid out.
        "parser-health-bottom" => {
            window.activate_action("parser-health");
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), || {
                let dialog = gtk4::Window::list_toplevels()
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk4::Window>().ok())
                    .find(|w| w.title().is_some_and(|t| t == "Parser Health"));
                if let Some(sw) = dialog
                    .and_then(|d| find_type(d.upcast_ref(), "GtkScrolledWindow"))
                    .and_then(|w| w.downcast::<gtk4::ScrolledWindow>().ok())
                {
                    let adj = sw.vadjustment();
                    adj.set_value(adj.upper() - adj.page_size());
                }
            });
        }
        // The dialog as a person meets it after opening it from the keyboard (menu, then Enter): the
        // toolkit shows the focus ring only after a key, which a shot has no key to give, so the
        // dialog's toplevel is told the last input was the keyboard and the focus is moved in (DAN-2330 item 2).
        "parser-health-kb" => {
            window.activate_action("parser-health");
            glib::timeout_add_local_once(std::time::Duration::from_millis(60), || {
                let dialog = gtk4::Window::list_toplevels()
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk4::Window>().ok())
                    .find(|w| w.title().is_some_and(|t| t == "Parser Health"));
                if let Some(d) = dialog {
                    d.set_focus_visible(true);
                    if GtkWindowExt::focus(&d).is_none() {
                        d.child_focus(gtk4::DirectionType::TabForward);
                    }
                }
            });
        }
        "logs-selected" | "logs-selected-body" => {
            window.activate_action("view-logs");
            let viewer = window
                .s
                .log_viewer
                .borrow()
                .clone()
                .expect("the log viewer");
            let body = state == "logs-selected-body";
            // The state a person meets when they tab onto the path line: focus on it selects it.
            // After the window is mapped: a selection made before that is cleared by the map.
            glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
                // Only one selection exists at a time (the other widget gives it up).
                if body {
                    let buffer = viewer.text.buffer();
                    viewer.text.grab_focus();
                    buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
                } else {
                    viewer.path_label.grab_focus();
                    viewer.path_label.select_region(0, -1);
                }
            });
        }
        "menu" => {
            if let Some(p) = first_popover_menu(window.win.upcast_ref()) {
                p.popup();
            }
        }
        "tab" | "tab-failed" | "tab-activity" => {
            // Walks Tab from the top and prints what takes focus, for the verdict's Tab order.
            // `tab-failed` and `tab-activity` do it with the Failed banner up or on the Activity page.
            match state {
                "tab-failed" => {
                    window.show_failed_banner("The run stopped on a fault (synthetic).")
                }
                "tab-activity" => window.set_mode("activity"),
                _ => {}
            }
            window.win.set_focus_visible(true);
            let win = window.win.clone();
            glib::idle_add_local_once(move || {
                let mut seen: Vec<String> = Vec::new();
                for i in 0..40 {
                    win.child_focus(gtk4::DirectionType::TabForward);
                    let Some(f) = GtkWindowExt::focus(&win) else {
                        break;
                    };
                    let what = describe(&f);
                    if seen.first() == Some(&what) && i > 0 {
                        break;
                    }
                    println!("TAB {i} {what}");
                    seen.push(what);
                }
            });
        }
        "menu-hover" => {
            if let Some(p) = first_popover_menu(window.win.upcast_ref()) {
                p.popup();
                // The popover builds its rows on popup; once they exist, put the hover state on
                // the row there is no pointer in a headless session to put it on.
                let root = window.win.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
                    match find_row(root.upcast_ref(), "Save Session As\u{2026}") {
                        Some(row) => row.set_state_flags(gtk4::StateFlags::PRELIGHT, false),
                        None => eprintln!("menu-hover: no row 'Save Session As…'"),
                    }
                });
            }
        }
        "hover-folder" => {
            // No pointer in a headless session: put the hover state on the button directly.
            if let Some(b) = find_button_labelled(window.win.upcast_ref(), "Add folder\u{2026}") {
                b.set_state_flags(gtk4::StateFlags::PRELIGHT, false);
            }
        }
        "focus-files" | "focus-folder" => {
            window.win.set_focus_visible(true);
            let want = if state == "focus-files" {
                "Add files\u{2026}"
            } else {
                "Add folder\u{2026}"
            };
            match find_button_labelled(window.win.upcast_ref(), want) {
                Some(b) => {
                    b.grab_focus();
                }
                None => eprintln!("{state}: no button {want}"),
            }
        }
        "focus" => {
            window.win.set_focus_visible(true);
            if let Some(b) = first_button(window.win.upcast_ref()) {
                b.grab_focus();
            }
        }
        _ => {}
    }
}

/// Scrolls the page that holds `w` to its end once it is laid out.
fn scroll_to_end(w: &gtk4::Widget) {
    let mut up = w.parent();
    while let Some(p) = up {
        if let Some(sw) = p.downcast_ref::<gtk4::ScrolledWindow>() {
            let sw = sw.clone();
            gtk4::glib::idle_add_local_once(move || {
                let adj = sw.vadjustment();
                adj.set_value(adj.upper());
            });
            return;
        }
        up = p.parent();
    }
}

/// Scrolls the page that holds `w` so that `w` sits `above` pixels under the top edge of the view,
/// once the page is laid out.
fn scroll_to_top_of(w: &gtk4::Widget, above: f64) {
    let target = w.clone();
    let mut up = w.parent();
    while let Some(p) = up {
        if let Some(sw) = p.downcast_ref::<gtk4::ScrolledWindow>() {
            let sw = sw.clone();
            gtk4::glib::idle_add_local_once(move || {
                let Some(content) = sw.child().and_then(|v| v.first_child()) else {
                    return;
                };
                if let Some(pt) =
                    target.compute_point(&content, &gtk4::graphene::Point::new(0.0, 0.0))
                {
                    sw.vadjustment().set_value(f64::from(pt.y()) - above);
                }
            });
            return;
        }
        up = p.parent();
    }
}

/// The 6.h states: the Settings window over a default profile (nothing here reaches a network;
/// the Test buttons are insensitive). The window is told the size `main` is given: its default
/// is Python's 940x660, its minimum is its own.
fn prepare_settings(window: &MainWindow, state: &str) {
    use hatate_gtk::settings_values::Value;
    window.activate_action("preferences");
    let d = window
        .s
        .settings_dialog
        .borrow()
        .clone()
        .expect("the Settings window");
    d.window.set_default_size(
        hatate_gtk::settings_dialog::DEFAULT_WIDTH,
        hatate_gtk::settings_dialog::DEFAULT_HEIGHT,
    );
    let rest = state.strip_prefix("settings-").unwrap_or("");
    if let Ok(n) = rest.parse::<u32>() {
        d.tabs.set_current_page(Some(n));
    }
    match rest {
        "find" => d.find.set_text("delay"),
        "find-none" => d.find.set_text("zzzzzz"),
        "found" => {
            d.find.set_text("minimum similarity");
            d.jump_to_setting(0);
            // The state after Down from the Find box: the first result is the keyboard's row, so it
            // wears the ring (a shot has no key to give, so the window is told the last input was the
            // keyboard, as in `parser-health-kb`), next to the stamp on the setting it jumped to.
            d.window.set_focus_visible(true);
            let d2 = d.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
                if let Some(row) = d2.results.row_at_index(0) {
                    row.grab_focus();
                }
            });
        }
        // `lens-box` is the same state seen from the main window: the "Google Lens cannot run" box is
        // an alert dialog on the main window, not on Settings, so the Settings shot never holds it.
        "lens" | "lens-box" => {
            d.tabs.set_current_page(Some(1));
            d.lens_env
                .replace(hatate_core::lens_browser::deps::BrowserEnv {
                    home: Some(std::env::temp_dir().join("hatate-shot-nohome")),
                    path: Some(OsString::from("/nonexistent")),
                    ..Default::default()
                });
            d.set_value("enable_google_lens", &Value::Bool(true));
        }
        // The middle of the MCP page (the Dry run sentence to the destructive box), which neither the top
        // nor the audit shot holds: `tiers` has the page scrolled to the Dry run box, `tiers-dry` the same
        // with Dry run ticked (the sentence flips), `tiers-low` scrolled to the Hydrus box (6.i fixup).
        "mcp-tiers" | "mcp-tiers-dry" | "mcp-tiers-low" => {
            d.tabs.set_current_page(Some(8));
            if rest == "mcp-tiers-dry" {
                d.set_value("mcp_dry_run", &Value::Bool(true));
            }
            let id = if rest == "mcp-tiers-low" {
                "mcp_allow_hydrus_writes"
            } else {
                "mcp_dry_run"
            };
            if let Some(Control::Check(c)) = d.control(id) {
                scroll_to_top_of(
                    c.upcast_ref(),
                    if rest == "mcp-tiers-low" { 44.0 } else { 6.0 },
                );
            }
        }
        // 6.h-2: the Tests in each state. The words are the ones the Tests print (`settings_tests.rs`),
        // put on the labels the way a run leaves them; the Site Logins page holds the five, the Hydrus
        // page its Test and Fetch.
        r if r.starts_with("tests-") => {
            d.tabs.set_current_page(Some(6));
            let (busy, texts): (bool, [&str; 5]) = match r.trim_end_matches("-low") {
                "tests-running" => (
                    true,
                    [
                        "Testing with 2 cookie(s): auth, auth_secure\u{2026}",
                        "Testing with 1 cookie(s): anime_pictures_jwt\u{2026}",
                        "Testing\u{2026}",
                        "Testing as synthetic-user\u{2026}",
                        "Testing\u{2026}",
                    ],
                ),
                "tests-ok" => (
                    false,
                    [
                        "Working. Artist 'synthetic'. Picture available (unblurred).",
                        "Working. 14 tag(s), 1200\u{d7}1600, jpg.",
                        "The post loaded normally - it would be kept as a live match.",
                        "e621 accepted the credentials for synthetic-user. Posts under its global blacklist will be visible to searches.",
                        "Working. Extracted 23 tag(s), artist 'synthetic', full-resolution URL found, 1200x1600.",
                    ],
                ),
                _ => (
                    false,
                    [
                        "Read the deviation (artist 'synthetic'), but the picture is still the BLURRED logged-out copy. Re-copy the cookies from a logged-in browser - they may have expired or been cut off.",
                        "That post is gone (HTTP 404) - it was deleted or the ID does not exist. Try a different one; this says nothing about whether the cookies work.",
                        "Came back as the blank \"no such post\" page, so it would be dropped. If you can see this post in your browser while logged in, the cookies above aren't working - re-copy them.",
                        "e621 rejected the username or API key (401). Check both on e621 under Account > Manage API Access - the key is not your password, and it is case-sensitive.\n\nLeave them blank rather than wrong: a bad key makes e621 refuse every request, including ones that would have worked anonymously.",
                        "Page fetched, but no data could be read from it. The cookie may have expired (they do, periodically) - try copying a fresh PHPSESSID from your browser. See Help > View Logs for the specific reason.",
                    ],
                ),
            };
            for (test, text) in [
                Test::DeviantArt,
                Test::AnimePictures,
                Test::Sankaku,
                Test::E621,
                Test::Pixiv,
            ]
            .into_iter()
            .zip(texts)
            {
                d.preview_result(test.button_id(), test.result_id(), text, busy);
            }
            // `-low`: scrolled to the lower Tests (Sankaku to DeviantArt), which the top shot does not hold
            if rest.ends_with("-low") {
                if let Some(b) = d.button_widget(Test::AnimePictures.button_id()) {
                    scroll_to_top_of(b.upcast_ref(), 150.0);
                }
            }
        }
        "hydrus-running"
        | "hydrus-ok"
        | "hydrus-failed"
        | "hydrus-fetched"
        | "hydrus-fetched-open" => {
            d.tabs.set_current_page(Some(4));
            let (text, busy) = match rest {
                "hydrus-running" => ("", true),
                "hydrus-ok" => ("Connected \u{2713}", false),
                "hydrus-failed" => (
                    "Failed: Could not reach Hydrus at http://127.0.0.1:45869. Is the client running with its API switched on?",
                    false,
                ),
                _ => ("", false),
            };
            d.preview_result("button.test_connection", "hydrus_test_result", text, busy);
            if rest == "hydrus-running" {
                d.preview_result("button.fetch_from_hydrus", "hydrus_test_result", "", true);
            }
            if rest.starts_with("hydrus-fetched") {
                d.preview_services(vec![
                    ("aaaaaaaaaaaaaaaaaaaaaaaa".to_owned(), "my tags".to_owned()),
                    (
                        "bbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                        "downloader tags".to_owned(),
                    ),
                    (
                        "cccccccccccccccccccccccc".to_owned(),
                        "public tag repository".to_owned(),
                    ),
                ]);
            }
            if rest == "hydrus-fetched-open" {
                // after the window is up: a popover opened before it maps has nothing to hang from
                let d = d.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                    d.open_services()
                });
            }
        }
        // The MCP tab with the server started at launch (a free port, or a held one) and the
        // Shortcuts tab with a conflict or a recorder waiting for its key (6.i).
        "mcp-running" | "mcp-held" => d.tabs.set_current_page(Some(8)),
        "mcp-audit" => {
            d.tabs.set_current_page(Some(8));
            d.audit.all.set_active(true);
            d.audit.refresh();
            // The card is under the fold: the page scrolls to its end once it is laid out.
            scroll_to_end(d.audit.all.upcast_ref());
        }
        "shortcuts-conflict" => {
            d.tabs.set_current_page(Some(7));
            d.shortcuts.edit("prev_row").expect("prev_row").set("J");
            d.shortcuts.refresh_conflicts();
            // the conflict line sits under the list of actions
            scroll_to_end(d.shortcuts.conflict.upcast_ref());
        }
        "shortcuts-record" => {
            d.tabs.set_current_page(Some(7));
            d.shortcuts
                .edit("prev_row")
                .expect("prev_row")
                .start_recording();
        }
        // `tab-<n>`: Tab walked from the top of that tab of the window, one `TAB` line per stop
        // (the verdict's Tab order for the Shortcuts and MCP tabs, 6.i).
        f if f.starts_with("tab-") => {
            let n: u32 = f["tab-".len()..].parse().expect("tab-<n>");
            d.tabs.set_current_page(Some(n));
            d.window.set_focus_visible(true);
            let win = d.window.clone();
            glib::idle_add_local_once(move || {
                let mut seen: Vec<String> = Vec::new();
                for i in 0..60 {
                    win.child_focus(gtk4::DirectionType::TabForward);
                    let Some(f) = GtkWindowExt::focus(&win) else {
                        break;
                    };
                    let what = describe(&f);
                    if seen.first() == Some(&what) && i > 0 {
                        break;
                    }
                    println!("TAB {i} {what}");
                    seen.push(what);
                }
            });
        }
        f if f.starts_with("focus-") => {
            d.window.set_focus_visible(true);
            let target: Option<gtk4::Widget> = match &f["focus-".len()..] {
                "find" => Some(d.find.clone().upcast()),
                "ok" => Some(d.ok_button.clone().upcast()),
                "cancel" => Some(d.cancel_button.clone().upcast()),
                "tab" => Some(d.tabs.clone().upcast()),
                // 6.i: the MCP tab's server box and the Shortcuts tab's first key button.
                "mcp" => {
                    d.tabs.set_current_page(Some(8));
                    d.tabs
                        .nth_page(Some(8))
                        .and_then(|p| find_type(&p, "GtkCheckButton"))
                }
                // a tier box's own check box, inside its group box (the research one), scrolled in view
                "mcp-tier" => {
                    d.tabs.set_current_page(Some(8));
                    match d.control("mcp_allow_research") {
                        Some(Control::Check(c)) => {
                            scroll_to_top_of(c.upcast_ref(), 120.0);
                            Some(c.clone().upcast())
                        }
                        _ => None,
                    }
                }
                "test" => {
                    d.tabs.set_current_page(Some(6));
                    d.button_widget(Test::E621.button_id())
                        .map(|b| b.upcast::<gtk4::Widget>())
                }
                "fetch" => {
                    d.tabs.set_current_page(Some(4));
                    d.button_widget("button.fetch_from_hydrus")
                        .map(|b| b.upcast::<gtk4::Widget>())
                }
                "shortcut" => {
                    d.tabs.set_current_page(Some(7));
                    d.tabs
                        .nth_page(Some(7))
                        .and_then(|p| find_type(&p, "GtkButton"))
                }
                kind => {
                    let want = match kind {
                        "check" => "GtkCheckButton",
                        "spin" => "GtkSpinButton",
                        "combo" => "GtkDropDown",
                        _ => "GtkEntry",
                    };
                    // The first control of its kind that is on screen: the General page opens
                    // on spin boxes, the Hydrus page on a check box and its URL entry, the
                    // Import page on a drop-down.
                    let tab = match kind {
                        "check" | "entry" => 4,
                        "combo" => 2,
                        _ => 0,
                    };
                    d.tabs.set_current_page(Some(tab));
                    d.tabs.nth_page(Some(tab)).and_then(|p| find_type(&p, want))
                }
            };
            match target {
                Some(w) => {
                    w.grab_focus();
                }
                None => eprintln!("{state}: no control"),
            }
        }
        _ => {}
    }
}

/// The part of `b` that a scrolled window above `w` shows; `None` when it is cut vertically (a label
/// scrolled out of its page is mapped and has bounds, but its box would read other pixels, or a row of
/// the page without a glyph in it). A label cut at the side keeps its visible part.
fn clip_to_scrolled(
    w: &gtk4::Widget,
    root: &gtk4::Widget,
    mut b: gtk4::graphene::Rect,
) -> Option<gtk4::graphene::Rect> {
    let mut up = w.parent();
    while let Some(p) = up {
        if p.is::<gtk4::ScrolledWindow>() {
            let shown = b.intersection(&p.compute_bounds(root)?)?;
            if shown.height() < b.height() - 0.5 {
                return None;
            }
            b = shown;
        }
        up = p.parent();
    }
    Some(b)
}

/// Every mapped label under `w` with its box in `root`'s coordinates.
fn label_rects(w: &gtk4::Widget, root: &gtk4::Widget, out: &mut String) {
    // A popover is its own surface: its labels are measured against its own snapshot.
    if w.is::<gtk4::Popover>() && !std::ptr::eq(w.as_ptr(), root.as_ptr()) {
        return;
    }
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        let text = l.text();
        if l.is_mapped() && !text.is_empty() {
            if let Some(b) = w
                .compute_bounds(root)
                .and_then(|b| clip_to_scrolled(w, root, b))
            {
                // A selected label's box is cut to its text: past it is the page, and the farthest
                // pixel from the wash would be the page, not a glyph (two pixels in: the selection ends short of the
                // layout's rounded width).
                let selected = l.selection_bounds().is_some();
                let width = if selected {
                    (l.layout().pixel_extents().1.width() - 2) as f32
                } else {
                    b.width()
                };
                out.push_str(&format!(
                    "{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\n",
                    b.x(),
                    b.y(),
                    width,
                    b.height(),
                    if selected {
                        "selected"
                    } else if w.is_sensitive() {
                        "on"
                    } else {
                        "disabled"
                    },
                    text.replace(['\t', '\n'], " ")
                ));
            }
        }
    }
    // The log body is a text view, not a label: one box per line, `selected` where the selection
    // covers it, so the sweep reads the selected-text state too.
    if let Some(tv) = w.downcast_ref::<gtk4::TextView>() {
        if tv.is_mapped() {
            text_view_rects(tv, root, out);
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        label_rects(&child, root, out);
        c = child.next_sibling();
    }
}

/// One `<x y w h state text>` line per text line of `tv`: `selected` when the selection touches it.
fn text_view_rects(tv: &gtk4::TextView, root: &gtk4::Widget, out: &mut String) {
    let buffer = tv.buffer();
    let sel = buffer.selection_bounds();
    let origin = tv
        .compute_point(root, &gtk4::graphene::Point::new(0.0, 0.0))
        .expect("the text view's place");
    for line in 0..buffer.line_count() {
        let Some(start) = buffer.iter_at_line(line) else {
            break;
        };
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let text = buffer.text(&start, &end, false);
        if text.is_empty() {
            continue;
        }
        let (a, b) = (tv.iter_location(&start), tv.iter_location(&end));
        let (wx, wy) = tv.buffer_to_window_coords(gtk4::TextWindowType::Widget, a.x(), a.y());
        let selected = sel
            .is_some_and(|(s0, s1)| s0.offset() <= start.offset() && end.offset() <= s1.offset());
        let line_box = gtk4::graphene::Rect::new(
            wx as f32 + origin.x(),
            wy as f32 + origin.y(),
            (b.x() - a.x()).max(1) as f32,
            a.height() as f32,
        );
        let Some(shown) = clip_to_scrolled(tv.upcast_ref(), root, line_box) else {
            continue;
        };
        out.push_str(&format!(
            "{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\n",
            shown.x(),
            shown.y(),
            shown.width(),
            shown.height(),
            if selected { "selected" } else { "on" },
            text
        ));
    }
}

/// One line per widget, indented by depth.
fn tree_dump(w: &gtk4::Widget, root: &gtk4::Widget, depth: usize, out: &mut String) {
    let b = w.compute_bounds(root);
    let mut text = String::new();
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        text = format!(" label={:?}", l.label());
    }
    out.push_str(&format!(
        "{}{} [{}] {} mapped={} visible={}{}\n",
        "  ".repeat(depth),
        w.type_().name(),
        w.css_classes().join("."),
        b.map_or("-".to_owned(), |b| format!(
            "x={:.0} y={:.0} w={:.0} h={:.0}",
            b.x(),
            b.y(),
            b.width(),
            b.height()
        )),
        w.is_mapped(),
        w.is_visible(),
        text
    ));
    let mut c = w.first_child();
    while let Some(child) = c {
        tree_dump(&child, root, depth + 1, out);
        c = child.next_sibling();
    }
}

/// A focused widget in words: its type and the first text under it.
fn describe(w: &gtk4::Widget) -> String {
    let mut text = String::new();
    first_text(w, &mut text);
    let classes = w.css_classes().join(".");
    format!("{} [{}] {}", w.type_().name(), classes, text)
}

fn first_text(w: &gtk4::Widget, out: &mut String) {
    if !out.is_empty() {
        return;
    }
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        out.push_str(&l.text());
        return;
    }
    if let Some(t) = w.tooltip_text() {
        out.push_str(&t);
        return;
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        first_text(&child, out);
        c = child.next_sibling();
    }
}

/// The widget as the display draws it: a `WidgetPaintable` asked for the widget's allocated size
/// scales its intrinsic size (the window's own 5 px margin included) down to it, and that smears every
/// 1 px border over two pixels, so a hairline measures fainter than it is drawn. Snapshot at the
/// intrinsic size and cut the allocation out of it, 1:1.
fn texture_of(w: &gtk4::Widget, renderer: &gtk4::gsk::Renderer) -> Option<gdk::Texture> {
    use gtk4::gdk::prelude::PaintableExt;
    let paintable = gtk4::WidgetPaintable::new(Some(w));
    let (iw, ih) = (
        f64::from(paintable.intrinsic_width()),
        f64::from(paintable.intrinsic_height()),
    );
    let snap = gtk4::Snapshot::new();
    paintable.snapshot(&snap, iw, ih);
    let node = snap.to_node()?;
    let (aw, ah) = (f64::from(w.width()), f64::from(w.height()));
    let view = gtk4::graphene::Rect::new(
        ((iw - aw) / 2.0).floor() as f32,
        ((ih - ah) / 2.0).floor() as f32,
        aw as f32,
        ah as f32,
    );
    Some(renderer.render_texture(&node, Some(&view)))
}

/// A popover menu that is open now (the menubar's closed ones are in the tree too).
fn visible_popover_menu(w: &gtk4::Widget) -> Option<gtk4::Popover> {
    if let Some(p) = w.downcast_ref::<gtk4::Popover>().filter(|p| p.is_visible()) {
        return Some(p.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(p) = visible_popover_menu(&child) {
            return Some(p);
        }
        c = child.next_sibling();
    }
    None
}

/// The Review states' rows: real pictures (a gradient and a nearby one, so the pane has something
/// to decode and draw), a Good row with two matches and tags, a Not found row, a Good row with one.
fn review_rows(pics: &std::path::Path, state: &str) -> Vec<Entry> {
    let full_match = is_compare_state(state);
    use hatate_core::model::{Tag, TagSource};
    let tag = |ns: Option<&str>, name: &str, source| Tag {
        name: name.to_owned(),
        namespace: ns.map(str::to_owned),
        source,
    };
    let mut rows = Vec::new();
    for (i, (status, tags, cands)) in [
        (MatchStatus::Good, 3, 2),
        (MatchStatus::NotFound, 0, 0),
        (MatchStatus::Good, 0, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let path = pics.join(format!("review-{i}.png"));
        review_picture(640, 480, i as u8)
            .save(&path)
            .expect("picture");
        let mut e = Entry::new(path.to_string_lossy().into_owned());
        // What the search worker records of the local file; without it the readout has no size chip.
        e.local_width = Some(640);
        e.local_height = Some(480);
        e.status = status;
        e.hydrus_hash = Some(format!("{i:064x}"));
        for (n, (url, sim, site)) in [
            ("https://danbooru.donmai.us/posts/1", 96.0, "Danbooru"),
            ("https://gelbooru.com/index.php?id=2", 80.0, "Gelbooru"),
        ]
        .into_iter()
        .take(cands)
        .enumerate()
        {
            let mut c = Candidate::new(url);
            c.similarity = sim;
            c.similarity_measured = true;
            c.engine = "IQDB".to_owned();
            c.source_name = Some(site.to_owned());
            c.width = Some(1280 + 40 * n as i64);
            c.height = Some(960 + 30 * n as i64);
            if full_match {
                // The full file as a `data:` URI, so the comparison needs no network at all.
                let mut png = std::io::Cursor::new(Vec::new());
                match_picture(800, 600, i as u8)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .expect("png");
                c.direct_file_url = Some(data_uri(png.get_ref()));
            }
            e.candidates.push(c);
        }
        if tags > 0 {
            e.tags = vec![
                tag(None, "solo", TagSource::Booru),
                tag(Some("character"), "reimu", TagSource::User),
                tag(None, "sketch", TagSource::Hydrus),
            ];
        }
        rows.push(e);
    }
    if state == "review-error" {
        // The local file of the selected row is gone: the error state (design gate 2.8).
        std::fs::remove_file(&rows[0].path).expect("remove the synthetic picture");
    }
    rows
}

/// The match's file as a `data:` URI.
fn data_uri(bytes: &[u8]) -> String {
    let mut uri = String::from("data:image/png,");
    for b in bytes {
        uri.push_str(&format!("%{b:02X}"));
    }
    uri
}

/// The local picture at another size with a block painted over a corner, so Differences has a
/// region to mark.
fn match_picture(w: u32, h: u32, seed: u8) -> image::RgbaImage {
    let mut img = review_picture(w, h, seed);
    for y in h * 3 / 4..h * 7 / 8 {
        for x in w / 8..w * 3 / 8 {
            img.put_pixel(x, y, image::Rgba([250, 250, 250, 255]));
        }
    }
    img
}

/// Whether the state's page has settled (the comparison loaded), polled by the tick callback.
static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// A synthetic picture: a colour field with a diagonal bar, `seed` shifting the hue.
fn review_picture(w: u32, h: u32, seed: u8) -> image::RgbaImage {
    image::RgbaImage::from_fn(w, h, |x, y| {
        let bar = (x as i32 - y as i32).abs() < 24;
        let hue = seed.wrapping_mul(50);
        if bar {
            image::Rgba([240, 220, 90, 255])
        } else {
            image::Rgba([
                hue.wrapping_add((x * 255 / w) as u8),
                70 + (y * 120 / h) as u8,
                200u8.wrapping_sub(hue),
                255,
            ])
        }
    })
}

/// What a Review state does once the window is up.
fn prepare_review(window: &MainWindow, state: &str) {
    // A session keeps no picture bytes and no "fetched" flags; these rows are as a search left them.
    {
        let mut q = window.s.queue.borrow_mut();
        let ids: Vec<_> = (0..q.len() as i64).filter_map(|i| q.id_at(i)).collect();
        for id in ids {
            for (n, c) in q.entry_mut(id).unwrap().candidates.iter_mut().enumerate() {
                let mut png = std::io::Cursor::new(Vec::new());
                review_picture(640, 480, 1 + n as u8)
                    .write_to(&mut png, image::ImageFormat::Png)
                    .expect("png");
                c.thumb_bytes = Some(png.into_inner());
                c.booru_tags_fetched = true;
                c.remote_info_fetched = true;
            }
        }
    }
    let r = window.s.review.clone();
    window.set_mode("review");
    let row = if matches!(
        state,
        "review-nomatch" | "review-wipe-nomatch" | "review-compare-nomatch"
    ) {
        1
    } else {
        0
    };
    if !matches!(state, "review-empty" | "review-bare") {
        window.s.queue_view.select_row(row);
    }
    let win = window.win.clone();
    if is_compare_state(state) {
        prepare_compare(window, state);
    }
    match state {
        "review-picker" => {
            // After the pictures have loaded, so the popup opens over the settled page.
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                r.candidate_combo.activate();
            });
        }
        // The popup after Down from the first row: the second row is the keyboard's, the current
        // candidate keeps its mark (DAN-2472 item 2).
        "review-picker-kb" => {
            win.set_focus_visible(true);
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                r.candidate_combo.activate();
                // Polled: the popup is mapped a few frames after it is asked for.
                let mut tries = 0;
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    tries += 1;
                    let list = visible_popover_menu(win.upcast_ref())
                        .and_then(|p| find_type(p.upcast_ref(), "GtkListView"));
                    let Some(list) = list else {
                        return if tries < 30 {
                            glib::ControlFlow::Continue
                        } else {
                            glib::ControlFlow::Break
                        };
                    };
                    // What Down does in a list view: the keyboard focus moves to the next item and the
                    // selection (the current candidate) stays where it is.
                    let second = list.first_child().and_then(|first| first.next_sibling());
                    let moved = second.as_ref().is_some_and(|row| row.grab_focus());
                    println!(
                        "PICKER-KB focus on the second row: {moved} after {tries} polls, focus {:?}",
                        list.root()
                            .and_then(|r| gtk4::prelude::RootExt::focus(&r))
                            .map(|f| describe(&f))
                    );
                    glib::ControlFlow::Break
                });
            });
        }
        "review-addtags" => window.add_tags_to_current(),
        "review-conditions" => window.action_edit_conditions(),
        st if st.starts_with("review-focus-") && is_compare_state(st) => {
            // Focus is taken in `prepare_compare`, once the comparison has loaded.
            win.set_focus_visible(true);
        }
        st if st.starts_with("review-focus-") => {
            win.set_focus_visible(true);
            let target: gtk4::Widget = match &st["review-focus-".len()..] {
                "combo" => r.candidate_combo.clone().upcast(),
                "mark" => r.mark_btn.button.clone().upcast(),
                "next" => r.next_btn.button.clone().upcast(),
                "addtags" => r.add_tags_btn.clone().upcast(),
                "taglist" => r.tag_list.clone().upcast(),
                "view" => r.view_buttons.values().next().unwrap().clone().upcast(),
                other => panic!("no focus target {other}"),
            };
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                target.grab_focus();
                let b = target.compute_bounds(&win);
                println!(
                    "FOCUS {} focus_visible={} bounds={:?}",
                    describe(&target),
                    target.has_visible_focus(),
                    b.map(|b| (b.x(), b.y(), b.width(), b.height()))
                );
            });
        }
        _ => {}
    }
}

/// Whether `state` is one of the 6.f-2 states, which need the match's full file.
fn is_compare_state(state: &str) -> bool {
    state.contains("wipe")
        || state.contains("diff")
        || state.contains("compare")
        || state == "review-error"
        || matches!(
            state,
            "review-focus-wslider"
                | "review-focus-wview"
                | "review-focus-zoom"
                | "review-focus-popout"
        )
}

/// The Wipe, Differences, error and compare-window states (6.f-2).
fn prepare_compare(window: &MainWindow, state: &str) {
    use std::sync::atomic::Ordering::SeqCst;
    let r = window.s.review.clone();
    let mode = if state.contains("diff") && !state.contains("compare") && !state.contains("focus") {
        "diff"
    } else {
        "wipe"
    };
    if state.contains("compare") {
        // The compare window, over the Review page on Side by side.
        let id = window.current_id().expect("a selected row");
        window.open_compare_dialog(id);
        if state == "review-compare-nomatch" {
            return;
        }
        let dialog = window
            .s
            .compare_open
            .borrow()
            .clone()
            .expect("the dialog is open");
        let want_diff = state.contains("differences") || state == "review-focus-compare-diff";
        READY.store(false, SeqCst);
        let d = dialog.clone();
        let focus: Option<gtk4::Widget> = state.strip_prefix("review-focus-compare-").map(|w| {
            if w == "slider" {
                dialog.slider.clone().upcast()
            } else {
                dialog.diff_radio.clone().upcast()
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if d.view.has_pictures() == (true, true) {
                if want_diff && !d.diff_radio.is_active() {
                    d.diff_radio.set_active(true);
                }
                if !want_diff || d.view.showing_differences() {
                    if let Some(t) = &focus {
                        d.window.set_focus_visible(true);
                        t.grab_focus();
                        println!(
                            "FOCUS {} focus_visible={}",
                            describe(t),
                            t.has_visible_focus()
                        );
                    }
                    glib::timeout_add_local_once(std::time::Duration::from_millis(250), || {
                        READY.store(true, SeqCst)
                    });
                    return glib::ControlFlow::Break;
                }
            }
            glib::ControlFlow::Continue
        });
        return;
    }
    r.view_buttons[if mode == "diff" { "diff" } else { "wipe" }].set_active(true);
    READY.store(false, SeqCst);
    let state = state.to_owned();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let done = match state.as_str() {
            "review-error" => r.error_detail.is_visible(),
            "review-wipe-nomatch" => r.wipe.has_pictures() == (true, false),
            _ if mode == "diff" => r.wipe.showing_differences(),
            _ => r.wipe.has_pictures() == (true, true) && !r.zoom_label.text().is_empty(),
        };
        if done {
            if let Some(w) = state.strip_prefix("review-focus-") {
                let t: gtk4::Widget = match w {
                    "wslider" => r.wipe_slider.clone().upcast(),
                    "wview" => r.wipe.area.clone().upcast(),
                    "zoom" => r.zoom_tools.first_child().expect("zoom button"),
                    _ => r.pop_out_btn.clone().upcast(),
                };
                t.grab_focus();
                println!(
                    "FOCUS {} focus_visible={}",
                    describe(&t),
                    t.has_visible_focus()
                );
            }
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), || {
                READY.store(true, SeqCst)
            });
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

/// What a Queue state does once the window is up.
fn prepare_queue(window: &MainWindow, state: &str) {
    use hatate_core::table_cells::Column;
    let view = window.s.queue_view.clone();
    let ids = view.visible_ids();
    match state {
        "queue-selected" => view.select_ids(&ids[1..3]),
        "queue-sorted" => view.on_header_clicked(Column::Status),
        "queue-filter" => window.s.filter_bar.text.set_text("synthetic-001"),
        "queue-filter-empty" => window.s.filter_bar.text.set_text("no such file"),
        "queue-menu" => {
            view.select_ids(&ids[1..3]);
            window.show_table_context_menu(80.0, 120.0);
        }
        "queue-remove" => {
            view.select_ids(&ids[1..3]);
            window.remove_rows(&ids[1..3]);
        }
        "queue-focus" | "queue-focus-selected" => {
            window.win.set_focus_visible(true);
            if state == "queue-focus-selected" {
                view.select_row(0);
            }
            view.view.grab_focus();
            // What holds focus, for the verdict (and for a ring that does not show).
            let win = window.win.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                if let Some(f) = GtkWindowExt::focus(&win) {
                    let b = f.compute_bounds(&win);
                    println!(
                        "FOCUS {} classes={:?} focus_visible={} bounds={:?}",
                        describe(&f),
                        f.css_classes(),
                        f.has_visible_focus(),
                        b.map(|b| (b.x(), b.y(), b.width(), b.height()))
                    );
                }
            });
        }
        "queue-5000" => {
            let adj = view.scroller.vadjustment();
            glib::idle_add_local_once(move || adj.set_value(adj.upper() / 2.0));
        }
        "queue-5000-scroll" => {
            // Scroll frame timing, as the spike measured it: 240 frames of a steady scroll
            // down the list; report the frames over 16.7 ms and the 95th percentile.
            let adj = view.scroller.vadjustment();
            let last = Cell::new(0i64);
            let times = std::cell::RefCell::new(Vec::<f64>::new());
            view.view.add_tick_callback(move |w, clock| {
                let now = clock.frame_time();
                if last.get() != 0 {
                    times.borrow_mut().push((now - last.get()) as f64 / 1000.0);
                }
                last.set(now);
                let n = times.borrow().len();
                adj.set_value(adj.value() + 40.0);
                if n >= 240 {
                    let mut t = times.borrow().clone();
                    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let over = t.iter().filter(|ms| **ms > 16.7).count();
                    println!(
                        "SCROLL frames={} over_16.7ms={} p95={:.2}ms max={:.2}ms rows={} scrolled_to={:.0}/{:.0}",
                        t.len(),
                        over,
                        t[(t.len() * 95 / 100).min(t.len() - 1)],
                        t[t.len() - 1],
                        w.height(),
                        adj.value(),
                        adj.upper()
                    );
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }
        _ => {}
    }
}

fn first_popover_menu(w: &gtk4::Widget) -> Option<gtk4::PopoverMenu> {
    if let Some(p) = w.downcast_ref::<gtk4::PopoverMenu>() {
        return Some(p.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(p) = first_popover_menu(&child) {
            return Some(p);
        }
        c = child.next_sibling();
    }
    None
}

/// Hides every label whose text starts with `prefix`.
fn hide_label_starting(w: &gtk4::Widget, prefix: &str) {
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        if l.text().starts_with(prefix) {
            l.set_visible(false);
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        hide_label_starting(&child, prefix);
        c = child.next_sibling();
    }
}

/// The `modelbutton` row that holds a label reading `text`, anywhere under `w` (popovers
/// included).
fn find_row(w: &gtk4::Widget, text: &str) -> Option<gtk4::Widget> {
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        if l.text() == text {
            let mut up = Some(w.clone());
            while let Some(a) = up {
                if a.css_name() == "modelbutton" {
                    return Some(a);
                }
                up = a.parent();
            }
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(r) = find_row(&child, text) {
            return Some(r);
        }
        c = child.next_sibling();
    }
    None
}

/// The focusable button whose first label reads `text`.
fn find_button_labelled(w: &gtk4::Widget, text: &str) -> Option<gtk4::Button> {
    if let Some(b) = w.downcast_ref::<gtk4::Button>() {
        let mut t = String::new();
        first_text(w, &mut t);
        if b.is_focusable() && t == text {
            return Some(b.clone());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(b) = find_button_labelled(&child, text) {
            return Some(b);
        }
        c = child.next_sibling();
    }
    None
}

fn first_button(w: &gtk4::Widget) -> Option<gtk4::Button> {
    if let Some(b) = w.downcast_ref::<gtk4::Button>() {
        if b.is_focusable() && b.is_visible() {
            return Some(b.clone());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(b) = first_button(&child) {
            return Some(b);
        }
        c = child.next_sibling();
    }
    None
}

/// The first widget under `w` whose type is named `name`.
/// The chain of widgets that sets a height at `width`: each step is the child asking the most.
fn tallest_chain(root: &gtk4::Widget, width: i32) -> String {
    let mut out = Vec::new();
    let mut w = root.clone();
    for _ in 0..40 {
        let min = w.measure(gtk4::Orientation::Vertical, width).0;
        let mut kids = Vec::new();
        let mut best: Option<(i32, gtk4::Widget)> = None;
        let mut c = w.first_child();
        while let Some(child) = c {
            if child.is_visible() {
                let m = child.measure(gtk4::Orientation::Vertical, width).0;
                kids.push(m.to_string());
                if best.as_ref().is_none_or(|(b, _)| m > *b) {
                    best = Some((m, child.clone()));
                }
            }
            c = child.next_sibling();
        }
        out.push(format!(
            "{}[{}]={min} ({})",
            w.type_().name(),
            w.css_name(),
            kids.join("+")
        ));
        match best {
            Some((_, b)) => w = b,
            None => break,
        }
    }
    out.join(" > ")
}

/// The `other` lookup key for the alert dialog's own toplevel (`settings-lens-box`).
const ALERT_TOPLEVEL: &str = "\u{0}alert";

fn find_type(w: &gtk4::Widget, name: &str) -> Option<gtk4::Widget> {
    if w.type_().name() == name {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(f) = find_type(&child, name) {
            return Some(f);
        }
        c = child.next_sibling();
    }
    None
}
