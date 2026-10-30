//! Renders the real window to a PNG, headless (`xvfb-run`), with a temp profile and the bundled
//! fonts: `cargo run --example shot -- OUT.png [dark|light] [WxH|min] [STATE]`. A test-only tool:
//! the shipped binary has no way to do this. Shows only the synthetic profile it creates.
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
//! `review-focus-compare-<slider|diff>`. The Hydrus and Pawchive states (6.g) are `hydrus-delete` (the Delete from Hydrus box, Cancel default), `hydrus-query`, `hydrus-query-results` and `hydrus-query-focus` (the Query Hydrus window, empty with its search button focused, so the ring is the diff against `hydrus-query`; with synthetic results as `hydrus-query-results`), `hydrus-empty` (the empty page with Query Hydrus enabled) and `pawchive` (the Pawchive Index window over an empty index). The Settings states (6.h) are `settings-<0..8>` (that tab of the Settings window, in Python's order: General to MCP), `settings-find` (a query with results), `settings-find-none` ("Nothing matches that."), `settings-lens` (the Engine page with Google Lens ticked and its dependencies missing; libadwaita presents the warning box as a window of its own, which this snapshot does not hold), `settings-found` (a jumped-to setting wearing the stamp) and `settings-focus-<find|tab|ok|cancel|check|spin|combo|entry>` (that control focused with the ring showing; `check` and `entry` are on the Hydrus page, `combo` on the Import page, the rest on General). The size `min` is the window's stated minimum.

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::config::Settings;
use hatate_core::model::{Candidate, Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::SessionStore;
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

fn main() -> glib::ExitCode {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("OUT.png"));
    let scheme = args.next().unwrap_or_else(|| "dark".into());
    let size = args.next();
    let state = args.next().unwrap_or_else(|| "empty".into());
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
        | "tab-failed" | "tab-activity" => save(&rows(12)),
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
                st if st.starts_with("review") => 60,
                _ => 14,
            };
            if frames.get() < settle || !READY.load(std::sync::atomic::Ordering::SeqCst) {
                return glib::ControlFlow::Continue;
            }
            // The log viewer is its own window; its shot is that window's, not the main one's.
            let other = if state.starts_with("logs") {
                Some("Logs")
            } else if state == "parser-health" {
                Some("Parser Health")
            } else if state.starts_with("hydrus-query") {
                Some("Query Hydrus")
            } else if state.starts_with("pawchive") {
                Some("Pawchive Index")
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
                        w.is_mapped()
                            && w.title().is_some_and(|t| {
                                t == title || (title == "Pawchive Index" && t.starts_with(title))
                            })
                    })
            });
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
        }
        "lens" => {
            d.tabs.set_current_page(Some(1));
            d.lens_env
                .replace(hatate_core::lens_browser::deps::BrowserEnv {
                    home: Some(std::env::temp_dir().join("hatate-shot-nohome")),
                    path: Some(OsString::from("/nonexistent")),
                    ..Default::default()
                });
            d.set_value("enable_google_lens", &Value::Bool(true));
        }
        f if f.starts_with("focus-") => {
            d.window.set_focus_visible(true);
            let target: Option<gtk4::Widget> = match &f["focus-".len()..] {
                "find" => Some(d.find.clone().upcast()),
                "ok" => Some(d.ok_button.clone().upcast()),
                "cancel" => Some(d.cancel_button.clone().upcast()),
                "tab" => Some(d.tabs.clone().upcast()),
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
